//! E06 runtime ownership: the capture thread owns native capture resources;
//! the storage thread creates and owns its SQLite connection.
use std::{path::{Path,PathBuf},sync::{Arc,Mutex,Condvar,OnceLock,mpsc::{self,SyncSender,Receiver}},thread::JoinHandle,time::{Duration,Instant}};
use crate::{contracts::*,runtime::{CaptureState,CapturePipeline,Admission,Notice},store::Store};
use crate::clipboard::origin::{WriteApi,WriteFailure,WriteFormat};
use crate::startup_capture::{Preference, StartupCapture, StartupCaptureSnapshot, CHANGED_EVENT};
use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};

struct WritePermit(Arc<AtomicBool>);
impl Drop for WritePermit {fn drop(&mut self){self.0.store(false,Ordering::Release);}}
type WriteReply=Result<u64,WriteFailure>;
struct CopyOwner {owner:Option<usize>,borrowers:usize}
struct CopyOwnerLease<'a> {shared:&'a SharedRuntime,owner:usize}
impl Drop for CopyOwnerLease<'_> {
    fn drop(&mut self){let mut owner=self.shared.copy_owner.lock().unwrap_or_else(|e|e.into_inner());owner.borrowers-=1;self.shared.owner_idle.notify_all();}
}

pub type Events=Arc<dyn Fn(&str,u64)+Send+Sync>;
type FavoriteEvents = Arc<dyn Fn(&FavoriteChange) + Send + Sync>;
pub struct SharedRuntime {
    pub state:Arc<Mutex<CaptureState>>,
    pub gate:Mutex<()>,
    wake:Condvar,
    events:Events,
    favorite_events: OnceLock<FavoriteEvents>,
    _instance:Option<std::os::windows::io::OwnedHandle>,
    listener_failed:std::sync::atomic::AtomicBool,
    // A completed failed cleanup cannot be cleared by consuming its thread handle.
    cleanup_failed:std::sync::atomic::AtomicBool,
    native_writes:bool,
    copy_busy:Arc<AtomicBool>,
    copy_stopping:AtomicBool,
    // Live resource authority; OriginRegistry also retains historical attribution.
    copy_owner:Mutex<CopyOwner>,
    owner_idle:Condvar,
    closing:Arc<crate::runtime::CloseControl>,
    cleanup_errors:Mutex<Vec<String>>,
    settings_file:Mutex<Option<crate::config::SettingsFile>>,
    startup_capture: Mutex<StartupCapture>,
    // Serializes writes and records the latest manual choice already resolved by a writer.
    startup_capture_writer: Mutex<u64>,
    manual_capture_sequence: AtomicU64,
    #[cfg(all(test,feature="test-fixtures"))]
    test_cleanup_attempt:Mutex<Option<SyncSender<(u64,usize,bool)>>>,
    #[cfg(feature="test-fixtures")]
    test_writer:Mutex<Option<Arc<dyn Fn()->ContractResult<Box<dyn WriteApi+Send>>+Send+Sync>>>,
    #[cfg(feature="test-fixtures")]
    pub before_commit:Mutex<Option<Arc<dyn Fn()+Send+Sync>>>,
    #[cfg(feature="test-fixtures")]
    pub before_policy:Mutex<Option<Arc<dyn Fn()+Send+Sync>>>,
    #[cfg(feature="test-fixtures")]
    pub before_settings_replace:Mutex<Option<Arc<dyn Fn()+Send+Sync>>>,
    #[cfg(all(test,feature="test-fixtures"))]
    before_startup_capture_write: Mutex<Option<Arc<dyn Fn()+Send+Sync>>>,
}
enum Target {File(PathBuf),#[cfg(feature="test-fixtures")] Memory,#[cfg(feature="test-fixtures")] Manual{path:PathBuf,product:PathBuf}}
#[derive(Debug,serde::Deserialize)]
#[serde(tag="action",rename_all="snake_case",deny_unknown_fields)]
pub enum HistoryManagement {
    Delete { clip_id:String, expected_items:u64 },
    Clear { expected_items:u64, expected_history_revision:String },
    Backup {},
}
#[derive(Debug,serde::Serialize)]
pub struct HistoryManagementResult { pub items:u64, pub favorites:u64, pub deleted:u64, pub backup_path:Option<String> }

enum Query {
    VerifyStartupCapture(StartupCapture, SyncSender<(StartupCapture, bool)>),
    Manage(HistoryManagement,PathBuf,Instant,SyncSender<ContractResult<HistoryManagementResult>>),
    SetFavorite(SetFavoriteRequest, Instant, SyncSender<ContractResult<SetFavoriteResult>>),
    List(ListClipsRequest,SyncSender<ContractResult<ListClipsResult>>),
    Copy(i64,WriteFormat,Instant,WritePermit,SyncSender<WriteReply>),
    Text(GetClipTextRequest,SyncSender<ContractResult<GetClipTextResult>>),
    Png(i64, Instant, SyncSender<ContractResult<Vec<u8>>>),
    #[cfg(feature="test-fixtures")]
    Fault(bool,SyncSender<ContractResult<()>>),
}
pub struct HostRuntime {
    pub shared:Arc<SharedRuntime>,
    queries:SyncSender<Query>,
    storage:Mutex<Option<JoinHandle<()>>>,
    capture:Mutex<Option<JoinHandle<()>>>,
    startup_writer: Mutex<Option<JoinHandle<()>>>,
    data_dir:String,
    production:bool,
    pub exit_pending:std::sync::atomic::AtomicBool,
    pub exit_ready:std::sync::atomic::AtomicBool,
}
struct StartupCaptureSave<'a>(&'a SharedRuntime);
enum CaptureChange { Manual(u64), Startup(Instant) }
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

impl Drop for StartupCaptureSave<'_> {
    fn drop(&mut self) {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner()).startup_capture_writes.saving -= 1;
        self.0.wake.notify_all();
    }
}
fn unavailable()->ContractError{ContractError::new(ErrorCode::StorageUnavailable,"history storage unavailable")}
fn cleanup()->ContractError{ContractError::new(ErrorCode::CleanupFailed,"runtime cleanup incomplete")}
fn capture_active(state: &CaptureState) -> bool {
    state.user_enabled && state.admission == Admission::Active && !state.storage_failed && !state.stopping
}
static RETAINED:Mutex<Vec<JoinHandle<()>>>=Mutex::new(Vec::new());

fn single_instance()->ContractResult<std::os::windows::io::OwnedHandle>{
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{Foundation::{GetLastError,ERROR_ALREADY_EXISTS,CloseHandle},System::Threading::CreateMutexW};
    let name:Vec<u16>="Local\\Pinfeather.FirstIteration".encode_utf16().chain([0]).collect();
    let handle=unsafe{CreateMutexW(std::ptr::null(),0,name.as_ptr())};
    if handle.is_null(){return Err(cleanup());}
    if unsafe{GetLastError()}==ERROR_ALREADY_EXISTS{unsafe{CloseHandle(handle);}return Err(ContractError::new(ErrorCode::Busy,"another Pinfeather instance is running"));}
    Ok(unsafe{std::os::windows::io::OwnedHandle::from_raw_handle(handle)})
}

impl SharedRuntime {
    fn publish_copy_owner(&self,owner:usize){
        let mut lifetime=self.copy_owner.lock().unwrap_or_else(|e|e.into_inner());
        assert!(owner!=0&&lifetime.owner.is_none()&&lifetime.borrowers==0);
        if !self.copy_stopping.load(Ordering::Acquire){lifetime.owner=Some(owner);}
    }
    fn borrow_copy_owner(&self)->ContractResult<CopyOwnerLease<'_>>{
        let mut lifetime=self.copy_owner.lock().unwrap_or_else(|e|e.into_inner());
        let owner=lifetime.owner.filter(|_|!self.copy_stopping.load(Ordering::Acquire)).ok_or_else(||ContractError::new(ErrorCode::InjectionFailed,"clipboard owner unavailable"))?;
        lifetime.borrowers+=1;Ok(CopyOwnerLease{shared:self,owner})
    }
    fn revoke_copy_owner(&self){
        self.copy_stopping.store(true,Ordering::Release);
        self.copy_owner.lock().unwrap_or_else(|e|e.into_inner()).owner=None;
    }
    #[cfg(all(test,feature="test-fixtures"))]
    fn close_capture(&self,close:impl FnOnce()->ContractResult<()>){
        self.revoke_copy_owner();
        let mut owner=self.copy_owner.lock().unwrap_or_else(|e|e.into_inner());
        // The listener has its own pump thread; waiting here must not stop it.
        while owner.borrowers!=0{owner=self.owner_idle.wait(owner).unwrap_or_else(|e|e.into_inner());}
        drop(owner);self.capture_closed(close());
    }
    fn await_copy_borrowers(&self,deadline:Instant)->bool{
        let mut owner=self.copy_owner.lock().unwrap_or_else(|e|e.into_inner());
        while owner.borrowers!=0{
            let budget=deadline.saturating_duration_since(Instant::now());if budget.is_zero(){return false;}
            owner=self.owner_idle.wait_timeout(owner,budget).unwrap_or_else(|e|e.into_inner()).0;
        }
        true
    }
    fn close_pipeline(&self,pipeline:&mut CapturePipeline){
        self.revoke_copy_owner();let mut generation=0;
        loop {
            let(next,deadline)=self.closing.next(generation);generation=next;
            let worker=pipeline.cancel_worker_until(deadline);
            self.record_cleanup(&worker);
            if !self.await_copy_borrowers(deadline){#[cfg(all(test,feature="test-fixtures"))]self.m2_report_attempt(generation,pipeline);continue;}
            let result=pipeline.close_until(deadline);
            self.record_cleanup(&result);
            #[cfg(all(test,feature="test-fixtures"))]self.m2_report_attempt(generation,pipeline);
            if !pipeline.cleanup_pending(){self.capture_closed(result);break;}
        }
    }
    #[cfg(all(test,feature="test-fixtures"))]
    fn m2_report_attempt(&self,generation:u64,pipeline:&CapturePipeline){if let Some(tx)=self.test_cleanup_attempt.lock().unwrap().as_ref(){let _=tx.try_send((generation,pipeline as *const _ as usize,pipeline.m2_listener_pending()));}}
    fn record_cleanup(&self,result:&ContractResult<()>){if let Err(error)=result{if error.code==ErrorCode::CleanupFailed{self.cleanup_failed.store(true,Ordering::Release);let mut errors=self.cleanup_errors.lock().unwrap_or_else(|e|e.into_inner());if !errors.contains(&error.message){errors.push(error.message.clone());}}}}
    fn capture_closed(&self,result:ContractResult<()>){
        if result.is_err(){self.cleanup_failed.store(true,std::sync::atomic::Ordering::Release);}
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.listener_ready=false;s.worker_service=if result.is_ok(){"stopped"}else{"failed"}.into();drop(s);self.notify_status();
    }
    fn notify_status(&self){let revision={let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.revision+=1;s.revision};(self.events)("status_changed",revision);(self.events)("settings_changed",revision);self.wake.notify_all();}

    fn admit_manual_capture_save(&self, state: &mut CaptureState, enabled: bool) -> StartupCaptureSave<'_> {
        let writes = &mut state.startup_capture_writes;
        writes.record_manual_choice(enabled);
        writes.saving += 1;
        StartupCaptureSave(self)
    }

    fn queue_recovered_capture_save(&self, state: &mut CaptureState) {
        if state.startup_capture_writes.pending_start && capture_active(state) && !self.closing.closing() {
            let writes = &mut state.startup_capture_writes;
            writes.record_manual_choice(true);
            if !writes.recovery_queued {
                // Count the queued write before publishing the work to the writer.
                writes.recovery_queued = true;
                writes.saving += 1;
                self.wake.notify_all();
            }
        }
    }

    fn write_recovered_capture(&self) {
        loop {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let mut state = self.wake.wait_while(state, |s| {
                !s.startup_capture_writes.recovery_queued && !s.stopping && !self.closing.closing()
            }).unwrap_or_else(|e| e.into_inner());
            if !state.startup_capture_writes.recovery_queued { return; }
            state.startup_capture_writes.recovery_queued = false;
            drop(state);
            self.persist_manual_capture(StartupCaptureSave(self));
        }
    }

    fn update_startup_capture<T>(&self, update: impl FnOnce(&mut StartupCapture, &mut u64) -> T) -> T {
        let mut resolved = self.startup_capture_writer.lock().unwrap_or_else(|e| e.into_inner());
        let mut startup = self.startup_capture.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let previous = startup.revision();
        let result = update(&mut startup, &mut resolved);
        let revision = startup.revision();
        *self.startup_capture.lock().unwrap_or_else(|e| e.into_inner()) = startup;
        drop(resolved);
        if revision != previous { (self.events)(CHANGED_EVENT, revision); }
        result
    }

    fn persist_startup_capture(&self, startup: &mut StartupCapture, preference: Preference) -> ContractResult<()> {
        #[cfg(all(test,feature="test-fixtures"))]
        {
            let hook = self.before_startup_capture_write.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(hook) = hook { hook(); }
        }
        startup.save(preference)
    }

    fn persist_manual_capture(&self, _save: StartupCaptureSave<'_>) {
        self.update_startup_capture(|startup, resolved| {
            let Some((generation, enabled)) = self.state.lock().unwrap_or_else(|e| e.into_inner()).startup_capture_writes.last_manual else {
                return;
            };
            if generation <= *resolved { return; }
            if startup.remembers_manual_state() {
                let _ = self.persist_startup_capture(startup, Preference::manual(enabled));
            }
            if enabled { startup.clear_restore_block(); }
            *resolved = generation;
        });
    }

    fn worker_restart_finished(&self, worker_service: &str, result: ContractResult<()>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.worker_service = worker_service.into();
        if result.is_ok() && !state.storage_failed && !state.stopping && state.user_enabled && !self.closing.closing() {
            state.reopen_after_recovery();
            self.queue_recovered_capture_save(&mut state);
        }
        drop(state);
        self.notify_status();
    }

    fn recover_capture_after_storage(&self, state: &mut CaptureState) {
        if state.stopping || !state.user_enabled || self.closing.closing() { return; }
        if self.native_writes && (!state.listener_ready || self.listener_failed.load(Ordering::Acquire)) { return; }
        if self.native_writes && state.worker_service == "failed" {
            if state.startup_capture_writes.pending_start { state.restart_worker = true; }
            return;
        }
        state.reopen_after_recovery();
    }
    fn commit_one(&self,store:&mut Store)->bool{
        let gate=self.gate.lock().unwrap_or_else(|e|e.into_inner());
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        if s.storage_failed&&!s.storage_retry{return false;}
        if s.storage_retry {
            s.storage_retry=false;
            match store.usage(){Ok(usage)=>{s.history=usage;s.storage_failed=false;},Err(_)=>return false,}
            if s.queue.is_empty() {
                self.recover_capture_after_storage(&mut s);
                self.queue_recovered_capture_save(&mut s);
                drop(s);drop(gate);
                self.notify_status();
                return true;
            }
        }
        let Some(front)=s.queue.front() else{return false;};
        if front.ticket.message.policy_epoch!=s.epoch||!s.user_enabled {
            let item=s.pop_queued().unwrap();s.counters.cancelled+=1;drop(item);drop(s);drop(gate);self.notify_status();return true;
        }
        let origin=front.snapshot.internal_origin.clone();let owner=front.snapshot.response.owner.hwnd.as_ref().and_then(|id|id.parse::<usize>().ok());let sequence=front.snapshot.response.sequence_after;
        match s.origins.classify(origin.as_deref(),owner,sequence) {
            crate::clipboard::origin::Classification::Pending=>return false,
            crate::clipboard::origin::Classification::Internal=>{s.pop_queued();s.counters.ignored+=1;s.internal_ignored+=1;drop(s);drop(gate);self.notify_status();return true;}
            _=>{}
        }
        let source=s.queue.front().unwrap().snapshot.response.owner.process.clone();
        if !s.owner_allowed(source.as_deref()){s.pop_queued();s.counters.ignored+=1;drop(s);drop(gate);self.notify_status();return true;}
        let mut item=s.pop_queued().unwrap();
        let bytes=item.snapshot.payloads.iter().map(|p|p.data.len() as u64).sum::<u64>();
        s.storage_inflight=(1,bytes);drop(s);
        #[cfg(feature="test-fixtures")]if let Some(hook)=self.before_commit.lock().unwrap_or_else(|e|e.into_inner()).clone(){hook();}
        let source=item.snapshot.response.owner.process.clone();
        let mut snapshot=crate::store::Snapshot{capture_id:item.ticket.message.capture_id.clone(),captured_at_ms:item.captured_at_ms,source_confidence:if source.is_some(){crate::store::SourceConfidence::Owner}else{crate::store::SourceConfidence::Unknown},source_process:source,omitted_formats:std::mem::take(&mut item.snapshot.response.omitted_formats),payloads:std::mem::take(&mut item.snapshot.payloads)};
        let result=store.insert(&snapshot);
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.storage_inflight=(0,0);
        let history=match result {
            Ok(ack)=>{
                s.history=ack.usage;s.storage_failed=false;
                if s.admission == Admission::Failed { self.recover_capture_after_storage(&mut s); }
                if ack.inserted{s.counters.persisted+=1;s.history_revision+=1;Some(s.history_revision)}else{None}
            }
            Err(_)=>{
                item.snapshot.payloads=std::mem::take(&mut snapshot.payloads);item.snapshot.response.omitted_formats=std::mem::take(&mut snapshot.omitted_formats);
                s.bytes+=bytes;s.queue.push_front(item);s.storage_failed=true;s.admission=Admission::Failed;s.counters.failed+=1;
                if let Ok(usage)=store.usage(){s.history=usage;}
                None
            }
        };
        self.queue_recovered_capture_save(&mut s);
        drop(s);drop(gate);
        if let Some(revision)=history{(self.events)("history_changed",revision);}
        self.notify_status();
        true
    }
}

impl HostRuntime {
    pub fn start(data_dir:PathBuf,events:Events,start_paused:bool)->ContractResult<Self>{
        let instance=single_instance()?;
        Self::create(Target::File(data_dir.join("history.sqlite3")),data_dir.display().to_string(),events,true,Some(instance),start_paused)
    }
    #[cfg(feature="test-fixtures")]
    pub fn for_manual_product(data_dir:PathBuf,product:PathBuf,events:Events)->ContractResult<Self>{Self::create(Target::Manual{path:data_dir.join("history.sqlite3"),product},data_dir.display().to_string(),events,true,Some(single_instance()?),true)}
    #[cfg(feature="test-fixtures")]
    pub fn for_test_memory(events:Events)->ContractResult<Self>{Self::create(Target::Memory,"D:\\Synthetic\\Pinfeather".into(),events,false,None,false)}
    #[cfg(feature="test-fixtures")]
    pub fn for_test_file(path:PathBuf,events:Events)->ContractResult<Self>{Self::create(Target::File(path.clone()),path.parent().unwrap().display().to_string(),events,false,None,false)}

    fn create(target:Target,data_dir:String,events:Events,production:bool,instance:Option<std::os::windows::io::OwnedHandle>,start_paused:bool)->ContractResult<Self>{
        Self::create_with_startup_io(target, data_dir, events, production, instance, start_paused,
            Store::usage, crate::config::read_settings)
    }

    fn create_with_startup_io(
        target: Target, data_dir: String, events: Events, production: bool,
        instance: Option<std::os::windows::io::OwnedHandle>, start_paused: bool,
        storage_usage: impl FnOnce(&Store) -> ContractResult<HistoryUsage> + Send + 'static,
        read_preference: impl Fn(&Path) -> std::io::Result<Option<Vec<u8>>> + Send + 'static,
    ) -> ContractResult<Self> {
        if !RETAINED.lock().unwrap_or_else(|e|e.into_inner()).is_empty(){return Err(cleanup());}
        let state=Arc::new(Mutex::new(CaptureState::new(&uuid::Uuid::new_v4().simple().to_string())));
        let shared=Arc::new(SharedRuntime{state,gate:Mutex::new(()),wake:Condvar::new(),events,favorite_events:OnceLock::new(),_instance:instance,listener_failed:std::sync::atomic::AtomicBool::new(false),cleanup_failed:std::sync::atomic::AtomicBool::new(false),native_writes:production,
            copy_busy:Arc::new(AtomicBool::new(false)),copy_stopping:AtomicBool::new(false),
            copy_owner:Mutex::new(CopyOwner{owner:None,borrowers:0}),owner_idle:Condvar::new(),
            closing:Arc::new(crate::runtime::CloseControl::default()),
            cleanup_errors:Mutex::new(Vec::new()),
            settings_file:Mutex::new(None),
            startup_capture:Mutex::new(StartupCapture::without_file(start_paused)),
            startup_capture_writer:Mutex::new(0),
            manual_capture_sequence:AtomicU64::new(0),
            #[cfg(all(test,feature="test-fixtures"))]test_cleanup_attempt:Mutex::new(None),
            #[cfg(feature="test-fixtures")]test_writer:Mutex::new(None),
            #[cfg(feature="test-fixtures")]before_commit:Mutex::new(None),
            #[cfg(feature="test-fixtures")]before_policy:Mutex::new(None),
            #[cfg(feature="test-fixtures")]before_settings_replace:Mutex::new(None),
            #[cfg(all(test,feature="test-fixtures"))]before_startup_capture_write:Mutex::new(None),
        });
        let writer_shared = shared.clone();
        let startup_writer = std::thread::Builder::new().name("startup-capture-writer".into())
            .spawn(move || writer_shared.write_recovered_capture()).map_err(|_| cleanup())?;
        #[cfg(feature="test-fixtures")]let test_product=match &target{Target::Manual{product,..}=>Some(product.clone()),_=>None};
        let(tx,rx)=mpsc::sync_channel(16);let(ready_tx,ready_rx)=mpsc::sync_channel(1);let storage_shared=shared.clone();
        let startup_deadline = Instant::now() + STARTUP_TIMEOUT;
        let storage=std::thread::Builder::new().name("history-storage".into()).spawn(move||{
            let settings_path=match &target{Target::File(path)=>Some(path.with_file_name("settings.json")),#[cfg(feature="test-fixtures")]Target::Manual{path,..}=>Some(path.with_file_name("settings.json")),#[cfg(feature="test-fixtures")]Target::Memory=>None};
            let preference_dir = settings_path.as_deref().and_then(Path::parent);
            let startup = StartupCapture::load(preference_dir, start_paused, &read_preference);
            if let Some(path)=settings_path {
                let(file,loaded)=crate::config::SettingsFile::load(path);
                let mut state=storage_shared.state.lock().unwrap_or_else(|e|e.into_inner());
                if !state.storage_failed {
                    match loaded{Ok(paths)=>state.exclusions=paths,Err(error)=>{state.settings_blocked=true;state.settings_error=Some(error.message);}}
                }
                *storage_shared.settings_file.lock().unwrap_or_else(|e|e.into_inner())=Some(file);
            }
            let opened=match target{Target::File(path)=>Store::open(&path),#[cfg(feature="test-fixtures")]Target::Memory=>Store::open_in_memory(),#[cfg(feature="test-fixtures")]Target::Manual{path,..}=>Store::open(&path)};
            let mut store=opened.ok();
            let usage = store.as_ref().map(storage_usage);
            {
                let mut state = storage_shared.state.lock().unwrap_or_else(|e| e.into_inner());
                // A startup timeout keeps capture blocked for this launch.
                if !state.storage_failed {
                    if let Some(Ok(usage)) = usage {
                        state.history = usage;
                        state.storage_ready = true;
                    } else {
                        state.storage_failed = true;
                        state.admission = Admission::Failed;
                    }
                }
            }
            let _=ready_tx.send(startup);
            storage_loop(&storage_shared,&mut store,rx,read_preference);
            drop(store);
            storage_shared.notify_status();
        });
        let runtime=Self{shared,queries:tx,storage:Mutex::new(storage.ok()),capture:Mutex::new(None),startup_writer:Mutex::new(Some(startup_writer)),data_dir,production,exit_pending:std::sync::atomic::AtomicBool::new(false),exit_ready:std::sync::atomic::AtomicBool::new(false)};
        let Ok(startup) = ready_rx.recv_timeout(startup_deadline.saturating_duration_since(Instant::now())) else {
            if !production { return Err(cleanup()); }
            let mut state = runtime.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.settings_blocked = true;
            state.settings_error = Some("启动时未能确认排除设置。收录已阻止，请重新启动。".into());
            state.storage_ready = false;
            state.storage_failed = true;
            state.admission = Admission::Failed;
            drop(state);
            runtime.shared.startup_capture.lock().unwrap_or_else(|e| e.into_inner()).read_timed_out();
            return Ok(runtime);
        };
        *runtime.shared.startup_capture.lock().unwrap_or_else(|e| e.into_inner()) = startup;
        if production {
            let shared=runtime.shared.clone();let(ready_tx,ready_rx)=mpsc::sync_channel(1);
            let capture=std::thread::Builder::new().name("clipboard-capture".into()).spawn(move||{
                let wake=shared.clone();
                let mut pipeline=match CapturePipeline::start_controlled(shared.state.clone(),Arc::new(move||wake.wake.notify_all()),shared.closing.clone()){
                    Ok(pipeline)=>pipeline,Err(_)=>{shared.listener_failed.store(true,std::sync::atomic::Ordering::Release);let mut s=shared.state.lock().unwrap_or_else(|e|e.into_inner());s.admission=Admission::Failed;drop(s);let _=ready_tx.send(());shared.notify_status();return;}
                };
                #[cfg(feature="test-fixtures")]if let Some(exe)=test_product{pipeline.use_test_product(exe);}
                let owner=shared.state.lock().unwrap_or_else(|e|e.into_inner()).origins.owner();shared.publish_copy_owner(owner);
                let _=ready_tx.send(());shared.notify_status();
                let capture_result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
                let mut last_status=0;
                loop {
                    if shared.state.lock().unwrap_or_else(|e|e.into_inner()).stopping{break;}
                    let restart={let mut s=shared.state.lock().unwrap_or_else(|e|e.into_inner());let restart=s.restart_worker;s.restart_worker=false;restart};
                    if restart {
                        let result = pipeline.reset_worker();
                        shared.worker_restart_finished(pipeline.worker_service(), result);
                    }
                    let work=pipeline.poll_capture();
                    if !pipeline.listener_healthy(){shared.listener_failed.store(true,std::sync::atomic::Ordering::Release);break;}
                    let changed={let mut s=shared.state.lock().unwrap_or_else(|e|e.into_inner());s.worker_service=pipeline.worker_service().into();s.revision!=last_status||!matches!(work,Ok(false))};
                    if changed{shared.notify_status();last_status=shared.state.lock().unwrap_or_else(|e|e.into_inner()).revision;}
                    shared.wake.notify_all();
                    let s=shared.state.lock().unwrap_or_else(|e|e.into_inner());
                    if s.stopping{break;}
                    let _=shared.wake.wait_timeout(s,Duration::from_millis(20));
                }
                }));
                // Listener failure also closes through the same owner and borrower order.
                if !shared.closing.closing(){shared.closing.publish(Instant::now()+Duration::from_secs(3));}
                shared.close_pipeline(&mut pipeline);
                if let Err(panic)=capture_result{std::panic::resume_unwind(panic);}
            });
            *runtime.capture.lock().unwrap_or_else(|e|e.into_inner())=capture.ok();
            if ready_rx.recv_timeout(startup_deadline.saturating_duration_since(Instant::now())).is_err() {
                // Even if a delayed listener becomes ready later, this launch stays paused.
                runtime.block_startup_restore("启动时剪贴板监听未就绪。片羽保持暂停。");
                return Ok(runtime);
            }
        }
        runtime.restore_capture_at_startup(startup_deadline, |startup| {
            let (tx, rx) = mpsc::sync_channel(1);
            runtime.queries.try_send(Query::VerifyStartupCapture(startup.clone(), tx)).ok()?;
            runtime.shared.wake.notify_all();
            rx.recv_timeout(startup_deadline.saturating_duration_since(Instant::now())).ok()
        });
        Ok(runtime)
    }

    pub fn status(&self)->StatusResult{
        let s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());let(items,bytes)=s.queue_usage();
        StatusResult{revision:s.revision.to_string(),state:if s.storage_failed||s.admission==Admission::Failed{"failed"}else if s.stopping{"stopping"}else{match s.admission{Admission::Active=>"listening",Admission::Overloaded=>"overloaded",_=>"paused"}}.into(),services:ServiceReadiness{listener:if self.shared.listener_failed.load(std::sync::atomic::Ordering::Acquire){"failed"}else if s.listener_ready{"ready"}else if s.stopping{"stopped"}else{"not_started"}.into(),worker:s.worker_service.clone(),storage:if s.storage_failed{"failed"}else if s.storage_ready{"ready"}else{"stopped"}.into(),hotkey:"not_started".into(),target_observer:"not_started".into()},capture:CaptureFlags{user_enabled:s.user_enabled,admission_open:s.admission==Admission::Active,policy_epoch:s.epoch},counters:CaptureCounters{observed:s.counters.observed.to_string(),captured:s.counters.captured.to_string(),persisted:s.counters.persisted.to_string(),ignored:s.counters.ignored.to_string(),superseded:s.counters.superseded.to_string(),failed:s.counters.failed.to_string(),overloaded:s.counters.overloaded.to_string(),cancelled:s.counters.cancelled.to_string()},queue:QueueUsage{items:items as i64,bytes:bytes as i64},history:HistoryStatus{items:s.history.items,payload_bytes:s.history.payload_bytes,favorites:s.history.favorites,revision:s.history_revision.to_string()},data_dir:self.data_dir.clone(),versions:VersionsInfo{app:env!("CARGO_PKG_VERSION").into(),schema:SCHEMA_VERSION,worker_protocol:1},last_paste:None}
    }
    fn change_capture(&self,enabled:bool,change:CaptureChange)->ContractResult<Option<StartupCaptureSave<'_>>>{
        #[cfg(feature="test-fixtures")]if let Some(hook)=self.shared.before_policy.lock().unwrap_or_else(|e|e.into_inner()).clone(){hook();}
        let gate=self.shared.gate.lock().unwrap_or_else(|e|e.into_inner());
        let mut s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if let CaptureChange::Manual(sequence) = change {
            if !s.accept_manual_capture(sequence) { return Ok(None); }
        }
        if s.stopping||self.shared.closing.closing(){return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping"));}
        if enabled && s.settings_blocked{return Err(ContractError::new(ErrorCode::StorageUnavailable,"设置文件需要核对；收录已阻止，请退出后修正并重新启动。"));}
        if enabled && !s.storage_ready{return Err(unavailable());}
        if let CaptureChange::Startup(deadline) = change {
            if s.storage_failed { return Err(unavailable()); }
            if self.production && (self.shared.listener_failed.load(Ordering::Acquire) || s.worker_service == "failed") { return Err(cleanup()); }
            if Instant::now() >= deadline {
                return Err(ContractError::new(ErrorCode::Timeout, "startup capture verification timed out"));
            }
        }
        if enabled && s.storage_failed{s.storage_retry=true;s.user_enabled=true;}
        else{
            if enabled && self.production && !s.listener_ready{return Err(cleanup());}
            if enabled && s.capture_capacity_notice() == Some(Notice::HistoryCapacity) {
                return Err(ContractError::new(ErrorCode::CapacityExceeded,"history capacity reached"));
            }
            if enabled&&self.production&&s.worker_service=="failed"{s.user_enabled=true;s.restart_worker=true;}else{s.set_enabled(enabled);}
        }
        let manual = matches!(change, CaptureChange::Manual(_));
        let save = if manual && s.user_enabled == enabled {
            if !enabled || capture_active(&s) {
                Some(self.shared.admit_manual_capture_save(&mut s, enabled))
            } else {
                s.startup_capture_writes.pending_start = true;
                None
            }
        } else { None };
        drop(s);drop(gate);
        // Publish the live choice before entering the serialized preference writer.
        if manual { self.shared.notify_status(); }
        Ok(save)
    }
    pub fn set_enabled(&self, enabled: bool) -> ContractResult<StatusResult> {
        self.set_enabled_in_order(self.reserve_manual_capture_sequence(), enabled)
    }

    pub(crate) fn reserve_manual_capture_sequence(&self) -> u64 {
        self.shared.manual_capture_sequence.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(crate) fn set_enabled_in_order(&self, sequence: u64, enabled: bool) -> ContractResult<StatusResult> {
        let save = self.change_capture(enabled, CaptureChange::Manual(sequence))?;
        if let Some(save) = save { self.shared.persist_manual_capture(save); }
        Ok(self.status())
    }

    fn block_startup_restore(&self, reason: &str) {
        let mut startup = self.shared.startup_capture.lock().unwrap_or_else(|e| e.into_inner());
        if startup.should_restore() { startup.restored(Some(reason.into())); }
    }

    fn restore_capture_at_startup(&self, startup_deadline: Instant, verify: impl FnOnce(&StartupCapture) -> Option<(StartupCapture, bool)>) {
        let mut startup = self.shared.startup_capture.lock().unwrap_or_else(|e| e.into_inner());
        if !startup.should_restore() { return; }
        let reason = {
            let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if !state.storage_ready || state.storage_failed {
                Some("历史存储未就绪。片羽保持暂停。")
            } else if state.settings_blocked {
                Some("应用排除设置不可用。片羽保持暂停，请退出后核对 settings.json。")
            } else if self.production && !state.listener_ready {
                Some("剪贴板监听不可用。片羽保持暂停。")
            } else { None }
        };
        let reason = if reason.is_some() { reason }
        else {
            let current = verify(&startup).filter(|_| Instant::now() < startup_deadline).map(|(verified, current)| {
                *startup = verified;
                current
            });
            match current {
                None => { startup.read_timed_out(); return; }
                Some(false) => Some("启动时无法确认启动恢复配置。片羽保持暂停。"),
                Some(true) => match self.change_capture(true, CaptureChange::Startup(startup_deadline)) {
                    Ok(_) => None,
                    Err(error) if error.code == ErrorCode::Timeout => { startup.read_timed_out(); return; }
                    Err(error) if error.code == ErrorCode::CapacityExceeded => Some("历史容量已满。片羽保持暂停。"),
                    Err(_) => Some("未能恢复上次收录状态。片羽保持暂停。"),
                },
            }
        };
        startup.restored(reason.map(str::to_owned));
    }

    pub(crate) fn startup_capture(&self) -> StartupCaptureSnapshot {
        self.shared.startup_capture.lock().unwrap_or_else(|e| e.into_inner()).snapshot()
    }

    pub(crate) fn set_startup_capture(&self, enabled: bool) -> ContractResult<StartupCaptureSnapshot> {
        let mut _save = None;
        self.shared.update_startup_capture(|startup, resolved| {
            // Serialize opt-in/out before taking the live snapshot. Later manual
            // changes can proceed and will reconcile against this saved preference.
            let (preference, manual_generation) = {
                let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.stopping || self.shared.closing.closing() {
                    return Err(ContractError::new(ErrorCode::Busy, "runtime is stopping"));
                }
                state.startup_capture_writes.saving += 1;
                _save = Some(StartupCaptureSave(&self.shared));
                let preference = if enabled { Preference::manual(capture_active(&state)) } else { Preference::Disabled };
                (preference, state.startup_capture_writes.last_manual.map_or(0, |(generation, _)| generation))
            };
            self.shared.persist_startup_capture(startup, preference)?;
            *resolved = manual_generation;
            if !enabled { startup.clear_restore_block(); }
            Ok(startup.snapshot())
        })
    }
    pub(crate) fn set_favorite_listener(&self, listener: FavoriteEvents) {
        let _ = self.shared.favorite_events.set(listener);
    }

    pub fn set_favorite(
        &self,
        request: SetFavoriteRequest,
        panel: Option<(&crate::manual_panel::PanelSessions, &str)>,
    ) -> ContractResult<SetFavoriteResult> {
        let deadline = Instant::now() + Duration::from_secs(5);
        parse_positive_id_str(&request.clip_id)?;
        if self.shared.closing.closing() {
            return Err(ContractError::new(ErrorCode::Busy, "favorite request is no longer active"));
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let enqueue = || self.queries.try_send(Query::SetFavorite(request, deadline, tx))
            .map_err(|_| ContractError::new(ErrorCode::Busy, "history query queue is full"));
        if let Some((sessions, id)) = panel { sessions.admit(id, enqueue)??; }
        else { enqueue()?; }
        self.shared.wake.notify_all();
        let result = rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| ContractError::new(ErrorCode::Timeout, "favorite result unknown; refresh history"))?;
        if let Some((sessions, id)) = panel { sessions.admit(id, || ())?; }
        result
    }

    pub fn manage_history(&self,request:HistoryManagement)->ContractResult<HistoryManagementResult>{
        match &request {
            HistoryManagement::Delete { clip_id, .. } => { parse_positive_id(&serde_json::json!(clip_id))?; }
            HistoryManagement::Clear { expected_history_revision, .. } => {
                parse_u64_counter(&serde_json::json!(expected_history_revision))?;
            }
            HistoryManagement::Backup {} => {}
        }
        let(tx,rx)=mpsc::sync_channel(1);
        self.queries.try_send(Query::Manage(request,PathBuf::from(&self.data_dir),Instant::now()+Duration::from_secs(5),tx)).map_err(|_|ContractError::new(ErrorCode::Busy,"history query queue is full"))?;
        self.shared.wake.notify_all();
        rx.recv_timeout(Duration::from_secs(10)).map_err(|_|ContractError::new(ErrorCode::Timeout,"management result unknown; refresh before another operation"))?
    }
    pub fn list(&self,request:ListClipsRequest)->ContractResult<ListClipsResult>{
        if self.shared.state.lock().unwrap_or_else(|e|e.into_inner()).stopping{return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping"));}
        let(tx,rx)=mpsc::sync_channel(1);self.queries.try_send(Query::List(request,tx)).map_err(|_|ContractError::new(ErrorCode::Busy,"history query queue is full"))?;self.shared.wake.notify_all();rx.recv_timeout(Duration::from_secs(5)).map_err(|_|ContractError::new(ErrorCode::Timeout,"history query timed out"))?
    }
    pub fn get_clip_text(&self,request:GetClipTextRequest)->ContractResult<GetClipTextResult>{
        let request=parse_get_clip_text_request(&serde_json::json!({"request_id":request.request_id,"clip_id":request.clip_id,"offset":request.offset}))?;
        if self.shared.state.lock().unwrap_or_else(|e|e.into_inner()).stopping{return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping"));}
        let(tx,rx)=mpsc::sync_channel(1);self.queries.try_send(Query::Text(request,tx)).map_err(|_|ContractError::new(ErrorCode::Busy,"history query queue is full"))?;self.shared.wake.notify_all();rx.recv_timeout(Duration::from_secs(5)).map_err(|_|ContractError::new(ErrorCode::Timeout,"history query timed out"))?
    }
    pub(crate) fn get_png_bytes(
        &self,
        clip_id: i64,
        panel: Option<(&crate::manual_panel::PanelSessions, &str)>,
    ) -> ContractResult<Vec<u8>> {
        if self.shared.closing.closing() {
            return Err(ContractError::new(ErrorCode::Busy, "runtime is stopping"));
        }
        let timeout = Duration::from_secs(5);
        let (tx, rx) = mpsc::sync_channel(1);
        let enqueue = || self.queries.try_send(Query::Png(clip_id, Instant::now() + timeout, tx))
            .map_err(|_| ContractError::new(ErrorCode::Busy, "history query queue is full"));
        match panel {
            Some((sessions, session)) => sessions.admit(session, enqueue)??,
            None => enqueue()?,
        }
        self.shared.wake.notify_all();
        rx.recv_timeout(timeout).map_err(|_| ContractError::new(ErrorCode::Timeout, "PNG query timed out"))?
    }
    /// Internal Rust entry only; the WebView has no arbitrary write command.
    pub fn write_stored_text(&self,clip_id:i64)->ContractResult<u64>{
        if clip_id<=0{return Err(ContractError::invalid_input("invalid clip id"));}
        self.copy_with_budget(clip_id,WriteFormat::Text,Instant::now()+Duration::from_secs(3),Duration::from_secs(3)).map_err(|failure|failure.error)
    }
    fn queue_copy(&self,id:i64,format:WriteFormat,deadline:Instant)->Result<Receiver<WriteReply>,WriteFailure>{
        if self.shared.copy_stopping.load(Ordering::Acquire){return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping").into());}
        if self.shared.copy_busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return Err(ContractError::new(ErrorCode::Busy,"clipboard write is pending").into());}
        let permit=WritePermit(self.shared.copy_busy.clone());
        let(tx,rx)=mpsc::sync_channel(1);
        self.queries.try_send(Query::Copy(id,format,deadline,permit,tx)).map_err(|_|WriteFailure::from(ContractError::new(ErrorCode::Busy,"history query queue is full")))?;
        self.shared.wake.notify_all();Ok(rx)
    }
    fn copy_with_budget(&self,id:i64,format:WriteFormat,deadline:Instant,wait:Duration)->WriteReply{
        self.queue_copy(id,format,deadline)?.recv_timeout(wait).unwrap_or_else(|_|Err(WriteFailure{error:ContractError::new(ErrorCode::Timeout,"clipboard write result is unknown"),may_have_written:true}))
    }
    pub fn copy_clip_text(&self,request:CopyClipTextRequest)->ContractResult<CopyClipTextResult>{
        self.copy_clip(request, WriteFormat::Text)
    }
    pub(crate) fn copy_clip(&self,request:CopyClipTextRequest,format:WriteFormat)->ContractResult<CopyClipTextResult>{
        let request=parse_copy_clip_text_request(&serde_json::json!({"request_id":request.request_id,"clip_id":request.clip_id}))?;
        let result=self.copy_with_budget(positive_id_as_i64(&request.clip_id),format,Instant::now()+Duration::from_secs(3),Duration::from_secs(3));
        Ok(copy_result(&request,result))
    }
    pub fn copy_panel_text(&self,panel:&crate::manual_panel::PanelSessions,session:&str,request:CopyClipTextRequest)->ContractResult<CopyClipTextResult>{
        self.copy_panel(panel, session, request, WriteFormat::Text)
    }
    pub(crate) fn copy_panel(&self,panel:&crate::manual_panel::PanelSessions,session:&str,request:CopyClipTextRequest,format:WriteFormat)->ContractResult<CopyClipTextResult>{
        let request=parse_copy_clip_text_request(&serde_json::json!({"request_id":request.request_id,"clip_id":request.clip_id}))?;
        // Linearization point: hide/new-session cannot pass between validation and try_send.
        let admitted=panel.admit(session,||self.queue_copy(positive_id_as_i64(&request.clip_id),format,Instant::now()+Duration::from_secs(3)));
        let result=match admitted {
            Err(error)=>Err(error.into()),
            Ok(Err(error))=>Err(error),
            Ok(Ok(reply))=>reply.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_|Err(WriteFailure{error:ContractError::new(ErrorCode::Timeout,"clipboard write result is unknown"),may_have_written:true})),
        };
        Ok(copy_result(&request,result))
    }
    pub fn settings(&self)->SettingsSnapshot {
        let s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        SettingsSnapshot{revision:s.revision.to_string(),schema_version:1,excluded_apps:s.exclusions.clone(),saving:s.settings_saving,blocked:s.settings_blocked,error:s.settings_error.clone(),last_skip:s.last_application_skip().map(|reason|match reason{crate::runtime::ApplicationSkip::ExcludedOwner=>"excluded_owner",crate::runtime::ApplicationSkip::UnknownOwner=>"unknown_owner"}.into())}
    }
    pub fn set_exclusions(&self,paths:Vec<String>)->ContractResult<StatusResult>{self.apply_exclusions(paths)?;Ok(self.status())}
    pub fn apply_exclusions(&self,paths:Vec<String>)->ContractResult<SettingsSnapshot>{
        let mut paths=crate::config::normalize_exclusions(paths)?;
        let unchanged={
            let mut s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
            if s.stopping||self.shared.closing.closing()||s.settings_saving{return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping or settings save is pending"));}
            if s.settings_blocked{return Err(ContractError::new(ErrorCode::StorageUnavailable,"设置文件需要核对；当前实例不会覆盖该文件。请退出后修正并重新启动。"));}
            let unchanged=crate::config::same_exclusions(&s.exclusions,&paths)?;
            if unchanged{paths=s.exclusions.clone();}
            s.settings_saving=true;unchanged
        };
        self.shared.notify_status();
        // All file work runs in the command's spawn_blocking worker, outside the GUI thread.
        let prepared=self.shared.settings_file.lock().unwrap_or_else(|e|e.into_inner()).as_ref().map(|file|file.prepare(&paths)).transpose();
        let result=(||{
            let staged=prepared?;
            let _gate=self.shared.gate.lock().unwrap_or_else(|e|e.into_inner());
            if self.shared.state.lock().unwrap_or_else(|e|e.into_inner()).stopping{return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping"));}
            #[cfg(feature="test-fixtures")]if let Some(hook)=self.shared.before_settings_replace.lock().unwrap_or_else(|e|e.into_inner()).clone(){hook();}
            // Shutdown may fence during IO. It must not wait for IO on the GUI thread.
            let saved=if let Some(staged)=staged.as_ref(){self.shared.settings_file.lock().unwrap_or_else(|e|e.into_inner()).as_mut().expect("settings file owner").replace(staged)}else{Ok(())};
            let mut s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
            if let Err(failure)=saved {
                if failure.uncertain{s.settings_blocked=true;s.settings_error=Some(failure.error.message.clone());if !s.stopping{s.set_enabled(false);}}
                return Err(failure.error);
            }
            if s.stopping {
                s.settings_blocked=true;s.settings_error=Some("设置保存与退出重叠；本次没有发布新规则。请在下次启动时核对保存的排除列表。".into());
                return Err(ContractError::new(ErrorCode::Busy,"runtime stopped before settings publication; review settings on next startup"));
            }
            // Publication and the shutdown fence share this state lock; commits share gate.
            if !unchanged{s.set_exclusions(paths);}Ok(())
        })();
        self.shared.state.lock().unwrap_or_else(|e|e.into_inner()).settings_saving=false;self.shared.notify_status();
        result.map(|_|self.settings())
    }
    #[cfg(feature="test-fixtures")]
    pub fn queue_test_query(&self,request:ListClipsRequest)->ContractResult<Receiver<ContractResult<ListClipsResult>>>{let(tx,rx)=mpsc::sync_channel(1);self.queries.try_send(Query::List(request,tx)).map_err(|_|ContractError::new(ErrorCode::Busy,"history query queue is full"))?;self.shared.wake.notify_all();Ok(rx)}
    #[cfg(feature="test-fixtures")]
    pub fn queue_test_text_query(&self,request:GetClipTextRequest)->ContractResult<Receiver<ContractResult<GetClipTextResult>>>{let(tx,rx)=mpsc::sync_channel(1);self.queries.try_send(Query::Text(request,tx)).map_err(|_|ContractError::new(ErrorCode::Busy,"history query queue is full"))?;self.shared.wake.notify_all();Ok(rx)}
    #[cfg(feature="test-fixtures")]
    pub fn expired_test_copy(&self,id:i64)->ContractResult<u64>{self.copy_with_budget(id,WriteFormat::Text,Instant::now()-Duration::from_secs(1),Duration::from_secs(2)).map_err(|failure|failure.error)}
    #[cfg(feature="test-fixtures")]
    pub fn inject_response(&self,sequence:u32,mut response:crate::clipboard::capture::CaptureResponse)->ContractResult<()> {
        let mut s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());s.observe(sequence,0);let ticket=s.begin_current(sequence).ok_or_else(||ContractError::new(ErrorCode::Busy,"capture admission closed"))?;response.capture_id=ticket.message.capture_id.clone();response.policy_epoch=ticket.message.policy_epoch;let decoded=crate::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&response).map_err(|_|unavailable())?,&ticket.message)?;s.complete(ticket,decoded);drop(s);self.shared.notify_status();Ok(())
    }
    #[cfg(feature="test-fixtures")]
    pub fn set_test_fault(&self,enabled:bool)->ContractResult<()> {let(tx,rx)=mpsc::sync_channel(1);self.queries.try_send(Query::Fault(enabled,tx)).map_err(|_|unavailable())?;self.shared.wake.notify_all();rx.recv_timeout(Duration::from_secs(3)).map_err(|_|unavailable())?}
    pub fn begin_shutdown(&self,deadline:Instant){
        {let mut state=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());self.shared.closing.publish(deadline);state.stop_observing();}
        self.shared.revoke_copy_owner();
        self.shared.wake.notify_all();
        self.shared.notify_status();
    }
    pub fn retry_saving(&self)->ContractResult<StatusResult>{
        let mut s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if !s.stopping{return Err(ContractError::new(ErrorCode::Busy,"save retry requires closing runtime"));}
        if s.storage_failed{s.storage_retry=true;}
        drop(s);self.shared.notify_status();Ok(self.status())
    }
    pub fn shutdown(&self,budget:Duration)->ContractResult<()> {self.shutdown_until(Instant::now()+budget).map_err(|e|if e.code==ErrorCode::Timeout{cleanup()}else{e})}
    pub fn shutdown_until(&self,deadline:Instant)->ContractResult<()> {
        if self.shared.closing.current().1!=Some(deadline){self.begin_shutdown(deadline);}
        let mut state=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        while state.settings_saving || state.startup_capture_writes.saving != 0 {
            let remaining=deadline.saturating_duration_since(Instant::now());if remaining.is_zero(){break;}
            state=self.shared.wake.wait_timeout(state,remaining).unwrap_or_else(|e|e.into_inner()).0;
        }
        let settings_pending=state.settings_saving;
        let startup_pending=state.startup_capture_writes.saving != 0;drop(state);
        let capture=finish_thread(&self.capture,deadline,&self.shared.cleanup_failed);
        let storage=finish_thread(&self.storage,deadline,&self.shared.cleanup_failed);
        let writer = finish_thread(&self.startup_writer, deadline, &self.shared.cleanup_failed);
        let terminal=self.shared.cleanup_failed.load(Ordering::Acquire);
        if capture.is_err()||storage.is_err()||writer.is_err()||terminal||settings_pending||startup_pending{
            let mut errors=self.shared.cleanup_errors.lock().unwrap_or_else(|e|e.into_inner()).clone();
            if capture.is_err(){errors.push("抓取资源仍在回收或已失败".into());}if storage.is_err(){errors.push("存储资源仍在回收或已失败".into());}
            if settings_pending{errors.push("设置保存仍在结束；尚不能退出".into());}
            if startup_pending{errors.push("启动恢复设置仍在保存；尚不能退出".into());}
            else if writer.is_err(){errors.push("启动恢复设置写入线程仍在回收或已失败".into());}
            if self.shared.state.lock().unwrap_or_else(|e|e.into_inner()).storage_failed{errors.push("保存失败，已接纳队列仍保留".into());}
            return Err(ContractError::new(if terminal{ErrorCode::CleanupFailed}else{ErrorCode::Timeout},&format!("资源回收未完成：{}",errors.join("; "))));
        }
        self.exit_ready.store(true,std::sync::atomic::Ordering::Release);Ok(())
    }
    pub fn exit_failed(&self){self.shared.state.lock().unwrap_or_else(|e|e.into_inner()).admission=Admission::Failed;self.shared.notify_status();}
}
fn copy_result(request:&CopyClipTextRequest,result:WriteReply)->CopyClipTextResult{
    match result{
        Ok(_)=>CopyClipTextResult::new(request,CopyOutcome::Copied,None),
        Err(failure)=>CopyClipTextResult::new(request,if failure.may_have_written{CopyOutcome::Unknown}else{CopyOutcome::NotWritten},Some(failure.error.code)),
    }
}

fn execute_copy(shared:&SharedRuntime,store:&Option<Store>,id:i64,format:WriteFormat,deadline:Instant)->WriteReply{
    let before_write=||{
        if shared.copy_stopping.load(Ordering::Acquire){return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping"));}
        if Instant::now()>=deadline{return Err(ContractError::new(ErrorCode::Timeout,"clipboard write expired"));}
        Ok(())
    };
    before_write()?;
    #[cfg(feature="test-fixtures")]
    let test_writer=shared.test_writer.lock().unwrap_or_else(|e|e.into_inner()).clone();
    #[cfg(feature="test-fixtures")]
    let synthetic=test_writer.is_some();
    #[cfg(not(feature="test-fixtures"))]
    let synthetic=false;
    if !shared.native_writes&&!synthetic{return Err(ContractError::new(ErrorCode::PermissionUnknown,"native clipboard writes are disabled in synthetic tests").into());}
    let store = store.as_ref().ok_or_else(unavailable)?;
    let data = match format {
        WriteFormat::Text => {
            let clip = store.get(id)?.ok_or_else(|| ContractError::new(ErrorCode::NotFound, "clip not found"))?;
            let text = clip.snapshot.payloads.into_iter().find(|payload| payload.format_name == "CF_UNICODETEXT")
                .ok_or_else(|| ContractError::invalid_input("clip has no Unicode text"))?;
            crate::clipboard::capture::validate_payloads(std::slice::from_ref(&text))
                .map_err(|_| ContractError::invalid_input("invalid clipboard text"))?;
            text.data
        }
        WriteFormat::Png => {
            let data = store.get_png(id)?;
            crate::png_clip::validate(&data)?;
            data
        }
    };
    before_write()?;
    let owner=shared.borrow_copy_owner()?;
    let mut writer:Box<dyn WriteApi>= {
        #[cfg(feature="test-fixtures")]
        if let Some(factory)=test_writer {factory()?}else{Box::new(crate::clipboard::windows::WindowsWriter::new()?)}
        #[cfg(not(feature="test-fixtures"))]
        {Box::new(crate::clipboard::windows::WindowsWriter::new()?)}
    };
    let(counter,marker)={
        let mut s=shared.state.lock().unwrap_or_else(|e|e.into_inner());
        before_write()?;
        if s.stopping{return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping").into());}
        if s.origins.owner()!=owner.owner{return Err(ContractError::new(ErrorCode::InjectionFailed,"clipboard owner unavailable").into());}
        // Pausing clears queued snapshots and invalidates in-flight captures by epoch.
        if !s.user_enabled&&s.origins.len()>=crate::clipboard::origin::MAX_OPERATIONS{s.origins.retire_finished_noncurrent();}
        s.origins.begin()?
    };
    let attempt=crate::clipboard::origin::write_registered_payload(writer.as_mut(),owner.owner,counter,&marker,format,&data,before_write);
    // Native writer Drop may finish a failed close. Keep the listener borrowed
    // through that cleanup and the final origin-registry transition.
    drop(writer);
    shared.state.lock().unwrap_or_else(|e|e.into_inner()).origins.finish(counter,attempt.sequence);
    drop(owner);
    attempt.result
}
fn finish_thread(slot:&Mutex<Option<JoinHandle<()>>>,deadline:Instant,cleanup_failed:&std::sync::atomic::AtomicBool)->ContractResult<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::WAIT_OBJECT_0,System::Threading::WaitForSingleObject};
    let mut slot=slot.lock().unwrap_or_else(|e|e.into_inner());let Some(thread)=slot.as_ref() else{return Ok(());};
    let timeout=deadline.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32;
    if unsafe{WaitForSingleObject(thread.as_raw_handle(),timeout)}!=WAIT_OBJECT_0{return Err(cleanup());}
    // A consumed failed join must not become success on the next shutdown call.
    slot.take().unwrap().join().map_err(|_|{cleanup_failed.store(true,std::sync::atomic::Ordering::Release);cleanup()})
}
impl Drop for HostRuntime {
    fn drop(&mut self) {
        if !self.shared.closing.closing() {
            let _ = self.shutdown(Duration::from_secs(3));
        } else {
            let _state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            self.shared.wake.notify_all();
        }
        for slot in [&self.capture, &self.storage, &self.startup_writer] {
            if let Some(thread) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                RETAINED.lock().unwrap_or_else(|e| e.into_inner()).push(thread);
            }
        }
    }
}

fn manage_on_storage(shared:&SharedRuntime,store:&mut Option<Store>,request:HistoryManagement,directory:&std::path::Path,deadline:Instant)->ContractResult<HistoryManagementResult>{
    let _gate=shared.gate.lock().unwrap_or_else(|e|e.into_inner());
    let mut state=shared.state.lock().unwrap_or_else(|e|e.into_inner());
    if Instant::now()>deadline || state.stopping || shared.closing.closing() || state.user_enabled || state.queue_usage().0!=0 || state.storage_failed {
        return Err(ContractError::new(ErrorCode::Busy,"pause capture and wait for an empty queue before managing history"));
    }
    let db=store.as_mut().ok_or_else(unavailable)?;
    let usage = db.usage()?;
    let changed = || ContractError::new(ErrorCode::Busy, "history changed; refresh before confirming");
    let mut backup_path = None;
    let (deleted, usage) = match request {
        HistoryManagement::Delete { clip_id, expected_items } => {
            if usage.items as u64 != expected_items { return Err(changed()); }
            let deleted = db.delete(positive_id_as_i64(&clip_id))? as u64;
            (deleted, db.usage()?)
        }
        HistoryManagement::Clear { expected_items, expected_history_revision } => {
            if usage.items as u64 != expected_items || state.history_revision.to_string() != expected_history_revision {
                return Err(changed());
            }
            db.clear_history()?
        }
        HistoryManagement::Backup {} => {
            let backups = directory.join("backups");
            std::fs::create_dir_all(&backups).map_err(|_| unavailable())?;
            let path = backups.join(format!("history-{}.sqlite3", uuid::Uuid::new_v4()));
            db.backup_to(&path)?;
            backup_path = Some(path.display().to_string());
            (0, usage)
        }
    };
    let result = HistoryManagementResult { items: usage.items as u64, favorites: usage.favorites as u64, deleted, backup_path };
    state.history = usage;
    let revision = if deleted != 0 {
        state.history_revision += 1;
        Some(state.history_revision)
    } else { None };
    drop(state);
    drop(_gate);
    if let Some(revision) = revision { (shared.events)("history_changed", revision); }
    Ok(result)
}

fn check_favorite_write(shared: &SharedRuntime, state: &CaptureState, deadline: Instant) -> ContractResult<()> {
    if Instant::now() >= deadline || state.stopping || shared.closing.closing() {
        return Err(ContractError::new(ErrorCode::Busy, "favorite request is no longer active"));
    }
    if !state.storage_ready || state.storage_failed { return Err(unavailable()); }
    Ok(())
}

fn favorite_on_storage(
    shared: &SharedRuntime, store: &mut Option<Store>, request: SetFavoriteRequest, deadline: Instant,
) -> ContractResult<SetFavoriteResult> {
    let gate = shared.gate.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    check_favorite_write(shared, &state, deadline)?;
    let db = store.as_mut().ok_or_else(unavailable)?;
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis().min(JS_SAFE_INTEGER_MAX as u128) as i64;
    let (changed, usage) = db.set_favorite(positive_id_as_i64(&request.clip_id), request.favorited, now_ms)?;
    state.history = usage;
    if changed { state.history_revision += 1; }
    let change = FavoriteChange {
        clip_id: request.clip_id, favorited: request.favorited, revision: state.history_revision.to_string(),
    };
    drop(state);
    drop(gate);
    if changed {
        if let Some(emit) = shared.favorite_events.get() { emit(&change); }
        shared.notify_status();
    }
    Ok(SetFavoriteResult { request_id: request.request_id, changed, change })
}

fn storage_loop(shared:&SharedRuntime,store:&mut Option<Store>,queries:Receiver<Query>,read_preference:impl Fn(&Path)->std::io::Result<Option<Vec<u8>>>){
    loop {
        for _ in 0..4 {match queries.try_recv(){
            Ok(Query::VerifyStartupCapture(mut startup, reply)) => {
                let current = startup.startup_file_current(&read_preference);
                let _ = reply.send((startup, current));
            }
            Ok(Query::Manage(request,directory,deadline,reply))=>{
                let result=manage_on_storage(shared,store,request,&directory,deadline);
                let _=reply.send(result);
                shared.notify_status();
            }
            Ok(Query::SetFavorite(request, deadline, reply)) => {
                let result = favorite_on_storage(shared, store, request, deadline);
                let _ = reply.send(result);
            }
            Ok(Query::List(request,reply))=>{let _=reply.send(store.as_ref().ok_or_else(unavailable).and_then(|s|s.list_recent(&request)));}
            Ok(Query::Text(request,reply))=>{let _=reply.send(store.as_ref().ok_or_else(unavailable).and_then(|s|s.get_clip_text(&request)));}
            Ok(Query::Png(id, deadline, reply)) => {
                let result = if shared.closing.closing() || Instant::now() >= deadline {
                    Err(ContractError::new(ErrorCode::Busy, "PNG query is no longer active"))
                } else {
                    store.as_ref().ok_or_else(unavailable).and_then(|db| db.get_png(id))
                };
                let _ = reply.send(result);
            }
            Ok(Query::Copy(id,format,deadline,permit,reply))=>{
                let result=execute_copy(shared,store,id,format,deadline);
                drop(permit);let _=reply.send(result);shared.wake.notify_all();
            }
            #[cfg(feature="test-fixtures")]
            Ok(Query::Fault(enabled,reply))=>{let _=reply.send(store.as_mut().ok_or_else(unavailable).and_then(|s|s.set_test_commit_fault(enabled)));}
            Err(_)=>break,
        }}
        let wrote=store.as_mut().is_some_and(|s|shared.commit_one(s));
        let mut state=shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if state.stopping&&state.queue_usage().0==0{state.storage_ready=false;break;}
        if wrote{continue;}
        let _=shared.wake.wait_timeout(state,Duration::from_millis(20));
    }
}

#[cfg(all(test,feature="test-fixtures"))]
#[path="host_explicit_copy_tests.rs"]
mod explicit_copy_tests;

#[cfg(all(test,feature="test-fixtures"))]
#[path="host_residency_tests.rs"]
mod residency_tests;

#[cfg(all(test,feature="test-fixtures"))]
#[path="host_settings_tests.rs"]
mod settings_tests;

#[cfg(all(test, feature = "test-fixtures"))]
#[path = "host_startup_capture_tests.rs"]
mod startup_capture_tests;

#[cfg(all(test,feature="test-fixtures"))]
mod shutdown_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    // Inject only the close-result boundary; never construct native capture resources.
    fn capture(runtime:&HostRuntime,result:ContractResult<()>,panic:bool)->mpsc::Sender<()> {
        let shared=runtime.shared.clone();let(tx,rx)=mpsc::channel();let(ready_tx,ready_rx)=mpsc::channel();
        let thread=std::thread::spawn(move||{
            ready_tx.send(()).unwrap();let _=rx.recv_timeout(Duration::from_secs(5));
            assert!(!panic,"synthetic capture shutdown panic");
            shared.capture_closed(result);
        });
        *runtime.capture.lock().unwrap()=Some(thread);
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();tx
    }
    fn memory()->HostRuntime{HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap()}
    fn assert_failed(runtime:&HostRuntime,result:ContractResult<()>){
        assert_eq!(result.unwrap_err().code,ErrorCode::CleanupFailed);
        assert!(!runtime.exit_ready.load(Ordering::Acquire));
    }
    fn assert_joined(runtime:&HostRuntime){
        assert!(runtime.capture.lock().unwrap().is_none());
        assert!(runtime.storage.lock().unwrap().is_none(),"storage must be joined despite capture failure");
        assert_eq!(runtime.status().services.storage,"stopped");
    }

    #[test]
    fn r1_shutdown_cleanup_success_is_repeatable(){
        let runtime=memory();capture(&runtime,Ok(()),false).send(()).unwrap();
        runtime.shutdown(Duration::from_secs(2)).unwrap();runtime.shutdown(Duration::ZERO).unwrap();
        assert!(runtime.exit_ready.load(Ordering::Acquire));assert_joined(&runtime);
    }
    #[test]
    fn r1_shutdown_operational_failure_does_not_mean_cleanup_failure(){
        let runtime=memory();runtime.shared.state.lock().unwrap().worker_service="failed".into();
        runtime.shutdown(Duration::from_secs(2)).unwrap();
        assert!(runtime.exit_ready.load(Ordering::Acquire));assert_joined(&runtime);
    }
    #[test]
    fn r1_shutdown_cleanup_error_survives_repeated_shutdown(){
        let runtime=memory();capture(&runtime,Err(cleanup()),false).send(()).unwrap();
        assert_failed(&runtime,runtime.shutdown(Duration::from_secs(2)));assert_joined(&runtime);
        assert_failed(&runtime,runtime.shutdown(Duration::ZERO));
        assert_eq!(runtime.status().services.worker,"failed");
    }
    #[test]
    fn r1_shutdown_capture_panic_survives_consumed_handle(){
        let runtime=memory();capture(&runtime,Ok(()),true).send(()).unwrap();
        assert_failed(&runtime,runtime.shutdown(Duration::from_secs(2)));assert_joined(&runtime);
        assert_failed(&runtime,runtime.shutdown(Duration::ZERO));
    }
    #[test]
    fn r1_shutdown_storage_panic_survives_consumed_handle(){
        let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{
            assert_ne!(std::thread::current().name(),Some("history-storage"),"synthetic storage shutdown panic");
        })).unwrap();
        assert_failed(&runtime,runtime.shutdown(Duration::from_secs(2)));assert_joined(&runtime);
        assert_failed(&runtime,runtime.shutdown(Duration::ZERO));
    }
    #[test]
    fn r1_shutdown_timeout_retains_capture_until_success(){
        let runtime=memory();let release=capture(&runtime,Ok(()),false);
        let timed_out=runtime.shutdown(Duration::ZERO);let retained=runtime.capture.lock().unwrap().is_some();
        let ready=runtime.exit_ready.load(Ordering::Acquire);release.send(()).unwrap();
        let completed=runtime.shutdown(Duration::from_secs(2));
        assert_eq!(timed_out.unwrap_err().code,ErrorCode::CleanupFailed);assert!(retained);assert!(!ready);
        completed.unwrap();assert!(runtime.exit_ready.load(Ordering::Acquire));assert_joined(&runtime);
    }
    #[test]
    fn r1_shutdown_timeout_then_cleanup_error_stays_failed(){
        let runtime=memory();let release=capture(&runtime,Err(cleanup()),false);
        let timed_out=runtime.shutdown(Duration::ZERO);release.send(()).unwrap();
        assert_eq!(timed_out.unwrap_err().code,ErrorCode::CleanupFailed);
        assert_failed(&runtime,runtime.shutdown(Duration::from_secs(2)));assert_joined(&runtime);
        assert_failed(&runtime,runtime.shutdown(Duration::ZERO));
    }
}

#[cfg(all(test,feature="test-fixtures"))]
mod m2_retry_baseline_tests {
    use super::*;
    #[test]
    fn m2_save_retry_cannot_reopen_capture_after_stopping(){
        let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();
        {let mut s=runtime.shared.state.lock().unwrap();s.stop_observing();s.storage_failed=true;}
        let result=runtime.set_enabled(true);
        let reopened=runtime.status().capture.user_enabled;
        {let mut s=runtime.shared.state.lock().unwrap();s.storage_failed=false;s.storage_retry=false;}
        runtime.shutdown(Duration::from_secs(1)).unwrap();
        assert!(result.is_err(),"management capture intent must reject after closing");
        assert!(!reopened,"save retry must never reopen capture");
    }
}

#[cfg(all(test, feature = "test-fixtures"))]
mod favorite_tests {
    use super::*;
    use crate::{manual_panel::{PanelSessions, PanelSnapshot, PanelWindow}, store::{Payload, Snapshot, SourceConfidence}};

    struct Window;
    impl PanelWindow for Window {
        fn hide(&self) -> Result<(), String> { Ok(()) }
        fn show(&self) -> Result<(), String> { Ok(()) }
        fn focus(&self) -> Result<(), String> { Ok(()) }
        fn publish(&self, _: &PanelSnapshot) -> Result<(), String> { Ok(()) }
    }

    fn favorite_request() -> SetFavoriteRequest {
        parse_set_favorite_request(&serde_json::json!({"request_id":"fav","clip_id":"1","favorited":true})).unwrap()
    }

    fn runtime(events: Events) -> HostRuntime {
        let root = PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").unwrap())
            .join(format!("favorite-host-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("history.sqlite3");
        let mut store = Store::open(&path).unwrap();
        store.insert(&Snapshot {
            capture_id: "synthetic".into(), captured_at_ms: 1, source_process: None,
            source_confidence: SourceConfidence::Unknown, omitted_formats: vec![],
            payloads: vec![Payload { format_name: "CF_UNICODETEXT".into(), data: vec![65,0,0,0] }],
        }).unwrap();
        drop(store);
        HostRuntime::for_test_file(path, events).unwrap()
    }

    #[test]
    fn expired_query_does_not_write_and_dropped_reply_still_broadcasts_durable_change() {
        let (status_tx, status_rx) = mpsc::channel();
        let runtime = runtime(Arc::new(move |event, _| {
            if event == "status_changed" { let _ = status_tx.send(()); }
        }));
        while status_rx.try_recv().is_ok() {}
        let (event_tx, event_rx) = mpsc::channel();
        runtime.set_favorite_listener(Arc::new(move |change| {
            event_tx.send((change.clip_id.clone(), change.favorited, change.revision.clone())).unwrap();
        }));
        let (tx, rx) = mpsc::sync_channel(1);
        runtime.queries.try_send(Query::SetFavorite(favorite_request(), Instant::now(), tx)).unwrap();
        runtime.shared.wake.notify_all();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap_err().code, ErrorCode::Busy);
        assert_eq!(runtime.status().history.favorites, 0);
        assert!(event_rx.try_recv().is_err());

        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        runtime.queries.try_send(Query::SetFavorite(favorite_request(), Instant::now() + Duration::from_secs(2), tx)).unwrap();
        runtime.shared.wake.notify_all();
        assert_eq!(event_rx.recv_timeout(Duration::from_secs(2)).unwrap(), ("1".into(), true, "1".into()));
        status_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let status = runtime.status();
        assert_eq!(status.history.favorites, 1);
        assert_eq!(status.history.revision, "1");
        runtime.shutdown(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn favorite_wait_times_out_while_storage_state_is_locked_without_writing_later() {
        let runtime = runtime(Arc::new(|_, _| {}));
        let (started_tx, started_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let result = std::thread::scope(|scope| {
            let state = runtime.shared.state.lock().unwrap();
            let runtime = &runtime;
            scope.spawn(move || {
                let started = Instant::now();
                started_tx.send(()).unwrap();
                let result = runtime.set_favorite(favorite_request(), None);
                result_tx.send((started.elapsed(), result)).unwrap();
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let result = result_rx.recv_timeout(Duration::from_millis(6500));
            drop(state);
            result
        });

        // An ordered list query drains the earlier favorite request after unlocking.
        let list = parse_list_clips_request(&serde_json::json!({"request_id":"after-fav","query":""})).unwrap();
        let rows = runtime.list(list).unwrap().summaries;
        let history = runtime.status().history;
        runtime.shutdown(Duration::from_secs(2)).unwrap();

        let (elapsed, result) = result.expect("set_favorite did not return while state was locked");
        assert!(elapsed >= Duration::from_secs(5), "returned before its deadline: {elapsed:?}");
        assert_eq!(result.unwrap_err().code, ErrorCode::Timeout);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].favorited);
        assert_eq!(history.favorites, 0);
        assert_eq!(history.revision, "0");
    }

    #[test]
    fn panel_must_be_ready_at_enqueue_and_still_current_after_storage_reply() {
        let runtime = runtime(Arc::new(|_, _| {}));
        let sessions = PanelSessions::new(true);
        let session = sessions.open(&Window).unwrap().session_id.unwrap();
        let unready = runtime.set_favorite(favorite_request(), Some((&sessions, &session))).unwrap_err();
        assert_eq!(unready.code, ErrorCode::PermissionUnknown);
        assert_eq!(runtime.status().history.favorites, 0);
        sessions.ready(&session, &Window).unwrap();
        let (committed, observed) = mpsc::channel();
        let (release, done) = mpsc::channel();
        let done = Mutex::new(done);
        runtime.set_favorite_listener(Arc::new(move |_| {
            committed.send(()).unwrap();
            done.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
        }));
        std::thread::scope(|scope| {
            let result = scope.spawn(|| runtime.set_favorite(favorite_request(), Some((&sessions, &session))));
            observed.recv_timeout(Duration::from_secs(2)).unwrap();
            sessions.hide(&session, &Window, false).unwrap();
            release.send(()).unwrap();
            assert_eq!(result.join().unwrap().unwrap_err().code, ErrorCode::PermissionUnknown);
        });
        assert_eq!(runtime.status().history.favorites, 1);
        runtime.shutdown(Duration::from_secs(2)).unwrap();
    }
}
