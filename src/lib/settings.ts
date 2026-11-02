import { useEffect, useRef, useState } from "react";
import { parseSetExcludedAppsRequest } from "./ipc";

export interface SettingsSnapshot { revision: string; schema_version: 1; excluded_apps: string[]; saving: boolean; blocked: boolean; error: string | null; last_skip: "excluded_owner" | "unknown_owner" | null }
export interface SettingsControl {
  available: boolean;
  snapshot(): Promise<SettingsSnapshot>;
  subscribe(callback: (revision: string) => void): Promise<() => void>;
  apply(paths: string[]): Promise<SettingsSnapshot>;
}
export function parseSettingsSnapshot(value: unknown): SettingsSnapshot | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const v = value as Record<string, unknown>;
  if (Object.keys(v).sort().join(",") !== "blocked,error,excluded_apps,last_skip,revision,saving,schema_version") return null;
  if (typeof v.revision !== "string" || !/^(0|[1-9]\d{0,19})$/.test(v.revision) || BigInt(v.revision) > 18446744073709551615n || v.schema_version !== 1 || typeof v.saving !== "boolean" || typeof v.blocked !== "boolean" || (v.error !== null && typeof v.error !== "string") || ![null, "unknown_owner", "excluded_owner"].includes(v.last_skip as string | null)) return null;
  const paths = parseSetExcludedAppsRequest({ paths: v.excluded_apps });
  if (!paths.ok || paths.value.some((path, i) => path !== (v.excluded_apps as string[])[i])) return null;
  if (v.blocked && v.error === null) return null;
  return v as unknown as SettingsSnapshot;
}
export function acceptSettings(current: SettingsSnapshot | null, next: SettingsSnapshot): SettingsSnapshot {
  return current && BigInt(next.revision) <= BigInt(current.revision) ? current : next;
}
export function useSettings(control: SettingsControl, enabled: boolean) {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const latest = useRef<SettingsSnapshot | null>(null), active = useRef(0), busy = useRef(false);
  function accept(next: SettingsSnapshot) { latest.current = acceptSettings(latest.current, next); setSnapshot(latest.current); }
  useEffect(() => {
    const token = ++active.current; let off: (() => void) | undefined, reading = false, again = false;
    latest.current = null; setSnapshot(null); setError(null);
    async function refresh() {
      if (reading) { again = true; return; }
      reading = true;
      try {
        do { again = false; const next = await control.snapshot(); if (active.current !== token) return; accept(next); } while (again);
      } catch { if (active.current === token) setError("当前宿主不支持持久排除设置，或设置状态读取失败。请保留窗口并检查宿主版本。"); }
      finally { reading = false; }
    }
    if (enabled && control.available) void (async () => {
      try {
        off = await control.subscribe(revision => { if (active.current === token && (!latest.current || BigInt(revision) > BigInt(latest.current.revision))) void refresh(); });
        if (active.current !== token) { off(); return; }
        await refresh();
      } catch { if (active.current === token) setError("当前宿主不支持持久排除设置，或设置状态读取失败。请保留窗口并检查宿主版本。"); }
    })();
    return () => { if (active.current === token) active.current++; off?.(); };
  }, [control, enabled]);
  async function apply(paths: string[]) {
    if (!enabled || !control.available || !latest.current || latest.current.blocked || latest.current.saving || busy.current) return false;
    const parsed = parseSetExcludedAppsRequest({ paths });
    if (!parsed.ok) { setError("请填写规范的 Windows 完整 exe 路径，最多 32 条。"); return false; }
    const token = active.current; busy.current = true; setPending(true); setError(null);
    try {
      const next = await control.apply(parsed.value);
      if (active.current !== token) return false;
      accept(next);
      const confirmed = latest.current;
      const applied = confirmed !== null && !confirmed.blocked && confirmed.excluded_apps.length === next.excluded_apps.length && confirmed.excluded_apps.every((path, index) => path === next.excluded_apps[index]);
      if (!applied) setError("设置状态已更新，请核对当前规则后重新应用。");
      return applied;
    } catch (cause) {
      if (active.current === token) {
        const code = typeof cause === "object" && cause !== null && "code" in cause ? cause.code : null;
        setError(code === "invalid_input" ? "路径无效，设置未应用。请检查完整 exe 路径和数量。" : "设置未确认生效，请核对当前列表与错误后重试。");
        try { const next = await control.snapshot(); if (active.current === token) accept(next); } catch { /* Keep last confirmed state and the failure visible. */ }
      }
      return false;
    } finally { busy.current = false; setPending(false); }
  }
  return { snapshot, error, pending, apply };
}
