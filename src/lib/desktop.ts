import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseStatusResult, parseListClipsResult, parseRevisionEvent, type StatusResult, type ListClipsResult, type ListCursor, type ContractResult } from "./ipc";

export interface DesktopBridge {
  available: boolean;
  status(): Promise<StatusResult>;
  list(requestId: string, cursor: ListCursor | null): Promise<ListClipsResult>;
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
  async list(requestId, cursor) {
    const result = checked(parseListClipsResult(await invoke("list_clips", { request: { request_id: requestId, query: "", limit: 50, cursor } })));
    if (result.request_id !== requestId || result.normalized_query !== "") throw new Error("无法读取历史，请重试。");
    return result;
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
