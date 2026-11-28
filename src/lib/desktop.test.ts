import { beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/response_dtos.json";
const mocks = vi.hoisted(() => ({ invoke: vi.fn<(...args: unknown[]) => Promise<unknown>>(), listen: vi.fn<(name: string, callback: (event: { payload: unknown }) => void) => Promise<() => void>>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
import { nativeBridge, nativePanelControl, nativeResidencyControl, nativeSettingsControl, nativeStartupCaptureControl } from "./desktop";
import settingsFixture from "../../tests/fixtures/contracts/settings.json";
beforeEach(() => { mocks.invoke.mockReset(); mocks.listen.mockReset(); });
it("uses the separate startup capture commands and rejects malformed payloads", async () => {
  const snapshot = { revision: "2", enabled: true, blocked: false, outcome: "forced_paused", reason: null, error: null };
  mocks.invoke.mockResolvedValue(snapshot);
  expect(await nativeStartupCaptureControl.snapshot()).toEqual(snapshot);
  expect(mocks.invoke).toHaveBeenLastCalledWith("get_startup_capture");
  expect(await nativeStartupCaptureControl.setEnabled(true)).toEqual(snapshot);
  expect(mocks.invoke).toHaveBeenLastCalledWith("set_startup_capture", { request: { enabled: true } });
  mocks.invoke.mockClear();
  await expect(nativeStartupCaptureControl.setEnabled("true" as unknown as boolean)).rejects.toThrow("invalid startup capture request");
  expect(mocks.invoke).not.toHaveBeenCalled();
  for (const invalid of [{ revision: "02" }, { outcome: "unknown" }, { outcome: "blocked" }, { extra: true }]) {
    mocks.invoke.mockResolvedValue({ ...snapshot, ...invalid });
    await expect(nativeStartupCaptureControl.snapshot()).rejects.toThrow("invalid startup capture state");
  }
  const callback = vi.fn();
  mocks.listen.mockResolvedValue(() => {});
  await nativeStartupCaptureControl.subscribe(callback);
  const [event, listener] = mocks.listen.mock.calls[0];
  expect(event).toBe("startup_capture_changed");
  for (const payload of [{ revision: "01" }, { revision: 2 }, { revision: "2", extra: true }]) listener({ payload });
  expect(callback).not.toHaveBeenCalled();
  listener({ payload: { revision: "2" } });
  expect(callback).toHaveBeenCalledTimes(1);
});
it("requests only the selected PNG thumbnail and binds quick-panel requests to the session", async () => {
  const request = { request_id: "png-1", clip_id: "2" };
  const response = { ...request, source_width: 2, source_height: 2, width: 1, height: 1, rgba_base64: "IkRm/w==" };
  mocks.invoke.mockResolvedValue(response);
  expect((await nativeBridge.getPngPreview!("png-1", "2")).rgba).toEqual(new Uint8ClampedArray([34, 68, 102, 255]));
  expect(mocks.invoke).toHaveBeenLastCalledWith("get_clip_png_preview", { request });
  const session = "a".repeat(32);
  const panel = nativePanelControl.history(session);
  await panel.getPngPreview!("png-1", "2");
  expect(mocks.invoke).toHaveBeenLastCalledWith("get_clip_png_preview", { request: { session_id: session, request } });
  for (const bad of [{ clip_id: "3" }, { rgba_base64: "A!AAAA==" }, { width: 321 }]) {
    mocks.invoke.mockResolvedValue({ ...response, ...bad });
    await expect(panel.getPngPreview!("png-1", "2")).rejects.toThrow();
  }
});
it("m2 settings uses separate strict commands and rejects invalid requests before invoke",async()=>{
  mocks.invoke.mockResolvedValue(settingsFixture.snapshot);expect(await nativeSettingsControl.snapshot()).toEqual(settingsFixture.snapshot);expect(mocks.invoke).toHaveBeenLastCalledWith("get_settings_state");
  await nativeSettingsControl.apply(["c:/Private/old/../vault.exe"]);expect(mocks.invoke).toHaveBeenLastCalledWith("apply_excluded_apps",{request:{paths:["C:\\Private\\vault.exe"]}});
  mocks.invoke.mockClear();await expect(nativeSettingsControl.apply(["vault.exe"])).rejects.toThrow();expect(mocks.invoke).not.toHaveBeenCalled();
  mocks.invoke.mockResolvedValue({...settingsFixture.snapshot,capture_enabled:true});await expect(nativeSettingsControl.snapshot()).rejects.toThrow("invalid settings");
});
it("m2 settings revision events reject invalid and extra fields",async()=>{
  const callback=vi.fn();mocks.listen.mockResolvedValue(()=>{});await nativeSettingsControl.subscribe(callback);expect(mocks.listen.mock.calls[0][0]).toBe("settings_changed");
  const emit=mocks.listen.mock.calls[0][1];for(const payload of [{revision:"01"},{revision:"1",enabled:true},{revision:1}])emit({payload});expect(callback).not.toHaveBeenCalled();emit({payload:{revision:"7"}});expect(callback).toHaveBeenCalledWith("7");
});
it("m2 residency uses separate strict commands and retains the old status shape for save retry", async()=>{
  const state={revision:"1",phase:"running",tray_available:true,error:null};mocks.invoke.mockResolvedValue(state);expect(await nativeResidencyControl.snapshot()).toEqual(state);expect(mocks.invoke).toHaveBeenLastCalledWith("get_residency_state");await nativeResidencyControl.exit();expect(mocks.invoke).toHaveBeenLastCalledWith("request_exit");
  mocks.invoke.mockResolvedValue({...state,enabled:true});await expect(nativeResidencyControl.snapshot()).rejects.toThrow("invalid residency");
  mocks.invoke.mockResolvedValue(fixture.status_cases.find(item=>item.id==="st_ok")!.value);await nativeResidencyControl.retrySaving();expect(mocks.invoke).toHaveBeenLastCalledWith("retry_exit_saving");
});
it("m2 residency events reject malformed states before reaching consumers",async()=>{
  const callback=vi.fn();const off=vi.fn();mocks.listen.mockResolvedValue(off);const dispose=await nativeResidencyControl.subscribe(callback);expect(mocks.listen.mock.calls[0][0]).toBe("residency_changed");const listener=mocks.listen.mock.calls[0][1];listener({payload:{revision:"1",phase:"running",tray_available:true,error:null,extra:true}});expect(callback).not.toHaveBeenCalled();listener({payload:{revision:"2",phase:"stopping",tray_available:true,error:null}});expect(callback).toHaveBeenCalledTimes(1);dispose();expect(off).toHaveBeenCalledTimes(1);
});
it("uses the existing status contract and one request argument for changes", async () => {
  mocks.invoke.mockResolvedValue(fixture.status_cases.find(item => item.id === "st_ok")!.value);
  const status = await nativeBridge.status(); expect(status.revision).toBe("3");
  await nativeBridge.setEnabled(false); expect(mocks.invoke).toHaveBeenLastCalledWith("set_capture_enabled", { request: { enabled: false } });
});
it("verifies list correlation instead of displaying an unrelated response", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "wrong", normalized_query: "", summaries: [], next_cursor: null });
  await expect(nativeBridge.list("current", null)).rejects.toThrow();
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "current", query: "", limit: 50, cursor: null, favorites_only: false } });
});
it("rejects unknown status shapes and ignores malformed event revisions", async () => {
  mocks.invoke.mockResolvedValue({ private_data: "never display" }); await expect(nativeBridge.status()).rejects.toThrow("无法读取宿主数据");
  const off = vi.fn(); const callback = vi.fn(); mocks.listen.mockResolvedValue(off); const dispose = await nativeBridge.subscribe("history_changed", callback);
  const listener = mocks.listen.mock.calls[0][1]; listener({ payload: { revision: "01" } }); listener({ payload: { revision: "1", extra: true } }); expect(callback).not.toHaveBeenCalled();listener({ payload: { revision: "1" } });expect(callback).toHaveBeenCalledTimes(1);dispose();expect(off).toHaveBeenCalledTimes(1);
});

function summary(id: string) {
  return { id, captured_at_ms: 1, kind: "text" as const, preview: "p", format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 1, source_process: null, source_confidence: "unknown" as const, favorited: false };
}

it("defaults to an unfiltered empty-query list", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "r", normalized_query: "", summaries: [], next_cursor: null });
  await nativeBridge.list("r", null);
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "r", query: "", limit: 50, cursor: null, favorites_only: false } });
});

it("sends the normalized query literally and accepts its own non-empty echo", async () => {
  const sent = "剪贴  板%_q'\"\\z";
  mocks.invoke.mockResolvedValue({ request_id: "q1", normalized_query: sent, summaries: [], next_cursor: null });
  const result = await nativeBridge.list("q1", null, "\u{3000}\u{2003}剪贴  板%_q'\"\\z\u{a0}\t");
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "q1", query: sent, limit: 50, cursor: null, favorites_only: false } });
  expect(result.request_id).toBe("q1");
  expect(result.normalized_query).toBe(sent);
});

it("keeps the page snapshot upper_id and rejects mismatched correlation", async () => {
  const cursor = { normalized_query: "中", upper_id: "9", last_captured_at_ms: 5, favorites_only: false, last_id: "5" };
  const page = { request_id: "p1", normalized_query: "中", summaries: [], next_cursor: { normalized_query: "中", upper_id: "9", last_captured_at_ms: 4, favorites_only: false, last_id: "4" } };
  mocks.invoke.mockResolvedValue(page);
  await nativeBridge.list("p1", cursor, "中");
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "p1", query: "中", limit: 50, cursor, favorites_only: false } });

  mocks.invoke.mockResolvedValue({ ...page, request_id: "other" });
  await expect(nativeBridge.list("p1", cursor, "中")).rejects.toThrow();
  mocks.invoke.mockResolvedValue({ ...page, normalized_query: "别的" });
  await expect(nativeBridge.list("p1", cursor, "中")).rejects.toThrow();

  mocks.invoke.mockClear();
  await expect(nativeBridge.list("p1", { normalized_query: "别的", upper_id: "9", last_captured_at_ms: 5, favorites_only: false, last_id: "5" }, "中")).rejects.toThrow();
  expect(mocks.invoke).not.toHaveBeenCalled();

  mocks.invoke.mockResolvedValue({ ...page, next_cursor: { normalized_query: "中", upper_id: "8", last_captured_at_ms: 4, favorites_only: false, last_id: "4" } });
  await expect(nativeBridge.list("p1", cursor, "中")).rejects.toThrow();
});

it("rejects more summaries than this request's limit even under the global cap", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "s1", normalized_query: "", summaries: Array.from({ length: 51 }, (_, i) => summary(String(i + 1))), next_cursor: null });
  await expect(nativeBridge.list("s1", null)).rejects.toThrow();
});

it("rejects invalid list inputs before invoke and never echoes the raw query", async () => {
  const long = "密".repeat(257);
  mocks.invoke.mockClear();
  const longError = await nativeBridge.list("q", null, long).then(() => null, (e: unknown) => e as Error);
  expect(longError).toBeInstanceOf(Error);
  expect(String(longError)).not.toContain("密");
  expect(mocks.invoke).not.toHaveBeenCalled();
  await expect(nativeBridge.list("", null)).rejects.toThrow();
  await expect(nativeBridge.list("q\n", null)).rejects.toThrow();
  await expect(nativeBridge.list("q", { normalized_query: "q", upper_id: "0", last_captured_at_ms: 1, favorites_only: false, last_id: "1" })).rejects.toThrow();
  expect(mocks.invoke).not.toHaveBeenCalled();
});

it("invokes get_clip_text with the exact request and UTF-8 byte progress", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "t1", clip_id: "3", text: "中😀", next_offset: 10 });
  const frame = await nativeBridge.getText("t1", "3", 3);
  expect(mocks.invoke).toHaveBeenCalledWith("get_clip_text", { request: { request_id: "t1", clip_id: "3", offset: 3 } });
  expect(frame.text).toBe("中😀");
  expect(frame.next_offset).toBe(10);

  mocks.invoke.mockResolvedValue({ request_id: "t1", clip_id: "3", text: "尾", next_offset: null });
  const end = await nativeBridge.getText("t1", "3", 10);
  expect(end.text).toBe("尾");
  expect(end.next_offset).toBeNull();
});

it("rejects invalid text requests and responses without inventing text", async () => {
  const badRequests: [string, string, number][] = [
    ["t", "0", 0], ["t", "abc", 0], ["t", "1", -1], ["t", "1", 1.5], ["t", "1", 9007199254740992], ["", "1", 0],
  ];
  for (const [rid, cid, offset] of badRequests) {
    mocks.invoke.mockClear();
    await expect(nativeBridge.getText(rid, cid, offset)).rejects.toThrow();
    expect(mocks.invoke).not.toHaveBeenCalled();
  }
  const bad: unknown[] = [
    { request_id: "other", clip_id: "1", text: "a", next_offset: 2 },
    { request_id: "t", clip_id: "2", text: "a", next_offset: 2 },
    { request_id: "t", clip_id: "1", text: "a", next_offset: 99 },
    { request_id: "t", clip_id: "1", text: "x".repeat(65537), next_offset: null },
    { request_id: "t", clip_id: "1", text: "a" },
    { request_id: "t", clip_id: "1", text: "a", next_offset: 2, extra: 1 },
    { request_id: "t", clip_id: "1", text: "\ud800", next_offset: null },
    { request_id: "t", clip_id: "1", text: "", next_offset: 2 },
    "shape",
  ];
  for (const value of bad) {
    mocks.invoke.mockResolvedValue(value);
    await expect(nativeBridge.getText("t", "1", 1)).rejects.toThrow();
  }
  mocks.invoke.mockRejectedValue({ code: "not_found", message: "clip text not found", retryable: false });
  await expect(nativeBridge.getText("t", "1", 0)).rejects.toThrow();
});

it("invokes merged copy only through the panel and rejects invalid or mismatched receipts", async () => {
  const session_id = "a".repeat(32);
  const panel = nativePanelControl.history(session_id);
  expect(nativeBridge.copyMergedText).toBeUndefined();
  const request = { request_id: "merge", clip_ids: ["2", "1"] };
  const result = { ...request, outcome: "copied", reason: null, missing_clip_id: null };
  mocks.invoke.mockResolvedValueOnce({ session_id, result });
  expect(await panel.copyMergedText!("merge", ["2", "1"])).toEqual(result);
  expect(mocks.invoke).toHaveBeenLastCalledWith("copy_manual_panel_merged_text", { request: { session_id, request } });
  for (const bad of [
    { session_id: "b".repeat(32), result },
    { session_id, result: { ...result, clip_ids: ["1", "2"] } },
    { session_id, result: { ...result, request_id: "other" } },
    { session_id, result, extra: true },
  ]) {
    mocks.invoke.mockResolvedValueOnce(bad);
    expect(await panel.copyMergedText!("merge", ["2", "1"])).toMatchObject({ outcome: "unknown", reason: "protocol_error" });
  }
  mocks.invoke.mockRejectedValueOnce(new Error("transport closed"));
  expect(await panel.copyMergedText!("merge", ["2", "1"])).toMatchObject({ outcome: "unknown" });
  await expect(panel.copyMergedText!("merge", ["2", "2"])).rejects.toThrow();
  expect(mocks.invoke).toHaveBeenCalledTimes(6);
});

it("shares the physical panel copy permit between merged, text and image across sessions", async () => {
  const session_id = "a".repeat(32);
  const old = nativePanelControl.history(session_id);
  const fresh = nativePanelControl.history("b".repeat(32));
  let complete!: (reply: unknown) => void;
  mocks.invoke.mockImplementationOnce(() => new Promise(resolve => { complete = resolve; }));
  const write = old.copyMergedText!("merge", ["2", "1"]);
  expect(await fresh.copyText!("text", "1")).toMatchObject({ outcome: "not_written", reason: "busy" });
  expect(await fresh.copyPng!("png", "1")).toMatchObject({ outcome: "not_written", reason: "busy" });
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  complete({ session_id, result: { request_id: "merge", clip_ids: ["2", "1"], outcome: "copied", reason: null, missing_clip_id: null } });
  expect((await write).outcome).toBe("copied");
  mocks.invoke.mockImplementationOnce(() => new Promise(resolve => { complete = resolve; }));
  const image = fresh.copyPng!("png", "1");
  expect(await old.copyMergedText!("merge-2", ["1", "2"])).toMatchObject({ outcome: "not_written", reason: "busy" });
  complete(null);
  expect((await image).outcome).toBe("unknown");
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
});
