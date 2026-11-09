import { useEffect, useRef, useState } from "react";
import type { ManagementControl, ManagementRequest } from "./lib/history-management";
export function HistoryManagement({ control, ready, items, selectedId, onChanged }: { control: ManagementControl; ready: boolean; items: number; selectedId: string | null; onChanged(): void }) {
  const [confirmation, setConfirmation] = useState<ManagementRequest | null>(null);
  const [busy, setBusy] = useState(false), [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const pending = useRef(false);
  const disabled = !ready || busy;
  useEffect(() => { setConfirmation(null); }, [ready, items, selectedId]);
  if (!control.available) return null;
  const validConfirmation = confirmation && (confirmation.action === "delete" ? confirmation.clip_id === selectedId && confirmation.expected_items === items : confirmation.action === "clear" && confirmation.expected_items === items);
  async function run(request: ManagementRequest) {
    if (disabled || pending.current) return;
    pending.current = true; setBusy(true); setMessage(""); setFailed(false); setConfirmation(null);
    try {
      const result = await control.run(request);
      setMessage(request.action === "backup" ? `历史备份已保存：${result.backup_path}` : `已删除 ${result.deleted} 条历史。`);
      if (request.action !== "backup") onChanged();
    } catch { setFailed(true); setMessage("操作未确认完成。请刷新核对历史及备份目录，再决定后续操作。"); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section className="status-card" aria-label="历史管理" aria-busy={busy}>
    <h2>历史管理</h2>
    <p className="notice">上限 10,000 条、正文载荷合计 256 MiB；不会自动删除。管理前请暂停收录，等待未保存队列为零。</p>
    <div className="toolbar">
      <button disabled={disabled || !selectedId} onClick={() => setConfirmation({ action: "delete", clip_id: selectedId!, expected_items: items })}>删除选中条目</button>
      <button disabled={disabled || items === 0} onClick={() => setConfirmation({ action: "clear", expected_items: items })}>清空全部历史</button>
      <button disabled={disabled} onClick={() => void run({ action: "backup" })}>备份历史</button>
    </div>
    {validConfirmation && <div role="group" aria-label="删除确认">
      <p>{confirmation.action === "delete" ? `将永久删除条目 ${confirmation.clip_id}，不影响其他历史。` : `将永久删除全部 ${items} 条历史，不受搜索词限制。`}已有备份不受影响。</p>
      <button disabled={disabled} onClick={() => void run(confirmation)}>确认删除</button>
      <button disabled={busy} onClick={() => setConfirmation(null)}>取消</button>
    </div>}
    {message && <p role={failed ? "alert" : "status"}>{message}</p>}
    <p className="notice">备份仅包含已保存历史和格式数据，位于数据目录的 backups 文件夹；不含排除设置及未保存队列。恢复工具只写入新目录。</p>
  </section>;
}
