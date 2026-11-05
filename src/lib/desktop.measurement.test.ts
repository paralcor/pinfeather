import { afterEach, beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/measurement.json";
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { nativeBridge } from "./desktop";
import { nativeMeasurement, parseIdentity } from "./measurement";
const identity = parseIdentity(fixture.identity);
const result = { request_id: "actual", normalized_query: "", summaries: [], next_cursor: null };
beforeEach(() => mocks.invoke.mockReset());
afterEach(() => { delete (globalThis as Record<string, unknown>).__PINFEATHER_MEASUREMENT__; });
it("default mode keeps the old command and exact DTO without a measurement handshake", async () => {
  expect(nativeBridge.measurement).toBeUndefined(); mocks.invoke.mockResolvedValue(result);
  expect(await nativeBridge.list("actual", null)).toEqual(result);
  expect(mocks.invoke.mock.calls).toEqual([["list_clips", { request: { request_id: "actual", query: "", limit: 50, cursor: null } }]]);
});
it("measurement command preserves the original list request/result and rejects bad host receipts", async () => {
  const receipt = { ...fixture.receipt, content: "empty" };
  mocks.invoke.mockResolvedValue({ result, receipt });
  const page = await nativeMeasurement.list(identity, "0", "1", "actual", null, ""); expect(page.result).toEqual(result); expect(page.receipt?.content).toBe("empty");
  expect(mocks.invoke).toHaveBeenLastCalledWith("measure_list_clips", { request: { ...identity, generation: "0", sequence: "1", query_request: { request_id: "actual", query: "", limit: 50, cursor: null } } });
  for (const change of [{ run_id: "old" }, { attempt: "7" }, { model: "9" }, { sequence: "2" }, { generation: "1" }, { content: "nonempty" }, { private: "unrecognized" }]) {
    mocks.invoke.mockResolvedValue({ result, receipt: { ...receipt, ...change } }); await expect(nativeMeasurement.list(identity, "0", "1", "actual", null, "")).rejects.toThrow();
  }
  mocks.invoke.mockResolvedValue({ result: { ...result, extra: true }, receipt }); await expect(nativeMeasurement.list(identity, "0", "1", "actual", null, "")).rejects.toThrow();
});
