import { invoke, isTauri } from "@tauri-apps/api/core";
export type ManagementRequest = { action: "delete"; clip_id: string; expected_items: number } | { action: "clear"; expected_items: number; expected_history_revision: string } | { action: "backup" };
export type ManagementResult = { items: number; favorites: number; deleted: number; backup_path: string | null };
export interface ManagementControl { available: boolean; run(request: ManagementRequest): Promise<ManagementResult> }
export const nativeManagement: ManagementControl = {
  available: isTauri(),
  async run(request) {
    const raw: unknown = await invoke("manage_history", { request });
    if (!raw || typeof raw !== "object" || Object.keys(raw).sort().join(",") !== "backup_path,deleted,favorites,items" || !("items" in raw) || !("favorites" in raw) || !("deleted" in raw) || !("backup_path" in raw) || !Number.isSafeInteger(raw.items) || !Number.isSafeInteger(raw.deleted) || !Number.isSafeInteger(raw.favorites) || Number(raw.favorites) < 0 || Number(raw.favorites) > Number(raw.items) || Number(raw.items) < 0 || Number(raw.deleted) < 0 || (raw.backup_path !== null && typeof raw.backup_path !== "string")) throw new Error("invalid management response");
    return { items: Number(raw.items), favorites: Number(raw.favorites), deleted: Number(raw.deleted), backup_path: raw.backup_path as string | null };
  },
};
