//! E06 runtime ownership: the capture thread owns native capture resources;
//! the storage thread creates and owns its SQLite connection.
use std::{path::PathBuf,sync::{Arc,Mutex,Condvar,mpsc::{self,SyncSender,Receiver}},thread::JoinHandle,time::{Duration,Instant}};
use crate::{contracts::*,runtime::{CaptureState,CapturePipeline,Admission},store::Store};
use crate::clipboard::origin::{WriteApi,WriteFailure};
use std::sync::atomic::{AtomicBool,Ordering};

struct WritePermit(Arc<AtomicBool>);
impl Drop for WritePermit {fn drop(&mut self){self.0.store(false,Ordering::Release);}}
type WriteReply=Result<u64,WriteFailure>;
struct CopyOwner {owner:Option<usize>,borrowers:usize}
struct CopyOwnerLease<'a> {shared:&'a SharedRuntime,owner:usize}
impl Drop for CopyOwnerLease<'_> {
    fn drop(&mut self){let mut owner=self.shared.copy_owner.lock().unwrap_or_else(|e|e.into_inner());owner.borrowers-=1;self.shared.owner_idle.notify_all();}
}

pub type Events=Arc<dyn Fn(&str,u64)+Send+Sync>;
pub struct SharedRuntime {
    pub state:Arc<Mutex<CaptureState>>,
    pub gate:Mutex<()>,
    wake:Condvar,
    events:Events,
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
}
enum Target {File(PathBuf),#[cfg(feature="test-fixtures")] Memory,#[cfg(feature="test-fixtures")] Manual{path:PathBuf,product:PathBuf}}
#[derive(Debug,serde::Deserialize)]
#[serde(tag="action",rename_all="snake_case",deny_unknown_fields)]
pub enum HistoryManagement {
    Delete { clip_id:String, expected_items:u64 },
    Clear { expected_items:u64 },
    Backup {},
}
#[derive(Debug,serde::Serialize)]
pub struct HistoryManagementResult { pub items:u64, pub deleted:u64, pub backup_path:Option<String> }

enum Query {
    Manage(HistoryManagement,PathBuf,Instant,SyncSender<ContractResult<HistoryManagementResult>>),
    List(ListClipsRequest,SyncSender<ContractResult<ListClipsResult>>),
    Copy(i64,Instant,WritePermit,SyncSender<WriteReply>),
    Text(GetClipTextRequest,SyncSender<ContractResult<GetClipTextResult>>),
    #[cfg(feature="test-fixtures")]
    Fault(bool,SyncSender<ContractResult<()>>),
}
pub struct HostRuntime {
    pub shared:Arc<SharedRuntime>,
    queries:SyncSender<Query>,
    storage:Mutex<Option<JoinHandle<()>>>,
    capture:Mutex<Option<JoinHandle<()>>>,
    data_dir:String,
    production:bool,
    pub exit_pending:std::sync::atomic::AtomicBool,
    pub exit_ready:std::sync::atomic::AtomicBool,
}
fn unavailable()->ContractError{ContractError::new(ErrorCode::StorageUnavailable,"history storage unavailable")}
fn cleanup()->ContractError{ContractError::new(ErrorCode::CleanupFailed,"runtime cleanup incomplete")}
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
    fn commit_one(&self,store:&mut Store)->bool{
        let gate=self.gate.lock().unwrap_or_else(|e|e.into_inner());
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        if s.storage_failed&&!s.storage_retry{return false;}
        if s.storage_retry {
            s.storage_retry=false;
            match store.usage(){Ok(usage)=>{s.history=usage;s.storage_failed=false;},Err(_)=>return false,}
            if s.queue.is_empty(){if !s.stopping&&s.user_enabled{s.admission=Admission::Active;}drop(s);drop(gate);self.notify_status();return true;}
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
                if !s.stopping&&s.user_enabled&&s.admission==Admission::Failed{s.admission=Admission::Active;}
                if ack.inserted{s.counters.persisted+=1;s.history_revision+=1;Some(s.history_revision)}else{None}
            }
            Err(_)=>{
                item.snapshot.payloads=std::mem::take(&mut snapshot.payloads);item.snapshot.response.omitted_formats=std::mem::take(&mut snapshot.omitted_formats);
                s.bytes+=bytes;s.queue.push_front(item);s.storage_failed=true;s.admission=Admission::Failed;s.counters.failed+=1;
                if let Ok(usage)=store.usage(){s.history=usage;}
                None
            }
        };
        drop(s);drop(gate);
        if let Some(revision)=history{(self.events)("history_changed",revision);}
        self.notify_status();true
    }
}

impl HostRuntime {
    pub fn start(data_dir:PathBuf,events:Events)->ContractResult<Self>{
        let instance=single_instance()?;
        Self::create(Target::File(data_dir.join("history.sqlite3")),data_dir.display().to_string(),events,true,Some(instance))
    }
    #[cfg(feature="test-fixtures")]
    pub fn for_manual_product(data_dir:PathBuf,product:PathBuf,events:Events)->ContractResult<Self>{Self::create(Target::Manual{path:data_dir.join("history.sqlite3"),product},data_dir.display().to_string(),events,true,Some(single_instance()?))}
    #[cfg(feature="test-fixtures")]
    pub fn for_test_memory(events:Events)->ContractResult<Self>{Self::create(Target::Memory,"D:\\Synthetic\\Pinfeather".into(),events,false,None)}
    #[cfg(feature="test-fixtures")]
    pub fn for_test_file(path:PathBuf,events:Events)->ContractResult<Self>{Self::create(Target::File(path.clone()),path.parent().unwrap().display().to_string(),events,false,None)}

    fn create(target:Target,data_dir:String,events:Events,production:bool,instance:Option<std::os::windows::io::OwnedHandle>)->ContractResult<Self>{
        if !RETAINED.lock().unwrap_or_else(|e|e.into_inner()).is_empty(){return Err(cleanup());}
        let state=Arc::new(Mutex::new(CaptureState::new(&uuid::Uuid::new_v4().simple().to_string())));
        let shared=Arc::new(SharedRuntime{state,gate:Mutex::new(()),wake:Condvar::new(),events,_instance:instance,listener_failed:std::sync::atomic::AtomicBool::new(false),cleanup_failed:std::sync::atomic::AtomicBool::new(false),native_writes:production,
            copy_busy:Arc::new(AtomicBool::new(false)),copy_stopping:AtomicBool::new(false),
            copy_owner:Mutex::new(CopyOwner{owner:None,borrowers:0}),owner_idle:Condvar::new(),
            closing:Arc::new(crate::runtime::CloseControl::default()),
            cleanup_errors:Mutex::new(Vec::new()),
            settings_file:Mutex::new(None),
            #[cfg(all(test,feature="test-fixtures"))]test_cleanup_attempt:Mutex::new(None),
            #[cfg(feature="test-fixtures")]test_writer:Mutex::new(None),
            #[cfg(feature="test-fixtures")]before_commit:Mutex::new(None),
            #[cfg(feature="test-fixtures")]before_policy:Mutex::new(None),
            #[cfg(feature="test-fixtures")]before_settings_replace:Mutex::new(None),
        });
        #[cfg(feature="test-fixtures")]let test_product=match &target{Target::Manual{product,..}=>Some(product.clone()),_=>None};
        let(tx,rx)=mpsc::sync_channel(16);let(ready_tx,ready_rx)=mpsc::sync_channel(1);let storage_shared=shared.clone();
        let storage=std::thread::Builder::new().name("history-storage".into()).spawn(move||{
            let settings_path=match &target{Target::File(path)=>Some(path.with_file_name("settings.json")),#[cfg(feature="test-fixtures")]Target::Manual{path,..}=>Some(path.with_file_name("settings.json")),#[cfg(feature="test-fixtures")]Target::Memory=>None};
            if let Some(path)=settings_path {
                let(file,loaded)=crate::config::SettingsFile::load(path);
                let mut state=storage_shared.state.lock().unwrap_or_else(|e|e.into_inner());
                match loaded{Ok(paths)=>state.exclusions=paths,Err(error)=>{state.settings_blocked=true;state.settings_error=Some(error.message);}}
                *storage_shared.settings_file.lock().unwrap_or_else(|e|e.into_inner())=Some(file);
            }
            let opened=match target{Target::File(path)=>Store::open(&path),#[cfg(feature="test-fixtures")]Target::Memory=>Store::open_in_memory(),#[cfg(feature="test-fixtures")]Target::Manual{path,..}=>Store::open(&path)};
            let mut store=opened.ok();
            {let mut s=storage_shared.state.lock().unwrap_or_else(|e|e.into_inner());if let Some(db)=store.as_ref(){match db.usage(){Ok(usage)=>{s.history=usage;s.storage_ready=true;},Err(_)=>{s.storage_failed=true;s.admission=Admission::Failed;}}}else{s.storage_failed=true;s.admission=Admission::Failed;}}
            let _=ready_tx.send(());
            storage_loop(&storage_shared,&mut store,rx);
            drop(store);
            storage_shared.notify_status();
        }).map_err(|_|cleanup())?;
        let runtime=Self{shared,queries:tx,storage:Mutex::new(Some(storage)),capture:Mutex::new(None),data_dir,production,exit_pending:std::sync::atomic::AtomicBool::new(false),exit_ready:std::sync::atomic::AtomicBool::new(false)};
        if ready_rx.recv_timeout(Duration::from_secs(3)).is_err(){return Err(cleanup());}
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
                    if restart{let result=pipeline.reset_worker();let mut s=shared.state.lock().unwrap_or_else(|e|e.into_inner());s.worker_service=pipeline.worker_service().into();if result.is_ok()&&!s.storage_failed&&!s.stopping&&s.user_enabled{s.admission=Admission::Active;}}
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
            }).map_err(|_|cleanup())?;
            *runtime.capture.lock().unwrap_or_else(|e|e.into_inner())=Some(capture);
            if ready_rx.recv_timeout(Duration::from_secs(3)).is_err(){return Err(cleanup());}
        }
        Ok(runtime)
    }

    pub fn status(&self)->StatusResult{
        let s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());let(items,bytes)=s.queue_usage();
        StatusResult{revision:s.revision.to_string(),state:if s.storage_failed||s.admission==Admission::Failed{"failed"}else if s.stopping{"stopping"}else{match s.admission{Admission::Active=>"listening",Admission::Overloaded=>"overloaded",_=>"paused"}}.into(),services:ServiceReadiness{listener:if self.shared.listener_failed.load(std::sync::atomic::Ordering::Acquire){"failed"}else if s.listener_ready{"ready"}else if s.stopping{"stopped"}else{"not_started"}.into(),worker:s.worker_service.clone(),storage:if s.storage_failed{"failed"}else if s.storage_ready{"ready"}else{"stopped"}.into(),hotkey:"not_started".into(),target_observer:"not_started".into()},capture:CaptureFlags{user_enabled:s.user_enabled,admission_open:s.admission==Admission::Active,policy_epoch:s.epoch},counters:CaptureCounters{observed:s.counters.observed.to_string(),captured:s.counters.captured.to_string(),persisted:s.counters.persisted.to_string(),ignored:s.counters.ignored.to_string(),superseded:s.counters.superseded.to_string(),failed:s.counters.failed.to_string(),overloaded:s.counters.overloaded.to_string(),cancelled:s.counters.cancelled.to_string()},queue:QueueUsage{items:items as i64,bytes:bytes as i64},history:s.history.clone(),data_dir:self.data_dir.clone(),versions:VersionsInfo{app:env!("CARGO_PKG_VERSION").into(),schema:1,worker_protocol:1},last_paste:None}
    }
    pub fn set_enabled(&self,enabled:bool)->ContractResult<StatusResult>{
        #[cfg(feature="test-fixtures")]if let Some(hook)=self.shared.before_policy.lock().unwrap_or_else(|e|e.into_inner()).clone(){hook();}
        let gate=self.shared.gate.lock().unwrap_or_else(|e|e.into_inner());
        let mut s=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if s.stopping||self.shared.closing.closing(){return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping"));}
        if enabled && s.settings_blocked{return Err(ContractError::new(ErrorCode::StorageUnavailable,"设置文件需要核对；收录已阻止，请退出后修正并重新启动。"));}
        if enabled && !s.storage_ready{return Err(unavailable());}
        if enabled && s.storage_failed{s.storage_retry=true;s.user_enabled=true;}
        else{
            if enabled && self.production && !s.listener_ready{return Err(cleanup());}
            let(count,bytes)=s.queue_usage();
            if enabled&&(s.history.items+count as i64>=MAX_HISTORY_ITEMS as i64||s.history.payload_bytes+bytes as i64>=MAX_HISTORY_PAYLOAD_BYTES as i64){return Err(ContractError::new(ErrorCode::CapacityExceeded,"history capacity reached"));}
            if enabled&&self.production&&s.worker_service=="failed"{s.user_enabled=true;s.restart_worker=true;}else{s.set_enabled(enabled);}
        }
        drop(s);drop(gate);self.shared.notify_status();Ok(self.status())
    }
    pub fn manage_history(&self,request:HistoryManagement)->ContractResult<HistoryManagementResult>{
        if let HistoryManagement::Delete{clip_id,..}=&request {
            parse_copy_clip_text_request(&serde_json::json!({"request_id":"manage","clip_id":clip_id}))?;
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
    /// Internal Rust entry only; the WebView has no arbitrary write command.
    pub fn write_stored_text(&self,clip_id:i64)->ContractResult<u64>{
        if clip_id<=0{return Err(ContractError::invalid_input("invalid clip id"));}
        self.copy_with_budget(clip_id,Instant::now()+Duration::from_secs(3),Duration::from_secs(3)).map_err(|failure|failure.error)
    }
    fn queue_copy(&self,id:i64,deadline:Instant)->Result<Receiver<WriteReply>,WriteFailure>{
        if self.shared.copy_stopping.load(Ordering::Acquire){return Err(ContractError::new(ErrorCode::Busy,"runtime is stopping").into());}
        if self.shared.copy_busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return Err(ContractError::new(ErrorCode::Busy,"clipboard write is pending").into());}
        let permit=WritePermit(self.shared.copy_busy.clone());
        let(tx,rx)=mpsc::sync_channel(1);
        self.queries.try_send(Query::Copy(id,deadline,permit,tx)).map_err(|_|WriteFailure::from(ContractError::new(ErrorCode::Busy,"history query queue is full")))?;
        self.shared.wake.notify_all();Ok(rx)
    }
    fn copy_with_budget(&self,id:i64,deadline:Instant,wait:Duration)->WriteReply{
        self.queue_copy(id,deadline)?.recv_timeout(wait).unwrap_or_else(|_|Err(WriteFailure{error:ContractError::new(ErrorCode::Timeout,"clipboard write result is unknown"),may_have_written:true}))
    }
    pub fn copy_clip_text(&self,request:CopyClipTextRequest)->ContractResult<CopyClipTextResult>{
        let request=parse_copy_clip_text_request(&serde_json::json!({"request_id":request.request_id,"clip_id":request.clip_id}))?;
        let result=self.copy_with_budget(positive_id_as_i64(&request.clip_id),Instant::now()+Duration::from_secs(3),Duration::from_secs(3));
        Ok(copy_result(&request,result))
    }
    pub fn copy_panel_text(&self,panel:&crate::manual_panel::PanelSessions,session:&str,request:CopyClipTextRequest)->ContractResult<CopyClipTextResult>{
        let request=parse_copy_clip_text_request(&serde_json::json!({"request_id":request.request_id,"clip_id":request.clip_id}))?;
        // Linearization point: hide/new-session cannot pass between validation and try_send.
        let admitted=panel.admit(session,||self.queue_copy(positive_id_as_i64(&request.clip_id),Instant::now()+Duration::from_secs(3)));
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
    pub fn expired_test_copy(&self,id:i64)->ContractResult<u64>{self.copy_with_budget(id,Instant::now()-Duration::from_secs(1),Duration::from_secs(2)).map_err(|failure|failure.error)}
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
        while state.settings_saving {
            let remaining=deadline.saturating_duration_since(Instant::now());if remaining.is_zero(){break;}
            state=self.shared.wake.wait_timeout(state,remaining).unwrap_or_else(|e|e.into_inner()).0;
        }
        let settings_pending=state.settings_saving;drop(state);
        let capture=finish_thread(&self.capture,deadline,&self.shared.cleanup_failed);
        let storage=finish_thread(&self.storage,deadline,&self.shared.cleanup_failed);
        let terminal=self.shared.cleanup_failed.load(Ordering::Acquire);
        if capture.is_err()||storage.is_err()||terminal||settings_pending{
            let mut errors=self.shared.cleanup_errors.lock().unwrap_or_else(|e|e.into_inner()).clone();
            if capture.is_err(){errors.push("抓取资源仍在回收或已失败".into());}if storage.is_err(){errors.push("存储资源仍在回收或已失败".into());}
            if settings_pending{errors.push("设置保存仍在结束；尚不能退出".into());}
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

fn execute_copy(shared:&SharedRuntime,store:&Option<Store>,id:i64,deadline:Instant)->WriteReply{
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
    let clip=store.as_ref().ok_or_else(unavailable)?.get(id)?.ok_or_else(||ContractError::new(ErrorCode::NotFound,"clip not found"))?;
    let text=clip.snapshot.payloads.iter().find(|p|p.format_name=="CF_UNICODETEXT").ok_or_else(||ContractError::invalid_input("clip has no Unicode text"))?;
    crate::clipboard::capture::validate_payloads(&[text.clone()]).map_err(|_|ContractError::invalid_input("invalid clipboard text"))?;
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
    let attempt=crate::clipboard::origin::write_registered_text(writer.as_mut(),owner.owner,counter,&marker,&text.data,before_write);
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
impl Drop for HostRuntime {fn drop(&mut self){if !self.shared.closing.closing(){let _=self.shutdown(Duration::from_secs(3));}for slot in [&self.capture,&self.storage]{if let Some(thread)=slot.lock().unwrap_or_else(|e|e.into_inner()).take(){RETAINED.lock().unwrap_or_else(|e|e.into_inner()).push(thread);}}}}

fn manage_on_storage(shared:&SharedRuntime,store:&mut Option<Store>,request:HistoryManagement,directory:&std::path::Path,deadline:Instant)->ContractResult<HistoryManagementResult>{
    let _gate=shared.gate.lock().unwrap_or_else(|e|e.into_inner());
    let mut state=shared.state.lock().unwrap_or_else(|e|e.into_inner());
    if Instant::now()>deadline || state.stopping || shared.closing.closing() || state.user_enabled || state.queue_usage().0!=0 || state.storage_failed {
        return Err(ContractError::new(ErrorCode::Busy,"pause capture and wait for an empty queue before managing history"));
    }
    let db=store.as_mut().ok_or_else(unavailable)?;
    let usage=db.usage()?;
    let mut backup_path=None;
    let deleted=match request {
        HistoryManagement::Delete{clip_id,expected_items}=>{
            if usage.items as u64!=expected_items{return Err(ContractError::new(ErrorCode::Busy,"history changed; refresh before confirming"));}
            db.delete(positive_id_as_i64(&clip_id))? as u64
        },
        HistoryManagement::Clear{expected_items}=>{
            if usage.items as u64!=expected_items{return Err(ContractError::new(ErrorCode::Busy,"history changed; refresh before confirming"));}
            db.clear_history()?
        },
        HistoryManagement::Backup{}=>{
            let backups=directory.join("backups");std::fs::create_dir_all(&backups).map_err(|_|unavailable())?;
            let path=backups.join(format!("history-{}.sqlite3",uuid::Uuid::new_v4()));db.backup_to(&path)?;backup_path=Some(path.display().to_string());0
        },
    };
    state.history=db.usage()?;
    Ok(HistoryManagementResult{items:state.history.items as u64,deleted,backup_path})
}

fn storage_loop(shared:&SharedRuntime,store:&mut Option<Store>,queries:Receiver<Query>){
    loop {
        for _ in 0..4 {match queries.try_recv(){
            Ok(Query::Manage(request,directory,deadline,reply))=>{
                let result=manage_on_storage(shared,store,request,&directory,deadline);
                let changed=result.as_ref().is_ok_and(|r|r.deleted>0);
                let _=reply.send(result);
                if changed {let revision={let mut s=shared.state.lock().unwrap_or_else(|e|e.into_inner());s.history_revision+=1;s.history_revision};(shared.events)("history_changed",revision);}
                shared.notify_status();
            }
            Ok(Query::List(request,reply))=>{let _=reply.send(store.as_ref().ok_or_else(unavailable).and_then(|s|s.list_recent(&request)));}
            Ok(Query::Text(request,reply))=>{let _=reply.send(store.as_ref().ok_or_else(unavailable).and_then(|s|s.get_clip_text(&request)));}
            Ok(Query::Copy(id,deadline,permit,reply))=>{
                let result=execute_copy(shared,store,id,deadline);
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
