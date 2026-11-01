#![cfg(feature="test-fixtures")]
use std::{sync::{Arc,mpsc},time::Duration};
use pinfeather_lib::{host::{HostRuntime,SharedRuntime},clipboard::capture::*,clipboard::protocol::WorkerMessage,contracts::*};
use base64::{Engine,engine::general_purpose::STANDARD};
fn response()->CaptureResponse{let mut r=CaptureResponse::empty(&WorkerMessage{version:1,capture_id:"fixture".into(),policy_epoch:0});r.result=CaptureResult::Captured;r.reason=None;r.attempts=1;r.sequence_before=1;r.sequence_locked=1;r.sequence_after=1;r.formats=vec![WireFormat{name:"CF_UNICODETEXT".into(),data:STANDARD.encode([65u8,0,0,0])}];r}
fn request()->ListClipsRequest{parse_list_clips_request(&serde_json::json!({"request_id":"list","query":""})).unwrap()}
#[test]
fn real_storage_thread_commits_before_event_and_recent_list(){
    let(tx,rx)=mpsc::channel();let runtime=HostRuntime::for_test_memory(Arc::new(move|name,revision|{if name=="history_changed"{let _=tx.send(revision);}})).unwrap();runtime.set_enabled(true).unwrap();runtime.inject_response(1,response()).unwrap();rx.recv_timeout(Duration::from_secs(2)).expect("real commit must publish history revision");let status=runtime.status();assert_eq!(status.history.items,1);assert_eq!(status.queue.items,0);assert_eq!(status.counters.persisted,"1");parse_status_result(&serde_json::to_value(status).unwrap()).unwrap();let list=runtime.list(request()).unwrap();assert_eq!(list.summaries[0].preview,"A");runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn commit_failure_preserves_pending_snapshot_and_requires_explicit_retry(){
    let(tx,rx)=mpsc::channel();let(events_tx,events_rx)=mpsc::channel();let runtime=HostRuntime::for_test_memory(Arc::new(move|name,revision|{let _=events_tx.send(name.to_string());if name=="history_changed"{let _=tx.send(revision);}})).unwrap();
    runtime.set_test_fault(true).unwrap();runtime.set_enabled(true).unwrap();runtime.inject_response(1,response()).unwrap();
    while runtime.status().services.storage!="failed"{events_rx.recv_timeout(Duration::from_secs(2)).unwrap();}
    let failed=runtime.status();assert_eq!(failed.history.items,0);assert_eq!(failed.queue.items,1);assert_eq!(failed.counters.persisted,"0");assert!(rx.try_recv().is_err());assert!(runtime.list(request()).unwrap().summaries.is_empty());
    runtime.set_test_fault(false).unwrap();runtime.set_enabled(true).unwrap();rx.recv_timeout(Duration::from_secs(2)).unwrap();assert_eq!(runtime.status().queue.items,0);assert_eq!(runtime.list(request()).unwrap().summaries.len(),1);runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn pause_confirmation_waits_for_in_progress_commit_and_then_cancels_old_work(){
    let runtime=Arc::new(HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap());runtime.set_enabled(true).unwrap();
    std::thread::scope(|scope|{
        let pause=PauseCommit::install(&runtime);
        runtime.inject_response(1,response()).unwrap();pause.entered();
        let reserved=runtime.status().queue.items;
        let(attempt_tx,attempt_rx)=mpsc::channel();*runtime.shared.before_policy.lock().unwrap()=Some(Arc::new(move||{let _=attempt_tx.send(());}));
        let(tx,rx)=mpsc::channel();let handle=runtime.clone();let pausing=scope.spawn(move||{let _=tx.send(handle.set_enabled(false));});
        let attempted=attempt_rx.recv_timeout(Duration::from_secs(2));
        let premature=rx.recv_timeout(Duration::from_millis(100));
        pause.release();let result=rx.recv_timeout(Duration::from_secs(2));let joined=pausing.join();
        *runtime.shared.before_policy.lock().unwrap()=None;
        assert_eq!(reserved,1,"in-flight DB snapshot remains reserved");attempted.unwrap();
        assert!(premature.is_err(),"pause cannot acknowledge before the gate's commit");
        let status=result.unwrap().unwrap();assert!(!status.capture.user_enabled);assert_eq!(status.history.items,1);joined.unwrap();
    });
    assert!(runtime.inject_response(2,response()).is_err());assert_eq!(runtime.list(request()).unwrap().summaries.len(),1);runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn pause_before_reply_prevents_any_database_commit(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let ticket={let mut state=runtime.shared.state.lock().unwrap();state.observe(1,0);state.begin_current(1).unwrap()};
    runtime.set_enabled(false).unwrap();let mut r=response();r.capture_id=ticket.message.capture_id.clone();r.policy_epoch=ticket.message.policy_epoch;
    let decoded=pinfeather_lib::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&r).unwrap(),&ticket.message).unwrap();runtime.shared.state.lock().unwrap().complete(ticket,decoded);
    assert_eq!(runtime.status().counters.cancelled,"1");assert!(runtime.list(request()).unwrap().summaries.is_empty());runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn file_storage_thread_reopens_only_its_owned_isolated_database(){
    let root=std::path::PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("set isolated PINFEATHER_TEST_DATA_ROOT"));assert!(root.is_absolute());let dir=root.join(format!("runtime-{}-{}",std::process::id(),uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();let path=dir.join("history.sqlite3");
    let(tx,rx)=mpsc::channel();let runtime=HostRuntime::for_test_file(path.clone(),Arc::new(move|name,_|{if name=="history_changed"{let _=tx.send(());}})).unwrap();runtime.set_enabled(true).unwrap();runtime.inject_response(1,response()).unwrap();rx.recv_timeout(Duration::from_secs(2)).unwrap();runtime.shutdown(Duration::from_secs(2)).unwrap();drop(runtime);
    let reopened=HostRuntime::for_test_file(path,Arc::new(|_,_|{})).unwrap();assert_eq!(reopened.status().history.items,1);assert_eq!(reopened.list(request()).unwrap().summaries[0].preview,"A");reopened.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn pending_query_limit_is_sixteen_and_queued_snapshots_keep_their_reservation(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let pause=PauseCommit::install(&runtime);
    runtime.inject_response(1,response()).unwrap();pause.entered();
    let mut reads=Vec::new();for _ in 0..16{reads.push(runtime.queue_test_query(request()).unwrap());}let seventeenth=runtime.queue_test_query(request());let reserved=runtime.status().queue.items;
    pause.release();assert_eq!(seventeenth.unwrap_err().code,ErrorCode::Busy);assert_eq!(reserved,1);
    for read in reads{assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap().unwrap().summaries.len(),1);}runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn history_admission_counts_the_snapshot_currently_being_committed(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();runtime.shared.state.lock().unwrap().history.items=MAX_HISTORY_ITEMS as i64-1;
    let pause=PauseCommit::install(&runtime);
    runtime.inject_response(1,response()).unwrap();pause.entered();runtime.inject_response(2,response()).unwrap();let s=runtime.status();pause.release();
    assert_eq!(s.queue.items,1);assert_eq!(s.state,"overloaded");assert_eq!(s.counters.overloaded,"1");runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn rejected_existing_database_is_preserved_by_the_real_storage_thread(){
    let root=std::path::PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("set isolated PINFEATHER_TEST_DATA_ROOT"));let dir=root.join(format!("unknown-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();let path=dir.join("history.sqlite3");std::fs::write(&path,b"synthetic unrelated database bytes").unwrap();let before=std::fs::read(&path).unwrap();
    let runtime=HostRuntime::for_test_file(path.clone(),Arc::new(|_,_|{})).unwrap();assert_eq!(runtime.status().services.storage,"failed");assert_eq!(runtime.set_enabled(true).unwrap_err().code,ErrorCode::StorageUnavailable);assert!(runtime.list(request()).is_err());assert_eq!(runtime.get_clip_text(text_request("1",0)).unwrap_err().code,ErrorCode::StorageUnavailable);runtime.shutdown(Duration::from_secs(2)).unwrap();assert_eq!(std::fs::read(&path).unwrap(),before);assert_eq!(std::fs::read_dir(&dir).unwrap().count(),1);
}

#[test]
fn expired_write_never_starts_and_synthetic_runtime_disallows_native_writes(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();assert_eq!(runtime.expired_test_copy(1).unwrap_err().code,ErrorCode::Timeout);assert_eq!(runtime.write_stored_text(1).unwrap_err().code,ErrorCode::PermissionUnknown);runtime.shutdown(Duration::from_secs(2)).unwrap();
}

// ---- E08.3a storage-thread text queries ----

fn text_response(text:&str)->CaptureResponse{
    let mut r=CaptureResponse::empty(&WorkerMessage{version:1,capture_id:"fixture".into(),policy_epoch:0});
    r.result=CaptureResult::Captured;r.reason=None;r.attempts=1;r.sequence_before=1;r.sequence_locked=1;r.sequence_after=1;
    r.formats=vec![WireFormat{name:"CF_UNICODETEXT".into(),data:STANDARD.encode(text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<u8>>())}];
    r
}
fn text_request(clip_id:&str,offset:i64)->GetClipTextRequest{parse_get_clip_text_request(&serde_json::json!({"request_id":"txt","clip_id":clip_id,"offset":offset})).unwrap()}

/// Scoped ownership of the pause-at-commit fixture. Installing it hands the
/// commit checkpoint to this guard; exactly one release path — explicit
/// `release()` or `Drop`, including panic unwind — signals the hook before
/// clearing it, so a failing assertion cannot strand the storage thread.
/// (Edition 2021 keeps the host's `before_commit` guard alive for the whole
/// callback, so the signal must precede the lock that clears the hook.)
struct PauseCommit {
    shared:Arc<SharedRuntime>,
    ready:mpsc::Receiver<()>,
    release_tx:std::sync::Mutex<Option<mpsc::SyncSender<()>>>,
}
impl PauseCommit {
    fn install(runtime:&HostRuntime)->PauseCommit{
        let(ready_tx,ready)=mpsc::sync_channel(1);
        let(release_tx,release_rx)=mpsc::sync_channel(1);
        let release_rx=std::sync::Mutex::new(release_rx);
        *runtime.shared.before_commit.lock().unwrap_or_else(|e|e.into_inner())=Some(Arc::new(move||{
            let _=ready_tx.send(());
            let slot=release_rx.lock().unwrap_or_else(|e|e.into_inner());
            let _=slot.recv();
        }));
        PauseCommit{shared:runtime.shared.clone(),ready,release_tx:std::sync::Mutex::new(Some(release_tx))}
    }
    /// Bounded confirmation that the storage thread reached the checkpoint.
    fn entered(&self){
        self.ready.recv_timeout(Duration::from_secs(2)).expect("storage thread must reach the commit checkpoint");
    }
    /// One-shot: signal the waiting hook first (it may still hold the hook
    /// lock), then clear the hook. Idempotent, and never blocks on the peer.
    fn release(&self){
        if let Some(tx)=self.release_tx.lock().unwrap_or_else(|e|e.into_inner()).take(){let _=tx.try_send(());}
        *self.shared.before_commit.lock().unwrap_or_else(|e|e.into_inner())=None;
    }
}
impl Drop for PauseCommit{fn drop(&mut self){self.release();}}

#[test]
fn storage_thread_search_and_two_frame_text_read(){
    let body=format!("{}中😀b","a".repeat(65535));
    let(tx,rx)=mpsc::channel();let runtime=HostRuntime::for_test_memory(Arc::new(move|name,revision|{if name=="history_changed"{let _=tx.send(revision);}})).unwrap();
    runtime.set_enabled(true).unwrap();runtime.inject_response(1,text_response(&body)).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).expect("real commit must publish history revision");
    let list=runtime.list(parse_list_clips_request(&serde_json::json!({"request_id":"q","query":"中😀b","limit":10})).unwrap()).unwrap();
    assert_eq!(list.summaries.len(),1);
    assert_eq!(list.summaries[0].preview,"a".repeat(200),"preview holds only the head");
    assert!(!list.summaries[0].preview.contains("中😀b"),"the tail word lives past the preview");
    let id=list.summaries[0].id.clone();
    let first=runtime.get_clip_text(text_request(&id,0)).unwrap();
    assert_eq!(first.request_id,"txt");assert_eq!(first.clip_id,id);
    assert_eq!(first.text,"a".repeat(65535));assert_eq!(first.next_offset,Some(65535));
    parse_get_clip_text_result(&serde_json::to_value(&first).unwrap(),&text_request(&id,0)).unwrap();
    let second=runtime.get_clip_text(text_request(&id,65535)).unwrap();
    assert_eq!(second.text,"中😀b");assert!(second.next_offset.is_none());
    parse_get_clip_text_result(&serde_json::to_value(&second).unwrap(),&text_request(&id,65535)).unwrap();
    assert!(rx.try_recv().is_err(),"text reads must not publish history_changed");
    assert_eq!(runtime.status().history.items,1,"text reads must not change usage");
    runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn text_read_missing_invalid_and_busy_after_shutdown(){
    let body=format!("{}中😀b","a".repeat(65535));
    let(tx,rx)=mpsc::channel();let runtime=HostRuntime::for_test_memory(Arc::new(move|name,revision|{if name=="history_changed"{let _=tx.send(revision);}})).unwrap();
    runtime.set_enabled(true).unwrap();runtime.inject_response(1,text_response(&body)).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let id=runtime.list(request()).unwrap().summaries[0].id.clone();
    assert_eq!(runtime.get_clip_text(text_request("999",0)).unwrap_err().code,ErrorCode::NotFound);
    assert_eq!(runtime.get_clip_text(text_request(&id,65536)).unwrap_err().code,ErrorCode::InvalidInput,"offset inside 中");
    assert_eq!(runtime.get_clip_text(GetClipTextRequest{request_id:"x".into(),clip_id:id.clone(),offset:-1}).unwrap_err().code,ErrorCode::InvalidInput);
    assert_eq!(runtime.get_clip_text(GetClipTextRequest{request_id:"x".into(),clip_id:"0".into(),offset:0}).unwrap_err().code,ErrorCode::InvalidInput);
    runtime.shutdown(Duration::from_secs(2)).unwrap();
    assert_eq!(runtime.get_clip_text(text_request(&id,0)).unwrap_err().code,ErrorCode::Busy);
}

#[test]
fn shared_query_capacity_fits_eight_lists_and_eight_texts(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let pause=PauseCommit::install(&runtime);
    runtime.inject_response(1,response()).unwrap();pause.entered();
    let mut lists=Vec::new();let mut texts=Vec::new();
    for _ in 0..8{lists.push(runtime.queue_test_query(request()).unwrap());}
    for _ in 0..8{texts.push(runtime.queue_test_text_query(text_request("1",0)).unwrap());}
    let seventeenth=runtime.get_clip_text(text_request("1",0)).err().map(|e|e.code);
    pause.release();
    assert_eq!(seventeenth,Some(ErrorCode::Busy),"the seventeenth query shares the single sixteen-slot queue");
    for read in lists{assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap().unwrap().summaries.len(),1);}
    for read in texts{assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap().unwrap().text,"A");}
    assert_eq!(runtime.get_clip_text(text_request("1",0)).unwrap().text,"A","later queries keep working");
    runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn read_timeout_is_bounded_at_five_seconds(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let pause=PauseCommit::install(&runtime);
    runtime.inject_response(1,response()).unwrap();pause.entered();
    let started=std::time::Instant::now();
    let outcome=runtime.get_clip_text(text_request("1",0)).err().map(|e|e.code);
    let elapsed=started.elapsed();
    pause.release();
    assert_eq!(outcome,Some(ErrorCode::Timeout),"caller returns a bounded timeout");
    assert!(elapsed>=Duration::from_secs(5),"the production five-second bound is not loosened");
    assert!(elapsed<Duration::from_secs(30),"and the caller is not stuck");
    assert_eq!(runtime.list(request()).unwrap().summaries.len(),1,"a late reply must not block the storage thread");
    runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn panic_in_paused_scope_releases_commit_and_recovers(){
    let(tx,rx)=mpsc::channel();let runtime=HostRuntime::for_test_memory(Arc::new(move|name,revision|{if name=="history_changed"{let _=tx.send(revision);}})).unwrap();
    runtime.set_enabled(true).unwrap();
    let caught=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
        let pause=PauseCommit::install(&runtime);
        runtime.inject_response(1,response()).unwrap();
        pause.entered();
        panic!("deliberate failure inside the paused scope");
    }));
    assert!(caught.is_err(),"the deliberate panic must unwind through the guard");
    rx.recv_timeout(Duration::from_secs(2)).expect("the paused commit must finish once the guard releases it");
    assert!(runtime.shared.before_commit.lock().unwrap_or_else(|e|e.into_inner()).is_none(),"the guard must clear the hook");
    assert_eq!(runtime.list(request()).unwrap().summaries.len(),1,"queries work after the recovered commit");
    runtime.inject_response(2,response()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).expect("later commits still publish");
    assert_eq!(runtime.list(request()).unwrap().summaries.len(),2,"the storage thread keeps committing");
    runtime.shutdown(Duration::from_secs(2)).unwrap();
}
