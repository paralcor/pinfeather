//! worker 协议纯契约：帧边界与消息字段校验。
//! 不创建 worker、不实现 supervisor、不读剪贴板；真实流/超时/回收留给 E03。

use crate::contracts::{
    ContractError, ContractResult, WORKER_PROTOCOL_VERSION, WORKER_REQUEST_MAX_BYTES,
    WORKER_RESPONSE_MAX_BYTES,
};

pub const FRAME_LENGTH_BYTES: usize = 4;

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

/// 校验消息 JSON：version=1、capture_id 规范正整数 ID、policy_epoch 非负 i64。
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
