//! Schema v1 storage. Connections are owned by their creating storage thread.

use crate::contracts::{
    ContractError, ContractResult, ErrorCode, HistoryUsage, JS_SAFE_INTEGER_MAX,
    MAX_HISTORY_ITEMS, MAX_HISTORY_PAYLOAD_BYTES, MAX_PAYLOAD_TOTAL_BYTES, MAX_PNG_BYTES,
    MAX_PREVIEW_SCALARS, MAX_TEXT_FORMAT_BYTES,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

pub const SCHEMA_VERSION: i64 = 1;
pub const APPLICATION_ID: i64 = 0x504E4631;

// The SQL is also the v1 schema fingerprint. Unknown tables, triggers, views or
// altered constraints require an explicit future migration, never an auto-repair.
const CLIPS_SQL: &str = "CREATE TABLE clips (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    capture_id TEXT NOT NULL UNIQUE,
    captured_at_ms INTEGER NOT NULL CHECK(captured_at_ms >= 0),
    text_full TEXT,
    preview TEXT NOT NULL,
    source_process TEXT,
    source_confidence TEXT NOT NULL CHECK(source_confidence IN ('owner','unknown')),
    omitted_formats_json TEXT NOT NULL,
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0)
)";
const PAYLOADS_SQL: &str = "CREATE TABLE payloads (
    clip_id INTEGER NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
    format_name TEXT NOT NULL CHECK(format_name IN ('CF_UNICODETEXT','HTML Format','PNG')),
    encoding_version INTEGER NOT NULL CHECK(encoding_version = 1),
    byte_size INTEGER NOT NULL CHECK(byte_size >= 0 AND byte_size = length(data)),
    data BLOB NOT NULL,
    PRIMARY KEY(clip_id,format_name)
)";
const RECENT_INDEX_SQL: &str = "CREATE INDEX clips_recent ON clips(captured_at_ms DESC,id DESC)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payload {
    pub format_name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceConfidence {
    Owner,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub capture_id: String,
    pub captured_at_ms: i64,
    pub source_process: Option<String>,
    pub source_confidence: SourceConfidence,
    pub omitted_formats: Vec<String>,
    pub payloads: Vec<Payload>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InsertResult {
    pub clip_id: i64,
    pub inserted: bool,
    pub usage: HistoryUsage,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StoredClip {
    pub clip_id: i64,
    pub snapshot: Snapshot,
    pub text_full: Option<String>,
    pub preview: String,
    pub payload_bytes: i64,
}

/// Construct and use on the dedicated storage thread. No connection or SQL is
/// exposed to callers; Rc's marker also prevents moving a live Store to a thread.
///
/// ```compile_fail
/// use pinfeather_lib::store::Store;
/// let store = Store::open_in_memory().unwrap();
/// std::thread::spawn(move || drop(store));
/// ```
pub struct Store {
    conn: Connection,
    _owner: PhantomData<Rc<()>>,
    #[cfg(test)]
    limits: (i64, i64),
}

fn unavailable() -> ContractError {
    ContractError::new(ErrorCode::StorageUnavailable, "history storage is unavailable")
}

fn sql_error(error: rusqlite::Error) -> ContractError {
    if matches!(error.sqlite_error_code(), Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)) {
        ContractError::new(ErrorCode::Busy, "history storage is busy")
    } else {
        // SQLite messages can include SQL and user data (including trigger text).
        unavailable()
    }
}

#[derive(PartialEq, Eq)]
enum SchemaState { Empty, Current }

fn check_database(conn: &Connection) -> ContractResult<SchemaState> {
    let mut check = conn.prepare("PRAGMA quick_check").map_err(sql_error)?;
    let rows = check.query_map([], |r| r.get::<_, String>(0)).map_err(sql_error)?;
    let result: Vec<String> = rows.collect::<rusqlite::Result<_>>().map_err(sql_error)?;
    if result != ["ok"] { return Err(unavailable()); }
    let app: i64 = conn.pragma_query_value(None, "application_id", |r| r.get(0)).map_err(sql_error)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(sql_error)?;
    let mut statement = conn.prepare("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name").map_err(sql_error)?;
    let schema: Vec<(String, String)> = statement.query_map([], |r| Ok((r.get(0)?,r.get(1)?)))
        .map_err(sql_error)?.collect::<rusqlite::Result<_>>().map_err(sql_error)?;
    if app == 0 && version == 0 && schema.is_empty() { return Ok(SchemaState::Empty); }
    if app != APPLICATION_ID || version != SCHEMA_VERSION { return Err(unavailable()); }
    let expected = vec![
        ("clips".to_string(), CLIPS_SQL.to_string()),
        ("clips_recent".to_string(), RECENT_INDEX_SQL.to_string()),
        ("payloads".to_string(), PAYLOADS_SQL.to_string()),
        ("sqlite_sequence".to_string(), "CREATE TABLE sqlite_sequence(name,seq)".to_string()),
    ];
    if schema != expected { return Err(unavailable()); }
    if conn.prepare("PRAGMA foreign_key_check").map_err(sql_error)?.exists([]).map_err(sql_error)? {
        return Err(unavailable());
    }
    let inconsistent: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM clips c WHERE id <= 0 OR payload_bytes !=
         (SELECT coalesce(sum(byte_size),0) FROM payloads WHERE clip_id=c.id)
         OR NOT EXISTS(SELECT 1 FROM payloads WHERE clip_id=c.id))", [], |r| r.get(0),
    ).map_err(sql_error)?;
    if inconsistent { return Err(unavailable()); }
    let usage = read_usage(conn)?;
    if usage.items > MAX_HISTORY_ITEMS as i64 || usage.payload_bytes > MAX_HISTORY_PAYLOAD_BYTES as i64 {
        return Err(unavailable());
    }
    Ok(SchemaState::Current)
}

fn read_usage(conn: &Connection) -> ContractResult<HistoryUsage> {
    conn.query_row("SELECT count(*),coalesce(sum(payload_bytes),0) FROM clips", [], |r| {
        Ok(HistoryUsage { items: r.get(0)?, payload_bytes: r.get(1)? })
    }).map_err(sql_error)
}

/// A missing or zero-byte main file is a genuinely new target only when the
/// name has no sidecars at all. Leftover WAL/journal/shm files are preserved
/// evidence of an interrupted or unknown owner: never absorb, reset or delete
/// them by creating a fresh database over the scene.
fn ensure_no_leftover_sidecars(path: &Path) -> ContractResult<()> {
    for suffix in ["-wal", "-journal", "-shm"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(suffix);
        match std::fs::metadata(Path::new(&candidate)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(unavailable()),
        }
    }
    Ok(())
}

/// SQLite's normal read-only WAL opener can create shared-memory files. Freeze
/// the actual Windows files against writes/replacement first; then use SQLite's
/// built-in no-lock VFS with a private WAL index. This is never used for writes.
#[cfg(windows)]
fn preflight(path: &Path) -> ContractResult<SchemaState> {
    use std::os::windows::fs::OpenOptionsExt;
    fn freeze(path: &Path) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new().read(true)
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ).open(path)
    }
    let _main = freeze(path).map_err(|_| unavailable())?;
    let mut wal_path = path.as_os_str().to_os_string();
    wal_path.push("-wal");
    let wal = match freeze(Path::new(&wal_path)) {
        Ok(file) => Some(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(unavailable()),
    };
    let mut journal_path = path.as_os_str().to_os_string();
    journal_path.push("-journal");
    // A hot rollback journal needs a separately authorized recovery path. Do
    // not ignore it and inspect a possibly half-written main file as immutable.
    match std::fs::metadata(Path::new(&journal_path)) {
        Ok(meta) if meta.len() != 0 => return Err(unavailable()),
        Ok(_) => {},
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(_) => return Err(unavailable()),
    }
    let has_wal = wal.as_ref().map(|f| f.metadata().map(|m| m.len() > 0))
        .transpose().map_err(|_| unavailable())?.unwrap_or(false);
    let mut uri = tauri::Url::from_file_path(path).map_err(|_| unavailable())?;
    uri.set_query(Some(if has_wal { "mode=ro&vfs=win32-none" } else { "mode=ro&immutable=1" }));
    let conn = Connection::open_with_flags(uri.as_str(), OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).map_err(sql_error)?;
    conn.busy_timeout(Duration::from_millis(250)).map_err(sql_error)?;
    if has_wal { conn.pragma_update(None, "locking_mode", "EXCLUSIVE").map_err(sql_error)?; }
    let state = check_database(&conn)?;
    // Drop SQLite before the frozen handles; it cannot checkpoint this RO WAL.
    drop(conn);
    Ok(state)
}

#[cfg(not(windows))]
fn preflight(_path: &Path) -> ContractResult<SchemaState> {
    // File storage is currently a Windows-only first-iteration interface.
    Err(unavailable())
}

/// Initialization is only for a genuinely new target: a missing or zero-byte
/// file with no sidecars, or an ephemeral in-memory database. An existing
/// non-empty file is never adopted, even when its schema is empty (e.g. after
/// DROP TABLE); logical emptiness is not permission to take over.
enum InitPermit<'p> {
    Memory,
    FreshFile(&'p Path),
    Denied,
}

fn initialize_or_check(conn: &mut Connection, permit: InitPermit<'_>) -> ContractResult<()> {
    conn.busy_timeout(Duration::from_millis(250)).map_err(sql_error)?;
    conn.pragma_update(None, "foreign_keys", true).map_err(sql_error)?;
    conn.pragma_update(None, "synchronous", "FULL").map_err(sql_error)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
    // Re-check under the write lock. A preflight result must not authorize DDL
    // against a database another opener initialized in the meantime.
    if check_database(&tx)? == SchemaState::Empty {
        match permit {
            InitPermit::Denied => return Err(unavailable()),
            InitPermit::Memory => {}
            InitPermit::FreshFile(path) => {
                // Fresh length under the write lock, never the open-time
                // length: a concurrent creator's non-empty file must not be
                // taken over on a stale "it was new" observation.
                match std::fs::metadata(path) {
                    Ok(meta) if meta.is_file() && meta.len() == 0 => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return Err(unavailable()),
                }
            }
        }
        tx.execute_batch(CLIPS_SQL).map_err(sql_error)?;
        tx.execute_batch(PAYLOADS_SQL).map_err(sql_error)?;
        tx.execute_batch(RECENT_INDEX_SQL).map_err(sql_error)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID).map_err(sql_error)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(sql_error)?;
    }
    tx.commit().map_err(sql_error)
}

fn validate_snapshot(snapshot: &Snapshot) -> ContractResult<(Option<String>, i64)> {
    let invalid = || ContractError::invalid_input("invalid history snapshot");
    if snapshot.capture_id.is_empty() || snapshot.capture_id.len() > 64
        || !snapshot.capture_id.bytes().all(|b| b.is_ascii_graphic())
        || !(0..=JS_SAFE_INTEGER_MAX).contains(&snapshot.captured_at_ms)
        || snapshot.payloads.is_empty() || snapshot.payloads.len() > 3 {
        return Err(invalid());
    }
    if (snapshot.source_confidence == SourceConfidence::Owner) != snapshot.source_process.is_some() {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    let mut text = None;
    for payload in &snapshot.payloads {
        if !names.insert(payload.format_name.as_str()) { return Err(invalid()); }
        let limit = match payload.format_name.as_str() {
            "CF_UNICODETEXT" | "HTML Format" => MAX_TEXT_FORMAT_BYTES,
            "PNG" => MAX_PNG_BYTES,
            _ => return Err(invalid()),
        };
        if payload.data.len() > limit { return Err(invalid()); }
        total = total.checked_add(payload.data.len()).ok_or_else(invalid)?;
        match payload.format_name.as_str() {
            "CF_UNICODETEXT" => {
                if payload.data.len() % 2 != 0 { return Err(invalid()); }
                let mut units = Vec::with_capacity(payload.data.len() / 2);
                let mut terminated = false;
                for pair in payload.data.chunks_exact(2) {
                    let unit = u16::from_le_bytes([pair[0],pair[1]]);
                    if unit == 0 { terminated = true; break; }
                    units.push(unit);
                }
                if !terminated { return Err(invalid()); }
                text = Some(String::from_utf16(&units).map_err(|_| invalid())?);
            }
            "PNG" if !payload.data.starts_with(b"\x89PNG\r\n\x1a\n") => return Err(invalid()),
            _ => {}
        }
    }
    if total > MAX_PAYLOAD_TOTAL_BYTES { return Err(invalid()); }
    Ok((text, total as i64))
}

impl Store {
    /// Existing files are inspected read-only before changing journal mode or
    /// starting a write transaction. Opening failure never deletes/rebuilds them.
    pub fn open(path: &Path) -> ContractResult<Self> {
        if !path.is_absolute() { return Err(ContractError::invalid_input("history path must be absolute")); }
        let permit = match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => {
                if meta.len() == 0 {
                    // A zero-byte main file may initialize only as a truly new
                    // target; same-name sidecars mark an interrupted owner's scene.
                    ensure_no_leftover_sidecars(path)?;
                    if preflight(path)? != SchemaState::Empty {
                        return Err(unavailable());
                    }
                    InitPermit::FreshFile(path)
                } else {
                    // An existing non-empty target must already be ours at the
                    // read-only stage: an unknown or schema-less database (even
                    // one whose WAL holds the DROP) is rejected before any RW
                    // connection, which would checkpoint its WAL on close and
                    // rewrite the very scene we are refusing to adopt.
                    if preflight(path)? != SchemaState::Current {
                        return Err(unavailable());
                    }
                    InitPermit::Denied
                }
            }
            Ok(_) => return Err(unavailable()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ensure_no_leftover_sidecars(path)?;
                InitPermit::FreshFile(path)
            }
            Err(_) => return Err(unavailable()),
        };
        let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE).map_err(sql_error)?;
        initialize_or_check(&mut conn, permit)?;
        let journal: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0)).map_err(sql_error)?;
        if !journal.eq_ignore_ascii_case("wal") { return Err(unavailable()); }
        Ok(Self::from_connection(conn))
    }

    /// Explicitly ephemeral storage for rule tests; it never resolves a user path.
    pub fn open_in_memory() -> ContractResult<Self> {
        let mut conn = Connection::open_in_memory().map_err(sql_error)?;
        initialize_or_check(&mut conn, InitPermit::Memory)?;
        Ok(Self::from_connection(conn))
    }

    fn from_connection(conn: Connection) -> Self {
        Self { conn, _owner: PhantomData, #[cfg(test)] limits: (MAX_HISTORY_ITEMS as i64, MAX_HISTORY_PAYLOAD_BYTES as i64) }
    }

    pub fn insert(&mut self, snapshot: &Snapshot) -> ContractResult<InsertResult> {
        let (text, bytes) = validate_snapshot(snapshot)?;
        let preview = text.as_deref().unwrap_or("").chars().take(MAX_PREVIEW_SCALARS).collect::<String>();
        let omitted = serde_json::to_string(&snapshot.omitted_formats).map_err(|_| unavailable())?;
        #[cfg(not(test))]
        let limits = (MAX_HISTORY_ITEMS as i64, MAX_HISTORY_PAYLOAD_BYTES as i64);
        #[cfg(test)]
        let limits = self.limits;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
        let existing: Option<i64> = tx.query_row("SELECT id FROM clips WHERE capture_id=?1", [&snapshot.capture_id], |r| r.get(0))
            .optional().map_err(sql_error)?;
        let usage = read_usage(&tx)?;
        if let Some(clip_id) = existing {
            tx.commit().map_err(sql_error)?;
            return Ok(InsertResult { clip_id, inserted: false, usage });
        }
        if usage.items >= limits.0 || usage.payload_bytes.checked_add(bytes).filter(|sum| *sum <= limits.1).is_none() {
            return Err(ContractError::new(ErrorCode::CapacityExceeded, "history capacity reached"));
        }
        let source = match snapshot.source_confidence { SourceConfidence::Owner => "owner", SourceConfidence::Unknown => "unknown" };
        tx.execute("INSERT INTO clips(capture_id,captured_at_ms,text_full,preview,source_process,source_confidence,omitted_formats_json,payload_bytes)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![snapshot.capture_id,snapshot.captured_at_ms,text,preview,snapshot.source_process,source,omitted,bytes]).map_err(sql_error)?;
        let clip_id = tx.last_insert_rowid();
        for payload in &snapshot.payloads {
            tx.execute("INSERT INTO payloads(clip_id,format_name,encoding_version,byte_size,data) VALUES (?1,?2,1,?3,?4)",
                params![clip_id,payload.format_name,payload.data.len() as i64,payload.data]).map_err(sql_error)?;
        }
        let usage = read_usage(&tx)?;
        tx.commit().map_err(sql_error)?;
        Ok(InsertResult { clip_id, inserted: true, usage })
    }

    pub fn usage(&self) -> ContractResult<HistoryUsage> { read_usage(&self.conn) }

    #[cfg(feature="test-fixtures")]
    pub fn set_test_commit_fault(&mut self,enabled:bool)->ContractResult<()> {
        self.conn.execute_batch(if enabled{"CREATE TABLE runtime_fault(parent_id INTEGER REFERENCES clips(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER runtime_fault_insert AFTER INSERT ON clips BEGIN INSERT INTO runtime_fault VALUES(-1); END;"}else{"DROP TRIGGER IF EXISTS runtime_fault_insert; DROP TABLE IF EXISTS runtime_fault;"}).map_err(sql_error)
    }

    /// E06 recent summaries; BLOBs never cross this listing boundary.
    pub fn list_recent(&self,request:&crate::contracts::ListClipsRequest)->ContractResult<crate::contracts::ListClipsResult>{
        use crate::contracts::{parse_list_clips_request,parse_list_clips_result,ClipSummary,ListCursor,ListClipsResult,positive_id_as_i64,MAX_PAGE_JSON_BYTES};
        let request=parse_list_clips_request(&serde_json::json!({"request_id":request.request_id,"query":request.query,"limit":request.limit,"cursor":request.cursor}))?;
        if !request.query.is_empty(){return Err(ContractError::invalid_input("query must be empty"));}
        let tx=self.conn.unchecked_transaction().map_err(sql_error)?;
        let upper=if let Some(c)=&request.cursor{positive_id_as_i64(&c.upper_id)}else{tx.query_row("SELECT coalesce(max(id),0) FROM clips",[],|r|r.get(0)).map_err(sql_error)?};
        let time=request.cursor.as_ref().map(|c|c.last_captured_at_ms);
        let last=request.cursor.as_ref().map(|c|positive_id_as_i64(&c.last_id)).unwrap_or(0);
        let mut result=ListClipsResult{request_id:request.request_id,normalized_query:String::new(),summaries:vec![],next_cursor:None};
        let mut statement=tx.prepare("SELECT c.id,c.captured_at_ms,c.preview,c.source_process,c.source_confidence,c.omitted_formats_json,c.payload_bytes,
            (SELECT group_concat(format_name,char(31)) FROM (SELECT format_name FROM payloads WHERE clip_id=c.id ORDER BY format_name)),
            c.text_full IS NOT NULL,
            CASE WHEN lower(substr(c.text_full,1,7))='http://' OR lower(substr(c.text_full,1,8))='https://' THEN c.text_full ELSE NULL END
            FROM clips c WHERE c.id<=?1 AND (?2 IS NULL OR c.captured_at_ms<?2 OR (c.captured_at_ms=?2 AND c.id<?3))
            ORDER BY c.captured_at_ms DESC,c.id DESC LIMIT ?4").map_err(sql_error)?;
        let mut rows=statement.query(params![upper,time,last,request.limit+1]).map_err(sql_error)?;
        while let Some(row)=rows.next().map_err(sql_error)? {
            if result.summaries.len()>=request.limit as usize {break;}
            let id:i64=row.get(0).map_err(sql_error)?;let at:i64=row.get(1).map_err(sql_error)?;
            let formats:String=row.get(7).map_err(sql_error)?;
            let formats:Vec<String>=formats.split('\u{1f}').map(str::to_owned).collect();
            let has_text:bool=row.get(8).map_err(sql_error)?;
            let candidate:Option<String>=row.get(9).map_err(sql_error)?;
            let link=candidate.as_ref().is_some_and(|s|!s.chars().any(char::is_whitespace)&&tauri::Url::parse(s).is_ok_and(|u|matches!(u.scheme(),"http"|"https")&&u.host_str().is_some()));
            let omitted:String=row.get(5).map_err(sql_error)?;
            let omitted:Vec<String>=serde_json::from_str(&omitted).map_err(|_|unavailable())?;
            let bytes:i64=row.get(6).map_err(sql_error)?;
            let summary=ClipSummary{id:id.to_string(),captured_at_ms:at,kind:if link{"link"}else if !has_text&&formats.iter().any(|n|n=="PNG"){"png"}else{"text"}.into(),preview:row.get(2).map_err(sql_error)?,format_names:formats,omitted_format_count:omitted.len() as u64,payload_bytes:u64::try_from(bytes).map_err(|_|unavailable())?,source_process:row.get(3).map_err(sql_error)?,source_confidence:row.get(4).map_err(sql_error)?};
            result.summaries.push(summary);
            let current=result.summaries.last().unwrap();
            result.next_cursor=Some(ListCursor{normalized_query:String::new(),upper_id:upper.to_string(),last_captured_at_ms:current.captured_at_ms,last_id:current.id.clone()});
            if serde_json::to_vec(&result).map_err(|_|unavailable())?.len()>MAX_PAGE_JSON_BYTES {
                result.summaries.pop();
                if result.summaries.is_empty(){return Err(unavailable());}
                break;
            }
        }
        // Probe one row beyond the last emitted key, including budget truncation.
        result.next_cursor=if let Some(last)=result.summaries.last(){
            let more:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM clips WHERE id<=?1 AND (captured_at_ms<?2 OR (captured_at_ms=?2 AND id<?3)))",params![upper,last.captured_at_ms,positive_id_as_i64(&last.id)],|r|r.get(0)).map_err(sql_error)?;
            if more{Some(ListCursor{normalized_query:String::new(),upper_id:upper.to_string(),last_captured_at_ms:last.captured_at_ms,last_id:last.id.clone()})}else{None}
        }else{None};
        drop(rows);drop(statement);tx.commit().map_err(sql_error)?;
        parse_list_clips_result(&serde_json::to_value(&result).map_err(|_|unavailable())?).map_err(|_|unavailable())
    }

    /// Internal full record retrieval. This is not a WebView command or DTO;
    /// E08 will provide bounded summaries and text chunks for the UI.
    pub fn get(&self, clip_id: i64) -> ContractResult<Option<StoredClip>> {
        if clip_id <= 0 { return Err(ContractError::invalid_input("invalid history id")); }
        let tx = self.conn.unchecked_transaction().map_err(sql_error)?;
        let record = tx.query_row("SELECT capture_id,captured_at_ms,text_full,preview,source_process,source_confidence,omitted_formats_json,payload_bytes
            FROM clips WHERE id=?1", [clip_id], |r| {
            Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?,r.get::<_, Option<String>>(2)?,r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,r.get::<_, String>(5)?,r.get::<_, String>(6)?,r.get::<_, i64>(7)?))
        }).optional().map_err(sql_error)?;
        let Some((capture_id,captured_at_ms,text_full,preview,source_process,confidence,omitted,payload_bytes)) = record else { return Ok(None); };
        let source_confidence = match confidence.as_str() {
            "owner" => SourceConfidence::Owner, "unknown" => SourceConfidence::Unknown, _ => return Err(unavailable()),
        };
        let omitted_formats = serde_json::from_str(&omitted).map_err(|_| unavailable())?;
        let payloads = {
            let mut statement = tx.prepare("SELECT format_name,data FROM payloads WHERE clip_id=?1 ORDER BY format_name").map_err(sql_error)?;
            let rows = statement.query_map([clip_id], |r| Ok(Payload { format_name: r.get(0)?, data: r.get(1)? })).map_err(sql_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_error)?
        };
        tx.commit().map_err(sql_error)?;
        Ok(Some(StoredClip { clip_id, snapshot: Snapshot { capture_id,captured_at_ms,source_process,source_confidence,omitted_formats,payloads },text_full,preview,payload_bytes }))
    }

    /// Called only for explicitly selected test records in the first iteration.
    pub fn delete(&mut self, clip_id: i64) -> ContractResult<bool> {
        if clip_id <= 0 { return Err(ContractError::invalid_input("invalid history id")); }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
        let deleted = tx.execute("DELETE FROM clips WHERE id=?1", [clip_id]).map_err(sql_error)? > 0;
        tx.commit().map_err(sql_error)?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> Snapshot {
        Snapshot {
            capture_id: id.into(), captured_at_ms: 1,
            source_process: None, source_confidence: SourceConfidence::Unknown,
            omitted_formats: vec![],
            payloads: vec![
                Payload { format_name: "CF_UNICODETEXT".into(), data: vec![65,0,0,0] },
                Payload { format_name: "HTML Format".into(), data: b"<b>A</b>".to_vec() },
            ],
        }
    }

    fn assert_empty_and_consistent(store: &Store) {
        assert_eq!(store.usage().unwrap(), HistoryUsage { items: 0, payload_bytes: 0 });
        assert_eq!(store.conn.query_row("SELECT count(*) FROM payloads", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert!(!store.conn.prepare("PRAGMA foreign_key_check").unwrap().exists([]).unwrap());
        assert_eq!(store.conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
    }

    #[test]
    fn sqlite_failures_after_metadata_before_second_payload_and_at_commit_roll_back_everything() {
        for fault in ["metadata", "second-payload", "commit"] {
            let mut store = Store::open_in_memory().unwrap();
            // These fixtures fail at real SQLite statement/COMMIT boundaries.
            // RAISE(FAIL) intentionally leaves a statement's earlier changes,
            // so only rollback of our enclosing transaction prevents a half row.
            let sql = match fault {
                "metadata" => "CREATE TRIGGER fail_insert AFTER INSERT ON clips BEGIN SELECT RAISE(FAIL,'private-sentinel'); END;",
                "second-payload" => "CREATE TRIGGER fail_insert BEFORE INSERT ON payloads WHEN NEW.format_name='HTML Format' BEGIN SELECT RAISE(FAIL,'private-sentinel'); END;",
                _ => "CREATE TABLE deferred_child(parent_id INTEGER REFERENCES clips(id) DEFERRABLE INITIALLY DEFERRED);
                    CREATE TRIGGER fail_insert AFTER INSERT ON clips BEGIN INSERT INTO deferred_child VALUES(-1); END;",
            };
            store.conn.execute_batch(sql).unwrap();
            let error = store.insert(&sample("fails")).unwrap_err();
            assert_eq!(error.code, ErrorCode::StorageUnavailable, "{fault}");
            assert!(!error.to_json().contains("private-sentinel"));
            assert!(store.conn.is_autocommit(), "{fault} left an open transaction");
            assert_empty_and_consistent(&store);
            if fault == "commit" {
                assert_eq!(store.conn.query_row("SELECT count(*) FROM deferred_child", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
            }
            store.conn.execute_batch("DROP TRIGGER fail_insert").unwrap();
            assert!(store.insert(&sample("fails")).unwrap().inserted, "retry after {fault}");
        }
    }

    #[test]
    fn capacity_boundaries_and_idempotent_retry_at_full_capacity() {
        let mut store = Store::open_in_memory().unwrap();
        let bytes = sample("a").payloads.iter().map(|p| p.data.len() as i64).sum::<i64>();
        store.limits = (2, bytes * 2);
        let first = store.insert(&sample("a")).unwrap();
        let second = store.insert(&sample("b")).unwrap();
        assert_eq!(second.usage, HistoryUsage { items: 2, payload_bytes: bytes * 2 });
        assert_eq!(store.insert(&sample("c")).unwrap_err().code, ErrorCode::CapacityExceeded);
        let retry = store.insert(&sample("a")).unwrap();
        assert!(!retry.inserted);
        assert_eq!(retry.clip_id, first.clip_id);
        assert_eq!(retry.usage, second.usage);
        assert!(store.delete(first.clip_id).unwrap());
        assert!(store.insert(&sample("c")).unwrap().inserted);
        // Separate byte pressure from item pressure, including one byte short.
        let mut bytes_only = Store::open_in_memory().unwrap();
        bytes_only.limits = (100, bytes * 2 - 1);
        bytes_only.insert(&sample("a")).unwrap();
        assert_eq!(bytes_only.insert(&sample("b")).unwrap_err().code, ErrorCode::CapacityExceeded);
        assert_eq!(bytes_only.usage().unwrap(), first.usage);
        let mut items_only = Store::open_in_memory().unwrap();
        items_only.limits = (1, i64::MAX);
        items_only.insert(&sample("a")).unwrap();
        assert_eq!(items_only.insert(&sample("b")).unwrap_err().code, ErrorCode::CapacityExceeded);
    }

    #[test]
    fn connection_settings_and_schema_constraints_are_effective() {
        let mut store = Store::open_in_memory().unwrap();
        for (name, value) in [("foreign_keys",1), ("synchronous",2), ("busy_timeout",250), ("user_version",SCHEMA_VERSION), ("application_id",APPLICATION_ID)] {
            assert_eq!(store.conn.pragma_query_value(None, name, |r| r.get::<_, i64>(0)).unwrap(), value, "{name}");
        }
        let result = store.insert(&sample("constraints")).unwrap();
        assert!(store.conn.execute("UPDATE payloads SET byte_size=0 WHERE clip_id=?1", [result.clip_id]).is_err());
        assert!(store.conn.execute("UPDATE payloads SET encoding_version=2 WHERE clip_id=?1", [result.clip_id]).is_err());
        assert!(store.conn.execute("INSERT INTO payloads VALUES (-1,'PNG',1,0,x'')", []).is_err());
        assert!(store.conn.execute("INSERT INTO payloads SELECT * FROM payloads", []).is_err());
        assert!(store.delete(result.clip_id).unwrap());
        assert_empty_and_consistent(&store);
    }

    #[test]
    fn initialization_rechecks_schema_under_the_write_lock() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert!(check_database(&conn).unwrap() == SchemaState::Empty);
        conn.execute_batch("CREATE TABLE another_owner(value TEXT)").unwrap();
        assert_eq!(initialize_or_check(&mut conn, InitPermit::Memory).unwrap_err().code, ErrorCode::StorageUnavailable);
        assert_eq!(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert!(!conn.table_exists(None, "clips").unwrap());
        assert!(conn.table_exists(None, "another_owner").unwrap());
    }

    #[test]
    fn schema_less_nonempty_file_is_denied_initialization() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE unrelated(secret TEXT); INSERT INTO unrelated VALUES ('sentinel'); DROP TABLE unrelated;").unwrap();
        assert!(check_database(&conn).unwrap() == SchemaState::Empty);
        let deny = initialize_or_check(&mut conn, InitPermit::Denied).unwrap_err();
        assert_eq!(deny.code, ErrorCode::StorageUnavailable);
        assert_eq!(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert!(!conn.table_exists(None, "clips").unwrap());
    }
}
