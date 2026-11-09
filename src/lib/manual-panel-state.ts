import { useEffect, useState } from "react";
import type { ManualPanelControl } from "./desktop";
import type { ManualPanelSnapshot } from "./ipc";

export function useManualPanel(control: ManualPanelControl, enabled = true) {
  const [snapshot, setSnapshot] = useState<ManualPanelSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true; let off: (() => void) | undefined;
    const accept = (next: ManualPanelSnapshot) => { if (active) setSnapshot(old => !old || BigInt(next.revision) >= BigInt(old.revision) ? next : old); };
    if (enabled && control.available) void (async () => { try { const unsubscribe = await control.subscribe(accept); if (!active) { unsubscribe(); return; } off = unsubscribe; accept(await control.snapshot()); } catch { if (active) setError("无法读取快捷面板状态，请重试打开。"); } })();
    return () => { active = false; off?.(); };
  }, [control, enabled]);
  return { snapshot, error, async open() { try { await control.open(); setError(null); } catch { setError("快捷面板打开失败，主窗口仍可使用。"); } } };
}
