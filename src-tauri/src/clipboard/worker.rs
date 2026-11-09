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
    #[cfg(all(test, feature = "test-fixtures"))]
    mock_spawn: Option<std::sync::Arc<r3_spawn_tests::Effects>>,
}

impl WorkerConfig {
    fn cleanup_pending(&self) -> bool {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let Some(effects) = &self.mock_spawn {
            return effects.pending.load(std::sync::atomic::Ordering::Acquire) != 0;
        }
        PENDING_SPAWN_CLEANUPS.load(std::sync::atomic::Ordering::Acquire) != 0
            || !FAILED_WORKERS.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }
}

// Failed startup must retain its process and any borrowed IO handles. This guard
// follows that owner, including retained Drop resources, and blocks replacement.
static PENDING_SPAWN_CLEANUPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct SpawnCleanupGuard {
    #[cfg(all(test, feature = "test-fixtures"))]
    mock: Option<std::sync::Arc<r3_spawn_tests::Effects>>,
}
impl SpawnCleanupGuard {
    fn new(process: &WorkerHandle) -> Self {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let WorkerHandle::Mock(handle) = process {
            handle.effects.pending.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            return Self { mock: Some(handle.effects.clone()) };
        }
        #[cfg(not(all(test, feature = "test-fixtures")))]
        let _ = process;
        PENDING_SPAWN_CLEANUPS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self { #[cfg(all(test, feature = "test-fixtures"))] mock: None }
    }
}
impl Drop for SpawnCleanupGuard {
    fn drop(&mut self) {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let Some(effects) = &self.mock {
            effects.pending.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            return;
        }
        PENDING_SPAWN_CLEANUPS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

struct SpawnFailure {
    error: ContractError,
    worker: Option<WorkerProcess>,
}
impl From<ContractError> for SpawnFailure {
    fn from(error: ContractError) -> Self { Self { error, worker: None } }
}

// Test handles never acquire or expose a fabricated native handle.
enum WorkerHandle {
    Native(OwnedHandle),
    #[cfg(all(test, feature = "test-fixtures"))]
    Mock(r3_spawn_tests::MockHandle),
}

impl AsRawHandle for WorkerHandle {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        match self {
            Self::Native(handle) => handle.as_raw_handle(),
            #[cfg(all(test, feature = "test-fixtures"))]
            Self::Mock(_) => panic!("mock handle must not reach native IO"),
        }
    }
}

enum PumpEvent {
    Frame(Vec<u8>),
    Stopped(Option<ContractError>),
}

struct WorkerProcess {
    _job: WorkerHandle,
    process: WorkerHandle,
    stdin_write: WorkerHandle,
    _stdout_read: WorkerHandle,
    _stderr_read: WorkerHandle,
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
    // Drop last, after the native handles have been released upon confirmed exit.
    spawn_cleanup: Option<SpawnCleanupGuard>,
}

// Drop 也不能丢掉未确认线程及其借用的句柄。不可恢复的清理失败保留至进程退出，
// 并阻止本进程再创建 worker；正常/可重试 close 均不走此路径。
static FAILED_WORKERS: Mutex<Vec<WorkerProcess>> = Mutex::new(Vec::new());

pub struct WorkerSupervisor {
    config: WorkerConfig,
    state: WorkerState,
    closed: bool,
    closing:Option<std::sync::Arc<crate::runtime::CloseControl>>,
    close_deadline:Option<Instant>,
    cleanup_failed:bool,
    #[cfg(all(test,feature="test-fixtures"))]
    test_wait_started:Option<std::sync::mpsc::SyncSender<()>>,
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
            #[cfg(all(test, feature = "test-fixtures"))]
            mock_spawn: None,
        })
    }

    /// 测试构造入口（仅 test-fixtures）：Cargo 定位的 fixture exe + 受控故障模式。
    #[cfg(feature = "test-fixtures")]
    pub fn for_test_fixture(exe: PathBuf, mode: &str) -> Self {
        Self::with_config(WorkerConfig {
            exe,
            args: vec![format!("--mode={mode}")],
            snapshot: false,
            #[cfg(all(test, feature = "test-fixtures"))]
            mock_spawn: None,
        })
    }

    #[cfg(feature = "test-fixtures")]
    pub fn for_snapshot_fixture(exe: PathBuf, mode: &str) -> Self {
        Self::with_config(WorkerConfig{exe,args:vec![format!("--mode={mode}")],snapshot:true,#[cfg(all(test, feature="test-fixtures"))]mock_spawn:None})
    }
    #[cfg(feature="test-fixtures")]
    pub fn for_test_product(exe:PathBuf)->Self{Self::with_config(WorkerConfig{exe,args:vec!["--capture-worker-v1".into()],snapshot:true,#[cfg(all(test, feature="test-fixtures"))]mock_spawn:None})}

    fn with_config(config: WorkerConfig) -> Self {
        let state = if !config.cleanup_pending() {
                WorkerState::Ready
            } else {
                WorkerState::FailedPaused
            };
        WorkerSupervisor {
            config,
            state,
            closed: false,
            closing:None,close_deadline:None,cleanup_failed:false,
            #[cfg(all(test,feature="test-fixtures"))]test_wait_started:None,
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
    pub fn set_close_control(&mut self,control:std::sync::Arc<crate::runtime::CloseControl>){self.closing=Some(control);}
    #[cfg(all(test,feature="test-fixtures"))]
    pub(crate) fn m2_synthetic(waits:&[u32])->Self{let effects=r3_spawn_tests::Effects::new(true,true,true,waits);let mut owner=effects.supervisor();owner.ensure_worker().unwrap();owner}
    #[cfg(all(test,feature="test-fixtures"))]
    pub(crate) fn m2_deadline(&self)->Option<Instant>{self.close_deadline}
    fn is_closing(&self)->bool{self.closing.as_ref().is_some_and(|c|c.closing())}
    fn cleanup_deadline(&self,normal:Instant)->Instant{self.closing.as_ref().map_or(self.close_deadline.unwrap_or(normal),|c|c.deadline(self.close_deadline.unwrap_or(normal)))}

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
        if self.closed || self.is_closing() {
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
        let mut stale = self.drain_stale();
        if matches!(stale, Some(None)) {
            // 空闲期 worker 已退出（干净 EOF 或崩溃断管），本次请求尚未发出：
            // 确认回收后重建一次再发送，不丢这次抓取；空闲期协议错误仍按失败处理。
            self.state = WorkerState::Reaping;
            self.reap();
            self.state = self.state_after_reap();
            self.ensure_worker()?;
            stale = self.drain_stale();
        }
        if let Some(status) = stale {
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
                            if self.is_closing(){return Err(self.fail_request(request,ContractError::new(ErrorCode::Busy,"capture closing"),false));}
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
        if self.closed && self.worker.is_none() && !self.config.cleanup_pending()
            && !self.last_reap.as_ref().is_some_and(|r| r.cleanup_failed) {
            return;
        }
        self.closed = true;
        self.state = WorkerState::Reaping;
        self.reap();
        self.state = self.state_after_reap();
    }
    pub fn close_until(&mut self,deadline:Instant)->ContractResult<()> {
        self.close_deadline=Some(deadline);self.closed=true;self.state=WorkerState::Reaping;
        self.reap_with_deadline(deadline);self.state=self.state_after_reap();
        if self.cleanup_failed{return Err(ContractError::new(ErrorCode::CleanupFailed,"抓取资源回收失败，退出状态已保留。"));}
        if self.worker.is_some(){return Err(ContractError::new(ErrorCode::Timeout,"抓取资源仍在回收，请保留窗口后重试。"));}
        if self.config.cleanup_pending(){return Err(ContractError::new(ErrorCode::CleanupFailed,"仍有保留的抓取资源尚未确认回收。"));}
        Ok(())
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
        let deadline=Instant::now()+timeout;
        loop {
        if self.is_closing(){return Some(PumpEvent::Stopped(Some(ContractError::new(ErrorCode::Busy,"capture closing"))));}
        if let Some(err) = self.reader_status().flatten() {
            return Some(PumpEvent::Stopped(Some(err)));
        }
        let wp = self.worker.as_ref()?;
        #[cfg(all(test,feature="test-fixtures"))]if let Some(started)=self.test_wait_started.take(){let _=started.send(());}
        let next = wp.frames.recv_timeout(deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(10)));
        // EOF 不遮住最后一份完整帧；fatal 错误则始终优先。
        if let Some(err) = self.reader_status().flatten() {
            return Some(PumpEvent::Stopped(Some(err)));
        }
        match next {
            Ok(body) => return Some(PumpEvent::Frame(body)),
            Err(RecvTimeoutError::Timeout) => if Instant::now()>=deadline{return None;}else{continue;},
            Err(RecvTimeoutError::Disconnected) => {
                return Some(PumpEvent::Stopped(self.reader_status().flatten()));
            }
        }
        }
    }

    fn write_request_cancellable(&mut self, frame: &[u8], deadline: Instant) -> ContractResult<()> {
        let handle = match self.worker.as_ref() {
            Some(wp) => wp.stdin_write.as_raw_handle(),
            None => return Err(ContractError::protocol_error("worker unavailable")),
        };
        let (result, stalled) = win::write_all_cancellable(handle, frame, deadline,
            self.closing.clone(),
            #[cfg(feature = "test-fixtures")]
            self.test_write_gate.take(),
        );
        if let Some(join) = stalled {
            self.worker.as_mut().expect("writer belongs to worker").writer = Some(join);
        }
        result
    }

    fn evaluate_frame(&mut self, request: &WorkerMessage, bytes: &[u8]) -> FrameOutcome {
        if self.is_closing(){return FrameOutcome::Failed(ContractError::new(ErrorCode::Busy,"capture closing"));}
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
        if self.closed || self.is_closing() || self.state == WorkerState::FailedPaused || self.config.cleanup_pending() {
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
                self.worker = e.worker;
                if self.worker.is_some() {
                    self.state = WorkerState::Reaping;
                    // Preserve the partial-startup branch's original wait budget.
                    self.reap_with_budget(Duration::from_millis(1000));
                    self.state = self.state_after_reap();
                } else {
                    self.state = WorkerState::Ready;
                }
                return Err(e.error);
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
                            if self.is_closing(){return Err(self.fail_handshake(ContractError::new(ErrorCode::Busy,"capture closing")));}
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
        self.reap_with_budget(CLEANUP_TIMEOUT);
    }

    fn reap_with_budget(&mut self, budget: Duration) {
        self.reap_with_deadline(self.cleanup_deadline(Instant::now()+budget));
    }
    fn reap_with_deadline(&mut self,deadline:Instant) {
        let start = Instant::now();
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
            win::terminate_owned(&wp.process);
            // writer、reader 与 stderr 排空共用同一个清理截止；循环取消覆盖取消早于 IO 的竞态。
            report.writer_stopped = win::finish_thread_controlled(&mut wp.writer, deadline,self.closing.as_ref());
            report.reader_stopped = win::finish_thread_controlled(&mut wp.reader, deadline,self.closing.as_ref());
            report.stderr_stopped = win::finish_thread_controlled(&mut wp.stderr, deadline,self.closing.as_ref());
            self.cleanup_failed|=(!report.writer_stopped&&wp.writer.is_none())||(!report.reader_stopped&&wp.reader.is_none())||(!report.stderr_stopped&&wp.stderr.is_none());
            report.exited = win::wait_owned_controlled(&wp.process, deadline,self.closing.as_ref());
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

        // A replacement supervisor must also report globally retained cleanup
        // through the existing runtime close/reset result consumer.
        report.cleanup_failed |= self.config.cleanup_pending();
        report.cleanup_failed |= self.cleanup_failed;
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
        if let Some(deadline)=self.close_deadline{let _=self.close_until(deadline);}else{self.close();}
        if let Some(worker) = self.worker.take() {
            eprintln!("pinfeather: worker cleanup_failed; resources retained, worker creation disabled");
            #[cfg(all(test, feature = "test-fixtures"))]
            if let WorkerHandle::Mock(handle) = &worker.process {
                let effects = handle.effects.clone();
                effects.retained.lock().unwrap().push(worker);
                return;
            }
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
            #[cfg(all(test, feature = "test-fixtures"))]
            mock_spawn: None,
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
            win::write_all_cancellable(write_h, &payload, Instant::now() + Duration::from_millis(200),None,
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

#[cfg(all(test, feature = "test-fixtures"))]
mod r3_spawn_tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    pub(super) struct Effects {
        pub assigned: bool,
        created: bool,
        terminate_ok: bool,
        waits: Mutex<VecDeque<u32>>,
        pub spawns: AtomicUsize,
        pub pending: AtomicUsize,
        pub retained: Mutex<Vec<WorkerProcess>>,
        terminations: Mutex<Vec<bool>>,
        budgets: Mutex<Vec<Duration>>,
        released: Mutex<Vec<&'static str>>,
        pub io_fail_at: AtomicUsize,
        pub io_attempts: AtomicUsize,
        reader_gate: Mutex<Option<Receiver<()>>>,
        reader_entered: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
        pub reader_started: Mutex<Option<Receiver<()>>>,
        reader_done: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
        borrowing: std::sync::atomic::AtomicBool,
        hello_frame:std::sync::atomic::AtomicBool,
        held_frames:Mutex<Vec<std::sync::mpsc::Sender<Vec<u8>>>>,
    }
    pub(super) struct MockHandle {
        pub effects: Arc<Effects>,
        name: &'static str,
    }
    impl Drop for MockHandle {
        fn drop(&mut self) { self.effects.released.lock().unwrap().push(self.name); }
    }
    impl Effects {
        pub(super) fn new(created: bool, assigned: bool, terminate_ok: bool, waits: &[u32]) -> Arc<Self> {
            Arc::new(Self { created, assigned, terminate_ok, waits: Mutex::new(waits.iter().copied().collect()),
                spawns: AtomicUsize::new(0), pending: AtomicUsize::new(0), retained: Mutex::new(vec![]),
                terminations: Mutex::new(vec![]), budgets: Mutex::new(vec![]), released: Mutex::new(vec![]),
                io_fail_at: AtomicUsize::new(usize::MAX), io_attempts: AtomicUsize::new(0),
                reader_gate: Mutex::new(None), reader_entered: Mutex::new(None), reader_started: Mutex::new(None),
                reader_done: Mutex::new(None), borrowing: std::sync::atomic::AtomicBool::new(false),hello_frame:std::sync::atomic::AtomicBool::new(true),held_frames:Mutex::new(vec![]) })
        }
        pub fn terminate(&self) { self.terminations.lock().unwrap().push(self.terminate_ok); }
        pub fn reader_task(self: &Arc<Self>) -> impl FnOnce() + Send + 'static {
            let effects = self.clone();
            move || {
                effects.borrowing.store(true, Ordering::Release);
                if let Some(entered) = effects.reader_entered.lock().unwrap().take() { let _ = entered.send(()); }
                if let Some(gate) = effects.reader_gate.lock().unwrap().take() { let _ = gate.recv(); }
                effects.borrowing.store(false, Ordering::Release);
                if let Some(done) = effects.reader_done.lock().unwrap().take() { let _ = done.send(()); }
            }
        }
        fn block_reader(&self) -> (std::sync::mpsc::Sender<()>, Receiver<()>) {
            let (release, gate) = std::sync::mpsc::channel();
            let (entered, started) = std::sync::mpsc::sync_channel(1);
            let (done, stopped) = std::sync::mpsc::sync_channel(1);
            *self.reader_gate.lock().unwrap() = Some(gate);
            *self.reader_entered.lock().unwrap() = Some(entered);
            *self.reader_started.lock().unwrap() = Some(started);
            *self.reader_done.lock().unwrap() = Some(done);
            (release, stopped)
        }
        pub fn wait(&self, deadline: Instant) -> u32 {
            self.budgets.lock().unwrap().push(deadline.saturating_duration_since(Instant::now()));
            self.waits.lock().unwrap().pop_front().unwrap_or(258)
        }
        pub fn spawn(self: &Arc<Self>) -> ContractResult<WorkerProcess> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            if !self.created { return Err(ContractError::new(ErrorCode::CleanupFailed, "worker spawn failed")); }
            let handle = |name| WorkerHandle::Mock(MockHandle { effects: self.clone(), name });
            let (frames_tx, frames) = std::sync::mpsc::channel();
            if self.hello_frame.load(Ordering::Acquire){frames_tx.send(crate::clipboard::protocol::HELLO_BODY.as_bytes().to_vec()).unwrap();}else{self.held_frames.lock().unwrap().push(frames_tx);}
            let (_, status) = std::sync::mpsc::channel();
            Ok(WorkerProcess { _job: handle("job"), process: handle("process"), stdin_write: handle("stdin"),
                _stdout_read: handle("stdout"), _stderr_read: handle("stderr"), identity: WorkerIdentity { pid: 7, create_time: 11 },
                reader: None, writer: None, stderr: None, frames, status, reader_status: None, spawn_cleanup: None })
        }
        pub(super) fn supervisor(self: &Arc<Self>) -> WorkerSupervisor {
            WorkerSupervisor::with_config(WorkerConfig { exe: PathBuf::from("must-not-launch.exe"), args: vec![], snapshot: true, mock_spawn: Some(self.clone()) })
        }
        fn assert_released_once(&self) {
            let mut released = self.released.lock().unwrap().clone(); released.sort();
            assert_eq!(released, ["job", "process", "stderr", "stdin", "stdout"]);
        }
    }

    #[test]
    fn m2_cancellation_during_read_wait_rejects_late_frame_and_new_spawn(){
        let effects=Effects::new(true,true,true,&[0]);let mut owner=effects.supervisor();owner.ensure_worker().unwrap();
        let control=Arc::new(crate::runtime::CloseControl::default());owner.set_close_control(control.clone());
        let(frame_tx,frames)=std::sync::mpsc::channel();let(_status_tx,status)=std::sync::mpsc::channel();
        owner.worker.as_mut().unwrap().frames=frames;owner.worker.as_mut().unwrap().status=status;
        let(tx,rx)=std::sync::mpsc::sync_channel(0);owner.test_wait_started=Some(tx);
        let thread=std::thread::spawn(move||{let event=owner.pump_next(Duration::from_secs(30));assert!(matches!(event,Some(PumpEvent::Stopped(Some(ref e))) if e.code==ErrorCode::Busy));owner});
        rx.recv_timeout(Duration::from_secs(1)).unwrap();let deadline=Instant::now()+Duration::from_millis(200);control.publish(deadline);
        let mut owner=thread.join().unwrap();let request=WorkerMessage{version:1,capture_id:"m2-late".into(),policy_epoch:0};
        let late=serde_json::to_vec(&serde_json::json!({"version":1,"capture_id":"m2-late","policy_epoch":0})).unwrap();let _=frame_tx.send(late.clone());
        assert!(matches!(owner.evaluate_frame(&request,&late),FrameOutcome::Failed(e) if e.code==ErrorCode::Busy));
        assert_eq!(owner.capture_snapshot(&request).unwrap_err().code,ErrorCode::Busy);
        assert_eq!(effects.spawns.load(Ordering::SeqCst),1);owner.close_until(deadline).unwrap();effects.assert_released_once();
    }
    #[test]
    fn m2_closing_interrupts_already_running_handshake_without_replacement(){
        let effects=Effects::new(true,true,true,&[0,0]);effects.hello_frame.store(false,Ordering::Release);let mut owner=effects.supervisor();
        let control=Arc::new(crate::runtime::CloseControl::default());owner.set_close_control(control.clone());let(tx,rx)=std::sync::mpsc::sync_channel(0);owner.test_wait_started=Some(tx);
        let waiting=std::thread::spawn(move||{let result=owner.ensure_worker();(owner,result)});rx.recv_timeout(Duration::from_secs(1)).unwrap();control.publish(Instant::now());
        let(mut owner,result)=waiting.join().unwrap();assert_eq!(result.unwrap_err().code,ErrorCode::Busy);assert_eq!(effects.spawns.load(Ordering::SeqCst),1);assert!(owner.ensure_worker().is_err());assert_eq!(effects.spawns.load(Ordering::SeqCst),1);
        assert_eq!(effects.budgets.lock().unwrap()[0],Duration::ZERO);let deadline=Instant::now()+Duration::from_secs(1);control.publish(deadline);owner.close_until(deadline).unwrap();effects.assert_released_once();
    }
    #[test]
    fn m2_shared_deadline_reaches_reaper_retains_identity_and_retry(){
        let effects=Effects::new(true,true,true,&[258,0]);let mut owner=effects.supervisor();owner.ensure_worker().unwrap();
        let identity=owner.current_identity();let control=Arc::new(crate::runtime::CloseControl::default());owner.set_close_control(control.clone());
        let expired=Instant::now();control.publish(expired);assert_eq!(owner.close_until(expired).unwrap_err().code,ErrorCode::Timeout);
        assert_eq!(owner.current_identity(),identity);assert!(!owner.cleanup_failed);assert_eq!(effects.budgets.lock().unwrap()[0],Duration::ZERO);
        let deadline=Instant::now()+Duration::from_secs(1);control.publish(deadline);owner.close_until(deadline).unwrap();assert_eq!(effects.spawns.load(Ordering::SeqCst),1);effects.assert_released_once();
    }
    #[test]
    fn m2_cancelled_write_wait_retains_thread_without_native_write(){
        let control=Arc::new(crate::runtime::CloseControl::default());let c=control.clone();let(tx,gate)=std::sync::mpsc::channel();
        // The synthetic writer waits on a gate, then observes closing before any OS IO.
        let thread=std::thread::spawn(move||win::write_all_cancellable(std::ptr::null_mut(),b"synthetic",Instant::now()+Duration::from_secs(30),Some(c),Some(gate)));
        let bound=Instant::now()+Duration::from_secs(1);while Arc::strong_count(&control)<3{assert!(Instant::now()<bound);std::thread::yield_now();}control.publish(Instant::now());
        let(result,mut pending)=thread.join().unwrap();assert_eq!(result.unwrap_err().code,ErrorCode::Busy);
        assert!(pending.is_some(),"cancellation occurs after a writer thread is owned");
        tx.send(()).ok();if let Some(join)=pending.take(){join.join().unwrap();}
    }
    #[test]
    fn m2_pending_drop_uses_expired_deadline_without_new_budget(){
        let effects=Effects::new(true,true,true,&[258,258,0]);let mut owner=effects.supervisor();owner.ensure_worker().unwrap();
        let expired=Instant::now();assert_eq!(owner.close_until(expired).unwrap_err().code,ErrorCode::Timeout);drop(owner);
        assert_eq!(effects.budgets.lock().unwrap().as_slice(),&[Duration::ZERO,Duration::ZERO]);
        let mut recovery=effects.supervisor();recovery.worker=effects.retained.lock().unwrap().pop();recovery.close();effects.assert_released_once();
    }
    #[test]
    fn m2_worker_io_panic_stays_failed_after_its_join_handle_is_consumed(){
        let effects=Effects::new(true,true,true,&[0,0]);let mut owner=effects.supervisor();owner.ensure_worker().unwrap();
        let reader=std::thread::spawn(||panic!("synthetic worker reader panic"));while !reader.is_finished(){std::thread::yield_now();}owner.worker.as_mut().unwrap().reader=Some(reader);
        assert_eq!(owner.close_until(Instant::now()+Duration::from_secs(1)).unwrap_err().code,ErrorCode::CleanupFailed);
        assert!(owner.cleanup_failed);assert!(owner.worker.as_ref().unwrap().reader.is_none());
        assert_eq!(owner.close_until(Instant::now()+Duration::from_secs(1)).unwrap_err().code,ErrorCode::CleanupFailed);assert!(owner.worker.is_none());effects.assert_released_once();
        assert_eq!(owner.close_until(Instant::now()).unwrap_err().code,ErrorCode::CleanupFailed);assert_eq!(effects.spawns.load(Ordering::SeqCst),1);
    }

    fn unconfirmed(terminate_ok: bool, wait: u32) {
        let effects = Effects::new(true, false, terminate_ok, &[wait, 0]);
        let mut supervisor = effects.supervisor();
        let error = supervisor.ensure_worker().unwrap_err();
        // Finish cleanup even on the unfixed revision, before assertions unwind.
        let state = supervisor.state();
        let owned = supervisor.worker.is_some();
        let report = supervisor.last_reap().cloned();
        let released = effects.released.lock().unwrap().clone();
        supervisor.close();
        assert_eq!(error.message, "job assign failed", "preserve original error");
        assert_eq!(state, WorkerState::FailedPaused, "unconfirmed child cannot become Ready");
        assert!(owned, "supervisor must retain partial child");
        assert!(report.is_some_and(|r| r.cleanup_failed && !r.exited && r.reader_stopped && r.writer_stopped && r.stderr_stopped));
        assert!(released.is_empty(), "last process handle must not close before confirmed exit");
        effects.assert_released_once();
        assert!(effects.budgets.lock().unwrap()[0] <= Duration::from_millis(1000));
    }

    #[test] fn r3_spawn_cleanup_terminate_failed_timeout() { unconfirmed(false, 258); }
    #[test] fn r3_spawn_cleanup_terminate_success_timeout() { unconfirmed(true, 258); }
    #[test] fn r3_spawn_cleanup_wait_failed() { unconfirmed(true, u32::MAX); }
    #[test] fn r3_spawn_cleanup_wait_unknown() { unconfirmed(true, 0x80); }
    #[test]
    fn r3_spawn_cleanup_confirmed_exit_after_terminate_failure() {
        let effects = Effects::new(true, false, false, &[0]);
        let mut supervisor = effects.supervisor();
        assert_eq!(supervisor.ensure_worker().unwrap_err().message, "job assign failed");
        assert_eq!(supervisor.state(), WorkerState::Ready);
        assert!(supervisor.worker.is_none()); effects.assert_released_once();
    }
    #[test]
    fn r3_spawn_cleanup_no_child_is_retryable() {
        let effects = Effects::new(false, false, false, &[]);
        let mut supervisor = effects.supervisor();
        for _ in 0..2 { assert_eq!(supervisor.ensure_worker().unwrap_err().message, "worker spawn failed"); }
        assert_eq!(supervisor.state(), WorkerState::Ready);
        assert_eq!(effects.spawns.load(Ordering::SeqCst), 2);
        assert!(supervisor.worker.is_none()); assert!(effects.released.lock().unwrap().is_empty());
        assert!(effects.terminations.lock().unwrap().is_empty());
    }
    #[test]
    fn r3_spawn_cleanup_successful_assignment_keeps_handshake() {
        let effects = Effects::new(true, true, true, &[0]);
        let mut supervisor = effects.supervisor(); supervisor.ensure_worker().unwrap();
        assert_eq!(supervisor.state(), WorkerState::Ready); assert_eq!(supervisor.spawn_count(), 1);
        supervisor.ensure_worker().unwrap(); assert_eq!(effects.spawns.load(Ordering::SeqCst), 1);
        supervisor.close(); effects.assert_released_once();
    }
    #[test]
    fn r3_spawn_cleanup_capture_error_and_repeated_close_preserve_owner() {
        let effects = Effects::new(true, false, true, &[258, u32::MAX, 258, 0]);
        let mut supervisor = effects.supervisor();
        let request = WorkerMessage { version: 1, capture_id: "r3-capture".into(), policy_epoch: 0 };
        let error = supervisor.capture_snapshot(&request).unwrap_err();
        assert_eq!(error.message, "job assign failed");
        assert_eq!(supervisor.state(), WorkerState::FailedPaused);
        assert_eq!(supervisor.capture_snapshot(&request).unwrap_err().code, ErrorCode::Busy);
        assert_eq!(effects.spawns.load(Ordering::SeqCst), 1, "no replacement on request retry");
        for _ in 0..2 {
            supervisor.close();
            assert!(supervisor.last_reap().unwrap().cleanup_failed, "runtime close/reset must see unresolved cleanup");
            assert!(supervisor.worker.is_some());
            assert_eq!(supervisor.state(), WorkerState::FailedPaused);
            assert!(effects.released.lock().unwrap().is_empty());
        }
        supervisor.close(); supervisor.close();
        assert!(!supervisor.last_reap().unwrap().cleanup_failed);
        assert!(supervisor.last_reap().unwrap().exited);
        assert!(supervisor.worker.is_none()); effects.assert_released_once();
        assert_eq!(effects.pending.load(Ordering::Acquire), 0);
        assert!(effects.budgets.lock().unwrap().iter().skip(1).all(|budget| *budget <= CLEANUP_TIMEOUT));
    }
    #[test]
    fn r3_spawn_cleanup_replacement_supervisor_cannot_bypass_live_owner() {
        let effects = Effects::new(true, false, false, &[258, 0]);
        let mut owner = effects.supervisor(); owner.ensure_worker().unwrap_err();
        let mut replacement = effects.supervisor();
        assert_eq!(replacement.state(), WorkerState::FailedPaused);
        assert_eq!(replacement.ensure_worker().unwrap_err().code, ErrorCode::Busy);
        replacement.close();
        assert!(replacement.last_reap().unwrap().cleanup_failed, "replacement close cannot falsely report clean");
        assert_eq!(effects.spawns.load(Ordering::SeqCst), 1);
        owner.close(); effects.assert_released_once();
        // Retrying close reevaluates the external cleanup gate after confirmed recovery.
        replacement.close(); assert!(!replacement.last_reap().unwrap().cleanup_failed);
        assert_eq!(effects.supervisor().state(), WorkerState::Ready);
    }
    #[test]
    fn r3_spawn_cleanup_drop_retains_owner_and_replacement_gate() {
        let effects = Effects::new(true, false, true, &[258, 258, 0]);
        {
            let mut owner = effects.supervisor(); owner.ensure_worker().unwrap_err();
        }
        let retained = effects.retained.lock().unwrap().len();
        let released = effects.released.lock().unwrap().clone();
        let mut replacement = effects.supervisor();
        let state = replacement.state(); let error = replacement.ensure_worker().unwrap_err();
        replacement.close(); let cleanup_failed = replacement.last_reap().unwrap().cleanup_failed;
        // Test-only drain of its local retained registry, using the real reaper;
        // production retains these resources until host-process exit.
        replacement.worker = effects.retained.lock().unwrap().pop();
        replacement.close(); effects.assert_released_once();
        assert_eq!(retained, 1); assert!(released.is_empty());
        assert_eq!(state, WorkerState::FailedPaused); assert_eq!(error.code, ErrorCode::Busy);
        assert!(cleanup_failed); assert_eq!(effects.spawns.load(Ordering::SeqCst), 1);
        assert_eq!(effects.pending.load(Ordering::Acquire), 0);
    }

    #[test]
    fn r3_spawn_cleanup_second_io_start_failure_retains_borrowed_handles() {
        let effects = Effects::new(true, true, true, &[0, 258, 0]);
        effects.io_fail_at.store(2, Ordering::SeqCst);
        let (release, stopped) = effects.block_reader();
        let mut supervisor = effects.supervisor();
        // Catch only the real shared startup/consumer call on the unfixed revision.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| supervisor.ensure_worker()));
        let released_while_borrowed = effects.released.lock().unwrap().clone();
        let borrowing = effects.borrowing.load(Ordering::Acquire);
        let owned = supervisor.worker.as_ref().is_some_and(|worker| worker.reader.is_some());
        let report = supervisor.last_reap().cloned();
        let state = supervisor.state();
        eprintln!("second IO failure: borrowing={borrowing}, owned={owned}, released={released_while_borrowed:?}");
        // Drain the synthetic borrower even when the failing assertion will unwind.
        release.send(()).unwrap(); stopped.recv_timeout(Duration::from_secs(3)).unwrap();
        supervisor.close();
        let stopped_reader_report = supervisor.last_reap().cloned();
        let retained_after_reader = supervisor.worker.is_some();
        let released_after_reader = effects.released.lock().unwrap().clone();
        supervisor.close();
        assert!(result.is_ok(), "thread creation failure must return its partial owner, not unwind");
        assert_eq!(result.unwrap().unwrap_err().message, "worker IO thread start failed");
        assert!(released_while_borrowed.is_empty(), "no handle may close under the running reader: {released_while_borrowed:?}");
        assert!(borrowing && owned); assert_eq!(state, WorkerState::FailedPaused);
        assert!(report.is_some_and(|r| r.cleanup_failed && r.exited && !r.reader_stopped));
        assert!(stopped_reader_report.is_some_and(|r| r.cleanup_failed && !r.exited && r.reader_stopped));
        assert!(retained_after_reader && released_after_reader.is_empty(), "reader completion alone cannot release process ownership");
        assert!(!supervisor.last_reap().unwrap().cleanup_failed);
        effects.assert_released_once();
    }

    #[test]
    fn r3_spawn_cleanup_first_io_start_failure_retains_process_and_gate() {
        let effects = Effects::new(true, true, true, &[258, 0]);
        effects.io_fail_at.store(1, Ordering::SeqCst);
        let mut supervisor = effects.supervisor();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| supervisor.ensure_worker()));
        let owned = supervisor.worker.is_some();
        let state = supervisor.state();
        let report = supervisor.last_reap().cloned();
        let released = effects.released.lock().unwrap().clone();
        let mut replacement = effects.supervisor();
        let replacement_state = replacement.state();
        // Do not allow an unfixed Ready replacement to execute another injected spawn.
        if replacement_state == WorkerState::FailedPaused {
            assert_eq!(replacement.ensure_worker().unwrap_err().code, ErrorCode::Busy);
            replacement.close(); assert!(replacement.last_reap().unwrap().cleanup_failed);
        }
        supervisor.close();
        assert!(result.is_ok(), "first IO creation failure must not unwind");
        assert_eq!(result.unwrap().unwrap_err().message, "worker IO thread start failed");
        assert!(owned && released.is_empty()); assert_eq!(state, WorkerState::FailedPaused);
        assert_eq!(replacement_state, WorkerState::FailedPaused);
        assert!(report.is_some_and(|r| r.cleanup_failed && !r.exited && r.reader_stopped && r.stderr_stopped));
        assert_eq!(effects.io_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(effects.spawns.load(Ordering::SeqCst), 1);
        assert!(!supervisor.last_reap().unwrap().cleanup_failed);
        effects.assert_released_once(); assert_eq!(effects.pending.load(Ordering::Acquire), 0);
    }

    #[test]
    fn r3_spawn_cleanup_partial_io_drop_retains_reader_then_recovers() {
        let effects = Effects::new(true, true, true, &[258, 258, 258, 0]);
        effects.io_fail_at.store(2, Ordering::SeqCst);
        let (release, stopped) = effects.block_reader();
        let mut owner = effects.supervisor();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.ensure_worker()));
        owner.close();
        let report = owner.last_reap().cloned();
        drop(owner);
        let retained = effects.retained.lock().unwrap().len();
        let released = effects.released.lock().unwrap().clone();
        let borrowing = effects.borrowing.load(Ordering::Acquire);
        let mut replacement = effects.supervisor();
        let state = replacement.state();
        replacement.close(); let cleanup_failed = replacement.last_reap().unwrap().cleanup_failed;
        release.send(()).unwrap(); stopped.recv_timeout(Duration::from_secs(3)).unwrap();
        // Test-local recovery only; production retention remains process lifetime.
        replacement.worker = effects.retained.lock().unwrap().pop();
        replacement.close();
        assert!(result.is_ok(), "partial startup must reach the reaper and Drop owner");
        assert!(report.is_some_and(|r| r.cleanup_failed && !r.exited && !r.reader_stopped));
        assert_eq!(retained, 1); assert!(borrowing && released.is_empty());
        assert_eq!(state, WorkerState::FailedPaused); assert!(cleanup_failed);
        assert!(!replacement.last_reap().unwrap().cleanup_failed);
        effects.assert_released_once(); assert_eq!(effects.pending.load(Ordering::Acquire), 0);
        let budgets = effects.budgets.lock().unwrap();
        assert!(budgets[0] <= Duration::from_millis(1000));
        assert!(budgets.iter().skip(1).all(|budget| *budget <= CLEANUP_TIMEOUT));
        assert_eq!(effects.supervisor().state(), WorkerState::Ready);
    }

    #[test]
    fn r3_spawn_cleanup_normal_io_start_keeps_handshake_and_joins() {
        let effects = Effects::new(true, true, true, &[0]);
        effects.io_fail_at.store(0, Ordering::SeqCst);
        let mut supervisor = effects.supervisor(); supervisor.ensure_worker().unwrap();
        assert_eq!(supervisor.state(), WorkerState::Ready);
        assert_eq!(supervisor.spawn_count(), 1);
        assert_eq!(effects.io_attempts.load(Ordering::SeqCst), 2);
        assert!(supervisor.worker.as_ref().is_some_and(|w| w.reader.is_some() && w.stderr.is_some()));
        supervisor.close();
        assert!(supervisor.last_reap().is_some_and(|r| !r.cleanup_failed && r.exited && r.reader_stopped && r.stderr_stopped));
        effects.assert_released_once(); assert_eq!(effects.pending.load(Ordering::Acquire), 0);
    }
}

mod win {
    use super::{SpawnCleanupGuard, SpawnFailure, WorkerConfig, WorkerHandle, WorkerIdentity, WorkerProcess};
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

    pub fn terminate_owned(process: &WorkerHandle) {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let WorkerHandle::Mock(handle) = process {
            handle.effects.terminate();
            return;
        }
        terminate(process.as_raw_handle());
    }

    pub fn wait_owned(process: &WorkerHandle, deadline: Instant) -> bool {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let WorkerHandle::Mock(handle) = process {
            return handle.effects.wait(deadline) == WAIT_OBJECT_0;
        }
        wait_exit(process.as_raw_handle(), deadline)
    }
    pub fn wait_owned_controlled(process:&WorkerHandle,deadline:Instant,closing:Option<&std::sync::Arc<crate::runtime::CloseControl>>)->bool{
        if closing.is_none(){return wait_owned(process,deadline);}
        #[cfg(all(test,feature="test-fixtures"))]
        if let WorkerHandle::Mock(_)=process{return wait_owned(process,closing.map_or(deadline,|c|c.deadline(deadline)));}
        loop {
            let deadline=closing.map_or(deadline,|c|c.deadline(deadline));let budget=deadline.saturating_duration_since(Instant::now()).as_millis().min(5) as u32;
            let result=unsafe{WaitForSingleObject(process.as_raw_handle(),budget)};if result==WAIT_OBJECT_0{return true;}
            if result!=258||Instant::now()>=deadline{return false;}
        }
    }

    fn assign_worker(mut worker: WorkerProcess) -> Result<WorkerProcess, SpawnFailure> {
        #[cfg(all(test, feature = "test-fixtures"))]
        let assigned = if let WorkerHandle::Mock(handle) = &worker.process {
            handle.effects.assigned
        } else {
            unsafe { AssignProcessToJobObject(worker._job.as_raw_handle(), worker.process.as_raw_handle()) != 0 }
        };
        #[cfg(not(all(test, feature = "test-fixtures")))]
        let assigned = unsafe { AssignProcessToJobObject(worker._job.as_raw_handle(), worker.process.as_raw_handle()) != 0 };
        if !assigned {
            worker.spawn_cleanup = Some(SpawnCleanupGuard::new(&worker.process));
            return Err(SpawnFailure { error: last_error("job assign failed"), worker: Some(worker) });
        }
        Ok(worker)
    }

    fn spawn_io_thread(process: &WorkerHandle, task: impl FnOnce() + Send + 'static) -> std::io::Result<JoinHandle<()>> {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let WorkerHandle::Mock(handle) = process {
            let attempt = handle.effects.io_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if attempt == 2 {
                if let Some(started) = handle.effects.reader_started.lock().unwrap().take() {
                    started.recv_timeout(Duration::from_secs(3)).expect("synthetic reader must start before second IO creation");
                }
            }
            if attempt == handle.effects.io_fail_at.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(std::io::Error::other("injected IO thread creation failure"));
            }
        }
        #[cfg(not(all(test, feature = "test-fixtures")))]
        let _ = process;
        thread::Builder::new().spawn(task)
    }

    fn start_io_threads(
        mut worker: WorkerProcess,
        reader: impl FnOnce() + Send + 'static,
        stderr: impl FnOnce() + Send + 'static,
    ) -> Result<WorkerProcess, SpawnFailure> {
        if let Ok(reader) = spawn_io_thread(&worker.process, reader) {
            worker.reader = Some(reader);
            if let Ok(stderr) = spawn_io_thread(&worker.process, stderr) {
                worker.stderr = Some(stderr);
                return Ok(worker);
            }
        }
        // A started reader still borrows stdout even when the next spawn fails.
        // Publish the partial owner through the same bounded cleanup as assign failure.
        worker.spawn_cleanup = Some(SpawnCleanupGuard::new(&worker.process));
        Err(SpawnFailure { error: last_error("worker IO thread start failed"), worker: Some(worker) })
    }

    /// 仅在确认线程完成后 join；取消是请求，不能当作退出确认。
    pub fn finish_thread(thread: &mut Option<JoinHandle<()>>, deadline: Instant) -> bool {
        finish_thread_controlled(thread,deadline,None)
    }
    pub fn finish_thread_controlled(thread:&mut Option<JoinHandle<()>>,deadline:Instant,closing:Option<&std::sync::Arc<crate::runtime::CloseControl>>)->bool {
        let Some(join) = thread.as_ref() else { return true };
        // Rust is_finished 不保证 OS 线程析构已结束；等待原生线程句柄 signaled 才 join。
        while unsafe { WaitForSingleObject(join.as_raw_handle(), 0) } != WAIT_OBJECT_0 {
            // 同步匿名管道 IO 使用线程级取消；反复请求以覆盖线程启动竞态。
            unsafe { CancelSynchronousIo(join.as_raw_handle()) };
            let now = Instant::now();
            let deadline=closing.map_or(deadline,|c|c.deadline(deadline));
            if now >= deadline {
                return false;
            }
            let millis = (deadline - now).as_millis().clamp(1, 5) as u32;
            unsafe { WaitForSingleObject(join.as_raw_handle(), millis) };
        }
        thread.take().expect("completed thread retained").join().is_ok()
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

    fn write_all(handle: HANDLE, mut data: &[u8], deadline: Instant,closing:Option<&std::sync::Arc<crate::runtime::CloseControl>>) -> ContractResult<()> {
        while !data.is_empty() {
            if closing.is_some_and(|c|c.closing()){return Err(ContractError::new(ErrorCode::Busy,"capture closing"));}
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
        closing:Option<std::sync::Arc<crate::runtime::CloseControl>>,
        #[cfg(feature = "test-fixtures")] gate: Option<mpsc::Receiver<()>>,
    ) -> (ContractResult<()>, Option<JoinHandle<()>>) {
        if closing.as_ref().is_some_and(|c|c.closing()) {
            return (Err(ContractError::new(ErrorCode::Busy,"capture closing")),None);
        }
        if Instant::now() >= deadline {
            return (Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded")), None);
        }
        let data = data.to_vec();
        let handle_usize = handle as usize;
        let (tx, rx) = mpsc::channel();
        let writer_closing=closing.clone();
        let join = thread::spawn(move || {
            #[cfg(feature = "test-fixtures")]
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            let result = write_all(handle_usize as HANDLE, &data, deadline,writer_closing.as_ref());
            let _ = tx.send(result);
        });
        let mut pending = Some(join);
        loop {
        if closing.as_ref().is_some_and(|c|c.closing()) {
            if let Some(thread)=pending.as_ref(){unsafe{CancelSynchronousIo(thread.as_raw_handle());}}
            return (Err(ContractError::new(ErrorCode::Busy,"capture closing")),pending);
        }
        let budget=deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(budget.min(Duration::from_millis(10))) {
            Ok(result) => {
                if finish_thread(&mut pending, closing.as_ref().map_or(deadline,|c|c.deadline(deadline))) {
                    return (result, None);
                } else {
                    return (Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded")), pending);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now()<deadline=>continue,
            Err(_) => {
                return (Err(ContractError::new(ErrorCode::Timeout, "write deadline exceeded")), pending);
            }
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

    pub(super) fn spawn_worker(config: &WorkerConfig) -> Result<WorkerProcess, SpawnFailure> {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let Some(effects) = &config.mock_spawn {
            let worker = effects.spawn().map_err(SpawnFailure::from).and_then(assign_worker)?;
            if effects.io_fail_at.load(std::sync::atomic::Ordering::SeqCst) != usize::MAX {
                return start_io_threads(worker, effects.reader_task(), || {});
            }
            return Ok(worker);
        }
        unsafe { spawn_worker_inner(config) }
    }

    unsafe fn spawn_worker_inner(config: &WorkerConfig) -> Result<WorkerProcess, SpawnFailure> {
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
            return Err(last_error("worker spawn failed").into());
        }
        // 仅子进程端点可继承。
        for child_end in [stdin_read, stdout_write, stderr_write] {
            if SetHandleInformation(child_end, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
                return Err(last_error("worker spawn failed").into());
            }
        }

        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            close_all(&[stdin_read, stdin_write, stdout_read, stdout_write, stderr_read, stderr_write]);
            return Err(last_error("worker spawn failed").into());
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
            return Err(last_error("worker spawn failed").into());
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
            return Err(last_error("worker spawn failed").into());
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
            return Err(last_error("worker spawn failed").into());
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
            return Err(last_error("worker spawn failed").into());
        }
        close_handle(pi.hThread);

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
        let worker = assign_worker(WorkerProcess {
            _job: WorkerHandle::Native(OwnedHandle::from_raw_handle(job)),
            process: WorkerHandle::Native(OwnedHandle::from_raw_handle(pi.hProcess)),
            stdin_write: WorkerHandle::Native(OwnedHandle::from_raw_handle(stdin_write)),
            _stdout_read: WorkerHandle::Native(OwnedHandle::from_raw_handle(stdout_read)),
            _stderr_read: WorkerHandle::Native(OwnedHandle::from_raw_handle(stderr_read)),
            identity,
            reader: None,
            writer: None,
            stderr: None,
            frames: frame_rx,
            status: status_rx,
            reader_status: None,
            spawn_cleanup: None,
        })?;
        // Both native and synthetic startup use the same partial-owner path.
        let reader_handle = stdout_read as usize;
        let snapshot = config.snapshot;
        let stderr_handle = stderr_read as usize;
        start_io_threads(worker, move || {
            reader_loop(reader_handle as HANDLE, frame_tx, status_tx, snapshot);
        }, move || drain_loop(stderr_handle as HANDLE))
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
