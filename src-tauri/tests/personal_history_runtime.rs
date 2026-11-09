#![cfg(feature="test-fixtures")]
use pinfeather_lib::{host::{HostRuntime,HistoryManagement},contracts::ErrorCode};
use std::{sync::Arc,time::Duration};
#[test]
fn storage_owner_requires_paused_state_and_current_count() {
 let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();
 runtime.set_enabled(true).unwrap();
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:0}).unwrap_err().code,ErrorCode::Busy);
 runtime.set_enabled(false).unwrap();
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:1}).unwrap_err().code,ErrorCode::Busy);
 assert!(runtime.manage_history(HistoryManagement::Delete{clip_id:"0".into(),expected_items:0}).is_err());
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:0}).unwrap().deleted,0);
 runtime.shared.state.lock().unwrap().storage_inflight=(1,8);
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:0}).unwrap_err().code,ErrorCode::Busy);
 runtime.shared.state.lock().unwrap().storage_inflight=(0,0);
 runtime.shutdown(Duration::from_secs(2)).unwrap();
}
#[test]
fn management_contract_rejects_extra_fields_and_main_only_authorization() {
 assert!(serde_json::from_value::<HistoryManagement>(serde_json::json!({"action":"backup","path":"D:\\arbitrary"})).is_err());
 assert!(serde_json::from_value::<HistoryManagement>(serde_json::json!({"action":"clear","expected_items":-1})).is_err());
 let url=tauri::Url::parse("http://tauri.localhost/").unwrap();
 assert!(pinfeather_lib::commands::validate_main_caller("quick-panel",&url).is_err());
 assert!(pinfeather_lib::commands::validate_main_caller("main",&url).is_ok());
}

#[test]
fn runtime_management_updates_counts_events_and_uses_fixed_backup_directory() {
 use pinfeather_lib::store::{Store,Snapshot,SourceConfidence,Payload};
 let root=std::env::temp_dir().join(format!("pf-runtime-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let path=root.join("history.sqlite3");
 let mut store=Store::open(&path).unwrap();let id=store.insert(&Snapshot{capture_id:"runtime".into(),captured_at_ms:1,source_process:None,source_confidence:SourceConfidence::Unknown,omitted_formats:vec![],payloads:vec![Payload{format_name:"CF_UNICODETEXT".into(),data:vec![65,0,0,0]}]}).unwrap().clip_id;drop(store);
 let(tx,rx)=std::sync::mpsc::channel();let runtime=HostRuntime::for_test_file(path,Arc::new(move|name,_|{if name=="history_changed"{let _=tx.send(());}})).unwrap();
 let backup=runtime.manage_history(HistoryManagement::Backup{}).unwrap().backup_path.unwrap();assert_eq!(std::path::Path::new(&backup).parent().unwrap(),root.join("backups"));assert_eq!(Store::open(std::path::Path::new(&backup)).unwrap().usage().unwrap().items,1);
 let result=runtime.manage_history(HistoryManagement::Delete{clip_id:id.to_string(),expected_items:1}).unwrap();assert_eq!((result.deleted,result.items),(1,0));rx.recv_timeout(Duration::from_secs(2)).unwrap();assert_eq!(runtime.status().history.items,0);
 runtime.shutdown(Duration::from_secs(2)).unwrap();
}
