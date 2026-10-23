//! worker 协议纯契约：帧边界、hello 与消息字段校验。
//! 不创建 worker、不实现 supervisor、不读剪贴板；真实流/超时/回收由 worker.rs 承担。
//! 该内部协议只由 Rust 解析；WebView 仅接收宿主转换后的界面 DTO。

use crate::contracts::{
    ContractError, ContractResult, WORKER_PROTOCOL_VERSION, WORKER_REQUEST_MAX_BYTES,
    WORKER_RESPONSE_MAX_BYTES,
};

pub const FRAME_LENGTH_BYTES: usize = 4;

/// E03a 控制帧（hello/ACK）正文上限：先检查长度再分配/解析（方案 5.2.1）。
/// 第 5.2 节的 18 MiB 是快照响应总上限，不用于三字段 ACK。
pub const CONTROL_FRAME_MAX_BYTES: usize = 4 * 1024;

/// 4 字节 little-endian 长度前缀；在分配正文之前检查上限。
pub fn parse_frame_prefix(prefix: [u8; FRAME_LENGTH_BYTES], max: usize) -> ContractResult<usize> {
    let declared = u32::from_le_bytes(prefix) as usize;
    if declared > max {
        return Err(ContractError::protocol_error(
            "frame length exceeds limit before allocation",
        ));
    }
    Ok(declared)
}

/// 声明字节数必须与实际字节数一致；返回正文切片（不额外分配）。
pub fn split_frame<'a>(buf: &'a [u8], max: usize) -> ContractResult<&'a [u8]> {
    if buf.len() < FRAME_LENGTH_BYTES {
        return Err(ContractError::protocol_error("frame shorter than prefix"));
    }
    let mut prefix = [0u8; FRAME_LENGTH_BYTES];
    prefix.copy_from_slice(&buf[..FRAME_LENGTH_BYTES]);
    let declared = parse_frame_prefix(prefix, max)?;
    let body = &buf[FRAME_LENGTH_BYTES..];
    if body.len() != declared {
        return Err(ContractError::protocol_error(
            "declared length does not match body",
        ));
    }
    Ok(body)
}

pub fn split_request_frame(buf: &[u8]) -> ContractResult<&[u8]> {
    split_frame(buf, WORKER_REQUEST_MAX_BYTES)
}

pub fn split_response_frame(buf: &[u8]) -> ContractResult<&[u8]> {
    split_frame(buf, WORKER_RESPONSE_MAX_BYTES)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerMessage {
    pub version: u32,
    pub capture_id: String,
    pub policy_epoch: i64,
}

fn require_field<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> ContractResult<&'a serde_json::Value> {
    obj.get(name)
        .ok_or_else(|| ContractError::protocol_error("missing field"))
}

/// version=1；capture_id 为 1–64 字节 ASCII graphic 标识；policy_epoch 为 JS 安全非负整数。
pub fn parse_worker_message_json(body: &str) -> ContractResult<WorkerMessage> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| ContractError::protocol_error("body is not valid JSON"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| ContractError::protocol_error("body must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "version" | "capture_id" | "policy_epoch") {
            return Err(ContractError::protocol_error("unknown field"));
        }
    }
    let version = require_field(obj, "version")?
        .as_u64()
        .ok_or_else(|| ContractError::protocol_error("version must be an integer"))?;
    if version != WORKER_PROTOCOL_VERSION as u64 {
        return Err(ContractError::protocol_error("unsupported protocol version"));
    }
    let capture_value = require_field(obj, "capture_id")?;
    let capture_id = capture_value
        .as_str()
        .ok_or_else(|| ContractError::protocol_error("capture_id must be a string"))?
        .to_string();
    if capture_id.is_empty() || capture_id.len() > 64 {
        return Err(ContractError::protocol_error("capture_id length invalid"));
    }
    if !capture_id.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ContractError::protocol_error("capture_id must be ASCII"));
    }
    let policy_epoch = require_field(obj, "policy_epoch")?
        .as_i64()
        .ok_or_else(|| ContractError::protocol_error("policy_epoch must be an integer"))?;
    // 7.1：policy_epoch 为 0..=9007199254740991 的 JSON 整数；超限不得因 JS 舍入被接受。
    if policy_epoch < 0 || policy_epoch > crate::contracts::JS_SAFE_INTEGER_MAX {
        return Err(ContractError::protocol_error("policy_epoch out of range"));
    }
    Ok(WorkerMessage {
        version: version as u32,
        capture_id,
        policy_epoch,
    })
}

/// 关联校验：响应的 capture_id/policy_epoch 必须与在途请求一致。
pub fn check_correlation(request: &WorkerMessage, response: &WorkerMessage) -> ContractResult<()> {
    if request.capture_id != response.capture_id {
        return Err(ContractError::protocol_error("capture_id mismatch"));
    }
    if request.policy_epoch != response.policy_epoch {
        return Err(ContractError::protocol_error("policy_epoch mismatch"));
    }
    Ok(())
}

/// 握手帧唯一合法正文（严格相等，拒绝任何额外字段/错类型/错版本）。
pub const HELLO_BODY: &str = "{\"kind\":\"hello\",\"version\":1}";

/// 用权威规则校验请求字段与编码长度，并组出请求帧；纯函数，无副作用。
/// 校验顺序与 parse_worker_message_json 一致（version/capture_id/policy_epoch），
/// 编码后仍检查请求帧正文 <= WORKER_REQUEST_MAX_BYTES。
pub fn encode_request_message(message: &WorkerMessage) -> ContractResult<Vec<u8>> {
    let body = serde_json::to_string(&serde_json::json!({
        "version": message.version,
        "capture_id": message.capture_id,
        "policy_epoch": message.policy_epoch,
    }))
    .map_err(|_| ContractError::protocol_error("request encode failed"))?;
    parse_worker_message_json(&body)?;
    if body.len() > WORKER_REQUEST_MAX_BYTES {
        return Err(ContractError::protocol_error("frame length exceeds limit"));
    }
    encode_request_frame(body.as_bytes())
}

/// 组帧：4 字节 LE 长度前缀 + 正文；超限拒绝，避免发出越界帧。
pub fn encode_frame(body: &[u8], max: usize) -> ContractResult<Vec<u8>> {
    if body.len() > max {
        return Err(ContractError::protocol_error("frame length exceeds limit"));
    }
    let mut frame = Vec::with_capacity(FRAME_LENGTH_BYTES + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

pub fn encode_request_frame(body: &[u8]) -> ContractResult<Vec<u8>> {
    encode_frame(body, WORKER_REQUEST_MAX_BYTES)
}

pub fn encode_response_frame(body: &[u8]) -> ContractResult<Vec<u8>> {
    encode_frame(body, WORKER_RESPONSE_MAX_BYTES)
}

/// 严格 hello：恰好 {"kind":"hello","version":1}，字段必填且无额外字段。
pub fn parse_hello_json(body: &str) -> ContractResult<()> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| ContractError::protocol_error("body is not valid JSON"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| ContractError::protocol_error("body must be an object"))?;
    if obj.len() != 2 {
        return Err(ContractError::protocol_error("hello fields must be exact"));
    }
    let kind = require_field(obj, "kind")?
        .as_str()
        .ok_or_else(|| ContractError::protocol_error("kind must be a string"))?;
    if kind != "hello" {
        return Err(ContractError::protocol_error("kind must be hello"));
    }
    let version = require_field(obj, "version")?
        .as_u64()
        .ok_or_else(|| ContractError::protocol_error("version must be an integer"))?;
    if version != WORKER_PROTOCOL_VERSION as u64 {
        return Err(ContractError::protocol_error("unsupported protocol version"));
    }
    Ok(())
}

#[derive(Debug)]
pub struct ValidatedSnapshot {
    /// Large wire strings are removed after decoding; no redundant text copy.
    pub response: super::capture::CaptureResponse,
    pub payloads: Vec<crate::store::Payload>,
    pub internal_origin: Option<Vec<u8>>,
}

pub fn encode_snapshot(response: &super::capture::CaptureResponse) -> ContractResult<Vec<u8>> {
    let bytes=serde_json::to_vec(response).map_err(|_|ContractError::protocol_error("snapshot encode failed"))?;
    encode_response_frame(&bytes)
}

pub fn decode_snapshot(body: &[u8], request: &WorkerMessage) -> ContractResult<ValidatedSnapshot> {
    let parsed = parse_snapshot(body)?;
    check_correlation(request, &parsed.message())?;
    Ok(parsed)
}

impl ValidatedSnapshot {
    pub fn message(&self) -> WorkerMessage {
        WorkerMessage{version:self.response.version,capture_id:self.response.capture_id.clone(),policy_epoch:self.response.policy_epoch}
    }
}

pub(crate) fn parse_snapshot(body: &[u8]) -> ContractResult<ValidatedSnapshot> {
    use super::capture::{CaptureResponse,CaptureResult,CaptureReason,format_limit,validate_payloads,MAX_FORMATS};
    use base64::{Engine,engine::general_purpose::STANDARD};
    let bad=||ContractError::protocol_error("invalid snapshot response");
    if body.len()>WORKER_RESPONSE_MAX_BYTES {return Err(bad());}
    let mut r:CaptureResponse=serde_json::from_slice(body).map_err(|_|bad())?;
    let msg=WorkerMessage{version:r.version,capture_id:r.capture_id.clone(),policy_epoch:r.policy_epoch};
    encode_request_message(&msg)?;
    if r.kind!="snapshot" || !(1..=5).contains(&r.attempts) || r.formats.len()>3 || r.omitted_formats.len()>MAX_FORMATS {return Err(bad());}
    let mut names=std::collections::BTreeSet::new();
    if r.omitted_formats.iter().any(|n| n.is_empty() || n.chars().count()>256 || format_limit(n).is_some() || !names.insert(n)) {return Err(bad());}
    if r.owner.pid==Some(0) || (r.owner.pid.is_some() && r.owner.hwnd.is_none()) || (r.owner.process.is_some() && r.owner.pid.is_none())
        || r.owner.process.as_ref().is_some_and(|p|p.is_empty()||p.chars().count()>1024||p.contains('\0')) {return Err(bad());}
    if let Some(hwnd)=&r.owner.hwnd {
        if hwnd.parse::<u64>().ok().filter(|n|*n>0 && n.to_string()==*hwnd).is_none() {return Err(bad());}
    }
    let mut payloads=Vec::new();
    let mut internal_origin=None;
    match r.result {
        CaptureResult::Captured => {
            if r.reason.is_some() || r.sequence_before==0 || r.sequence_before!=r.sequence_locked || r.sequence_before!=r.sequence_after {return Err(bad());}
            for f in std::mem::take(&mut r.formats) {
                let limit=format_limit(&f.name).ok_or_else(bad)?;
                if f.data.len()>limit.div_ceil(3)*4 {return Err(bad());}
                payloads.push(crate::store::Payload{format_name:f.name,data:STANDARD.decode(f.data).map_err(|_|bad())?});
            }
            validate_payloads(&payloads).map_err(|_|bad())?;
            if let Some(raw)=r.internal_origin.take() {
                if raw.len()>256usize.div_ceil(3)*4 {return Err(bad());}
                let bytes=STANDARD.decode(raw).map_err(|_|bad())?;
                if !(28..=256).contains(&bytes.len()) {return Err(bad());}
                internal_origin=Some(bytes);
            }
        }
        CaptureResult::Ignored => {
            if !matches!(r.reason,Some(CaptureReason::Excluded|CaptureReason::HistoryDenied|CaptureReason::InvalidHistoryMarker|CaptureReason::NoSupportedFormat))
                || !r.formats.is_empty() || r.sequence_before==0 || r.sequence_before!=r.sequence_locked || r.sequence_before!=r.sequence_after {return Err(bad());}
        }
        CaptureResult::Failed => {
            if !matches!(r.reason,Some(CaptureReason::Busy|CaptureReason::ZeroSequence|CaptureReason::SequenceChanged|CaptureReason::InvalidPayload|CaptureReason::ReadFailed|CaptureReason::CloseFailed|CaptureReason::Deadline))
                || !r.formats.is_empty() || r.internal_origin.is_some() {return Err(bad());}
        }
    }
    if let Some(raw)=r.internal_origin.take() {
        if raw.len()>256usize.div_ceil(3)*4 {return Err(bad());}
        let bytes=STANDARD.decode(raw).map_err(|_|bad())?;
        if !(28..=256).contains(&bytes.len()) {return Err(bad());}
        internal_origin=Some(bytes);
    }
    Ok(ValidatedSnapshot{response:r,payloads,internal_origin})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_strict_accepts_exact_body() {
        parse_hello_json(HELLO_BODY).unwrap();
        parse_hello_json("{\"version\":1,\"kind\":\"hello\"}").unwrap();
    }

    #[test]
    fn hello_rejects_variant_bodies() {
        // 无 hello 字段、额外字段、错类型、错版本、非对象、非 JSON 全部拒绝。
        for bad in [
            "{}",
            "{\"kind\":\"hello\"}",
            "{\"kind\":\"hello\",\"version\":1,\"extra\":true}",
            "{\"kind\":\"hi\",\"version\":1}",
            "{\"kind\":\"hello\",\"version\":2}",
            "{\"kind\":\"hello\",\"version\":\"1\"}",
            "{\"kind\":1,\"version\":1}",
            "[]",
            "not json",
        ] {
            assert!(
                parse_hello_json(bad).is_err(),
                "should reject hello body: {bad}"
            );
        }
    }

    #[test]
    fn encode_frame_checks_limit_and_prefix() {
        let frame = encode_request_frame(b"abc").unwrap();
        assert_eq!(&frame[..4], &3u32.to_le_bytes());
        assert_eq!(&frame[4..], b"abc");
        assert!(encode_frame(&[0u8; 5], 4).is_err());
    }
}
