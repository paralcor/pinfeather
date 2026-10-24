//! E09.4a restart acceptance: real HostRuntime + storage thread + file store
//! across close/reopen for persisted search, UTF-8 body framing and pagination.
//! Only synthetic CF_UNICODETEXT responses enter via inject_response; the
//! databases are fresh UUID directories under PINFEATHER_TEST_DATA_ROOT and are
//! kept on disk as evidence.
#![cfg(feature="test-fixtures")]
use std::{path::{Path,PathBuf},sync::{Arc,mpsc},time::Duration};
use pinfeather_lib::{host::HostRuntime,clipboard::capture::*,clipboard::protocol::WorkerMessage,contracts::*};
use base64::{Engine,engine::general_purpose::STANDARD};

fn text_response(sequence:u32,text:&str)->CaptureResponse{
    let mut r=CaptureResponse::empty(&WorkerMessage{version:1,capture_id:"fixture".into(),policy_epoch:0});
    r.result=CaptureResult::Captured;r.reason=None;r.attempts=1;
    r.sequence_before=sequence;r.sequence_locked=sequence;r.sequence_after=sequence;
    r.formats=vec![WireFormat{name:"CF_UNICODETEXT".into(),data:STANDARD.encode(text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<u8>>())}];
    r
}

/// Fresh UUID directory under the isolated root: create_dir must fail if the
/// directory already exists, and a genuinely first open has no main/WAL/SHM.
fn fresh_db_path(tag:&str)->PathBuf{
    let root=PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("set isolated PINFEATHER_TEST_DATA_ROOT"));
    assert!(root.is_absolute());
    let dir=root.join(format!("{}-{}-{}",tag,std::process::id(),uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).expect("uuid directory must not pre-exist");
    let path=dir.join("history.sqlite3");
    for suffix in ["","-wal","-shm"]{
        let mut probe=path.as_os_str().to_os_string();probe.push(suffix);
        assert!(!Path::new(&probe).exists(),"first open must see no history.sqlite3{suffix} file");
    }
    path
}

/// Owns one runtime until an explicit bounded shutdown succeeds; Drop is
/// best-effort and never unwraps, so an assertion panic cannot mask itself.
struct OwnedRuntime{runtime:Option<HostRuntime>}
impl OwnedRuntime{
    fn open(path:&Path)->(OwnedRuntime,mpsc::Receiver<u64>){
        let(tx,rx)=mpsc::channel();
        let runtime=HostRuntime::for_test_file(path.to_path_buf(),Arc::new(move|name,revision|{if name=="history_changed"{let _=tx.send(revision);}})).expect("isolated fresh database must open");
        runtime.set_enabled(true).expect("synthetic storage must enable");
        (OwnedRuntime{runtime:Some(runtime)},rx)
    }
    fn get(&self)->&HostRuntime{self.runtime.as_ref().expect("runtime held until shutdown")}
    /// Consuming close: a failed shutdown panics here and can never reopen this
    /// same database, because the old runtime is not released for a new open.
    fn shutdown(mut self){
        let runtime=self.runtime.take().expect("shutdown is one-shot");
        runtime.shutdown(Duration::from_secs(2)).expect("storage thread must stop before reopen");
        drop(runtime);
    }
}
impl Drop for OwnedRuntime{
    fn drop(&mut self){
        if let Some(runtime)=self.runtime.take(){let _=runtime.shutdown(Duration::from_secs(2));}
    }
}

fn commit(runtime:&HostRuntime,rx:&mpsc::Receiver<u64>,sequence:u32,text:&str){
    runtime.inject_response(sequence,text_response(sequence,text)).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).expect("real commit must publish history revision");
}

fn list_page(runtime:&HostRuntime,tag:&str,query:&str,limit:u32,cursor:Option<&ListCursor>)->ListClipsResult{
    let request=parse_list_clips_request(&serde_json::json!({
        "request_id":tag,"query":query,"limit":limit,
        "cursor":cursor.map(|c|serde_json::to_value(c).unwrap()),
    })).unwrap();
    let expected_normalized=request.query.clone();
    let result=runtime.list(request).unwrap();
    assert_eq!(result.request_id,tag,"request_id echo");
    assert_eq!(result.normalized_query,expected_normalized,"normalized query echo");
    parse_list_clips_result(&serde_json::to_value(&result).unwrap()).expect("real parser accepts the list result");
    result
}

fn ids_of(page:&ListClipsResult)->Vec<String>{page.summaries.iter().map(|s|s.id.clone()).collect()}

fn text_frame(runtime:&HostRuntime,clip_id:&str,offset:i64)->GetClipTextResult{
    let request=parse_get_clip_text_request(&serde_json::json!({"request_id":"frame","clip_id":clip_id,"offset":offset})).unwrap();
    let frame=runtime.get_clip_text(request.clone()).unwrap();
    parse_get_clip_text_result(&serde_json::to_value(&frame).unwrap(),&request).expect("real parser checks request_id/clip_id echo");
    frame
}

fn text_frame_err(runtime:&HostRuntime,clip_id:&str,offset:i64)->ErrorCode{
    let request=parse_get_clip_text_request(&serde_json::json!({"request_id":"frame","clip_id":clip_id,"offset":offset})).unwrap();
    runtime.get_clip_text(request).unwrap_err().code
}

#[test]
fn persisted_literal_search_survives_reopen(){
    let path=fresh_db_path("e094a-search");
    let hit=format!("{}尾部命中词 %_ [x] .*","前".repeat(512));
    let control="对照正文乙丙丁句段";
    assert!(['%','_','[',']','.','*','前','尾'].iter().all(|c|!control.contains(*c)),"control body carries none of the probed symbols");
    let(hit_id,control_id);
    {
        let(runtime,rx)=OwnedRuntime::open(&path);
        commit(runtime.get(),&rx,1,&hit);
        commit(runtime.get(),&rx,2,control);
        let all=list_page(runtime.get(),"all","",50,None);
        assert_eq!(all.summaries.len(),2);
        hit_id=all.summaries.iter().find(|s|s.preview=="前".repeat(200)).expect("hit preview is the 前 head").id.clone();
        control_id=all.summaries.iter().find(|s|s.preview==control).expect("control preview is its own short text").id.clone();
        runtime.shutdown();
    }
    let(runtime,_rx)=OwnedRuntime::open(&path);
    let expected=vec![hit_id.clone()];
    for (tag,query) in [("q1","前"),("q2","尾"),("q3","尾部"),("q4","尾部命中词"),("q5","尾部命中词 %_ [x] .*"),("q6","%"),("q7","_"),("q8","%_"),("q9","[x]"),("q10",".*"),("q11","*")]{
        let page=list_page(runtime.get(),tag,query,50,None);
        assert_eq!(ids_of(&page),expected,"literal query {query:?} returns only the hit id");
    }
    // The tail words live past the 200-scalar preview yet full-text search hits.
    let tail=list_page(runtime.get(),"q-tail","尾部命中词",50,None);
    assert_eq!(tail.summaries[0].preview,"前".repeat(200));
    assert!(!tail.summaries[0].preview.contains("尾部命中词"),"preview must not carry the tail word");
    // Wildcard-looking prefixes are literals and must not match.
    for (tag,query) in [("q-px","前%"),("q-pu","前_"),("q-ps","前*"),("q-pd","前.")]{
        let page=list_page(runtime.get(),tag,query,50,None);
        assert!(page.summaries.is_empty()&&page.next_cursor.is_none(),"literal negative {query:?} must not match");
    }
    assert_eq!(ids_of(&list_page(runtime.get(),"q-ctl","对照",50,None)),vec![control_id.clone()]);
    assert!(list_page(runtime.get(),"q-miss","无此词句",50,None).summaries.is_empty());
    // Whitespace edges normalize through the real parser before storage.
    let padded=list_page(runtime.get(),"q-pad","  尾部命中词  ",50,None);
    assert_eq!(padded.normalized_query,"尾部命中词");
    assert_eq!(ids_of(&padded),expected);
    runtime.shutdown();
}

#[test]
fn persisted_multiframe_text_frames_survive_reopen(){
    let path=fresh_db_path("e094a-text");
    let cn_body=format!("{}😀终","中".repeat(21845));
    let mixed_body=format!("{}终","甲😀".repeat(20000));
    assert_eq!(cn_body.len(),65542,"中*21845+😀终 is 65542 UTF-8 bytes");
    assert_eq!(mixed_body.len(),140003,"甲😀*20000+终 is 140003 UTF-8 bytes");
    let(cn_id,mixed_id);
    {
        let(runtime,rx)=OwnedRuntime::open(&path);
        commit(runtime.get(),&rx,1,&cn_body);
        commit(runtime.get(),&rx,2,&mixed_body);
        let all=list_page(runtime.get(),"all","",50,None);
        assert_eq!(all.summaries.len(),2);
        cn_id=all.summaries.iter().find(|s|s.preview=="中".repeat(200)).expect("cn preview").id.clone();
        mixed_id=all.summaries.iter().find(|s|s.preview=="甲😀".repeat(100)).expect("mixed preview").id.clone();
        runtime.shutdown();
    }
    let(runtime,_rx)=OwnedRuntime::open(&path);
    // Server byte authority: 65535+7, boundary backs off 中|😀.
    let first=text_frame(runtime.get(),&cn_id,0);
    assert_eq!(first.text.len(),65535);
    assert_eq!(first.text,"中".repeat(21845));
    assert_eq!(first.next_offset,Some(65535));
    let second=text_frame(runtime.get(),&cn_id,first.next_offset.expect("first frame continues"));
    assert_eq!(second.text.len(),7);
    assert_eq!(second.text,"😀终");
    assert!(second.next_offset.is_none(),"last frame ends with an explicit null");
    assert_eq!(format!("{}{}",first.text,second.text),cn_body,"65535+7 reassembles exactly without splitting 中/😀");
    let eof=text_frame(runtime.get(),&cn_id,cn_body.len() as i64);
    assert_eq!(eof.text,"");
    assert!(eof.next_offset.is_none(),"legal EOF is an empty frame with explicit null");
    assert_eq!(text_frame_err(runtime.get(),&cn_id,65536),ErrorCode::InvalidInput,"65536 falls inside the first 😀");
    // Second body: 65534+65534+8935 driven by server next_offset only.
    let mut joined=String::new();
    let mut offset=0i64;
    let mut lens=Vec::new();
    for _ in 0..8{
        let frame=text_frame(runtime.get(),&mixed_id,offset);
        lens.push(frame.text.len());
        joined.push_str(&frame.text);
        match frame.next_offset{
            None=>break,
            Some(next)=>{assert!(next>offset,"offset must strictly advance");offset=next;}
        }
    }
    assert_eq!(lens,vec![65534,65534,8935],"server frames are 65534+65534+8935 bytes");
    assert_eq!(joined,mixed_body,"all frames reassemble the full body intact");
    assert_eq!(text_frame_err(runtime.get(),&mixed_id,65535),ErrorCode::InvalidInput,"65535 falls inside 甲");
    assert_eq!(text_frame_err(runtime.get(),&mixed_id,140004),ErrorCode::InvalidInput,"past EOF");
    let eof=text_frame(runtime.get(),&mixed_id,140003);
    assert_eq!(eof.text,"");
    assert!(eof.next_offset.is_none());
    runtime.shutdown();
}

#[test]
fn pagination_cursor_stable_across_insert_and_reopen(){
    let path=fresh_db_path("e094a-page");
    let bodies=["锚点甲一","锚点乙二","锚点丙三","锚点丁四"];
    let newer="锚点戊五";
    let(initial_ids,page1_ids,cursor1,newer_id);
    {
        let(runtime,rx)=OwnedRuntime::open(&path);
        for(i,body) in bodies.iter().enumerate(){commit(runtime.get(),&rx,(i+1) as u32,body);}
        let all=list_page(runtime.get(),"all","锚点",50,None);
        assert_eq!(all.summaries.len(),4);
        initial_ids=ids_of(&all);
        let page1=list_page(runtime.get(),"p1","锚点",2,None);
        assert_eq!(page1.summaries.len(),2);
        page1_ids=ids_of(&page1);
        assert_eq!(page1_ids,initial_ids[..2],"first page is the newest two of the initial set");
        let cursor=page1.next_cursor.clone().expect("two more matches need a cursor");
        assert_eq!(cursor.normalized_query,"锚点");
        assert_eq!(cursor.upper_id,page1_ids[0],"upper_id is the newest id at first-page time");
        cursor1=cursor;
        commit(runtime.get(),&rx,5,newer);
        let fresh=list_page(runtime.get(),"fresh","锚点",50,None);
        assert_eq!(fresh.summaries.len(),5);
        assert_eq!(fresh.summaries[0].preview,newer);
        newer_id=fresh.summaries[0].id.clone();
        let old_ids=ids_of(&list_page(runtime.get(),"old","锚点",2,Some(&cursor1)));
        assert_eq!(old_ids,initial_ids[2..4],"old cursor resumes the remaining initial ids");
        assert!(!old_ids.contains(&newer_id),"the newer capture must not mix into old cursor pages");
        runtime.shutdown();
    }
    let(runtime,_rx)=OwnedRuntime::open(&path);
    let resumed=ids_of(&list_page(runtime.get(),"again","锚点",2,Some(&cursor1)));
    assert_eq!(resumed,initial_ids[2..4],"the saved cursor resumes the same keyset after reopen");
    assert!(!resumed.contains(&newer_id));
    let upper=cursor1.upper_id.parse::<i64>().unwrap();
    assert!(newer_id.parse::<i64>().unwrap()>upper,"the newer id lies past the cursor upper_id bound");
    for id in &resumed{assert!(id.parse::<i64>().unwrap()<=upper,"resumed ids stay within upper_id");}
    let mut seen=page1_ids.clone();seen.extend(resumed.iter().cloned());seen.sort();
    let mut expected=initial_ids.clone();expected.sort();
    assert_eq!(seen,expected,"saved cursor pages cover the initial set with no duplicate and no omission");
    assert_eq!(ids_of(&list_page(runtime.get(),"all2","锚点",50,None)).len(),5,"the reopened store still sees every capture");
    runtime.shutdown();
}
