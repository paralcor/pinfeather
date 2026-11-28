import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/measurement.json";
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { nativeBridge } from "./desktop";
import { nativeMeasurement, parseIdentity } from "./measurement";
const identity = parseIdentity(fixture.identity);
const result = { request_id: "actual", normalized_query: "", summaries: [], next_cursor: null };
beforeEach(() => { mocks.invoke.mockReset(); });
afterEach(() => { delete (globalThis as Record<string, unknown>).__PINFEATHER_MEASUREMENT__; });
it("default mode uses the ordinary list command without a measurement handshake", async () => {
  expect(nativeBridge.measurement).toBeUndefined(); mocks.invoke.mockResolvedValue(result);
  expect(await nativeBridge.list("actual", null)).toEqual(result);
  expect(mocks.invoke.mock.calls).toEqual([["list_clips", { request: { request_id: "actual", query: "", limit: 50, cursor: null, favorites_only: false } }]]);
});
it("measurement command forwards the normalized list request/result and rejects bad host receipts", async () => {
  const receipt = { ...fixture.receipt, content: "empty" };
  mocks.invoke.mockResolvedValue({ result, receipt });
  const page = await nativeMeasurement.list(identity, "0", "1", "actual", null, ""); expect(page.result).toEqual(result); expect(page.receipt?.content).toBe("empty");
  expect(mocks.invoke).toHaveBeenLastCalledWith("measure_list_clips", { request: { ...identity, generation: "0", sequence: "1", query_request: { request_id: "actual", query: "", limit: 50, cursor: null, favorites_only: false } } });
  for (const change of [{ run_id: "old" }, { attempt: "7" }, { model: "9" }, { sequence: "2" }, { generation: "1" }, { content: "nonempty" }, { private: "unrecognized" }]) {
    mocks.invoke.mockResolvedValue({ result, receipt: { ...receipt, ...change } }); await expect(nativeMeasurement.list(identity, "0", "1", "actual", null, "")).rejects.toThrow();
  }
  mocks.invoke.mockResolvedValue({ result: { ...result, extra: true }, receipt }); await expect(nativeMeasurement.list(identity, "0", "1", "actual", null, "")).rejects.toThrow();
});

it.each([true, false])("forwards the favorite-only page with or without a measured result (%s)", async measured => {
  const { createHistoryModel } = await import("./history-store");
  const status = (await import("../../tests/fixtures/contracts/response_dtos.json")).default.status_cases.find(test => test.id === "st_ok")!.value;
  const summary = { id: "3", captured_at_ms: 3, kind: "text", preview: "sample", format_names: ["CF_UNICODETEXT"],
    omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown", favorited: true };
  mocks.invoke.mockImplementation(async (name: string, args?: { request: Record<string, unknown> }) => {
    if (name === "get_status") return status;
    if (name === "measure_start_model") return identity;
    if (name === "measure_invalidate" || name === "measure_stop_model") return;
    const request = name === "measure_list_clips" ? args?.request.query_request as Record<string, unknown> : args?.request;
    const page = { ...result, request_id: request?.request_id, normalized_query: request?.query,
      summaries: request?.favorites_only ? [summary] : [] };
    if (name === "measure_list_clips") {
      if (!measured) throw "stale";
      return { result: page, receipt: { ...identity, content: page.summaries.length ? "nonempty" : "empty",
        generation: args?.request.generation, sequence: args?.request.sequence, receipt: "1" } };
    }
    if (name === "list_clips") return page;
    throw new Error("unexpected transport call " + name);
  });
  const model = createHistoryModel({ ...nativeBridge, subscribeFavorite: undefined, subscribe: async () => () => {}, measurement: nativeMeasurement });
  const stop = model.start();
  await vi.waitFor(() => expect(model.store.getState().loading).toBe(false));
  model.setQuery(" sample ");
  model.setFavoritesOnly(true);
  await vi.waitFor(() => expect(model.store.getState().summaries).toEqual([summary]));
  expect(model.store.getState().cursor).toBeNull();
  const expected = { request_id: expect.any(String), query: "sample", limit: 50, cursor: null, favorites_only: true };
  expect(mocks.invoke).toHaveBeenCalledWith("measure_list_clips", { request: expect.objectContaining({ schema: 1, query_request: expected }) });
  if (!measured) expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: expected });
  expect(model.measurementReceipt()?.schema ?? null).toBe(measured ? 1 : null);
  stop();
});
