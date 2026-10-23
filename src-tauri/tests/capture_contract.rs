//! 合成字节测试，不访问系统剪贴板、用户库或桌面输入。
use pinfeather_lib::clipboard::capture::*;
use pinfeather_lib::store::Payload;
use pinfeather_lib::clipboard::protocol::WorkerMessage;
use std::collections::VecDeque;

fn text(value: &str) -> Payload {
    Payload { format_name: "CF_UNICODETEXT".into(), data: value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect() }
}

#[test]
fn valid_original_formats_and_utf16_padding_are_preserved() {
    let mut padded = text("中文😀"); padded.data.extend_from_slice(&[0, 216, 255, 255]);
    assert_eq!(validate_payloads(&[padded]), Ok(()));
    assert_eq!(validate_payloads(&[Payload {format_name:"HTML Format".into(),data:b"<b>raw</b>".to_vec()}]), Ok(()));
    assert_eq!(validate_payloads(&[Payload {format_name:"PNG".into(),data:b"\x89PNG\r\n\x1a\n".to_vec()}]), Ok(()));
}

#[test]
fn invalid_utf16_and_png_are_rejected() {
    for data in [vec![65], vec![65,0], vec![0,216,0,0],vec![0,220,0,0]] {
        assert!(validate_payloads(&[Payload{format_name:"CF_UNICODETEXT".into(),data}]).is_err());
    }
    assert!(validate_payloads(&[Payload{format_name:"PNG".into(),data:b"not png".to_vec()}]).is_err());
}

#[derive(Default)]
struct FakeClock { ms:u64, sleeps:Vec<u64> }
impl Clock for FakeClock {
    fn now_ms(&self)->u64 {self.ms}
    fn sleep_ms(&mut self,ms:u64) {self.ms+=ms;self.sleeps.push(ms);}
}
struct Api {
    seq:VecDeque<u32>, last:u32, busy:usize, opened:bool, closes:usize,
    formats:Vec<(u32,String,Result<Vec<u8>,CaptureReason>)>, reads:Vec<u32>, owner:Owner,close_fails:bool,
}
impl Api {
    fn new()->Self {Self{seq:VecDeque::new(),last:1,busy:0,opened:false,closes:0,formats:vec![(13,"CF_UNICODETEXT".into(),Ok(text("same").data))],reads:vec![],owner:Owner::default(),close_fails:false}}
    fn marker(&mut self,name:&str,data:Result<Vec<u8>,CaptureReason>) {let id=100+self.formats.len() as u32;self.formats.insert(0,(id,name.into(),data));}
}
impl ClipboardApi for Api {
    fn sequence(&mut self)->u32 { if let Some(seq)=self.seq.pop_front(){self.last=seq;} self.last }
    fn open(&mut self)->bool {if self.busy>0 {self.busy-=1;false} else {self.opened=true;true}}
    fn close(&mut self)->Result<(),CaptureReason>{assert!(self.opened);self.opened=false;self.closes+=1;if self.close_fails{Err(CaptureReason::CloseFailed)}else{Ok(())}}
    fn owner(&mut self)->Owner {assert!(self.opened);self.owner.clone()}
    fn formats(&mut self)->Result<Vec<(u32,String)>,CaptureReason>{assert!(self.opened);Ok(self.formats.iter().map(|(id,n,_)|(*id,n.clone())).collect())}
    fn read(&mut self,id:u32,limit:usize)->Result<Vec<u8>,CaptureReason>{assert!(self.opened);self.reads.push(id);let bytes=self.formats.iter().find(|(i,_,_)|*i==id).unwrap().2.clone()?;if bytes.len()>limit {Err(CaptureReason::InvalidPayload)}else{Ok(bytes)}}
}
fn request()->WorkerMessage{WorkerMessage{version:1,capture_id:"capture-1".into(),policy_epoch:3}}
fn run(api:&mut Api)->CaptureResponse{capture_request(&request(),api,&mut FakeClock::default())}

#[test]
fn privacy_markers_precede_payload_reads_and_cloud_is_local_neutral(){
    for name in [EXCLUDE,IGNORE]{let mut a=Api::new();a.marker(name,Err(CaptureReason::ReadFailed));let r=run(&mut a);assert_eq!(r.result,CaptureResult::Ignored);assert!(a.reads.is_empty());assert_eq!(a.closes,1);}
    for value in [0u32,1,2]{let mut a=Api::new();a.marker(HISTORY,Ok(value.to_le_bytes().to_vec()));assert_eq!(run(&mut a).result,if value==1 {CaptureResult::Captured}else{CaptureResult::Ignored});}
    for data in [Ok(vec![]),Ok(vec![1,0]),Err(CaptureReason::ReadFailed)]{let mut a=Api::new();a.marker(HISTORY,data);assert_eq!(run(&mut a).reason,Some(CaptureReason::InvalidHistoryMarker));assert!(!a.reads.contains(&13));}
    for data in [Ok(vec![0;4]),Ok(vec![1,0,0,0]),Ok(vec![2,0,0,0]),Err(CaptureReason::ReadFailed)]{let mut a=Api::new();a.marker(CLOUD,data);assert_eq!(run(&mut a).result,CaptureResult::Captured);}
}

#[test]
fn retries_keep_identity_five_attempts_and_close_each_successful_open(){
    let mut a=Api::new();a.busy=4;let mut clock=FakeClock::default();let r=capture_request(&request(),&mut a,&mut clock);
    assert_eq!(r.result,CaptureResult::Captured);assert_eq!(r.attempts,5);assert_eq!(r.capture_id,"capture-1");assert_eq!(clock.sleeps,vec![20,40,80,160]);assert_eq!(a.closes,1);
    let mut a=Api::new();a.busy=100;let r=capture_request(&request(),&mut a,&mut clock);assert_eq!(r.attempts,5);assert_eq!(r.reason,Some(CaptureReason::Busy));
    let mut a=Api::new();assert_eq!(run(&mut a).attempts,1);
}

#[test]
fn zero_and_unstable_sequence_never_mix_formats_and_wrap_is_equality_only(){
    let mut a=Api::new();a.last=0;assert_eq!(run(&mut a).reason,Some(CaptureReason::ZeroSequence));assert!(a.reads.is_empty());
    let mut a=Api::new();a.seq=VecDeque::from([u32::MAX,u32::MAX,1,1,1,1]);let r=run(&mut a);assert_eq!(r.result,CaptureResult::Captured);assert_eq!(r.sequence_before,1);assert_eq!(r.sequence_after,1);assert_eq!(r.attempts,2);assert_eq!(a.closes,2);
    let mut a=Api::new();a.formats[0].2=Err(CaptureReason::ReadFailed);assert_eq!(run(&mut a).result,CaptureResult::Failed);assert!(!a.opened);assert_eq!(a.closes,1);
}

#[test]
fn snapshot_protocol_accepts_real_bytes_and_rejects_ack_and_invalid_fields(){
    use pinfeather_lib::clipboard::protocol::{decode_snapshot,encode_snapshot,split_response_frame};
    let r=run(&mut Api::new());let frame=encode_snapshot(&r).unwrap();
    let decoded=decode_snapshot(split_response_frame(&frame).unwrap(),&request()).unwrap();assert_eq!(decoded.payloads,vec![text("same")]);assert!(decoded.response.formats.is_empty());
    let body=serde_json::to_vec(&serde_json::json!({"version":1,"capture_id":"capture-1","policy_epoch":3})).unwrap();assert!(decode_snapshot(&body,&request()).is_err());
    for change in ["kind","version","capture_id","policy_epoch","sequence_after","base64","unknown","owner","attempts","result"] {
        let mut v=serde_json::to_value(&r).unwrap();match change{
            "kind"=>v["kind"]="ack".into(),"version"=>v["version"]=2.into(),"capture_id"=>v["capture_id"]="wrong".into(),"policy_epoch"=>v["policy_epoch"]=4.into(),"sequence_after"=>v["sequence_after"]=0.into(),"base64"=>v["formats"][0]["data"]="!bad".into(),"unknown"=>v["extra"]=true.into(),"owner"=>v["owner"]["process"]="orphan".into(),"attempts"=>v["attempts"]=6.into(),_=>v["result"]="invented".into()};
        assert!(decode_snapshot(&serde_json::to_vec(&v).unwrap(),&request()).is_err(),"{change}");
    }
}

fn decoded()->pinfeather_lib::clipboard::protocol::ValidatedSnapshot{let r=run(&mut Api::new());pinfeather_lib::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&r).unwrap(),&request()).unwrap()}
#[test]
fn paused_notifications_duplicates_wrap_and_notification_overrun_are_explicit(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("run");assert!(s.observe(1,0).is_none());assert!(s.begin_current(1).is_none());
    s.set_enabled(true);let first=s.observe(u32::MAX,1).unwrap();assert_eq!(s.observe(u32::MAX,2).unwrap(),first);let next=s.observe(1,3).unwrap();assert_ne!(first,next);assert_eq!(s.notification_count(),2);
    for n in 2..=255 {s.observe(n,4);}assert_eq!(s.notification_count(),256);let epoch=s.epoch;assert!(s.observe(256,4).is_none());assert_eq!(s.admission,Admission::Overloaded);assert_eq!(s.counters.overloaded,1);assert_eq!(s.epoch,epoch);assert!(s.begin_current(1).is_none());
}

#[test]
fn snapshot_queue_is_fifo_bounded_and_pause_cancels_old_epoch(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("run");s.set_enabled(true);
    for n in 1..=65 {s.observe(n,1);let ticket=s.begin_current(n).unwrap();let mut snap=decoded();snap.response.capture_id=ticket.message.capture_id.clone();snap.response.policy_epoch=ticket.message.policy_epoch;s.complete(ticket,snap);}
    assert_eq!(s.queue_usage().0,64);assert_eq!(s.admission,Admission::Overloaded);let first=s.pop_queued().unwrap();assert!(first.ticket.message.capture_id.ends_with("-1"));assert_eq!(s.queue_usage().0,63);
    s.set_enabled(true);s.observe(500,1);let ticket=s.begin_current(500).unwrap();let mut snap=decoded();snap.response.capture_id=ticket.message.capture_id.clone();snap.response.policy_epoch=ticket.message.policy_epoch;s.set_enabled(false);s.complete(ticket,snap);assert_eq!(s.queue_usage(),(0,0));assert!(s.counters.cancelled>0);
}

#[test]
fn every_format_at_byte_limit_and_one_byte_over_is_independently_checked(){
    use pinfeather_lib::contracts::{MAX_TEXT_FORMAT_BYTES,MAX_PNG_BYTES,MAX_PAYLOAD_TOTAL_BYTES};
    let mut unicode=vec![65u8;MAX_TEXT_FORMAT_BYTES];unicode[MAX_TEXT_FORMAT_BYTES-2..].fill(0);
    let mut png=vec![0;MAX_PNG_BYTES];png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let payloads=vec![Payload{format_name:"CF_UNICODETEXT".into(),data:unicode},Payload{format_name:"HTML Format".into(),data:vec![65;MAX_TEXT_FORMAT_BYTES]},Payload{format_name:"PNG".into(),data:png}];
    assert_eq!(payloads.iter().map(|p|p.data.len()).sum::<usize>(),MAX_PAYLOAD_TOTAL_BYTES);assert_eq!(validate_payloads(&payloads),Ok(()));
    for p in &payloads {let mut over=p.clone();over.data.push(0);assert_eq!(validate_payloads(&[over]),Err(CaptureReason::InvalidPayload));}
    assert!(validate_payloads(&[text("a"),text("a")]).is_err());assert!(validate_payloads(&[Payload{format_name:"unknown".into(),data:vec![]}]).is_err());
}

fn complete_for(s:&mut pinfeather_lib::runtime::CaptureState,sequence:u32,mut snapshot:pinfeather_lib::clipboard::protocol::ValidatedSnapshot){
    s.observe(sequence,0);let ticket=s.begin_current(sequence).unwrap();snapshot.response.capture_id=ticket.message.capture_id.clone();snapshot.response.policy_epoch=ticket.message.policy_epoch;s.complete(ticket,snapshot);
}

#[test]
fn snapshot_byte_limit_overload_preserves_fifo_epoch_and_user_recovery(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("bytes");s.set_enabled(true);
    for n in 1..=4 {let mut snapshot=decoded();snapshot.payloads=vec![Payload{format_name:"PNG".into(),data:vec![0;8*1024*1024]}];complete_for(&mut s,n,snapshot);}
    assert_eq!(s.queue_usage(),(4,32*1024*1024));let epoch=s.epoch;complete_for(&mut s,5,decoded());assert_eq!(s.admission,Admission::Overloaded);assert_eq!(s.epoch,epoch);assert_eq!(s.queue_usage(),(4,32*1024*1024));
    s.set_enabled(true);assert_eq!(s.admission,Admission::Overloaded);s.pop_queued().unwrap();s.set_enabled(true);assert_eq!(s.admission,Admission::Active);assert_eq!(s.epoch,epoch);assert_eq!(s.queue_usage(),(3,24*1024*1024));
}

#[test]
fn owner_exclusions_unknown_source_and_same_text_distinct_captures(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("owner");s.set_enabled(true);complete_for(&mut s,1,decoded());complete_for(&mut s,2,decoded());
    let first=s.pop_queued().unwrap();let second=s.pop_queued().unwrap();assert_ne!(first.ticket.message.capture_id,second.ticket.message.capture_id);assert_eq!(first.snapshot.payloads,second.snapshot.payloads);
    s.set_exclusions(vec!["C:\\Synthetic\\excluded.exe".into()]);complete_for(&mut s,3,decoded());assert_eq!(s.queue_usage().0,0);assert_eq!(s.counters.ignored,1);
    let mut known=decoded();known.response.owner=Owner{hwnd:Some("7".into()),pid:Some(42),process:Some("c:\\synthetic\\EXCLUDED.exe".into())};complete_for(&mut s,4,known);assert_eq!(s.counters.ignored,2);
    let mut known=decoded();known.response.owner=Owner{hwnd:Some("7".into()),pid:Some(42),process:Some("C:\\Synthetic\\allowed.exe".into())};complete_for(&mut s,5,known);assert_eq!(s.queue_usage().0,1);
}

#[test]
fn busy_capture_does_not_block_observation_and_policy_change_cancels_late_reply(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("busy");s.set_enabled(true);s.observe(1,0);let ticket=s.begin_current(1).unwrap();assert!(s.begin_current(2).is_none());s.observe(2,1);s.observe(3,2);assert_eq!(s.notification_count(),2);
    let mut snapshot=decoded();snapshot.response.capture_id=ticket.message.capture_id.clone();snapshot.response.policy_epoch=ticket.message.policy_epoch;
    s.set_exclusions(vec!["C:\\blocked.exe".into()]);s.complete(ticket,snapshot);assert_eq!(s.counters.cancelled,3);assert_eq!(s.queue_usage().0,0);assert!(s.begin_current(3).is_none());
}

#[test]
fn invalid_internal_marker_cannot_suppress_external_snapshot_and_omitted_formats_are_bounded(){
    let mut a=Api::new();a.marker(INTERNAL,Ok(vec![0;28]));let r=run(&mut a);assert_eq!(r.result,CaptureResult::Captured);let d=pinfeather_lib::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&r).unwrap(),&request()).unwrap();assert_eq!(d.internal_origin,Some(vec![0;28]));
    let mut a=Api::new();a.marker(INTERNAL,Err(CaptureReason::ReadFailed));assert_eq!(run(&mut a).result,CaptureResult::Captured);
    let mut a=Api::new();a.formats.push((999,"unknown-raw".into(),Err(CaptureReason::ReadFailed)));let r=run(&mut a);assert_eq!(r.omitted_formats,vec!["unknown-raw"]);assert!(!a.reads.contains(&999));
}

#[test]
fn covered_notifications_are_superseded_without_recapturing_current_version(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("versions");s.set_enabled(true);for n in [u32::MAX,1,2]{s.observe(n,0);}
    let ticket=s.begin_current(2).unwrap();assert_eq!(ticket.sequence,2);assert_eq!(s.notification_count(),0);assert_eq!(s.counters.superseded,2);
    let mut snapshot=decoded();snapshot.response.capture_id=ticket.message.capture_id.clone();snapshot.response.policy_epoch=ticket.message.policy_epoch;snapshot.response.sequence_before=2;snapshot.response.sequence_locked=2;snapshot.response.sequence_after=2;s.complete(ticket,snapshot);
    s.observe(2,1);assert!(s.begin_current(2).is_none());assert_eq!(s.queue_usage().0,1);assert_eq!(s.counters.superseded,2);
}

#[test]
fn sequence_value_reappearing_after_observed_change_is_a_new_capture(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("wrap");s.set_enabled(true);s.observe(u32::MAX,0);let ticket=s.begin_current(u32::MAX).unwrap();let first=ticket.message.capture_id.clone();
    let mut snapshot=decoded();snapshot.response.capture_id=ticket.message.capture_id.clone();snapshot.response.policy_epoch=ticket.message.policy_epoch;snapshot.response.sequence_before=u32::MAX;snapshot.response.sequence_locked=u32::MAX;snapshot.response.sequence_after=u32::MAX;s.complete(ticket,snapshot);
    s.observe(1,1);s.observe(u32::MAX,2);let ticket=s.begin_current(u32::MAX).expect("same numeric sequence after a real observed transition cannot reuse permanent identity");assert_ne!(ticket.message.capture_id,first);
}

#[test]
fn slow_read_uses_original_deadline_and_clears_all_payloads(){
    use std::{rc::Rc,cell::Cell};
    struct Time(Rc<Cell<u64>>);impl Clock for Time{fn now_ms(&self)->u64{self.0.get()}fn sleep_ms(&mut self,ms:u64){self.0.set(self.0.get()+ms)}}
    struct Slow(Api,Rc<Cell<u64>>);impl ClipboardApi for Slow{
        fn sequence(&mut self)->u32{self.0.sequence()}fn open(&mut self)->bool{self.0.open()}fn close(&mut self)->Result<(),CaptureReason>{self.0.close()}fn owner(&mut self)->Owner{self.0.owner()}fn formats(&mut self)->Result<Vec<(u32,String)>,CaptureReason>{self.0.formats()}
        fn read(&mut self,id:u32,limit:usize)->Result<Vec<u8>,CaptureReason>{self.1.set(760);self.0.read(id,limit)}
    }
    let time=Rc::new(Cell::new(0));let mut api=Slow(Api::new(),time.clone());let response=capture_request(&request(),&mut api,&mut Time(time));assert_eq!(response.reason,Some(CaptureReason::Deadline));assert!(response.formats.is_empty());assert_eq!(api.0.closes,1);
}

#[test]
fn failed_clipboard_close_discards_snapshot_and_is_not_retried(){
    let mut a=Api::new();a.close_fails=true;let r=run(&mut a);assert_eq!(r.result,CaptureResult::Failed);assert_eq!(r.reason,Some(CaptureReason::CloseFailed));assert!(r.formats.is_empty());assert_eq!(r.attempts,1);
}

#[test]
fn registered_runtime_ids_keep_privacy_semantics_regardless_of_producer_case(){
    use pinfeather_lib::clipboard::windows::RegisteredFormats;
    let names=["html format","png","excludeclipboardcontentfrommonitorprocessing","canincludeinclipboardhistory","canuploadtocloudclipboard","clipboard viewer ignore","pinfeather.internalorigin.v1"];
    let formats=RegisteredFormats::register(|name|0xc000+names.iter().position(|n|n.eq_ignore_ascii_case(name)).unwrap() as u32).unwrap();
    assert_eq!(formats.canonical_name(0xc002),Some(EXCLUDE));assert_eq!(formats.canonical_name(0xc003),Some(HISTORY));assert_eq!(formats.canonical_name(13),Some("CF_UNICODETEXT"));assert_eq!(formats.canonical_name(0xc100),None);
    assert!(RegisteredFormats::register(|_|0).is_err());assert!(RegisteredFormats::register(|_|0xc000).is_err());
}

#[test]
fn zero_sequence_breaks_previous_identity_until_a_valid_observation_returns(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("zero");s.set_enabled(true);let first=s.observe(7,0).unwrap();assert!(s.observe(0,1).is_none());let after=s.observe(7,2).unwrap();assert_ne!(first,after,"zero cannot prove continuity with the earlier observation");assert_eq!(s.counters.failed,1);assert_eq!(s.notice,Some(Notice::ZeroSequence));
}

#[test]
fn excluded_owner_path_uses_windows_unicode_case_comparison(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("unicode-owner");s.set_enabled(true);s.set_exclusions(vec!["C:\\研究\\ÄPP.exe".into()]);
    let mut snapshot=decoded();snapshot.response.owner=Owner{hwnd:Some("7".into()),pid:Some(42),process:Some("c:\\研究\\äpp.exe".into())};complete_for(&mut s,1,snapshot);assert_eq!(s.queue_usage().0,0);assert_eq!(s.notice,Some(Notice::ExcludedOwner));
}

#[test]
fn failure_after_user_pause_settles_old_request_as_cancelled(){
    use pinfeather_lib::runtime::*;
    let mut s=CaptureState::new("cancel");s.set_enabled(true);s.observe(1,0);let ticket=s.begin_current(1).unwrap();s.set_enabled(false);s.capture_failed(&ticket,false);assert_eq!(s.counters.cancelled,1);assert_eq!(s.counters.failed,0);assert_eq!(s.admission,Admission::UserPaused);
}

#[test]
fn history_denied_internal_write_keeps_only_origin_metadata_for_parent_classification(){
    let mut a=Api::new();a.marker(INTERNAL,Ok(vec![1;28]));a.marker(HISTORY,Ok(vec![0;4]));a.owner=Owner{hwnd:Some("99".into()),pid:Some(7),process:None};
    let response=run(&mut a);assert_eq!(response.reason,Some(CaptureReason::HistoryDenied));assert!(response.internal_origin.is_some());assert_eq!(response.owner.hwnd.as_deref(),Some("99"));assert!(response.formats.is_empty());assert!(!a.reads.contains(&13));
    let decoded=pinfeather_lib::clipboard::protocol::decode_snapshot(&serde_json::to_vec(&response).unwrap(),&request()).unwrap();assert_eq!(decoded.internal_origin,Some(vec![1;28]));
}
