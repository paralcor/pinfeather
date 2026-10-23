import { createStore } from "zustand/vanilla";
import type { ClipSummary, ListCursor, StatusResult } from "./ipc";
import { readableError, type DesktopBridge } from "./desktop";
export interface HistoryState { status: StatusResult | null; summaries: ClipSummary[]; cursor: ListCursor | null; loading: boolean; changing: boolean; error: string | null; hostMissing: boolean; }
export function createHistoryModel(bridge: DesktopBridge) {
  const store = createStore<HistoryState>(() => ({ status: null, summaries: [], cursor: null, loading: false, changing: false, error: null, hostMissing: !bridge.available }));
  let active = false, lifetime = 0, busy = false, again = false, needsHistory = false, request = 0;
  function acceptStatus(status: StatusResult) {
    const current = store.getState().status;
    if (!current || BigInt(status.revision) >= BigInt(current.revision)) store.setState({ status });
  }
  async function refresh(history = true): Promise<void> {
    if (!active || !bridge.available) return;
    needsHistory ||= history;
    if (busy) { again = true; return; }
    const token = lifetime;
    busy = true;
    try {
      do {
        again = false;
        const list = needsHistory;
        needsHistory = false;
        if (list) store.setState({ loading: true, error: null });
        const id = `recent-${++request}`;
        const [status, page] = await Promise.all([bridge.status(), list ? bridge.list(id, null) : Promise.resolve(null)]);
        if (!active || token !== lifetime) return;
        const previous = store.getState().status;
        const missedHistory = !page && previous && BigInt(status.revision) >= BigInt(previous.revision) && (status.history.items !== previous.history.items || status.history.payload_bytes !== previous.history.payload_bytes);
        acceptStatus(status);
        if (missedHistory) { needsHistory = true; again = true; }
        if (page) {
          if (page.request_id !== id || page.normalized_query !== "") throw new Error("invalid list correlation");
          store.setState({ summaries: page.summaries, cursor: page.next_cursor });
        }
      } while (again && active && token === lifetime);
    } catch (error) {
      if (active && token === lifetime) store.setState({ error: readableError(error) });
    } finally {
      busy = false;
      if (active && token === lifetime) store.setState({ loading: false });
      if (active && again) { again = false; void refresh(needsHistory); }
    }
  }
  function start() {
    if (!bridge.available) return () => {};
    active = true;
    const token = ++lifetime;
    const disposers: Array<() => void> = [];
    store.setState({ loading: true });
    void (async () => {
      try {
        for (const name of ["history_changed", "status_changed"] as const) {
          const off = await bridge.subscribe(name, () => { if (active && token === lifetime) void refresh(name === "history_changed"); });
          if (!active || token !== lifetime) { off(); return; }
          disposers.push(off);
        }
        await refresh();
      } catch (error) {
        disposers.splice(0).forEach(off => off());
        if (active && token === lifetime) store.setState({ error: readableError(error), loading: false });
      }
    })();
    return () => { if (token === lifetime) { active = false; lifetime++; } disposers.splice(0).forEach(off => off()); };
  }
  async function more() {
    const cursor = store.getState().cursor;
    if (!active || busy || !cursor) return;
    const token = lifetime;
    busy = true;
    store.setState({ loading: true, error: null });
    try {
      const id = `recent-${++request}`;
      const page = await bridge.list(id, cursor);
      if (!active || token !== lifetime) return;
      if (page.request_id !== id || page.normalized_query !== "") throw new Error("invalid list correlation");
      const seen = new Set(store.getState().summaries.map(item => item.id));
      store.setState({ summaries: [...store.getState().summaries, ...page.summaries.filter(item => !seen.has(item.id))], cursor: page.next_cursor });
    } catch (error) {
      if (active && token === lifetime) store.setState({ error: readableError(error) });
    } finally {
      busy = false;
      if (active && token === lifetime) store.setState({ loading: false });
      if (active && again) { again = false; void refresh(needsHistory); }
    }
  }
  async function toggle() {
    const current = store.getState();
    if (!active || current.changing || !current.status) return;
    const token = lifetime;
    const retry = current.status.services.storage === "failed" && current.status.queue.items > 0;
    const enabled = retry || current.status.state !== "listening";
    store.setState({ changing: true, error: null });
    try {
      const status = await bridge.setEnabled(enabled);
      if (active && token === lifetime) { acceptStatus(status); void refresh(false); }
    } catch (error) {
      if (active && token === lifetime) store.setState({ error: readableError(error) });
    } finally {
      if (active && token === lifetime) store.setState({ changing: false });
    }
  }
  return { store, start, refresh, more, toggle };
}
