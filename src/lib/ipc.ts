/**
 * E02 共享契约 TS 镜像（未来宿主调用入口）。
 * Rust 权威见 src-tauri/src/contracts.rs；两侧读取 tests/fixtures/contracts/ 同一合成夹具。
 * 仅定义形状与运行时校验，不注册命令、不返回假历史。
 */

export const MAX_REQUEST_ID_BYTES = 64;
export const MAX_QUERY_CHARS = 256;
export const MIN_PAGE_LIMIT = 1;
export const MAX_PAGE_LIMIT = 100;
export const DEFAULT_PAGE_LIMIT = 50;
export const MAX_PREVIEW_SCALARS = 200;
export const MAX_SUMMARY_FORMAT_NAMES = 3;
export const MAX_EXCLUDED_APPS = 32;
export const MAX_EXCLUDED_APP_CHARS = 1024;
export const MAX_TEXT_FORMAT_BYTES = 2 * 1024 * 1024;
export const MAX_PNG_BYTES = 8 * 1024 * 1024;
export const MAX_PAYLOAD_TOTAL_BYTES = 12 * 1024 * 1024;
export const MAX_QUEUE_ITEMS = 64;
export const MAX_QUEUE_BYTES = 32 * 1024 * 1024;
export const MAX_HISTORY_ITEMS = 10000;
export const MAX_HISTORY_PAYLOAD_BYTES = 256 * 1024 * 1024;
export const MAX_TEXT_FRAME_BYTES = 64 * 1024;
export const MAX_PAGE_JSON_BYTES = 1024 * 1024;
export const WORKER_REQUEST_MAX_BYTES = 4 * 1024;
export const WORKER_RESPONSE_MAX_BYTES = 18 * 1024 * 1024;
export const WORKER_PROTOCOL_VERSION = 1;
export const JS_SAFE_INTEGER_MAX = 9007199254740991;

export type ErrorCode =
  | "invalid_input"
  | "not_found"
  | "busy"
  | "capacity_exceeded"
  | "storage_unavailable"
  | "shortcut_unavailable"
  | "target_changed"
  | "permission_unknown"
  | "unsupported_input_state"
  | "clipboard_changed"
  | "injection_failed"
  | "timeout"
  | "protocol_error"
  | "cleanup_failed";

export const ERROR_CODES: readonly ErrorCode[] = [
  "invalid_input",
  "not_found",
  "busy",
  "capacity_exceeded",
  "storage_unavailable",
  "shortcut_unavailable",
  "target_changed",
  "permission_unknown",
  "unsupported_input_state",
  "clipboard_changed",
  "injection_failed",
  "timeout",
  "protocol_error",
  "cleanup_failed",
];

const RETRYABLE: ReadonlySet<ErrorCode> = new Set(["busy", "timeout", "storage_unavailable"]);

export interface ContractError {
  code: ErrorCode;
  message: string;
  retryable: boolean;
}

export type ContractResult<T> = { ok: true; value: T } | { ok: false; error: ContractError };

function fail(code: ErrorCode, message: string): ContractError {
  return { code, message, retryable: RETRYABLE.has(code) };
}

function invalid(message: string): ContractError {
  return fail("invalid_input", message);
}

function protocol(message: string): ContractError {
  return fail("protocol_error", message);
}

function ok<T>(value: T): ContractResult<T> {
  return { ok: true, value };
}

function err<T>(error: ContractError): ContractResult<T> {
  return { ok: false, error };
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function rejectUnknownFields(obj: Record<string, unknown>, allowed: readonly string[]): ContractError | null {
  const allow = new Set(allowed);
  for (const key of Object.keys(obj)) {
    if (!allow.has(key)) return invalid("unknown field");
  }
  return null;
}

/** 非空 ASCII 标识，最长 64 字节。 */
export function validateRequestId(value: unknown): ContractResult<string> {
  if (typeof value !== "string") return err(invalid("request_id must be a string"));
  if (value.length === 0) return err(invalid("request_id must not be empty"));
  if (byteLength(value) > MAX_REQUEST_ID_BYTES) return err(invalid("request_id exceeds max length"));
  for (let i = 0; i < value.length; i += 1) {
    const c = value.charCodeAt(i);
    if (c < 0x21 || c > 0x7e) return err(invalid("request_id must be ASCII identifier"));
  }
  return ok(value);
}

function byteLength(s: string): number {
  let n = 0;
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    if (c < 0x80) n += 1;
    else if (c < 0x800) n += 2;
    else if (c >= 0xd800 && c <= 0xdbff) {
      n += 4;
      i += 1;
    } else n += 3;
  }
  return n;
}

/** 规范正整数十进制字符串：1..=9223372036854775807。不得经 Number 转换再判范围。 */
export function parsePositiveId(value: unknown): ContractResult<string> {
  if (typeof value !== "string") return err(invalid("id must be a string"));
  if (value.length === 0 || value.length > 19) return err(invalid("id has invalid length"));
  for (let i = 0; i < value.length; i += 1) {
    const c = value.charCodeAt(i);
    if (c < 0x30 || c > 0x39) return err(invalid("id must be decimal digits"));
  }
  if (value.length > 1 && value.startsWith("0")) return err(invalid("id must be canonical"));
  if (value.length === 19 && value > "9223372036854775807") {
    return err(invalid("id out of range"));
  }
  if (value === "0") return err(invalid("id out of range"));
  return ok(value);
}

/** 规范非负 u64 十进制字符串（允许 "0"，上限 18446744073709551615）。 */
export function parseU64String(value: unknown): ContractResult<string> {
  if (typeof value !== "string") return err(invalid("counter must be a string"));
  if (value.length === 0 || value.length > 20) return err(invalid("counter has invalid length"));
  for (let i = 0; i < value.length; i += 1) {
    const c = value.charCodeAt(i);
    if (c < 0x30 || c > 0x39) return err(invalid("counter must be decimal digits"));
  }
  if (value.length > 1 && value.startsWith("0")) return err(invalid("counter must be canonical"));
  if (value.length === 20 && value > "18446744073709551615") {
    return err(invalid("counter out of range"));
  }
  return ok(value);
}

/** 7.1 White_Space 集合；U+FEFF 不属于该集合。不使用语义不同的 JS trim。 */
export function isTrimWhitespace(codePoint: number): boolean {
  return (
    (codePoint >= 0x9 && codePoint <= 0xd) ||
    codePoint === 0x20 ||
    codePoint === 0x85 ||
    codePoint === 0xa0 ||
    codePoint === 0x1680 ||
    (codePoint >= 0x2000 && codePoint <= 0x200a) ||
    codePoint === 0x2028 ||
    codePoint === 0x2029 ||
    codePoint === 0x202f ||
    codePoint === 0x205f ||
    codePoint === 0x3000
  );
}

function hasUnpairedSurrogate(s: string): boolean {
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {
      const next = s.charCodeAt(i + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return true;
      i += 1;
    } else if (c >= 0xdc00 && c <= 0xdfff) {
      return true;
    }
  }
  return false;
}

function trimWhitespace(s: string): string {
  const points = [...s];
  let start = 0;
  let end = points.length;
  while (start < end && isTrimWhitespace(points[start].codePointAt(0)!)) start += 1;
  while (end > start && isTrimWhitespace(points[end - 1].codePointAt(0)!)) end -= 1;
  return points.slice(start, end).join("");
}

export function validateTimestampMs(value: unknown): ContractResult<number> {
  if (typeof value !== "number" || !Number.isInteger(value)) {
    return err(invalid("timestamp must be an integer"));
  }
  if (value < 0 || value > JS_SAFE_INTEGER_MAX) {
    return err(invalid("timestamp out of JS safe integer range"));
  }
  return ok(value);
}

export function validateLimit(value: unknown): ContractResult<number> {
  if (typeof value !== "number" || !Number.isInteger(value)) {
    return err(invalid("limit must be an integer"));
  }
  if (value < MIN_PAGE_LIMIT || value > MAX_PAGE_LIMIT) {
    return err(invalid("limit out of range"));
  }
  return ok(value);
}

export function normalizeQuery(value: unknown): ContractResult<string> {
  if (typeof value !== "string") return err(invalid("query must be a string"));
  if (hasUnpairedSurrogate(value)) return err(invalid("query has unpaired surrogate"));
  const normalized = trimWhitespace(value);
  if ([...normalized].length > MAX_QUERY_CHARS) return err(invalid("query exceeds max length"));
  return ok(normalized);
}

export interface ListCursor {
  normalized_query: string;
  upper_id: string;
  last_captured_at_ms: number;
  last_id: string;
}

export interface ListClipsRequest {
  request_id: string;
  query: string;
  limit: number;
  cursor: ListCursor | null;
}

function parseCursor(value: unknown, expectedQuery: string): ContractResult<ListCursor | null> {
  if (value === null || value === undefined) return ok(null);
  if (!isPlainObject(value)) return err(invalid("cursor must be an object"));
  const bad = rejectUnknownFields(value, ["normalized_query", "upper_id", "last_captured_at_ms", "last_id"]);
  if (bad) return err(bad);
  const nq = value.normalized_query;
  if (typeof nq !== "string") return err(invalid("cursor.normalized_query must be a string"));
  if (nq !== expectedQuery) return err(invalid("cursor does not match query"));
  const upper = parsePositiveId(value.upper_id);
  if (!upper.ok) return upper;
  const lastTs = validateTimestampMs(value.last_captured_at_ms);
  if (!lastTs.ok) return lastTs;
  const lastId = parsePositiveId(value.last_id);
  if (!lastId.ok) return lastId;
  if (compareIdStrings(lastId.value, upper.value) > 0) {
    return err(invalid("cursor last_id exceeds upper_id"));
  }
  return ok({
    normalized_query: nq,
    upper_id: upper.value,
    last_captured_at_ms: lastTs.value,
    last_id: lastId.value,
  });
}

/** 规范十进制 ID 字符串比较（同长度 19 位内可直接按长度/字典序）。 */
function compareIdStrings(a: string, b: string): number {
  if (a.length !== b.length) return a.length < b.length ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

export function parseListClipsRequest(value: unknown): ContractResult<ListClipsRequest> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["request_id", "query", "limit", "cursor"]);
  if (bad) return err(bad);
  const requestId = validateRequestId(value.request_id);
  if (!requestId.ok) return requestId;
  const query = normalizeQuery(value.query);
  if (!query.ok) return query;
  // 7.1：缺失 limit 为 50；显式 null、非整数或越界均拒绝。
  let limit: number;
  if (!("limit" in value) || value.limit === undefined) {
    limit = DEFAULT_PAGE_LIMIT;
  } else if (value.limit === null) {
    return err(invalid("limit must not be null"));
  } else {
    const parsed = validateLimit(value.limit);
    if (!parsed.ok) return parsed;
    limit = parsed.value;
  }
  // 7.1：缺失或显式 null 的 cursor 均表示首页。
  let cursor: ListCursor | null = null;
  if ("cursor" in value && value.cursor !== undefined && value.cursor !== null) {
    const parsed = parseCursor(value.cursor, query.value);
    if (!parsed.ok) return parsed;
    cursor = parsed.value;
  }
  return ok({
    request_id: requestId.value,
    query: query.value,
    limit,
    cursor,
  });
}

export interface GetClipTextRequest {
  request_id: string;
  clip_id: string;
  offset: number;
}

export function parseGetClipTextRequest(value: unknown): ContractResult<GetClipTextRequest> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["request_id", "clip_id", "offset"]);
  if (bad) return err(bad);
  const requestId = validateRequestId(value.request_id);
  if (!requestId.ok) return requestId;
  const clipId = parsePositiveId(value.clip_id);
  if (!clipId.ok) return clipId;
  const offset = value.offset;
  if (typeof offset !== "number" || !Number.isInteger(offset) || offset < 0 || offset > JS_SAFE_INTEGER_MAX) {
    return err(invalid("offset out of range"));
  }
  return ok({ request_id: requestId.value, clip_id: clipId.value, offset });
}

export function parseSetCaptureEnabledRequest(value: unknown): ContractResult<boolean> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["enabled"]);
  if (bad) return err(bad);
  if (typeof value.enabled !== "boolean") return err(invalid("enabled must be a boolean"));
  return ok(value.enabled);
}

/** Windows 路径词法规范化（7.1）：不做文件系统查询。 */
export function normalizeWindowsPath(
  input: string,
  requireExe: boolean,
  allowRoot: boolean,
): ContractResult<string> {
  const inputChars = [...input].length;
  if (input.length === 0 || inputChars > MAX_EXCLUDED_APP_CHARS) {
    return err(invalid("path length invalid"));
  }
  for (let i = 0; i < input.length; i += 1) {
    const c = input.charCodeAt(i);
    if (c < 0x20 || c === 0x7f) return err(invalid("path contains control character"));
  }
  const s = input.replace(/\//g, "\\");
  if (s.startsWith("\\\\?\\") || s.startsWith("\\\\.\\")) {
    return err(invalid("device namespace not allowed"));
  }

  let root: string;
  let segs: string[];
  if (s.startsWith("\\\\")) {
    const raw = s.slice(2).split("\\").filter((p) => p.length > 0);
    if (raw.length < 2) return err(invalid("UNC missing server or share"));
    const [server, share] = raw;
    if (server === "." || server === ".." || share === "." || share === "..") {
      return err(invalid("UNC root segment invalid"));
    }
    for (const part of [server, share]) {
      const e = validatePathSegment(part);
      if (e) return err(e);
    }
    root = `\\\\${server}\\${share}`;
    segs = raw.slice(2);
  } else if (
    s.length >= 3 &&
    isAsciiLetter(s.charCodeAt(0)) &&
    s.charCodeAt(1) === 0x3a &&
    s.charCodeAt(2) === 0x5c
  ) {
    const drive = s[0].toUpperCase();
    root = `${drive}:\\`;
    segs = s.slice(3).split("\\").filter((p) => p.length > 0);
  } else {
    return err(invalid("path must be a full drive or UNC path"));
  }

  if (s.endsWith("\\") && !(allowRoot && segs.length === 0)) {
    return err(invalid("path must not end with separator"));
  }

  const stack: string[] = [];
  for (const seg of segs) {
    if (seg === ".") continue;
    if (seg === "..") {
      if (stack.length === 0) return err(invalid("path escapes root"));
      stack.pop();
      continue;
    }
    const e = validatePathSegment(seg);
    if (e) return err(e);
    stack.push(seg);
  }

  if (stack.length === 0) {
    if (allowRoot) return ok(root.endsWith("\\") ? root : `${root}\\`);
    return err(invalid("path must end with .exe segment"));
  }
  if (requireExe) {
    const last = stack[stack.length - 1];
    const dot = last.lastIndexOf(".");
    const ext = dot >= 0 ? last.slice(dot + 1) : "";
    if (ext.length === 0 || ext.toLowerCase() !== "exe") {
      return err(invalid("path must end with .exe segment"));
    }
  }
  const joined = stack.join("\\");
  const out = root.endsWith("\\") ? `${root}${joined}` : `${root}\\${joined}`;
  if ([...out].length > MAX_EXCLUDED_APP_CHARS) {
    return err(invalid("path length invalid"));
  }
  return ok(out);
}

function isAsciiLetter(code: number): boolean {
  return (code >= 0x41 && code <= 0x5a) || (code >= 0x61 && code <= 0x7a);
}

function validatePathSegment(seg: string): ContractError | null {
  for (const ch of seg) {
    if (ch === "<" || ch === ">" || ch === ":" || ch === '"' || ch === "|" || ch === "?" || ch === "*") {
      return invalid("path segment has forbidden character");
    }
  }
  if (seg.endsWith(".") || seg.endsWith(" ")) {
    return invalid("path segment must not end with dot or space");
  }
  return null;
}

export function normalizeWindowsExePath(input: string): ContractResult<string> {
  return normalizeWindowsPath(input, true, false);
}

export function normalizeWindowsDirPath(input: string): ContractResult<string> {
  return normalizeWindowsPath(input, false, true);
}

export function parseSetExcludedAppsRequest(value: unknown): ContractResult<string[]> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["paths"]);
  if (bad) return err(bad);
  const paths = value.paths;
  if (!Array.isArray(paths)) return err(invalid("paths must be an array"));
  if (paths.length > MAX_EXCLUDED_APPS) return err(invalid("too many excluded apps"));
  const out: string[] = [];
  for (const p of paths) {
    if (typeof p !== "string") return err(invalid("path must be a string"));
    const normalized = normalizeWindowsExePath(p);
    if (!normalized.ok) return normalized;
    out.push(normalized.value);
  }
  return ok(out);
}

export const SUPPORTED_HOTKEY_KEYS: readonly string[] = [
  "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
  "T", "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
  "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
  "Space", "Insert", "Delete", "Home", "End", "PageUp", "PageDown",
];

export interface SetHotkeyRequest {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
  key: string;
}

export function parseSetHotkeyRequest(value: unknown): ContractResult<SetHotkeyRequest> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["ctrl", "alt", "shift", "win", "key"]);
  if (bad) return err(bad);
  for (const k of ["ctrl", "alt", "shift", "win"] as const) {
    if (typeof value[k] !== "boolean") return err(invalid(`${k} must be a boolean`));
  }
  const key = value.key;
  if (typeof key !== "string") return err(invalid("key must be a string"));
  if (!SUPPORTED_HOTKEY_KEYS.includes(key)) return err(invalid("unsupported key"));
  if (value.win === true && value.ctrl !== true && value.alt !== true && value.shift !== true && key === "V") {
    return err(invalid("Win+V is reserved"));
  }
  return ok({
    ctrl: value.ctrl as boolean,
    alt: value.alt as boolean,
    shift: value.shift as boolean,
    win: value.win as boolean,
    key,
  });
}

function validateSessionId(value: unknown): ContractResult<string> {
  if (typeof value !== "string") return err(invalid("panel_session_id must be a string"));
  if (value.length === 0 || byteLength(value) > MAX_REQUEST_ID_BYTES) {
    return err(invalid("panel_session_id invalid"));
  }
  for (let i = 0; i < value.length; i += 1) {
    const c = value.charCodeAt(i);
    if (c < 0x21 || c > 0x7e) return err(invalid("panel_session_id must be ASCII"));
  }
  return ok(value);
}

export function parsePanelReadyRequest(value: unknown): ContractResult<string> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["panel_session_id"]);
  if (bad) return err(bad);
  return validateSessionId(value.panel_session_id);
}

export interface PasteClipRequest {
  panel_session_id: string;
  clip_id: string;
}

export function parsePasteClipRequest(value: unknown): ContractResult<PasteClipRequest> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["panel_session_id", "clip_id"]);
  if (bad) return err(bad);
  const session = validateSessionId(value.panel_session_id);
  if (!session.ok) return session;
  const clipId = parsePositiveId(value.clip_id);
  if (!clipId.ok) return clipId;
  return ok({ panel_session_id: session.value, clip_id: clipId.value });
}

export function parseDismissPanelRequest(value: unknown): ContractResult<string> {
  if (!isPlainObject(value)) return err(invalid("request must be an object"));
  const bad = rejectUnknownFields(value, ["panel_session_id"]);
  if (bad) return err(bad);
  return validateSessionId(value.panel_session_id);
}

export type ClipKind = "text" | "link" | "png";
export type SourceConfidence = "owner" | "unknown";
export type StatusState = "startup" | "listening" | "paused" | "overloaded" | "failed" | "stopping";
export type ClipboardState = "not_written" | "written" | "failed" | "replaced";
export type InputState = "not_sent" | "sent" | "cancelled" | "failed";

function parseEnum(value: unknown, allowed: readonly string[], message: string): ContractResult<string> {
  if (typeof value !== "string") return err(invalid(message));
  if (!allowed.includes(value)) return err(invalid(message));
  return ok(value);
}

export function parseClipKind(value: unknown): ContractResult<ClipKind> {
  return parseEnum(value, ["text", "link", "png"], "unknown kind") as ContractResult<ClipKind>;
}

export function parseSourceConfidence(value: unknown): ContractResult<SourceConfidence> {
  return parseEnum(value, ["owner", "unknown"], "unknown source_confidence") as ContractResult<SourceConfidence>;
}

export function parseStatusState(value: unknown): ContractResult<StatusState> {
  return parseEnum(
    value,
    ["startup", "listening", "paused", "overloaded", "failed", "stopping"],
    "unknown status state",
  ) as ContractResult<StatusState>;
}

export function parseClipboardState(value: unknown): ContractResult<ClipboardState> {
  return parseEnum(
    value,
    ["not_written", "written", "failed", "replaced"],
    "unknown clipboard_state",
  ) as ContractResult<ClipboardState>;
}

export function parseInputState(value: unknown): ContractResult<InputState> {
  return parseEnum(value, ["not_sent", "sent", "cancelled", "failed"], "unknown input_state") as ContractResult<InputState>;
}

export function parseErrorCode(value: unknown): ContractResult<ErrorCode> {
  if (typeof value !== "string") return err(invalid("error code must be a string"));
  if (!ERROR_CODES.includes(value as ErrorCode)) return err(invalid("unknown error code"));
  return ok(value as ErrorCode);
}

export interface ClipSummary {
  id: string;
  captured_at_ms: number;
  kind: ClipKind;
  preview: string;
  format_names: string[];
  omitted_format_count: number;
  payload_bytes: number;
  source_process: string | null;
  source_confidence: SourceConfidence;
}

export function parseClipSummary(value: unknown): ContractResult<ClipSummary> {
  if (!isPlainObject(value)) return err(invalid("summary must be an object"));
  const bad = rejectUnknownFields(value, [
    "id",
    "captured_at_ms",
    "kind",
    "preview",
    "format_names",
    "omitted_format_count",
    "payload_bytes",
    "source_process",
    "source_confidence",
  ]);
  if (bad) return err(bad);
  const id = parsePositiveId(value.id);
  if (!id.ok) return id;
  const ts = validateTimestampMs(value.captured_at_ms);
  if (!ts.ok) return ts;
  const kind = parseClipKind(value.kind);
  if (!kind.ok) return kind;
  const preview = value.preview;
  if (typeof preview !== "string") return err(invalid("preview must be a string"));
  if ([...preview].length > MAX_PREVIEW_SCALARS) return err(invalid("preview exceeds max length"));
  const formats = value.format_names;
  if (!Array.isArray(formats)) return err(invalid("format_names must be an array"));
  if (formats.length > MAX_SUMMARY_FORMAT_NAMES) return err(invalid("too many format_names"));
  const formatNames: string[] = [];
  for (const f of formats) {
    if (typeof f !== "string") return err(invalid("format name must be a string"));
    formatNames.push(f);
  }
  const omitted = value.omitted_format_count;
  if (typeof omitted !== "number" || !Number.isInteger(omitted) || omitted < 0) {
    return err(invalid("omitted_format_count must be an integer"));
  }
  const payloadBytes = value.payload_bytes;
  if (typeof payloadBytes !== "number" || !Number.isInteger(payloadBytes) || payloadBytes < 0) {
    return err(invalid("payload_bytes must be an integer"));
  }
  const sp = value.source_process;
  if (sp !== null && typeof sp !== "string") {
    return err(invalid("source_process must be a string or null"));
  }
  const confidence = parseSourceConfidence(value.source_confidence);
  if (!confidence.ok) return confidence;
  return ok({
    id: id.value,
    captured_at_ms: ts.value,
    kind: kind.value,
    preview,
    format_names: formatNames,
    omitted_format_count: omitted,
    payload_bytes: payloadBytes,
    source_process: sp,
    source_confidence: confidence.value,
  });
}

export interface WorkerMessage {
  version: number;
  capture_id: string;
  policy_epoch: number;
}

/** worker 帧纯契约：4 字节 LE 长度，上限在分配之前检查，声明与实际字节一致。 */
export function parseFramePrefix(prefix: Uint8Array, max: number): ContractResult<number> {
  if (prefix.length < 4) return err(protocol("frame shorter than prefix"));
  const declared = prefix[0]! | (prefix[1]! << 8) | (prefix[2]! << 16) | (prefix[3]! << 24);
  const unsigned = declared >>> 0;
  if (unsigned > max) return err(protocol("frame length exceeds limit before allocation"));
  return ok(unsigned);
}

export function splitFrame(buf: Uint8Array, max: number): ContractResult<Uint8Array> {
  const declared = parseFramePrefix(buf, max);
  if (!declared.ok) return declared;
  const body = buf.subarray(4);
  if (body.length !== declared.value) {
    return err(protocol("declared length does not match body"));
  }
  return ok(body);
}

export function parseWorkerMessageJson(body: string): ContractResult<WorkerMessage> {
  let value: unknown;
  try {
    value = JSON.parse(body);
  } catch {
    return err(protocol("body is not valid JSON"));
  }
  if (!isPlainObject(value)) return err(protocol("body must be an object"));
  for (const key of Object.keys(value)) {
    if (key !== "version" && key !== "capture_id" && key !== "policy_epoch") {
      return err(protocol("unknown field"));
    }
  }
  const version = value.version;
  if (typeof version !== "number" || !Number.isInteger(version)) {
    return err(protocol("version must be an integer"));
  }
  if (version !== WORKER_PROTOCOL_VERSION) return err(protocol("unsupported protocol version"));
  const captureId = value.capture_id;
  if (typeof captureId !== "string") return err(protocol("capture_id must be a string"));
  if (captureId.length === 0 || captureId.length > 64) {
    return err(protocol("capture_id length invalid"));
  }
  for (let i = 0; i < captureId.length; i += 1) {
    const c = captureId.charCodeAt(i);
    if (c < 0x21 || c > 0x7e) return err(protocol("capture_id must be ASCII"));
  }
  const epoch = value.policy_epoch;
  if (typeof epoch !== "number" || !Number.isInteger(epoch) || epoch < 0) {
    return err(protocol("policy_epoch must be an integer"));
  }
  // 7.1：policy_epoch 为 0..=9007199254740991；超限原始 JSON 整数不得因舍入被接受。
  if (epoch > JS_SAFE_INTEGER_MAX) {
    return err(protocol("policy_epoch out of range"));
  }
  return ok({ version, capture_id: captureId, policy_epoch: epoch });
}

export function checkWorkerCorrelation(request: WorkerMessage, response: WorkerMessage): ContractResult<null> {
  if (request.capture_id !== response.capture_id) return err(protocol("capture_id mismatch"));
  if (request.policy_epoch !== response.policy_epoch) return err(protocol("policy_epoch mismatch"));
  return ok(null);
}

/** 稳定错误序列化；测试用此检查不泄漏敏感哨兵。 */
export function contractErrorToJson(error: ContractError): string {
  return JSON.stringify({ code: error.code, message: error.message, retryable: error.retryable });
}

export function errorJsonLeaks(json: string, sentinel: string): boolean {
  return json.includes(sentinel);
}

function validateShortId(value: unknown): ContractResult<string> {
  if (typeof value !== "string") return err(invalid("id must be a string"));
  if (value.length === 0 || byteLength(value) > MAX_REQUEST_ID_BYTES) {
    return err(invalid("id invalid length"));
  }
  for (let i = 0; i < value.length; i += 1) {
    const c = value.charCodeAt(i);
    if (c < 0x21 || c > 0x7e) return err(invalid("id must be ASCII"));
  }
  return ok(value);
}

export interface PasteClipResult {
  operation_id: string;
  clipboard_state: ClipboardState;
  input_state: InputState;
  reason: ErrorCode | null;
}

export function parsePasteClipResult(value: unknown): ContractResult<PasteClipResult> {
  if (!isPlainObject(value)) return err(invalid("result must be an object"));
  const bad = rejectUnknownFields(value, ["operation_id", "clipboard_state", "input_state", "reason"]);
  if (bad) return err(bad);
  const operationId = validateShortId(value.operation_id);
  if (!operationId.ok) return operationId;
  const clipboardState = parseClipboardState(value.clipboard_state);
  if (!clipboardState.ok) return clipboardState;
  const inputState = parseInputState(value.input_state);
  if (!inputState.ok) return inputState;
  let reason: ErrorCode | null = null;
  if (value.reason !== null) {
    const parsed = parseErrorCode(value.reason);
    if (!parsed.ok) return parsed;
    reason = parsed.value;
  }
  return ok({
    operation_id: operationId.value,
    clipboard_state: clipboardState.value,
    input_state: inputState.value,
    reason,
  });
}

export function parsePasteFinishedEvent(
  value: unknown,
): ContractResult<{ operation_id: string; result: PasteClipResult }> {
  if (!isPlainObject(value)) return err(invalid("event must be an object"));
  const bad = rejectUnknownFields(value, ["operation_id", "result"]);
  if (bad) return err(bad);
  const operationId = validateShortId(value.operation_id);
  if (!operationId.ok) return operationId;
  const result = parsePasteClipResult(value.result);
  if (!result.ok) return result;
  if (result.value.operation_id !== operationId.value) {
    return err(invalid("operation_id mismatch"));
  }
  return ok({ operation_id: operationId.value, result: result.value });
}

export function parseRevisionEvent(
  value: unknown,
  field: "revision" | "session_id",
): ContractResult<string> {
  if (!isPlainObject(value)) return err(invalid("event must be an object"));
  const bad = rejectUnknownFields(value, [field]);
  if (bad) return err(bad);
  // 7.2：history/status revision 为规范非负 u64 字符串；panel session 仍为短标识。
  if (field === "revision") return parseU64String(value[field]);
  return validateShortId(value[field]);
}

export interface ListClipsResult {
  request_id: string;
  normalized_query: string;
  summaries: ClipSummary[];
  next_cursor: ListCursor | null;
}

export function parseListClipsResult(value: unknown): ContractResult<ListClipsResult> {
  if (!isPlainObject(value)) return err(invalid("result must be an object"));
  const bad = rejectUnknownFields(value, ["request_id", "normalized_query", "summaries", "next_cursor"]);
  if (bad) return err(bad);
  const requestId = validateRequestId(value.request_id);
  if (!requestId.ok) return requestId;
  const nq = value.normalized_query;
  if (typeof nq !== "string") return err(invalid("normalized_query must be a string"));
  if (hasUnpairedSurrogate(nq)) return err(invalid("normalized_query has unpaired surrogate"));
  // 7.2：normalized_query 必须已经规范化；拒绝非规范值，不静默 trim 响应。
  const canonical = normalizeQuery(nq);
  if (!canonical.ok) return canonical;
  if (canonical.value !== nq) return err(invalid("normalized_query is not canonical"));
  const summariesRaw = value.summaries;
  if (!Array.isArray(summariesRaw)) return err(invalid("summaries must be an array"));
  if (summariesRaw.length > MAX_SUMMARIES_PER_PAGE) return err(invalid("too many summaries"));
  const summaries: ClipSummary[] = [];
  for (const s of summariesRaw) {
    const parsed = parseClipSummary(s);
    if (!parsed.ok) return parsed;
    summaries.push(parsed.value);
  }
  // 7.2：响应字段均必填；next_cursor 仅允许显式 null，不允许缺失。
  if (!("next_cursor" in value)) return err(invalid("missing field"));
  let nextCursor: ListCursor | null = null;
  if (value.next_cursor !== null) {
    const parsed = parseCursor(value.next_cursor, nq);
    if (!parsed.ok) return parsed;
    nextCursor = parsed.value;
  }
  const result: ListClipsResult = {
    request_id: requestId.value,
    normalized_query: nq,
    summaries,
    next_cursor: nextCursor,
  };
  const encoded = JSON.stringify(result);
  if (byteLength(encoded) > MAX_LIST_JSON_BYTES) {
    return err(invalid("list result exceeds size budget"));
  }
  return ok(result);
}

export interface GetClipTextResult {
  request_id: string;
  clip_id: string;
  text: string;
  next_offset: number | null;
}

export function parseGetClipTextResult(
  value: unknown,
  request: GetClipTextRequest,
): ContractResult<GetClipTextResult> {
  if (!isPlainObject(value)) return err(invalid("result must be an object"));
  const bad = rejectUnknownFields(value, ["request_id", "clip_id", "text", "next_offset"]);
  if (bad) return err(bad);
  const requestId = validateRequestId(value.request_id);
  if (!requestId.ok) return requestId;
  if (requestId.value !== request.request_id) return err(invalid("request_id echo mismatch"));
  const clipId = parsePositiveId(value.clip_id);
  if (!clipId.ok) return clipId;
  if (clipId.value !== request.clip_id) return err(invalid("clip_id echo mismatch"));
  const text = value.text;
  if (typeof text !== "string") return err(invalid("text must be a string"));
  if (hasUnpairedSurrogate(text)) return err(invalid("text has unpaired surrogate"));
  if (byteLength(text) > MAX_TEXT_FRAME_BYTES) return err(invalid("text exceeds frame byte limit"));
  let nextOffset: number | null = null;
  if (value.next_offset !== null) {
    const n = value.next_offset;
    if (typeof n !== "number" || !Number.isInteger(n) || n < 0 || n > JS_SAFE_INTEGER_MAX) {
      return err(invalid("next_offset out of range"));
    }
    if (text.length === 0) return err(invalid("next_offset requires non-empty text"));
    const expected = request.offset + byteLength(text);
    if (expected > JS_SAFE_INTEGER_MAX) return err(invalid("next_offset overflow"));
    if (n !== expected) return err(invalid("next_offset must equal offset plus text bytes"));
    nextOffset = n;
  }
  return ok({
    request_id: requestId.value,
    clip_id: clipId.value,
    text,
    next_offset: nextOffset,
  });
}

export type ServiceState = "not_started" | "starting" | "ready" | "failed" | "stopping" | "stopped";

export interface StatusResult {
  revision: string;
  state: StatusState;
  services: {
    listener: ServiceState;
    worker: ServiceState;
    storage: ServiceState;
    hotkey: ServiceState;
    target_observer: ServiceState;
  };
  capture: { user_enabled: boolean; admission_open: boolean; policy_epoch: number };
  counters: Record<string, string>;
  queue: { items: number; bytes: number };
  history: { items: number; payload_bytes: number };
  data_dir: string;
  versions: { app: string; schema: number; worker_protocol: number };
  last_paste: PasteClipResult | null;
}

const SERVICE_KEYS = ["listener", "worker", "storage", "hotkey", "target_observer"] as const;
const COUNTER_KEYS = [
  "observed",
  "captured",
  "persisted",
  "ignored",
  "superseded",
  "failed",
  "overloaded",
  "cancelled",
] as const;

export const MAX_QUEUE_ITEMS_LIMIT = 64;
export const MAX_QUEUE_BYTES_LIMIT = 33554432;
export const MAX_HISTORY_ITEMS_LIMIT = 10000;
export const MAX_HISTORY_PAYLOAD_BYTES_LIMIT = 268435456;
export const MAX_SUMMARIES_PER_PAGE = 100;
export const MAX_LIST_JSON_BYTES = 1048576;

function parseServiceState(value: unknown): ContractResult<ServiceState> {
  return parseEnum(
    value,
    ["not_started", "starting", "ready", "failed", "stopping", "stopped"],
    "unknown service state",
  ) as ContractResult<ServiceState>;
}

function parseBoundedCount(value: unknown, max: number): ContractResult<number> {
  if (typeof value !== "number" || !Number.isInteger(value)) {
    return err(invalid("count must be an integer"));
  }
  if (value < 0 || value > max) return err(invalid("count out of range"));
  return ok(value);
}

export function parseStatusResult(value: unknown): ContractResult<StatusResult> {
  if (!isPlainObject(value)) return err(invalid("result must be an object"));
  const bad = rejectUnknownFields(value, [
    "revision",
    "state",
    "services",
    "capture",
    "counters",
    "queue",
    "history",
    "data_dir",
    "versions",
    "last_paste",
  ]);
  if (bad) return err(bad);
  const revision = parseU64String(value.revision);
  if (!revision.ok) return revision;
  const state = parseStatusState(value.state);
  if (!state.ok) return state;

  if (!isPlainObject(value.services)) return err(invalid("services must be an object"));
  const svcBad = rejectUnknownFields(value.services, SERVICE_KEYS);
  if (svcBad) return err(svcBad);
  const services = {} as StatusResult["services"];
  for (const key of SERVICE_KEYS) {
    const parsed = parseServiceState(value.services[key]);
    if (!parsed.ok) return parsed;
    services[key] = parsed.value;
  }

  if (!isPlainObject(value.capture)) return err(invalid("capture must be an object"));
  const capBad = rejectUnknownFields(value.capture, ["user_enabled", "admission_open", "policy_epoch"]);
  if (capBad) return err(capBad);
  if (typeof value.capture.user_enabled !== "boolean") {
    return err(invalid("user_enabled must be a boolean"));
  }
  if (typeof value.capture.admission_open !== "boolean") {
    return err(invalid("admission_open must be a boolean"));
  }
  const epoch = value.capture.policy_epoch;
  if (typeof epoch !== "number" || !Number.isInteger(epoch) || epoch < 0 || epoch > JS_SAFE_INTEGER_MAX) {
    return err(invalid("policy_epoch out of range"));
  }

  if (!isPlainObject(value.counters)) return err(invalid("counters must be an object"));
  const cntBad = rejectUnknownFields(value.counters, COUNTER_KEYS);
  if (cntBad) return err(cntBad);
  const counters: Record<string, string> = {};
  for (const key of COUNTER_KEYS) {
    const parsed = parseU64String(value.counters[key]);
    if (!parsed.ok) return parsed;
    counters[key] = parsed.value;
  }

  if (!isPlainObject(value.queue)) return err(invalid("queue must be an object"));
  const qBad = rejectUnknownFields(value.queue, ["items", "bytes"]);
  if (qBad) return err(qBad);
  const qItems = parseBoundedCount(value.queue.items, MAX_QUEUE_ITEMS_LIMIT);
  if (!qItems.ok) return qItems;
  const qBytes = parseBoundedCount(value.queue.bytes, MAX_QUEUE_BYTES_LIMIT);
  if (!qBytes.ok) return qBytes;

  if (!isPlainObject(value.history)) return err(invalid("history must be an object"));
  const hBad = rejectUnknownFields(value.history, ["items", "payload_bytes"]);
  if (hBad) return err(hBad);
  const hItems = parseBoundedCount(value.history.items, MAX_HISTORY_ITEMS_LIMIT);
  if (!hItems.ok) return hItems;
  const hBytes = parseBoundedCount(value.history.payload_bytes, MAX_HISTORY_PAYLOAD_BYTES_LIMIT);
  if (!hBytes.ok) return hBytes;

  const dataDirRaw = value.data_dir;
  if (typeof dataDirRaw !== "string") return err(invalid("data_dir must be a string"));
  const dataDir = normalizeWindowsDirPath(dataDirRaw);
  if (!dataDir.ok) return dataDir;

  if (!isPlainObject(value.versions)) return err(invalid("versions must be an object"));
  const vBad = rejectUnknownFields(value.versions, ["app", "schema", "worker_protocol"]);
  if (vBad) return err(vBad);
  const app = value.versions.app;
  if (typeof app !== "string") return err(invalid("app version must be a string"));
  if (app.length === 0 || byteLength(app) > 32) return err(invalid("app version invalid"));
  for (let i = 0; i < app.length; i += 1) {
    const c = app.charCodeAt(i);
    if (c < 0x21 || c > 0x7e) return err(invalid("app version invalid"));
  }
  if (value.versions.schema !== 1) return err(invalid("schema version mismatch"));
  if (value.versions.worker_protocol !== WORKER_PROTOCOL_VERSION) {
    return err(invalid("worker protocol version mismatch"));
  }

  let lastPaste: PasteClipResult | null = null;
  if (value.last_paste !== null) {
    const parsed = parsePasteClipResult(value.last_paste);
    if (!parsed.ok) return parsed;
    lastPaste = parsed.value;
  }

  return ok({
    revision: revision.value,
    state: state.value,
    services,
    capture: {
      user_enabled: value.capture.user_enabled,
      admission_open: value.capture.admission_open,
      policy_epoch: epoch,
    },
    counters,
    queue: { items: qItems.value, bytes: qBytes.value },
    history: { items: hItems.value, payload_bytes: hBytes.value },
    data_dir: dataDir.value,
    versions: { app, schema: 1, worker_protocol: WORKER_PROTOCOL_VERSION },
    last_paste: lastPaste,
  });
}
