use super::*;
use crate::startup_capture::{StartupOutcome, FILE_NAME};
use std::path::Path;

const EXCLUSIONS: &[u8] = br#"{"schema_version":1,"excluded_apps":["C:\\Synthetic\\excluded.exe"]}"#;
const LISTENING: &[u8] = br#"{"schema_version":1,"last_manual_state":"listening"}"#;
const PAUSED: &[u8] = br#"{"schema_version":1,"last_manual_state":"paused"}"#;
const STARTUP_TIMEOUT_MESSAGE: &str = "启动时核对历史存储或启动恢复配置超时。片羽保持暂停，请重新启动。";
const ABRUPT_CHILD_DB: &str = "PINFEATHER_STARTUP_ABRUPT_CHILD_DB";

fn directory() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("isolated test root required"));
    assert!(root.is_absolute());
    let dir = root.join(format!("startup-capture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn start(path: &Path, force_paused: bool) -> HostRuntime {
    HostRuntime::create(Target::File(path.to_owned()), path.parent().unwrap().display().to_string(),
        Arc::new(|_, _| {}), false, None, force_paused).unwrap()
}

fn close(runtime: HostRuntime) {
    runtime.shutdown(Duration::from_secs(3)).unwrap();
}

fn saved_state(path: &Path) -> String {
    let raw = std::fs::read(path).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(document["schema_version"], 1);
    document["last_manual_state"].as_str().unwrap().to_owned()
}

#[test]
fn manual_choices_survive_restart_and_leave_legacy_settings_unchanged() {
    let dir = directory();
    let path = dir.join("history.sqlite3");
    let pref = dir.join(FILE_NAME);
    let settings = dir.join("settings.json");
    std::fs::write(&settings, EXCLUSIONS).unwrap();
    let first = start(&path, false);
    assert_eq!(first.status().state, "paused");
    assert_eq!(first.startup_capture().outcome, StartupOutcome::DefaultPaused);
    first.set_enabled(true).unwrap();
    first.set_enabled(false).unwrap();
    assert!(!pref.exists());

    first.set_enabled(true).unwrap();
    assert!(first.set_startup_capture(true).unwrap().enabled);
    assert_eq!(saved_state(&pref), "listening", "opt-in snapshots live capture");
    close(first);

    let remembered = std::fs::read(&pref).unwrap();
    let forced = start(&path, true);
    assert_eq!(forced.status().state, "paused");
    assert_eq!(forced.startup_capture().outcome, StartupOutcome::ForcedPaused);
    close(forced);
    assert_eq!(std::fs::read(&pref).unwrap(), remembered);

    let resumed = start(&path, false);
    assert_eq!(resumed.status().state, "listening");
    assert_eq!(resumed.startup_capture().outcome, StartupOutcome::Restored);
    resumed.set_enabled(false).unwrap();
    close(resumed);
    let paused = start(&path, false);
    assert_eq!(saved_state(&pref), "paused");
    assert_eq!(paused.status().state, "paused");
    assert_eq!(paused.startup_capture().outcome, StartupOutcome::RememberedPaused);
    assert!(!paused.set_startup_capture(false).unwrap().enabled);
    assert_eq!(saved_state(&pref), "disabled");
    paused.set_enabled(true).unwrap();
    close(paused);
    let disabled = start(&path, false);
    assert_eq!(disabled.status().state, "paused");
    assert_eq!(saved_state(&pref), "disabled");
    close(disabled);
    assert_eq!(std::fs::read(&settings).unwrap(), EXCLUSIONS);
    assert_eq!(crate::config::SettingsFile::load(settings).1.unwrap(), vec![r"C:\Synthetic\excluded.exe"]);
}

#[test]
fn malformed_or_unknown_preferences_are_preserved_and_never_restore() {
    for bytes in [
        b"{".as_slice(),
        br#"{"schema_version":2,"last_manual_state":"listening"}"#,
        br#"{"schema_version":1,"last_manual_state":"listening","extra":true}"#,
        br#"{"schema_version":1,"last_manual_state":true}"#,
        br#"{"schema_version":1}"#,
    ] {
        let dir = directory();
        let pref = dir.join(FILE_NAME);
        std::fs::write(&pref, bytes).unwrap();
        let runtime = start(&dir.join("history.sqlite3"), false);
        let startup = runtime.startup_capture();
        assert_eq!(runtime.status().state, "paused");
        assert_eq!(startup.outcome, StartupOutcome::Blocked);
        assert!(startup.blocked && startup.error.is_some());
        assert_eq!(runtime.set_enabled(true).unwrap().state, "listening");
        assert!(runtime.set_startup_capture(true).is_err());
        assert!(runtime.set_startup_capture(false).is_err());
        let unchanged = runtime.startup_capture();
        assert_eq!(unchanged.revision, startup.revision);
        assert!(unchanged.blocked);
        assert_eq!(unchanged.outcome, startup.outcome);
        assert_eq!(unchanged.error, startup.error);
        assert_eq!(std::fs::read(&pref).unwrap(), bytes);
        close(runtime);
    }
}

#[test]
fn external_preference_edit_is_preserved_even_on_live_manual_pause() {
    let dir = directory();
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, LISTENING).unwrap();
    let runtime = start(&dir.join("history.sqlite3"), false);
    assert_eq!(runtime.status().state, "listening");
    let external = br#"{"schema_version":1,"last_manual_state":"listening","external":true}"#;
    std::fs::write(&pref, external).unwrap();
    assert_eq!(runtime.set_enabled(false).unwrap().state, "paused");
    let blocked = runtime.startup_capture();
    assert!(blocked.blocked);
    assert!(blocked.error.unwrap().contains("外部更改"));
    assert!(runtime.set_startup_capture(false).is_err());
    close(runtime);
    assert_eq!(std::fs::read(&pref).unwrap(), external);
    assert!(!std::fs::read_dir(&dir).unwrap().any(|entry|
        entry.unwrap().file_name().to_string_lossy().starts_with(".startup-capture-")));
}

#[test]
fn invalid_exclusions_and_unavailable_storage_do_not_restore_capture() {
    for failure in ["settings.json", "history.sqlite3"] {
        let dir = directory();
        let pref = dir.join(FILE_NAME);
        let db = dir.join("history.sqlite3");
        std::fs::write(&pref, LISTENING).unwrap();
        if failure == "settings.json" { std::fs::write(dir.join(failure), b"{damaged").unwrap(); }
        else { std::fs::create_dir(&db).unwrap(); }
        let runtime = start(&db, false);
        assert!(!runtime.status().capture.admission_open);
        let startup = runtime.startup_capture();
        assert_eq!(startup.outcome, StartupOutcome::Blocked);
        let expected = if failure == "settings.json" { "排除设置" } else { "存储未就绪" };
        assert!(startup.reason.unwrap().contains(expected));
        if failure == "settings.json" { assert_eq!(std::fs::read(dir.join(failure)).unwrap(), b"{damaged"); }
        assert_eq!(std::fs::read(&pref).unwrap(), LISTENING);
        close(runtime);
    }
}

#[test]
fn unavailable_listener_or_full_history_blocks_restoration() {
    for listener_missing in [true, false] {
        let dir = directory();
        std::fs::write(dir.join(FILE_NAME), LISTENING).unwrap();
        let mut runtime = start(&dir.join("history.sqlite3"), true);
        *runtime.shared.startup_capture.lock().unwrap() = StartupCapture::load(Some(&dir), false, crate::config::read_settings);
        if listener_missing {
            runtime.production = true;
        } else {
            runtime.shared.state.lock().unwrap().history.items = MAX_HISTORY_ITEMS as i64;
        }
        runtime.restore_capture_at_startup(Instant::now() + STARTUP_TIMEOUT, verify_startup_file);
        let startup = runtime.startup_capture();
        assert_eq!(startup.outcome, StartupOutcome::Blocked);
        assert!(startup.reason.unwrap().contains(if listener_missing { "监听不可用" } else { "历史容量已满" }));
        assert!(!runtime.status().capture.admission_open);
        close(runtime);
    }
}

fn verify_startup_file(startup: &StartupCapture) -> Option<(StartupCapture, bool)> {
    let mut startup = startup.clone();
    let current = startup.startup_file_current(crate::config::read_settings);
    Some((startup, current))
}

#[test]
fn queued_startup_verification_after_deadline_is_not_accepted() {
    let dir = directory();
    std::fs::write(dir.join(FILE_NAME), LISTENING).unwrap();
    let runtime = start(&dir.join("history.sqlite3"), true);
    *runtime.shared.startup_capture.lock().unwrap() = StartupCapture::load(Some(&dir), false, crate::config::read_settings);
    let entered_policy = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_flag = entered_policy.clone();
    *runtime.shared.before_policy.lock().unwrap() = Some(Arc::new(move || hook_flag.store(true, Ordering::Relaxed)));
    let deadline = Instant::now();

    runtime.restore_capture_at_startup(deadline, |startup| {
        let (tx, rx) = mpsc::sync_channel(1);
        assert!(Instant::now() >= deadline);
        tx.send(verify_startup_file(startup).unwrap()).unwrap();
        rx.recv_timeout(Duration::ZERO).ok()
    });

    let status = runtime.status();
    let startup = runtime.startup_capture();
    close(runtime);
    assert!(!entered_policy.load(Ordering::Relaxed), "an expired response must not reach the capture policy");
    assert!(!status.capture.user_enabled && !status.capture.admission_open);
    assert!(startup.blocked);
    assert_eq!(startup.outcome, StartupOutcome::Blocked);
    assert_eq!(startup.error.as_deref(), Some(STARTUP_TIMEOUT_MESSAGE));
}

#[test]
fn startup_verification_expiring_before_admission_stays_paused() {
    let dir = directory();
    std::fs::write(dir.join(FILE_NAME), LISTENING).unwrap();
    let runtime = start(&dir.join("history.sqlite3"), true);
    *runtime.shared.startup_capture.lock().unwrap() = StartupCapture::load(Some(&dir), false, crate::config::read_settings);
    let deadline = Instant::now() + Duration::from_millis(500);
    let entered_policy = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_flag = entered_policy.clone();
    *runtime.shared.before_policy.lock().unwrap() = Some(Arc::new(move || {
        hook_flag.store(true, Ordering::Relaxed);
        while Instant::now() < deadline {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
    }));

    runtime.restore_capture_at_startup(deadline, verify_startup_file);

    let status = runtime.status();
    let startup = runtime.startup_capture();
    close(runtime);
    assert!(entered_policy.load(Ordering::Relaxed), "the response must reach the capture policy before expiring");
    assert!(!status.capture.user_enabled && !status.capture.admission_open);
    assert!(startup.blocked);
    assert_eq!(startup.outcome, StartupOutcome::Blocked);
    assert_eq!(startup.error.as_deref(), Some(STARTUP_TIMEOUT_MESSAGE));
}

#[test]
fn startup_timeout_snapshots_serialize_an_error_for_both_outcomes() {
    let mut snapshots = Vec::new();
    for force_paused in [false, true] {
        let mut startup = StartupCapture::without_file(force_paused);
        startup.read_timed_out();
        snapshots.push(serde_json::to_value(startup.snapshot()).unwrap());
    }
    let actual = serde_json::to_value(snapshots).unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/contracts/startup_timeout_snapshots.json"
    )).unwrap();
    assert_eq!(actual, expected);
    for force_paused in [false, true] {
        let mut startup = StartupCapture::without_file(force_paused);
        startup.read_timed_out();
        let error = startup.save(Preference::Paused).unwrap_err();
        assert_eq!(error.code, ErrorCode::StorageUnavailable);
        assert_eq!(error.message, STARTUP_TIMEOUT_MESSAGE);
    }
}

fn empty_list_request() -> ListClipsRequest {
    ListClipsRequest { request_id: "1".into(), query: String::new(), limit: 10, cursor: None }
}

#[test]
fn slow_initial_usage_does_not_block_startup_timeout_or_restore_capture_later() {
    let dir = directory();
    std::fs::write(dir.join(FILE_NAME), LISTENING).unwrap();
    let (reading, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let (ready, result) = mpsc::channel();
    let startup = std::thread::spawn(move || {
        let runtime = HostRuntime::create_with_startup_io(
            Target::File(dir.join("history.sqlite3")), dir.display().to_string(),
            Arc::new(|_, _| {}), true, None, false, move |store| {
                reading.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(10)).unwrap();
                store.usage()
            }, crate::config::read_settings,
        );
        ready.send(runtime).unwrap();
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let early = result.recv_timeout(Duration::from_secs(5));
    release.send(()).unwrap();
    startup.join().unwrap();
    let (before_release, runtime) = match early {
        Ok(runtime) => (true, runtime.unwrap()),
        Err(_) => (false, result.recv().unwrap().unwrap()),
    };
    runtime.list(empty_list_request()).unwrap();
    let state = runtime.shared.state.lock().unwrap();
    let guarded = state.settings_blocked && state.storage_failed && !state.storage_ready;
    drop(state);
    let start_error = runtime.set_enabled(true).unwrap_err().code;
    let restore = runtime.startup_capture().outcome;
    let admission = runtime.status().capture.admission_open;
    close(runtime);
    assert!(before_release, "startup timeout must return while usage is still blocked");
    assert!(guarded, "late usage must not replace timed-out readiness");
    assert_eq!(start_error, ErrorCode::StorageUnavailable);
    assert_eq!(restore, StartupOutcome::Blocked);
    assert!(!admission);
}

fn assert_slow_preference_read_is_bounded(blocked_read: usize) {
    let dir = directory();
    std::fs::write(dir.join(FILE_NAME), LISTENING).unwrap();
    let (reading, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let (ready, result) = mpsc::channel();
    let read_count = std::cell::Cell::new(0);
    let startup = std::thread::spawn(move || {
        let runtime = HostRuntime::create_with_startup_io(
            Target::File(dir.join("history.sqlite3")), dir.display().to_string(),
            Arc::new(|_, _| {}), blocked_read == 1, None, false, Store::usage, move |path| {
                read_count.set(read_count.get() + 1);
                if read_count.get() == blocked_read {
                    reading.send(()).unwrap();
                    blocked.recv_timeout(Duration::from_secs(10)).unwrap();
                }
                crate::config::read_settings(path)
            },
        );
        ready.send(runtime).unwrap();
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let early = result.recv_timeout(Duration::from_secs(4));
    let before_release = early.is_ok();
    let paused_before_release = early.as_ref().ok().and_then(|r| r.as_ref().ok())
        .is_some_and(|runtime| !runtime.status().capture.admission_open);
    release.send(()).unwrap();
    startup.join().unwrap();
    let runtime = early.unwrap_or_else(|_| result.recv().unwrap()).unwrap();
    runtime.list(empty_list_request()).unwrap();
    let startup = runtime.startup_capture();
    let admission = runtime.status().capture.admission_open;
    close(runtime);

    assert!(before_release, "startup must return while preference read {blocked_read} is blocked");
    assert!(paused_before_release && !admission);
    assert_eq!(startup.outcome, StartupOutcome::Blocked);
    assert_eq!(startup.error.as_deref(), Some(STARTUP_TIMEOUT_MESSAGE));
    assert!(startup.reason.is_none());
}

#[test]
fn slow_initial_preference_read_does_not_block_startup_timeout_or_restore_later() {
    assert_slow_preference_read_is_bounded(1);
}

#[test]
fn slow_preference_recheck_does_not_block_startup_timeout_or_restore_later() {
    assert_slow_preference_read_is_bounded(2);
}

#[test]
fn stale_manual_start_cannot_supersede_a_newer_pause_that_runs_first() {
    let dir = directory();
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    let runtime = start(&dir.join("history.sqlite3"), false);
    let start_request = runtime.reserve_manual_capture_sequence();
    let pause_request = runtime.reserve_manual_capture_sequence();
    let pause = runtime.set_enabled_in_order(pause_request, false).unwrap();
    let stale = runtime.set_enabled_in_order(start_request, true).unwrap();
    close(runtime);

    assert!(!stale.capture.user_enabled && !stale.capture.admission_open);
    assert_eq!(stale.revision, pause.revision);
    assert_eq!(saved_state(&pref), "paused");
}

#[test]
fn terminal_poll_failure_is_visible_to_a_manual_start_before_capture_loop_updates() {
    let dir = directory();
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    let mut runtime = start(&dir.join("history.sqlite3"), false);
    runtime.production = true;
    {
        let mut state = runtime.shared.state.lock().unwrap();
        state.listener_ready = true;
        state.worker_service = "ready".into();
        state.set_enabled(true);
        state.observe(12, 0);
    }
    let mut pipeline = CapturePipeline::m2_synthetic(
        runtime.shared.state.clone(),
        crate::clipboard::listener::ClipboardListener::m2_synthetic(std::thread::spawn(|| {})),
        crate::clipboard::worker::WorkerSupervisor::snapshot_with_failed_reap_for_test(),
        runtime.shared.closing.clone(),
    );
    assert_eq!(pipeline.poll_worker(|| 12).unwrap_err().message, "job assign failed");
    assert_eq!(pipeline.worker_service(), "failed");
    let failed = runtime.status();
    let started = std::thread::scope(|scope| {
        scope.spawn(|| runtime.set_enabled(true)).join().unwrap().unwrap()
    });
    let restart = runtime.shared.state.lock().unwrap().restart_worker;
    let saved = saved_state(&pref);
    pipeline.close().unwrap();
    close(runtime);
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.services.worker, "failed");
    assert!(restart && !started.capture.admission_open);
    assert_eq!(saved, "paused");
}

#[test]
fn manual_start_remains_paused_until_worker_restart_succeeds() {
    let dir = directory();
    let path = dir.join("history.sqlite3");
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    let mut runtime = start(&path, false);
    runtime.production = true;
    {
        let mut state = runtime.shared.state.lock().unwrap();
        state.listener_ready = true;
        state.worker_service = "failed".into();
        state.admission = Admission::Failed;
    }

    let status = runtime.set_enabled(true).unwrap();
    assert_eq!(status.state, "failed");
    assert!(status.capture.user_enabled && !status.capture.admission_open);
    assert!(runtime.shared.state.lock().unwrap().restart_worker);
    let pending = saved_state(&pref);
    assert!(runtime.set_startup_capture(true).unwrap().enabled);
    let opted_in = saved_state(&pref);
    runtime.shared.worker_restart_finished("failed", Err(cleanup()));
    assert!(!runtime.status().capture.admission_open);
    let failed = saved_state(&pref);
    assert!(!runtime.set_enabled(true).unwrap().capture.admission_open);
    let (writing, write_started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    *runtime.shared.before_startup_capture_write.lock().unwrap() = Some(Arc::new(move || {
        writing.send(()).unwrap();
        blocked.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let (finished, restart_finished) = mpsc::channel();
    let shared = runtime.shared.clone();
    let capture = std::thread::spawn(move || {
        shared.worker_restart_finished("ready", Ok(()));
        finished.send(()).unwrap();
    });
    write_started.recv_timeout(Duration::from_secs(2)).unwrap();
    let restart_returned = restart_finished.recv_timeout(Duration::from_secs(2)).is_ok();
    let disk_while_blocked = saved_state(&pref);
    release.send(()).unwrap();
    capture.join().unwrap();
    assert!(runtime.status().capture.admission_open);
    close(runtime);
    let recovered = saved_state(&pref);
    assert!(restart_returned, "capture restart must return before the preference write finishes");
    assert_eq!([pending.as_str(), opted_in.as_str(), failed.as_str(), disk_while_blocked.as_str(), recovered.as_str()],
        ["paused", "paused", "paused", "paused", "listening"]);
    let reopened = start(&path, false);
    assert_eq!(reopened.status().state, "listening");
    close(reopened);
}

#[test]
fn manual_start_saves_listening_only_after_overloaded_admission_recovers() {
    let dir = directory();
    let path = dir.join("history.sqlite3");
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    let runtime = start(&path, false);
    {
        let mut state = runtime.shared.state.lock().unwrap();
        state.admission = Admission::Overloaded;
        state.bytes = MAX_QUEUE_BYTES;
    }

    let status = runtime.set_enabled(true).unwrap();
    assert_eq!(status.state, "overloaded");
    assert!(status.capture.user_enabled && !status.capture.admission_open);
    let pending = saved_state(&pref);
    runtime.shared.state.lock().unwrap().bytes = 0;
    assert!(runtime.set_enabled(true).unwrap().capture.admission_open);
    let recovered = saved_state(&pref);
    close(runtime);
    assert_eq!([pending.as_str(), recovered.as_str()], ["paused", "listening"]);
    let reopened = start(&path, false);
    assert_eq!(reopened.status().state, "listening");
    close(reopened);
}

#[test]
fn storage_recovery_keeps_queries_live_and_shutdown_waits_for_ordered_writes() {
    let dir = directory();
    let path = dir.join("history.sqlite3");
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    let runtime = Arc::new(start(&path, false));
    {
        let mut state = runtime.shared.state.lock().unwrap();
        state.storage_failed = true;
        state.admission = Admission::Failed;
    }
    let (writing_tx, writing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    *runtime.shared.before_startup_capture_write.lock().unwrap() = Some(Arc::new(move || {
        writing_tx.send(()).unwrap();
        release.lock().unwrap().recv_timeout(Duration::from_secs(12)).unwrap();
    }));
    runtime.set_enabled(true).unwrap();
    writing_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let paused_on_disk = saved_state(&pref);
    let recovered = runtime.status().capture.admission_open;
    let listed = runtime.list(empty_list_request());
    let pause = {
        let runtime = runtime.clone();
        std::thread::spawn(move || runtime.set_enabled(false))
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut state = runtime.shared.state.lock().unwrap();
    while state.startup_capture_writes.last_manual != Some((2, false)) && Instant::now() < deadline {
        state = runtime.shared.wake.wait_timeout(state, deadline.saturating_duration_since(Instant::now())).unwrap().0;
    }
    let pause_admitted = state.startup_capture_writes.last_manual == Some((2, false));
    drop(state);
    let first_expired = runtime.shutdown_until(Instant::now()).unwrap_err();
    release_tx.send(()).unwrap();
    writing_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let listening_on_disk = saved_state(&pref);
    let second_expired = runtime.shutdown_until(Instant::now()).unwrap_err();
    release_tx.send(()).unwrap();
    assert!(pause.join().unwrap().is_ok());
    close(Arc::try_unwrap(runtime).ok().unwrap());
    assert!(listed.is_ok(), "blocked preference write must not stall history list: {listed:?}");
    assert_eq!(paused_on_disk, "paused");
    assert!(recovered);
    assert!(pause_admitted);
    assert_eq!(listening_on_disk, "listening");
    assert!(first_expired.message.contains("启动恢复设置仍在保存"));
    assert!(second_expired.message.contains("启动恢复设置仍在保存"));
    assert_eq!(saved_state(&pref), "paused");
    let reopened = start(&path, false);
    assert!(!reopened.status().capture.admission_open);
    close(reopened);
}

#[test]
fn storage_retry_filling_history_capacity_keeps_admission_closed_and_preference_paused() {
    use crate::clipboard::{capture::{CaptureResponse, CaptureResult, WireFormat}, protocol::WorkerMessage};
    use base64::{engine::general_purpose::STANDARD, Engine};
    let dir = directory();
    let path = dir.join("history.sqlite3");
    let pref = dir.join(FILE_NAME);
    let mut store = Store::open(&path).unwrap();
    let mut snapshot = crate::store::Snapshot {
        capture_id: String::new(), captured_at_ms: 0, source_process: None,
        source_confidence: crate::store::SourceConfidence::Unknown, omitted_formats: vec![],
        payloads: vec![crate::store::Payload { format_name: "CF_UNICODETEXT".into(), data: vec![65, 0, 0, 0] }],
    };
    for id in 1..MAX_HISTORY_ITEMS {
        snapshot.capture_id = format!("fixture-{id}");
        store.insert(&snapshot).unwrap();
    }
    drop(store);

    let (event_tx, event_rx) = mpsc::channel();
    let runtime = HostRuntime::for_test_file(path, Arc::new(move |name, _| {
        let _ = event_tx.send(name.to_owned());
    })).unwrap();
    runtime.set_enabled(true).unwrap();
    runtime.set_test_fault(true).unwrap();
    let mut response = CaptureResponse::empty(&WorkerMessage { version: 1, capture_id: "retry".into(), policy_epoch: 0 });
    response.result = CaptureResult::Captured;
    response.reason = None;
    response.attempts = 1;
    response.sequence_before = 1;
    response.sequence_locked = 1;
    response.sequence_after = 1;
    response.formats = vec![WireFormat { name: "CF_UNICODETEXT".into(), data: STANDARD.encode([65, 0, 0, 0]) }];
    runtime.inject_response(1, response).unwrap();
    while runtime.status().services.storage != "failed" { event_rx.recv_timeout(Duration::from_secs(2)).unwrap(); }
    assert_eq!(runtime.status().queue.items, 1);
    runtime.set_startup_capture(true).unwrap();
    assert_eq!(saved_state(&pref), "paused");

    runtime.set_test_fault(false).unwrap();
    runtime.set_enabled(true).unwrap();
    while event_rx.recv_timeout(Duration::from_secs(2)).unwrap() != "history_changed" {}
    let status = runtime.status();
    let notice = runtime.shared.state.lock().unwrap().notice;
    close(runtime);

    assert_eq!(status.history.items, MAX_HISTORY_ITEMS as i64);
    assert_eq!(status.queue.items, 0);
    assert_eq!(status.state, "overloaded");
    assert!(!status.capture.admission_open);
    assert_eq!(notice, Some(crate::runtime::Notice::HistoryCapacity));
    assert_eq!(saved_state(&pref), "paused");
}

#[test]
fn worker_restart_checks_history_and_queue_capacity_before_opening_admission() {
    use crate::runtime::Notice;
    for (history_bytes, queue_bytes, expected) in [
        (MAX_HISTORY_PAYLOAD_BYTES, 0, Notice::HistoryCapacity),
        (0, MAX_QUEUE_BYTES, Notice::SnapshotQueueFull),
    ] {
        let dir = directory();
        let pref = dir.join(FILE_NAME);
        std::fs::write(&pref, PAUSED).unwrap();
        let mut runtime = start(&dir.join("history.sqlite3"), false);
        runtime.production = true;
        {
            let mut state = runtime.shared.state.lock().unwrap();
            state.listener_ready = true;
            state.worker_service = "failed".into();
            state.admission = Admission::Failed;
        }
        assert!(!runtime.set_enabled(true).unwrap().capture.admission_open);
        {
            let mut state = runtime.shared.state.lock().unwrap();
            state.history.payload_bytes = history_bytes as i64;
            state.bytes = queue_bytes;
        }
        runtime.shared.worker_restart_finished("ready", Ok(()));
        let status = runtime.status();
        let notice = runtime.shared.state.lock().unwrap().notice;
        close(runtime);
        assert_eq!(status.state, "overloaded");
        assert!(!status.capture.admission_open);
        assert_eq!(notice, Some(expected));
        assert_eq!(saved_state(&pref), "paused");
    }
}

#[test]
fn a_worker_restart_completed_after_pause_cannot_save_listening() {
    let dir = directory();
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    let mut runtime = start(&dir.join("history.sqlite3"), false);
    runtime.production = true;
    {
        let mut state = runtime.shared.state.lock().unwrap();
        state.listener_ready = true;
        state.worker_service = "failed".into();
        state.admission = Admission::Failed;
    }
    assert!(!runtime.set_enabled(true).unwrap().capture.admission_open);
    runtime.set_enabled(false).unwrap();
    runtime.shared.worker_restart_finished("ready", Ok(()));
    assert!(!runtime.status().capture.admission_open);
    assert_eq!(saved_state(&pref), "paused");
    close(runtime);
}

fn startup_blocked_by_full_history(dir: &Path) -> (HostRuntime, Receiver<u64>) {
    let (send, events) = mpsc::channel();
    std::fs::write(dir.join(FILE_NAME), LISTENING).unwrap();
    let runtime = HostRuntime::create(
        Target::File(dir.join("history.sqlite3")),
        dir.display().to_string(),
        Arc::new(move |name, revision| {
            if name == CHANGED_EVENT { send.send(revision).unwrap(); }
        }),
        false,
        None,
        true,
    ).unwrap();
    *runtime.shared.startup_capture.lock().unwrap() = StartupCapture::load(Some(dir), false, crate::config::read_settings);
    runtime.shared.state.lock().unwrap().history.items = MAX_HISTORY_ITEMS as i64;
    runtime.restore_capture_at_startup(Instant::now() + STARTUP_TIMEOUT, verify_startup_file);
    let startup = runtime.startup_capture();
    assert_eq!(startup.outcome, StartupOutcome::Blocked);
    assert!(!startup.blocked);
    assert!(startup.reason.unwrap().contains("历史容量已满"));
    assert!(!runtime.status().capture.admission_open);
    (runtime, events)
}

#[test]
fn manual_start_clears_startup_capacity_reason_after_space_is_available() {
    let dir = directory();
    let (runtime, events) = startup_blocked_by_full_history(&dir);
    let before = runtime.startup_capture();
    assert_eq!(runtime.set_enabled(true).unwrap_err().code, ErrorCode::CapacityExceeded);
    assert_eq!(runtime.startup_capture().revision, before.revision);
    assert_eq!(runtime.startup_capture().reason, before.reason);
    runtime.shared.state.lock().unwrap().history.items = 0;

    assert_eq!(runtime.set_enabled(true).unwrap().state, "listening");
    let after = runtime.startup_capture();
    assert!(after.reason.is_none());
    assert_ne!(after.outcome, StartupOutcome::Blocked);
    assert_ne!(after.revision, before.revision);
    assert_eq!(events.try_recv().unwrap().to_string(), after.revision);
    assert!(events.try_recv().is_err());
    close(runtime);
}

#[test]
fn opting_out_clears_startup_capacity_reason_even_while_history_is_full() {
    let dir = directory();
    let (runtime, events) = startup_blocked_by_full_history(&dir);
    let before = runtime.startup_capture();

    let after = runtime.set_startup_capture(false).unwrap();
    assert!(!after.enabled);
    assert!(after.reason.is_none());
    assert_ne!(after.outcome, StartupOutcome::Blocked);
    assert_ne!(after.revision, before.revision);
    assert_eq!(events.try_recv().unwrap().to_string(), after.revision);
    assert!(events.try_recv().is_err());
    assert_eq!(runtime.status().state, "paused");
    assert_eq!(saved_state(&dir.join(FILE_NAME)), "disabled");
    close(runtime);
}

#[test]
fn manual_start_preserves_external_file_protection_after_startup_is_blocked() {
    let dir = directory();
    let (runtime, _events) = startup_blocked_by_full_history(&dir);
    let original_reason = runtime.startup_capture().reason;
    let pref = dir.join(FILE_NAME);
    std::fs::write(&pref, PAUSED).unwrap();
    runtime.shared.state.lock().unwrap().history.items = 0;

    assert_eq!(runtime.set_enabled(true).unwrap().state, "listening");
    assert!(runtime.set_startup_capture(false).is_err());
    let after = runtime.startup_capture();
    assert!(after.blocked);
    assert_eq!(after.reason, original_reason);
    assert!(after.error.unwrap().contains("外部更改"));
    assert_eq!(std::fs::read(&pref).unwrap(), PAUSED);
    close(runtime);
}

#[test]
fn live_pause_during_opt_in_write_closes_admission_and_wins_on_disk() {
    let dir = directory();
    let pref = dir.join(FILE_NAME);
    let runtime = Arc::new(start(&dir.join("history.sqlite3"), false));
    runtime.set_enabled(true).unwrap();
    let (writing_tx, writing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    *runtime.shared.before_startup_capture_write.lock().unwrap() = Some(Arc::new(move || {
        writing_tx.send(()).unwrap();
        release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let opting_in = runtime.clone();
    let opt_in = std::thread::spawn(move || opting_in.set_startup_capture(true));
    writing_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let pausing = runtime.clone();
    let pause = std::thread::spawn(move || pausing.set_enabled(false));
    let elapsed = Instant::now() + Duration::from_secs(1);
    let mut state = runtime.shared.state.lock().unwrap();
    while state.user_enabled && Instant::now() < elapsed {
        state = runtime.shared.wake.wait_timeout(state, elapsed.saturating_duration_since(Instant::now())).unwrap().0;
    }
    let closed_during_write = !state.user_enabled && state.admission != Admission::Active;
    drop(state);
    assert!(!runtime.startup_capture().enabled);
    release_tx.send(()).unwrap();
    assert!(opt_in.join().unwrap().unwrap().enabled);
    writing_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let expired = runtime.shutdown_until(Instant::now()).unwrap_err();
    release_tx.send(()).unwrap();
    assert!(!pause.join().unwrap().unwrap().capture.admission_open);
    assert_eq!(saved_state(&pref), "paused");
    runtime.shutdown(Duration::from_secs(3)).unwrap();
    assert!(closed_during_write, "preference I/O delayed the live pause");
    assert!(expired.message.contains("启动恢复设置仍在保存"));
}

#[test]
fn shutdown_waits_for_an_admitted_preference_write_and_cannot_override_it() {
    let dir = directory();
    let pref = dir.join(FILE_NAME);
    let path = dir.join("history.sqlite3");
    let runtime = Arc::new(start(&path, false));
    runtime.set_startup_capture(true).unwrap();
    let (writing_tx, writing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    *runtime.shared.before_startup_capture_write.lock().unwrap() = Some(Arc::new(move || {
        let _ = writing_tx.send(());
        let _ = release.lock().unwrap().recv_timeout(Duration::from_secs(3));
    }));
    let capture = runtime.clone();
    let write = std::thread::spawn(move || capture.set_enabled(true));
    writing_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let expired = runtime.shutdown_until(Instant::now());
    release_tx.send(()).unwrap();
    write.join().unwrap().unwrap();
    assert!(expired.unwrap_err().message.contains("启动恢复设置仍在保存"));
    assert!(!runtime.exit_ready.load(Ordering::Acquire));
    assert_eq!(saved_state(&pref), "listening");
    assert_eq!(runtime.set_startup_capture(false).unwrap_err().code, ErrorCode::Busy);
    runtime.shutdown(Duration::from_secs(3)).unwrap();
    drop(runtime);
    let reopened = start(&path, false);
    assert_eq!(reopened.status().state, "listening");
    close(reopened);
}

#[test]
fn startup_capture_abrupt_exit_child() {
    let Some(path) = std::env::var_os(ABRUPT_CHILD_DB) else { return; };
    let runtime = start(Path::new(&path), false);
    runtime.set_startup_capture(true).unwrap();
    runtime.set_enabled(true).unwrap();
    std::process::exit(0);
}

#[test]
fn a_synchronized_manual_choice_survives_abrupt_process_exit() {
    let dir = directory();
    let path = dir.join("history.sqlite3");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "host::startup_capture_tests::startup_capture_abrupt_exit_child"])
        .env(ABRUPT_CHILD_DB, &path).status().unwrap();
    assert!(status.success());
    assert_eq!(saved_state(&dir.join(FILE_NAME)), "listening");
    let reopened = start(&path, false);
    assert_eq!(reopened.status().state, "listening");
    close(reopened);
}
