import { parseRevisionEvent } from "./ipc";

const outcomes = ["default_paused", "forced_paused", "remembered_paused", "restored", "blocked"] as const;

export interface StartupCaptureSnapshot {
  revision: string;
  enabled: boolean;
  blocked: boolean;
  outcome: typeof outcomes[number];
  reason: string | null;
  error: string | null;
}

export interface StartupCaptureControl {
  available: boolean;
  snapshot(): Promise<StartupCaptureSnapshot>;
  subscribe(callback: () => void): Promise<() => void>;
  setEnabled(enabled: boolean): Promise<StartupCaptureSnapshot>;
}

export function parseStartupCaptureSnapshot(value: unknown): StartupCaptureSnapshot | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const v = value as Record<string, unknown>;
  if (Object.keys(v).sort().join(",") !== "blocked,enabled,error,outcome,reason,revision") return null;
  if (!parseRevisionEvent({ revision: v.revision }, "revision").ok ||
      typeof v.enabled !== "boolean" || typeof v.blocked !== "boolean" ||
      !outcomes.includes(v.outcome as StartupCaptureSnapshot["outcome"]) ||
      (v.reason !== null && typeof v.reason !== "string") ||
      (v.error !== null && typeof v.error !== "string")) return null;
  if ((v.outcome === "blocked" && v.reason === null && v.error === null) || (v.blocked && v.error === null)) return null;
  return v as unknown as StartupCaptureSnapshot;
}
