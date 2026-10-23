//! 同次持锁快照规则；底层 API 与时钟可替换，生产和测试共用状态机。
use crate::clipboard::protocol::WorkerMessage;
use crate::store::Payload;
use serde::{Deserialize, Serialize};
use crate::contracts::{MAX_TEXT_FORMAT_BYTES, MAX_PNG_BYTES, MAX_PAYLOAD_TOTAL_BYTES};

pub const MAX_FORMATS: usize = 256;
pub const RETRY_DELAYS_MS: [u64; 4] = [20, 40, 80, 160];
pub const DEADLINE_MS: u64 = 750;
pub const EXCLUDE: &str = "ExcludeClipboardContentFromMonitorProcessing";
pub const HISTORY: &str = "CanIncludeInClipboardHistory";
pub const CLOUD: &str = "CanUploadToCloudClipboard";
pub const IGNORE: &str = "Clipboard Viewer Ignore";
pub const INTERNAL: &str = "Pinfeather.InternalOrigin.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureReason {
    Excluded, HistoryDenied, InvalidHistoryMarker, NoSupportedFormat,
    Busy, ZeroSequence, SequenceChanged, InvalidPayload, ReadFailed, CloseFailed, Deadline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureResult { Captured, Ignored, Failed }

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub hwnd: Option<String>,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireFormat { pub name: String, pub data: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureResponse {
    pub kind: String,
    pub version: u32,
    pub capture_id: String,
    pub policy_epoch: i64,
    pub result: CaptureResult,
    pub reason: Option<CaptureReason>,
    pub sequence_before: u32,
    pub sequence_locked: u32,
    pub sequence_after: u32,
    pub owner: Owner,
    pub formats: Vec<WireFormat>,
    pub omitted_formats: Vec<String>,
    pub internal_origin: Option<String>,
    pub cloud_marker_anomaly: bool,
    pub attempts: u8,
}

impl CaptureResponse {
    pub fn empty(request: &WorkerMessage) -> Self {
        Self {
            kind: "snapshot".into(), version: request.version, capture_id: request.capture_id.clone(),
            policy_epoch: request.policy_epoch, result: CaptureResult::Failed,
            reason: Some(CaptureReason::ReadFailed), sequence_before: 0, sequence_locked: 0,
            sequence_after: 0, owner: Owner::default(), formats: vec![], omitted_formats: vec![],
            internal_origin: None, cloud_marker_anomaly: false, attempts: 0,
        }
    }
}

pub fn validate_payloads(payloads: &[Payload]) -> Result<(), CaptureReason> {
    let invalid = CaptureReason::InvalidPayload;
    if payloads.is_empty() || payloads.len() > 3 { return Err(invalid); }
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for p in payloads {
        if !names.insert(&p.format_name) { return Err(invalid); }
        let limit = format_limit(&p.format_name).ok_or(invalid)?;
        if p.data.len() > limit { return Err(invalid); }
        total = total.checked_add(p.data.len()).ok_or(invalid)?;
        match p.format_name.as_str() {
            "CF_UNICODETEXT" => {
                if p.data.len() % 2 != 0 { return Err(invalid); }
                let units: Vec<u16> = p.data.chunks_exact(2).map(|p| u16::from_le_bytes([p[0],p[1]])).collect();
                let end = units.iter().position(|u| *u == 0).ok_or(invalid)?;
                if char::decode_utf16(units[..end].iter().copied()).any(|c| c.is_err()) { return Err(invalid); }
            }
            "PNG" if !p.data.starts_with(b"\x89PNG\r\n\x1a\n") => return Err(invalid),
            _ => {}
        }
    }
    if total > MAX_PAYLOAD_TOTAL_BYTES { return Err(invalid); }
    Ok(())
}

pub fn format_limit(name: &str) -> Option<usize> {
    match name { "CF_UNICODETEXT" | "HTML Format" => Some(MAX_TEXT_FORMAT_BYTES), "PNG" => Some(MAX_PNG_BYTES), _ => None }
}

pub trait ClipboardApi {
    fn sequence(&mut self) -> u32;
    fn open(&mut self) -> bool;
    fn close(&mut self) -> Result<(), CaptureReason>;
    fn owner(&mut self) -> Owner;
    fn formats(&mut self) -> Result<Vec<(u32, String)>, CaptureReason>;
    /// Implementations must bound GlobalSize BEFORE copying or allocating.
    fn read(&mut self, format: u32, limit: usize) -> Result<Vec<u8>, CaptureReason>;
}

pub trait Clock {
    fn now_ms(&self) -> u64;
    fn sleep_ms(&mut self, millis: u64);
}

pub fn capture_request(request: &WorkerMessage, api: &mut impl ClipboardApi, clock: &mut impl Clock) -> CaptureResponse {
    let started = clock.now_ms();
    let mut response = CaptureResponse::empty(request);
    for attempt in 0..5 {
        if clock.now_ms().saturating_sub(started) >= DEADLINE_MS {
            response.reason = Some(CaptureReason::Deadline); break;
        }
        // Every retry discards all bytes from the previous lock interval.
        response = CaptureResponse::empty(request);
        response.attempts = attempt + 1;
        response.sequence_before = api.sequence();
        let outcome = if response.sequence_before == 0 {
            Err(CaptureReason::ZeroSequence)
        } else if !api.open() {
            Err(CaptureReason::Busy)
        } else {
            response.sequence_locked = api.sequence();
            let outcome = if response.sequence_locked == 0 || response.sequence_locked != response.sequence_before {
                Err(CaptureReason::SequenceChanged)
            } else { read_locked(api, &mut response) };
            response.sequence_after = api.sequence();
            let closed = api.close();
            if closed.is_err() {
                Err(CaptureReason::CloseFailed)
            } else if response.sequence_after == 0 || response.sequence_before != response.sequence_after {
                Err(CaptureReason::SequenceChanged)
            } else { outcome }
        };
        if clock.now_ms().saturating_sub(started) >= DEADLINE_MS {
            response.result = CaptureResult::Failed; response.reason = Some(CaptureReason::Deadline);
            response.formats.clear(); response.internal_origin = None; break;
        }
        match outcome {
            Ok(()) => return response,
            Err(reason) => {
                response.result = CaptureResult::Failed; response.reason = Some(reason);
                response.formats.clear(); response.internal_origin = None;
                if !matches!(reason, CaptureReason::Busy | CaptureReason::ZeroSequence | CaptureReason::SequenceChanged) || attempt == 4 { break; }
                let delay = RETRY_DELAYS_MS[attempt as usize];
                if clock.now_ms().saturating_sub(started).saturating_add(delay) >= DEADLINE_MS {
                    response.reason = Some(CaptureReason::Deadline); break;
                }
                clock.sleep_ms(delay);
            }
        }
    }
    response
}

fn read_locked(api: &mut impl ClipboardApi, response: &mut CaptureResponse) -> Result<(), CaptureReason> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let formats = api.formats()?;
    if formats.len() > MAX_FORMATS || formats.iter().any(|(_,n)| n.is_empty() || n.chars().count() > 256) {
        return Err(CaptureReason::ReadFailed);
    }
    let find = |name: &str| formats.iter().find(|(_,n)| n == name).map(|(id,_)| *id);
    fn ignore(response:&mut CaptureResponse,reason:CaptureReason){response.result=CaptureResult::Ignored;response.reason=Some(reason);}
    if find(EXCLUDE).is_some() || find(IGNORE).is_some() { ignore(response,CaptureReason::Excluded); return Ok(()); }
    response.owner = api.owner();
    if let Some(id) = find(INTERNAL) {
        response.internal_origin = api.read(id,256).ok().filter(|b| b.len() >= 28).map(|b| STANDARD.encode(b));
    }
    if let Some(id) = find(HISTORY) {
        match api.read(id,4).ok().and_then(|b| <[u8;4]>::try_from(b).ok()).map(u32::from_le_bytes) {
            Some(1) => {},
            Some(0) => {ignore(response,CaptureReason::HistoryDenied); return Ok(());}
            _ => {ignore(response,CaptureReason::InvalidHistoryMarker); return Ok(());}
        }
    }
    if let Some(id) = find(CLOUD) {
        response.cloud_marker_anomaly = !matches!(api.read(id,4).ok().and_then(|b| <[u8;4]>::try_from(b).ok()).map(u32::from_le_bytes),Some(0|1));
    }
    let mut payloads = Vec::new();
    for (id, name) in formats {
        if let Some(limit) = format_limit(&name) {
            payloads.push(Payload { format_name:name, data:api.read(id,limit)? });
        } else if !matches!(name.as_str(),EXCLUDE|IGNORE|HISTORY|CLOUD|INTERNAL) {
            response.omitted_formats.push(name);
        }
    }
    if payloads.is_empty() { response.result=CaptureResult::Ignored;response.reason=Some(CaptureReason::NoSupportedFormat);return Ok(()); }
    validate_payloads(&payloads)?;
    response.formats=payloads.into_iter().map(|p| WireFormat{name:p.format_name,data:STANDARD.encode(p.data)}).collect();
    response.result=CaptureResult::Captured;response.reason=None;
    Ok(())
}
