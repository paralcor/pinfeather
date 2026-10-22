import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ShellStatus {
  data_dir: string;
  start_paused: boolean;
}

export default function App() {
  const [status, setStatus] = useState<ShellStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<ShellStatus>("get_shell_status")
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main className="shell">
      <h1>片羽</h1>
      <p className="tagline">剪贴板历史工具 · 最小工程壳</p>
      {error && <p className="error">无法读取宿主状态：{error}</p>}
      {status && (
        <dl className="status-card">
          <div className="row">
            <dt>隔离数据目录</dt>
            <dd>{status.data_dir}</dd>
          </div>
          <div className="row">
            <dt>收录</dt>
            <dd>
              <span className="badge">
                {status.start_paused ? "已暂停（启动参数 --start-paused）" : "未接入"}
              </span>
            </dd>
          </div>
        </dl>
      )}
      <p className="notice">
        系统功能尚未实现：剪贴板监听、存储、搜索、热键与回贴均未接入本壳。
      </p>
    </main>
  );
}
