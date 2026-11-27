#![cfg(feature="test-fixtures")]
use pinfeather_lib::{host::{HostRuntime,HistoryManagement},contracts::ErrorCode};
use std::{sync::Arc,time::Duration};
#[test]
fn storage_owner_requires_paused_state_and_current_count() {
 let runtime=HostRuntime::for_test_memory(Arc::new(|_,_|{})).unwrap();
 runtime.set_enabled(true).unwrap();
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:0,expected_history_revision:"0".into()}).unwrap_err().code,ErrorCode::Busy);
 runtime.set_enabled(false).unwrap();
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:1,expected_history_revision:"0".into()}).unwrap_err().code,ErrorCode::Busy);
 assert!(runtime.manage_history(HistoryManagement::Delete{clip_id:"0".into(),expected_items:0}).is_err());
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:0,expected_history_revision:"0".into()}).unwrap().deleted,0);
 runtime.shared.state.lock().unwrap().storage_inflight=(1,8);
 assert_eq!(runtime.manage_history(HistoryManagement::Clear{expected_items:0,expected_history_revision:"0".into()}).unwrap_err().code,ErrorCode::Busy);
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

#[test]
fn clear_requires_a_history_revision_even_when_the_count_is_valid() {
    let missing = serde_json::json!({"action":"clear","expected_items":0});
    assert!(serde_json::from_value::<HistoryManagement>(missing).is_err());
    let current = serde_json::json!({"action":"clear","expected_items":0,"expected_history_revision":"0"});
    assert!(serde_json::from_value::<HistoryManagement>(current).is_ok());
}

#[test]
fn favorites_change_revision_and_conflict_with_a_clear_confirmed_in_another_window() {
    use pinfeather_lib::{
        contracts::{parse_set_favorite_request, parse_status_result},
        store::{Store, Snapshot, SourceConfidence, Payload},
    };
    let root = std::env::temp_dir().join(format!("pf-fav-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("history.sqlite3");
    let mut store = Store::open(&path).unwrap();
    for capture_id in ["one", "two"] {
        store.insert(&Snapshot {
            capture_id:capture_id.into(),captured_at_ms:1,source_process:None,source_confidence:SourceConfidence::Unknown,
            omitted_formats:vec![],payloads:vec![Payload{format_name:"CF_UNICODETEXT".into(),data:vec![65,0,0,0]}],
        }).unwrap();
    }
    drop(store);
    let (events, changes) = std::sync::mpsc::channel();
    let runtime = HostRuntime::for_test_file(path, Arc::new(move |event, revision| {
        if event == "history_changed" { let _ = events.send(revision); }
    })).unwrap();
    let favorite = |clip_id, favorited| {
        let request = parse_set_favorite_request(&serde_json::json!({"request_id":"fav","clip_id":clip_id,"favorited":favorited})).unwrap();
        runtime.set_favorite(request, None).unwrap()
    };
    runtime.set_enabled(true).unwrap();
    assert!(favorite("1", true).changed);
    assert!(!favorite("1", true).changed);
    assert_eq!(runtime.status().history.revision, "1");
    runtime.set_enabled(false).unwrap();
    let confirmed = runtime.status().history;
    assert!(favorite("1", false).changed);
    assert!(favorite("2", true).changed);
    let live = runtime.status();
    parse_status_result(&serde_json::to_value(&live).unwrap()).unwrap();
    assert_eq!((live.history.items, live.history.favorites, &*live.history.revision), (confirmed.items, confirmed.favorites, "3"));
    assert!(changes.try_recv().is_err(), "favorites must not emit the clearing history_changed event");
    let old = HistoryManagement::Clear { expected_items: confirmed.items as u64, expected_history_revision: confirmed.revision };
    assert_eq!(runtime.manage_history(old).unwrap_err().code, ErrorCode::Busy);
    assert_eq!(runtime.status().history.items, 2);
    let current = HistoryManagement::Clear { expected_items: 2, expected_history_revision: "3".into() };
    let cleared = runtime.manage_history(current).unwrap();
    assert_eq!((cleared.deleted, cleared.items, cleared.favorites), (1, 1, 1));
    assert_eq!(changes.recv_timeout(Duration::from_secs(2)).unwrap(), 4);
    assert_eq!(runtime.status().history.revision, "4");
    runtime.shutdown(Duration::from_secs(2)).unwrap();
}
