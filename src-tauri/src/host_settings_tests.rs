//! Synthetic files, memory/native-disabled hosts and deterministic barriers only.
use super::*;
use crate::clipboard::{capture::*,protocol::WorkerMessage};
use base64::{Engine,engine::general_purpose::STANDARD};

fn directory()->PathBuf{let root=PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("isolated test root required"));assert!(root.is_absolute());let dir=root.join(format!("host-settings-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();dir}
fn host(dir:&std::path::Path,events:Events)->HostRuntime{HostRuntime::for_test_file(dir.join("history.sqlite3"),events).unwrap()}
fn response()->CaptureResponse{let mut r=CaptureResponse::empty(&WorkerMessage{version:1,capture_id:"synthetic".into(),policy_epoch:0});r.result=CaptureResult::Captured;r.reason=None;r.attempts=1;r.sequence_before=1;r.sequence_locked=1;r.sequence_after=1;r.formats=vec![WireFormat{name:"CF_UNICODETEXT".into(),data:STANDARD.encode([65,0,0,0])}];r}

#[test]
fn m2_settings_invalid_startup_blocks_enable_and_updates_without_overwrite(){
    for bytes in [b"{".as_slice(),br#"{"schema_version":8,"excluded_apps":[]}"#,br#"{"schema_version":1,"excluded_apps":[],"unknown":true}"#,br#"{"schema_version":1,"excluded_apps":["vault.exe"]}"#] {
        let dir=directory();let path=dir.join("settings.json");std::fs::write(&path,bytes).unwrap();let runtime=host(&dir,Arc::new(|_,_|{}));
        assert!(runtime.settings().blocked);assert!(runtime.settings().error.is_some());assert!(!runtime.status().capture.user_enabled);
        assert!(runtime.set_enabled(true).is_err());assert!(runtime.apply_exclusions(vec![r"C:\Private\vault.exe".into()]).is_err());
        assert!(!runtime.status().capture.admission_open);assert_eq!(std::fs::read(path).unwrap(),bytes);runtime.shutdown(Duration::from_secs(1)).unwrap();
    }
}
#[test]
fn m2_settings_failed_windows_replacement_retains_disk_rules_and_can_retry(){
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ,FILE_SHARE_WRITE};
    let dir=directory();let path=dir.join("settings.json");let bytes=br#"{"schema_version":1,"excluded_apps":["C:\\Private\\old.exe"]}"#;std::fs::write(&path,bytes).unwrap();let runtime=host(&dir,Arc::new(|_,_|{}));runtime.set_enabled(true).unwrap();let epoch=runtime.status().capture.policy_epoch;
    let deny_delete=std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE).open(&path).unwrap();
    let error=runtime.apply_exclusions(vec![r"C:\Private\new.exe".into()]).unwrap_err();assert_eq!(error.code,ErrorCode::StorageUnavailable);
    assert_eq!(std::fs::read(&path).unwrap(),bytes);assert_eq!(runtime.settings().excluded_apps,vec![r"C:\Private\old.exe"]);assert!(!runtime.settings().blocked);assert!(!runtime.settings().saving);assert_eq!(runtime.status().capture.policy_epoch,epoch);assert!(runtime.status().capture.admission_open);
    assert!(!std::fs::read_dir(&dir).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().starts_with(".settings-")));
    drop(deny_delete);let updated=runtime.apply_exclusions(vec![r"C:\Private\new.exe".into(),r"c:\PRIVATE\NEW.EXE".into()]).unwrap();assert_eq!(updated.excluded_apps,vec![r"C:\Private\new.exe"]);assert!(runtime.status().capture.policy_epoch>epoch);
    let epoch=runtime.status().capture.policy_epoch;runtime.apply_exclusions(vec![r"c:\private\NEW.EXE".into()]).unwrap();assert_eq!(runtime.status().capture.policy_epoch,epoch,"repeated equivalent apply must be idempotent");runtime.shutdown(Duration::from_secs(1)).unwrap();
}
#[test]
fn m2_settings_external_change_blocks_capture_without_claiming_old_policy(){
    let dir=directory();let runtime=host(&dir,Arc::new(|_,_|{}));runtime.set_enabled(true).unwrap();let external=br#"{"schema_version":2,"excluded_apps":[]}"#;std::fs::write(dir.join("settings.json"),external).unwrap();
    assert!(runtime.apply_exclusions(vec![r"C:\Private\vault.exe".into()]).is_err());assert!(runtime.settings().blocked);assert!(!runtime.status().capture.admission_open);assert!(!runtime.status().capture.user_enabled);assert!(runtime.set_enabled(true).is_err());assert_eq!(std::fs::read(dir.join("settings.json")).unwrap(),external);runtime.shutdown(Duration::from_secs(1)).unwrap();
}
#[test]
fn m2_settings_equivalent_apply_rechecks_durability_before_success(){
    let dir=directory();let runtime=host(&dir,Arc::new(|_,_|{}));runtime.apply_exclusions(vec![r"C:\Private\vault.exe".into()]).unwrap();runtime.set_enabled(true).unwrap();
    let external=br#"{"schema_version":8,"excluded_apps":[]}"#;std::fs::write(dir.join("settings.json"),external).unwrap();
    assert!(runtime.apply_exclusions(vec![r"c:\PRIVATE\VAULT.EXE".into()]).is_err(),"equivalent apply must not acknowledge an unverified durable file");
    assert!(runtime.settings().blocked);assert!(!runtime.status().capture.admission_open);assert_eq!(std::fs::read(dir.join("settings.json")).unwrap(),external);runtime.shutdown(Duration::from_secs(1)).unwrap();
}
#[test]
fn m2_settings_save_waits_for_committing_history_then_cancels_old_queue_and_callback(){
    let dir=directory();let(tx,events)=mpsc::channel();let(release_history,history_wait)=mpsc::channel();let history_wait=Mutex::new(history_wait);
    let runtime=Arc::new(host(&dir,Arc::new(move|name,_|{let _=tx.send(name.to_owned());if name=="history_changed"{history_wait.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();}})));runtime.set_enabled(true).unwrap();
    let(entered,ready)=mpsc::sync_channel(1);let(release,go)=mpsc::channel();let go=Mutex::new(go);let once=AtomicBool::new(false);
    *runtime.shared.before_commit.lock().unwrap()=Some(Arc::new(move||{if !once.swap(true,Ordering::AcqRel){entered.send(()).unwrap();go.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();}}));
    runtime.inject_response(1,response()).unwrap();ready.recv_timeout(Duration::from_secs(1)).unwrap();runtime.inject_response(2,response()).unwrap();
    let ticket={let mut s=runtime.shared.state.lock().unwrap();s.observe(3,0);s.begin_current(3).unwrap()};let epoch=runtime.status().capture.policy_epoch;
    let updater=runtime.clone();let(done,applied)=mpsc::channel();let update=std::thread::spawn(move||{done.send(updater.apply_exclusions(vec![r"C:\Private\vault.exe".into()])).unwrap();});
    while !runtime.settings().saving{events.recv_timeout(Duration::from_secs(1)).unwrap();}assert!(applied.try_recv().is_err());release.send(()).unwrap();applied.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();update.join().unwrap();release_history.send(()).unwrap();
    let mut r=response();r.capture_id=ticket.message.capture_id.clone();r.policy_epoch=ticket.message.policy_epoch;let snapshot=crate::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&r).unwrap(),&ticket.message).unwrap();runtime.shared.state.lock().unwrap().complete(ticket,snapshot);
    assert_eq!(runtime.status().history.items,1);assert_eq!(runtime.status().queue.items,0);assert_eq!(runtime.status().counters.cancelled,"2");assert!(runtime.status().capture.policy_epoch>epoch);runtime.shutdown(Duration::from_secs(1)).unwrap();
}
#[test]
fn m2_settings_exit_fences_late_publication_and_waits_for_save_owner(){
    let dir=directory();let runtime=Arc::new(host(&dir,Arc::new(|_,_|{})));runtime.set_enabled(true).unwrap();let epoch=runtime.status().capture.policy_epoch;
    let(entered,ready)=mpsc::sync_channel(1);let(release,go)=mpsc::channel();let go=Mutex::new(go);
    *runtime.shared.before_settings_replace.lock().unwrap()=Some(Arc::new(move||{entered.send(()).unwrap();go.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();}));
    let updater=runtime.clone();let update=std::thread::spawn(move||updater.apply_exclusions(vec![r"C:\Private\vault.exe".into()]));ready.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(runtime.apply_exclusions(vec![r"C:\Private\second.exe".into()]).is_err(),"double apply must not create another save");
    runtime.begin_shutdown(Instant::now());assert!(!runtime.status().capture.admission_open);assert!(runtime.shutdown_until(Instant::now()).is_err());assert!(!runtime.exit_ready.load(Ordering::Acquire));
    release.send(()).unwrap();assert!(update.join().unwrap().is_err());assert!(runtime.settings().blocked);assert!(runtime.settings().excluded_apps.is_empty());assert_eq!(runtime.status().capture.policy_epoch,epoch);assert!(!runtime.settings().saving);
    runtime.shutdown(Duration::from_secs(1)).unwrap();drop(runtime);
    let reopened=host(&dir,Arc::new(|_,_|{}));assert_eq!(reopened.settings().excluded_apps,vec![r"C:\Private\vault.exe"]);assert!(!reopened.status().capture.user_enabled);reopened.shutdown(Duration::from_secs(1)).unwrap();
}
#[test]
fn m2_settings_owner_filter_reports_unknown_only_with_rules_and_matches_ordinal(){
    let mut state=CaptureState::new("settings-owner");assert!(state.owner_allowed(None));assert!(state.owner_allowed(Some("not-a-path")));
    state.set_exclusions(vec![r"C:\Private\Ä.exe".into()]);assert!(!state.owner_allowed(Some(r"c:\PRIVATE\ä.EXE")));assert_eq!(state.notice,Some(crate::runtime::Notice::ExcludedOwner));
    for source in [None,Some("invalid")]{assert!(!state.owner_allowed(source));assert_eq!(state.notice,Some(crate::runtime::Notice::UnknownOwner));}
    assert!(state.owner_allowed(Some(r"C:\Public\other.exe")));state.set_exclusions(vec![]);assert!(state.owner_allowed(None));
}

fn owner_response(sequence:u32,source:Option<&str>)->CaptureResponse{
    let mut r=response();r.sequence_before=sequence;r.sequence_locked=sequence;r.sequence_after=sequence;
    if let Some(source)=source{r.owner=Owner{hwnd:Some("7".into()),pid:Some(42),process:Some(source.into())};}r
}

#[test]
fn m2_settings_last_skip_survives_pause_and_resume_without_reopening_paused_admission(){
    let dir=directory();let runtime=host(&dir,Arc::new(|_,_|{}));runtime.apply_exclusions(vec![r"C:\Private\vault.exe".into()]).unwrap();runtime.set_enabled(true).unwrap();
    runtime.inject_response(1,owner_response(1,Some(r"c:\PRIVATE\VAULT.EXE"))).unwrap();
    assert_eq!(runtime.settings().last_skip.as_deref(),Some("excluded_owner"));assert_eq!(runtime.status().counters.ignored,"1");
    let ticket={let mut s=runtime.shared.state.lock().unwrap();s.observe(2,0);s.begin_current(2).unwrap()};let epoch=runtime.status().capture.policy_epoch;
    let paused=runtime.set_enabled(false).unwrap();let paused_settings=runtime.settings();
    assert!(!paused.capture.user_enabled);assert!(!paused.capture.admission_open);assert!(paused.capture.policy_epoch>epoch);assert_eq!(paused.counters.ignored,"1");assert_eq!(paused.queue.items,0);
    assert_eq!(runtime.shared.state.lock().unwrap().notice,None,"pause still clears the transient notice");
    assert_eq!(paused_settings.last_skip.as_deref(),Some("excluded_owner"),"pause must retain the last application skip; snapshot={paused_settings:?}");
    assert!(runtime.inject_response(3,owner_response(3,None)).is_err());
    let mut late=owner_response(2,None);late.capture_id=ticket.message.capture_id.clone();late.policy_epoch=ticket.message.policy_epoch;
    let snapshot=crate::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&late).unwrap(),&ticket.message).unwrap();runtime.shared.state.lock().unwrap().complete(ticket,snapshot);
    assert_eq!(runtime.status().counters.cancelled,"1");assert_eq!(runtime.status().counters.ignored,"1");assert_eq!(runtime.settings().last_skip.as_deref(),Some("excluded_owner"));
    let resumed=runtime.set_enabled(true).unwrap();assert!(resumed.capture.admission_open);assert!(resumed.capture.policy_epoch>paused.capture.policy_epoch);
    runtime.inject_response(4,owner_response(4,None)).unwrap();assert_eq!(runtime.settings().last_skip.as_deref(),Some("unknown_owner"));assert_eq!(runtime.status().counters.ignored,"2");
    runtime.shutdown(Duration::from_secs(1)).unwrap();
}

#[test]
fn m2_settings_last_skip_survives_notices_policy_and_accepted_capture_then_resets_on_restart(){
    let dir=directory();let(tx,committed)=mpsc::channel();let runtime=host(&dir,Arc::new(move|name,_|{if name=="history_changed"{let _=tx.send(());}}));
    assert_eq!(runtime.settings().last_skip,None);runtime.apply_exclusions(vec![r"C:\Private\vault.exe".into()]).unwrap();runtime.set_enabled(true).unwrap();
    runtime.inject_response(1,owner_response(1,None)).unwrap();assert_eq!(runtime.settings().last_skip.as_deref(),Some("unknown_owner"));
    runtime.shared.state.lock().unwrap().observe(0,0);assert_eq!(runtime.shared.state.lock().unwrap().notice,Some(crate::runtime::Notice::ZeroSequence));
    assert_eq!(runtime.settings().last_skip.as_deref(),Some("unknown_owner"),"an unrelated capture notice must not erase the last application skip");
    runtime.apply_exclusions(vec![r"C:\Private\second.exe".into()]).unwrap();assert_eq!(runtime.shared.state.lock().unwrap().notice,None);assert_eq!(runtime.settings().last_skip.as_deref(),Some("unknown_owner"));
    runtime.inject_response(2,owner_response(2,Some(r"C:\Public\allowed.exe"))).unwrap();committed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(runtime.status().counters.persisted,"1");assert_eq!(runtime.status().counters.ignored,"1");assert_eq!(runtime.settings().last_skip.as_deref(),Some("unknown_owner"));
    runtime.inject_response(3,owner_response(3,Some(r"C:\Private\second.exe"))).unwrap();assert_eq!(runtime.settings().last_skip.as_deref(),Some("excluded_owner"));assert_eq!(runtime.status().counters.ignored,"2");
    runtime.apply_exclusions(vec![]).unwrap();runtime.inject_response(4,owner_response(4,None)).unwrap();committed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(runtime.settings().last_skip.as_deref(),Some("excluded_owner"));assert_eq!(runtime.status().counters.ignored,"2");
    // A real overload warning still clears on explicit recovery without losing application feedback.
    {let mut s=runtime.shared.state.lock().unwrap();for sequence in 10..=10+crate::runtime::MAX_NOTIFICATIONS as u32{s.observe(sequence,0);}assert_eq!(s.admission,Admission::Overloaded);assert_eq!(s.notice,Some(crate::runtime::Notice::NotificationOverrun));}
    runtime.set_enabled(true).unwrap();assert_eq!(runtime.shared.state.lock().unwrap().notice,None);assert_eq!(runtime.settings().last_skip.as_deref(),Some("excluded_owner"));
    runtime.shutdown(Duration::from_secs(1)).unwrap();drop(runtime);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(dir.join("settings.json")).unwrap()).unwrap(),serde_json::json!({"schema_version":1,"excluded_apps":[]}));
    let reopened=host(&dir,Arc::new(|_,_|{}));assert_eq!(reopened.settings().last_skip,None);assert_eq!(reopened.status().counters.ignored,"0");assert!(!reopened.status().capture.admission_open);reopened.shutdown(Duration::from_secs(1)).unwrap();
}

#[test]
fn m2_settings_last_skip_from_commit_recheck_survives_pause(){
    for (source,expected) in [(Some(r"C:\Private\vault.exe"),"excluded_owner"),(None,"unknown_owner")] {
        let dir=directory();let(tx,changed)=mpsc::channel();let runtime=host(&dir,Arc::new(move|name,_|{if name=="status_changed"{let _=tx.send(());}}));runtime.set_enabled(true).unwrap();
        {
            let _gate=runtime.shared.gate.lock().unwrap();runtime.inject_response(1,owner_response(1,source)).unwrap();assert_eq!(runtime.status().queue.items,1);assert_eq!(runtime.settings().last_skip,None);
            // Install only the recheck precondition: normal policy publication cancels old queues.
            runtime.shared.state.lock().unwrap().exclusions=vec![r"C:\Private\vault.exe".into()];
        }
        while runtime.status().counters.ignored=="0"{changed.recv_timeout(Duration::from_secs(1)).unwrap();}
        assert_eq!(runtime.status().queue.items,0);assert_eq!(runtime.status().history.items,0);assert_eq!(runtime.status().counters.ignored,"1");assert_eq!(runtime.settings().last_skip.as_deref(),Some(expected));
        runtime.set_enabled(false).unwrap();assert!(!runtime.status().capture.admission_open);assert_eq!(runtime.status().counters.ignored,"1");assert_eq!(runtime.settings().last_skip.as_deref(),Some(expected),"commit-time rejection must use the same retained feedback producer");
        runtime.shutdown(Duration::from_secs(1)).unwrap();
    }
}
