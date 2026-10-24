import { createStore } from "zustand/vanilla";
import type { ClipSummary, ListCursor, StatusResult } from "./ipc";
import { normalizeQuery, parseGetClipTextRequest, parseGetClipTextResult } from "./ipc";
import { readableError, type DesktopBridge } from "./desktop";
export interface HistoryState { status: StatusResult | null; summaries: ClipSummary[]; cursor: ListCursor | null; loading: boolean; changing: boolean; error: string | null; hostMissing: boolean; query: string; selectedId: string | null; previewText: string; previewLoading: boolean; previewError: string | null; previewNextOffset: number | null; }
const QUERY_DEBOUNCE_MS = 50;
const MAX_LIST_CALLS = 2;
const MAX_PREVIEW_CALLS = 2;
export function createHistoryModel(bridge: DesktopBridge) {
  const store = createStore<HistoryState>(() => ({ status: null, summaries: [], cursor: null, loading: false, changing: false, error: null, hostMissing: !bridge.available, query: "", selectedId: null, previewText: "", previewLoading: false, previewError: null, previewNextOffset: null }));
  let active = false, ready = false, lifetime = 0, generation = 0, request = 0;
  let timer: ReturnType<typeof setTimeout> | null = null, wantPage1 = false, statusBusy = false, statusAgain = false;
  let selection = 0, previewRequest = 0;
  // Latest single pending preview need; a selection never owns a queue.
  type PreviewNeed = { clipId: string; offset: number; append: boolean };
  let pendingPreview: PreviewNeed | null = null;
  // Physical unsettled calls; a generation may own at most one of each kind at a time.
  const inflight: Array<{ token: number; gen: number }> = [];
  const previewInflight: Array<{ token: number; sel: number; gen: number }> = [];
  function acceptStatus(status: StatusResult) {
    const current = store.getState().status;
    if (!current || BigInt(status.revision) >= BigInt(current.revision)) store.setState({ status });
  }
  function owns(token: number, gen: number) { return active && token === lifetime && gen === generation; }
  function ownsPreview(token: number, sel: number, gen: number) { return active && token === lifetime && sel === selection && gen === generation; }
  function canDispatch(token: number, gen: number) {
    return inflight.length < MAX_LIST_CALLS && !inflight.some(entry => entry.token === token && entry.gen === gen);
  }
  function releaseSlot(token: number, gen: number) {
    const index = inflight.findIndex(entry => entry.token === token && entry.gen === gen);
    if (index >= 0) inflight.splice(index, 1);
  }
  function releasePreviewSlot(token: number, sel: number, gen: number) {
    const index = previewInflight.findIndex(entry => entry.token === token && entry.sel === sel && entry.gen === gen);
    if (index >= 0) previewInflight.splice(index, 1);
  }
  /** Normalized current query, or null when the raw input must not reach the host. */
  function currentQuery(): string | null {
    const parsed = normalizeQuery(store.getState().query);
    return parsed.ok ? parsed.value : null;
  }
  function clearTimer() { if (timer !== null) { clearTimeout(timer); timer = null; } }
  /** Drops the selection and any pending preview request, and ages out in-flight frames. */
  function clearSelection() {
    selection += 1;
    pendingPreview = null;
    store.setState({ selectedId: null, previewText: "", previewLoading: false, previewError: null, previewNextOffset: null });
  }
  /** A list write is authoritative for the rows: a selection that left them is dropped. */
  function syncSelectionWithRows() {
    const selected = store.getState().selectedId;
    if (selected !== null && !store.getState().summaries.some(item => item.id === selected)) clearSelection();
  }
  function pump() {
    if (!active || !ready || !bridge.available || !wantPage1) return;
    // Single dispatch gate: every path (timer, old-call settle, refresh, status
    // compensation) must honour the pending debounce instead of jumping ahead.
    if (timer !== null) return;
    const normalized = currentQuery();
    if (normalized === null) {
      // An unsendable retained query never loads: end the round instead of
      // leaving `start`'s loading up forever with no future to clear it.
      wantPage1 = false;
      store.setState({ loading: false });
      return;
    }
    const token = lifetime, gen = generation;
    if (!canDispatch(token, gen)) return;
    wantPage1 = false;
    void runList(token, gen, normalized, null);
  }
  function pumpPreview() {
    if (!active || !ready || !bridge.available || pendingPreview === null) return;
    const need = pendingPreview;
    const state = store.getState();
    if (state.selectedId !== need.clipId || !state.summaries.some(item => item.id === need.clipId)) {
      pendingPreview = null;
      return;
    }
    const token = lifetime, sel = selection, gen = generation;
    if (previewInflight.some(entry => entry.token === token && entry.sel === sel && entry.gen === gen)) return;
    if (previewInflight.length >= MAX_PREVIEW_CALLS) return;
    pendingPreview = null;
    void runPreview(token, sel, gen, need.clipId, need.offset, need.append);
  }
  async function runList(token: number, gen: number, normalized: string, cursor: ListCursor | null) {
    const id = `${cursor ? "more" : "page1"}-${++request}`;
    inflight.push({ token, gen });
    store.setState({ loading: true, error: null });
    try {
      const page = await bridge.list(id, cursor, normalized);
      if (!owns(token, gen)) return;
      if (page.request_id !== id || page.normalized_query !== normalized) throw new Error("invalid list correlation");
      if (cursor) {
        const seen = new Set(store.getState().summaries.map(item => item.id));
        store.setState({ summaries: [...store.getState().summaries, ...page.summaries.filter(item => !seen.has(item.id))], cursor: page.next_cursor });
      } else {
        store.setState({ summaries: page.summaries, cursor: page.next_cursor });
      }
      syncSelectionWithRows();
    } catch (error) {
      if (owns(token, gen)) store.setState({ error: readableError(error) });
    } finally {
      releaseSlot(token, gen);
      if (active) pump();
      if (owns(token, gen) && !inflight.some(entry => entry.token === lifetime && entry.gen === generation)) store.setState({ loading: false });
    }
  }
  async function runPreview(token: number, sel: number, gen: number, clipId: string, offset: number, append: boolean) {
    const requestId = `preview-${++previewRequest}`;
    const request = parseGetClipTextRequest({ request_id: requestId, clip_id: clipId, offset });
    if (!request.ok) {
      if (ownsPreview(token, sel, gen)) store.setState({ previewError: readableError(request.error), previewLoading: false });
      return;
    }
    previewInflight.push({ token, sel, gen });
    try {
      const raw = await bridge.getText(requestId, clipId, offset);
      const parsed = parseGetClipTextResult(raw, request.value);
      if (!parsed.ok) throw new Error("invalid preview frame");
      if (!ownsPreview(token, sel, gen)) return;
      // A continuation only lands while the frontier still points at its offset.
      if (append && store.getState().previewNextOffset !== offset) return;
      const prior = store.getState().previewText;
      store.setState({
        previewText: append ? prior + parsed.value.text : parsed.value.text,
        previewNextOffset: parsed.value.next_offset,
        previewLoading: false,
        previewError: null,
      });
    } catch (error) {
      if (ownsPreview(token, sel, gen)) store.setState({ previewError: readableError(error), previewLoading: false });
    } finally {
      releasePreviewSlot(token, sel, gen);
      if (active) pumpPreview();
      if (ownsPreview(token, sel, gen) && !previewInflight.some(entry => entry.token === lifetime && entry.sel === selection && entry.gen === generation)) {
        store.setState({ previewLoading: false });
      }
    }
  }
  async function refreshStatus() {
    if (!active || !bridge.available) return;
    if (statusBusy) { statusAgain = true; return; }
    statusBusy = true;
    const token = lifetime;
    try {
      do {
        statusAgain = false;
        const status = await bridge.status();
        if (!active || token !== lifetime) return;
        const previous = store.getState().status;
        const missedHistory = previous !== null && BigInt(status.revision) >= BigInt(previous.revision) && (status.history.items !== previous.history.items || status.history.payload_bytes !== previous.history.payload_bytes);
        acceptStatus(status);
        if (missedHistory) {
          clearSelection();
          store.setState({ cursor: null });
          wantPage1 = true;
          void pump();
        }
      } while (statusAgain && active && token === lifetime);
    } catch (error) {
      if (active && token === lifetime) store.setState({ error: readableError(error) });
    } finally {
      statusBusy = false;
      if (active && statusAgain) { statusAgain = false; void refreshStatus(); }
    }
  }
  function setQuery(value: string) {
    if (value === store.getState().query) return;
    clearSelection();
    if (!bridge.available) { store.setState({ query: value }); return; }
    generation += 1;
    clearTimer();
    wantPage1 = false;
    store.setState({ query: value, summaries: [], cursor: null });
    const parsed = normalizeQuery(value);
    if (!parsed.ok) {
      store.setState({ loading: false, error: readableError(parsed.error) });
      return;
    }
    store.setState({ loading: true, error: null });
    timer = setTimeout(() => { timer = null; wantPage1 = true; void pump(); }, QUERY_DEBOUNCE_MS);
  }
  async function refresh(history = true): Promise<void> {
    if (!active || !bridge.available) return;
    void refreshStatus();
    if (history) {
      clearSelection();
      store.setState({ cursor: null });
      wantPage1 = true;
      void pump();
    }
  }
  function selectClip(id: string | null) {
    if (!active || !bridge.available) return;
    if (id === null) { clearSelection(); return; }
    const state = store.getState();
    // While a list is loading the rendered rows are not the current authority.
    if (state.loading) return;
    if (state.selectedId === id) return;
    if (!state.summaries.some(item => item.id === id)) return;
    selection += 1;
    store.setState({ selectedId: id, previewText: "", previewLoading: true, previewError: null, previewNextOffset: null });
    pendingPreview = { clipId: id, offset: 0, append: false };
    void pumpPreview();
  }
  function retryPreview() {
    if (!active || !bridge.available) return;
    const state = store.getState();
    if (state.previewLoading || state.previewError === null || state.selectedId === null) return;
    const id = state.selectedId;
    if (!state.summaries.some(item => item.id === id)) return;
    selection += 1;
    store.setState({ previewText: "", previewLoading: true, previewError: null, previewNextOffset: null });
    pendingPreview = { clipId: id, offset: 0, append: false };
    void pumpPreview();
  }
  /** Explicit next frame only: never loops, and never reuses another selection's offset. */
  function morePreview() {
    if (!active || !ready || !bridge.available) return;
    const state = store.getState();
    if (state.loading || state.previewLoading || state.selectedId === null || state.previewNextOffset === null) return;
    const id = state.selectedId;
    if (!state.summaries.some(item => item.id === id)) return;
    store.setState({ previewLoading: true, previewError: null });
    pendingPreview = { clipId: id, offset: state.previewNextOffset, append: true };
    void pumpPreview();
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
        ready = true;
        void refreshStatus();
        wantPage1 = true;
        void pump();
      } catch (error) {
        disposers.splice(0).forEach(off => off());
        if (active && token === lifetime) store.setState({ error: readableError(error), loading: false });
      }
    })();
    return () => {
      clearTimer();
      ready = false;
      if (token === lifetime) {
        active = false;
        lifetime++;
        clearSelection();
      }
      disposers.splice(0).forEach(off => off());
    };
  }
  async function more() {
    const state = store.getState();
    if (!active || !ready || !bridge.available || state.loading || !state.cursor) return;
    const normalized = currentQuery();
    if (normalized === null) return;
    const token = lifetime, gen = generation;
    if (!canDispatch(token, gen)) return;
    void runList(token, gen, normalized, state.cursor);
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
  return { store, start, refresh, more, toggle, setQuery, selectClip, retryPreview, morePreview };
}
