//! worker_protocol 集成测试：真实 supervisor + 真实 fixture 子进程。
//! 不 mock 超时/回收逻辑；每个失败用例断言 PID/创建时间退出、读取线程确认与句柄释放。
//! 时间断言记录实测耗时并留调度余量，不放宽生产 750 ms / 2 s 截止。

#![cfg(feature = "test-fixtures")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pinfeather_lib::clipboard::protocol::WorkerMessage;
use pinfeather_lib::clipboard::worker::{ReapReport, WorkerState, WorkerSupervisor};
use pinfeather_lib::contracts::{ErrorCode, WORKER_PROTOCOL_VERSION};

fn fixture(mode: &str) -> WorkerSupervisor {
    WorkerSupervisor::for_test_fixture(
        PathBuf::from(env!("CARGO_BIN_EXE_worker_fixture")),
        mode,
    )
}

fn msg(id: &str) -> WorkerMessage {
    WorkerMessage {
        version: WORKER_PROTOCOL_VERSION,
        capture_id: String::from(id),
        policy_epoch: 1,
    }
}

fn assert_reaped(report: &ReapReport, label: &str) {
    let identity = report.identity.unwrap_or_else(|| panic!("{label}: reap missing identity"));
    assert!(identity.pid > 0, "{label}: pid recorded");
    assert!(identity.create_time > 0, "{label}: create time recorded");
    assert!(report.exited, "{label}: exact worker process observed exited");
    assert!(
        report.reader_stopped,
        "{label}: reader thread confirmed stopped"
    );
    assert!(report.writer_stopped, "{label}: writer thread confirmed stopped");
    assert!(!report.cleanup_failed, "{label}: cleanup succeeded");
    assert!(
        report.cleanup_elapsed <= Duration::from_millis(2500),
        "{label}: cleanup within bounded budget, took {:?}",
        report.cleanup_elapsed
    );
    eprintln!(
        "{label}: reap pid={} create_time={} elapsed={:?}",
        identity.pid, identity.create_time, report.cleanup_elapsed
    );
}

#[test]
fn handshake_and_roundtrip() {
    let mut s = fixture("good");
    let started = Instant::now();
    let ack = s.capture(&msg("OK-1")).expect("roundtrip should succeed");
    eprintln!("roundtrip elapsed={:?}", started.elapsed());
    assert_eq!(ack.capture_id, "OK-1");
    assert_eq!(ack.policy_epoch, 1);
    assert_eq!(ack.version, WORKER_PROTOCOL_VERSION);
    assert_eq!(s.state(), WorkerState::Ready);
    assert_eq!(s.spawn_count(), 1);
    assert_eq!(s.consecutive_timeouts(), 0);
    let identity = s.current_identity().expect("worker identity present");
    assert!(identity.pid > 0 && identity.create_time > 0);
    // 同一 worker 上第二次往返也必须成功。
    let ack2 = s.capture(&msg("OK-2")).expect("second roundtrip");
    assert_eq!(ack2.capture_id, "OK-2");
    assert_eq!(s.spawn_count(), 1);
    s.close();
    let report = s.last_reap().cloned().expect("reap recorded");
    assert_reaped(&report, "roundtrip-close");
}

#[test]
fn no_hello_handshake_timeout() {
    let mut s = fixture("no-hello");
    let started = Instant::now();
    let err = s.capture(&msg("OK-1")).expect_err("no hello must fail");
    let elapsed = started.elapsed();
    eprintln!("no_hello elapsed={:?} err={}", elapsed, err.message);
    assert_eq!(err.code, ErrorCode::Timeout);
    assert!(elapsed >= Duration::from_millis(1900), "handshake deadline honored");
    assert!(elapsed <= Duration::from_millis(4000), "bounded handshake wait");
    assert_reaped(s.last_reap().expect("reap recorded"), "no-hello");
}

#[test]
fn bad_hello_kind_rejected() {
    let mut s = fixture("bad-hello");
    let err = s.capture(&msg("OK-1")).expect_err("bad hello kind");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "bad-hello");
}

#[test]
fn bad_hello_version_rejected() {
    let mut s = fixture("bad-hello-version");
    let err = s.capture(&msg("OK-1")).expect_err("bad hello version");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "bad-hello-version");
}

#[test]
fn hello_extra_field_rejected() {
    let mut s = fixture("hello-extra");
    let err = s.capture(&msg("OK-1")).expect_err("hello extra field");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "hello-extra");
}

#[test]
fn no_response_capture_timeout() {
    let mut s = fixture("no-response");
    let started = Instant::now();
    let err = s.capture(&msg("OK-1")).expect_err("no response must time out");
    let elapsed = started.elapsed();
    eprintln!("no_response elapsed={:?} err={}", elapsed, err.message);
    assert_eq!(err.code, ErrorCode::Timeout);
    assert!(elapsed >= Duration::from_millis(700), "750ms deadline honored");
    assert!(elapsed <= Duration::from_millis(3000), "bounded capture wait");
    assert_eq!(s.consecutive_timeouts(), 1);
    assert_reaped(s.last_reap().expect("reap recorded"), "no-response");
}

#[test]
fn half_length_prefix_eof_safe_error() {
    let mut s = fixture("half-length");
    let err = s.capture(&msg("OK-1")).expect_err("half length prefix");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "half-length");
}

#[test]
fn half_body_eof_safe_error() {
    let mut s = fixture("half-body");
    let err = s.capture(&msg("OK-1")).expect_err("half body");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "half-body");
}

#[test]
fn oversize_declared_rejected_before_allocation() {
    let mut s = fixture("oversize");
    let err = s.capture(&msg("OK-1")).expect_err("oversize declared length");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "oversize");
}

#[test]
fn bad_json_rejected() {
    let mut s = fixture("bad-json");
    let err = s.capture(&msg("OK-1")).expect_err("bad json body");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "bad-json");
}

#[test]
fn wrong_capture_id_rejected() {
    let mut s = fixture("wrong-id");
    let err = s.capture(&msg("OK-1")).expect_err("wrong capture_id");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "wrong-id");
}

#[test]
fn wrong_epoch_rejected() {
    let mut s = fixture("wrong-epoch");
    let err = s.capture(&msg("OK-1")).expect_err("wrong epoch");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "wrong-epoch");
}

#[test]
fn crash_reaped() {
    let mut s = fixture("crash");
    let err = s.capture(&msg("OK-1")).expect_err("worker crash");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "crash");
}

#[test]
fn late_response_never_accepted() {
    let mut s = fixture("late");
    let started = Instant::now();
    let result = s.capture(&msg("OK-1"));
    let elapsed = started.elapsed();
    eprintln!("late elapsed={:?} ok={}", elapsed, result.is_ok());
    let err = result.expect_err("late response must not be accepted");
    assert_eq!(err.code, ErrorCode::Timeout);
    assert_eq!(s.state(), WorkerState::Ready);
    assert_reaped(s.last_reap().expect("reap recorded"), "late");
}

#[test]
fn unsolicited_frame_discarded_and_next_capture_ok() {
    let mut s = fixture("double-ack");
    let first = s.capture(&msg("TWICE-1")).expect("first ack");
    assert_eq!(first.capture_id, "TWICE-1");
    // 第二帧（迟到/未请求）不得破坏下一次捕获。
    let second = s.capture(&msg("TWICE-2")).expect("second capture after unsolicited frame");
    assert_eq!(second.capture_id, "TWICE-2");
    assert!(
        s.late_frames_discarded() >= 1,
        "unsolicited frame must be discarded, got {}",
        s.late_frames_discarded()
    );
    assert_eq!(s.spawn_count(), 1);
}

#[test]
fn consecutive_timeouts_pause() {
    let mut s = fixture("driven");
    let err1 = s.capture(&msg("STALL-1")).expect_err("first stall");
    assert_eq!(err1.code, ErrorCode::Timeout);
    assert_eq!(s.consecutive_timeouts(), 1);
    assert_eq!(s.state(), WorkerState::Ready);
    let err2 = s.capture(&msg("STALL-2")).expect_err("second stall");
    assert_eq!(err2.code, ErrorCode::Timeout);
    assert_eq!(s.consecutive_timeouts(), 2);
    assert_eq!(s.state(), WorkerState::FailedPaused, "two timeouts pause");
    let spawns_before = s.spawn_count();
    assert_eq!(spawns_before, 2, "one rebuild after first timeout only");
    let err3 = s.capture(&msg("OK-3")).expect_err("paused supervisor");
    assert_eq!(err3.code, ErrorCode::Busy);
    assert_eq!(s.spawn_count(), spawns_before, "no third spawn while paused");
    assert_eq!(s.stalled_reader_count(), 0);
}

#[test]
fn success_resets_timeout_counter() {
    let mut s = fixture("driven");
    s.capture(&msg("OK-1")).expect("initial success");
    let err1 = s.capture(&msg("STALL-1")).expect_err("first stall");
    assert_eq!(err1.code, ErrorCode::Timeout);
    assert_eq!(s.consecutive_timeouts(), 1);
    s.capture(&msg("OK-2")).expect("success resets counter");
    assert_eq!(s.consecutive_timeouts(), 0);
    let err2 = s.capture(&msg("STALL-2")).expect_err("stall after reset");
    assert_eq!(err2.code, ErrorCode::Timeout);
    assert_eq!(s.consecutive_timeouts(), 1, "counter restarted after reset");
    assert_eq!(s.state(), WorkerState::Ready, "not paused after reset window");
    s.capture(&msg("OK-3")).expect("rebuild still allowed");
    assert_eq!(s.spawn_count(), 3);
}

#[test]
fn repeated_close_idempotent() {
    let mut s = fixture("good");
    s.capture(&msg("OK-1")).expect("roundtrip");
    s.close();
    let first = s.last_reap().cloned().expect("reap after close");
    assert_reaped(&first, "close-1");
    s.close();
    s.close();
    let second = s.last_reap().cloned().expect("reap still present");
    assert_eq!(first.identity, second.identity);
    assert_eq!(first.exited, second.exited);
    assert_eq!(s.state(), WorkerState::Ready);
    assert_eq!(s.stalled_reader_count(), 0);
}

#[test]
fn trickle_frame_assembled() {
    let mut s = fixture("trickle");
    let started = Instant::now();
    let ack = s.capture(&msg("OK-1")).expect("trickled frame must assemble");
    let elapsed = started.elapsed();
    eprintln!("trickle elapsed={:?}", elapsed);
    assert_eq!(ack.capture_id, "OK-1");
    assert!(elapsed < Duration::from_millis(750), "assembled within deadline");
}

#[test]
fn worker_identity_stable_across_captures() {
    let mut s = fixture("good");
    s.capture(&msg("OK-1")).expect("first");
    let a = s.current_identity().expect("identity");
    s.capture(&msg("OK-2")).expect("second");
    let b = s.current_identity().expect("identity");
    assert_eq!(a, b, "same worker keeps pid and create time");
}

// ---- F3：请求校验、状态与发送截止 ----

#[test]
fn invalid_capture_id_rejected_without_side_effects() {
    let mut s = fixture("good");
    let err = s
        .capture(&msg(&"A".repeat(5000)))
        .expect_err("oversize capture_id must be rejected");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_ne!(s.state(), WorkerState::InFlight, "must not stay InFlight");
    assert_eq!(s.current_identity(), None, "no worker may be spawned");
    assert_eq!(s.spawn_count(), 0);
    // 已有正常 worker 也不能被本地非法输入卡死：先合法建立，再非法，再合法。
    s.capture(&msg("OK-1")).expect("valid request must still work");
    let err2 = s
        .capture(&msg(&"B".repeat(5000)))
        .expect_err("invalid again");
    assert_eq!(err2.code, ErrorCode::ProtocolError);
    assert_eq!(s.state(), WorkerState::Ready);
    assert!(s.current_identity().is_some(), "healthy worker survives invalid input");
    s.capture(&msg("OK-2")).expect("valid after invalid");
    assert_eq!(s.spawn_count(), 1);
}

#[test]
fn invalid_version_epoch_rejected_without_side_effects() {
    let mut s = fixture("good");
    let mut bad_version = msg("OK-1");
    bad_version.version = 2;
    assert!(s.capture(&bad_version).is_err());
    let mut bad_epoch = msg("OK-1");
    bad_epoch.policy_epoch = -1;
    assert!(s.capture(&bad_epoch).is_err());
    let mut huge_epoch = msg("OK-1");
    huge_epoch.policy_epoch = 9_007_199_254_740_992;
    assert!(s.capture(&huge_epoch).is_err());
    assert_eq!(s.spawn_count(), 0, "no worker spawned for invalid requests");
    assert_ne!(s.state(), WorkerState::InFlight);
    s.capture(&msg("OK-2")).expect("valid request works after rejects");
}

#[test]
fn never_read_worker_times_out_and_reaps() {
    let mut s = fixture("never-read");
    let started = Instant::now();
    let err = s.capture(&msg("OK-1")).expect_err("never-read must time out");
    eprintln!("never_read elapsed={:?}", started.elapsed());
    assert_eq!(err.code, ErrorCode::Timeout);
    assert_reaped(s.last_reap().expect("reap recorded"), "never-read");
}

// ---- F2：有界 reader 队列与可靠停止通知 ----

#[test]
fn idle_flood_overflows_queue_and_reaps() {
    let mut s = fixture("flood-idle");
    std::thread::sleep(Duration::from_millis(300));
    let err = s.capture(&msg("OK-1")).expect_err("idle flood must fail");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert!(
        s.late_frames_discarded() <= 2,
        "queue bound is 2 frames, discarded={}",
        s.late_frames_discarded()
    );
    assert_reaped(s.last_reap().expect("reap recorded"), "flood-idle");
}

#[test]
fn flood_before_ack_never_succeeds() {
    let mut s = fixture("flood-before-ack");
    let result = s.capture(&msg("OK-1"));
    assert!(result.is_err(), "flood must not be interpreted as success: {result:?}");
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "flood-before-ack");
}

#[test]
fn queue_two_cached_frames_exact_bound() {
    // 已结束 ID 的重复帧逐个到达（每批 ≤2 帧）：恰好 2 个被有界丢弃，不触发溢出。
    let mut s = fixture("dup-per-round");
    let ack = s.capture(&msg("DUP-1")).expect("first ACK accepted");
    assert_eq!(ack.capture_id, "DUP-1");
    let ack2 = s.capture(&msg("DUP-2")).expect("capture with one settled dup");
    assert_eq!(ack2.capture_id, "DUP-2");
    let ack3 = s.capture(&msg("DUP-3")).expect("capture with second settled dup");
    assert_eq!(ack3.capture_id, "DUP-3");
    assert_eq!(s.late_frames_discarded(), 2, "exactly two settled-id duplicates discarded");
    s.close();
    assert_reaped(s.last_reap().expect("reap recorded"), "dup-per-round");
}

#[test]
fn queue_full_then_close_clean() {
    let mut s = fixture("matching-ack-flood");
    s.begin_test_request(&msg("FILL-1")).expect("spawn and send");
    let identity = s.current_identity().expect("actual worker started");
    assert_eq!(s.spawn_count(), 1);
    assert!(s.wait_for_test_reader_stop(Duration::from_secs(1)), "reader completed flood before close");
    assert_eq!(s.test_reader_error().expect("reader failed").message, "reader queue overflow");
    s.close();
    let report = s.last_reap().cloned().expect("reap after close");
    assert_eq!(report.identity, Some(identity));
    assert_reaped(&report, "full-queue-close");
    assert_eq!(s.stalled_reader_count(), 0);
    assert_eq!(s.state(), WorkerState::Ready);
}

#[test]
fn matching_ack_cannot_hide_reader_overflow() {
    let mut s = fixture("matching-ack-flood");
    s.set_test_wait_reader_stop();
    let result = s.capture(&msg("MATCH-1"));
    eprintln!("matching flood: result={result:?}");
    let err = result.expect_err("known fatal overflow must reject the current matching ACK");
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_eq!(err.message, "reader queue overflow");
    assert_reaped(s.last_reap().expect("overflow reaped immediately"), "matching-flood");
}

#[test]
fn complete_ack_followed_by_clean_eof_is_accepted() {
    let mut s = fixture("delay-0");
    s.set_test_wait_reader_stop();
    let ack = s.capture(&msg("EOF-ACK")).expect("clean EOF must not mask a complete ACK");
    assert_eq!(ack.capture_id, "EOF-ACK");
    s.close();
    assert_reaped(s.last_reap().unwrap(), "clean-eof-ack");
}

#[test]
fn complete_ack_cannot_hide_truncated_next_frame() {
    let mut s = fixture("ack-then-partial");
    s.set_test_wait_reader_stop();
    let err = s.capture(&msg("PARTIAL-ACK")).expect_err("fatal partial frame must beat queued ACK");
    assert_eq!(err.message, "unexpected end of stream");
    assert_reaped(s.last_reap().unwrap(), "partial-after-ack");
}

#[test]
fn unconfirmed_writer_blocks_rebuild_and_close_retries_cleanup() {
    let mut s = fixture("never-read");
    let (release, gate) = std::sync::mpsc::channel();
    s.set_test_write_gate(gate);
    let result = s.capture(&msg("GATED-WRITE"));
    let state = s.state();
    let pending = s.pending_writer_count();
    let report = s.last_reap().cloned().expect("cleanup attempted");
    let retry = s.capture(&msg("MUST-NOT-SPAWN"));
    let spawns = s.spawn_count();
    // 即使下面的旧行为断言失败，也先释放本测试的 writer。
    let _ = release.send(());
    s.close();
    eprintln!("gated write: result={result:?} state={state:?} pending={pending} report={report:?} spawns={spawns}");
    assert_eq!(result.unwrap_err().code, ErrorCode::Timeout);
    assert!(report.cleanup_failed, "unconfirmed writer is a cleanup failure");
    assert!(!report.writer_stopped);
    assert_eq!(pending, 1);
    assert_eq!(state, WorkerState::FailedPaused);
    assert_eq!(retry.unwrap_err().code, ErrorCode::Busy);
    assert_eq!(spawns, 1, "no worker recreation with unconfirmed IO");
    assert_eq!(s.pending_writer_count(), 0, "close retries and joins released writer");
    assert_reaped(s.last_reap().expect("final cleanup report"), "gated-write-close");
}

#[test]
fn writer_confirmed_after_termination_allows_one_rebuild() {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

    let mut s = fixture("never-read");
    s.begin_test_request(&msg("PRIME")).expect("start real fixture");
    let identity = s.current_identity().unwrap();
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, identity.pid) };
    assert!(!process.is_null());
    let process = process as usize;
    let (release, gate) = std::sync::mpsc::channel();
    s.set_test_write_gate(gate);
    let releaser = std::thread::spawn(move || {
        let exited = unsafe { WaitForSingleObject(process as _, 5000) } == WAIT_OBJECT_0;
        let _ = release.send(());
        unsafe { CloseHandle(process as _) };
        exited
    });
    let err = s.capture(&msg("AFTER-CANCEL")).expect_err("writer reaches deadline");
    assert!(releaser.join().unwrap(), "release followed actual process exit");
    assert_eq!(err.code, ErrorCode::Timeout);
    assert_reaped(s.last_reap().unwrap(), "writer-confirmed-after-kill");
    assert_eq!(s.state(), WorkerState::Ready);
    assert_eq!(s.pending_writer_count(), 0);
    s.begin_test_request(&msg("REBUILD")).expect("rebuild only after confirmed cleanup");
    assert_eq!(s.spawn_count(), 2);
    s.close();
    assert_reaped(s.last_reap().unwrap(), "rebuilt-close");
}

#[test]
fn drop_retains_unconfirmed_resources_and_blocks_future_workers() {
    const CHILD: &str = "PINFEATHER_TEST_DROP_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut s = fixture("never-read");
        let (release, gate) = std::sync::mpsc::channel();
        s.set_test_write_gate(gate);
        assert_eq!(s.capture(&msg("DROP-GATED")).unwrap_err().code, ErrorCode::Timeout);
        assert_eq!(s.state(), WorkerState::FailedPaused);
        assert!(s.last_reap().unwrap().exited);
        drop(s);
        let _ = release.send(());
        let mut next = fixture("good");
        assert_eq!(next.state(), WorkerState::FailedPaused);
        assert_eq!(next.capture(&msg("NO-NEW-WORKER")).unwrap_err().code, ErrorCode::Busy);
        assert_eq!(next.spawn_count(), 0);
        return;
    }

    use std::os::windows::process::CommandExt;
    let root = PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT")
        .expect("set PINFEATHER_TEST_DATA_ROOT to this run's isolated archive directory"));
    assert!(root.is_absolute() && root.is_dir());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let output = root.join(format!("drop-{}-{stamp}.txt", std::process::id()));
    let log = std::fs::File::options().write(true).create_new(true).open(&output).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "drop_retains_unconfirmed_resources_and_blocks_future_workers", "--nocapture"])
        .env(CHILD, "1")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "drop subprocess failed; see {}", output.display());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("terminate only the test-owned child");
            let _ = child.wait();
            panic!("drop subprocess exceeded bounded deadline; see {}", output.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---- F1：截止覆盖解码与关联 ----

#[test]
fn expired_processing_rejected_not_success() {
    let mut s = fixture("delay-600");
    s.set_test_processing_delay(Duration::from_millis(300));
    let started = Instant::now();
    let result = s.capture(&msg("OK-1"));
    let elapsed = started.elapsed();
    eprintln!("expired_processing elapsed={:?} ok={}", elapsed, result.is_ok());
    let err = result.expect_err("result finishing after deadline must be rejected");
    assert_eq!(err.code, ErrorCode::Timeout);
    assert_eq!(s.consecutive_timeouts(), 1, "expired result counts as timeout");
    assert_reaped(s.last_reap().expect("reap recorded"), "expired-processing");
}

#[test]
fn in_time_processing_accepted() {
    let mut s = fixture("delay-100");
    let ack = s.capture(&msg("OK-1")).expect("in-time ACK must be accepted");
    assert_eq!(ack.capture_id, "OK-1");
    assert_eq!(s.consecutive_timeouts(), 0);
}

#[test]
fn control_frame_exact_4k_accepted() {
    let mut s = fixture("ack-pad-4096");
    let ack = s.capture(&msg("OK-1")).expect("4 KiB control frame within bound");
    assert_eq!(ack.capture_id, "OK-1");
}

#[test]
fn control_frame_oversize_rejected_never_ok() {
    let mut s = fixture("ack-oversize");
    let result = s.capture(&msg("OK-1"));
    assert!(result.is_err(), "oversize control frame must never be Ok: {result:?}");
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::ProtocolError);
    assert_reaped(s.last_reap().expect("reap recorded"), "ack-oversize");
}
