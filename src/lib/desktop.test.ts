import { beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/response_dtos.json";
const mocks = vi.hoisted(() => ({ invoke: vi.fn<(...args: unknown[]) => Promise<unknown>>(), listen: vi.fn<(name: string, callback: (event: { payload: unknown }) => void) => Promise<() => void>>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
import { nativeBridge } from "./desktop";
beforeEach(() => { mocks.invoke.mockReset(); mocks.listen.mockReset(); });
it("uses the existing status contract and one request argument for changes", async () => {
  mocks.invoke.mockResolvedValue(fixture.status_cases.find(item => item.id === "st_ok")!.value);
  const status = await nativeBridge.status(); expect(status.revision).toBe("3");
  await nativeBridge.setEnabled(false); expect(mocks.invoke).toHaveBeenLastCalledWith("set_capture_enabled", { request: { enabled: false } });
});
it("verifies list correlation instead of displaying an unrelated response", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "wrong", normalized_query: "", summaries: [], next_cursor: null });
  await expect(nativeBridge.list("current", null)).rejects.toThrow();
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "current", query: "", limit: 50, cursor: null } });
});
it("rejects unknown status shapes and ignores malformed event revisions", async () => {
  mocks.invoke.mockResolvedValue({ private_data: "never display" }); await expect(nativeBridge.status()).rejects.toThrow("无法读取宿主数据");
  const off = vi.fn(); const callback = vi.fn(); mocks.listen.mockResolvedValue(off); const dispose = await nativeBridge.subscribe("history_changed", callback);
  const listener = mocks.listen.mock.calls[0][1]; listener({ payload: { revision: "01" } }); listener({ payload: { revision: "1", extra: true } }); expect(callback).not.toHaveBeenCalled();listener({ payload: { revision: "1" } });expect(callback).toHaveBeenCalledTimes(1);dispose();expect(off).toHaveBeenCalledTimes(1);
});
