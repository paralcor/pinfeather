import { useEffect, useMemo, useState } from "react";
import App from "./App";
import { nativePanelControl, type ManualPanelControl } from "./lib/desktop";
import { useManualPanel } from "./lib/manual-panel-state";
function Session({ session, control }: { session: string; control: ManualPanelControl }) {
  const bridge = useMemo(() => control.history(session), [control, session]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { let active = true; void control.ready(session).catch(() => { if (active) setError("面板会话未就绪，请从主窗口重新打开。"); }); return () => { active = false; }; }, [control, session]);
  return <>{error && <p role="alert">{error}</p>}<App bridge={bridge} panel={{ hide: () => { void control.hide(session).catch(() => setError("面板隐藏未完成，请重试。")); } }} /></>;
}
export default function ManualPanel({ control = nativePanelControl }: { control?: ManualPanelControl }) {
  const { snapshot, error } = useManualPanel(control);
  if (error) return <p role="alert">{error}</p>;
  return snapshot?.session_id ? <Session key={snapshot.session_id} session={snapshot.session_id} control={control} /> : <p role="status">快捷面板已隐藏。</p>;
}
