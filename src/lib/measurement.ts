import { invoke } from "@tauri-apps/api/core";
import { parseListClipsRequest, parseListClipsResult, type ListCursor, type ListClipsResult } from "./ipc";

export type Identity = { schema: 1; run_id: string; attempt: string; window: string; session: string | null; model: string };
export type Receipt = Identity & { generation: string; sequence: string; receipt: string; content: "empty" | "nonempty" };
const decimal = (x: unknown) => typeof x === "string" && /^(0|[1-9][0-9]{0,19})$/.test(x) && BigInt(x) <= 18446744073709551615n;
const positive = (x: unknown) => decimal(x) && x !== "0";
function object(x: unknown): x is Record<string, unknown> { return typeof x === "object" && x !== null && !Array.isArray(x); }
function keys(x: Record<string, unknown>, names: string) { return Object.keys(x).sort().join(",") === names.split(",").sort().join(","); }
function identity(x: Record<string, unknown>): boolean { return x.schema === 1 && typeof x.run_id === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(x.run_id) && [x.attempt, x.window, x.model].every(positive) && (x.session === null || (typeof x.session === "string" && /^[0-9a-f]{32}$/.test(x.session))); }
export function parseIdentity(x: unknown): Identity {
  if (!object(x) || !keys(x, "schema,run_id,attempt,window,session,model") || !identity(x)) throw new Error("invalid measurement identity");
  return x as Identity;
}
export function parseReceipt(x: unknown): Receipt {
  if (!object(x) || !keys(x, "schema,run_id,attempt,window,session,model,generation,sequence,receipt,content") || !identity(x) || !decimal(x.generation) || !positive(x.sequence) || !positive(x.receipt) || !["empty", "nonempty"].includes(x.content as string)) throw new Error("invalid measurement receipt");
  return x as Receipt;
}
export function measurementEnabled(): boolean {
  const value = (globalThis as Record<string, unknown>).__PINFEATHER_MEASUREMENT__;
  return object(value) && keys(value, "schema,enabled") && value.schema === 1 && value.enabled === true;
}
export interface MeasurementTransport {
  start(): Promise<Identity>;
  invalidate(identity: Identity, generation: string, sequence: string): Promise<void>;
  list(identity: Identity, generation: string, sequence: string, requestId: string, cursor: ListCursor | null, query: string): Promise<{ result: ListClipsResult; receipt: Receipt | null }>;
  stop(identity: Identity): Promise<void>;
  ack(receipt: Receipt, inputFocused: boolean): Promise<void>;
}
export const nativeMeasurement: MeasurementTransport = {
  async start() { return parseIdentity(await invoke("measure_start_model", { request: { schema: 1 } })); },
  async invalidate(identity, generation, sequence) { await invoke("measure_invalidate", { request: { ...identity, generation, sequence } }); },
  async list(identity, generation, sequence, requestId, cursor, query) {
    const request = parseListClipsRequest({ request_id: requestId, query, limit: 50, cursor });
    if (!request.ok) throw new Error("invalid measurement list request");
    const value = await invoke<unknown>("measure_list_clips", { request: { ...identity, generation, sequence, query_request: request.value } });
    if (!object(value) || !keys(value, "result,receipt")) throw new Error("invalid measurement page");
    const parsed = parseListClipsResult(value.result);
    if (!parsed.ok || parsed.value.request_id !== requestId || parsed.value.normalized_query !== request.value.query || parsed.value.summaries.length > request.value.limit || (cursor && parsed.value.next_cursor && cursor.upper_id !== parsed.value.next_cursor.upper_id)) throw new Error("invalid measurement page");
    const receipt = value.receipt === null ? null : parseReceipt(value.receipt);
    if (receipt && (Object.entries(identity).some(([k, v]) => receipt[k as keyof Receipt] !== v) || receipt.generation !== generation || receipt.sequence !== sequence || receipt.content !== (parsed.value.summaries.length ? "nonempty" : "empty") || cursor !== null)) throw new Error("invalid measurement correlation");
    return { result: parsed.value, receipt };
  },
  async stop(identity) { await invoke("measure_stop_model", { request: identity }); },
  async ack(receipt, inputFocused) { await invoke("measure_ready", { request: { ...receipt, input_focused: inputFocused } }); },
};

/** The commit observer and the final synchronous model check run in one JS turn. */
export function interactiveInput(editor: HTMLInputElement | null, list: HTMLOListElement | null, panel: boolean, content: "empty" | "nonempty", allowed: boolean): boolean {
  if (!allowed || !editor?.isConnected || editor.disabled || editor.readOnly || document.visibilityState !== "visible" || !document.hasFocus()) return false;
  const rect = editor.getBoundingClientRect(), style = getComputedStyle(editor);
  if (rect.width <= 0 || rect.height <= 0 || style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return false;
  if (panel && document.activeElement !== editor) return false;
  return content === "empty" || !!list?.querySelector("button:not(:disabled)");
}
