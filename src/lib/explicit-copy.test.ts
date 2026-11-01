import { beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/explicit_copy.json";
import { parseCopyClipTextRequest, parseCopyClipTextResult } from "./ipc";
const mock = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mock.invoke, isTauri: () => true }));
import { nativeBridge } from "./desktop";
beforeEach(() => { mock.invoke.mockReset(); });
it("explicit_copy shares strict request and correlated result fixtures with Rust", () => {
  for (const c of fixture.requests) expect(parseCopyClipTextRequest(c.value).ok).toBe(c.valid);
  for (const c of fixture.results) expect(parseCopyClipTextResult(c.value, fixture.request).ok).toBe(c.valid);
});
it("explicit_copy sends only stored identity and validates success correlation", async () => {
  mock.invoke.mockResolvedValue({ ...fixture.request, outcome: "copied", reason: null });
  expect((await nativeBridge.copyText!("copy-1", "1")).outcome).toBe("copied");
  expect(mock.invoke).toHaveBeenCalledWith("copy_clip_text", { request: fixture.request });
  mock.invoke.mockResolvedValue({ ...fixture.request, clip_id: "2", outcome: "copied", reason: null });
  expect((await nativeBridge.copyText!("copy-1", "1")).outcome).toBe("unknown");
  mock.invoke.mockRejectedValue({ code: "busy", message: "private" });
  expect((await nativeBridge.copyText!("copy-1", "1")).outcome).toBe("unknown");
});
it("explicit_copy rejects invalid identity before invoke", async () => {
  await expect(nativeBridge.copyText!("copy-1", "01")).rejects.toThrow();
  expect(mock.invoke).not.toHaveBeenCalled();
});
