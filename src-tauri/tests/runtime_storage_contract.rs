#![cfg(feature="test-fixtures")]
use std::{sync::{Arc,mpsc},time::Duration};
use pinfeather_lib::{host::HostRuntime,clipboard::capture::*,clipboard::protocol::WorkerMessage,contracts::*};
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
    use std::sync::Barrier;
    let runtime=Arc::new(HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap());runtime.set_enabled(true).unwrap();
    let entered=Arc::new(Barrier::new(2));let release=Arc::new(Barrier::new(2));let (a,b)=(entered.clone(),release.clone());
    *runtime.shared.before_commit.lock().unwrap()=Some(Arc::new(move||{a.wait();b.wait();}));
    runtime.inject_response(1,response()).unwrap();entered.wait();assert_eq!(runtime.status().queue.items,1,"in-flight DB snapshot remains reserved");
    let(attempt_tx,attempt_rx)=mpsc::channel();*runtime.shared.before_policy.lock().unwrap()=Some(Arc::new(move||{let _=attempt_tx.send(());}));
    let(tx,rx)=mpsc::channel();let handle=runtime.clone();let pause=std::thread::spawn(move||{let _=tx.send(handle.set_enabled(false));});attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(),"pause cannot acknowledge before the gate's commit");
    release.wait();let status=rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();assert!(!status.capture.user_enabled);assert_eq!(status.history.items,1);pause.join().unwrap();
    *runtime.shared.before_commit.lock().unwrap()=None;*runtime.shared.before_policy.lock().unwrap()=None;
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
    use std::sync::Barrier;
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let entered=Arc::new(Barrier::new(2));let release=Arc::new(Barrier::new(2));let(a,b)=(entered.clone(),release.clone());*runtime.shared.before_commit.lock().unwrap()=Some(Arc::new(move||{a.wait();b.wait();}));
    runtime.inject_response(1,response()).unwrap();entered.wait();
    let mut reads=Vec::new();for _ in 0..16{reads.push(runtime.queue_test_query(request()).unwrap());}assert_eq!(runtime.queue_test_query(request()).unwrap_err().code,ErrorCode::Busy);assert_eq!(runtime.status().queue.items,1);
    release.wait();for read in reads{assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap().unwrap().summaries.len(),1);}*runtime.shared.before_commit.lock().unwrap()=None;runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn history_admission_counts_the_snapshot_currently_being_committed(){
    use std::sync::Barrier;
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();runtime.shared.state.lock().unwrap().history.items=MAX_HISTORY_ITEMS as i64-1;
    let entered=Arc::new(Barrier::new(2));let release=Arc::new(Barrier::new(2));let(a,b)=(entered.clone(),release.clone());*runtime.shared.before_commit.lock().unwrap()=Some(Arc::new(move||{a.wait();b.wait();}));
    runtime.inject_response(1,response()).unwrap();entered.wait();runtime.inject_response(2,response()).unwrap();let s=runtime.status();assert_eq!(s.queue.items,1);assert_eq!(s.state,"overloaded");assert_eq!(s.counters.overloaded,"1");release.wait();*runtime.shared.before_commit.lock().unwrap()=None;runtime.shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn rejected_existing_database_is_preserved_by_the_real_storage_thread(){
    let root=std::path::PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("set isolated PINFEATHER_TEST_DATA_ROOT"));let dir=root.join(format!("unknown-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();let path=dir.join("history.sqlite3");std::fs::write(&path,b"synthetic unrelated database bytes").unwrap();let before=std::fs::read(&path).unwrap();
    let runtime=HostRuntime::for_test_file(path.clone(),Arc::new(|_,_|{})).unwrap();assert_eq!(runtime.status().services.storage,"failed");assert_eq!(runtime.set_enabled(true).unwrap_err().code,ErrorCode::StorageUnavailable);assert!(runtime.list(request()).is_err());runtime.shutdown(Duration::from_secs(2)).unwrap();assert_eq!(std::fs::read(&path).unwrap(),before);assert_eq!(std::fs::read_dir(&dir).unwrap().count(),1);
}

#[test]
fn expired_write_never_starts_and_synthetic_runtime_disallows_native_writes(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();assert_eq!(runtime.expired_test_copy(1).unwrap_err().code,ErrorCode::Timeout);assert_eq!(runtime.write_stored_text(1).unwrap_err().code,ErrorCode::PermissionUnknown);runtime.shutdown(Duration::from_secs(2)).unwrap();
}
