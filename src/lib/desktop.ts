import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseStatusResult, parseListClipsRequest, parseListClipsResult, parseGetClipTextRequest, parseGetClipTextResult, parseRevisionEvent, type StatusResult, type ListClipsResult, type ListCursor, type GetClipTextResult, type ContractResult } from "./ipc";

export interface DesktopBridge {
  available: boolean;
  status(): Promise<StatusResult>;
  list(requestId: string, cursor: ListCursor | null, query?: string): Promise<ListClipsResult>;
  getText(requestId: string, clipId: string, offset: number): Promise<GetClipTextResult>;
  setEnabled(enabled: boolean): Promise<StatusResult>;
  subscribe(name: "history_changed" | "status_changed", callback: () => void): Promise<() => void>;
}
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
