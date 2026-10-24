import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createHistoryModel } from "./history-store";
import type { DesktopBridge } from "./desktop";
import type { ClipSummary, GetClipTextResult, ListClipsResult, ListCursor, StatusResult } from "./ipc";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function summary(id: string, preview = `row-${id}`): ClipSummary {
  return { id, captured_at_ms: 1, kind: "text", preview, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 1, source_process: null, source_confidence: "unknown" };
}

function page(requestId: string, normalizedQuery: string, rows: ClipSummary[], nextCursor: ListCursor | null = null): ListClipsResult {
  return { request_id: requestId, normalized_query: normalizedQuery, summaries: rows, next_cursor: nextCursor };
}

function cursorOf(normalizedQuery: string): ListCursor {
  return { normalized_query: normalizedQuery, upper_id: "9", last_captured_at_ms: 1, last_id: "5" };
}

function status(revision = "1", items = 1): StatusResult {
  return {
    revision,
    state: "paused",
    services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "not_started", target_observer: "not_started" },
    capture: { user_enabled: false, admission_open: false, policy_epoch: 1 },
    counters: { observed: "1", captured: "1", persisted: "1", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" },
    queue: { items: 0, bytes: 0 },
    history: { items, payload_bytes: 8 },
    data_dir: "D:\\Synthetic",
    versions: { app: "0.1.0", schema: 1, worker_protocol: 1 },
    last_paste: null,
  };
}

interface ListCall {
  id: string;
  cursor: ListCursor | null;
  query: string | undefined;
  d: ReturnType<typeof deferred<ListClipsResult>>;
}

function harness(options: { available?: boolean } = {}) {
  const events: string[] = [];
  const listCalls: ListCall[] = [];
  const textCalls: Array<{ requestId: string; clipId: string; offset: number; d: ReturnType<typeof deferred<GetClipTextResult>> }> = [];
  const listeners: Record<string, () => void> = {};
  const disposals: string[] = [];
  const statusHolder = {
    calls: 0,
    impl: (): Promise<StatusResult> => Promise.resolve(status()),
  };
  const bridge: DesktopBridge = {
    available: options.available ?? true,
    status: vi.fn(() => { statusHolder.calls += 1; events.push("status"); return statusHolder.impl(); }),
    list: vi.fn((id: string, cursor: ListCursor | null, query?: string) => {
      events.push(`list:${query ?? "<none>"}`);
      const d = deferred<ListClipsResult>();
      listCalls.push({ id, cursor, query, d });
      return d.promise;
    }),
    getText: vi.fn((requestId: string, clipId: string, offset: number) => {
      events.push(`text:${clipId}@${offset}`);
      const d = deferred<GetClipTextResult>();
      textCalls.push({ requestId, clipId, offset, d });
      return d.promise;
    }),
    setEnabled: vi.fn(async () => status("2", 1)),
    subscribe: vi.fn(async (name: "history_changed" | "status_changed", callback: () => void) => {
      events.push(`sub:${name}`);
      listeners[name] = callback;
      return () => { disposals.push(name); delete listeners[name]; };
    }),
  };
  return { bridge, events, listCalls, textCalls, listeners, disposals, statusHolder };
}

function frame(call: { requestId: string; clipId: string }, text: string, nextOffset: number | null): GetClipTextResult {
  return { request_id: call.requestId, clip_id: call.clipId, text, next_offset: nextOffset };
}

/** Drain microtasks and any due timers without advancing the 50ms debounce. */
async function settle() {
  for (let i = 0; i < 8; i += 1) await vi.advanceTimersByTimeAsync(0);
}

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

it("A: keeps hostMissing silent in ordinary browsers and leaves no timers", async () => {
  const h = harness({ available: false });
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  expect(model.store.getState().hostMissing).toBe(true);
  model.setQuery("任何输入");
  await settle();
  await model.refresh();
  await model.more();
  expect(h.statusHolder.calls).toBe(0);
  expect(h.listCalls.length).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
  stop();
});

it("A: subscribes before the first fetch and loads the empty query without a debounce", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  expect(h.events.slice(0, 2)).toEqual(["sub:history_changed", "sub:status_changed"]);
  expect(h.events.findIndex(e => e === "status" || e.startsWith("list:"))).toBe(2);
  expect(h.listCalls.length).toBe(1);
  expect(h.listCalls[0].query).toBe("");
  expect(h.listCalls[0].cursor).toBeNull();
  expect(model.store.getState().query).toBe("");
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["1"]);
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("A: stop disposes subscriptions and clears pending timers", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  model.setQuery("排队中");
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  stop();
  expect(vi.getTimerCount()).toBe(0);
  expect(h.disposals.sort()).toEqual(["history_changed", "status_changed"]);
  h.listCalls.forEach(call => call.d.resolve(page(call.id, "", [])));
  await settle();
});

it("B: debounces 50ms to the latest query and invalidates rows immediately", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")], cursorOf("")));
  await settle();
  expect(model.store.getState().summaries.length).toBe(1);
  expect(model.store.getState().cursor).not.toBeNull();

  model.setQuery("a");
  expect(model.store.getState().query).toBe("a");
  expect(model.store.getState().summaries).toEqual([]);
  expect(model.store.getState().cursor).toBeNull();
  expect(model.store.getState().loading).toBe(true);
  model.setQuery("ab");
  model.setQuery("abc");
  expect(model.store.getState().query).toBe("abc");
  await vi.advanceTimersByTimeAsync(49);
  expect(h.listCalls.length).toBe(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(h.listCalls.length).toBe(2);
  expect(h.listCalls[1].query).toBe("abc");
  expect(h.listCalls[1].cursor).toBeNull();
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "abc", [summary("2")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["2"]);
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("B: sends the parser-normalized query while keeping the raw input in state", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", []));
  await settle();
  model.setQuery("\u{3000}\u{2003}剪贴  板%\u{a0}\t");
  expect(model.store.getState().query).toBe("\u{3000}\u{2003}剪贴  板%\u{a0}\t");
  await vi.advanceTimersByTimeAsync(50);
  expect(h.listCalls[1].query).toBe("剪贴  板%");
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "剪贴  板%", [summary("2")]));
  await settle();
  stop();
});

it("C: an in-flight predecessor cannot render or end the newer query's loading", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", []));
  await settle();
  model.setQuery("Q1");
  await vi.advanceTimersByTimeAsync(50);
  const q1 = h.listCalls[1];
  expect(q1.query).toBe("Q1");
  model.setQuery("Q2");
  expect(model.store.getState().loading).toBe(true);
  q1.d.resolve(page(q1.id, "Q1", [summary("q1row")]));
  await settle();
  expect(model.store.getState().summaries).toEqual([]);
  expect(model.store.getState().loading).toBe(true);
  await vi.advanceTimersByTimeAsync(50);
  const q2 = h.listCalls[2];
  expect(q2.query).toBe("Q2");
  q2.d.resolve(page(q2.id, "Q2", [summary("q2row")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["q2row"]);
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("D: late predecessors cannot overwrite a newer page and generations beat equal text", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", []));
  await settle();
  model.setQuery("Q1");
  await vi.advanceTimersByTimeAsync(50);
  const q1 = h.listCalls[1];
  model.setQuery("Q2");
  await vi.advanceTimersByTimeAsync(50);
  const q2 = h.listCalls[2];
  q2.d.resolve(page(q2.id, "Q2", [summary("q2row")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["q2row"]);
  q1.d.reject(new Error("stale failure"));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["q2row"]);
  expect(model.store.getState().error).toBeNull();
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("D: identical query text in a new generation is not the same request", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", []));
  await settle();
  model.setQuery("same");
  await vi.advanceTimersByTimeAsync(50);
  const first = h.listCalls[1];
  model.setQuery("other");
  await vi.advanceTimersByTimeAsync(50);
  const second = h.listCalls[2];
  second.d.resolve(page(second.id, "other", [summary("second")]));
  await settle();
  model.setQuery("same");
  await vi.advanceTimersByTimeAsync(50);
  const third = h.listCalls[3];
  third.d.resolve(page(third.id, "same", [summary("third")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["third"]);
  first.d.resolve(page(first.id, "same", [summary("first")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["third"]);
  stop();
});

it("E: caps unsettled list calls at two and dispatches only the latest pending query", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  const c0 = h.listCalls[0];
  model.setQuery("q1");
  await vi.advanceTimersByTimeAsync(50);
  const c1 = h.listCalls[1];
  expect(h.listCalls.length).toBe(2);
  model.setQuery("q2");
  model.setQuery("q3");
  model.setQuery("q4");
  await vi.advanceTimersByTimeAsync(50);
  await vi.advanceTimersByTimeAsync(50);
  expect(h.listCalls.length).toBe(2);
  expect(model.store.getState().query).toBe("q4");
  expect(model.store.getState().loading).toBe(true);
  c0.d.resolve(page(c0.id, "", []));
  await settle();
  expect(h.listCalls.length).toBe(3);
  expect(h.listCalls[2].query).toBe("q4");
  expect(model.store.getState().loading).toBe(true);
  c1.d.resolve(page(c1.id, "q1", [summary("q1row")]));
  await settle();
  expect(model.store.getState().summaries).toEqual([]);
  h.listCalls[2].d.resolve(page(h.listCalls[2].id, "q4", [summary("q4row")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["q4row"]);
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("F: coalesces history notifications into one follow-up first page", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  expect(h.listCalls.length).toBe(1);
  for (let i = 0; i < 100; i += 1) h.listeners.history_changed();
  expect(h.listCalls.length).toBe(1);
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")]));
  await settle();
  expect(h.listCalls.length).toBe(2);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "", [summary("2")]));
  await settle();
  expect(h.listCalls.length).toBe(2);
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("F: notifications during a debounce neither restore the old query nor jump ahead", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("old")]));
  await settle();
  model.setQuery("新查询");
  await vi.advanceTimersByTimeAsync(10);
  h.listeners.history_changed();
  await settle();
  expect(h.listCalls.length).toBe(1);
  await vi.advanceTimersByTimeAsync(40);
  expect(h.listCalls.length).toBe(2);
  expect(h.listCalls[1].query).toBe("新查询");
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "新查询", [summary("new")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["new"]);
  stop();
});

it("F: status count compensation refreshes the current query and keeps revision rules", async () => {
  const h = harness();
  let revision = "1";
  let items = 1;
  h.statusHolder.impl = () => Promise.resolve(status(revision, items));
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")]));
  await settle();
  model.setQuery("当前查询");
  await vi.advanceTimersByTimeAsync(50);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "当前查询", [summary("2")]));
  await settle();
  revision = "2";
  items = 9;
  h.listeners.status_changed();
  await settle();
  expect(h.listCalls.length).toBe(3);
  expect(h.listCalls[2].query).toBe("当前查询");
  h.listCalls[2].d.resolve(page(h.listCalls[2].id, "当前查询", [summary("3")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["3"]);
  expect(model.store.getState().status?.revision).toBe("2");
  // an older status revision must not overwrite the confirmed newer one
  revision = "1";
  items = 1;
  h.listeners.status_changed();
  await settle();
  expect(model.store.getState().status?.revision).toBe("2");
  stop();
});

it("G: a stale more page cannot append or restore a cursor after the query changes", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")], cursorOf("")));
  await settle();
  void model.more();
  await settle();
  expect(h.listCalls.length).toBe(2);
  const oldMore = h.listCalls[1];
  expect(oldMore.cursor).not.toBeNull();
  expect(oldMore.query).toBe("");
  model.setQuery("新");
  expect(model.store.getState().cursor).toBeNull();
  expect(model.store.getState().summaries).toEqual([]);
  oldMore.d.resolve(page(oldMore.id, "", [summary("old-more")], cursorOf("")));
  await settle();
  expect(model.store.getState().summaries).toEqual([]);
  expect(model.store.getState().cursor).toBeNull();
  await vi.advanceTimersByTimeAsync(50);
  expect(h.listCalls.length).toBe(3);
  expect(h.listCalls[2].query).toBe("新");
  expect(h.listCalls[2].cursor).toBeNull();
  h.listCalls[2].d.resolve(page(h.listCalls[2].id, "新", [summary("新")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["新"]);
  stop();
});

it("G: more refuses while loading and without a cursor", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  void model.more();
  await settle();
  expect(h.listCalls.length).toBe(1);
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")], cursorOf("")));
  await settle();
  model.setQuery("占位");
  void model.more();
  await settle();
  expect(h.listCalls.length).toBe(1);
  await vi.advanceTimersByTimeAsync(50);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "占位", [summary("2")]));
  await settle();
  void model.more();
  await settle();
  expect(h.listCalls.length).toBe(2);
  stop();
});

it("H: invalid input never sends IPC, keeps a fixed safe error, and recovers", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page(h.listCalls[0].id, "", [summary("1")], cursorOf("")));
  await settle();
  model.setQuery("密".repeat(257));
  expect(model.store.getState().summaries).toEqual([]);
  expect(model.store.getState().cursor).toBeNull();
  expect(model.store.getState().loading).toBe(false);
  expect(model.store.getState().error).toBeTruthy();
  expect(model.store.getState().error).not.toContain("密");
  await vi.advanceTimersByTimeAsync(50);
  expect(h.listCalls.length).toBe(1);
  // a normal status read must not wipe the uncorrected query error
  h.listeners.status_changed();
  await settle();
  expect(model.store.getState().error).toBeTruthy();
  expect(model.store.getState().error).not.toContain("密");
  // clearing invalidates at once and returns to the recent list after the debounce
  model.setQuery("");
  expect(model.store.getState().summaries).toEqual([]);
  expect(model.store.getState().loading).toBe(true);
  await vi.advanceTimersByTimeAsync(50);
  expect(h.listCalls.length).toBe(2);
  expect(h.listCalls[1].query).toBe("");
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "", [summary("2")]));
  await settle();
  expect(model.store.getState().error).toBeNull();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["2"]);
  stop();
});

it("I: correlation failures never write rows and stale failures cannot overwrite new success", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  h.listCalls[0].d.resolve(page("wrong-id", "", [summary("bad")]));
  await settle();
  expect(model.store.getState().summaries).toEqual([]);
  expect(model.store.getState().error).toBeTruthy();
  expect(model.store.getState().error).not.toContain("wrong-id");
  model.setQuery("q1");
  await vi.advanceTimersByTimeAsync(50);
  const c1 = h.listCalls[1];
  model.setQuery("q2");
  await vi.advanceTimersByTimeAsync(50);
  const c2 = h.listCalls[2];
  c2.d.resolve(page(c2.id, "q2", [summary("ok")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["ok"]);
  expect(model.store.getState().error).toBeNull();
  c1.d.reject(new Error("private payload must not become a user error"));
  await settle();
  expect(model.store.getState().error).toBeNull();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["ok"]);
  stop();
});

it("J: stop-start isolates late subscriptions, timers and old promises", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop1 = model.start();
  await settle();
  const c0 = h.listCalls[0];
  model.setQuery("old");
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  stop1();
  expect(vi.getTimerCount()).toBe(0);
  expect(h.disposals.sort()).toEqual(["history_changed", "status_changed"]);
  const settledWhileDown = h.listCalls.length;
  c0.d.resolve(page(c0.id, "", [summary("stale")]));
  await settle();
  expect(h.listCalls.length).toBe(settledWhileDown);
  expect(model.store.getState().summaries).toEqual([]);

  const stop2 = model.start();
  await settle();
  expect(h.listCalls.length).toBe(settledWhileDown + 1);
  const c2 = h.listCalls[h.listCalls.length - 1];
  expect(c2.query).toBe("old");
  c2.d.resolve(page(c2.id, "old", [summary("fresh")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["fresh"]);
  expect(model.store.getState().loading).toBe(false);
  stop2();
});

it("J: a late subscription dispose keeps the host untouched after unmount", async () => {
  const h = harness();
  let resolveSub!: (off: () => void) => void;
  h.bridge.subscribe = vi.fn(async () => new Promise<() => void>(resolve => { resolveSub = resolve; })) as unknown as DesktopBridge["subscribe"];
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  stop();
  const off = vi.fn();
  resolveSub(off);
  await settle();
  expect(off).toHaveBeenCalledTimes(1);
  expect(h.statusHolder.calls).toBe(0);
  expect(h.listCalls.length).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it("F1: an old settle must not bypass the new query debounce after a notification", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  const old = h.listCalls[0];
  model.setQuery("new-query");
  await vi.advanceTimersByTimeAsync(10);
  h.listeners.history_changed();
  old.d.resolve(page(old.id, "", []));
  await settle();
  expect(h.listCalls.map(c => c.query)).toEqual([""]);
  expect(model.store.getState().loading).toBe(true);
  await vi.advanceTimersByTimeAsync(40);
  expect(h.listCalls.map(c => c.query)).toEqual(["", "new-query"]);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "new-query", [summary("n")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["n"]);
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("F1: an old settle must not bypass the new query debounce after a manual refresh", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  const old = h.listCalls[0];
  model.setQuery("new-query");
  await vi.advanceTimersByTimeAsync(10);
  void model.refresh();
  await settle();
  old.d.resolve(page(old.id, "", []));
  await settle();
  expect(h.listCalls.map(c => c.query)).toEqual([""]);
  expect(model.store.getState().loading).toBe(true);
  await vi.advanceTimersByTimeAsync(40);
  expect(h.listCalls.map(c => c.query)).toEqual(["", "new-query"]);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "new-query", [summary("n")]));
  await settle();
  expect(model.store.getState().loading).toBe(false);
  stop();
});

it("F1: an old rejection must not bypass the new query debounce either", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  const old = h.listCalls[0];
  model.setQuery("new-query");
  await vi.advanceTimersByTimeAsync(10);
  h.listeners.history_changed();
  old.d.reject(new Error("stale failure"));
  await settle();
  expect(h.listCalls.map(c => c.query)).toEqual([""]);
  expect(model.store.getState().error).toBeNull();
  expect(model.store.getState().loading).toBe(true);
  await vi.advanceTimersByTimeAsync(40);
  expect(h.listCalls.map(c => c.query)).toEqual(["", "new-query"]);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "new-query", [summary("n")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["n"]);
  stop();
});

it("F1: status count compensation during a debounce waits for the debounce too", async () => {
  const h = harness();
  let revision = "1";
  let items = 1;
  h.statusHolder.impl = () => Promise.resolve(status(revision, items));
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  const old = h.listCalls[0];
  model.setQuery("new-query");
  await vi.advanceTimersByTimeAsync(10);
  revision = "2";
  items = 9;
  h.listeners.status_changed();
  await settle();
  old.d.resolve(page(old.id, "", []));
  await settle();
  expect(h.listCalls.map(c => c.query)).toEqual([""]);
  await vi.advanceTimersByTimeAsync(40);
  expect(h.listCalls.map(c => c.query)).toEqual(["", "new-query"]);
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "new-query", [summary("n")]));
  await settle();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["n"]);
  stop();
});

it("F2: restarting with an invalid retained query stays nonloading and recovers", async () => {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop1 = model.start();
  await settle();
  const first = h.listCalls[0];
  first.d.resolve(page(first.id, "", []));
  await settle();
  const bad = "密".repeat(257);
  model.setQuery(bad);
  expect(model.store.getState().loading).toBe(false);
  stop1();
  const stop2 = model.start();
  await settle();
  expect(h.listCalls.length).toBe(1);
  expect(model.store.getState().query).toBe(bad);
  expect(model.store.getState().error).toBeTruthy();
  expect(model.store.getState().error).not.toContain("密");
  expect(model.store.getState().loading).toBe(false);
  model.setQuery("合法");
  await vi.advanceTimersByTimeAsync(50);
  expect(h.listCalls.length).toBe(2);
  expect(h.listCalls[1].query).toBe("合法");
  h.listCalls[1].d.resolve(page(h.listCalls[1].id, "合法", [summary("ok")]));
  await settle();
  expect(model.store.getState().error).toBeNull();
  expect(model.store.getState().summaries.map(s => s.id)).toEqual(["ok"]);
  expect(model.store.getState().loading).toBe(false);
  stop2();
});

// ---- E09.3a selection and first-frame preview ----

function previewClean() {
  return { selectedId: null, previewText: "", previewLoading: false, previewError: null, previewNextOffset: null };
}

function rowsOf(previews: string[]): ClipSummary[] {
  return previews.map((preview, index) => summary(String(index + 1), preview));
}

/** Starts a model, resolves the first page and leaves it selectable. */
async function mountRows(previews: string[], nextCursor: ListCursor | null = null) {
  const h = harness();
  const model = createHistoryModel(h.bridge);
  const stop = model.start();
  await settle();
  const call = h.listCalls[0];
  call.d.resolve(page(call.id, "", rowsOf(previews), nextCursor));
  await settle();
  return { h, model, stop };
}

it("P: exposes selection and retry on the model", async () => {
  const { model, stop } = await mountRows(["甲"]);
  expect(typeof model.selectClip).toBe("function");
  expect(typeof model.retryPreview).toBe("function");
  stop();
});

it("P: starts with no selection and never calls getText", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  expect(model.store.getState().selectedId).toBeNull();
  expect(model.store.getState().previewText).toBe("");
  expect(model.store.getState().previewLoading).toBe(false);
  expect(model.store.getState().previewError).toBeNull();
  expect(model.store.getState().previewNextOffset).toBeNull();
  expect(h.textCalls.length).toBe(0);
  stop();
});

it("P: selecting a real id requests the first frame at offset 0", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("1");
  expect(model.store.getState().selectedId).toBe("1");
  expect(model.store.getState().previewLoading).toBe(true);
  expect(h.textCalls.length).toBe(1);
  expect(h.textCalls[0].clipId).toBe("1");
  expect(h.textCalls[0].offset).toBe(0);
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲正文", 9));
  await settle();
  const state = model.store.getState();
  expect(state.previewText).toBe("甲正文");
  expect(state.previewNextOffset).toBe(9);
  expect(state.previewLoading).toBe(false);
  expect(state.previewError).toBeNull();
  stop();
});

it("P: a Chinese emoji first frame keeps its next_offset and stops there", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  model.selectClip("1");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "中😀", 7));
  await settle();
  expect(model.store.getState().previewText).toBe("中😀");
  expect(model.store.getState().previewNextOffset).toBe(7);
  expect(h.textCalls.length).toBe(1);
  stop();
});

it("P: a valid empty body is accepted", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  model.selectClip("1");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "", null));
  await settle();
  const state = model.store.getState();
  expect(state.previewText).toBe("");
  expect(state.previewNextOffset).toBeNull();
  expect(state.previewError).toBeNull();
  expect(state.previewLoading).toBe(false);
  stop();
});

it("P: unknown ids and inert runtimes never call getText", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("999");
  expect(model.store.getState().selectedId).toBeNull();
  expect(h.textCalls.length).toBe(0);
  model.selectClip("0");
  model.selectClip("");
  expect(h.textCalls.length).toBe(0);
  stop();
  model.selectClip("1");
  expect(h.textCalls.length).toBe(0);
  const silent = harness({ available: false });
  const offline = createHistoryModel(silent.bridge);
  offline.start();
  offline.selectClip("1");
  await settle();
  expect(silent.textCalls.length).toBe(0);
});

it("P: reselecting the same id is idempotent", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  model.selectClip("1");
  model.selectClip("1");
  expect(h.textCalls.length).toBe(1);
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲正文", null));
  await settle();
  model.selectClip("1");
  expect(h.textCalls.length).toBe(1);
  expect(model.store.getState().previewText).toBe("甲正文");
  stop();
});

it("P: out-of-order results and failures cannot cross-contaminate", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("1");
  model.selectClip("2");
  expect(h.textCalls.length).toBe(2);
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "乙正文", null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  h.textCalls[0].d.reject(new Error("stale failure"));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  expect(model.store.getState().previewError).toBeNull();
  expect(model.store.getState().previewLoading).toBe(false);
  stop();
});

it("P: A→B→A separates equal ids by generation", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("1");
  model.selectClip("2");
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "乙正文", null));
  await settle();
  model.selectClip("1");
  expect(h.textCalls.length).toBe(3);
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲陈旧", null));
  await settle();
  expect(model.store.getState().previewText).toBe("");
  expect(model.store.getState().previewLoading).toBe(true);
  h.textCalls[2].d.resolve(frame(h.textCalls[2], "甲新", null));
  await settle();
  expect(model.store.getState().previewText).toBe("甲新");
  stop();
});

it("P: input change, refresh and history events clear the preview immediately", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  model.selectClip("1");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲正文", 9));
  await settle();
  expect(model.store.getState().previewText).toBe("甲正文");
  model.setQuery("新查询");
  expect(model.store.getState()).toMatchObject(previewClean());
  await vi.advanceTimersByTimeAsync(50);
  const next = h.listCalls[h.listCalls.length - 1];
  expect(next.query).toBe("新查询");
  next.d.resolve(page(next.id, "新查询", rowsOf(["乙"])));
  await settle();
  model.selectClip("1");
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "乙正文", null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  void model.refresh();
  expect(model.store.getState()).toMatchObject(previewClean());
  await settle();
  const refreshed = h.listCalls[h.listCalls.length - 1];
  refreshed.d.resolve(page(refreshed.id, refreshed.query ?? "", rowsOf(["乙"])));
  await settle();
  model.selectClip("1");
  h.textCalls[2].d.resolve(frame(h.textCalls[2], "乙又", null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙又");
  h.listeners.history_changed();
  expect(model.store.getState()).toMatchObject(previewClean());
  stop();
});

it("P: status count compensation clears the preview but a pure status refresh keeps it", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  let revision = "1";
  let items = 1;
  h.statusHolder.impl = () => Promise.resolve(status(revision, items));
  model.selectClip("1");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲正文", 9));
  await settle();
  h.listeners.status_changed();
  await settle();
  expect(model.store.getState().previewText).toBe("甲正文");
  revision = "2";
  items = 9;
  h.listeners.status_changed();
  await settle();
  expect(model.store.getState()).toMatchObject(previewClean());
  stop();
});

it("P: paging append keeps a still-valid selection without re-reading the first frame", async () => {
  const { h, model, stop } = await mountRows(["甲"], cursorOf(""));
  model.selectClip("1");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲正文", 9));
  await settle();
  void model.more();
  await settle();
  const call = h.listCalls[1];
  call.d.resolve(page(call.id, "", rowsOf(["丙"]), null));
  await settle();
  expect(model.store.getState().selectedId).toBe("1");
  expect(model.store.getState().previewText).toBe("甲正文");
  expect(model.store.getState().previewNextOffset).toBe(9);
  expect(h.textCalls.length).toBe(1);
  stop();
});

it("P: a list replacement drops a selection whose id left the rows", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("2");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "乙正文", 9));
  await settle();
  h.listeners.history_changed();
  await settle();
  const call = h.listCalls[1];
  call.d.resolve(page(call.id, "", rowsOf(["丙"])));
  await settle();
  expect(model.store.getState().selectedId).toBeNull();
  expect(model.store.getState().previewText).toBe("");
  stop();
});

it("P: two unsettled previews queue only the latest selection", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙", "丙", "丁"]);
  model.selectClip("1");
  model.selectClip("2");
  expect(h.textCalls.length).toBe(2);
  model.selectClip("3");
  model.selectClip("4");
  model.selectClip("3");
  await settle();
  expect(h.textCalls.length).toBe(2);
  h.textCalls[0].d.resolve(frame(h.textCalls[0], "甲正文", null));
  await settle();
  expect(h.textCalls.length).toBe(3);
  expect(h.textCalls[2].clipId).toBe("3");
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "乙正文", null));
  await settle();
  expect(h.textCalls.length).toBe(3);
  h.textCalls[2].d.resolve(frame(h.textCalls[2], "丙正文", null));
  await settle();
  expect(model.store.getState().previewText).toBe("丙正文");
  expect(model.store.getState().selectedId).toBe("3");
  stop();
});

it("P: stop-start isolates old preview promises and keeps the physical cap", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("1");
  const stale = h.textCalls[0];
  stop();
  expect(model.store.getState()).toMatchObject(previewClean());
  const resumed = model.start();
  await settle();
  const reload = h.listCalls[h.listCalls.length - 1];
  reload.d.resolve(page(reload.id, "", rowsOf(["甲", "乙"])));
  await settle();
  model.selectClip("1");
  expect(h.textCalls.length).toBe(2);
  model.selectClip("2");
  await settle();
  expect(h.textCalls.length).toBe(2);
  stale.d.resolve(frame(stale, "甲陈旧", null));
  await settle();
  expect(model.store.getState().previewText).toBe("");
  expect(h.textCalls.length).toBe(3);
  expect(h.textCalls[2].clipId).toBe("2");
  h.textCalls[2].d.resolve(frame(h.textCalls[2], "乙正文", null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "甲旧选", null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  resumed();
});

it("P: the request-context parser rejects bad frames", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  const cases: Array<(call: { requestId: string; clipId: string }) => GetClipTextResult> = [
    call => frame({ requestId: "wrong", clipId: call.clipId }, "x", null),
    call => frame({ requestId: call.requestId, clipId: "2" }, "x", null),
    call => frame(call, "x".repeat(65537), null),
    call => frame(call, "x", 99),
    call => frame(call, "", 1),
  ];
  for (const build of cases) {
    model.selectClip("1");
    const call = h.textCalls[h.textCalls.length - 1];
    call.d.resolve(build(call));
    await settle();
    expect(model.store.getState().previewText).toBe("");
    expect(model.store.getState().previewError).toBeTruthy();
    model.selectClip(null);
    await settle();
  }
  stop();
});

it("P: failures show a safe error and retry recovers with a new identity", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  model.selectClip("1");
  h.textCalls[0].d.reject(new Error("private body must not become a user message"));
  await settle();
  expect(model.store.getState().previewError).toBe("无法完成操作，请重试。");
  expect(model.store.getState().previewText).toBe("");
  expect(model.store.getState().selectedId).toBe("1");
  model.retryPreview();
  expect(h.textCalls.length).toBe(2);
  expect(h.textCalls[1].requestId).not.toBe(h.textCalls[0].requestId);
  expect(model.store.getState().previewLoading).toBe(true);
  expect(model.store.getState().previewError).toBeNull();
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "甲正文", 9));
  await settle();
  expect(model.store.getState().previewText).toBe("甲正文");
  expect(model.store.getState().previewError).toBeNull();
  stop();
});

it("P: retryPreview only revives a failing selection still in the rows", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.selectClip("2");
  h.textCalls[0].d.reject(new Error("boom"));
  await settle();
  expect(model.store.getState().previewError).toBeTruthy();
  model.retryPreview();
  expect(h.textCalls.length).toBe(2);
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "乙正文", null));
  await settle();
  model.retryPreview();
  expect(h.textCalls.length).toBe(2);
  model.setQuery("切换");
  await settle();
  const call = h.listCalls[h.listCalls.length - 1];
  call.d.resolve(page(call.id, "切换", rowsOf(["丙"])));
  await settle();
  model.retryPreview();
  await settle();
  expect(h.textCalls.length).toBe(2);
  expect(model.store.getState().selectedId).toBeNull();
  stop();
});


// ---- E09.3b explicit single-frame continuation ----

function byteLen(s: string): number {
  return new TextEncoder().encode(s).length;
}

const PREFIX = "甲正文"; // 9 UTF-8 bytes
const FRONTIER = 9;

/** Selects clip "1" and lands its first frame, returning after the state settles. */
async function withPrefix(h: ReturnType<typeof harness>, model: ReturnType<typeof createHistoryModel>, text: string, nextOffset: number | null) {
  model.selectClip("1");
  const call = h.textCalls[h.textCalls.length - 1];
  call.d.resolve(frame(call, text, nextOffset));
  await settle();
}

it("P: exposes explicit continuation on the model", async () => {
  const { model, stop } = await mountRows(["甲"]);
  expect(typeof model.morePreview).toBe("function");
  stop();
});

it("P: continuation uses server byte offsets and concatenates a multi-frame body", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  const head = "中".repeat(21845); // 21845 scalars / 65535 bytes
  const tail = "😀尾尾尾"; // 4 scalars / 13 bytes
  const body = head + tail; // 21849 scalars / 65548 bytes
  expect(byteLen(head)).toBe(65535);
  expect(byteLen(body)).toBe(65548);
  expect([...body].length).toBe(21849);
  await withPrefix(h, model, head, 65535);
  expect(model.store.getState().previewText).toBe(head);
  expect(model.store.getState().previewNextOffset).toBe(65535);
  model.morePreview();
  expect(h.textCalls.length).toBe(2);
  expect(h.textCalls[1].clipId).toBe("1");
  expect(h.textCalls[1].offset).toBe(65535);
  h.textCalls[1].d.resolve(frame(h.textCalls[1], tail, null));
  await settle();
  const state = model.store.getState();
  expect(state.previewText).toBe(body);
  expect(byteLen(state.previewText)).toBe(65548);
  expect(state.previewNextOffset).toBeNull();
  expect(state.previewLoading).toBe(false);
  expect(state.previewError).toBeNull();
  stop();
});

it("P: accepts a full 64KiB frame and both a closing frame and an empty last frame", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  const full = "x".repeat(65536);
  await withPrefix(h, model, full, 65536);
  expect(model.store.getState().previewText).toBe(full);
  model.morePreview();
  expect(h.textCalls[1].offset).toBe(65536);
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "尾", null));
  await settle();
  expect(model.store.getState().previewText).toBe(full + "尾");
  expect(model.store.getState().previewNextOffset).toBeNull();

  model.selectClip(null);
  await settle();
  await withPrefix(h, model, full, 65536);
  model.morePreview();
  const emptyTail = h.textCalls[h.textCalls.length - 1];
  emptyTail.d.resolve(frame(emptyTail, "", null));
  await settle();
  expect(model.store.getState().previewText, "an empty last frame never fabricates text").toBe(full);
  expect(model.store.getState().previewNextOffset).toBeNull();
  expect(model.store.getState().previewLoading).toBe(false);
  stop();
});

it("P: continuation never starts without a live selection, frontier, host or idle list", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  model.morePreview();
  await settle();
  expect(h.textCalls.length).toBe(0);
  await withPrefix(h, model, PREFIX, null);
  model.morePreview();
  await settle();
  expect(h.textCalls.length, "no frontier means no continuation").toBe(1);
  model.selectClip(null);
  await settle();
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.setQuery("占位");
  model.morePreview();
  await settle();
  expect(h.textCalls.length, "list loading blocks continuation clicks").toBe(2);
  await vi.advanceTimersByTimeAsync(50);
  const call = h.listCalls[h.listCalls.length - 1];
  call.d.resolve(page(call.id, "占位", rowsOf(["甲"])));
  await settle();
  stop();
  model.morePreview();
  await settle();
  expect(h.textCalls.length).toBe(2);
  const silent = harness({ available: false });
  const offline = createHistoryModel(silent.bridge);
  offline.start();
  offline.morePreview();
  await settle();
  expect(silent.textCalls.length).toBe(0);
});

it("P: repeated continuation clicks collapse into one request", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  model.morePreview();
  model.morePreview();
  await settle();
  expect(h.textCalls.length).toBe(2);
  expect(h.textCalls[1].offset).toBe(FRONTIER);
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "乙", 12));
  await settle();
  expect(model.store.getState().previewText).toBe(PREFIX + "乙");
  expect(model.store.getState().previewNextOffset).toBe(12);
  stop();
});

it("P: a failed continuation keeps the prefix and retries the same offset", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  h.textCalls[1].d.reject(new Error("private body must not leak"));
  await settle();
  expect(model.store.getState().previewText).toBe(PREFIX);
  expect(model.store.getState().previewNextOffset).toBe(FRONTIER);
  expect(model.store.getState().previewError).toBe("无法完成操作，请重试。");
  expect(model.store.getState().previewLoading).toBe(false);
  model.morePreview();
  expect(h.textCalls.length).toBe(3);
  expect(h.textCalls[2].offset).toBe(FRONTIER);
  expect(model.store.getState().previewLoading).toBe(true);
  expect(model.store.getState().previewError).toBeNull();
  h.textCalls[2].d.resolve(frame(h.textCalls[2], "乙", 12));
  await settle();
  expect(model.store.getState().previewText).toBe(PREFIX + "乙");
  expect(model.store.getState().previewNextOffset).toBe(12);
  expect(model.store.getState().previewError).toBeNull();
  stop();
});

it("P: bad continuation frames are rejected without touching the prefix", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  const cases: Array<(call: { requestId: string; clipId: string }) => GetClipTextResult> = [
    call => frame({ requestId: "wrong", clipId: call.clipId }, "乙", 12),
    call => frame({ requestId: call.requestId, clipId: "2" }, "乙", 12),
    call => frame(call, "乙", 99),
    call => frame(call, "中".repeat(30000), 90009),
  ];
  for (const build of cases) {
    model.morePreview();
    const call = h.textCalls[h.textCalls.length - 1];
    expect(call.offset).toBe(FRONTIER);
    call.d.resolve(build(call));
    await settle();
    const state = model.store.getState();
    expect(state.previewText).toBe(PREFIX);
    expect(state.previewNextOffset).toBe(FRONTIER);
    expect(state.previewError).toBeTruthy();
    expect(state.previewLoading).toBe(false);
    model.selectClip(null);
    await settle();
    await withPrefix(h, model, PREFIX, FRONTIER);
  }
  stop();
});

it("P: a frame over 64KiB of multibyte text is rejected on the byte count", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  const huge = "中".repeat(30000);
  expect(huge.length, "30000 UTF-16 code units stay under the scalar cap").toBe(30000);
  expect(byteLen(huge)).toBe(90000);
  model.selectClip("1");
  h.textCalls[0].d.resolve(frame(h.textCalls[0], huge, 90000));
  await settle();
  const state = model.store.getState();
  expect(state.previewText, "an oversize frame must not be accepted").toBe("");
  expect(state.previewError).toBeTruthy();
  expect(state.previewNextOffset).toBeNull();
  stop();
});

it("P: a late continuation cannot backfill after the query changed", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  const stale = h.textCalls[h.textCalls.length - 1];
  model.setQuery("换");
  stale.d.resolve(frame(stale, "陈旧", 15));
  await settle();
  expect(model.store.getState()).toMatchObject(previewClean());
  stop();
});

it("P: a late continuation cannot backfill after a first-page refresh", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  const stale = h.textCalls[h.textCalls.length - 1];
  void model.refresh();
  stale.d.resolve(frame(stale, "陈旧", 15));
  await settle();
  expect(model.store.getState().previewText).toBe("");
  expect(model.store.getState().previewNextOffset).toBeNull();
  expect(model.store.getState().previewLoading).toBe(false);
  stop();
});

it("P: a late continuation cannot backfill after the selection changed or stopped", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  let stale = h.textCalls[h.textCalls.length - 1];
  model.selectClip("2");
  stale.d.resolve(frame(stale, "陈旧", 15));
  await settle();
  expect(model.store.getState().previewText).toBe("");
  expect(model.store.getState().selectedId).toBe("2");
  const second = h.textCalls[h.textCalls.length - 1];
  second.d.resolve(frame(second, "乙文", 6));
  await settle();
  expect(model.store.getState().previewText).toBe("乙文");
  model.morePreview();
  stale = h.textCalls[h.textCalls.length - 1];
  stop();
  stale.d.resolve(frame(stale, "陈旧", 6));
  await settle();
  expect(model.store.getState()).toMatchObject(previewClean());
});

it("P: a full slot pair dispatches the latest selection at offset 0, not a stale continuation", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙", "丙"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  expect(h.textCalls[1].offset).toBe(FRONTIER);
  model.selectClip("2");
  expect(h.textCalls.length).toBe(3);
  expect(h.textCalls[2].offset).toBe(0);
  model.selectClip("3");
  await settle();
  expect(h.textCalls.length, "two slots stay occupied").toBe(3);
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "陈旧", 15));
  await settle();
  expect(h.textCalls.length).toBe(4);
  expect(h.textCalls[3].clipId).toBe("3");
  expect(h.textCalls[3].offset, "the pending selection starts at zero").toBe(0);
  h.textCalls[3].d.resolve(frame(h.textCalls[3], "丙正文", null));
  await settle();
  expect(model.store.getState().previewText).toBe("丙正文");
  stop();
});

it("P: stop-start keeps the preview slot cap across lifetimes", async () => {
  const { h, model, stop } = await mountRows(["甲", "乙"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  const stale = h.textCalls[1];
  stop();
  const resumed = model.start();
  await settle();
  const reload = h.listCalls[h.listCalls.length - 1];
  reload.d.resolve(page(reload.id, "", rowsOf(["甲", "乙"])));
  await settle();
  model.selectClip("1");
  expect(h.textCalls.length).toBe(3);
  model.selectClip("2");
  await settle();
  expect(h.textCalls.length).toBe(3);
  stale.d.resolve(frame(stale, "陈旧", 15));
  await settle();
  expect(model.store.getState().previewText).toBe("");
  expect(h.textCalls.length).toBe(4);
  expect(h.textCalls[3].clipId).toBe("2");
  expect(h.textCalls[3].offset).toBe(0);
  h.textCalls[3].d.resolve(frame(h.textCalls[3], "乙正文", null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  h.textCalls[2].d.resolve(frame(h.textCalls[2], PREFIX, null));
  await settle();
  expect(model.store.getState().previewText).toBe("乙正文");
  resumed();
});

it("P: history paging keeps the same-id body and a pure status refresh keeps it too", async () => {
  const { h, model, stop } = await mountRows(["甲"], cursorOf(""));
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.morePreview();
  h.textCalls[1].d.resolve(frame(h.textCalls[1], "甲后", 15));
  await settle();
  expect(model.store.getState().previewText).toBe(PREFIX + "甲后");
  void model.more();
  await settle();
  const call = h.listCalls[1];
  call.d.resolve(page(call.id, "", rowsOf(["丙"]), null));
  await settle();
  expect(model.store.getState().previewText).toBe(PREFIX + "甲后");
  expect(model.store.getState().previewNextOffset).toBe(15);
  expect(model.store.getState().selectedId).toBe("1");
  h.listeners.status_changed();
  await settle();
  expect(model.store.getState().previewText).toBe(PREFIX + "甲后");
  expect(model.store.getState().previewNextOffset).toBe(15);
  stop();
});

it("P: whitespace-only and oversized input clear the preview immediately", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.setQuery("   ");
  expect(model.store.getState()).toMatchObject(previewClean());
  await vi.advanceTimersByTimeAsync(50);
  const blank = h.listCalls[h.listCalls.length - 1];
  blank.d.resolve(page(blank.id, "", rowsOf(["甲"])));
  await settle();
  await withPrefix(h, model, PREFIX, FRONTIER);
  model.setQuery("密".repeat(257));
  expect(model.store.getState()).toMatchObject(previewClean());
  expect(model.store.getState().error).toBeTruthy();
  expect(model.store.getState().error).not.toContain("密");
  stop();
});

it("P: retryPreview is idempotent while a frame is loading", async () => {
  const { h, model, stop } = await mountRows(["甲"]);
  model.selectClip("1");
  expect(model.store.getState().previewLoading).toBe(true);
  model.retryPreview();
  model.retryPreview();
  expect(h.textCalls.length).toBe(1);
  h.textCalls[0].d.resolve(frame(h.textCalls[0], PREFIX, FRONTIER));
  await settle();
  model.retryPreview();
  expect(h.textCalls.length).toBe(1);
  stop();
});
