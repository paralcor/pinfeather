use pinfeather_lib::{store::{Store,Snapshot,Payload,SourceConfidence},contracts::*};
use serde_json::json;
fn input(id:&str,time:i64,text:&str)->Snapshot{Snapshot{capture_id:id.into(),captured_at_ms:time,source_process:None,source_confidence:SourceConfidence::Unknown,omitted_formats:vec![],payloads:vec![Payload{format_name:"CF_UNICODETEXT".into(),data:text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()}]}}
#[test]
fn recent_summaries_are_real_bounded_and_cursor_stable_without_payload_data(){
    let mut store=Store::open_in_memory().unwrap();for (id,time,text) in [("one",1,"one"),("two",2,"https://example.invalid/a"),("three",2,"third")]{store.insert(&input(id,time,text)).unwrap();}
    let request=parse_list_clips_request(&json!({"request_id":"list-1","query":"","limit":2})).unwrap();let first=store.list_recent(&request).unwrap();assert_eq!(first.summaries.iter().map(|s|s.preview.as_str()).collect::<Vec<_>>(),vec!["third","https://example.invalid/a"]);assert_eq!(first.summaries[1].kind,"link");assert!(first.next_cursor.is_some());assert!(!serde_json::to_string(&first).unwrap().contains("CF_UNICODETEXT\": ["));parse_list_clips_result(&serde_json::to_value(&first).unwrap()).unwrap();
    store.insert(&input("new",3,"later")).unwrap();let next=parse_list_clips_request(&json!({"request_id":"list-2","query":"","limit":2,"cursor":first.next_cursor})).unwrap();let page=store.list_recent(&next).unwrap();assert_eq!(page.summaries.len(),1);assert_eq!(page.summaries[0].preview,"one");assert!(page.next_cursor.is_none());
}
#[test]
fn empty_database_and_nonempty_query_have_explicit_results(){
    let store=Store::open_in_memory().unwrap();let empty=parse_list_clips_request(&json!({"request_id":"empty","query":""})).unwrap();assert!(store.list_recent(&empty).unwrap().summaries.is_empty());let search=parse_list_clips_request(&json!({"request_id":"search","query":"future"})).unwrap();assert_eq!(store.list_recent(&search).unwrap_err().code,ErrorCode::InvalidInput);
}
