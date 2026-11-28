import { useCallback, useEffect, useRef, useState } from "react";
import type { StartupCaptureControl, StartupCaptureSnapshot } from "./lib/startup-capture";

const readError = "无法读取启动恢复设置。";

const outcomes: Record<StartupCaptureSnapshot["outcome"], string> = {
  default_paused: "",
  forced_paused: "本次启动使用了 --start-paused，已强制暂停。",
  remembered_paused: "本次启动沿用上次的暂停状态。",
  restored: "本次启动已恢复收录。",
  blocked: "",
};

export function StartupCaptureSettings({ control, closing }: { control: StartupCaptureControl; closing: boolean }) {
  const [snapshot, setSnapshot] = useState<StartupCaptureSnapshot | null>(null);
  const [pending, setPending] = useState(false);
  const [retryingRead, setRetryingRead] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [subscriptionFailed, setSubscriptionFailed] = useState(false);
  const connection = useRef(0);
  const accept = useCallback((next: StartupCaptureSnapshot) => {
    setSnapshot(current => current && BigInt(current.revision) > BigInt(next.revision) ? current : next);
  }, []);
  const read = useCallback(async (token: number) => {
    try {
      const next = await control.snapshot();
      if (connection.current === token) {
        accept(next);
        setError(current => current === readError ? null : current);
      }
    } catch {
      if (connection.current === token) setError(readError);
    }
  }, [control, accept]);

  useEffect(() => {
    const token = ++connection.current;
    let off: (() => void) | undefined;
    setSnapshot(null);
    setPending(false);
    setRetryingRead(false);
    setError(null);
    setSubscriptionFailed(false);
    void (async () => {
      try {
        off = await control.subscribe(() => { void read(token); });
        if (connection.current !== token) { off(); return; }
      } catch {
        if (connection.current === token) setSubscriptionFailed(true);
      }
      if (connection.current === token) await read(token);
    })();
    return () => { connection.current++; off?.(); };
  }, [control, read]);

  async function retryRead() {
    if (closing || retryingRead) return;
    const token = connection.current;
    setRetryingRead(true);
    await read(token);
    if (connection.current === token) setRetryingRead(false);
  }

  async function apply(enabled: boolean) {
    if (closing || pending || !snapshot || snapshot.blocked) return;
    const token = connection.current;
    setPending(true);
    setError(null);
    try {
      const next = await control.setEnabled(enabled);
      if (connection.current === token) accept(next);
    } catch {
      try {
        const next = await control.snapshot();
        if (connection.current === token) {
          accept(next);
          if (!next.error) setError("未能确认启动恢复设置是否已保存。");
        }
      } catch {
        if (connection.current === token) setError("保存结果未知，无法读取启动恢复设置。");
      }
    } finally {
      if (connection.current === token) setPending(false);
    }
  }

  const outcome = snapshot ? outcomes[snapshot.outcome] : "";
  const needsReadRetry = !snapshot && error === readError;
  return <section className="status-card" aria-labelledby="startup-capture-heading" aria-busy={pending || retryingRead}>
    <h2 id="startup-capture-heading">启动收录</h2>
    <label>
      <input type="checkbox" checked={snapshot?.enabled ?? false} disabled={closing || pending || !snapshot || snapshot.blocked}
        onChange={event => void apply(event.currentTarget.checked)} />
      启动时恢复上次手动收录状态
    </label>
    {!needsReadRetry && <p className="notice" role="status">
      {pending ? "正在保存启动恢复设置…" : snapshot ? (snapshot.enabled ? "已开启启动恢复。" : "当前目录默认在启动时暂停收录。") : "正在读取启动恢复设置…"}
    </p>}
    {outcome && <p className="notice">{outcome}</p>}
    {snapshot?.reason && <p className="error" role="alert">{snapshot.reason}</p>}
    {snapshot?.error && <p className="error" role="alert">{snapshot.error}</p>}
    {subscriptionFailed && <p className="error" role="alert">无法订阅启动恢复设置的变化。</p>}
    {error && <p className="error" role="alert">{error}</p>}
    {needsReadRetry && <button type="button" disabled={closing || retryingRead} onClick={() => void retryRead()}>
      {retryingRead ? "正在重试读取…" : "重试读取"}
    </button>}
  </section>;
}
