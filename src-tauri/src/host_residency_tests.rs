//! Synthetic owners, memory storage and deterministic barriers only.
use super::*;
use crate::clipboard::{capture::*,protocol::WorkerMessage};
use base64::{Engine,engine::general_purpose::STANDARD};
fn response()->CaptureResponse{let mut r=CaptureResponse::empty(&WorkerMessage{version:1,capture_id:"synthetic".into(),policy_epoch:0});r.result=CaptureResult::Captured;r.reason=None;r.attempts=1;r.sequence_before=1;r.sequence_locked=1;r.sequence_after=1;r.formats=vec![WireFormat{name:"CF_UNICODETEXT".into(),data:STANDARD.encode([65,0,0,0])}];r}
#[test]
fn m2_settings_persist_and_restart_paused(){
    let root=PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("isolated test root required"));
    let dir=root.join(format!("settings-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();
    let path=dir.join("history.sqlite3");let runtime=HostRuntime::for_test_file(path.clone(),Arc::new(|_,_|{})).unwrap();
    runtime.set_exclusions(vec!["d:/Private/old/../vault.exe".into()]).unwrap();
    assert!(dir.join("settings.json").is_file(),"accepted exclusion rules must be durable before acknowledgement");
    runtime.set_enabled(true).unwrap();runtime.shutdown(Duration::from_secs(2)).unwrap();drop(runtime);
    let reopened=HostRuntime::for_test_file(path,Arc::new(|_,_|{})).unwrap();
    assert_eq!(reopened.shared.state.lock().unwrap().exclusions,vec![r"D:\Private\vault.exe"]);
    assert!(!reopened.status().capture.user_enabled);assert!(!reopened.status().capture.admission_open);
    reopened.shutdown(Duration::from_secs(2)).unwrap();
}
#[test]
fn m2_panel_failure_and_panic_still_shutdown_real_host_with_same_deadline(){
    for panic in [false,true]{
        let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();let deadline=Instant::now()+Duration::from_secs(1);
        let result=crate::residency::cleanup_owners(deadline,|passed|{assert_eq!(passed,deadline);if panic{panic!("synthetic panel failure");}Err(ContractError::new(ErrorCode::CleanupFailed,"panel hide failed"))},|passed|{assert_eq!(passed,deadline);runtime.shutdown_until(passed)});
        assert!(result.unwrap_err().message.contains("面板"));assert!(runtime.status().state=="stopping");assert!(runtime.storage.lock().unwrap().is_none());assert!(runtime.shared.closing.closing());
    }
}
#[test]
fn m2_normal_exit_drains_accepted_queue_without_user_pause_or_epoch_change(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let(entered,ready)=mpsc::sync_channel(1);let(release,go)=mpsc::channel();let go=Mutex::new(go);let once=AtomicBool::new(false);
    *runtime.shared.before_commit.lock().unwrap()=Some(Arc::new(move||{if !once.swap(true,Ordering::AcqRel){entered.send(()).unwrap();go.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();}}));
    runtime.inject_response(1,response()).unwrap();ready.recv_timeout(Duration::from_secs(1)).unwrap();runtime.inject_response(2,response()).unwrap();
    let epoch=runtime.status().capture.policy_epoch;let deadline=Instant::now()+Duration::from_secs(2);runtime.begin_shutdown(deadline);
    let stopped=runtime.status();assert_eq!(stopped.queue.items,2);assert!(!stopped.capture.admission_open);assert_eq!(stopped.capture.policy_epoch,epoch);
    release.send(()).unwrap();runtime.shutdown_until(deadline).unwrap();let status=runtime.status();assert_eq!(status.history.items,2);assert_eq!(status.counters.cancelled,"0");assert_eq!(status.queue.items,0);
}
#[test]
fn m2_failed_save_retry_drains_same_queue_without_reopening_admission(){
    let(events,changed)=mpsc::channel();let runtime=HostRuntime::for_test_memory(Arc::new(move|name,_|{let _=events.send(name.to_owned());})).unwrap();
    runtime.set_test_fault(true).unwrap();runtime.set_enabled(true).unwrap();runtime.inject_response(1,response()).unwrap();
    while runtime.status().services.storage!="failed"{changed.recv_timeout(Duration::from_secs(1)).unwrap();}
    let epoch=runtime.status().capture.policy_epoch;assert_eq!(runtime.shutdown_until(Instant::now()).unwrap_err().code,ErrorCode::Timeout);assert_eq!(runtime.status().queue.items,1);
    assert!(runtime.set_enabled(true).is_err());assert!(runtime.inject_response(2,response()).is_err());runtime.set_test_fault(false).unwrap();runtime.retry_saving().unwrap();
    while runtime.status().history.items==0{changed.recv_timeout(Duration::from_secs(1)).unwrap();}
    runtime.shutdown_until(Instant::now()+Duration::from_secs(1)).unwrap();let status=runtime.status();assert_eq!(status.queue.items,0);assert_eq!(status.capture.policy_epoch,epoch);assert!(!status.capture.admission_open);assert_eq!(status.state,"stopping");assert!(!runtime.shared.state.lock().unwrap().restart_worker);
}
#[test]
fn m2_late_capture_callback_after_close_is_cancelled_without_new_queue(){
    let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();runtime.set_enabled(true).unwrap();
    let ticket={let mut state=runtime.shared.state.lock().unwrap();state.observe(1,0);state.begin_current(1).unwrap()};runtime.begin_shutdown(Instant::now()+Duration::from_secs(1));
    let mut response=response();response.capture_id=ticket.message.capture_id.clone();response.policy_epoch=ticket.message.policy_epoch;
    let snapshot=crate::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&response).unwrap(),&ticket.message).unwrap();{let mut s=runtime.shared.state.lock().unwrap();s.admission=Admission::Failed;s.complete(ticket,snapshot);}
    assert_eq!(runtime.status().queue.items,0);assert_eq!(runtime.status().counters.cancelled,"1");runtime.shutdown(Duration::from_secs(1)).unwrap();
}
