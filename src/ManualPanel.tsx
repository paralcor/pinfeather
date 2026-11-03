import { useEffect, useMemo, useState } from "react";
import App from "./App";
import { nativePanelControl, type ManualPanelControl } from "./lib/desktop";
import { useManualPanel } from "./lib/manual-panel-state";
function Session({ session, control }: { session: string; control: ManualPanelControl }) {
  const bridge = useMemo(() => control.history(session), [control, session]);
  const [error, setError] = useState<string | null>(null);
  const [presented, setPresented] = useState<{ session: string; control: ManualPanelControl } | null>(null);
  useEffect(() => {
    let active = true;
    setError(null);
    void control.ready(session).then(snapshot => {
      if (!active) return;
      if (snapshot.session_id !== session || snapshot.phase !== "ready") {
        setError("面板会话未就绪，请从主窗口重新打开。"); return;
      }
      setPresented({ session, control });
    }).catch(() => { if (active) { setError("面板会话未就绪，请从主窗口重新打开。"); } });
    return () => { active = false; };
  }, [control, session]);
  // App focuses its editor on mount, after the host has shown and focused this session.
  if (presented?.session !== session || presented.control !== control) {
    return error ? <p role="alert">{error}</p> : <p role="status">正在打开快捷面板…</p>;
  }
  return <>{error && <p role="alert">{error}</p>}<App bridge={bridge} panel={{ hide: () => { void control.hide(session).catch(() => setError("面板隐藏未完成，请重试。")); } }} /></>;
}
export default function ManualPanel({ control = nativePanelControl }: { control?: ManualPanelControl }) {
  const { snapshot, error } = useManualPanel(control);
  if (error) return <p role="alert">{error}</p>;
  return snapshot?.session_id ? <Session key={snapshot.session_id} session={snapshot.session_id} control={control} /> : <p role="status">快捷面板已隐藏。</p>;
}
