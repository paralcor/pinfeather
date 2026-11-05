import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createHistoryModel } from "./history-store";
import type { DesktopBridge } from "./desktop";
import type { Identity, Receipt } from "./measurement";
import type { ListClipsResult, StatusResult } from "./ipc";
import statuses from "../../tests/fixtures/contracts/response_dtos.json";
const status = statuses.status_cases.find(x => x.id === "st_ok")!.value as StatusResult;
beforeEach(() => vi.useFakeTimers()); afterEach(() => vi.useRealTimers());
async function settle() { for (let i = 0; i < 10; i++) await vi.advanceTimersByTimeAsync(0); }
function harness() {
  let next = 0, statusValue = status;
  const listeners: Record<string, () => void> = {};
  const calls: Array<{ identity: Identity; generation: string; sequence: string; id: string; cursor: unknown; query: string; resolve(value: { result: ListClipsResult; receipt: Receipt | null }): void; reject(error: unknown): void }> = [];
  const invalidations: unknown[] = [];
  const measurement = {
    start: vi.fn(async (): Promise<Identity> => ({ schema: 1, run_id: "test", attempt: "1", window: "2", session: null, model: String(++next) })),
    invalidate: vi.fn(async (...args: unknown[]) => { invalidations.push(args); }),
    list: vi.fn((identity: Identity, generation: string, sequence: string, id: string, cursor: unknown, query: string) => new Promise<{ result: ListClipsResult; receipt: Receipt | null }>((resolve, reject) => calls.push({ identity, generation, sequence, id, cursor, query, resolve, reject }))),
    stop: vi.fn(async () => {}), ack: vi.fn(async () => {}),
  };
  const bridge: DesktopBridge = { available: true, measurement, list: vi.fn(), status: vi.fn(async () => statusValue), getText: vi.fn(), setEnabled: vi.fn(), subscribe: vi.fn(async (name, callback) => { listeners[name] = callback; return () => { delete listeners[name]; }; }) };
  const model = createHistoryModel(bridge);
  function complete(index: number, empty = false, cursor = false) {
    const call = calls[index];
    const receipt: Receipt = { ...call.identity, generation: call.generation, sequence: call.sequence, receipt: String(index + 20), content: empty ? "empty" : "nonempty" };
    call.resolve({ result: { request_id: call.id, normalized_query: call.query, summaries: empty ? [] : [{ id: "1", captured_at_ms: 1, preview: "real row", kind: "text", format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown" }], next_cursor: cursor ? { normalized_query: call.query, upper_id: "9", last_id: "1", last_captured_at_ms: 1 } : null }, receipt: call.cursor ? null : receipt });
    return receipt;
  }
  return { model, measurement, calls, complete, listeners, invalidations, setStatus(value: StatusResult) { statusValue = value; } };
}
it("initial empty is not success; real empty and nonempty homepage commits carry host receipts", async () => {
  const h = harness(); expect(h.model.measurementReceipt()).toBeNull(); const stop = h.model.start(); await settle(); expect(h.model.measurementReceipt()).toBeNull(); const empty = h.complete(0, true); await settle(); expect(h.model.measurementReceipt()).toEqual(empty); h.model.acknowledgeMeasurement(empty, false); h.model.acknowledgeMeasurement(empty, false); expect(h.measurement.ack).toHaveBeenCalledTimes(1); stop(); expect(h.model.measurementReceipt()).toBeNull(); await settle(); expect(h.measurement.stop).toHaveBeenCalledTimes(1);
});
it("clears a committed receipt before query debounce, rejects late response, and isolates restart identities", async () => {
  const h = harness(); const stop = h.model.start(); await settle(); const old = h.complete(0); await settle(); h.model.setQuery("new"); expect(h.model.measurementReceipt()).toBeNull(); h.model.acknowledgeMeasurement(old, true); expect(h.measurement.ack).not.toHaveBeenCalled(); await vi.advanceTimersByTimeAsync(50); await settle(); stop(); const stopped = h.complete(1); const stop2 = h.model.start(); await settle(); expect(h.model.measurementReceipt()).toBeNull(); h.model.acknowledgeMeasurement(stopped, true); expect(h.measurement.ack).not.toHaveBeenCalled(); expect(h.calls[2].identity.model).not.toEqual(h.calls[1].identity.model); h.complete(2); await settle(); expect(h.model.measurementReceipt()?.model).toBe(h.calls[2].identity.model); stop2();
});
it("refresh/history/status compensation invalidate synchronously and require a new homepage version", async () => {
  const h = harness(); const stop = h.model.start(); await settle(); const a = h.complete(0); await settle(); void h.model.refresh(); expect(h.model.measurementReceipt()).toBeNull(); await settle(); h.complete(1); await settle(); expect(h.model.measurementReceipt()?.sequence).not.toBe(a.sequence); h.listeners.history_changed(); expect(h.model.measurementReceipt()).toBeNull(); await settle(); h.complete(2); await settle(); h.setStatus({ ...status, revision: String(BigInt(status.revision) + 1n), history: { ...status.history, items: status.history.items + 1 } }); h.listeners.status_changed(); await settle(); expect(h.model.measurementReceipt()).toBeNull(); h.model.acknowledgeMeasurement(a, false); expect(h.measurement.ack).not.toHaveBeenCalled(); h.complete(3); await settle(); expect(h.model.measurementReceipt()?.sequence).toBe(h.calls[3].sequence); stop();
});
it("request errors and pagination never mint a new ready receipt", async () => {
  const h = harness(); const stop = h.model.start(); await settle(); const a = h.complete(0, false, true); await settle(); void h.model.more(); await settle(); expect(h.model.store.getState().loading).toBe(true); h.model.acknowledgeMeasurement(a, false); expect(h.measurement.ack).not.toHaveBeenCalled(); h.complete(1); await settle(); expect(h.model.measurementReceipt()).toEqual(a); void h.model.refresh(); expect(h.model.measurementReceipt()).toBeNull(); await settle(); h.calls[2].reject(new Error("synthetic query failure")); await settle(); expect(h.model.store.getState().error).not.toBeNull(); expect(h.model.measurementReceipt()).toBeNull(); stop();
});
it("closed measurement falls back to the original list with no ACK or receipt", async () => {
  const h = harness(); h.measurement.start.mockRejectedValue("expired");
  // Exercise the production model's fallback transport, never a synthetic ready state.
  const { nativeBridge } = await import("./desktop");
  const ordinary = vi.spyOn(nativeBridge, "list").mockResolvedValue({ request_id: "page1-1", normalized_query: "", summaries: [], next_cursor: null });
  const bridge: DesktopBridge = { ...nativeBridge, available: true, measurement: h.measurement, status: async () => status, subscribe: async () => () => {}, list: ordinary };
  const model = createHistoryModel(bridge); const stop = model.start(); await settle(); expect(ordinary).toHaveBeenCalledTimes(1); expect(model.store.getState().error).toBeNull(); expect(model.measurementReceipt()).toBeNull(); expect(h.measurement.ack).not.toHaveBeenCalled(); stop(); ordinary.mockRestore();
});
it("rejected readiness submission retires its receipt without retry or accepted ACK", async () => {
  const h = harness(); let reject!: (error: unknown) => void;
  h.measurement.ack.mockImplementation(() => new Promise<void>((_, fail) => { reject = fail; }));
  const stop = h.model.start(); await settle(); const r = h.complete(0); await settle();
  h.model.acknowledgeMeasurement(r, false); h.model.acknowledgeMeasurement(r, false);
  expect(h.measurement.ack).toHaveBeenCalledTimes(1);
  reject("not_interactive"); await settle();
  expect(h.model.measurementReceipt()).toBeNull();
  h.model.acknowledgeMeasurement(r, false); await settle();
  expect(h.measurement.ack).toHaveBeenCalledTimes(1); expect(h.model.store.getState().error).toBeNull(); stop();
});
it.each(["refresh", "restart"])("late rejected readiness cannot clear a %s successor receipt", async replacement => {
  const h = harness(); let reject!: (error: unknown) => void;
  h.measurement.ack.mockImplementationOnce(() => new Promise<void>((_, fail) => { reject = fail; }));
  let stop = h.model.start(); await settle(); const a = h.complete(0); await settle(); h.model.acknowledgeMeasurement(a, false);
  if (replacement === "restart") { stop(); stop = h.model.start(); } else void h.model.refresh();
  await settle(); const b = h.complete(1); await settle();
  reject(new Error("uncertain transport")); await settle(); expect(h.model.measurementReceipt()).toEqual(b);
  h.model.acknowledgeMeasurement(b, false); await settle(); expect(h.measurement.ack).toHaveBeenCalledTimes(2); stop();
});
