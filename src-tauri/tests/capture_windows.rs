//! Explicit manual E07 entry. Never runs as part of ordinary automated checks.
#![cfg(feature="test-fixtures")]
#[test]
#[ignore = "replaces the real system clipboard; user must authorize the isolated run"]
fn internal_write_notification_is_classified_without_self_recording(){
    use std::{sync::{Arc,mpsc},time::{Duration,Instant}};
    use pinfeather_lib::{host::HostRuntime,store::{Store,Snapshot,Payload,SourceConfidence}};
    assert_eq!(std::env::var("PINFEATHER_ALLOW_SYSTEM_CLIPBOARD_TESTS").as_deref(),Ok("1"),"manual clipboard permission is required");
    let root=std::path::PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("set a new isolated test root"));assert!(root.is_absolute());let dir=root.join(format!("clipboard-manual-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();
    let mut store=Store::open(&dir.join("history.sqlite3")).unwrap();let saved=store.insert(&Snapshot{capture_id:"manual-seed".into(),captured_at_ms:1,source_process:None,source_confidence:SourceConfidence::Unknown,omitted_formats:vec![],payloads:vec![Payload{format_name:"CF_UNICODETEXT".into(),data:"Pinfeather isolated internal write".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()}]}).unwrap();drop(store);
    let(tx,rx)=mpsc::channel();let runtime=HostRuntime::for_manual_product(dir,std::path::PathBuf::from(env!("CARGO_BIN_EXE_pinfeather")),Arc::new(move|_,_|{let _=tx.send(());})).unwrap();runtime.set_enabled(true).unwrap();runtime.write_stored_text(saved.clip_id).unwrap();let deadline=Instant::now()+Duration::from_secs(5);
    while runtime.shared.state.lock().unwrap().internal_ignored==0 {rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("internal classification not observed");}
    assert_eq!(runtime.status().history.items,1);assert_eq!(runtime.status().queue.items,0);runtime.shutdown(Duration::from_secs(3)).unwrap();
}
