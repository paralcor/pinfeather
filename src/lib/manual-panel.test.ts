import { beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/manual_panel.json";
import { parseManualPanelSessionRequest, parseManualPanelCopyRequest, parseManualPanelSnapshot } from "./ipc";
const mock = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mock.invoke, isTauri: () => true }));
import { nativePanelControl } from "./desktop";
beforeEach(() => { mock.invoke.mockReset(); });
it("manual_panel shares strict session/copy fixtures with Rust and validates snapshots", () => {
  for (const c of fixture.sessions) expect(parseManualPanelSessionRequest(c.value).ok).toBe(c.valid);
  for (const c of fixture.copies) expect(parseManualPanelCopyRequest(c.value).ok).toBe(c.valid);
  for (const c of fixture.snapshots) expect(parseManualPanelSnapshot(c.value) !== null).toBe(c.valid);
});
it("manual_panel uses its session endpoint and rejects a late/malformed response without replay", async () => {
  const session = "a".repeat(32), request = { request_id: "panel-copy-1", clip_id: "1" };
  const bridge = nativePanelControl.history(session);
  mock.invoke.mockResolvedValue({ session_id: session, result: { ...request, outcome: "copied", reason: null } });
  expect((await bridge.copyText!(request.request_id, "1")).outcome).toBe("copied");
  expect(mock.invoke).toHaveBeenCalledWith("copy_manual_panel_text", { request: { session_id: session, request } });
  mock.invoke.mockResolvedValue({ session_id: "b".repeat(32), result: { ...request, outcome: "copied", reason: null } });
  expect((await bridge.copyText!(request.request_id, "1")).outcome).toBe("unknown");
  mock.invoke.mockRejectedValue({ code: "busy" }); expect((await bridge.copyText!(request.request_id, "1")).outcome).toBe("unknown"); expect(mock.invoke).toHaveBeenCalledTimes(3);
});
it("manual_panel rejects invalid session/identity before IPC", async () => { await expect(nativePanelControl.ready("forged")).rejects.toThrow(); await expect(nativePanelControl.history("forged").copyText!("copy", "1")).rejects.toThrow(); expect(mock.invoke).not.toHaveBeenCalled(); });
it("manual_panel keeps one physical IPC copy in flight across reopened sessions", async () => {
  const session = "a".repeat(32); let finish!: (value: unknown) => void;
  mock.invoke.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const first = nativePanelControl.history(session).copyText!("first", "1");
  const second = nativePanelControl.history("b".repeat(32)).copyText!("second", "2");
  expect(mock.invoke).toHaveBeenCalledTimes(1);
  expect(await second).toEqual({ request_id: "second", clip_id: "2", outcome: "not_written", reason: "busy" });
  finish({ session_id: session, result: { request_id: "first", clip_id: "1", outcome: "copied", reason: null } }); await first;
});
