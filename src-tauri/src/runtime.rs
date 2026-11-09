//! E05 admission and bounded queues. Storage/IPC assembly belongs to E06.
use std::collections::VecDeque;
use crate::{clipboard::{protocol::{WorkerMessage,ValidatedSnapshot},capture::CaptureResult},contracts::{MAX_QUEUE_ITEMS,MAX_QUEUE_BYTES}};

pub const MAX_NOTIFICATIONS: usize=256;

/// One capture owner observes every close attempt; closing never admits work again.
#[derive(Default)]
pub struct CloseControl {
    closing:std::sync::atomic::AtomicBool,
    attempt:std::sync::Mutex<(u64,Option<std::time::Instant>)>,
    changed:std::sync::Condvar,
}
impl CloseControl {
    pub fn publish(&self,deadline:std::time::Instant)->u64 {
        let mut attempt=self.attempt.lock().unwrap_or_else(|e|e.into_inner());
        self.closing.store(true,std::sync::atomic::Ordering::Release);
        attempt.0+=1;attempt.1=Some(deadline);self.changed.notify_all();attempt.0
    }
    pub fn closing(&self)->bool{self.closing.load(std::sync::atomic::Ordering::Acquire)}
    pub fn current(&self)->(u64,Option<std::time::Instant>){*self.attempt.lock().unwrap_or_else(|e|e.into_inner())}
    pub fn deadline(&self,normal:std::time::Instant)->std::time::Instant{self.current().1.unwrap_or(normal)}
    pub fn next(&self,previous:u64)->(u64,std::time::Instant){
        let mut attempt=self.attempt.lock().unwrap_or_else(|e|e.into_inner());
        while attempt.0==previous{attempt=self.changed.wait(attempt).unwrap_or_else(|e|e.into_inner());}
        (attempt.0,attempt.1.expect("published close deadline"))
    }
}

use crate::contracts::windows_path_equal;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Admission {UserPaused,Active,Overloaded,Failed,Stopped}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Notice {NotificationOverrun,SnapshotQueueFull,HistoryCapacity,UnknownOwner,ExcludedOwner,ZeroSequence,InvalidSnapshot,WorkerFailed,ListenerStopped,Capture(crate::clipboard::capture::CaptureReason)}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(crate) enum ApplicationSkip {UnknownOwner,ExcludedOwner}
#[derive(Debug,Default)]
pub struct Counters {pub observed:u64,pub captured:u64,pub persisted:u64,pub ignored:u64,pub superseded:u64,pub failed:u64,pub overloaded:u64,pub cancelled:u64}
#[derive(Debug,Clone)]
pub struct CaptureTicket {pub message:WorkerMessage,pub sequence:u32,pub observed_at_ms:u64}
pub struct QueuedSnapshot {pub ticket:CaptureTicket,pub snapshot:ValidatedSnapshot,pub captured_at_ms:i64}
pub struct CaptureState {
    pub admission:Admission,pub counters:Counters,pub epoch:i64,pub notice:Option<Notice>,
    pub user_enabled:bool,pub history:crate::contracts::HistoryUsage,pub storage_inflight:(usize,u64),
    pub storage_ready:bool,pub storage_failed:bool,pub storage_retry:bool,pub listener_ready:bool,pub worker_service:String,
    pub stopping:bool,pub revision:u64,pub history_revision:u64,pub origins:crate::clipboard::origin::OriginRegistry,pub internal_ignored:u64,pub restart_worker:bool,
    pub settings_saving:bool,pub settings_blocked:bool,pub settings_error:Option<String>,
    last_application_skip:Option<ApplicationSkip>,
    instance:String,next:u64,pending:VecDeque<CaptureTicket>,last:Option<(u32,String)>,
    in_flight:Option<String>,pub(crate) queue:VecDeque<QueuedSnapshot>,pub(crate) bytes:u64,pub(crate) exclusions:Vec<String>,acquired:Option<(u32,String)>,
}
impl CaptureState {
    pub fn new(instance:&str)->Self{
        assert!(!instance.is_empty()&&instance.len()<=40&&instance.bytes().all(|b|b.is_ascii_graphic()));
        Self{admission:Admission::UserPaused,counters:Counters::default(),epoch:0,notice:None,user_enabled:false,history:crate::contracts::HistoryUsage{items:0,payload_bytes:0},storage_inflight:(0,0),storage_ready:false,storage_failed:false,storage_retry:false,listener_ready:false,worker_service:"not_started".into(),stopping:false,revision:0,history_revision:0,origins:crate::clipboard::origin::OriginRegistry::new(uuid::Uuid::parse_str(instance).unwrap_or_else(|_|uuid::Uuid::new_v4()).into_bytes(),0),internal_ignored:0,restart_worker:false,settings_saving:false,settings_blocked:false,settings_error:None,last_application_skip:None,instance:instance.into(),next:0,pending:VecDeque::new(),last:None,in_flight:None,queue:VecDeque::new(),bytes:0,exclusions:vec![],acquired:None}
    }
    pub fn set_enabled(&mut self,enabled:bool){
        if self.admission==Admission::Stopped{return;}
        self.user_enabled=enabled;
        if enabled && self.admission==Admission::Overloaded {
            if self.queue.len() < MAX_QUEUE_ITEMS as usize && self.bytes < MAX_QUEUE_BYTES {
                self.admission=Admission::Active;
                self.notice=None;
            }
            return;
        }
        if (enabled && self.admission==Admission::Active)||(!enabled&&self.admission==Admission::UserPaused){return;}
        self.invalidate_policy();
        self.admission=if enabled{Admission::Active}else{Admission::UserPaused};
    }
    fn invalidate_policy(&mut self){
        self.epoch=self.epoch.checked_add(1).filter(|e|*e<=crate::contracts::JS_SAFE_INTEGER_MAX).expect("policy epoch exhausted");
        self.counters.cancelled+=self.pending.len() as u64+self.queue.len() as u64;
        self.pending.clear();self.queue.clear();self.bytes=0;self.last=None;self.acquired=None;self.notice=None;
    }
    pub fn set_exclusions(&mut self,paths:Vec<String>){self.invalidate_policy();self.exclusions=paths;}
    pub fn observe(&mut self,sequence:u32,at:u64)->Option<String>{
        if self.admission!=Admission::Active{return None;}
        self.revision+=1;
        self.counters.observed+=1;
        if sequence==0 {self.counters.failed+=1;self.last=None;self.acquired=None;self.notice=Some(Notice::ZeroSequence);return None;}
        if let Some((seq,id))=&self.last {if *seq==sequence{return Some(id.clone());}}
        // A sequence value is not a permanent identifier across transitions.
        self.acquired=None;
        if self.pending.len()>=MAX_NOTIFICATIONS {self.counters.overloaded+=1;self.admission=Admission::Overloaded;self.notice=Some(Notice::NotificationOverrun);return None;}
        self.next=self.next.checked_add(1).expect("capture identity exhausted");
        let id=format!("{}-{}",self.instance,self.next);
        self.pending.push_back(CaptureTicket{message:WorkerMessage{version:1,capture_id:id.clone(),policy_epoch:self.epoch},sequence,observed_at_ms:at});
        self.last=Some((sequence,id.clone()));Some(id)
    }
    pub fn notification_count(&self)->usize{self.pending.len()}
    pub fn queue_usage(&self)->(usize,u64){(self.queue.len()+self.storage_inflight.0,self.bytes+self.storage_inflight.1)}
    pub(crate) fn last_application_skip(&self)->Option<ApplicationSkip>{self.last_application_skip}
    pub(crate) fn owner_allowed(&mut self,source:Option<&str>)->bool{
        let owner_path=source.and_then(|p|crate::contracts::normalize_windows_exe_path(p).ok());
        if !self.exclusions.is_empty()&&owner_path.as_ref().is_none_or(|path|self.exclusions.iter().any(|excluded|windows_path_equal(path,excluded))){
            let reason=if owner_path.is_none(){ApplicationSkip::UnknownOwner}else{ApplicationSkip::ExcludedOwner};
            // Application feedback outlives transient notices and policy invalidation.
            self.last_application_skip=Some(reason);
            self.notice=Some(match reason{ApplicationSkip::UnknownOwner=>Notice::UnknownOwner,ApplicationSkip::ExcludedOwner=>Notice::ExcludedOwner});false
        }else{true}
    }
    pub fn begin_current(&mut self,sequence:u32)->Option<CaptureTicket>{
        if self.admission!=Admission::Active || self.in_flight.is_some(){return None;}
        if sequence==0 {self.counters.failed+=self.pending.len() as u64;self.pending.clear();self.last=None;self.notice=Some(Notice::ZeroSequence);return None;}
        let mut candidate=None;
        let mut covered=None;
        while let Some(ticket)=self.pending.pop_front(){
            if ticket.sequence==sequence {if candidate.replace(ticket).is_some(){self.counters.superseded+=1;}}else{self.counters.superseded+=1;covered=Some(ticket);}
        }
        if let Some((seq,id))=&self.acquired {
            if *seq==sequence {self.last=Some((*seq,id.clone()));return None;}
        }
        // If the latest state has no delivered notification yet, retain the
        // request identity while recording its covered observation separately.
        let mut ticket=candidate.or(covered)?;ticket.sequence=sequence;
        self.last=Some((sequence,ticket.message.capture_id.clone()));
        self.in_flight=Some(ticket.message.capture_id.clone());Some(ticket)
    }
    pub fn complete(&mut self,ticket:CaptureTicket,response:ValidatedSnapshot){
        if self.in_flight.as_ref()!=Some(&ticket.message.capture_id){self.counters.failed+=1;return;}
        self.in_flight=None;
        if self.stopping || ticket.message.policy_epoch!=self.epoch || matches!(self.admission,Admission::UserPaused|Admission::Stopped){self.counters.cancelled+=1;return;}
        if response.message()!=ticket.message {self.counters.failed+=1;self.admission=Admission::Failed;self.notice=Some(Notice::InvalidSnapshot);return;}
        let owner=response.response.owner.hwnd.as_ref().and_then(|s|s.parse::<usize>().ok());
        if self.origins.classify(response.internal_origin.as_deref(),owner,response.response.sequence_after)==crate::clipboard::origin::Classification::Internal {self.counters.ignored+=1;self.internal_ignored+=1;return;}
        if response.response.result!=CaptureResult::Failed && self.last.as_ref().is_none_or(|(seq,_)|*seq==response.response.sequence_after) {
            self.acquired=Some((response.response.sequence_after,ticket.message.capture_id.clone()));
        }
        match response.response.result {
            CaptureResult::Ignored=>{self.counters.ignored+=1;self.notice=response.response.reason.map(Notice::Capture);return;}
            CaptureResult::Failed=>{self.counters.failed+=1;self.notice=response.response.reason.map(Notice::Capture);return;}
            CaptureResult::Captured=>self.counters.captured+=1,
        }
        if ticket.sequence!=response.response.sequence_after {self.counters.superseded+=1;}
        if !self.owner_allowed(response.response.owner.process.as_deref()) {
            self.counters.ignored+=1;
            return;
        }
        let bytes=response.payloads.iter().map(|p|p.data.len() as u64).sum::<u64>();
        let (reserved,reserved_bytes)=self.queue_usage();
        if self.history.items as u64+reserved as u64>=crate::contracts::MAX_HISTORY_ITEMS || (self.history.payload_bytes as u64).saturating_add(reserved_bytes).saturating_add(bytes)>crate::contracts::MAX_HISTORY_PAYLOAD_BYTES {
            self.counters.overloaded+=1;self.admission=Admission::Overloaded;self.notice=Some(Notice::HistoryCapacity);return;
        }
        if reserved>=MAX_QUEUE_ITEMS as usize || reserved_bytes.saturating_add(bytes)>MAX_QUEUE_BYTES {
            self.counters.overloaded+=1;self.admission=Admission::Overloaded;self.notice=Some(Notice::SnapshotQueueFull);return;
        }
        let seq=response.response.sequence_after;
        // Notifications of the version just acquired refer to this same capture.
        self.pending.retain(|t|t.sequence!=seq);
        if let Some((last_seq,last_id))=&mut self.last {if *last_seq==seq {*last_id=ticket.message.capture_id.clone();}}
        let captured_at_ms=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis().min(crate::contracts::JS_SAFE_INTEGER_MAX as u128) as i64;
        self.bytes+=bytes;self.queue.push_back(QueuedSnapshot{ticket,snapshot:response,captured_at_ms});
    }
    pub fn pop_queued(&mut self)->Option<QueuedSnapshot>{let item=self.queue.pop_front()?;self.bytes-=item.snapshot.payloads.iter().map(|p|p.data.len() as u64).sum::<u64>();Some(item)}
    pub fn capture_failed(&mut self,ticket:&CaptureTicket,terminal:bool){
        if self.in_flight.as_ref()!=Some(&ticket.message.capture_id){return;}
        self.in_flight=None;
        if self.stopping || ticket.message.policy_epoch!=self.epoch || matches!(self.admission,Admission::UserPaused|Admission::Stopped){self.counters.cancelled+=1;}
        else{self.counters.failed+=1;self.notice=Some(Notice::WorkerFailed);}
        if terminal&&!self.stopping{self.admission=Admission::Failed;self.notice=Some(Notice::WorkerFailed);}
    }
    pub fn stop(&mut self){self.set_enabled(false);self.admission=Admission::Stopped;}
    pub fn stop_observing(&mut self){self.stopping=true;self.admission=Admission::Stopped;self.counters.cancelled+=self.pending.len() as u64;self.pending.clear();}
}

/// Owns the listener and supervisor. E06 calls poll_capture on its background
/// runtime thread and consumes pop_queued; the message thread never waits on IO.
pub struct CapturePipeline {
    state:std::sync::Arc<std::sync::Mutex<CaptureState>>,
    listener:crate::clipboard::listener::ClipboardListener,
    worker:crate::clipboard::worker::WorkerSupervisor,
    closing:std::sync::Arc<CloseControl>,
    close_attempted:bool,
    #[cfg(feature="test-fixtures")]
    test_product:Option<std::path::PathBuf>,
}
impl CapturePipeline {
    #[cfg(all(test,feature="test-fixtures"))]
    pub(crate) fn m2_synthetic(state:std::sync::Arc<std::sync::Mutex<CaptureState>>,listener:crate::clipboard::listener::ClipboardListener,mut worker:crate::clipboard::worker::WorkerSupervisor,closing:std::sync::Arc<CloseControl>)->Self{
        worker.set_close_control(closing.clone());Self{state,listener,worker,closing,close_attempted:false,test_product:None}
    }
    pub fn start_paused()->crate::contracts::ContractResult<Self>{
        let state=std::sync::Arc::new(std::sync::Mutex::new(CaptureState::new(&uuid::Uuid::new_v4().simple().to_string())));
        Self::start_shared(state,std::sync::Arc::new(||{}))
    }
    pub fn start_shared(state:std::sync::Arc<std::sync::Mutex<CaptureState>>,wake:std::sync::Arc<dyn Fn()+Send+Sync>)->crate::contracts::ContractResult<Self>{
        Self::start_controlled(state,wake,std::sync::Arc::new(CloseControl::default()))
    }
    pub fn start_controlled(state:std::sync::Arc<std::sync::Mutex<CaptureState>>,wake:std::sync::Arc<dyn Fn()+Send+Sync>,closing:std::sync::Arc<CloseControl>)->crate::contracts::ContractResult<Self>{
        let observed=state.clone();
        let listener=crate::clipboard::listener::ClipboardListener::start(std::sync::Arc::new(move|seq,at|{observed.lock().unwrap_or_else(|e|e.into_inner()).observe(seq,at);wake();}))?;
        {let mut s=state.lock().unwrap_or_else(|e|e.into_inner());s.origins.set_owner(listener.owner_hwnd());s.listener_ready=true;}
        let mut worker=crate::clipboard::worker::WorkerSupervisor::for_product();worker.set_close_control(closing.clone());
        Ok(Self{state,listener,worker,closing,close_attempted:false,#[cfg(feature="test-fixtures")]test_product:None})
    }
    pub fn state(&self)->&std::sync::Arc<std::sync::Mutex<CaptureState>>{&self.state}
    pub fn worker_service(&self)->&'static str{if self.worker.state()==crate::clipboard::worker::WorkerState::FailedPaused{"failed"}else if self.worker.current_identity().is_some(){"ready"}else{"not_started"}}
    pub fn listener_healthy(&self)->bool{self.listener.healthy()}
    #[cfg(feature="test-fixtures")]
    pub fn use_test_product(&mut self,exe:std::path::PathBuf){self.worker.close();self.worker=crate::clipboard::worker::WorkerSupervisor::for_test_product(exe.clone());self.worker.set_close_control(self.closing.clone());self.test_product=Some(exe);}
    pub fn reset_worker(&mut self)->crate::contracts::ContractResult<()>{
        if self.closing.closing(){return Err(crate::contracts::ContractError::new(crate::contracts::ErrorCode::Busy,"capture closing"));}
        self.worker.close();
        if self.worker.last_reap().is_some_and(|r|r.cleanup_failed){return Err(crate::contracts::ContractError::new(crate::contracts::ErrorCode::CleanupFailed,"worker resources are still in use"));}
        if self.closing.closing(){return Err(crate::contracts::ContractError::new(crate::contracts::ErrorCode::Busy,"capture closing"));}
        self.worker=crate::clipboard::worker::WorkerSupervisor::for_product();
        #[cfg(feature="test-fixtures")]if let Some(exe)=&self.test_product{self.worker=crate::clipboard::worker::WorkerSupervisor::for_test_product(exe.clone());}
        self.worker.set_close_control(self.closing.clone());
        if self.worker.state()==crate::clipboard::worker::WorkerState::FailedPaused{return Err(crate::contracts::ContractError::new(crate::contracts::ErrorCode::CleanupFailed,"worker resources are still in use"));}Ok(())
    }
    pub fn poll_capture(&mut self)->crate::contracts::ContractResult<bool>{
        if self.closing.closing(){return Ok(false);}
        if !self.listener.healthy(){let mut state=self.state.lock().unwrap_or_else(|e|e.into_inner());state.admission=Admission::Failed;state.notice=Some(Notice::ListenerStopped);return Err(crate::contracts::ContractError::new(crate::contracts::ErrorCode::CleanupFailed,"clipboard listener stopped"));}
        let ticket={
            let mut state=self.state.lock().unwrap_or_else(|e|e.into_inner());
            if state.admission!=Admission::Active || state.notification_count()==0 {return Ok(false);}
            let sequence=unsafe{windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber()};
            state.begin_current(sequence)
        };
        let Some(ticket)=ticket else{return Ok(false);};
        // Release the state lock while the worker renders/reads; notifications
        // continue to accumulate independently up to their own fixed bound.
        match self.worker.capture_snapshot(&ticket.message) {
            Ok(snapshot)=>{let mut state=self.state.lock().unwrap_or_else(|e|e.into_inner());state.complete(ticket,snapshot);if self.worker.state()==crate::clipboard::worker::WorkerState::FailedPaused{state.admission=Admission::Failed;state.notice=Some(Notice::WorkerFailed);}Ok(true)}
            Err(error)=>{self.state.lock().unwrap_or_else(|e|e.into_inner()).capture_failed(&ticket,self.worker.state()==crate::clipboard::worker::WorkerState::FailedPaused);Err(error)}
        }
    }
    pub fn close(&mut self)->crate::contracts::ContractResult<()> {
        self.close_until(std::time::Instant::now()+std::time::Duration::from_secs(2))
    }
    pub fn cancel_worker_until(&mut self,deadline:std::time::Instant)->crate::contracts::ContractResult<()>{self.worker.close_until(deadline)}
    pub fn cleanup_pending(&self)->bool{self.listener.cleanup_pending()||self.worker.current_identity().is_some()}
    #[cfg(all(test,feature="test-fixtures"))]
    pub(crate) fn m2_listener_pending(&self)->bool{self.listener.m2_untouched()}
    pub fn close_until(&mut self,deadline:std::time::Instant)->crate::contracts::ContractResult<()> {
        self.close_attempted=true;
        self.state.lock().unwrap_or_else(|e|e.into_inner()).stop_observing();
        let worker=self.worker.close_until(deadline);let listener=self.listener.close_until(deadline);
        match (worker,listener){(Err(a),Err(b))=>Err(crate::contracts::ContractError::new(if a.code==crate::contracts::ErrorCode::CleanupFailed||b.code==crate::contracts::ErrorCode::CleanupFailed{crate::contracts::ErrorCode::CleanupFailed}else{crate::contracts::ErrorCode::Timeout},&format!("{}; {}",a.message,b.message))),(Err(e),_)|(_,Err(e))=>Err(e),_=>Ok(())}
    }
}
impl Drop for CapturePipeline {fn drop(&mut self){if !self.close_attempted{let _=self.close();}}}

#[cfg(all(test,feature="test-fixtures"))]
mod m2_tests {
    use super::*;use std::{sync::{Arc,Mutex,mpsc},time::{Instant,Duration}};
    #[test]
    fn m2_production_pipeline_propagates_one_deadline_to_both_retained_owners(){
        let(release,gate)=mpsc::channel();let listener=crate::clipboard::listener::ClipboardListener::m2_synthetic(std::thread::spawn(move||{gate.recv_timeout(Duration::from_secs(5)).unwrap();}));
        let worker=crate::clipboard::worker::WorkerSupervisor::m2_synthetic(&[258,0]);let identity=worker.current_identity();let control=Arc::new(CloseControl::default());
        let mut pipeline=CapturePipeline::m2_synthetic(Arc::new(Mutex::new(CaptureState::new("m2-synthetic"))),listener,worker,control.clone());let expired=Instant::now();control.publish(expired);
        assert_eq!(pipeline.close_until(expired).unwrap_err().code,crate::contracts::ErrorCode::Timeout);assert_eq!(pipeline.listener.m2_deadline(),Some(expired));assert_eq!(pipeline.worker.m2_deadline(),Some(expired));assert_eq!(pipeline.worker.current_identity(),identity);assert!(pipeline.cleanup_pending());assert!(pipeline.reset_worker().is_err());
        release.send(()).unwrap();let retry=Instant::now()+Duration::from_secs(1);control.publish(retry);pipeline.close_until(retry).unwrap();assert_eq!(pipeline.listener.m2_deadline(),Some(retry));assert_eq!(pipeline.worker.m2_deadline(),Some(retry));assert!(!pipeline.cleanup_pending());
    }
}
