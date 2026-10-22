//! E02 共享契约权威：DTO、稳定错误与容量常量。
//! TS 镜像见 `src/lib/ipc.ts`；两侧读取 `tests/fixtures/contracts/` 同一合成夹具。
//! 只定义与验证形状，不注册 Tauri 命令，不返回假历史。

use std::collections::BTreeSet;

use serde::Serialize;

pub const MAX_REQUEST_ID_BYTES: usize = 64;
pub const MAX_QUERY_CHARS: usize = 256;
pub const MIN_PAGE_LIMIT: u32 = 1;
pub const MAX_PAGE_LIMIT: u32 = 100;
pub const DEFAULT_PAGE_LIMIT: u32 = 50;
pub const MAX_PREVIEW_SCALARS: usize = 200;
pub const MAX_SUMMARY_FORMAT_NAMES: usize = 3;
pub const MAX_EXCLUDED_APPS: usize = 32;
pub const MAX_EXCLUDED_APP_CHARS: usize = 1024;
pub const MAX_TEXT_FORMAT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PAYLOAD_TOTAL_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_QUEUE_ITEMS: u64 = 64;
pub const MAX_QUEUE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_HISTORY_ITEMS: u64 = 10_000;
pub const MAX_HISTORY_PAYLOAD_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_TEXT_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_PAGE_JSON_BYTES: usize = 1024 * 1024;
pub const WORKER_REQUEST_MAX_BYTES: usize = 4 * 1024;
pub const WORKER_RESPONSE_MAX_BYTES: usize = 18 * 1024 * 1024;
pub const WORKER_PROTOCOL_VERSION: u32 = 1;
pub const JS_SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    NotFound,
    Busy,
    CapacityExceeded,
    StorageUnavailable,
    ShortcutUnavailable,
    TargetChanged,
    PermissionUnknown,
    UnsupportedInputState,
    ClipboardChanged,
    InjectionFailed,
    Timeout,
    ProtocolError,
    CleanupFailed,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidInput => "invalid_input",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Busy => "busy",
            ErrorCode::CapacityExceeded => "capacity_exceeded",
            ErrorCode::StorageUnavailable => "storage_unavailable",
            ErrorCode::ShortcutUnavailable => "shortcut_unavailable",
            ErrorCode::TargetChanged => "target_changed",
            ErrorCode::PermissionUnknown => "permission_unknown",
            ErrorCode::UnsupportedInputState => "unsupported_input_state",
            ErrorCode::ClipboardChanged => "clipboard_changed",
            ErrorCode::InjectionFailed => "injection_failed",
            ErrorCode::Timeout => "timeout",
            ErrorCode::ProtocolError => "protocol_error",
            ErrorCode::CleanupFailed => "cleanup_failed",
        }
    }

    pub fn from_str(s: &str) -> Option<ErrorCode> {
        Some(match s {
            "invalid_input" => ErrorCode::InvalidInput,
            "not_found" => ErrorCode::NotFound,
            "busy" => ErrorCode::Busy,
            "capacity_exceeded" => ErrorCode::CapacityExceeded,
            "storage_unavailable" => ErrorCode::StorageUnavailable,
            "shortcut_unavailable" => ErrorCode::ShortcutUnavailable,
            "target_changed" => ErrorCode::TargetChanged,
            "permission_unknown" => ErrorCode::PermissionUnknown,
            "unsupported_input_state" => ErrorCode::UnsupportedInputState,
            "clipboard_changed" => ErrorCode::ClipboardChanged,
            "injection_failed" => ErrorCode::InjectionFailed,
            "timeout" => ErrorCode::Timeout,
            "protocol_error" => ErrorCode::ProtocolError,
            "cleanup_failed" => ErrorCode::CleanupFailed,
            _ => return None,
        })
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::Busy | ErrorCode::Timeout | ErrorCode::StorageUnavailable
        )
    }
}

/// 稳定错误：message 为安全常量文案，不得拼入正文、原始 JSON、SQL 或路径值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContractError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl ContractError {
    pub fn new(code: ErrorCode, message: &str) -> Self {
        ContractError {
            code,
            message: message.to_string(),
            retryable: code.retryable(),
        }
    }

    pub fn invalid_input(message: &str) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn protocol_error(message: &str) -> Self {
        Self::new(ErrorCode::ProtocolError, message)
    }

    /// 稳定错误序列化（serde 负责转义）；测试用此检查不泄漏敏感哨兵。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            "{\"code\":\"protocol_error\",\"message\":\"serialization failed\",\"retryable\":false}"
                .to_string()
        })
    }
}

pub type ContractResult<T> = Result<T, ContractError>;

fn reject(message: &str) -> ContractError {
    ContractError::invalid_input(message)
}

/// 非空 ASCII 标识，最长 64 字节。
pub fn validate_request_id(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("request_id must be a string"))?;
    if s.is_empty() {
        return Err(reject("request_id must not be empty"));
    }
    if s.len() > MAX_REQUEST_ID_BYTES {
        return Err(reject("request_id exceeds max length"));
    }
    if !s.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(reject("request_id must be ASCII identifier"));
    }
    Ok(s.to_string())
}

/// 规范正整数十进制字符串：1..=9223372036854775807，无前导零/符号/空白。
/// wire 上始终是字符串；返回规范字符串，内部比较可用 i64。
pub fn parse_positive_id(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("id must be a string"))?;
    parse_positive_id_str(s)
}

pub fn parse_positive_id_str(s: &str) -> ContractResult<String> {
    if s.is_empty() || s.len() > 19 {
        return Err(reject("id has invalid length"));
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(reject("id must be decimal digits"));
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(reject("id must be canonical"));
    }
    if s.len() == 19 && s > "9223372036854775807" {
        return Err(reject("id out of range"));
    }
    match s.parse::<i64>() {
        Ok(n) if n >= 1 => Ok(s.to_string()),
        _ => Err(reject("id out of range")),
    }
}

pub fn positive_id_as_i64(s: &str) -> i64 {
    s.parse::<i64>().unwrap_or(0)
}

/// 规范非负 u64 十进制字符串（允许 "0"，上限 18446744073709551615）。
pub fn parse_u64_string(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("counter must be a string"))?;
    if s.is_empty() || s.len() > 20 {
        return Err(reject("counter has invalid length"));
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(reject("counter must be decimal digits"));
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(reject("counter must be canonical"));
    }
    if s.len() == 20 && s > "18446744073709551615" {
        return Err(reject("counter out of range"));
    }
    Ok(s.to_string())
}

/// JS 安全整数范围内的非负毫秒时间戳。
pub fn validate_timestamp_ms(value: &serde_json::Value) -> ContractResult<i64> {
    let n = value
        .as_i64()
        .ok_or_else(|| reject("timestamp must be an integer"))?;
    if n < 0 || n > JS_SAFE_INTEGER_MAX {
        return Err(reject("timestamp out of JS safe integer range"));
    }
    Ok(n)
}

/// 分页 limit：整数 1..=100。
pub fn validate_limit(value: &serde_json::Value) -> ContractResult<u32> {
    let n = value
        .as_i64()
        .ok_or_else(|| reject("limit must be an integer"))?;
    if n < MIN_PAGE_LIMIT as i64 || n > MAX_PAGE_LIMIT as i64 {
        return Err(reject("limit out of range"));
    }
    Ok(n as u32)
}

/// 7.1 White_Space 集合：U+0009–000D、0020、0085、00A0、1680、2000–200A、2028、2029、202F、205F、3000。
/// U+FEFF 不属于该集合。
pub fn is_trim_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// 查询：按 7.1 White_Space 集合 trim 后按 Unicode 标量计数，上限 256；内部空白不变。
pub fn normalize_query(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("query must be a string"))?;
    normalize_query_str(s)
}

pub fn normalize_query_str(s: &str) -> ContractResult<String> {
    let normalized = s.trim_matches(is_trim_whitespace);
    if normalized.chars().count() > MAX_QUERY_CHARS {
        return Err(reject("query exceeds max length"));
    }
    Ok(normalized.to_string())
}

fn expect_object(value: &serde_json::Value) -> ContractResult<&serde_json::Map<String, serde_json::Value>> {
    value
        .as_object()
        .ok_or_else(|| reject("request must be an object"))
}

fn reject_unknown_fields(
    obj: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> ContractResult<()> {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    for key in obj.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(reject("unknown field"));
        }
    }
    Ok(())
}

fn require_field<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> ContractResult<&'a serde_json::Value> {
    obj.get(name).ok_or_else(|| reject("missing field"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListCursor {
    pub normalized_query: String,
    pub upper_id: String,
    pub last_captured_at_ms: i64,
    pub last_id: String,
}

fn parse_cursor(
    value: &serde_json::Value,
    expected_query: &str,
) -> ContractResult<Option<ListCursor>> {
    if value.is_null() {
        return Ok(None);
    }
    let obj = expect_object(value)?;
    reject_unknown_fields(
        obj,
        &[
            "normalized_query",
            "upper_id",
            "last_captured_at_ms",
            "last_id",
        ],
    )?;
    let nq = require_field(obj, "normalized_query")?
        .as_str()
        .ok_or_else(|| reject("cursor.normalized_query must be a string"))?;
    if nq != expected_query {
        return Err(reject("cursor does not match query"));
    }
    let upper_id = parse_positive_id(require_field(obj, "upper_id")?)?;
    let last_captured_at_ms = validate_timestamp_ms(require_field(obj, "last_captured_at_ms")?)?;
    let last_id = parse_positive_id(require_field(obj, "last_id")?)?;
    if positive_id_as_i64(&last_id) > positive_id_as_i64(&upper_id) {
        return Err(reject("cursor last_id exceeds upper_id"));
    }
    Ok(Some(ListCursor {
        normalized_query: nq.to_string(),
        upper_id,
        last_captured_at_ms,
        last_id,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListClipsRequest {
    pub request_id: String,
    pub query: String,
    pub limit: u32,
    pub cursor: Option<ListCursor>,
}

pub fn parse_list_clips_request(value: &serde_json::Value) -> ContractResult<ListClipsRequest> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["request_id", "query", "limit", "cursor"])?;
    let request_id = validate_request_id(require_field(obj, "request_id")?)?;
    let query = normalize_query(require_field(obj, "query")?)?;
    // 7.1：缺失 limit 为 50；显式 null、非整数或越界均拒绝。
    let limit = match obj.get("limit") {
        None => DEFAULT_PAGE_LIMIT,
        Some(v) if v.is_null() => return Err(reject("limit must not be null")),
        Some(v) => validate_limit(v)?,
    };
    // 7.1：缺失或显式 null 的 cursor 均表示首页。
    let cursor = match obj.get("cursor") {
        None => None,
        Some(v) if v.is_null() => None,
        Some(v) => parse_cursor(v, &query)?,
    };
    Ok(ListClipsRequest {
        request_id,
        query,
        limit,
        cursor,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetClipTextRequest {
    pub request_id: String,
    pub clip_id: String,
    pub offset: i64,
}

pub fn parse_get_clip_text_request(
    value: &serde_json::Value,
) -> ContractResult<GetClipTextRequest> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["request_id", "clip_id", "offset"])?;
    let request_id = validate_request_id(require_field(obj, "request_id")?)?;
    let clip_id = parse_positive_id(require_field(obj, "clip_id")?)?;
    let offset = require_field(obj, "offset")?
        .as_i64()
        .ok_or_else(|| reject("offset must be an integer"))?;
    if offset < 0 || offset > JS_SAFE_INTEGER_MAX {
        return Err(reject("offset out of range"));
    }
    Ok(GetClipTextRequest {
        request_id,
        clip_id,
        offset,
    })
}

pub fn parse_set_capture_enabled_request(value: &serde_json::Value) -> ContractResult<bool> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["enabled"])?;
    require_field(obj, "enabled")?
        .as_bool()
        .ok_or_else(|| reject("enabled must be a boolean"))
}

/// Windows 路径词法规范化（7.1）：不做文件系统查询。
/// require_exe=true 时末段必须是 .exe（扩展名 ASCII 不区分大小写）；
/// allow_root=true 时允许盘根 / UNC share 根（data_dir）。
pub fn normalize_windows_path(input: &str, require_exe: bool, allow_root: bool) -> ContractResult<String> {
    if input.is_empty() || input.chars().count() > MAX_EXCLUDED_APP_CHARS {
        return Err(reject("path length invalid"));
    }
    if input.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return Err(reject("path contains control character"));
    }
    let s = input.replace('/', "\\");
    if s.starts_with("\\\\?\\") || s.starts_with("\\\\.\\") {
        return Err(reject("device namespace not allowed"));
    }

    let (root, segs) = if s.starts_with("\\\\") {
        let raw: Vec<&str> = s[2..].split('\\').filter(|p| !p.is_empty()).collect();
        if raw.len() < 2 {
            return Err(reject("UNC missing server or share"));
        }
        let server = raw[0];
        let share = raw[1];
        if server == "." || server == ".." || share == "." || share == ".." {
            return Err(reject("UNC root segment invalid"));
        }
        validate_path_segment(server)?;
        validate_path_segment(share)?;
        let root = format!("\\\\{server}\\{share}");
        (root, raw[2..].to_vec())
    } else if s.len() >= 3
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.as_bytes()[1] == b':'
        && s.as_bytes()[2] == b'\\'
    {
        let drive = (s.as_bytes()[0] as char).to_ascii_uppercase();
        let segs: Vec<&str> = s[3..].split('\\').filter(|p| !p.is_empty()).collect();
        (format!("{drive}:\\"), segs)
    } else {
        return Err(reject("path must be a full drive or UNC path"));
    };

    if s.ends_with('\\') && !(allow_root && segs.is_empty()) {
        return Err(reject("path must not end with separator"));
    }

    let mut stack: Vec<String> = Vec::new();
    for seg in segs {
        if seg == "." {
            continue;
        }
        if seg == ".." {
            if stack.pop().is_none() {
                return Err(reject("path escapes root"));
            }
            continue;
        }
        validate_path_segment(seg)?;
        stack.push(seg.to_string());
    }

    if stack.is_empty() {
        if allow_root {
            return if root.ends_with('\\') {
                Ok(root)
            } else {
                Ok(format!("{root}\\"))
            };
        }
        return Err(reject("path must end with .exe segment"));
    }
    if require_exe {
        let last = stack.last().unwrap();
        let ext_ok = last
            .rsplit_once('.')
            .map(|(_, ext)| ext.eq_ignore_ascii_case("exe") && !ext.is_empty())
            .unwrap_or(false);
        if !ext_ok {
            return Err(reject("path must end with .exe segment"));
        }
    }
    let joined = stack.join("\\");
    let out = if root.ends_with('\\') {
        format!("{root}{joined}")
    } else {
        format!("{root}\\{joined}")
    };
    if out.chars().count() > MAX_EXCLUDED_APP_CHARS {
        return Err(reject("path length invalid"));
    }
    Ok(out)
}

fn validate_path_segment(seg: &str) -> ContractResult<()> {
    if seg
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(reject("path segment has forbidden character"));
    }
    if seg.ends_with('.') || seg.ends_with(' ') {
        return Err(reject("path segment must not end with dot or space"));
    }
    Ok(())
}

pub fn normalize_windows_exe_path(input: &str) -> ContractResult<String> {
    normalize_windows_path(input, true, false)
}

pub fn normalize_windows_dir_path(input: &str) -> ContractResult<String> {
    normalize_windows_path(input, false, true)
}

pub fn parse_set_excluded_apps_request(value: &serde_json::Value) -> ContractResult<Vec<String>> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["paths"])?;
    let paths = require_field(obj, "paths")?
        .as_array()
        .ok_or_else(|| reject("paths must be an array"))?;
    if paths.len() > MAX_EXCLUDED_APPS {
        return Err(reject("too many excluded apps"));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let s = p.as_str().ok_or_else(|| reject("path must be a string"))?;
        let normalized = normalize_windows_exe_path(s)?;
        out.push(normalized);
    }
    Ok(out)
}

/// 受支持按键集合（首轮最小集合；未列入的键名拒绝）。
pub const SUPPORTED_HOTKEY_KEYS: &[&str] = &[
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "F1",
    "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Space", "Insert",
    "Delete", "Home", "End", "PageUp", "PageDown",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetHotkeyRequest {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
}

pub fn parse_set_hotkey_request(value: &serde_json::Value) -> ContractResult<SetHotkeyRequest> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["ctrl", "alt", "shift", "win", "key"])?;
    let ctrl = require_field(obj, "ctrl")?
        .as_bool()
        .ok_or_else(|| reject("ctrl must be a boolean"))?;
    let alt = require_field(obj, "alt")?
        .as_bool()
        .ok_or_else(|| reject("alt must be a boolean"))?;
    let shift = require_field(obj, "shift")?
        .as_bool()
        .ok_or_else(|| reject("shift must be a boolean"))?;
    let win = require_field(obj, "win")?
        .as_bool()
        .ok_or_else(|| reject("win must be a boolean"))?;
    let key = require_field(obj, "key")?
        .as_str()
        .ok_or_else(|| reject("key must be a string"))?;
    if !SUPPORTED_HOTKEY_KEYS.contains(&key) {
        return Err(reject("unsupported key"));
    }
    if win && !ctrl && !alt && !shift && key == "V" {
        return Err(reject("Win+V is reserved"));
    }
    Ok(SetHotkeyRequest {
        ctrl,
        alt,
        shift,
        win,
        key: key.to_string(),
    })
}

fn validate_session_id(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("panel_session_id must be a string"))?;
    if s.is_empty() || s.len() > MAX_REQUEST_ID_BYTES {
        return Err(reject("panel_session_id invalid"));
    }
    if !s.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(reject("panel_session_id must be ASCII"));
    }
    Ok(s.to_string())
}

pub fn parse_panel_ready_request(value: &serde_json::Value) -> ContractResult<String> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["panel_session_id"])?;
    validate_session_id(require_field(obj, "panel_session_id")?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteClipRequest {
    pub panel_session_id: String,
    pub clip_id: String,
}

pub fn parse_paste_clip_request(value: &serde_json::Value) -> ContractResult<PasteClipRequest> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["panel_session_id", "clip_id"])?;
    let panel_session_id = validate_session_id(require_field(obj, "panel_session_id")?)?;
    let clip_id = parse_positive_id(require_field(obj, "clip_id")?)?;
    Ok(PasteClipRequest {
        panel_session_id,
        clip_id,
    })
}

pub fn parse_dismiss_panel_request(value: &serde_json::Value) -> ContractResult<String> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["panel_session_id"])?;
    validate_session_id(require_field(obj, "panel_session_id")?)
}

pub fn parse_clip_kind(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("kind must be a string"))?;
    match s {
        "text" | "link" | "png" => Ok(s.to_string()),
        _ => Err(reject("unknown kind")),
    }
}

pub fn parse_source_confidence(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("source_confidence must be a string"))?;
    match s {
        "owner" | "unknown" => Ok(s.to_string()),
        _ => Err(reject("unknown source_confidence")),
    }
}

pub fn parse_status_state(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("status state must be a string"))?;
    match s {
        "startup" | "listening" | "paused" | "overloaded" | "failed" | "stopping" => {
            Ok(s.to_string())
        }
        _ => Err(reject("unknown status state")),
    }
}

pub fn parse_clipboard_state(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("clipboard_state must be a string"))?;
    match s {
        "not_written" | "written" | "failed" | "replaced" => Ok(s.to_string()),
        _ => Err(reject("unknown clipboard_state")),
    }
}

pub fn parse_input_state(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("input_state must be a string"))?;
    match s {
        "not_sent" | "sent" | "cancelled" | "failed" => Ok(s.to_string()),
        _ => Err(reject("unknown input_state")),
    }
}

pub fn parse_error_code(value: &serde_json::Value) -> ContractResult<ErrorCode> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("error code must be a string"))?;
    ErrorCode::from_str(s).ok_or_else(|| reject("unknown error code"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClipSummary {
    pub id: String,
    pub captured_at_ms: i64,
    pub kind: String,
    pub preview: String,
    pub format_names: Vec<String>,
    pub omitted_format_count: u64,
    pub payload_bytes: u64,
    pub source_process: Option<String>,
    pub source_confidence: String,
}

pub fn parse_clip_summary(value: &serde_json::Value) -> ContractResult<ClipSummary> {
    let obj = expect_object(value)?;
    reject_unknown_fields(
        obj,
        &[
            "id",
            "captured_at_ms",
            "kind",
            "preview",
            "format_names",
            "omitted_format_count",
            "payload_bytes",
            "source_process",
            "source_confidence",
        ],
    )?;
    let id = parse_positive_id(require_field(obj, "id")?)?;
    let captured_at_ms = validate_timestamp_ms(require_field(obj, "captured_at_ms")?)?;
    let kind = parse_clip_kind(require_field(obj, "kind")?)?;
    let preview = require_field(obj, "preview")?
        .as_str()
        .ok_or_else(|| reject("preview must be a string"))?;
    if preview.chars().count() > MAX_PREVIEW_SCALARS {
        return Err(reject("preview exceeds max length"));
    }
    let formats = require_field(obj, "format_names")?
        .as_array()
        .ok_or_else(|| reject("format_names must be an array"))?;
    if formats.len() > MAX_SUMMARY_FORMAT_NAMES {
        return Err(reject("too many format_names"));
    }
    let mut format_names = Vec::with_capacity(formats.len());
    for f in formats {
        format_names.push(
            f.as_str()
                .ok_or_else(|| reject("format name must be a string"))?
                .to_string(),
        );
    }
    let omitted_format_count = require_field(obj, "omitted_format_count")?
        .as_u64()
        .ok_or_else(|| reject("omitted_format_count must be an integer"))?;
    let payload_bytes = require_field(obj, "payload_bytes")?
        .as_u64()
        .ok_or_else(|| reject("payload_bytes must be an integer"))?;
    let source_process_raw = require_field(obj, "source_process")?;
    let source_process = if source_process_raw.is_null() {
        None
    } else {
        Some(
            source_process_raw
                .as_str()
                .ok_or_else(|| reject("source_process must be a string or null"))?
                .to_string(),
        )
    };
    let source_confidence = parse_source_confidence(require_field(obj, "source_confidence")?)?;
    Ok(ClipSummary {
        id,
        captured_at_ms,
        kind,
        preview: preview.to_string(),
        format_names,
        omitted_format_count,
        payload_bytes,
        source_process,
        source_confidence,
    })
}

/// 校验夹具里的枚举列表：每项都必须是合法值。
pub fn validate_enum_list(value: &serde_json::Value, parse: fn(&serde_json::Value) -> ContractResult<String>) -> ContractResult<()> {
    let arr = value
        .as_array()
        .ok_or_else(|| reject("enum fixture must be an array"))?;
    for item in arr {
        parse(item)?;
    }
    Ok(())
}

/// 校验错误码列表夹具。
pub fn validate_error_code_list(value: &serde_json::Value) -> ContractResult<()> {
    let arr = value
        .as_array()
        .ok_or_else(|| reject("error code fixture must be an array"))?;
    for item in arr {
        parse_error_code(item)?;
    }
    Ok(())
}

/// 错误 JSON 不得包含敏感哨兵（正文/原始输入不得进入 code/message/retryable）。
pub fn error_json_leaks(json: &str, sentinel: &str) -> bool {
    json.contains(sentinel)
}

// ---- 7.2 完整响应 DTO ----

pub const SERVICE_KEYS: &[&str] = &["listener", "worker", "storage", "hotkey", "target_observer"];
pub const COUNTER_KEYS: &[&str] = &[
    "observed",
    "captured",
    "persisted",
    "ignored",
    "superseded",
    "failed",
    "overloaded",
    "cancelled",
];
pub const MAX_QUEUE_ITEMS_VALUE: i64 = 64;
pub const MAX_QUEUE_BYTES_VALUE: i64 = 33_554_432;
pub const MAX_HISTORY_ITEMS_VALUE: i64 = 10_000;
pub const MAX_HISTORY_PAYLOAD_BYTES_VALUE: i64 = 268_435_456;
pub const MAX_TEXT_FRAME_VALUE: usize = 65_536;
pub const MAX_SUMMARIES_PER_PAGE: usize = 100;
pub const MAX_LIST_RESULT_JSON_BYTES: usize = 1_048_576;
pub const SCHEMA_VERSION: u32 = 1;
pub const APP_VERSION_MAX_BYTES: usize = 32;

fn parse_service_state(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("service state must be a string"))?;
    match s {
        "not_started" | "starting" | "ready" | "failed" | "stopping" | "stopped" => {
            Ok(s.to_string())
        }
        _ => Err(reject("unknown service state")),
    }
}

fn parse_u64_counter(value: &serde_json::Value) -> ContractResult<String> {
    parse_u64_string(value)
}

fn parse_bounded_count(value: &serde_json::Value, max: i64) -> ContractResult<i64> {
    let n = value
        .as_i64()
        .ok_or_else(|| reject("count must be an integer"))?;
    if n < 0 || n > max {
        return Err(reject("count out of range"));
    }
    Ok(n)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceReadiness {
    pub listener: String,
    pub worker: String,
    pub storage: String,
    pub hotkey: String,
    pub target_observer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureFlags {
    pub user_enabled: bool,
    pub admission_open: bool,
    pub policy_epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureCounters {
    pub observed: String,
    pub captured: String,
    pub persisted: String,
    pub ignored: String,
    pub superseded: String,
    pub failed: String,
    pub overloaded: String,
    pub cancelled: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueUsage {
    pub items: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryUsage {
    pub items: i64,
    pub payload_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VersionsInfo {
    pub app: String,
    pub schema: u32,
    pub worker_protocol: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusResult {
    pub revision: String,
    pub state: String,
    pub services: ServiceReadiness,
    pub capture: CaptureFlags,
    pub counters: CaptureCounters,
    pub queue: QueueUsage,
    pub history: HistoryUsage,
    pub data_dir: String,
    pub versions: VersionsInfo,
    pub last_paste: Option<PasteClipResult>,
}

pub fn parse_status_result(value: &serde_json::Value) -> ContractResult<StatusResult> {
    let obj = expect_object(value)?;
    reject_unknown_fields(
        obj,
        &[
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
        ],
    )?;
    let revision = parse_u64_string(require_field(obj, "revision")?)?;
    let state = parse_status_state(require_field(obj, "state")?)?;

    let services_obj = expect_object(require_field(obj, "services")?)?;
    reject_unknown_fields(services_obj, SERVICE_KEYS)?;
    if services_obj.len() != SERVICE_KEYS.len() {
        return Err(reject("services keys incomplete"));
    }
    let get_service = |name: &str| -> ContractResult<String> {
        let v = services_obj
            .get(name)
            .ok_or_else(|| reject("missing field"))?;
        parse_service_state(v)
    };
    let services = ServiceReadiness {
        listener: get_service("listener")?,
        worker: get_service("worker")?,
        storage: get_service("storage")?,
        hotkey: get_service("hotkey")?,
        target_observer: get_service("target_observer")?,
    };

    let capture_obj = expect_object(require_field(obj, "capture")?)?;
    reject_unknown_fields(capture_obj, &["user_enabled", "admission_open", "policy_epoch"])?;
    let user_enabled = capture_obj
        .get("user_enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| reject("user_enabled must be a boolean"))?;
    let admission_open = capture_obj
        .get("admission_open")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| reject("admission_open must be a boolean"))?;
    let policy_epoch = capture_obj
        .get("policy_epoch")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| reject("policy_epoch must be an integer"))?;
    if policy_epoch < 0 || policy_epoch > JS_SAFE_INTEGER_MAX {
        return Err(reject("policy_epoch out of range"));
    }
    let capture = CaptureFlags {
        user_enabled,
        admission_open,
        policy_epoch,
    };

    let counters_obj = expect_object(require_field(obj, "counters")?)?;
    reject_unknown_fields(counters_obj, COUNTER_KEYS)?;
    if counters_obj.len() != COUNTER_KEYS.len() {
        return Err(reject("counters keys incomplete"));
    }
    let get_counter = |name: &str| -> ContractResult<String> {
        let v = counters_obj.get(name).ok_or_else(|| reject("missing field"))?;
        parse_u64_counter(v)
    };
    let counters = CaptureCounters {
        observed: get_counter("observed")?,
        captured: get_counter("captured")?,
        persisted: get_counter("persisted")?,
        ignored: get_counter("ignored")?,
        superseded: get_counter("superseded")?,
        failed: get_counter("failed")?,
        overloaded: get_counter("overloaded")?,
        cancelled: get_counter("cancelled")?,
    };

    let queue_obj = expect_object(require_field(obj, "queue")?)?;
    reject_unknown_fields(queue_obj, &["items", "bytes"])?;
    let queue = QueueUsage {
        items: parse_bounded_count(
            queue_obj.get("items").ok_or_else(|| reject("missing field"))?,
            MAX_QUEUE_ITEMS_VALUE,
        )?,
        bytes: parse_bounded_count(
            queue_obj.get("bytes").ok_or_else(|| reject("missing field"))?,
            MAX_QUEUE_BYTES_VALUE,
        )?,
    };

    let history_obj = expect_object(require_field(obj, "history")?)?;
    reject_unknown_fields(history_obj, &["items", "payload_bytes"])?;
    let history = HistoryUsage {
        items: parse_bounded_count(
            history_obj.get("items").ok_or_else(|| reject("missing field"))?,
            MAX_HISTORY_ITEMS_VALUE,
        )?,
        payload_bytes: parse_bounded_count(
            history_obj
                .get("payload_bytes")
                .ok_or_else(|| reject("missing field"))?,
            MAX_HISTORY_PAYLOAD_BYTES_VALUE,
        )?,
    };

    let data_dir_raw = require_field(obj, "data_dir")?
        .as_str()
        .ok_or_else(|| reject("data_dir must be a string"))?;
    let data_dir = normalize_windows_dir_path(data_dir_raw)?;

    let versions_obj = expect_object(require_field(obj, "versions")?)?;
    reject_unknown_fields(versions_obj, &["app", "schema", "worker_protocol"])?;
    let app = versions_obj
        .get("app")
        .and_then(|v| v.as_str())
        .ok_or_else(|| reject("app version must be a string"))?;
    if app.is_empty() || app.len() > APP_VERSION_MAX_BYTES || !app.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(reject("app version invalid"));
    }
    let schema = versions_obj
        .get("schema")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| reject("schema must be an integer"))?;
    if schema != SCHEMA_VERSION as u64 {
        return Err(reject("schema version mismatch"));
    }
    let worker_protocol = versions_obj
        .get("worker_protocol")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| reject("worker_protocol must be an integer"))?;
    if worker_protocol != WORKER_PROTOCOL_VERSION as u64 {
        return Err(reject("worker protocol version mismatch"));
    }
    let versions = VersionsInfo {
        app: app.to_string(),
        schema: schema as u32,
        worker_protocol: worker_protocol as u32,
    };

    let last_paste_raw = require_field(obj, "last_paste")?;
    let last_paste = if last_paste_raw.is_null() {
        None
    } else {
        Some(parse_paste_clip_result(last_paste_raw)?)
    };

    Ok(StatusResult {
        revision,
        state,
        services,
        capture,
        counters,
        queue,
        history,
        data_dir,
        versions,
        last_paste,
    })
}

/// 事件/结果用的短标识：非空 ASCII graphic，最长 64 字节。
fn validate_short_id(value: &serde_json::Value) -> ContractResult<String> {
    let s = value
        .as_str()
        .ok_or_else(|| reject("id must be a string"))?;
    if s.is_empty() || s.len() > MAX_REQUEST_ID_BYTES {
        return Err(reject("id invalid length"));
    }
    if !s.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(reject("id must be ASCII"));
    }
    Ok(s.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PasteClipResult {
    pub operation_id: String,
    pub clipboard_state: String,
    pub input_state: String,
    pub reason: Option<ErrorCode>,
}

pub fn parse_paste_clip_result(value: &serde_json::Value) -> ContractResult<PasteClipResult> {
    let obj = expect_object(value)?;
    reject_unknown_fields(
        obj,
        &["operation_id", "clipboard_state", "input_state", "reason"],
    )?;
    let operation_id = validate_short_id(require_field(obj, "operation_id")?)?;
    let clipboard_state = parse_clipboard_state(require_field(obj, "clipboard_state")?)?;
    let input_state = parse_input_state(require_field(obj, "input_state")?)?;
    let reason_raw = require_field(obj, "reason")?;
    let reason = if reason_raw.is_null() {
        None
    } else {
        Some(parse_error_code(reason_raw)?)
    };
    Ok(PasteClipResult {
        operation_id,
        clipboard_state,
        input_state,
        reason,
    })
}

pub fn parse_paste_finished_event(
    value: &serde_json::Value,
) -> ContractResult<(String, PasteClipResult)> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["operation_id", "result"])?;
    let operation_id = validate_short_id(require_field(obj, "operation_id")?)?;
    let result = parse_paste_clip_result(require_field(obj, "result")?)?;
    if result.operation_id != operation_id {
        return Err(reject("operation_id mismatch"));
    }
    Ok((operation_id, result))
}

pub fn parse_revision_event(
    value: &serde_json::Value,
    field: &str,
    allowed: &[&str],
) -> ContractResult<String> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, allowed)?;
    // 7.2：history/status revision 为规范非负 u64 字符串；panel session 仍为短标识。
    let parsed = require_field(obj, field)?;
    if field == "revision" {
        parse_u64_string(parsed)
    } else {
        validate_short_id(parsed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListClipsResult {
    pub request_id: String,
    pub normalized_query: String,
    pub summaries: Vec<ClipSummary>,
    pub next_cursor: Option<ListCursor>,
}

pub fn parse_list_clips_result(value: &serde_json::Value) -> ContractResult<ListClipsResult> {
    let obj = expect_object(value)?;
    reject_unknown_fields(
        obj,
        &["request_id", "normalized_query", "summaries", "next_cursor"],
    )?;
    let request_id = validate_request_id(require_field(obj, "request_id")?)?;
    let normalized = require_field(obj, "normalized_query")?
        .as_str()
        .ok_or_else(|| reject("normalized_query must be a string"))?;
    // 7.2：normalized_query 必须已经规范化；拒绝非规范值，不静默 trim 响应。
    if normalize_query_str(normalized)? != normalized {
        return Err(reject("normalized_query is not canonical"));
    }
    let summaries_raw = require_field(obj, "summaries")?
        .as_array()
        .ok_or_else(|| reject("summaries must be an array"))?;
    if summaries_raw.len() > MAX_SUMMARIES_PER_PAGE {
        return Err(reject("too many summaries"));
    }
    let mut summaries = Vec::with_capacity(summaries_raw.len());
    for s in summaries_raw {
        summaries.push(parse_clip_summary(s)?);
    }
    let cursor_raw = require_field(obj, "next_cursor")?;
    let next_cursor = if cursor_raw.is_null() {
        None
    } else {
        parse_cursor(cursor_raw, normalized)?
    };
    let result = ListClipsResult {
        request_id,
        normalized_query: normalized.to_string(),
        summaries,
        next_cursor,
    };
    let serialized = serde_json::to_string(&result)
        .map_err(|_| reject("result serialization failed"))?;
    if serialized.len() > MAX_LIST_RESULT_JSON_BYTES {
        return Err(reject("list result exceeds size budget"));
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GetClipTextResult {
    pub request_id: String,
    pub clip_id: String,
    pub text: String,
    pub next_offset: Option<i64>,
}

/// 结合对应请求验证回声 ID 与 next_offset 进展（7.2）。
pub fn parse_get_clip_text_result(
    value: &serde_json::Value,
    request: &GetClipTextRequest,
) -> ContractResult<GetClipTextResult> {
    let obj = expect_object(value)?;
    reject_unknown_fields(obj, &["request_id", "clip_id", "text", "next_offset"])?;
    let request_id = validate_request_id(require_field(obj, "request_id")?)?;
    if request_id != request.request_id {
        return Err(reject("request_id echo mismatch"));
    }
    let clip_id = parse_positive_id(require_field(obj, "clip_id")?)?;
    if clip_id != request.clip_id {
        return Err(reject("clip_id echo mismatch"));
    }
    let text = require_field(obj, "text")?
        .as_str()
        .ok_or_else(|| reject("text must be a string"))?;
    if text.len() > MAX_TEXT_FRAME_VALUE {
        return Err(reject("text exceeds frame byte limit"));
    }
    let next_raw = require_field(obj, "next_offset")?;
    let next_offset = if next_raw.is_null() {
        None
    } else {
        let n = next_raw
            .as_i64()
            .ok_or_else(|| reject("next_offset must be an integer"))?;
        if n < 0 || n > JS_SAFE_INTEGER_MAX {
            return Err(reject("next_offset out of range"));
        }
        if text.is_empty() {
            return Err(reject("next_offset requires non-empty text"));
        }
        let expected = request
            .offset
            .checked_add(text.len() as i64)
            .ok_or_else(|| reject("next_offset overflow"))?;
        if n != expected {
            return Err(reject("next_offset must equal offset plus text bytes"));
        }
        Some(n)
    };
    Ok(GetClipTextResult {
        request_id,
        clip_id,
        text: text.to_string(),
        next_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::protocol::{
        check_correlation, parse_frame_prefix, parse_worker_message_json, split_frame,
        split_request_frame, FRAME_LENGTH_BYTES,
    };
    use serde_json::{json, Value};

    fn load_fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("contracts")
            .join(name);
        let text = std::fs::read_to_string(path).expect("fixture readable");
        serde_json::from_str(&text).expect("fixture json")
    }

    fn fill_from(case: &Value, key: &str) -> Option<String> {
        let fill = case.get(key)?;
        let ch = fill.get("char").and_then(|v| v.as_str()).unwrap_or("");
        let count = fill.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let prefix = fill.get("prefix").and_then(|v| v.as_str()).unwrap_or("");
        let suffix = fill.get("suffix").and_then(|v| v.as_str()).unwrap_or("");
        let mut s = String::from(prefix);
        for _ in 0..count {
            s.push_str(ch);
        }
        s.push_str(suffix);
        Some(s)
    }

    fn case_value(case: &Value) -> Value {
        if let Some(v) = case.get("value") {
            return v.clone();
        }
        if let Some(s) = fill_from(case, "value_fill") {
            return Value::String(s);
        }
        Value::Null
    }

    #[test]
    fn request_id_fixtures() {
        let doc = load_fixture("primitives.json");
        for case in doc["cases"].as_array().unwrap() {
            if case["group"] != "request_id" {
                continue;
            }
            let value = case_value(case);
            let id = case["id"].as_str().unwrap();
            let result = validate_request_id(&value);
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn positive_id_fixtures() {
        let doc = load_fixture("primitives.json");
        for case in doc["cases"].as_array().unwrap() {
            if case["group"] != "positive_id" {
                continue;
            }
            let value = case_value(case);
            let id = case["id"].as_str().unwrap();
            let result = parse_positive_id(&value);
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn query_limit_timestamp_fixtures() {
        let doc = load_fixture("query_limits.json");
        for case in doc["cases"].as_array().unwrap() {
            let value = case_value(case);
            let id = case["id"].as_str().unwrap();
            let result: ContractResult<i64> = match case["group"].as_str().unwrap() {
                "query" => normalize_query(&value).map(|_| 0),
                "limit" => validate_limit(&value).map(|n| n as i64),
                "timestamp_ms" => validate_timestamp_ms(&value),
                other => panic!("unknown group {other}"),
            };
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn query_normalization_matches_fixture() {
        let doc = load_fixture("query_limits.json");
        for case in doc["cases"].as_array().unwrap() {
            if case["group"] != "query" || case["expect"] != "accept" {
                continue;
            }
            let value = case_value(case);
            let normalized = normalize_query(&value).unwrap();
            let expected = if let Some(n) = case.get("normalized") {
                n.as_str().unwrap().to_string()
            } else {
                fill_from(case, "normalized_fill").unwrap()
            };
            assert_eq!(normalized, expected, "{}", case["id"]);
        }
    }

    #[test]
    fn request_dto_fixtures() {
        let doc = load_fixture("requests.json");
        for case in doc["cases"].as_array().unwrap() {
            let value = case_value(case);
            let id = case["id"].as_str().unwrap();
            let group = case["group"].as_str().unwrap();
            let result: ContractResult<()> = match group {
                "list_clips_request" => parse_list_clips_request(&value).map(|_| ()),
                "get_clip_text_request" => parse_get_clip_text_request(&value).map(|_| ()),
                "set_capture_enabled_request" => parse_set_capture_enabled_request(&value).map(|_| ()),
                "set_excluded_apps_request" => {
                    if value.get("paths_count").is_some() {
                        let n = value["paths_count"].as_u64().unwrap() as usize;
                        let paths: Vec<Value> =
                            (0..n).map(|i| json!(format!("C:\\App\\p{i}.exe"))).collect();
                        parse_set_excluded_apps_request(&json!({ "paths": paths })).map(|_| ())
                    } else {
                        parse_set_excluded_apps_request(&value).map(|_| ())
                    }
                }
                "set_hotkey_request" => parse_set_hotkey_request(&value).map(|_| ()),
                "panel_ready_request" => parse_panel_ready_request(&value).map(|_| ()),
                "paste_clip_request" => parse_paste_clip_request(&value).map(|_| ()),
                "dismiss_panel_request" => parse_dismiss_panel_request(&value).map(|_| ()),
                other => panic!("unknown group {other}"),
            };
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn response_shape_and_enum_fixtures() {
        let doc = load_fixture("responses.json");
        for case in doc["cases"].as_array().unwrap() {
            let value = case_value(case);
            let id = case["id"].as_str().unwrap();
            let group = case["group"].as_str().unwrap();
            let result: ContractResult<()> = match group {
                "clip_summary" => parse_clip_summary(&value).map(|_| ()),
                "error_code" => {
                    if value.is_array() {
                        validate_error_code_list(&value)
                    } else {
                        parse_error_code(&value).map(|_| ())
                    }
                }
                "status_state" => {
                    if value.is_array() {
                        validate_enum_list(&value, parse_status_state)
                    } else {
                        parse_status_state(&value).map(|_| ())
                    }
                }
                "clipboard_state" => {
                    if value.is_array() {
                        validate_enum_list(&value, parse_clipboard_state)
                    } else {
                        parse_clipboard_state(&value).map(|_| ())
                    }
                }
                "input_state" => {
                    if value.is_array() {
                        validate_enum_list(&value, parse_input_state)
                    } else {
                        parse_input_state(&value).map(|_| ())
                    }
                }
                other => panic!("unknown group {other}"),
            };
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn error_json_does_not_leak_sentinel() {
        let doc = load_fixture("errors_worker.json");
        let sentinel = doc["sensitive_sentinel"].as_str().unwrap();
        for case in doc["cases"].as_array().unwrap() {
            if case["group"] != "error_no_leak" {
                continue;
            }
            let id = case["id"].as_str().unwrap();
            let value = case_value(case);
            let group = value["group"].as_str().unwrap();
            let input = if let Some(s) = value.get("input") {
                s.as_str().unwrap().to_string()
            } else {
                fill_from(&value, "input_fill").unwrap()
            };
            let err = match group {
                "query" => normalize_query(&json!(input)).unwrap_err(),
                "request_id" => validate_request_id(&json!(input)).unwrap_err(),
                "positive_id" => parse_positive_id(&json!(input)).unwrap_err(),
                other => panic!("unknown group {other}"),
            };
            let serialized = err.to_json();
            assert!(
                !error_json_leaks(&serialized, sentinel),
                "{id} leaked sentinel: {serialized}"
            );
            assert!(err.message.contains("must") || err.message.contains("exceeds") || err.message.contains("invalid") || err.message.contains("length") || err.message.contains("range") || err.message.contains("canonical") || err.message.contains("decimal"), "{id} message: {}", err.message);
        }
    }

    #[test]
    fn worker_frame_fixtures() {
        let doc = load_fixture("errors_worker.json");
        for case in doc["cases"].as_array().unwrap() {
            if case["group"] != "worker_frame" {
                continue;
            }
            let id = case["id"].as_str().unwrap();
            let value = case_value(case);
            let expect = case["expect"].as_str().unwrap();

            if let Some(declared) = value.get("declared_length").and_then(|v| v.as_u64()) {
                let max = if value.get("over_cap") == Some(&json!("request")) {
                    WORKER_REQUEST_MAX_BYTES
                } else {
                    WORKER_RESPONSE_MAX_BYTES
                };
                let mut prefix = [0u8; FRAME_LENGTH_BYTES];
                prefix.copy_from_slice(&(declared as u32).to_le_bytes());
                let result = parse_frame_prefix(prefix, max);
                match expect {
                    "reject" => assert!(result.is_err(), "{id} should reject"),
                    "accept" => assert!(result.is_ok(), "{id} should accept"),
                    _ => {}
                }
                continue;
            }

            let body = value["body"].as_str().unwrap().as_bytes().to_vec();
            let mut frame = Vec::new();
            let declared = if value.get("force_length_mismatch") == Some(&json!(true)) {
                (body.len() + 1) as u32
            } else {
                body.len() as u32
            };
            frame.extend_from_slice(&declared.to_le_bytes());
            frame.extend_from_slice(&body);

            let split = split_request_frame(&frame);
            match expect {
                "reject" => {
                    if split.is_ok() {
                        let msg = parse_worker_message_json(std::str::from_utf8(split.unwrap()).unwrap());
                        assert!(msg.is_err(), "{id} should reject at message parse");
                    }
                }
                "accept" => {
                    let body = split.unwrap_or_else(|e| panic!("{id} frame split: {e:?}"));
                    parse_worker_message_json(std::str::from_utf8(body).unwrap())
                        .unwrap_or_else(|e| panic!("{id} message parse: {e:?}"));
                }
                _ => {}
            }
        }
    }

    #[test]
    fn worker_frame_checks_before_allocation() {
        let mut prefix = [0u8; FRAME_LENGTH_BYTES];
        prefix.copy_from_slice(&5000u32.to_le_bytes());
        assert!(parse_frame_prefix(prefix, WORKER_REQUEST_MAX_BYTES).is_err());
        let mut prefix = [0u8; FRAME_LENGTH_BYTES];
        prefix.copy_from_slice(&20_000_000u32.to_le_bytes());
        assert!(parse_frame_prefix(prefix, WORKER_RESPONSE_MAX_BYTES).is_err());
        let mut prefix = [0u8; FRAME_LENGTH_BYTES];
        prefix.copy_from_slice(&10u32.to_le_bytes());
        assert_eq!(
            parse_frame_prefix(prefix, WORKER_REQUEST_MAX_BYTES).unwrap(),
            10
        );
    }

    #[test]
    fn worker_correlation_id_epoch() {
        let request = parse_worker_message_json("{\"version\":1,\"capture_id\":\"1\",\"policy_epoch\":7}")
            .unwrap();
        let ok = parse_worker_message_json("{\"version\":1,\"capture_id\":\"1\",\"policy_epoch\":7}")
            .unwrap();
        assert!(check_correlation(&request, &ok).is_ok());
        let bad_id =
            parse_worker_message_json("{\"version\":1,\"capture_id\":\"2\",\"policy_epoch\":7}").unwrap();
        assert!(check_correlation(&request, &bad_id).is_err());
        let bad_epoch =
            parse_worker_message_json("{\"version\":1,\"capture_id\":\"1\",\"policy_epoch\":8}").unwrap();
        assert!(check_correlation(&request, &bad_epoch).is_err());
    }

    #[test]
    fn worker_frame_length_mismatch_rejected() {
        let body = b"{\"version\":1,\"capture_id\":\"1\",\"policy_epoch\":1}";
        let mut frame = Vec::new();
        frame.extend_from_slice(&((body.len() + 5) as u32).to_le_bytes());
        frame.extend_from_slice(body);
        assert!(split_frame(&frame, WORKER_REQUEST_MAX_BYTES).is_err());
    }

    #[test]
    fn constants_match_spec() {
        assert_eq!(MAX_QUERY_CHARS, 256);
        assert_eq!(DEFAULT_PAGE_LIMIT, 50);
        assert_eq!(MAX_PAGE_LIMIT, 100);
        assert_eq!(MAX_REQUEST_ID_BYTES, 64);
        assert_eq!(JS_SAFE_INTEGER_MAX, 9007199254740991);
        assert_eq!(WORKER_PROTOCOL_VERSION, 1);
        assert_eq!(WORKER_REQUEST_MAX_BYTES, 4096);
        assert_eq!(WORKER_RESPONSE_MAX_BYTES, 18 * 1024 * 1024);
    }

    #[test]
    fn event_and_result_fixtures() {
        let doc = load_fixture("events_results.json");
        for case in doc["cases"].as_array().unwrap() {
            let value = case_value(case);
            let id = case["id"].as_str().unwrap();
            let group = case["group"].as_str().unwrap();
            let result: ContractResult<()> = match group {
                "history_changed" => {
                    parse_revision_event(&value, "revision", &["revision"]).map(|_| ())
                }
                "status_changed" => {
                    parse_revision_event(&value, "revision", &["revision"]).map(|_| ())
                }
                "panel_opened" => {
                    parse_revision_event(&value, "session_id", &["session_id"]).map(|_| ())
                }
                "paste_finished" => parse_paste_finished_event(&value).map(|_| ()),
                "list_clips_result" => parse_list_clips_result(&value).map(|_| ()),
                other => panic!("unknown group {other}"),
            };
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                _ => {}
            }
        }
    }

    fn units_to_string(units: &[u32]) -> ContractResult<String> {
        let mut out = String::new();
        for u in units {
            if (0xD800..=0xDFFF).contains(u) {
                return Err(reject("unpaired surrogate"));
            }
            let ch = std::char::from_u32(*u).ok_or_else(|| reject("invalid scalar"))?;
            out.push(ch);
        }
        Ok(out)
    }

    #[test]
    fn f1_trim_whitespace_set_matches_spec() {
        let doc = load_fixture("fix_boundaries.json");
        for case in doc["trim_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let units: Vec<u32> = case["value_units"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let built = units_to_string(&units);
            if case["expect"] == "reject" {
                match built {
                    Err(_) => {}
                    Ok(s) => {
                        let r = normalize_query(&serde_json::json!(s));
                        assert!(r.is_err(), "{id} should reject");
                    }
                }
                continue;
            }
            let s = built.unwrap();
            let normalized = normalize_query(&serde_json::json!(s)).unwrap();
            let expected_units: Vec<u32> = case["normalized_units"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let expected = units_to_string(&expected_units).unwrap();
            assert_eq!(normalized, expected, "{id}");
        }
    }

    #[test]
    fn f2_excluded_apps_path_lexical() {
        let doc = load_fixture("fix_boundaries.json");
        for case in doc["path_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let input = case["input"].as_str().unwrap();
            let result = parse_set_excluded_apps_request(&serde_json::json!({ "paths": [input] }));
            if case["expect"] == "reject" {
                assert!(result.is_err(), "{id} should reject: {result:?}");
                continue;
            }
            let paths = result.unwrap_or_else(|e| panic!("{id} should accept: {e:?}"));
            assert_eq!(paths[0], case["normalized"].as_str().unwrap(), "{id}");
        }
    }

    #[test]
    fn f3_id_range_string_canonical() {
        let doc = load_fixture("fix_boundaries.json");
        for case in doc["id_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = case["value"].as_str().unwrap();
            let result = parse_positive_id(&serde_json::json!(value));
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => assert!(result.is_err(), "{id} should reject"),
                other => panic!("unknown expect {other}"),
            }
        }
    }

    #[test]
    fn f4_list_clips_optional_defaults() {
        let doc = load_fixture("fix_boundaries.json");
        for case in doc["list_optional_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = case_value(case);
            let result = parse_list_clips_request(&value);
            if case["expect"] == "reject" {
                assert!(result.is_err(), "{id} should reject: {result:?}");
                continue;
            }
            let req = result.unwrap_or_else(|e| panic!("{id} should accept: {e:?}"));
            assert_eq!(
                req.limit,
                case["expect_limit"].as_u64().unwrap() as u32,
                "{id} limit"
            );
            let expect_cursor = &case["expect_cursor"];
            if expect_cursor.is_null() {
                assert!(req.cursor.is_none(), "{id} cursor should be none");
            } else {
                assert!(req.cursor.is_some(), "{id} cursor should be present");
            }
        }
    }

    #[test]
    fn f5_policy_epoch_range() {
        let doc = load_fixture("fix_boundaries.json");
        for case in doc["epoch_raw_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let raw = case["raw"].as_str().unwrap();
            let result = parse_worker_message_json(raw);
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => assert!(result.is_err(), "{id} should reject"),
                other => panic!("unknown expect {other}"),
            }
            if let Some(correlate_raw) = case.get("correlate_with") {
                let request = result.clone().unwrap();
                let response = parse_worker_message_json(correlate_raw.as_str().unwrap()).unwrap();
                let check = check_correlation(&request, &response);
                match case["correlate_expect"].as_str().unwrap() {
                    "accept" => assert!(check.is_ok(), "{id} correlate should accept"),
                    "reject" => assert!(check.is_err(), "{id} correlate should reject"),
                    other => panic!("unknown expect {other}"),
                }
            }
        }
    }

    fn expand_fill(value: &Value) -> Value {
        if let Some(fill) = value.as_object().and_then(|o| o.get("text_fill")) {
            let ch = fill.get("char").and_then(|v| v.as_str()).unwrap_or("");
            let count = fill.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let mut s = String::new();
            for _ in 0..count {
                s.push_str(ch);
            }
            return Value::String(s);
        }
        value.clone()
    }

    fn expand_value_case(case: &Value) -> Value {
        let mut v = case_value(case);
        if let Some(obj) = v.as_object_mut() {
            if obj.contains_key("text_fill") {
                let fill = obj.remove("text_fill").unwrap();
                let ch = fill.get("char").and_then(|x| x.as_str()).unwrap_or("");
                let count = fill.get("count").and_then(|x| x.as_u64()).unwrap_or(0);
                let mut s = String::new();
                for _ in 0..count {
                    s.push_str(ch);
                }
                obj.insert("text".to_string(), Value::String(s));
            }
            if obj.contains_key("summaries_count") {
                let n = obj.remove("summaries_count").unwrap().as_u64().unwrap();
                let summary = json!({
                    "id": "1",
                    "captured_at_ms": 1000,
                    "kind": "text",
                    "preview": "hi",
                    "format_names": ["CF_UNICODETEXT"],
                    "omitted_format_count": 0,
                    "payload_bytes": 4,
                    "source_process": null,
                    "source_confidence": "unknown"
                });
                let arr: Vec<Value> = (0..n).map(|_| summary.clone()).collect();
                obj.insert("summaries".to_string(), Value::Array(arr));
            }
        }
        v
    }

    #[test]
    fn status_result_fixtures_and_serde() {
        let doc = load_fixture("response_dtos.json");
        for case in doc["status_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = case_value(case);
            let result = parse_status_result(&value);
            if case["expect"] == "reject" {
                assert!(result.is_err(), "{id} should reject: {result:?}");
                continue;
            }
            let status = result.unwrap_or_else(|e| panic!("{id} should accept: {e:?}"));
            let serialized = serde_json::to_string(&status).unwrap();
            let wire: Value = serde_json::from_str(&serialized).unwrap();
            assert!(wire["revision"].is_string(), "{id} revision must be string");
            assert!(
                wire["counters"]["observed"].is_string(),
                "{id} counters must be strings"
            );
            assert!(wire["capture"]["policy_epoch"].is_number(), "{id} epoch number");
            let reparsed = parse_status_result(&wire).unwrap();
            assert_eq!(reparsed, status, "{id} serde roundtrip");
        }
    }

    #[test]
    fn get_clip_text_result_fixtures() {
        let doc = load_fixture("response_dtos.json");
        for case in doc["text_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = expand_value_case(case);
            let request = parse_get_clip_text_request(&case["request"]).unwrap();
            let result = parse_get_clip_text_result(&value, &request);
            if case["expect"] == "reject" {
                assert!(result.is_err(), "{id} should reject: {result:?}");
                continue;
            }
            let text_result = result.unwrap_or_else(|e| panic!("{id} should accept: {e:?}"));
            assert_eq!(text_result.request_id, request.request_id, "{id} echo");
            assert_eq!(text_result.clip_id, request.clip_id, "{id} echo");
            let serialized = serde_json::to_string(&text_result).unwrap();
            let wire: Value = serde_json::from_str(&serialized).unwrap();
            assert!(wire["clip_id"].is_string(), "{id} clip_id string on wire");
            let reparsed = parse_get_clip_text_result(&wire, &request).unwrap();
            assert_eq!(reparsed, text_result, "{id} serde roundtrip");
        }
    }

    #[test]
    fn list_clips_result_fixtures() {
        let doc = load_fixture("response_dtos.json");
        for case in doc["list_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = expand_value_case(case);
            let result = parse_list_clips_result(&value);
            if case["expect"] == "reject" {
                assert!(result.is_err(), "{id} should reject: {result:?}");
                continue;
            }
            let list = result.unwrap_or_else(|e| panic!("{id} should accept: {e:?}"));
            assert!(list.summaries.len() <= MAX_SUMMARIES_PER_PAGE, "{id}");
            let serialized = serde_json::to_string(&list).unwrap();
            assert!(serialized.len() <= MAX_LIST_RESULT_JSON_BYTES, "{id} budget");
            let wire: Value = serde_json::from_str(&serialized).unwrap();
            assert!(wire["request_id"].is_string(), "{id}");
            if let Some(first) = wire["summaries"].as_array().unwrap().first() {
                assert!(first["id"].is_string(), "{id} summary id string");
            }
            let reparsed = parse_list_clips_result(&wire).unwrap();
            assert_eq!(reparsed, list, "{id} serde roundtrip");
        }
    }

    #[test]
    fn list_result_budget_exceed_rejected() {
        let mut summaries = Vec::new();
        let long_source = "s".repeat(12_000);
        for i in 0..100 {
            summaries.push(json!({
                "id": format!("{}", i + 1),
                "captured_at_ms": 1000,
                "kind": "text",
                "preview": "hi",
                "format_names": ["CF_UNICODETEXT"],
                "omitted_format_count": 0,
                "payload_bytes": 4,
                "source_process": long_source,
                "source_confidence": "unknown"
            }));
        }
        let value = json!({
            "request_id": "r1",
            "normalized_query": "",
            "summaries": summaries,
            "next_cursor": null
        });
        let result = parse_list_clips_result(&value);
        assert!(result.is_err(), "over-budget list must reject");
    }

    #[test]
    fn f6_event_revision_u64() {
        let doc = load_fixture("events_results.json");
        for case in doc["revision_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = case_value(case);
            let group = case["group"].as_str().unwrap();
            let result: ContractResult<String> = match group {
                "history_changed" | "status_changed" => {
                    parse_revision_event(&value, "revision", &["revision"])
                }
                "panel_opened" => parse_revision_event(&value, "session_id", &["session_id"]),
                other => panic!("unknown group {other}"),
            };
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => {
                    let err = result.err().unwrap_or_else(|| panic!("{id} should reject"));
                    assert_eq!(err.code, ErrorCode::InvalidInput, "{id}");
                }
                other => panic!("unknown expect {other}"),
            }
        }
    }

    #[test]
    fn f7_list_normalized_query_required_fields() {
        let doc = load_fixture("response_dtos.json");
        for case in doc["list_norm_cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let value = case_value(case);
            let result = parse_list_clips_result(&value);
            match case["expect"].as_str().unwrap() {
                "accept" => assert!(result.is_ok(), "{id} should accept: {result:?}"),
                "reject" => assert!(result.is_err(), "{id} should reject"),
                other => panic!("unknown expect {other}"),
            }
        }
    }
}
