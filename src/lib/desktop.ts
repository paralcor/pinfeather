import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseResidencySnapshot, type ResidencyControl } from "./residency";
import { parseSettingsSnapshot, type SettingsControl } from "./settings";
import { parseManualPanelSnapshot, parseManualPanelSessionRequest, parseManualPanelCopyRequest, type ManualPanelSnapshot } from "./ipc";
import { parseCopyClipTextRequest, parseCopyClipTextResult, type CopyClipTextResult } from "./ipc";
import { parseStatusResult, parseListClipsRequest, parseListClipsResult, parseGetClipTextRequest, parseGetClipTextResult, parseRevisionEvent, type StatusResult, type ListClipsResult, type ListCursor, type GetClipTextResult, type ContractResult } from "./ipc";
import { parseSetExcludedAppsRequest } from "./ipc";

function settingsSnapshot(value: unknown) { const state = parseSettingsSnapshot(value); if (!state) throw new Error("invalid settings state"); return state; }
export const nativeSettingsControl: SettingsControl = {
  available: isTauri(),
  async snapshot() { return settingsSnapshot(await invoke("get_settings_state")); },
  async subscribe(callback) { return listen("settings_changed", event => { const revision = parseRevisionEvent(event.payload, "revision"); if (revision.ok) callback(revision.value); }); },
  async apply(paths) { const request = { paths: checked(parseSetExcludedAppsRequest({ paths })) }; return settingsSnapshot(await invoke("apply_excluded_apps", { request })); },
};

export interface DesktopBridge {
  available: boolean;
  status(): Promise<StatusResult>;
  list(requestId: string, cursor: ListCursor | null, query?: string): Promise<ListClipsResult>;
  getText(requestId: string, clipId: string, offset: number): Promise<GetClipTextResult>;
  copyText?(requestId: string, clipId: string): Promise<CopyClipTextResult>;
  setEnabled(enabled: boolean): Promise<StatusResult>;
  subscribe(name: "history_changed" | "status_changed", callback: () => void): Promise<() => void>;
}
function residencySnapshot(value: unknown) { const state = parseResidencySnapshot(value); if (!state) throw new Error("invalid residency state"); return state; }
export const nativeResidencyControl: ResidencyControl = {
  available: isTauri(),
  async snapshot() { return residencySnapshot(await invoke("get_residency_state")); },
  async subscribe(callback) { return listen("residency_changed", event => { const state = parseResidencySnapshot(event.payload); if (state) callback(state); }); },
  async exit() { return residencySnapshot(await invoke("request_exit")); },
  async retrySaving() { checked(parseStatusResult(await invoke("retry_exit_saving"))); },
};
export interface ManualPanelControl {
  available: boolean;
  snapshot(): Promise<ManualPanelSnapshot>;
  subscribe(callback: (snapshot: ManualPanelSnapshot) => void): Promise<() => void>;
  open(): Promise<ManualPanelSnapshot>;
  ready(session: string): Promise<ManualPanelSnapshot>;
  hide(session: string): Promise<ManualPanelSnapshot>;
  history(session: string): DesktopBridge;
}
function panelSnapshot(value: unknown) { const result = parseManualPanelSnapshot(value); if (!result) throw new Error("invalid panel state"); return result; }
// This WebView can replace its model on reopen while the old IPC is still unsettled.
let manualPanelCopyPending = false;
export const nativePanelControl: ManualPanelControl = {
  available: isTauri(),
  async snapshot() { return panelSnapshot(await invoke("get_manual_panel_state")); },
  async subscribe(callback) { return listen("manual_panel_changed", event => { const result = parseManualPanelSnapshot(event.payload); if (result) callback(result); }); },
  async open() { return panelSnapshot(await invoke("open_manual_panel")); },
  async ready(session) { return panelSnapshot(await invoke("ready_manual_panel", { request: checked(parseManualPanelSessionRequest({ session_id: session })) })); },
  async hide(session) { return panelSnapshot(await invoke("hide_manual_panel", { request: checked(parseManualPanelSessionRequest({ session_id: session })) })); },
  history(session) { return { ...nativeBridge, async copyText(requestId, clipId) {
    const request = checked(parseCopyClipTextRequest({ request_id: requestId, clip_id: clipId }));
    const envelope = checked(parseManualPanelCopyRequest({ session_id: session, request }));
    if (manualPanelCopyPending) return { ...request, outcome: "not_written", reason: "busy" };
    manualPanelCopyPending = true;
    try {
      const raw: unknown = await invoke("copy_manual_panel_text", { request: envelope });
      if (typeof raw === "object" && raw !== null && "session_id" in raw && raw.session_id === session && "result" in raw && Object.keys(raw).sort().join(",") === "result,session_id") {
        const parsed = parseCopyClipTextResult(raw.result, request); if (parsed.ok) return parsed.value;
      }
    } catch { /* An accepted write is not cancelled by window/session teardown. */ }
    finally { manualPanelCopyPending = false; }
    return { ...request, outcome: "unknown", reason: "protocol_error" };
  } }; },
};
function checked<T>(result: ContractResult<T>): T {
  if (!result.ok) throw new Error("无法读取宿主数据，请重试。");
  return result.value;
}
export const nativeBridge: DesktopBridge = {
  available: isTauri(),
  async status() { return checked(parseStatusResult(await invoke("get_status"))); },
  async list(requestId, cursor, query = "") {
    const request = checked(parseListClipsRequest({ request_id: requestId, query, limit: 50, cursor }));
    const result = checked(parseListClipsResult(await invoke("list_clips", { request })));
    if (result.request_id !== request.request_id || result.normalized_query !== request.query) throw new Error("无法读取历史，请重试。");
    if (result.summaries.length > request.limit) throw new Error("无法读取历史，请重试。");
    if (request.cursor && result.next_cursor && result.next_cursor.upper_id !== request.cursor.upper_id) throw new Error("无法读取历史，请重试。");
    return result;
  },
  async getText(requestId, clipId, offset) {
    const request = checked(parseGetClipTextRequest({ request_id: requestId, clip_id: clipId, offset }));
    return checked(parseGetClipTextResult(await invoke("get_clip_text", { request }), request));
  },
  async setEnabled(enabled) { return checked(parseStatusResult(await invoke("set_capture_enabled", { request: { enabled } }))); },
  async copyText(requestId, clipId) {
    const parsed = parseCopyClipTextRequest({ request_id: requestId, clip_id: clipId });
    if (!parsed.ok) throw new Error("invalid copy request");
    const request = parsed.value;
    try {
      const result = parseCopyClipTextResult(await invoke("copy_clip_text", { request }), request);
      if (result.ok) return result.value;
    } catch { /* A rejected response cannot prove that dispatch had no effects. */ }
    return { ...request, outcome: "unknown", reason: "protocol_error" };
  },
  async subscribe(name, callback) { return listen(name, event => { if (parseRevisionEvent(event.payload, "revision").ok) callback(); }); },
};

export function readableError(error: unknown): string {
  const code = typeof error === "object" && error !== null && "code" in error ? error.code : null;
  if (code === "capacity_exceeded") return "历史容量已满，已停止新增收录。";
  if (code === "storage_unavailable") return "存储暂不可用，请检查隔离数据目录。未保存内容仍保留在队列中。";
  if (code === "busy") return "当前操作暂不可用，请稍后重试。";
  if (code === "cleanup_failed") return "退出或资源回收未完成，请保留窗口并重试。";
  return "无法完成操作，请重试。";
}
