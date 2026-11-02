import { useEffect, useRef, useState } from "react";

export interface ResidencySnapshot { revision: string; phase: "running" | "stopping" | "failed" | "exited"; tray_available: boolean; error: string | null }
export interface ResidencyControl {
  available: boolean;
  snapshot(): Promise<ResidencySnapshot>;
  subscribe(callback: (state: ResidencySnapshot) => void): Promise<() => void>;
  exit(): Promise<ResidencySnapshot>;
  retrySaving(): Promise<void>;
}
export function parseResidencySnapshot(value: unknown): ResidencySnapshot | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const s = value as Record<string, unknown>;
  if (Object.keys(s).sort().join(",") !== "error,phase,revision,tray_available") return null;
  if (typeof s.revision !== "string" || !/^(0|[1-9]\d{0,19})$/.test(s.revision) || BigInt(s.revision) > 18446744073709551615n) return null;
  if (typeof s.phase !== "string" || !["running", "stopping", "failed", "exited"].includes(s.phase) || typeof s.tray_available !== "boolean" || (s.error !== null && typeof s.error !== "string")) return null;
  return s as unknown as ResidencySnapshot;
}
export function acceptResidency(current: ResidencySnapshot | null, next: ResidencySnapshot): ResidencySnapshot {
  if (current && (BigInt(next.revision) <= BigInt(current.revision) || (current.phase !== "running" && next.phase === "running"))) return current;
  return next;
}
export function useResidency(control: ResidencyControl, enabled: boolean) {
  const [snapshot, setSnapshot] = useState<ResidencySnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const active = useRef(0), busy = useRef(false);
  useEffect(() => {
    const token = ++active.current; let off: (() => void) | undefined;
    setSnapshot(null); setError(null);
    if (enabled && control.available) void (async () => {
      try {
        off = await control.subscribe(next => { if (active.current === token) setSnapshot(old => acceptResidency(old, next)); });
        if (active.current !== token) { off(); return; }
        const next = await control.snapshot();
        if (active.current === token) setSnapshot(old => acceptResidency(old, next));
      } catch { if (active.current === token) setError("当前宿主不支持驻留与退出控制，或状态读取失败。请保留主窗口。"); }
    })();
    return () => { if (active.current === token) active.current++; off?.(); };
  }, [control, enabled]);
  async function action(save: boolean) {
    if (!enabled || !control.available || !snapshot || busy.current) return;
    const token = active.current; busy.current = true; setPending(true); setError(null);
    try {
      if (save) await control.retrySaving();
      else { const next = await control.exit(); if (active.current === token) setSnapshot(old => acceptResidency(old, next)); }
    } catch { if (active.current === token) setError("退出或保存未完成，请保留主窗口并重试。"); }
    finally { busy.current = false; if (active.current === token) setPending(false); }
  }
  return { snapshot, error, pending, exit: () => action(false), retrySaving: () => action(true) };
}
