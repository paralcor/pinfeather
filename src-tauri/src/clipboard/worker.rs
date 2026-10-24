//! 抓取 worker 的 supervisor：唯一持有子进程、Job、管道与读取线程。
//! 产品路径固定 current_exe + --capture-worker-v1；仅 test-fixtures 可指定 fixture 与受控故障模式。
//! 本模块只传输并校验快照，不直接读剪贴板、写库或启动 WebView。

use std::collections::VecDeque;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::clipboard::protocol::{
    check_correlation, encode_request_message, parse_hello_json, parse_worker_message_json,
    WorkerMessage,
};
use crate::contracts::{ContractError, ContractResult, ErrorCode};

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(2000);
pub const CAPTURE_TIMEOUT: Duration = Duration::from_millis(750);
pub const CLEANUP_TIMEOUT: Duration = Duration::from_millis(2000);

/// reader 最多缓存2帧；测试ACK限4KiB，产品快照限18MiB，hello始终限4KiB。
const READER_QUEUE_FRAMES: usize = 2;
const SETTLED_ID_MEMORY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Starting,
    Ready,
    InFlight,
    Reaping,
    FailedPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerIdentity {
    pub pid: u32,
    pub create_time: u64,
}

#[derive(Debug, Clone)]
pub struct ReapReport {
    pub identity: Option<WorkerIdentity>,
    pub exited: bool,
    pub reader_stopped: bool,
    pub writer_stopped: bool,
    pub stderr_stopped: bool,
    pub cleanup_failed: bool,
    pub cleanup_elapsed: Duration,
}

/// 仅模块内部可见：产品构造固定 current_exe；测试构造走 feature 入口。
struct WorkerConfig {
    exe: PathBuf,
    args: Vec<String>,
    snapshot: bool,
}

enum PumpEvent {
    Frame(Vec<u8>),
    Stopped(Option<ContractError>),
}

struct WorkerProcess {
    _job: OwnedHandle,
    process: OwnedHandle,
    stdin_write: OwnedHandle,
    _stdout_read: OwnedHandle,
    _stderr_read: OwnedHandle,
    identity: WorkerIdentity,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    /// 持续排空 stderr，避免 worker 诊断写满管道后阻塞；宿主尚无日志设施，内容直接丢弃。
    stderr: Option<JoinHandle<()>>,
    /// 有界帧队列：sync_channel(2)，大小取决于控制/快照模式。
    frames: Receiver<Vec<u8>>,
    /// 终止/失败通知：独立于帧队列容量，读者绝不阻塞在此发送上。
    status: Receiver<Option<ContractError>>,
    reader_status: Option<Option<ContractError>>,
}

// Drop 也不能丢掉未确认线程及其借用的句柄。不可恢复的清理失败保留至进程退出，
// 并阻止本进程再创建 worker；正常/可重试 close 均不走此路径。
static FAILED_WORKERS: Mutex<Vec<WorkerProcess>> = Mutex::new(Vec::new());

pub struct WorkerSupervisor {
    config: WorkerConfig,
    state: WorkerState,
    closed: bool,
    consecutive_timeouts: u32,
    spawn_count: u32,
    late_frames_discarded: u64,
    last_reap: Option<ReapReport>,
    worker: Option<WorkerProcess>,
    settled_ids: VecDeque<String>,
    #[cfg(feature = "test-fixtures")]
    test_processing_delay: Option<Duration>,
    #[cfg(feature = "test-fixtures")]
    test_write_gate: Option<Receiver<()>>,
    #[cfg(feature = "test-fixtures")]
    test_wait_reader_stop: bool,
}

impl WorkerSupervisor {
    /// 产品路径：当前产品 exe + --capture-worker-v1，不接受任意可执行文件。
    pub fn for_product() -> Self {
        Self::with_config(WorkerConfig {
            exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pinfeather.exe")),
            args: vec![String::from("--capture-worker-v1")],
            snapshot: true,
        })
    }

    /// 测试构造入口（仅 test-fixtures）：Cargo 定位的 fixture exe + 受控故障模式。
    #[cfg(feature = "test-fixtures")]
    pub fn for_test_fixture(exe: PathBuf, mode: &str) -> Self {
        Self::with_config(WorkerConfig {
            exe,
            args: vec![format!("--mode={mode}")],
            snapshot: false,
        })
    }

    #[cfg(feature = "test-fixtures")]
    pub fn for_snapshot_fixture(exe: PathBuf, mode: &str) -> Self {
        Self::with_config(WorkerConfig{exe,args:vec![format!("--mode={mode}")],snapshot:true})
    }
    #[cfg(feature="test-fixtures")]
    pub fn for_test_product(exe:PathBuf)->Self{Self::with_config(WorkerConfig{exe,args:vec!["--capture-worker-v1".into()],snapshot:true})}

    fn with_config(config: WorkerConfig) -> Self {
        WorkerSupervisor {
            config,
            state: if FAILED_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
                WorkerState::Ready
            } else {
                WorkerState::FailedPaused
            },
            closed: false,
            consecutive_timeouts: 0,
            spawn_count: 0,
            late_frames_discarded: 0,
            last_reap: None,
            worker: None,
            settled_ids: VecDeque::new(),
            #[cfg(feature = "test-fixtures")]
            test_processing_delay: None,
            #[cfg(feature = "test-fixtures")]
            test_write_gate: None,
            #[cfg(feature = "test-fixtures")]
            test_wait_reader_stop: false,
        }
    }

    pub fn state(&self) -> WorkerState {
        self.state
    }

    pub fn consecutive_timeouts(&self) -> u32 {
        self.consecutive_timeouts
    }

    pub fn spawn_count(&self) -> u32 {
        self.spawn_count
    }

    pub fn late_frames_discarded(&self) -> u64 {
        self.late_frames_discarded
    }

    pub fn last_reap(&self) -> Option<&ReapReport> {
        self.last_reap.as_ref()
    }

    pub fn current_identity(&self) -> Option<WorkerIdentity> {
        self.worker.as_ref().map(|w| w.identity)
    }

    /// 清理超时后仍未确认退出、被保留的线程数量（应为 0）。
    pub fn stalled_reader_count(&self) -> usize {
        usize::from(self.state == WorkerState::FailedPaused && self.worker.as_ref().is_some_and(|w| w.reader.is_some()))
    }

    pub fn pending_writer_count(&self) -> usize {
        usize::from(self.worker.as_ref().is_some_and(|w| w.writer.is_some()))
    }

    /// 测试专用：注入处理耗时（模拟慢解码），驱动真实截止判定分支。
    #[cfg(feature = "test-fixtures")]
    pub fn set_test_processing_delay(&mut self, delay: Duration) {
        self.test_processing_delay = Some(delay);
    }

    /// 测试分离发送与接收，以便在已确认 reader 溢出后直接验证 close。
    #[cfg(feature = "test-fixtures")]
    pub fn begin_test_request(&mut self, request: &WorkerMessage) -> ContractResult<()> {
        let frame = encode_request_message(request)?;
        self.ensure_worker()?;
        self.write_request_cancellable(&frame, Instant::now() + CAPTURE_TIMEOUT)
    }

    #[cfg(feature = "test-fixtures")]
    pub fn wait_for_test_reader_stop(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.worker.as_ref().and_then(|w| w.reader.as_ref()).is_some_and(|h| !h.is_finished()) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.worker.is_some()
    }

    #[cfg(feature = "test-fixtures")]
    pub fn test_reader_error(&mut self) -> Option<ContractError> {
        self.reader_status().flatten()
    }

    /// 将 writer 保持在真实 WriteFile 之前，确定性覆盖取消早于 IO 的竞态。
    #[cfg(feature = "test-fixtures")]
    pub fn set_test_write_gate(&mut self, gate: Receiver<()>) {
        self.test_write_gate = Some(gate);
    }

    #[cfg(feature = "test-fixtures")]
    pub fn set_test_wait_reader_stop(&mut self) {
        self.test_wait_reader_stop = true;
    }

    /// ACK round trips exist only for the E03 transport fixture.
    #[cfg(feature = "test-fixtures")]
    pub fn capture(&mut self, request: &WorkerMessage) -> ContractResult<WorkerMessage> {
        if self.config.snapshot {return Err(ContractError::protocol_error("snapshot worker requires snapshot API"));}
        match self.exchange(request)? {WorkerReply::Ack(ack)=>Ok(ack),_=>Err(ContractError::protocol_error("expected control ACK"))}
    }

    pub fn capture_snapshot(&mut self, request: &WorkerMessage) -> ContractResult<super::protocol::ValidatedSnapshot> {
        if !self.config.snapshot {return Err(ContractError::protocol_error("control fixture cannot capture"));}
        match self.exchange(request)? {WorkerReply::Snapshot(snapshot)=>Ok(snapshot),_=>Err(ContractError::protocol_error("expected snapshot"))}
    }

    fn exchange(&mut self, request: &WorkerMessage) -> ContractResult<WorkerReply> {
        if self.closed {
            return Err(ContractError::new(ErrorCode::Busy, "supervisor closed"));
        }
        if self.state == WorkerState::FailedPaused {
            return Err(ContractError::new(ErrorCode::Busy, "supervisor paused"));
        }
        if self.state == WorkerState::InFlight {
            return Err(ContractError::new(ErrorCode::Busy, "capture already in flight"));
        }

        // F3：在任何副作用（建 worker/改状态/写管道）之前，用权威规则校验请求。
        let frame = encode_request_message(request)?;

        self.ensure_worker()?;

        // 发送前排空迟到帧（有界）；不得吞掉已停止事实。
        if let Some(status) = self.drain_stale() {
            return Err(self.fail_request(request, stopped_error(status), false));
        }

        let deadline = Instant::now() + CAPTURE_TIMEOUT;
        self.state = WorkerState::InFlight;

        // F3：发送同样受同一 deadline 控制，可取消并回收实际写操作。
        if let Err(e) = self.write_request_cancellable(&frame, deadline) {
            let timed_out = e.code == ErrorCode::Timeout;
            return Err(self.fail_request(request, e, timed_out));
        }

        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(self.fail_request(
                    request,
                    ContractError::new(ErrorCode::Timeout, "capture deadline exceeded"),
                    true,
                ));
            }
            match self.pump_next(deadline - now) {
                None => {
                    return Err(self.fail_request(
                        request,
                        ContractError::new(ErrorCode::Timeout, "capture deadline exceeded"),
                        true,
                    ));
                }
                Some(PumpEvent::Stopped(status)) => {
                    return Err(self.fail_request(request, stopped_error(status), false));
                }
                Some(PumpEvent::Frame(bytes)) => {
                    #[cfg(feature = "test-fixtures")]
                    if self.test_wait_reader_stop && !self.wait_for_test_reader_stop(deadline.saturating_duration_since(Instant::now())) {
                        return Err(self.fail_request(request, ContractError::protocol_error("test reader barrier timed out"), false));
                    }
                    // F1：解码/关联耗时也必须落在同一截止内。
                    #[cfg(feature = "test-fixtures")]
                    if let Some(delay) = self.test_processing_delay {
                        std::thread::sleep(delay);
                    }
                    match self.evaluate_frame(request, &bytes) {
                        FrameOutcome::Accepted(ack) => {
                            if let Some(err) = self.reader_status().flatten() {
                                return Err(self.fail_request(request, err, false));
                            }
                            if Instant::now() >= deadline {
                                return Err(self.fail_request(
                                    request,
                                    ContractError::new(
                                        ErrorCode::Timeout,
                                        "capture deadline exceeded",
                                    ),
                                    true,
                                ));
                            }
                            self.state = WorkerState::Ready;
                            if matches!(&ack,WorkerReply::Snapshot(s) if s.response.reason==Some(super::capture::CaptureReason::CloseFailed)) {
                                self.state=WorkerState::Reaping;
                                self.reap();
                                self.state=self.state_after_reap();
                            }
                            if matches!(&ack,WorkerReply::Ack(_)) || matches!(&ack,WorkerReply::Snapshot(s) if s.response.result==super::capture::CaptureResult::Captured) {
                                self.consecutive_timeouts = 0;
                            }
                            self.settle_id(&request.capture_id);
                            return Ok(ack);
                        }
                        FrameOutcome::DiscardedLate => continue,
                        FrameOutcome::Failed(err) => {
                            return Err(self.fail_request(request, err, false));
                        }
                    }
                }
            }
        }
    }

    /// 幂等关闭：仅在线程确认结束后释放句柄；失败保留资源，后续 close 可重试。
    pub fn close(&mut self) {
        if self.closed && self.worker.is_none() {
            return;
        }
        self.closed = true;
        self.state = WorkerState::Reaping;
        self.reap();
        self.state = self.state_after_reap();
    }

    fn reader_status(&mut self) -> Option<Option<ContractError>> {
        let wp = self.worker.as_mut()?;
        if wp.reader_status.is_none() {
            if let Ok(status) = wp.status.try_recv() {
                wp.reader_status = Some(status);
            }
        }
        wp.reader_status.clone()
    }

    fn pump_next(&mut self, timeout: Duration) -> Option<PumpEvent> {
        if let Some(err) = self.reader_status().flatten() {
            return Some(PumpEvent::Stopped(Some(err)));
        }
        let wp = self.worker.as_ref()?;
        let next = wp.frames.recv_timeout(timeout);
        // EOF 不遮住最后一份完整帧；fatal 错误则始终优先。
        if let Some(err) = self.reader_status().flatten() {
            return Some(PumpEvent::Stopped(Some(err)));
        }
        match next {
            Ok(body) => Some(PumpEvent::Frame(body)),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                Some(PumpEvent::Stopped(self.reader_status().flatten()))
            }
        }
    }

    fn write_request_cancellable(&mut self, frame: &[u8], deadline: Instant) -> ContractResult<()> {
        let handle = match self.worker.as_ref() {
            Some(wp) => wp.stdin_write.as_raw_handle(),
            None => return Err(ContractError::protocol_error("worker unavailable")),
        };
        let (result, stalled) = win::write_all_cancellable(handle, frame, deadline,
            #[cfg(feature = "test-fixtures")]
            self.test_write_gate.take(),
        );
        if let Some(join) = stalled {
            self.worker.as_mut().expect("writer belongs to worker").writer = Some(join);
        }
        result
    }

    fn evaluate_frame(&mut self, request: &WorkerMessage, bytes: &[u8]) -> FrameOutcome {
        let body = match std::str::from_utf8(bytes) {
            Ok(body) => body,
            Err(_) => {
                return FrameOutcome::Failed(ContractError::protocol_error("frame body not UTF-8"))
            }
        };
        let reply = if self.config.snapshot {
            match super::protocol::parse_snapshot(bytes) {Ok(s)=>WorkerReply::Snapshot(s),Err(e)=>return FrameOutcome::Failed(e)}
        } else {
            match parse_worker_message_json(body) {Ok(m)=>WorkerReply::Ack(m),Err(e)=>return FrameOutcome::Failed(e)}
        };
        let response = match &reply {WorkerReply::Ack(m)=>m.clone(),WorkerReply::Snapshot(s)=>s.message()};
        if response.capture_id != request.capture_id && self.settled_ids.contains(&response.capture_id)
        {
            self.late_frames_discarded += 1;
            return FrameOutcome::DiscardedLate;
        }
        match check_correlation(request, &response) {
            Ok(()) => FrameOutcome::Accepted(reply),
            Err(e) => FrameOutcome::Failed(e),
        }
    }

    fn settle_id(&mut self, id: &str) {
        while self.settled_ids.len() >= SETTLED_ID_MEMORY {
            self.settled_ids.pop_front();
        }
        self.settled_ids.push_back(id.to_string());
    }

    /// 有界排空（最多 READER_QUEUE_FRAMES 帧）。返回 None=reader 正常；
    /// Some(status)=reader 已停止（Stopped 事实不丢）。
    fn drain_stale(&mut self) -> Option<Option<ContractError>> {
        if let Some(status) = self.reader_status() {
            return Some(status);
        }
        let wp = match self.worker.as_ref() {
            Some(wp) => wp,
            None => return Some(None),
        };
        let mut discarded = 0u64;
        for _ in 0..READER_QUEUE_FRAMES {
            match wp.frames.try_recv() {
                Ok(_) => discarded += 1,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.late_frames_discarded += discarded;
                    return Some(self.reader_status().flatten());
                }
            }
        }
        self.late_frames_discarded += discarded;
        self.reader_status()
    }

    fn fail_request(
        &mut self,
        request: &WorkerMessage,
        err: ContractError,
        is_timeout: bool,
    ) -> ContractError {
        self.state = WorkerState::Reaping;
        self.settle_id(&request.capture_id);
        if is_timeout {
            self.consecutive_timeouts += 1;
        }
        self.reap();
        self.state = self.state_after_reap();
        err
    }

    fn fail_handshake(&mut self, err: ContractError) -> ContractError {
        self.state = WorkerState::Reaping;
        self.reap();
        self.state = self.state_after_reap();
        err
    }

    fn state_after_reap(&self) -> WorkerState {
        if self.worker.is_some() || self.last_reap.as_ref().map(|r| r.cleanup_failed) == Some(true) {
            WorkerState::FailedPaused
        } else if self.consecutive_timeouts >= 2 {
            WorkerState::FailedPaused
        } else {
            WorkerState::Ready
        }
    }

    fn ensure_worker(&mut self) -> ContractResult<()> {
        if self.closed || self.state == WorkerState::FailedPaused ||
            !FAILED_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
            self.state = WorkerState::FailedPaused;
            return Err(ContractError::new(ErrorCode::Busy, "supervisor paused"));
        }
        if self.worker.is_some() {
            return Ok(());
        }
        if self.consecutive_timeouts >= 2 {
            self.state = WorkerState::FailedPaused;
            return Err(ContractError::new(ErrorCode::Busy, "supervisor paused"));
        }
        self.state = WorkerState::Starting;
        // 握手截止从创建前起算：包含创建、Job 绑定与 hello 读取。
        let handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        match win::spawn_worker(&self.config) {
            Ok(process) => {
                self.spawn_count += 1;
                self.worker = Some(process);
            }
            Err(e) => {
                self.state = WorkerState::Ready;
                return Err(e);
            }
        }

        loop {
            let now = Instant::now();
            if now >= handshake_deadline {
                return Err(self.fail_handshake(ContractError::new(
                    ErrorCode::Timeout,
                    "handshake deadline exceeded",
                )));
            }
            match self.pump_next(handshake_deadline - now) {
                None => {
                    return Err(self.fail_handshake(ContractError::new(
                        ErrorCode::Timeout,
                        "handshake deadline exceeded",
                    )));
                }
                Some(PumpEvent::Stopped(status)) => {
                    return Err(self.fail_handshake(stopped_error(status)));
                }
                Some(PumpEvent::Frame(bytes)) => {
                    let hello = match std::str::from_utf8(&bytes) {
                        Ok(body) => parse_hello_json(body),
                        Err(_) => Err(ContractError::protocol_error("hello frame body not UTF-8")),
                    };
                    return match hello {
                        Ok(()) => {
                            if let Some(err) = self.reader_status().flatten() {
                                return Err(self.fail_handshake(err));
                            }
                            // F1：解码完成后仍须落在握手截止内。
                            if Instant::now() >= handshake_deadline {
                                return Err(self.fail_handshake(ContractError::new(
                                    ErrorCode::Timeout,
                                    "handshake deadline exceeded",
                                )));
                            }
                            self.state = WorkerState::Ready;
                            Ok(())
                        }
                        Err(e) => Err(self.fail_handshake(e)),
                    };
                }
            }
        }
    }

    fn reap(&mut self) {
        let start = Instant::now();
        let deadline = start + CLEANUP_TIMEOUT;
        let mut report = ReapReport {
            identity: None,
            exited: true,
            reader_stopped: true,
            writer_stopped: true,
            stderr_stopped: true,
            cleanup_failed: false,
            cleanup_elapsed: Duration::ZERO,
        };

        if let Some(mut wp) = self.worker.take() {
            report.identity = Some(wp.identity);
            win::terminate(wp.process.as_raw_handle());
            // writer、reader 与 stderr 排空共用同一个清理截止；循环取消覆盖取消早于 IO 的竞态。
            report.writer_stopped = win::finish_thread(&mut wp.writer, deadline);
            report.reader_stopped = win::finish_thread(&mut wp.reader, deadline);
            report.stderr_stopped = win::finish_thread(&mut wp.stderr, deadline);
            report.exited = win::wait_exit(wp.process.as_raw_handle(), deadline);
            for _ in 0..READER_QUEUE_FRAMES {
                if wp.frames.try_recv().is_err() {
                    break;
                }
                self.late_frames_discarded += 1;
            }
            report.cleanup_failed = !(report.exited
                && report.reader_stopped
                && report.writer_stopped
                && report.stderr_stopped);
            if report.cleanup_failed {
                // 仍有线程借用管道，不能关闭/复用句柄，也不能创建另一 worker。
                self.worker = Some(wp);
            }
            // 全部确认后 wp 的 OwnedHandle 才释放 Job/进程/管道。
        }

        report.cleanup_elapsed = start.elapsed();
        self.last_reap = Some(report);
    }
}

enum WorkerReply { Ack(WorkerMessage), Snapshot(super::protocol::ValidatedSnapshot) }

enum FrameOutcome {
    Accepted(WorkerReply),
    DiscardedLate,
    Failed(ContractError),
}

fn stopped_error(status: Option<ContractError>) -> ContractError {
    status.unwrap_or_else(|| ContractError::protocol_error("worker terminated before response"))
}

impl Drop for WorkerSupervisor {
    fn drop(&mut self) {
        self.close();
        if let Some(worker) = self.worker.take() {
            eprintln!("pinfeather: worker cleanup_failed; resources retained, worker creation disabled");
            FAILED_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).push(worker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::protocol::CONTROL_FRAME_MAX_BYTES;

    #[test]
    fn constants_match_plan() {
        assert_eq!(HANDSHAKE_TIMEOUT, Duration::from_millis(2000));
        assert_eq!(CAPTURE_TIMEOUT, Duration::from_millis(750));
        assert_eq!(CLEANUP_TIMEOUT, Duration::from_millis(2000));
        assert_eq!(READER_QUEUE_FRAMES, 2);
        assert_eq!(CONTROL_FRAME_MAX_BYTES, 4 * 1024);
    }

    #[test]
    fn close_is_idempotent_without_worker() {
        let mut s = WorkerSupervisor::with_config(WorkerConfig {
            exe: PathBuf::from("unused.exe"),
            args: vec![],
            snapshot: false,
        });
        s.close();
        s.close();
        assert_eq!(s.state(), WorkerState::Ready);
    }

    #[test]
    fn write_deadline_cancels_blocked_pipe() {
        // 真实匿名管道 + 大于缓冲的写入制造受控阻塞；驱动生产的发送/取消/清理路径。
        let (read_h, write_h) = unsafe { win::create_test_pipe() };
        let payload = vec![0u8; 1024 * 1024];
        let started = Instant::now();
        let (result, mut stalled) =
            win::write_all_cancellable(write_h, &payload, Instant::now() + Duration::from_millis(200),
                #[cfg(feature = "test-fixtures")]
                None,
            );
        assert!(win::finish_thread(&mut stalled, Instant::now() + CLEANUP_TIMEOUT));
        let elapsed = started.elapsed();
        eprintln!("write cancel: elapsed={:?} stalled={}", elapsed, stalled.is_some());
        assert!(result.is_err(), "blocked write must not succeed");
        assert!(stalled.is_none(), "write thread must be confirmed exited");
        assert!(elapsed >= Duration::from_millis(150), "deadline must be honored, got {elapsed:?}");
        assert!(elapsed <= Duration::from_millis(2000), "cancel must be bounded, got {elapsed:?}");
        win::close_handle(read_h);
        win::close_handle(write_h);
    }
}

mod win {
    use super::{WorkerConfig, WorkerIdentity, WorkerProcess};
    use crate::clipboard::protocol::{parse_frame_prefix, CONTROL_FRAME_MAX_BYTES, FRAME_LENGTH_BYTES};
    use crate::contracts::{ContractError, ContractResult, ErrorCode};
    use std::sync::mpsc::{self, Sender, SyncSender, TrySendError};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    pub use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, SetHandleInformation, ERROR_BROKEN_PIPE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::IO::CancelSynchronousIo;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetProcessTimes,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT,
        LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
        STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };

    type ReaderEvent = Option<ContractError>;

    fn last_error(message: &str) -> ContractError {
        ContractError::new(ErrorCode::CleanupFailed, message)
    }

    pub fn close_handle(handle: HANDLE) {
        if !handle.is_null() {
            unsafe { CloseHandle(handle) };
        }
    }

    pub fn terminate(process: HANDLE) {
        if !process.is_null() {
            unsafe { TerminateProcess(process, 1) };
        }
    }

    pub fn wait_exit(process: HANDLE, deadline: Instant) -> bool {
        let now = Instant::now();
        let budget = if now >= deadline {
            0u32
        } else {
            (deadline - now).as_millis() as u32
        };
        unsafe { WaitForSingleObject(process, budget) == WAIT_OBJECT_0 }
    }

    /// 仅在确认线程完成后 join；取消是请求，不能当作退出确认。
    pub fn finish_thread(thread: &mut Option<JoinHandle<()>>, deadline: Instant) -> bool {
        let Some(join) = thread.as_ref() else { return true };
        // Rust is_finished 不保证 OS 线程析构已结束；等待原生线程句柄 signaled 才 join。
        while unsafe { WaitForSingleObject(join.as_raw_handle(), 0) } != WAIT_OBJECT_0 {
            // 同步匿名管道 IO 使用线程级取消；反复请求以覆盖线程启动竞态。
            unsafe { CancelSynchronousIo(join.as_raw_handle()) };
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let millis = (deadline - now).as_millis().clamp(1, 5) as u32;
            unsafe { WaitForSingleObject(join.as_raw_handle(), millis) };
        }
        let _ = thread.take().expect("completed thread retained").join();
        true
    }

    fn read_exact(handle: HANDLE, buf: &mut [u8]) -> ContractResult<usize> {
        let mut filled = 0usize;
        while filled < buf.len() {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buf[filled..].as_mut_ptr(),
                    (buf.len() - filled) as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
                    return Ok(filled);
                }
                return Err(ContractError::protocol_error("pipe read failed"));
            }
            if read == 0 {
                return Ok(filled);
            }
            filled += read as usize;
        }
        Ok(filled)
    }

    fn write_all(handle: HANDLE, mut data: &[u8], deadline: Instant) -> ContractResult<()> {
        while !data.is_empty() {
            if Instant::now() >= deadline {
                return Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded"));
            }
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    handle,
                    data.as_ptr(),
                    data.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || written == 0 {
                return Err(ContractError::protocol_error("pipe write failed"));
            }
            data = &data[written as usize..];
        }
        Ok(())
    }

    /// 阻塞写由专属线程执行；到期后将线程交回 supervisor，在同一清理预算内回收。
    pub fn write_all_cancellable(
        handle: HANDLE,
        data: &[u8],
        deadline: Instant,
        #[cfg(feature = "test-fixtures")] gate: Option<mpsc::Receiver<()>>,
    ) -> (ContractResult<()>, Option<JoinHandle<()>>) {
        if Instant::now() >= deadline {
            return (Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded")), None);
        }
        let data = data.to_vec();
        let handle_usize = handle as usize;
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            #[cfg(feature = "test-fixtures")]
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            let result = write_all(handle_usize as HANDLE, &data, deadline);
            let _ = tx.send(result);
        });
        let now = Instant::now();
        let budget = if now >= deadline {
            Duration::ZERO
        } else {
            deadline - now
        };
        let mut pending = Some(join);
        match rx.recv_timeout(budget) {
            Ok(result) => {
                if finish_thread(&mut pending, deadline) {
                    (result, None)
                } else {
                    (Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded")), pending)
                }
            }
            Err(_) => {
                (Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded")), pending)
            }
        }
    }

    /// 有界前缀+正文完整帧读取；干净 EOF 返回 None，半帧/读失败/超限给安全协议错误。
    fn read_frame(handle: HANDLE, max: usize) -> ContractResult<Option<Vec<u8>>> {
        let mut prefix = [0u8; FRAME_LENGTH_BYTES];
        let got = read_exact(handle, &mut prefix)?;
        if got == 0 {
            return Ok(None);
        }
        if got < prefix.len() {
            return Err(ContractError::protocol_error("unexpected end of stream"));
        }
        let declared = parse_frame_prefix(prefix, max)?;
        let mut body = vec![0u8; declared];
        let got = read_exact(handle, &mut body)?;
        if got < declared {
            return Err(ContractError::protocol_error("unexpected end of stream"));
        }
        Ok(Some(body))
    }

    /// 读线程：帧走有界队列（满=协议失败并退出），终止通知走独立通道且绝不阻塞。
    fn reader_loop(handle: HANDLE, frames: SyncSender<Vec<u8>>, status: Sender<ReaderEvent>, snapshot: bool) {
        let mut hello = true;
        loop {
            let max=if hello || !snapshot {CONTROL_FRAME_MAX_BYTES}else{crate::contracts::WORKER_RESPONSE_MAX_BYTES};
            match read_frame(handle, max) {
                Ok(None) => {
                    let _ = status.send(None);
                    break;
                }
                Ok(Some(body)) => match frames.try_send(body) {
                    Ok(()) => {hello=false;}
                    Err(TrySendError::Full(_)) => {
                        let _ = status.send(Some(ContractError::protocol_error(
                            "reader queue overflow",
                        )));
                        break;
                    }
                    Err(TrySendError::Disconnected(_)) => break,
                },
                Err(e) => {
                    let _ = status.send(Some(e));
                    break;
                }
            }
        }
    }

    /// stderr 排空线程：固定缓冲读取后丢弃，EOF、断管或取消即退出。
    fn drain_loop(handle: HANDLE) {
        let mut sink = [0u8; 4096];
        loop {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    sink.as_mut_ptr(),
                    sink.len() as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                break;
            }
        }
    }

    fn quote_wide(arg: &[u16], out: &mut Vec<u16>) {
        out.push(b'"' as u16);
        out.extend_from_slice(arg);
        out.push(b'"' as u16);
        out.push(b' ' as u16);
    }

    pub fn spawn_worker(config: &WorkerConfig) -> ContractResult<WorkerProcess> {
        unsafe { spawn_worker_inner(config) }
    }

    unsafe fn spawn_worker_inner(config: &WorkerConfig) -> ContractResult<WorkerProcess> {
        use std::os::windows::ffi::OsStrExt;

        let mut cmdline: Vec<u16> = Vec::new();
        quote_wide(&config.exe.as_os_str().encode_wide().collect::<Vec<u16>>(), &mut cmdline);
        for arg in &config.args {
            quote_wide(&arg.encode_utf16().collect::<Vec<u16>>(), &mut cmdline);
        }
        while cmdline.last() == Some(&(b' ' as u16)) {
            cmdline.pop();
        }
        cmdline.push(0);

        let mut stdin_read: HANDLE = std::ptr::null_mut();
        let mut stdin_write: HANDLE = std::ptr::null_mut();
        let mut stdout_read: HANDLE = std::ptr::null_mut();
        let mut stdout_write: HANDLE = std::ptr::null_mut();
        let mut stderr_read: HANDLE = std::ptr::null_mut();
        let mut stderr_write: HANDLE = std::ptr::null_mut();

        if CreatePipe(&mut stdin_read, &mut stdin_write, std::ptr::null(), 0) == 0
            || CreatePipe(&mut stdout_read, &mut stdout_write, std::ptr::null(), 0) == 0
            || CreatePipe(&mut stderr_read, &mut stderr_write, std::ptr::null(), 0) == 0
        {
            close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
            return Err(last_error("worker spawn failed"));
        }
        // 仅子进程端点可继承。
        for child_end in [stdin_read, stdout_write, stderr_write] {
            if SetHandleInformation(child_end, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
                return Err(last_error("worker spawn failed"));
            }
        }

        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
            return Err(last_error("worker spawn failed"));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            close_handle(job);
            close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
            return Err(last_error("worker spawn failed"));
        }

        // 仅继承三根子进程端点。
        let handles: [HANDLE; 3] = [stdin_read, stdout_write, stderr_write];
        let mut attr_size: usize = 0;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size);
        let mut attr_buf = vec![0u8; attr_size];
        let attr_list = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) == 0 {
            close_handle(job);
            close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
            return Err(last_error("worker spawn failed"));
        }
        if UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr() as *const core::ffi::c_void,
            std::mem::size_of_val(&handles),
            std::ptr::null_mut(),
            std::ptr::null(),
        ) == 0
        {
            DeleteProcThreadAttributeList(attr_list);
            close_handle(job);
            close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
            return Err(last_error("worker spawn failed"));
        }

        let mut siex: STARTUPINFOEXW = std::mem::zeroed();
        siex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        siex.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        siex.StartupInfo.hStdInput = stdin_read;
        siex.StartupInfo.hStdOutput = stdout_write;
        siex.StartupInfo.hStdError = stderr_write;
        siex.lpAttributeList = attr_list;

        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let created = CreateProcessW(
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &siex.StartupInfo as *const _ as *const _,
            &mut pi,
        );
        DeleteProcThreadAttributeList(attr_list);
        // 父端的子进程端副本立即关闭：子进程退出后读端才能观察到 EOF。
        close_handle(stdin_read);
        close_handle(stdout_write);
        close_handle(stderr_write);
        if created == 0 {
            close_handle(job);
            close_all(&[stdin_write, stdout_read, stderr_read]);
            return Err(last_error("worker spawn failed"));
        }
        close_handle(pi.hThread);

        // 处理任何请求前归入专属 Job；失败立即清理，不无 Job 运行。
        if AssignProcessToJobObject(job, pi.hProcess) == 0 {
            TerminateProcess(pi.hProcess, 1);
            WaitForSingleObject(pi.hProcess, 1000);
            close_handle(job);
            close_all(&[stdin_write, stdout_read, stderr_read, pi.hProcess]);
            return Err(last_error("job assign failed"));
        }

        let mut creation = std::mem::zeroed();
        let mut exit_t = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        let mut create_time: u64 = 0;
        if GetProcessTimes(pi.hProcess, &mut creation, &mut exit_t, &mut kernel, &mut user) != 0 {
            create_time = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        }
        let identity = WorkerIdentity {
            pid: pi.dwProcessId,
            create_time,
        };

        let (frame_tx, frame_rx) = mpsc::sync_channel(super::READER_QUEUE_FRAMES);
        let (status_tx, status_rx) = mpsc::channel();
        let reader_handle = stdout_read as usize;
        let snapshot = config.snapshot;
        let reader = thread::spawn(move || {
            reader_loop(reader_handle as HANDLE, frame_tx, status_tx, snapshot);
        });
        let stderr_handle = stderr_read as usize;
        let stderr = thread::spawn(move || drain_loop(stderr_handle as HANDLE));

        Ok(WorkerProcess {
            _job: OwnedHandle::from_raw_handle(job),
            process: OwnedHandle::from_raw_handle(pi.hProcess),
            stdin_write: OwnedHandle::from_raw_handle(stdin_write),
            _stdout_read: OwnedHandle::from_raw_handle(stdout_read),
            _stderr_read: OwnedHandle::from_raw_handle(stderr_read),
            identity,
            reader: Some(reader),
            writer: None,
            stderr: Some(stderr),
            frames: frame_rx,
            status: status_rx,
            reader_status: None,
        })
    }

    fn close_all(handles: &[HANDLE]) {
        for h in handles {
            close_handle(*h);
        }
    }

    /// 测试用：创建一对匿名管道句柄（读端, 写端）。
    #[cfg(test)]
    pub unsafe fn create_test_pipe() -> (HANDLE, HANDLE) {
        let mut read_h: HANDLE = std::ptr::null_mut();
        let mut write_h: HANDLE = std::ptr::null_mut();
        CreatePipe(&mut read_h, &mut write_h, std::ptr::null(), 4096);
        (read_h, write_h)
    }
}
