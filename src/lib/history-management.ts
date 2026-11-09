import { invoke, isTauri } from "@tauri-apps/api/core";
export type ManagementRequest = { action: "delete"; clip_id: string; expected_items: number } | { action: "clear"; expected_items: number } | { action: "backup" };
export type ManagementResult = { items: number; deleted: number; backup_path: string | null };
export interface ManagementControl { available: boolean; run(request: ManagementRequest): Promise<ManagementResult> }
export const nativeManagement: ManagementControl = {
  available: isTauri(),
  async run(request) {
    const raw: unknown = await invoke("manage_history", { request });
    if (!raw || typeof raw !== "object" || Object.keys(raw).sort().join(",") !== "backup_path,deleted,items" || !("items" in raw) || !("deleted" in raw) || !("backup_path" in raw) || !Number.isSafeInteger(raw.items) || !Number.isSafeInteger(raw.deleted) || Number(raw.items) < 0 || Number(raw.deleted) < 0 || (raw.backup_path !== null && typeof raw.backup_path !== "string")) throw new Error("invalid management response");
    return { items: Number(raw.items), deleted: Number(raw.deleted), backup_path: raw.backup_path as string | null };
  },
};
