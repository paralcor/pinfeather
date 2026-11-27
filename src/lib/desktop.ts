import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseResidencySnapshot, type ResidencyControl } from "./residency";
import { parseSettingsSnapshot, type SettingsControl } from "./settings";
import { parseStartupCaptureSnapshot, type StartupCaptureControl } from "./startup-capture";
import { parseManualPanelSnapshot, parseManualPanelSessionRequest, parseManualPanelCopyRequest, type ManualPanelSnapshot } from "./ipc";
import { parseCopyClipTextRequest, parseCopyClipTextResult, parsePngPreviewResult, type CopyClipTextResult, type PngPreview } from "./ipc";
import { parseStatusResult, parseListClipsRequest, parseListClipsResult, parseGetClipTextRequest, parseGetClipTextResult, parseRevisionEvent, type StatusResult, type ListClipsResult, type ListCursor, type GetClipTextResult, type ContractResult } from "./ipc";
import { parseSetExcludedAppsRequest } from "./ipc";
import { measurementEnabled, nativeMeasurement, type MeasurementTransport } from "./measurement";

function settingsSnapshot(value: unknown) { const state = parseSettingsSnapshot(value); if (!state) throw new Error("invalid settings state"); return state; }
export const nativeSettingsControl: SettingsControl = {
  available: isTauri(),
  async snapshot() { return settingsSnapshot(await invoke("get_settings_state")); },
  async subscribe(callback) { return listen("settings_changed", event => { const revision = parseRevisionEvent(event.payload, "revision"); if (revision.ok) callback(revision.value); }); },
  async apply(paths) { const request = { paths: checked(parseSetExcludedAppsRequest({ paths })) }; return settingsSnapshot(await invoke("apply_excluded_apps", { request })); },
};

function startupCaptureSnapshot(value: unknown) {
  const snapshot = parseStartupCaptureSnapshot(value);
  if (!snapshot) throw new Error("invalid startup capture state");
  return snapshot;
}
export const nativeStartupCaptureControl: StartupCaptureControl = {
  available: isTauri(),
  async snapshot() { return startupCaptureSnapshot(await invoke("get_startup_capture")); },
  async subscribe(callback) {
    return listen("startup_capture_changed", event => {
      if (parseRevisionEvent(event.payload, "revision").ok) callback();
    });
  },
  async setEnabled(enabled) {
    if (typeof enabled !== "boolean") throw new Error("invalid startup capture request");
    return startupCaptureSnapshot(await invoke("set_startup_capture", { request: { enabled } }));
  },
};

export interface DesktopBridge {
  available: boolean;
  measurement?: MeasurementTransport;
  status(): Promise<StatusResult>;
  list(requestId: string, cursor: ListCursor | null, query?: string): Promise<ListClipsResult>;
  getText(requestId: string, clipId: string, offset: number): Promise<GetClipTextResult>;
  getPngPreview?(requestId: string, clipId: string): Promise<PngPreview>;
  copyText?(requestId: string, clipId: string): Promise<CopyClipTextResult>;
  copyPng?(requestId: string, clipId: string): Promise<CopyClipTextResult>;
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
  hide(session: string, afterCopy?: true): Promise<ManualPanelSnapshot>;
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
  async hide(session, afterCopy) {
    const request = checked(parseManualPanelSessionRequest({ session_id: session }));
    return panelSnapshot(await invoke("hide_manual_panel", afterCopy ? { request, afterCopy } : { request }));
  },
  history(session) {
    return {
      ...nativeBridge,
      getPngPreview: (requestId, clipId) => getPngPreview(requestId, clipId, session),
      copyText: (requestId, clipId) => copyFromPanel("copy_manual_panel_text", session, requestId, clipId),
      copyPng: (requestId, clipId) => copyFromPanel("copy_manual_panel_png", session, requestId, clipId),
    };
  },
};
function checked<T>(result: ContractResult<T>): T {
  if (!result.ok) throw new Error("无法读取宿主数据，请重试。");
  return result.value;
}
async function copyFromPanel(
  command: "copy_manual_panel_text" | "copy_manual_panel_png", session: string, requestId: string, clipId: string,
): Promise<CopyClipTextResult> {
  const request = checked(parseCopyClipTextRequest({ request_id: requestId, clip_id: clipId }));
  const envelope = checked(parseManualPanelCopyRequest({ session_id: session, request }));
  if (manualPanelCopyPending) return { ...request, outcome: "not_written", reason: "busy" };
  manualPanelCopyPending = true;
  try {
    const raw: unknown = await invoke(command, { request: envelope });
    if (typeof raw === "object" && raw !== null && "session_id" in raw && raw.session_id === session && "result" in raw && Object.keys(raw).sort().join(",") === "result,session_id") {
      const parsed = parseCopyClipTextResult(raw.result, request);
      if (parsed.ok) return parsed.value;
    }
  } catch { /* An accepted write keeps the permit after the old window session ends. */ }
  finally { manualPanelCopyPending = false; }
  return { ...request, outcome: "unknown", reason: "protocol_error" };
}

async function copyFromMain(
  command: "copy_clip_text" | "copy_clip_png", requestId: string, clipId: string,
): Promise<CopyClipTextResult> {
  const parsed = parseCopyClipTextRequest({ request_id: requestId, clip_id: clipId });
  if (!parsed.ok) throw new Error("invalid copy request");
  const request = parsed.value;
  try {
    const result = parseCopyClipTextResult(await invoke(command, { request }), request);
    if (result.ok) return result.value;
  } catch { /* A rejected response cannot prove that dispatch had no effects. */ }
  return { ...request, outcome: "unknown", reason: "protocol_error" };
}
async function getPngPreview(requestId: string, clipId: string, session?: string): Promise<PngPreview> {
  const request = checked(parseCopyClipTextRequest({ request_id: requestId, clip_id: clipId }));
  const payload = session === undefined ? request : checked(parseManualPanelCopyRequest({ session_id: session, request }));
  return checked(parsePngPreviewResult(await invoke("get_clip_png_preview", { request: payload }), request));
}
export const nativeBridge: DesktopBridge = {
  available: isTauri(),
  get measurement() { return isTauri() && measurementEnabled() ? nativeMeasurement : undefined; },
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
  getPngPreview,
  async setEnabled(enabled) { return checked(parseStatusResult(await invoke("set_capture_enabled", { request: { enabled } }))); },
  copyText: (requestId, clipId) => copyFromMain("copy_clip_text", requestId, clipId),
  copyPng: (requestId, clipId) => copyFromMain("copy_clip_png", requestId, clipId),
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
