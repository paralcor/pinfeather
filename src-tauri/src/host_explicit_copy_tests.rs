//! Only synthetic handles and memory SQLite. The unit under test is the host queue.
use super::*;
use crate::clipboard::{capture::*,origin::Classification,protocol::WorkerMessage};
use base64::{Engine,engine::general_purpose::STANDARD};
use std::{collections::BTreeMap,sync::atomic::{AtomicUsize,AtomicU32}};

#[derive(Default)]
struct Written { calls:usize,created:usize,next:usize,owned:BTreeMap<usize,Vec<u8>>,transferred:Vec<(String,Vec<u8>)>,freed:usize,closed:usize,dropped:usize }
struct Control { fail:AtomicUsize,block:AtomicUsize,sequence:AtomicU32,entered:mpsc::SyncSender<()>,release:Mutex<mpsc::Receiver<()>>,written:Mutex<Written>,owner_alive:Arc<AtomicBool>,owner_violation:AtomicBool }
struct MockWriter {control:Arc<Control>,step:usize}
impl MockWriter {
    fn check(&mut self)->ContractResult<()> {
        if !self.control.owner_alive.load(Ordering::Acquire){self.control.owner_violation.store(true,Ordering::Release);}
        self.step+=1;self.control.written.lock().unwrap().calls+=1;
        if self.control.block.load(Ordering::Acquire)==self.step {
            self.control.entered.send(()).unwrap();
            self.control.release.lock().unwrap().recv_timeout(Duration::from_secs(8)).expect("release synthetic write barrier");
        }
        if !self.control.owner_alive.load(Ordering::Acquire){self.control.owner_violation.store(true,Ordering::Release);}
        if self.control.fail.load(Ordering::Acquire)==self.step {Err(ContractError::new(ErrorCode::InjectionFailed,"synthetic write failure"))}else{Ok(())}
    }
}
impl WriteApi for MockWriter {
    fn open(&mut self,owner:usize)->ContractResult<()>{
        assert_eq!(owner,99);
        if self.control.block.load(Ordering::Acquire)==13{self.control.entered.send(()).unwrap();self.control.release.lock().unwrap().recv_timeout(Duration::from_secs(8)).unwrap();}
        self.check()
    }
    fn empty(&mut self)->ContractResult<()>{self.check()}
    fn allocate(&mut self,bytes:&[u8])->ContractResult<usize>{self.check()?;let mut w=self.control.written.lock().unwrap();w.next+=1;let id=w.next;w.owned.insert(id,bytes.to_vec());Ok(id)}
    fn set(&mut self,name:&str,handle:usize)->ContractResult<()>{self.check()?;let mut w=self.control.written.lock().unwrap();let bytes=w.owned.remove(&handle).expect("transfer an owned handle once");w.transferred.push((name.into(),bytes));Ok(())}
    fn free(&mut self,handle:usize){let mut w=self.control.written.lock().unwrap();assert!(w.owned.remove(&handle).is_some());w.freed+=1;}
    fn close(&mut self)->ContractResult<()>{self.control.written.lock().unwrap().closed+=1;self.check()}
    fn sequence(&mut self)->u32{self.control.sequence.load(Ordering::Acquire)}
}
impl Drop for MockWriter {
    fn drop(&mut self){
        if self.control.block.load(Ordering::Acquire)==12{self.control.entered.send(()).unwrap();self.control.release.lock().unwrap().recv_timeout(Duration::from_secs(8)).unwrap();}
        if !self.control.owner_alive.load(Ordering::Acquire){self.control.owner_violation.store(true,Ordering::Release);}
        self.control.written.lock().unwrap().dropped+=1;
    }
}
struct Fixture {runtime:Arc<HostRuntime>,control:Arc<Control>,entered:mpsc::Receiver<()>,release:mpsc::SyncSender<()>,committed:mpsc::Receiver<()>}
impl Drop for Fixture {fn drop(&mut self){let _=self.release.try_send(());let _=self.runtime.shutdown(Duration::from_secs(2));}}
fn fixture(formats:Vec<WireFormat>)->Fixture {
    let(tx,rx)=mpsc::sync_channel(8);
    let runtime=Arc::new(HostRuntime::for_test_memory(Arc::new(move|name,_|{if name=="history_changed"{let _=tx.try_send(());}})).unwrap());
    let(entered_tx,entered)=mpsc::sync_channel(1);let(release,release_rx)=mpsc::sync_channel(1);
    let control=Arc::new(Control{fail:AtomicUsize::new(0),block:AtomicUsize::new(0),sequence:AtomicU32::new(50),entered:entered_tx,release:Mutex::new(release_rx),written:Mutex::new(Written::default()),owner_alive:Arc::new(AtomicBool::new(true)),owner_violation:AtomicBool::new(false)});
    let c=control.clone();*runtime.shared.test_writer.lock().unwrap()=Some(Arc::new(move||{c.written.lock().unwrap().created+=1;Ok(Box::new(MockWriter{control:c.clone(),step:0}))}));
    runtime.shared.state.lock().unwrap().origins.set_owner(99);
    runtime.shared.publish_copy_owner(99);
    runtime.set_enabled(true).unwrap();runtime.inject_response(1,response(formats)).unwrap();rx.recv_timeout(Duration::from_secs(2)).unwrap();runtime.set_enabled(false).unwrap();
    Fixture{runtime,control,entered,release,committed:rx}
}
fn text(text:&str)->WireFormat{WireFormat{name:"CF_UNICODETEXT".into(),data:STANDARD.encode(text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>())}}
fn response(formats:Vec<WireFormat>)->CaptureResponse {
    let mut r=CaptureResponse::empty(&WorkerMessage{version:1,capture_id:"synthetic".into(),policy_epoch:0});
    r.result=CaptureResult::Captured;r.reason=None;r.attempts=1;r.sequence_before=1;r.sequence_locked=1;r.sequence_after=1;r.formats=formats;r
}
fn request()->CopyClipTextRequest{CopyClipTextRequest{request_id:"copy-1".into(),clip_id:"1".into()}}
fn copied(f:&Fixture)->CopyClipTextResult{f.runtime.copy_clip_text(request()).unwrap()}
fn list()->ListClipsRequest{parse_list_clips_request(&serde_json::json!({"request_id":"list","query":""})).unwrap()}
fn assert_outcome(result:CopyClipTextResult,outcome:CopyOutcome,reason:Option<ErrorCode>){assert_eq!(result,CopyClipTextResult::new(&request(),outcome,reason));}

struct PauseCommit {shared:Arc<SharedRuntime>,release:Option<mpsc::SyncSender<()>>}
impl Drop for PauseCommit {fn drop(&mut self){if let Some(tx)=self.release.take(){let _=tx.try_send(());}*self.shared.before_commit.lock().unwrap_or_else(|e|e.into_inner())=None;}}
fn pause_commit(f:&Fixture)->PauseCommit {
    let(entered_tx,entered)=mpsc::sync_channel(1);let(release,release_rx)=mpsc::sync_channel(1);let release_rx=Mutex::new(release_rx);
    *f.runtime.shared.before_commit.lock().unwrap()=Some(Arc::new(move||{entered_tx.send(()).unwrap();release_rx.lock().unwrap().recv_timeout(Duration::from_secs(8)).unwrap();}));
    let pause=PauseCommit{shared:f.runtime.shared.clone(),release:Some(release)};
    f.runtime.set_enabled(true).unwrap();f.runtime.inject_response(2,response(vec![text("next")])).unwrap();entered.recv_timeout(Duration::from_secs(2)).unwrap();pause
}

#[test]
fn explicit_copy_shared_contract_and_caller_boundaries(){
    let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/contracts/explicit_copy.json")).unwrap();
    for c in fixture["requests"].as_array().unwrap(){assert_eq!(parse_copy_clip_text_request(&c["value"]).is_ok(),c["valid"].as_bool().unwrap(),"{c}");}
    for c in fixture["results"].as_array().unwrap(){
        let result=parse_copy_clip_text_result(&c["value"],&request());assert_eq!(result.is_ok(),c["valid"].as_bool().unwrap(),"{c}");
        if let Ok(result)=result {assert_eq!(serde_json::to_value(result).unwrap(),c["value"]);}
    }
    for(label,url)in[("untrusted","http://tauri.localhost/"),("main","https://example.invalid/"),("main","http://tauri.localhost:7777/")]{assert!(crate::commands::validate_caller(label,&tauri::Url::parse(url).unwrap()).is_err());}
    assert!(crate::commands::validate_caller("main",&tauri::Url::parse("http://tauri.localhost/").unwrap()).is_ok());
}
#[test]
fn explicit_copy_full_unicode_multi_format_payload_and_self_filter(){
    let body="中😀a\r\n".repeat(20000);let bytes=STANDARD.decode(text(&body).data).unwrap();
    let f=fixture(vec![text(&body),WireFormat{name:"HTML Format".into(),data:STANDARD.encode(b"synthetic html")}]);
    let before=f.runtime.list(list()).unwrap();assert!(before.summaries[0].preview.len()<body.len());
    assert_outcome(copied(&f),CopyOutcome::Copied,None);
    let w=f.control.written.lock().unwrap();assert!(w.owned.is_empty());assert_eq!(w.closed,1);
    assert_eq!(w.transferred.iter().map(|(name,_)|name.as_str()).collect::<Vec<_>>(),vec![INTERNAL,HISTORY,CLOUD,"CF_UNICODETEXT"]);
    assert_eq!(w.transferred[1].1,[0;4]);assert_eq!(w.transferred[2].1,[0;4]);assert_eq!(w.transferred[3].1,bytes);
    let marker=w.transferred[0].1.clone();drop(w);
    f.runtime.set_enabled(true).unwrap();let mut r=response(vec![text(&body)]);r.sequence_before=50;r.sequence_locked=50;r.sequence_after=50;r.owner.hwnd=Some("99".into());r.internal_origin=Some(STANDARD.encode(&marker));
    f.runtime.inject_response(50,r).unwrap();assert_eq!(f.runtime.status().counters.ignored,"1");assert_eq!(f.runtime.status().history.items,1);
    let after=f.runtime.list(list()).unwrap();assert_eq!(before.summaries,after.summaries);
    assert_eq!(f.runtime.shared.state.lock().unwrap().origins.classify(Some(&marker),Some(98),50),Classification::External);
    let mut external=response(vec![text(&body)]);external.sequence_before=51;external.sequence_locked=51;external.sequence_after=51;external.owner.hwnd=Some("99".into());
    f.runtime.inject_response(51,external).unwrap();f.committed.recv_timeout(Duration::from_secs(2)).unwrap();assert_eq!(f.runtime.status().history.items,2,"equal external text without the registered marker remains collectable");
}
#[test]
fn explicit_copy_paused_registry_reclaims_capacity_and_resumes_self_filter(){
    let body="paused registry";
    let f=fixture(vec![text(body)]);
    for sequence in 50..350 {
        f.control.sequence.store(sequence,Ordering::Release);
        let result=copied(&f);
        assert_eq!(result.outcome,CopyOutcome::Copied,"copy {}: {result:?}",sequence-49);
        assert!(f.runtime.shared.state.lock().unwrap().origins.len()<=crate::clipboard::origin::MAX_OPERATIONS);
    }
    assert_eq!(f.control.written.lock().unwrap().created,300);
    assert_eq!(f.runtime.status().history.items,1);
    let marker=f.control.written.lock().unwrap().transferred.iter().rev().find(|(name,_)|name==INTERNAL).unwrap().1.clone();
    f.runtime.set_enabled(true).unwrap();
    let mut internal=response(vec![text(body)]);internal.sequence_before=349;internal.sequence_locked=349;internal.sequence_after=349;
    internal.owner.hwnd=Some("99".into());internal.internal_origin=Some(STANDARD.encode(&marker));
    f.runtime.inject_response(349,internal).unwrap();
    assert_eq!(f.runtime.status().counters.ignored,"1");assert_eq!(f.runtime.status().history.items,1);
    let mut external=response(vec![text(body)]);external.sequence_before=350;external.sequence_locked=350;external.sequence_after=350;external.owner.hwnd=Some("99".into());
    f.runtime.inject_response(350,external).unwrap();f.committed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(f.runtime.status().history.items,2,"equal external text remains collectable after reclamation");
}

#[test]
fn explicit_copy_paused_registry_keeps_pending_and_current_operations(){
    let f=fixture(vec![text("protected origins")]);
    let(pending,current)={
        let mut state=f.runtime.shared.state.lock().unwrap();
        let(_,pending)=state.origins.begin().unwrap();let mut current=None;
        for sequence in 1..128 {let(id,marker)=state.origins.begin().unwrap();state.origins.finish(id,Some(sequence));current=Some(marker);}
        (pending,current.unwrap())
    };
    f.control.sequence.store(128,Ordering::Release);
    assert_outcome(copied(&f),CopyOutcome::Copied,None);
    let mut state=f.runtime.shared.state.lock().unwrap();assert_eq!(state.origins.len(),3);
    assert_eq!(state.origins.classify(Some(&current),Some(99),127),Classification::Internal,"the previous current operation survives reclamation");
    assert_eq!(state.origins.classify(Some(&pending),Some(99),128),Classification::Pending,"unfinished operations survive reclamation");
}

#[test]
fn explicit_copy_listening_registry_preserves_unclassified_operations(){
    let f=fixture(vec![text("active registry")]);
    for sequence in 50..178 {f.control.sequence.store(sequence,Ordering::Release);assert_outcome(copied(&f),CopyOutcome::Copied,None);}
    f.runtime.set_enabled(true).unwrap();
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));
    assert_eq!(f.runtime.shared.state.lock().unwrap().origins.len(),crate::clipboard::origin::MAX_OPERATIONS);
    let written=f.control.written.lock().unwrap();assert_eq!(written.calls,128*11,"capacity rejection must precede native OpenClipboard");
}

#[test]
fn explicit_copy_paused_registry_rejects_old_epoch_before_classification(){
    let f=fixture(vec![text("old epoch")]);assert_outcome(copied(&f),CopyOutcome::Copied,None);
    let marker=f.control.written.lock().unwrap().transferred[0].1.clone();
    f.runtime.set_enabled(true).unwrap();
    let ticket={let mut state=f.runtime.shared.state.lock().unwrap();state.observe(50,0);state.begin_current(50).unwrap()};
    let mut late=response(vec![text("old epoch")]);late.capture_id=ticket.message.capture_id.clone();late.policy_epoch=ticket.message.policy_epoch;
    late.sequence_before=50;late.sequence_locked=50;late.sequence_after=50;late.owner.hwnd=Some("99".into());late.internal_origin=Some(STANDARD.encode(&marker));
    let late=crate::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&late).unwrap(),&ticket.message).unwrap();
    f.runtime.set_enabled(false).unwrap();
    for sequence in 51..179 {f.control.sequence.store(sequence,Ordering::Release);assert_outcome(copied(&f),CopyOutcome::Copied,None);}
    // Resume first: rejection must depend on the old epoch, rather than the pause flag.
    f.runtime.set_enabled(true).unwrap();
    let mut state=f.runtime.shared.state.lock().unwrap();let cancelled=state.counters.cancelled;let captured=state.counters.captured;
    state.complete(ticket,late);
    assert_eq!(state.counters.cancelled,cancelled+1);assert_eq!(state.counters.captured,captured);
    assert_eq!(state.internal_ignored,0,"a retired old marker must never reach classification");
    assert!(state.queue.is_empty());assert_eq!(state.history.items,1);
}

#[test]
fn explicit_copy_each_os_failure_has_conservative_outcome_and_owned_handles(){
    for fail in 1..=11 {
        let f=fixture(vec![text("合成😀")]);f.control.fail.store(fail,Ordering::Release);
        assert_outcome(copied(&f),if fail==1{CopyOutcome::NotWritten}else{CopyOutcome::Unknown},Some(ErrorCode::InjectionFailed));
        let w=f.control.written.lock().unwrap();assert!(w.owned.is_empty(),"step {fail}");assert_eq!(w.closed,usize::from(fail!=1));
        assert_eq!(w.freed,usize::from([4,6,8,10].contains(&fail)),"only a failed Set owns a handle to free");
        if fail<10{assert!(!w.transferred.iter().any(|(name,_)|name=="CF_UNICODETEXT"));}
        drop(w);assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));
    }
}
#[test]
fn explicit_copy_missing_nontext_invalid_and_synthetic_native_disabled(){
    let f=fixture(vec![WireFormat{name:"HTML Format".into(),data:STANDARD.encode(b"synthetic")}]);
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::InvalidInput));
    let mut req=request();req.clip_id="2".into();assert_eq!(f.runtime.copy_clip_text(req).unwrap().reason,Some(ErrorCode::NotFound));
    let mut req=request();req.clip_id="01".into();assert!(f.runtime.copy_clip_text(req).is_err());
    assert_eq!(f.control.written.lock().unwrap().created,0);
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();assert_eq!(runtime.write_stored_text(1).unwrap_err().code,ErrorCode::PermissionUnknown);
    runtime.shutdown(Duration::from_secs(2)).unwrap();
}
#[test]
fn explicit_copy_registration_and_factory_failure_never_open(){
    let f=fixture(vec![text("A")]);f.runtime.shared.state.lock().unwrap().origins.set_owner(0);
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::InjectionFailed));assert_eq!(f.control.written.lock().unwrap().calls,0);
    *f.runtime.shared.test_writer.lock().unwrap()=Some(Arc::new(||Err(ContractError::new(ErrorCode::InjectionFailed,"synthetic factory failure"))));
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::InjectionFailed));
}
#[test]
fn explicit_copy_pending_origin_and_concurrent_write_rejected_until_close(){
    let f=fixture(vec![text("A")]);f.control.block.store(11,Ordering::Release);
    let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let marker=f.control.written.lock().unwrap().transferred[0].1.clone();
    assert_eq!(f.runtime.shared.state.lock().unwrap().origins.classify(Some(&marker),Some(99),50),Classification::Pending);
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));assert_eq!(f.control.written.lock().unwrap().created,1);
    f.release.send(()).unwrap();assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
    assert_eq!(f.runtime.shared.state.lock().unwrap().origins.classify(Some(&marker),Some(99),50),Classification::Internal);
    f.control.block.store(0,Ordering::Release);assert_outcome(copied(&f),CopyOutcome::Copied,None);
}
#[test]
fn explicit_copy_actual_three_second_timeout_keeps_running_write_admission(){
    let f=fixture(vec![text("A")]);f.control.block.store(11,Ordering::Release);
    let runtime=f.runtime.clone();let call=std::thread::spawn(move||runtime.copy_clip_text(request()).unwrap());
    f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_outcome(call.join().unwrap(),CopyOutcome::Unknown,Some(ErrorCode::Timeout));
    assert!(f.runtime.shared.copy_busy.load(Ordering::Acquire));assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));
    f.release.send(()).unwrap();f.runtime.list(list()).unwrap(); // FIFO round-trip observes actual completion, without polling.
    assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));assert_eq!(f.control.written.lock().unwrap().created,1);
}
#[test]
fn explicit_copy_deadline_after_open_prevents_empty_and_closes_once(){
    let f=fixture(vec![text("A")]);f.control.block.store(1,Ordering::Release);
    let deadline=Instant::now()+Duration::from_secs(1);
    let rx=f.runtime.queue_copy(1,deadline).unwrap();
    f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
    // The receive deadline confirms expiry while Open is held by an owned barrier.
    assert!(matches!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())),Err(mpsc::RecvTimeoutError::Timeout)));
    f.release.send(()).unwrap();let failed=rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap_err();assert!(!failed.may_have_written);assert_eq!(failed.error.code,ErrorCode::Timeout);
    let w=f.control.written.lock().unwrap();assert_eq!(w.calls,2);assert_eq!(w.closed,1);assert!(w.transferred.is_empty());
}
#[test]
fn explicit_copy_expired_queued_request_never_constructs_writer(){
    let f=fixture(vec![text("A")]);
    let pause=pause_commit(&f);
    let rx=f.runtime.queue_copy(1,Instant::now()-Duration::from_secs(1)).unwrap();
    assert!(matches!(rx.try_recv(),Err(mpsc::TryRecvError::Empty)));assert!(f.runtime.shared.copy_busy.load(Ordering::Acquire));
    drop(pause);let failure=rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap_err();assert!(!failure.may_have_written);assert_eq!(failure.error.code,ErrorCode::Timeout);
    assert_eq!(f.control.written.lock().unwrap().created,0);
}
#[test]
fn explicit_copy_full_query_queue_releases_rejected_permit(){
    let f=fixture(vec![text("A")]);let pause=pause_commit(&f);
    let reads:Vec<_>=(0..16).map(|_|f.runtime.queue_test_query(list()).unwrap()).collect();
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));assert_eq!(f.control.written.lock().unwrap().created,0);
    drop(pause);for rx in reads{rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();}
    assert_outcome(copied(&f),CopyOutcome::Copied,None);
}
#[test]
fn explicit_copy_shutdown_before_queued_work_never_opens_writer(){
    let f=fixture(vec![text("A")]);let pause=pause_commit(&f);
    let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();
    assert!(f.runtime.shutdown(Duration::ZERO).is_err());assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));
    drop(pause);f.runtime.shutdown(Duration::from_secs(2)).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());assert_eq!(f.control.written.lock().unwrap().created,0);assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));
}
#[test]
fn explicit_copy_shutdown_during_open_prevents_empty(){
    let f=fixture(vec![text("A")]);f.control.block.store(1,Ordering::Release);
    let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(f.runtime.shutdown(Duration::ZERO).is_err());f.release.send(()).unwrap();
    let failure=rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap_err();assert!(!failure.may_have_written);assert_eq!(failure.error.code,ErrorCode::Busy);
    f.runtime.shutdown(Duration::from_secs(2)).unwrap();let w=f.control.written.lock().unwrap();assert_eq!(w.calls,2);assert_eq!(w.closed,1);assert!(w.transferred.is_empty());
}
#[test]
fn explicit_copy_shutdown_keeps_writer_owned_and_rejects_future_calls(){
    let f=fixture(vec![text("A")]);f.control.block.store(11,Ordering::Release);
    let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(f.runtime.shutdown(Duration::ZERO).unwrap_err().code,ErrorCode::CleanupFailed);
    assert!(f.runtime.storage.lock().unwrap().is_some());assert!(f.runtime.shared.copy_busy.load(Ordering::Acquire));
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));
    f.release.send(()).unwrap();assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());f.runtime.shutdown(Duration::from_secs(2)).unwrap();
    assert!(f.runtime.storage.lock().unwrap().is_none());assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));
}
#[test]
fn explicit_copy_repeated_known_failure_requires_new_call_and_preserves_internal_api(){
    let f=fixture(vec![text("A")]);f.control.fail.store(1,Ordering::Release);
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::InjectionFailed));assert_eq!(f.control.written.lock().unwrap().created,1);
    f.control.fail.store(0,Ordering::Release);assert_eq!(f.runtime.write_stored_text(1).unwrap(),2);assert_eq!(f.control.written.lock().unwrap().created,2);
}

// The capture thread owns the fake resource and invokes the exact production
// teardown boundary. Only native resource creation/destruction is substituted.
struct SyntheticCapture {end:mpsc::SyncSender<()>,started:mpsc::Receiver<()>,destroyed:mpsc::Receiver<()>}
impl Drop for SyntheticCapture {fn drop(&mut self){let _=self.end.try_send(());}}
fn synthetic_capture(f:&Fixture,failed:bool,cleanup_fails:bool)->SyntheticCapture {
    let shared=f.runtime.shared.clone();let alive=f.control.owner_alive.clone();let control=f.control.clone();
    let(end,end_rx)=mpsc::sync_channel(1);let(started_tx,started)=mpsc::sync_channel(1);let(destroyed_tx,destroyed)=mpsc::sync_channel(1);
    shared.state.lock().unwrap().listener_ready=true;
    let capture=std::thread::spawn(move||{
        end_rx.recv_timeout(Duration::from_secs(8)).unwrap();
        if failed{shared.listener_failed.store(true,Ordering::Release);shared.state.lock().unwrap().admission=Admission::Failed;}
        let _=started_tx.send(());
        shared.close_capture(||{
            assert_eq!(shared.copy_owner.lock().unwrap().borrowers,0);
            let written=control.written.lock().unwrap();assert_eq!(written.dropped,written.created,"writer Drop precedes owner destruction");
            let marker=written.transferred.iter().find(|(name,_)|name==INTERNAL).map(|(_,bytes)|bytes.clone());drop(written);
            let mut state=shared.state.lock().unwrap();
            assert_ne!(state.origins.classify(marker.as_deref(),Some(99),50),Classification::Pending,"origin finish precedes owner destruction");
            state.stop_observing();drop(state);
            assert!(alive.swap(false,Ordering::AcqRel),"destroy owned resource exactly once");let _=destroyed_tx.send(());
            if cleanup_fails{Err(cleanup())}else{Ok(())}
        });
    });
    assert!(f.runtime.capture.lock().unwrap().replace(capture).is_none());
    SyntheticCapture{end,started,destroyed}
}

#[test]
fn explicit_copy_listener_owner_survives_managed_cleanup_until_close_finishes(){
    let f=fixture(vec![text("owner lifetime")]);let capture=synthetic_capture(&f,false,false);
    f.control.block.store(11,Ordering::Release);
    let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
    capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();
    let destroyed_while_borrowed=capture.destroyed.recv_timeout(Duration::from_millis(100)).is_ok();
    let shutdown=f.runtime.shutdown(Duration::ZERO);
    let retained_capture=f.runtime.capture.lock().unwrap().is_some();let retained_storage=f.runtime.storage.lock().unwrap().is_some();
    f.release.send(()).unwrap();let reply=rx.recv_timeout(Duration::from_secs(2)).unwrap();
    if !destroyed_while_borrowed{capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();}
    f.runtime.shutdown(Duration::from_secs(2)).unwrap();
    assert!(!destroyed_while_borrowed,"managed capture cleanup destroyed the owner during an active clipboard write");
    assert!(retained_capture&&retained_storage);assert!(shutdown.is_err());assert!(reply.is_ok());
    assert!(!f.control.owner_violation.load(Ordering::Acquire));
    assert!(f.runtime.capture.lock().unwrap().is_none());assert!(f.runtime.storage.lock().unwrap().is_none());
}

#[test]
fn explicit_copy_listener_owner_survives_before_open_and_writer_drop(){
    for (block,fail) in [(13,0),(12,0),(12,11)]{
        let f=fixture(vec![text("owner phases")]);let capture=synthetic_capture(&f,false,false);f.control.block.store(block,Ordering::Release);
        f.control.fail.store(fail,Ordering::Release);
        let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();f.entered.recv_timeout(Duration::from_secs(2)).unwrap();
        capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(capture.destroyed.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(f.runtime.shutdown(Duration::ZERO).unwrap_err().code,ErrorCode::CleanupFailed);assert!(!f.runtime.exit_ready.load(Ordering::Acquire));
        assert!(f.runtime.capture.lock().unwrap().is_some());assert!(f.runtime.storage.lock().unwrap().is_some());
        f.release.send(()).unwrap();let reply=rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if block==13{let err=reply.unwrap_err();assert!(!err.may_have_written);assert_eq!(err.error.code,ErrorCode::Busy);}
        else if fail==11{let err=reply.unwrap_err();assert!(err.may_have_written);assert_eq!(err.error.code,ErrorCode::InjectionFailed);}
        else{assert!(reply.is_ok());}
        capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();f.runtime.shutdown(Duration::from_secs(2)).unwrap();
        assert!(!f.control.owner_violation.load(Ordering::Acquire));assert!(f.runtime.capture.lock().unwrap().is_none());assert!(f.runtime.storage.lock().unwrap().is_none());
    }
}

#[test]
fn explicit_copy_listener_owner_timeout_keeps_capture_and_second_write_blocked(){
    let f=fixture(vec![text("timeout")]);let capture=synthetic_capture(&f,true,false);f.control.block.store(11,Ordering::Release);
    let runtime=f.runtime.clone();let caller=std::thread::spawn(move||runtime.copy_clip_text(request()).unwrap());
    f.entered.recv_timeout(Duration::from_secs(2)).unwrap();assert_outcome(caller.join().unwrap(),CopyOutcome::Unknown,Some(ErrorCode::Timeout));
    assert!(f.runtime.shared.copy_busy.load(Ordering::Acquire));assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));
    capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(capture.destroyed.recv_timeout(Duration::from_millis(100)).is_err());assert!(f.runtime.shutdown(Duration::ZERO).is_err());
    assert!(f.runtime.capture.lock().unwrap().is_some());assert!(f.runtime.storage.lock().unwrap().is_some());
    f.release.send(()).unwrap();capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();f.runtime.shutdown(Duration::from_secs(2)).unwrap();
    assert_eq!(f.control.written.lock().unwrap().created,1);assert!(!f.control.owner_violation.load(Ordering::Acquire));
    assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));assert!(f.runtime.capture.lock().unwrap().is_none());assert!(f.runtime.storage.lock().unwrap().is_none());
}

#[test]
fn explicit_copy_listener_owner_failure_rejects_stale_recycled_value_without_writer(){
    let f=fixture(vec![text("stale")]);let capture=synthetic_capture(&f,true,false);
    capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();
    finish_thread(&f.runtime.capture,Instant::now()+Duration::from_secs(2),&f.runtime.shared.cleanup_failed).unwrap();
    assert_eq!(f.runtime.shared.state.lock().unwrap().origins.owner(),99,"retain attribution of already captured internal snapshots");
    f.runtime.shared.state.lock().unwrap().origins.set_owner(99); // Same number is not a new lifetime.
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));
    assert_eq!(f.control.written.lock().unwrap().created,0);assert_eq!(f.control.written.lock().unwrap().calls,0);
    assert_eq!(f.runtime.status().services.listener,"failed");
    f.runtime.shutdown(Duration::from_secs(2)).unwrap();f.runtime.shutdown(Duration::ZERO).unwrap();
}

#[test]
fn explicit_copy_listener_owner_unpublished_or_revoked_queued_work_never_constructs_writer(){
    let f=fixture(vec![text("unpublished")]);f.runtime.shared.copy_owner.lock().unwrap().owner=None;
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::InjectionFailed));assert_eq!(f.control.written.lock().unwrap().created,0);
    f.runtime.shared.publish_copy_owner(99);
    let capture=synthetic_capture(&f,true,false);let pause=pause_commit(&f);
    let rx=f.runtime.queue_copy(1,Instant::now()+Duration::from_secs(5)).unwrap();
    capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(pause);f.runtime.shutdown(Duration::from_secs(2)).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());assert_eq!(f.control.written.lock().unwrap().created,0);assert_eq!(f.control.written.lock().unwrap().calls,0);
    assert!(!f.runtime.shared.copy_busy.load(Ordering::Acquire));
}

#[test]
fn explicit_copy_listener_owner_revocation_preserves_completed_origin_attribution(){
    let f=fixture(vec![text("attribution")]);let capture=synthetic_capture(&f,true,false);
    assert_outcome(copied(&f),CopyOutcome::Copied,None);let marker=f.control.written.lock().unwrap().transferred[0].1.clone();
    capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();
    finish_thread(&f.runtime.capture,Instant::now()+Duration::from_secs(2),&f.runtime.shared.cleanup_failed).unwrap();
    assert_eq!(f.runtime.shared.state.lock().unwrap().origins.classify(Some(&marker),Some(99),50),Classification::Internal);
    assert_outcome(copied(&f),CopyOutcome::NotWritten,Some(ErrorCode::Busy));assert_eq!(f.control.written.lock().unwrap().created,1);
    f.runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn explicit_copy_listener_owner_live_while_user_paused_and_cleanup_failure_stays_failed(){
    let f=fixture(vec![text("paused")]);let capture=synthetic_capture(&f,false,true);
    assert!(!f.runtime.status().capture.user_enabled);assert_outcome(copied(&f),CopyOutcome::Copied,None);
    assert!(f.control.owner_alive.load(Ordering::Acquire));assert!(!f.control.owner_violation.load(Ordering::Acquire));
    capture.end.send(()).unwrap();capture.started.recv_timeout(Duration::from_secs(2)).unwrap();capture.destroyed.recv_timeout(Duration::from_secs(2)).unwrap();
    for budget in [Duration::from_secs(2),Duration::ZERO]{assert_eq!(f.runtime.shutdown(budget).unwrap_err().code,ErrorCode::CleanupFailed);assert!(!f.runtime.exit_ready.load(Ordering::Acquire));}
    assert_eq!(f.runtime.status().services.worker,"failed");assert!(f.runtime.capture.lock().unwrap().is_none());assert!(f.runtime.storage.lock().unwrap().is_none());
}
