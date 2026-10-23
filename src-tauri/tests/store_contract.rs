//! Real SQLite storage entry points; file databases belong only to this test run.
use pinfeather_lib::contracts::{
    ErrorCode, JS_SAFE_INTEGER_MAX, MAX_PNG_BYTES, MAX_TEXT_FORMAT_BYTES,
};
use pinfeather_lib::store::{Payload, Snapshot, SourceConfidence, Store};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn text_payload(text: &str) -> Payload {
    Payload {
        format_name: "CF_UNICODETEXT".into(),
        data: text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect(),
    }
}

fn snapshot(id: &str, text: &str) -> Snapshot {
    Snapshot {
        capture_id: id.into(),
        captured_at_ms: 1_700_000_000_000,
        source_process: Some("C:\\Synthetic\\editor.exe".into()),
        source_confidence: SourceConfidence::Owner,
        omitted_formats: vec!["Rich Text Format".into()],
        payloads: vec![text_payload(text)],
    }
}

fn file_path(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT")
        .expect("file tests require an explicit isolated PINFEATHER_TEST_DATA_ROOT"));
    assert!(root.is_absolute());
    let dir = root.join(format!("store-{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&dir).expect("unique test-owned directory");
    dir.join("历史.sqlite3")
}

fn side_path(path: &Path, suffix: &str) -> PathBuf {
    let mut candidate = path.as_os_str().to_os_string();
    candidate.push(suffix);
    PathBuf::from(candidate)
}

/// Full file set of the test-owned directory, names and bytes, for scene checks.
fn scene(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name().to_string_lossy().into_owned(), std::fs::read(entry.path()).unwrap())
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn saves_text_html_as_one_record_with_full_text_and_original_payloads() {
    let mut store = Store::open_in_memory().unwrap();
    let text = format!("{}摘要以外仍需保存", "中😀".repeat(150));
    let mut input = snapshot("capture-1", &text);
    input.payloads.push(Payload { format_name: "HTML Format".into(), data: b"<b>synthetic</b>".to_vec() });
    let result = store.insert(&input).expect("one atomic multi-format insert");
    assert!(result.inserted);
    assert!(result.clip_id > 0);
    assert_eq!(result.usage.items, 1);
    assert_eq!(result.usage.payload_bytes, input.payloads.iter().map(|p| p.data.len() as i64).sum::<i64>());
    let saved = store.get(result.clip_id).unwrap().unwrap();
    assert_eq!(saved.snapshot, input);
    assert_eq!(saved.text_full.as_deref(), Some(text.as_str()));
    assert_eq!(saved.preview, text.chars().take(200).collect::<String>());
    assert_eq!(saved.payload_bytes, result.usage.payload_bytes);
}

#[test]
fn same_capture_is_idempotent_but_same_content_from_another_capture_is_new() {
    let mut store = Store::open_in_memory().unwrap();
    let first = store.insert(&snapshot("first", "相同内容")).unwrap();
    let retry = store.insert(&snapshot("first", "相同内容")).unwrap();
    assert_eq!(retry.clip_id, first.clip_id);
    assert!(!retry.inserted);
    assert_eq!(retry.usage, first.usage);
    store.insert(&snapshot("first", "重试不得覆盖原始内容")).unwrap();
    assert_eq!(store.get(first.clip_id).unwrap().unwrap().text_full.as_deref(), Some("相同内容"));
    let second = store.insert(&snapshot("second", "相同内容")).unwrap();
    assert_ne!(second.clip_id, first.clip_id);
    assert_eq!(second.usage.items, 2);
    assert_eq!(second.usage.payload_bytes, first.usage.payload_bytes * 2);
}

#[test]
fn close_reopen_preserves_data_and_autoincrement_does_not_reuse_deleted_ids() {
    let path = file_path("reopen");
    let input = snapshot("file-1", "中文路径😀");
    let first;
    {
        let mut store = Store::open(&path).unwrap();
        first = store.insert(&input).unwrap();
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.usage().unwrap(), first.usage);
    assert_eq!(store.get(first.clip_id).unwrap().unwrap().snapshot, input);
    assert!(store.delete(first.clip_id).unwrap());
    assert!(!store.delete(first.clip_id).unwrap());
    assert!(store.get(first.clip_id).unwrap().is_none());
    assert_eq!(store.usage().unwrap().items, 0);
    assert_eq!(store.usage().unwrap().payload_bytes, 0);
    let next = store.insert(&snapshot("file-2", "下一条")).unwrap();
    assert!(next.clip_id > first.clip_id);
    drop(store);
    let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM payloads", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
    assert!(!conn.prepare("PRAGMA foreign_key_check").unwrap().exists([]).unwrap());
}

#[test]
fn legal_utf16_payload_can_expand_beyond_two_mib_in_utf8() {
    let text = "中".repeat(MAX_TEXT_FORMAT_BYTES / 2 - 1);
    let input = snapshot("large-text", &text);
    assert_eq!(input.payloads[0].data.len(), MAX_TEXT_FORMAT_BYTES);
    assert!(text.len() > MAX_TEXT_FORMAT_BYTES);
    let mut store = Store::open_in_memory().unwrap();
    let result = store.insert(&input).unwrap();
    assert_eq!(store.get(result.clip_id).unwrap().unwrap().text_full, Some(text));
}

#[test]
fn png_and_html_without_unicode_text_keep_payloads_without_inventing_text() {
    let mut input = snapshot("png", "unused");
    input.payloads = vec![Payload { format_name: "PNG".into(), data: b"\x89PNG\r\n\x1a\nsynthetic".to_vec() }];
    let mut store = Store::open_in_memory().unwrap();
    let result = store.insert(&input).unwrap();
    let saved = store.get(result.clip_id).unwrap().unwrap();
    assert_eq!(saved.text_full, None);
    assert_eq!(saved.preview, "");
    assert_eq!(saved.snapshot, input);
    input.capture_id = "html".into();
    input.payloads = vec![Payload { format_name: "HTML Format".into(), data: b"<script>never execute</script>".to_vec() }];
    let result = store.insert(&input).unwrap();
    assert_eq!(store.get(result.clip_id).unwrap().unwrap().text_full, None);
}

#[test]
fn invalid_formats_encoding_metadata_and_limits_leave_no_record() {
    let base = snapshot("valid", "ok");
    let mut cases = Vec::new();
    let mut bad = base.clone(); bad.capture_id = "bad id".into(); cases.push(bad);
    let mut bad = base.clone(); bad.captured_at_ms = -1; cases.push(bad);
    let mut bad = base.clone(); bad.payloads.clear(); cases.push(bad);
    let mut bad = base.clone(); bad.payloads.push(bad.payloads[0].clone()); cases.push(bad);
    for bytes in [vec![0], vec![65, 0], vec![0, 216, 0, 0]] {
        let mut bad = base.clone(); bad.payloads[0].data = bytes; cases.push(bad);
    }
    let mut bad = base.clone(); bad.payloads[0].data.resize(MAX_TEXT_FORMAT_BYTES + 2, 0); cases.push(bad);
    let mut bad = base.clone(); bad.payloads[0] = Payload { format_name:"HTML Format".into(),data:vec![0;MAX_TEXT_FORMAT_BYTES+1] }; cases.push(bad);
    let mut bad = base.clone(); bad.payloads[0] = Payload { format_name:"PNG".into(),data:vec![0;8*1024*1024+1] }; cases.push(bad);
    let mut bad = base.clone(); bad.payloads[0].format_name = "CF_BITMAP".into(); cases.push(bad);
    let mut bad = base.clone(); bad.payloads[0] = Payload {format_name:"PNG".into(),data:b"not png".to_vec()}; cases.push(bad);
    let mut store = Store::open_in_memory().unwrap();
    for bad in cases {
        let error = store.insert(&bad).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(!error.message.contains("bad id"));
        assert_eq!(store.usage().unwrap().items, 0);
    }
    store.insert(&base).unwrap();
}

#[test]
fn text_ends_at_first_nul_but_original_allocation_is_preserved() {
    let mut input = snapshot("padding", "正文");
    input.payloads[0].data.extend_from_slice(&[0, 216, 255, 255]);
    let mut store = Store::open_in_memory().unwrap();
    let result = store.insert(&input).unwrap();
    let saved = store.get(result.clip_id).unwrap().unwrap();
    assert_eq!(saved.text_full.as_deref(), Some("正文"));
    assert_eq!(saved.snapshot, input);
}

#[test]
fn unknown_future_tampered_and_corrupt_databases_are_rejected_without_modification() {
    for kind in ["unknown", "future", "wrong-app", "schema-drift", "corrupt"] {
        let path = file_path(kind);
        if kind == "corrupt" {
            std::fs::write(&path, b"synthetic not a database").unwrap();
        } else if kind == "unknown" {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE unrelated(secret TEXT); INSERT INTO unrelated VALUES ('sentinel');").unwrap();
        } else {
            drop(Store::open(&path).unwrap());
            let conn = rusqlite::Connection::open(&path).unwrap();
            match kind {
                "future" => conn.execute_batch("PRAGMA user_version=2").unwrap(),
                "wrong-app" => conn.execute_batch("PRAGMA application_id=1234").unwrap(),
                _ => conn.execute_batch("CREATE TRIGGER unexpected AFTER INSERT ON clips BEGIN SELECT 1; END;").unwrap(),
            }
        }
        let before = std::fs::read(&path).unwrap();
        let files_before: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        let error = match Store::open(&path) { Ok(_) => panic!("must reject {kind}"), Err(e) => e };
        assert_eq!(error.code, ErrorCode::StorageUnavailable);
        assert_eq!(std::fs::read(&path).unwrap(), before, "{kind} bytes changed");
        let files_after: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files_after, files_before, "{kind} created a sidecar");
    }
}

// Copy only a stable synthetic database and its committed WAL, while the source
// owner is idle. This models an interrupted process without killing any app.
fn wal_sample(future: bool) -> (PathBuf, i64) {
    let source = file_path("wal-source");
    let target = file_path(if future { "wal-future" } else { "wal-recover" });
    let mut store = Store::open(&source).unwrap();
    let result = store.insert(&snapshot("wal-event", "已提交的 WAL 正文")).unwrap();
    if future {
        let conn = rusqlite::Connection::open(&source).unwrap();
        conn.execute_batch("PRAGMA user_version=2").unwrap();
    }
    std::fs::copy(&source, &target).unwrap();
    let mut source_wal = source.as_os_str().to_os_string(); source_wal.push("-wal");
    let mut target_wal = target.as_os_str().to_os_string(); target_wal.push("-wal");
    std::fs::copy(source_wal, target_wal).unwrap();
    drop(store);
    (target, result.clip_id)
}

#[test]
fn committed_wal_is_read_and_recovered_after_an_interrupted_owner() {
    let (path, id) = wal_sample(false);
    let store = Store::open(&path).expect("committed WAL must remain readable");
    assert_eq!(store.get(id).unwrap().unwrap().text_full.as_deref(), Some("已提交的 WAL 正文"));
    assert_eq!(store.usage().unwrap().items, 1);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.usage().unwrap().items, 1);
}

#[test]
fn version_change_in_wal_is_rejected_without_writing_or_creating_sidecars() {
    let (path, _) = wal_sample(true);
    let files = || {
        let mut files: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| {
            let entry = e.unwrap(); (entry.file_name(), std::fs::read(entry.path()).unwrap())
        }).collect();
        files.sort_by(|a,b| a.0.cmp(&b.0));
        files
    };
    let before = files();
    let error = match Store::open(&path) { Ok(_) => panic!("must reject WAL version 2"), Err(e) => e };
    assert_eq!(error.code, ErrorCode::StorageUnavailable);
    assert_eq!(files(), before);
}

#[test]
fn per_format_byte_limits_with_valid_content_pin_the_size_check() {
    let mut store = Store::open_in_memory().unwrap();
    let mut html = snapshot("html-at-limit", "");
    html.payloads = vec![Payload { format_name: "HTML Format".into(), data: vec![b'x'; MAX_TEXT_FORMAT_BYTES] }];
    assert!(store.insert(&html).unwrap().inserted);
    let mut over = snapshot("html-over-limit", "");
    over.payloads = vec![Payload { format_name: "HTML Format".into(), data: vec![b'x'; MAX_TEXT_FORMAT_BYTES + 1] }];
    assert_eq!(store.insert(&over).unwrap_err().code, ErrorCode::InvalidInput);
    // Valid PNG signature so only the byte limit, not content validation, can reject.
    let mut png = vec![b'x'; MAX_PNG_BYTES];
    png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut at = snapshot("png-at-limit", "");
    at.payloads = vec![Payload { format_name: "PNG".into(), data: png.clone() }];
    assert!(store.insert(&at).unwrap().inserted);
    png.push(b'x');
    let mut over = snapshot("png-over-limit", "");
    over.payloads = vec![Payload { format_name: "PNG".into(), data: png }];
    assert_eq!(store.insert(&over).unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(store.usage().unwrap().items, 2);
}

#[test]
fn capture_id_and_timestamp_bounds_accept_edge_and_reject_beyond() {
    let mut store = Store::open_in_memory().unwrap();
    let mut at_edge = snapshot(&"c".repeat(64), "边界");
    at_edge.captured_at_ms = JS_SAFE_INTEGER_MAX;
    assert!(store.insert(&at_edge).unwrap().inserted);
    let long_id = snapshot(&"c".repeat(65), "过长");
    assert_eq!(store.insert(&long_id).unwrap_err().code, ErrorCode::InvalidInput);
    let mut over_time = snapshot("time-over", "超时");
    over_time.captured_at_ms = JS_SAFE_INTEGER_MAX + 1;
    assert_eq!(store.insert(&over_time).unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(store.usage().unwrap().items, 1);
}

#[test]
fn all_allowed_formats_at_their_exact_limits_preserve_twelve_mib() {
    use pinfeather_lib::contracts::MAX_PAYLOAD_TOTAL_BYTES;
    let mut input = snapshot("all-limits", "");
    input.payloads[0].data.resize(MAX_TEXT_FORMAT_BYTES,0);
    input.payloads.push(Payload {format_name:"HTML Format".into(),data:vec![0;MAX_TEXT_FORMAT_BYTES]});
    let mut png = vec![0;MAX_PNG_BYTES]; png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    input.payloads.push(Payload {format_name:"PNG".into(),data:png});
    let mut store = Store::open_in_memory().unwrap();
    let result = store.insert(&input).unwrap();
    assert_eq!(result.usage.payload_bytes, MAX_PAYLOAD_TOTAL_BYTES as i64);
    assert_eq!(store.get(result.clip_id).unwrap().unwrap().snapshot, input);
}

#[test]
fn config_names_history_without_opening_it_and_file_connection_uses_wal() {
    let test_path = file_path("config");
    let prepared = pinfeather_lib::config::prepare_data_dir(test_path.parent().unwrap()).unwrap();
    assert_eq!(prepared.history_path, prepared.data_dir.join("history.sqlite3"));
    assert!(!prepared.history_path.exists());
    let store = Store::open(&prepared.history_path).unwrap();
    let conn = rusqlite::Connection::open_with_flags(&prepared.history_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(conn.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0)).unwrap(), "wal");
    drop(conn);
    drop(store);
}

#[test]
fn empty_file_is_initializable_and_nonempty_rollback_journal_is_preserved() {
    let path = file_path("empty");
    std::fs::write(&path, []).unwrap();
    drop(Store::open(&path).unwrap());
    let mut journal = path.as_os_str().to_os_string(); journal.push("-journal");
    std::fs::write(&journal,b"synthetic rollback journal").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(&journal).unwrap(),b"synthetic rollback journal");
}

#[test]
fn missing_or_zero_byte_main_with_leftover_sidecar_is_rejected_and_scene_preserved() {
    // A real committed WAL from a synthetic store, as interrupted-owner evidence.
    let source = file_path("sidecar-source");
    let mut store = Store::open(&source).unwrap();
    store.insert(&snapshot("sidecar-event", "已提交 WAL 正文")).unwrap();
    let committed_wal = std::fs::read(side_path(&source, "-wal")).expect("committed WAL exists");
    assert!(!committed_wal.is_empty(), "sample must be a real committed WAL");
    drop(store);
    for zero_main in [false, true] {
        for (suffix, bytes) in [
            ("-wal", committed_wal.clone()),
            ("-journal", b"synthetic-rollback-evidence".to_vec()),
            ("-shm", b"synthetic-shared-index".to_vec()),
            ("-wal", Vec::new()),
            ("-journal", Vec::new()),
            ("-shm", Vec::new()),
        ] {
            let label = format!(
                "sidecar-{}-{}-{}",
                if zero_main { "zero" } else { "missing" },
                &suffix[1..],
                if bytes.is_empty() { "empty" } else { "nonempty" }
            );
            let path = file_path(&label);
            if zero_main {
                std::fs::write(&path, []).unwrap();
            }
            std::fs::write(side_path(&path, suffix), &bytes).unwrap();
            let before = scene(&path);
            let error = match Store::open(&path) {
                Ok(_) => panic!("must reject {label}"),
                Err(error) => error,
            };
            assert_eq!(error.code, ErrorCode::StorageUnavailable, "{label}");
            assert_eq!(scene(&path), before, "{label} scene changed");
        }
    }
}

#[test]
fn nonempty_unknown_database_with_empty_schema_is_rejected_and_scene_preserved() {
    let path = file_path("unknown-empty-schema");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE unrelated(secret TEXT); INSERT INTO unrelated VALUES ('sentinel'); DROP TABLE unrelated;",
        ).unwrap();
    }
    let before = scene(&path);
    let main_len = before.iter().find(|(name, _)| name == "历史.sqlite3").unwrap().1.len();
    assert!(main_len > 0, "fixture must be a non-empty unknown database");
    let error = match Store::open(&path) {
        Ok(_) => panic!("must reject unknown nonempty empty-schema database"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::StorageUnavailable);
    assert_eq!(scene(&path), before);
}

#[test]
fn nonempty_unknown_empty_schema_database_with_committed_wal_is_rejected_and_scene_preserved() {
    // Interrupted unknown owner scene: WAL-mode SQLite with CREATE/INSERT/DROP
    // all committed into the WAL. Copy main+WAL while the writer is still open
    // so no checkpoint-on-close can erase the WAL from the sample.
    let source = file_path("unknown-wal-source");
    let target = file_path("unknown-wal-target");
    {
        let conn = rusqlite::Connection::open(&source).unwrap();
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)).unwrap();
        conn.execute_batch(
            "CREATE TABLE unrelated(secret TEXT); INSERT INTO unrelated VALUES ('sentinel'); DROP TABLE unrelated;",
        ).unwrap();
        std::fs::copy(&source, &target).unwrap();
        std::fs::copy(side_path(&source, "-wal"), side_path(&target, "-wal")).unwrap();
    }
    let before = scene(&target);
    assert_eq!(before.len(), 2, "fixture must be main plus a committed WAL");
    let main_len = before.iter().find(|(name, _)| name == "历史.sqlite3").unwrap().1.len();
    let wal_len = before.iter().find(|(name, _)| name.ends_with("-wal")).unwrap().1.len();
    assert!(main_len > 0, "fixture main must be non-empty");
    assert!(wal_len > 0, "fixture must carry a real committed WAL");
    let error = match Store::open(&target) {
        Ok(_) => panic!("must reject unknown nonempty empty-schema database with WAL"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::StorageUnavailable);
    let after = scene(&target);
    let meta = |entries: &[(String, Vec<u8>)]| -> Vec<(String, usize)> {
        entries.iter().map(|(name, bytes)| (name.clone(), bytes.len())).collect()
    };
    assert_eq!(meta(&after), meta(&before), "scene file set or lengths changed");
    for (name, bytes) in &before {
        let other = after.iter().find(|(n, _)| n == name).unwrap();
        assert!(other.1 == *bytes, "scene bytes changed for {name}");
    }
}
