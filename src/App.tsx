import { useEffect, useMemo } from "react";
import { useStore } from "zustand";
import { nativeBridge, type DesktopBridge } from "./lib/desktop";
import { createHistoryModel } from "./lib/history-store";
import type { ClipSummary } from "./lib/ipc";

function HistoryItem({ item }: { item: ClipSummary }) {
  const date = new Date(item.captured_at_ms);
  const validTime = Number.isFinite(date.getTime());
  return <li><p className="preview">{item.preview || (item.kind === "png" ? "已保存 PNG 样本" : "已保存非文本内容")}</p><p className="item-meta"><time dateTime={validTime ? date.toISOString() : undefined}>{validTime ? date.toLocaleString() : "时间超出可显示范围"}</time><span>{item.format_names.join(" · ")}</span><span>{Math.ceil(item.payload_bytes / 1024)} KiB</span><span>{item.source_confidence === "unknown" ? "来源未知" : "来源已核对"}</span></p></li>;
}

export default function App({ bridge = nativeBridge }: { bridge?: DesktopBridge }) {
  const model = useMemo(() => createHistoryModel(bridge), [bridge]);
  const view = useStore(model.store);
  useEffect(() => model.start(), [model]);
  const status = view.status;
  const label = !status ? "正在读取状态" : ({ listening: "正在收录", paused: "已暂停", overloaded: "已停止新增：队列或历史容量已满", failed: "存储或抓取异常", stopping: "正在退出", startup: "正在启动" })[status.state];
  const retry = status?.services.storage === "failed" && status.queue.items > 0;
  return (
    <main className="shell history-shell">
      <header><h1>片羽</h1><p className="tagline">剪贴板历史 · 隔离运行</p></header>
      {view.hostMissing ? <p className="notice" role="status">需要桌面宿主。请从片羽桌面程序打开此界面。</p> : <>
        <section className="status-card" aria-label="收录状态">
          <div className="toolbar"><p role="status" aria-live="polite">{label}</p>
            <button disabled={!status || view.changing || (status.state === "stopping" && !retry)} onClick={() => void model.toggle()}>{view.changing ? "正在更改…" : retry ? "重试保存" : status?.capture.user_enabled && status.state === "listening" ? "暂停收录" : "开始收录"}</button>
          </div>
          {status && <><dl className="counts"><div><dt>已存历史</dt><dd>{status.history.items} 条</dd></div><div><dt>未保存队列</dt><dd>{status.queue.items} 条 · {Math.ceil(status.queue.bytes / 1024)} KiB</dd></div><div><dt>已忽略 / 已覆盖</dt><dd>{status.counters.ignored} / {status.counters.superseded}</dd></div></dl><details><summary>隔离数据目录</summary><p className="data-path">{status.data_dir}</p></details></>}
          {status?.state === "failed" && <p className="error" role="alert">{status.queue.items > 0 ? "仍有未保存内容。可重试保存，退出未完成时窗口会保留。" : "服务暂不可用，请检查隔离目录或重启程序。"}</p>}
        </section>
        {view.error && <p className="error" role="alert">{view.error}</p>}
        <section aria-labelledby="history-heading" aria-busy={view.loading}>
          <div className="toolbar"><h2 id="history-heading">最近历史</h2><button disabled={view.loading} onClick={() => void model.refresh()}>刷新</button></div>
          {view.loading && <p role="status">正在读取历史…</p>}
          {!view.loading && !view.error && status?.services.storage === "ready" && view.summaries.length === 0 && <p className="notice">暂无历史。开始收录后，在其他应用中复制内容。</p>}
          <ol className="history-list">{view.summaries.map(item => <HistoryItem key={item.id} item={item} />)}</ol>
          {view.cursor && <button disabled={view.loading} onClick={() => void model.more()}>加载更多</button>}
        </section>
        <p className="notice">列表只显示已提交的历史；未保存队列单独计数。隐私标记、内部写回及应用排除会跳过收录。搜索、快捷面板与回贴尚未开放。</p>
      </>}
    </main>
  );
}
