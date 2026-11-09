//! Opt-in QPC evidence. No clipboard, worker, SQL, UI dispatch or default output.
use std::{collections::HashMap, io::Write, path::{Path,PathBuf}, sync::{Arc,Mutex}};
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Copy,Serialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum MeasureError { Disabled, InvalidContract, WrongWindow, Stale, NotInteractive, Expired, Clock, Output, Limit, Stopped }
pub type Result<T> = std::result::Result<T,MeasureError>;
fn required_session<'de,D:serde::Deserializer<'de>>(deserializer:D)->std::result::Result<Option<String>,D::Error>{Option::<String>::deserialize(deserializer)}
pub trait Clock: Send+Sync { fn now(&self)->Result<u64>; fn frequency(&self)->Result<u64>; }
pub struct Qpc;
impl Clock for Qpc {
    fn now(&self)->Result<u64>{let mut value=0; if unsafe{windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value)}==0||value<=0{Err(MeasureError::Clock)}else{Ok(value as u64)}}
    fn frequency(&self)->Result<u64>{let mut value=0;if unsafe{windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut value)}==0||value<=0{Err(MeasureError::Clock)}else{Ok(value as u64)}}
}
pub fn decimal(value:&str,zero:bool)->Result<u64>{
    if value.is_empty()||value.len()>20||!value.bytes().all(|b|b.is_ascii_digit())||(value.len()>1&&value.starts_with('0')){return Err(MeasureError::InvalidContract);}
    let n=value.parse::<u64>().map_err(|_|MeasureError::InvalidContract)?;if n==0&&!zero{return Err(MeasureError::InvalidContract);}Ok(n)
}
#[derive(Clone,Debug)]
pub struct MeasureArgs {pub run_id:String,pub output:PathBuf,pub cold:u64,pub frequency:u64}
impl MeasureArgs {
    pub fn parse(run_id:&str,output:&str,cold:&str,frequency:&str)->Result<Self>{
        if run_id.is_empty()||run_id.len()>64||!run_id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-'){return Err(MeasureError::InvalidContract);}
        let path=PathBuf::from(output);
        let parent=path.parent().ok_or(MeasureError::InvalidContract)?;
        let file=path.file_name().and_then(|x|x.to_str()).ok_or(MeasureError::InvalidContract)?;
        let normalized=crate::contracts::normalize_windows_dir_path(parent.to_str().ok_or(MeasureError::InvalidContract)?).map_err(|_|MeasureError::InvalidContract)?;
        if Path::new(&normalized).join(file).as_os_str()!=path.as_os_str()||!path.is_absolute()||file.ends_with(['.',' '])||file.contains([':', '*','?','<','>','|','"']){return Err(MeasureError::InvalidContract);}
        Ok(Self{run_id:run_id.into(),output:path,cold:decimal(cold,false)?,frequency:decimal(frequency,false)?})
    }
    pub fn open(&self,data:&Path,clock:&dyn Clock)->Result<MeasureHost>{
        let now=clock.now()?;if clock.frequency()?!=self.frequency||self.cold>now{return Err(MeasureError::Clock);}
        // Inspect ancestors only; never recurse or resolve through reparse points.
        use std::os::windows::fs::MetadataExt;
        for p in self.output.parent().ok_or(MeasureError::InvalidContract)?.ancestors(){
            let m=std::fs::symlink_metadata(p).map_err(|_|MeasureError::Output)?;
            if !m.is_dir()||m.file_attributes()&0x400!=0{return Err(MeasureError::Output);}
        }
        for p in data.ancestors(){if let Ok(m)=std::fs::symlink_metadata(p){if m.file_attributes()&0x400!=0{return Err(MeasureError::Output);}}}
        let lower=|p:&Path|p.to_string_lossy().trim_end_matches('\\').to_lowercase();
        let out=lower(&self.output);let root=lower(data);
        if out==root||out.starts_with(&(root+"\\")){return Err(MeasureError::Output);}
        let file=std::fs::OpenOptions::new().write(true).create_new(true).open(&self.output).map_err(|_|MeasureError::Output)?;
        MeasureHost::new(self.clone(),Arc::new(Qpc),Box::new(file))
    }
}

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Identity {pub schema:u8,pub run_id:String,pub attempt:String,pub window:String,#[serde(deserialize_with="required_session")]pub session:Option<String>,pub model:String}
impl Identity {fn validate(&self)->Result<()> {if self.schema!=1||self.run_id.is_empty()||self.run_id.len()>64||!self.run_id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')||self.session.as_ref().is_some_and(|s|s.len()!=32||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err(MeasureError::InvalidContract);}for x in [&self.attempt,&self.window,&self.model]{decimal(x,false)?;}Ok(())}}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum Content {Empty,Nonempty}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {pub schema:u8,pub run_id:String,pub attempt:String,pub window:String,#[serde(deserialize_with="required_session")]pub session:Option<String>,pub model:String,pub generation:String,pub sequence:String,pub receipt:String,pub content:Content}
impl Receipt {
    pub fn identity(&self)->Identity{Identity{schema:self.schema,run_id:self.run_id.clone(),attempt:self.attempt.clone(),window:self.window.clone(),session:self.session.clone(),model:self.model.clone()}}
    pub fn validate(&self)->Result<()>{self.identity().validate()?;decimal(&self.generation,true)?;decimal(&self.sequence,false)?;decimal(&self.receipt,false)?;Ok(())}
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {pub schema:u8}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRequest {pub schema:u8,pub run_id:String,pub attempt:String,pub window:String,#[serde(deserialize_with="required_session")]pub session:Option<String>,pub model:String,pub generation:String,pub sequence:String}
impl VersionRequest {
    fn identity(&self)->Identity{Identity{schema:self.schema,run_id:self.run_id.clone(),attempt:self.attempt.clone(),window:self.window.clone(),session:self.session.clone(),model:self.model.clone()}}
    fn version(&self)->Result<(u64,u64)>{self.identity().validate()?;Ok((decimal(&self.generation,true)?,decimal(&self.sequence,false)?))}
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {pub schema:u8,pub run_id:String,pub attempt:String,pub window:String,#[serde(deserialize_with="required_session")]pub session:Option<String>,pub model:String,pub generation:String,pub sequence:String,pub query_request:serde_json::Value}
impl ListRequest {pub fn version(&self)->VersionRequest{VersionRequest{schema:self.schema,run_id:self.run_id.clone(),attempt:self.attempt.clone(),window:self.window.clone(),session:self.session.clone(),model:self.model.clone(),generation:self.generation.clone(),sequence:self.sequence.clone()}}}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadyRequest {pub schema:u8,pub run_id:String,pub attempt:String,pub window:String,#[serde(deserialize_with="required_session")]pub session:Option<String>,pub model:String,pub generation:String,pub sequence:String,pub receipt:String,pub content:Content,pub input_focused:bool}
impl ReadyRequest {pub fn receipt(&self)->Receipt{Receipt{schema:self.schema,run_id:self.run_id.clone(),attempt:self.attempt.clone(),window:self.window.clone(),session:self.session.clone(),model:self.model.clone(),generation:self.generation.clone(),sequence:self.sequence.clone(),receipt:self.receipt.clone(),content:self.content.clone()}}}
#[derive(Serialize)]
pub struct MeasuredPage {pub result:crate::contracts::ListClipsResult,pub receipt:Option<Receipt>}
#[derive(Clone,Copy)]
pub struct NativeState {pub exists:bool,pub visible:bool,pub minimized:bool,pub focused:bool}
impl NativeState {fn interactive(self)->bool{self.exists&&self.visible&&!self.minimized&&self.focused}}

#[derive(Serialize)]
struct Event<'a> {
    schema:u8,run_id:&'a str,sequence:String,event:&'a str,qpc:String,
    #[serde(skip_serializing_if="Option::is_none")] frequency:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] cold_start_qpc:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] attempt:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] window:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] model:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] result:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")] content:Option<Content>,
    #[serde(skip_serializing_if="Option::is_none")] reason:Option<&'a str>,
    #[serde(skip_serializing_if="Option::is_none")] input_focused:Option<bool>,
    #[serde(skip_serializing_if="Option::is_none")] complete:Option<bool>,
}
struct Attempt {id:u64,window:u64,start:u64,session:Option<String>,model:Option<Identity>,version:Option<(u64,u64)>,requested:Option<(u64,u64)>,receipt:Option<Receipt>,pending:Option<(Receipt,bool,u64)>,terminal:bool}
struct State {writer:Box<dyn Write+Send>,events:u64,last:u64,next:u64,failed:bool,ended:bool,native:Option<bool>,attempts:HashMap<&'static str,Attempt>}
pub struct MeasureHost {args:MeasureArgs,clock:Arc<dyn Clock>,state:Mutex<State>}
#[derive(Clone,Default)]
pub struct Measurement(pub Option<Arc<MeasureHost>>);
impl Measurement {
    pub fn enabled(&self)->Result<&Arc<MeasureHost>>{self.0.as_ref().ok_or(MeasureError::Disabled)}
    pub fn invalidate_window(&self,label:&str,reason:&'static str){if let Some(h)=&self.0{let _=h.invalidate_window(label,reason);}}
    pub fn end(&self){if let Some(h)=&self.0{let _=h.end();}}
    pub fn fail(&self){if let Some(h)=&self.0{let mut s=h.state.lock().unwrap();if let Ok(now)=h.now(&mut s){let _=h.write(&mut s,now,"invalid",None,Some("hotkey_failed"),None,None);let _=h.write(&mut s,now,"run_end",None,None,None,Some(false));s.ended=true;}}}
    pub fn script(&self)->&'static str{if self.0.is_some(){"Object.defineProperty(globalThis,'__PINFEATHER_MEASUREMENT__',{value:Object.freeze({schema:1,enabled:true}),writable:false,configurable:false});"}else{""}}
}
impl MeasureHost {
    pub fn new(args:MeasureArgs,clock:Arc<dyn Clock>,writer:Box<dyn Write+Send>)->Result<Self>{
        let now=clock.now()?;if args.frequency!=clock.frequency()?||args.cold>now{return Err(MeasureError::Clock);}
        let host=Self{args,clock,state:Mutex::new(State{writer,events:0,last:now,next:3,failed:false,ended:false,native:None,attempts:HashMap::new()})};
        {let mut s=host.state.lock().unwrap();host.write(&mut s,now,"run_start",None,None,None,None)?;s.attempts.insert("main",Attempt{id:1,window:2,start:host.args.cold,session:None,model:None,version:None,requested:None,receipt:None,pending:None,terminal:false});}
        Ok(host)
    }
    fn write(&self,s:&mut State,qpc:u64,event:&str,association:Option<&Receipt>,reason:Option<&'static str>,input_focused:Option<bool>,complete:Option<bool>)->Result<()>{
        if s.failed{return Err(MeasureError::Output);}if s.ended{return Err(MeasureError::Stopped);}if s.events>=4096{s.failed=true;return Err(MeasureError::Limit);}
        s.events+=1;
        let e=Event{schema:1,run_id:&self.args.run_id,sequence:s.events.to_string(),event,qpc:qpc.to_string(),frequency:(event=="run_start").then(||self.args.frequency.to_string()),cold_start_qpc:(event=="run_start").then(||self.args.cold.to_string()),attempt:association.map(|r|r.attempt.clone()),window:association.map(|r|r.window.clone()),model:association.filter(|r|r.model!="0").map(|r|r.model.clone()),result:association.filter(|r|r.receipt!="0").map(|r|r.receipt.clone()),content:association.filter(|r|r.receipt!="0").map(|r|r.content.clone()),reason,input_focused,complete};
        let bytes=serde_json::to_vec(&e).map_err(|_|MeasureError::Output)?;
        if s.writer.write_all(&bytes).and_then(|_|s.writer.write_all(b"\n")).and_then(|_|s.writer.flush()).is_err(){s.failed=true;return Err(MeasureError::Output);}Ok(())
    }
    fn now(&self,s:&mut State)->Result<u64>{
        if s.failed{return Err(MeasureError::Output);}if s.ended{return Err(MeasureError::Stopped);}
        let now=match self.clock.now(){Ok(n) if n>=s.last=>n,_=>{s.failed=true;return Err(MeasureError::Clock)}};s.last=now;
        if now.saturating_sub(self.args.cold)>self.args.frequency.saturating_mul(1800)||s.events>=4090{
            let reason=if s.events>=4090{"event_limit"}else{"run_timeout"};
            self.write(s,now,"invalid",None,Some(reason),None,None)?;self.write(s,now,"run_end",None,None,None,Some(false))?;s.ended=true;
            return Err(if reason=="event_limit"{MeasureError::Limit}else{MeasureError::Expired});
        }Ok(now)
    }
    fn association(a:&Attempt)->Receipt {a.receipt.clone().unwrap_or_else(||{let i=a.model.clone().unwrap_or(Identity{schema:1,run_id:String::new(),attempt:a.id.to_string(),window:a.window.to_string(),session:a.session.clone(),model:"0".into()});Receipt{schema:1,run_id:i.run_id,attempt:i.attempt,window:i.window,session:i.session,model:i.model,generation:"0".into(),sequence:"0".into(),receipt:"0".into(),content:Content::Empty}})}
    fn invalid(&self,s:&mut State,label:&str,now:u64,reason:&'static str)->Result<()>{
        let association=if let Some(a)=s.attempts.get_mut(label){if a.terminal{return Ok(());}let r=Self::association(a);a.terminal=true;a.receipt=None;a.pending=None;Some(r)}else{None};
        if let Some(r)=association{self.write(s,now,"invalid",Some(&r),Some(reason),None,None)?;}Ok(())
    }
    fn check<'a>(&self,s:&'a mut State,label:&str,i:&Identity,now:u64)->Result<&'a mut Attempt>{
        i.validate()?;if i.run_id!=self.args.run_id{return Err(MeasureError::Stale);}
        let a=s.attempts.get_mut(label).ok_or(MeasureError::WrongWindow)?;
        if a.terminal||a.model.as_ref()!=Some(i){return Err(MeasureError::Stale);}
        if now.saturating_sub(a.start)>self.args.frequency.saturating_mul(30){self.invalid(s,label,now,"attempt_timeout")?;return Err(MeasureError::Expired);}
        Ok(s.attempts.get_mut(label).unwrap())
    }
    pub fn start_model(&self,label:&str)->Result<Identity>{
        let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;
        let id=s.next;s.next+=1;
        let a=s.attempts.get_mut(label).ok_or(MeasureError::WrongWindow)?;
        if a.terminal||(label=="quick-panel"&&a.session.is_none()){return Err(MeasureError::Stale);}
        let i=Identity{schema:1,run_id:self.args.run_id.clone(),attempt:a.id.to_string(),window:a.window.to_string(),session:a.session.clone(),model:id.to_string()};
        a.model=Some(i.clone());a.version=None;a.requested=None;a.receipt=None;a.pending=None;
        let r=Self::association(a);self.write(&mut s,now,"model_start",Some(&r),None,None,None)?;Ok(i)
    }
    pub fn invalidate(&self,label:&str,v:&VersionRequest)->Result<()>{
        let version=v.version()?;let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;let a=self.check(&mut s,label,&v.identity(),now)?;
        if a.version.is_some_and(|old|version.0<old.0||version.1<=old.1){return Err(MeasureError::Stale);}
        a.version=Some(version);a.requested=None;a.receipt=None;a.pending=None;Ok(())
    }
    pub fn stop_model(&self,label:&str,i:&Identity)->Result<()>{let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;let a=self.check(&mut s,label,i,now)?;a.model=None;a.receipt=None;a.pending=None;a.version=None;let r=Self::association(a);self.write(&mut s,now,"model_stop",Some(&r),None,None,None)}
    pub fn begin_list(&self,label:&str,v:&VersionRequest)->Result<()> {let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;let a=self.check(&mut s,label,&v.identity(),now)?;let version=v.version()?;if a.version!=Some(version)||a.requested==Some(version){return Err(MeasureError::Stale);}a.requested=Some(version);a.receipt=None;a.pending=None;Ok(())}
    pub fn finish_list(&self,label:&str,v:&VersionRequest,content:Content)->Result<Receipt>{
        let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;let result=s.next;s.next+=1;
        let a=self.check(&mut s,label,&v.identity(),now)?;if a.version!=Some(v.version()?)||a.requested!=a.version||a.receipt.is_some(){return Err(MeasureError::Stale);}
        let i=v.identity();let r=Receipt{schema:1,run_id:i.run_id,attempt:i.attempt,window:i.window,session:i.session,model:i.model,generation:v.generation.clone(),sequence:v.sequence.clone(),receipt:result.to_string(),content};a.receipt=Some(r.clone());
        self.write(&mut s,now,"result_receipt",Some(&r),None,None,None)?;Ok(r)
    }
    pub fn list(&self,label:&str,request:ListRequest,runtime:&crate::host::HostRuntime)->Result<MeasuredPage>{
        self.list_query(label,request,|query|runtime.list(query))
    }
    fn fail_list(&self,label:&str,v:&VersionRequest)->Result<()>{
        let identity=v.identity();let version=v.version()?;let mut s=self.state.lock().unwrap();
        if !s.attempts.get(label).is_some_and(|a|!a.terminal&&a.model.as_ref()==Some(&identity)&&a.version==Some(version)&&a.requested==Some(version)){return Ok(());}
        let now=self.now(&mut s)?;self.check(&mut s,label,&identity,now)?;
        self.invalid(&mut s,label,now,"query_failed")
    }
    // Keep parsing, request ownership and completion in the production path;
    // only the blocking query boundary is replaceable by deterministic tests.
    fn list_query(&self,label:&str,request:ListRequest,execute:impl FnOnce(crate::contracts::ListClipsRequest)->crate::contracts::ContractResult<crate::contracts::ListClipsResult>)->Result<MeasuredPage>{
        let v=request.version();let query=crate::contracts::parse_list_clips_request(&request.query_request).map_err(|_|MeasureError::InvalidContract)?;
        let first=query.cursor.is_none();if first{self.begin_list(label,&v)?;}else{let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;self.check(&mut s,label,&v.identity(),now)?;}
        let result=match execute(query){Ok(result)=>result,Err(_)=>{self.fail_list(label,&v)?;return Err(MeasureError::Stale)}};
        let receipt=if first{Some(self.finish_list(label,&v,if result.summaries.is_empty(){Content::Empty}else{Content::Nonempty})?)}else{None};Ok(MeasuredPage{result,receipt})
    }
    fn complete(&self,s:&mut State,label:&str,now:u64)->Result<()> {
        let a=s.attempts.get_mut(label).ok_or(MeasureError::WrongWindow)?;
        let Some((r,focused,ack))=a.pending.clone()else{return Ok(());};
        if a.terminal{return Err(MeasureError::Stale);}a.terminal=true;a.pending=None;
        if now.saturating_sub(a.start)>self.args.frequency.saturating_mul(30){a.terminal=false;self.invalid(s,label,now,"attempt_timeout")?;return Err(MeasureError::Expired);}
        self.write(s,now.max(ack),if label=="main"{"main_interactive_ready"}else{"panel_interactive_ready"},Some(&r),None,Some(focused),None)
    }
    pub fn ack(&self,label:&str,r:&Receipt,input_focused:bool,native:impl FnOnce()->Result<NativeState>)->Result<()> {
        r.validate()?;let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;
        let a=self.check(&mut s,label,&r.identity(),now)?;
        if a.receipt.as_ref()!=Some(r)||a.pending.is_some(){return Err(MeasureError::Stale);}
        if !self.probe_native(&mut s,label,now,native)?.interactive(){self.invalid(&mut s,label,now,"window_not_interactive")?;return Err(MeasureError::NotInteractive);}
        if label=="quick-panel"&&!input_focused{self.invalid(&mut s,label,now,"input_not_focused")?;return Err(MeasureError::NotInteractive);}
        s.attempts.get_mut(label).unwrap().pending=Some((r.clone(),input_focused,now));
        self.write(&mut s,now,"react_commit_ack",Some(r),None,Some(input_focused),None)?;
        if label!="main"||s.native==Some(true){self.complete(&mut s,label,now)?;}else if s.native==Some(false){self.invalid(&mut s,label,now,"native_entry_degraded")?;}Ok(())
    }
    pub fn native_ready(&self,ready:bool,native:impl FnOnce()->Result<NativeState>)->Result<()> {
        let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;
        if s.native.is_some(){return Err(MeasureError::Stale);}s.native=Some(ready);
        self.write(&mut s,now,if ready{"native_entry_ready"}else{"native_entry_degraded"},None,None,None,None)?;
        if !ready{self.invalid(&mut s,"main",now,"native_entry_degraded")?;}
        else if s.attempts.get("main").is_some_and(|a|a.pending.is_some()&&!a.terminal){
            if self.probe_native(&mut s,"main",now,native)?.interactive(){self.complete(&mut s,"main",now)?;}else{self.invalid(&mut s,"main",now,"window_not_interactive")?;}
        }Ok(())
    }
    fn probe_native(&self,s:&mut State,label:&str,now:u64,native:impl FnOnce()->Result<NativeState>)->Result<NativeState>{
        match native(){Ok(state)=>Ok(state),Err(error)=>{
            if matches!(error,MeasureError::Clock|MeasureError::Output){s.failed=true;}else{self.invalid(s,label,now,"native_probe_failed")?;}
            Err(error)
        }}
    }
    pub fn hotkey(&self,tick:u64,reason:Option<&'static str>)->Result<String>{
        let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;if tick<self.args.cold||tick>now{s.failed=true;return Err(MeasureError::Clock);}
        let id=s.next;s.next+=1;let r=Receipt{schema:1,run_id:self.args.run_id.clone(),attempt:id.to_string(),window:"3".into(),session:None,model:"0".into(),generation:"0".into(),sequence:"0".into(),receipt:"0".into(),content:Content::Empty};
        self.write(&mut s,tick,"native_hotkey_dequeued",Some(&r),None,None,None)?;
        if let Some(reason)=reason{self.write(&mut s,now,"invalid",Some(&r),Some(reason),None,None)?;}
        else{self.invalid(&mut s,"quick-panel",now,"replaced")?;s.attempts.insert("quick-panel",Attempt{id,window:3,start:tick,session:None,model:None,version:None,requested:None,receipt:None,pending:None,terminal:false});}Ok(id.to_string())
    }
    pub fn panel_open(&self,attempt:&str,tick:u64,session:String)->Result<()> {
        let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;let id=decimal(attempt,false)?;
        let a=s.attempts.get_mut("quick-panel").ok_or(MeasureError::Stale)?;
        if a.terminal||a.id!=id||a.start!=tick||now.saturating_sub(tick)>self.args.frequency.saturating_mul(30){return Err(MeasureError::Stale);}a.session=Some(session);Ok(())
    }
    pub fn panel_hiding(&self)->Result<()>{let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;if s.attempts.get("quick-panel").is_some_and(|a|a.session.is_some()){self.invalid(&mut s,"quick-panel",now,"hidden")?;}Ok(())}
    pub fn invalidate_window(&self,label:&str,reason:&'static str)->Result<()> {let mut s=self.state.lock().unwrap();let now=self.now(&mut s)?;self.invalid(&mut s,label,now,reason)}
    pub fn end(&self)->Result<()> {let mut s=self.state.lock().unwrap();if s.ended{return if s.failed{Err(MeasureError::Output)}else{Ok(())};}let now=self.now(&mut s)?;for label in ["main","quick-panel"]{self.invalid(&mut s,label,now,"run_closed")?;}self.write(&mut s,now,"run_end",None,None,None,Some(true))?;s.ended=true;Ok(())}
}

pub fn native_state(window:&tauri::WebviewWindow)->Result<NativeState>{
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow,IsWindowVisible,IsIconic,GetForegroundWindow};
    let hwnd=window.hwnd().map_err(|_|MeasureError::NotInteractive)?.0 as windows_sys::Win32::Foundation::HWND;
    // Direct Win32 probes: no event-loop dispatch while the measurement lock is held.
    unsafe{Ok(NativeState{exists:IsWindow(hwnd)!=0,visible:IsWindowVisible(hwnd)!=0,minimized:IsIconic(hwnd)!=0,focused:GetForegroundWindow()==hwnd})}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64,AtomicBool,Ordering};
    #[derive(Clone)]struct TestClock(Arc<AtomicU64>);
    impl Clock for TestClock {fn now(&self)->Result<u64>{Ok(self.0.load(Ordering::SeqCst))}fn frequency(&self)->Result<u64>{Ok(10)}}
    #[derive(Clone)]struct Sink {bytes:Arc<Mutex<Vec<u8>>>,fail:Arc<AtomicBool>,flush_fail:Arc<AtomicBool>}
    impl Write for Sink {fn write(&mut self,b:&[u8])->std::io::Result<usize>{if self.fail.load(Ordering::SeqCst){return Err(std::io::Error::other("synthetic write failure"));}self.bytes.lock().unwrap().extend_from_slice(b);Ok(b.len())}fn flush(&mut self)->std::io::Result<()>{if self.flush_fail.load(Ordering::SeqCst){Err(std::io::Error::other("synthetic flush failure"))}else{Ok(())}}}
    fn fixture()->(MeasureHost,TestClock,Sink){let c=TestClock(Arc::new(AtomicU64::new(10)));let out=Sink{bytes:Arc::new(Mutex::new(Vec::new())),fail:Arc::new(AtomicBool::new(false)),flush_fail:Arc::new(AtomicBool::new(false))};let args=MeasureArgs{run_id:"fixture".into(),output:PathBuf::new(),cold:1,frequency:10};(MeasureHost::new(args,Arc::new(c.clone()),Box::new(out.clone())).unwrap(),c,out)}
    fn events(s:&Sink)->Vec<serde_json::Value>{String::from_utf8(s.bytes.lock().unwrap().clone()).unwrap().lines().map(|x|serde_json::from_str(x).unwrap()).collect()}
    fn good()->Result<NativeState>{Ok(NativeState{exists:true,visible:true,minimized:false,focused:true})}
    fn version(i:&Identity,generation:u64,sequence:u64)->VersionRequest{let mut value=serde_json::to_value(i).unwrap();value["generation"]=generation.to_string().into();value["sequence"]=sequence.to_string().into();serde_json::from_value(value).unwrap()}
    fn page(h:&MeasureHost,label:&str,i:&Identity,g:u64,s:u64,content:Content)->Receipt{let v=version(i,g,s);h.invalidate(label,&v).unwrap();h.begin_list(label,&v).unwrap();h.finish_list(label,&v,content).unwrap()}
    fn list_request(i:&Identity,g:u64,s:u64,paginated:bool)->ListRequest{
        let mut request=serde_json::to_value(i).unwrap();request["generation"]=g.to_string().into();request["sequence"]=s.to_string().into();
        request["query_request"]=serde_json::json!({"request_id":"boundary","query":"","limit":50,"cursor":if paginated{serde_json::json!({"normalized_query":"","upper_id":"9","last_id":"1","last_captured_at_ms":1})}else{serde_json::Value::Null}});
        serde_json::from_value(request).unwrap()
    }
    fn replaced_query_failure(replacement:&str,paginated:bool){
        let(h,_,out)=fixture();let attempt=h.hotkey(10,None).unwrap();h.panel_open(&attempt,10,"a".repeat(32)).unwrap();let i=h.start_model("quick-panel").unwrap();
        if paginated{page(&h,"quick-panel",&i,0,1,Content::Nonempty);}else{h.invalidate("quick-panel",&version(&i,0,1)).unwrap();}
        let mut successor=None;
        let result=h.list_query("quick-panel",list_request(&i,0,1,paginated),|_|{
            let fresh=match replacement{
                "attempt"=>{let b=h.hotkey(10,None).unwrap();h.panel_open(&b,10,"b".repeat(32)).unwrap();h.start_model("quick-panel").unwrap()},
                "model"=>{h.stop_model("quick-panel",&i).unwrap();h.start_model("quick-panel").unwrap()},
                _=>i.clone(),
            };
            successor=Some(page(&h,"quick-panel",&fresh,if replacement=="generation"{1}else{0},if fresh==i{2}else{1},Content::Nonempty));
            Err(crate::contracts::ContractError::protocol_error("synthetic query failure"))
        });
        assert!(matches!(result,Err(MeasureError::Stale)));
        let r=successor.unwrap();
        assert_eq!(h.ack("quick-panel",&r,true,good),Ok(()),"obsolete query failure must preserve successor receipt");
        let e=events(&out);assert!(!e.iter().any(|e|e["reason"]=="query_failed"),"obsolete failure must not emit query_failed");
        assert_eq!(e.iter().filter(|e|e["event"]=="panel_interactive_ready"&&e["result"]==r.receipt).count(),1);
    }
    #[test]fn measurement_obsolete_attempt_query_failure(){replaced_query_failure("attempt",false)}
    #[test]fn measurement_obsolete_model_query_failure(){replaced_query_failure("model",false)}
    #[test]fn measurement_obsolete_refresh_query_failure(){replaced_query_failure("refresh",false)}
    #[test]fn measurement_obsolete_generation_query_failure(){replaced_query_failure("generation",false)}
    #[test]fn measurement_obsolete_paginated_attempt_query_failure(){replaced_query_failure("attempt",true)}
    #[test]fn measurement_obsolete_paginated_model_query_failure(){replaced_query_failure("model",true)}
    #[test]fn measurement_obsolete_paginated_refresh_query_failure(){replaced_query_failure("refresh",true)}
    #[test]fn measurement_obsolete_paginated_generation_query_failure(){replaced_query_failure("generation",true)}
    #[test]fn measurement_current_query_failure_is_terminal(){for paginated in [false,true]{
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let receipt=if paginated{Some(page(&h,"main",&i,0,1,Content::Nonempty))}else{h.invalidate("main",&version(&i,0,1)).unwrap();None};
        assert!(matches!(h.list_query("main",list_request(&i,0,1,paginated),|_|Err(crate::contracts::ContractError::protocol_error("synthetic"))),Err(MeasureError::Stale)));
        if let Some(r)=receipt{assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Stale));}
        h.native_ready(true,good).unwrap();h.end().unwrap();let e=events(&out);assert_eq!(e.iter().filter(|e|e["event"]=="invalid"&&e["reason"]=="query_failed").count(),1);assert!(!e.iter().any(|e|e["event"]=="main_interactive_ready"));
    }}
    fn rejected_ack_is_terminal(probe_failure:bool){
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);
        assert_eq!(h.ack("main",&r,false,||if probe_failure{Err(MeasureError::NotInteractive)}else{Ok(NativeState{exists:true,visible:true,minimized:false,focused:false})}),Err(MeasureError::NotInteractive));
        let e=events(&out);let invalid=e.iter().filter(|e|e["event"]=="invalid").collect::<Vec<_>>();
        assert_eq!(invalid.len(),1,"rejected native readiness must immediately record terminal invalid");
        assert_eq!(invalid[0]["reason"],if probe_failure{"native_probe_failed"}else{"window_not_interactive"});assert_eq!(invalid[0]["result"],r.receipt);
        assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Stale));h.native_ready(true,good).unwrap();h.end().unwrap();
        let e=events(&out);assert_eq!(e.iter().filter(|e|e["event"]=="invalid").count(),1);assert!(!e.iter().any(|e|e["event"]=="react_commit_ack"||e["event"]=="main_interactive_ready"));
    }
    #[test]fn measurement_rejected_native_ack_is_terminal(){rejected_ack_is_terminal(false)}
    #[test]fn measurement_failed_native_probe_is_terminal(){rejected_ack_is_terminal(true)}
    #[test]fn measurement_pending_ack_probe_failure_is_terminal(){
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);h.ack("main",&r,false,good).unwrap();
        assert_eq!(h.native_ready(true,||Err(MeasureError::NotInteractive)),Err(MeasureError::NotInteractive));
        let e=events(&out);assert_eq!(e.iter().filter(|e|e["event"]=="invalid"&&e["reason"]=="native_probe_failed").count(),1);
        assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Stale));h.end().unwrap();assert!(!events(&out).iter().any(|e|e["event"]=="main_interactive_ready"));
    }
    #[test]fn measurement_native_probe_clock_error_remains_sticky(){
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);
        assert_eq!(h.ack("main",&r,false,||Err(MeasureError::Clock)),Err(MeasureError::Clock));
        assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Output));assert_eq!(h.end(),Err(MeasureError::Output));
        assert!(!events(&out).iter().any(|e|e["event"]=="invalid"||e["event"]=="react_commit_ack"||e["event"]=="main_interactive_ready"));
    }
    #[test]fn measurement_rejected_ack_does_not_hide_output_failure(){for flush in [false,true]{
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);
        if flush{out.flush_fail.store(true,Ordering::SeqCst);}else{out.fail.store(true,Ordering::SeqCst);}
        assert_eq!(h.ack("main",&r,false,||Err(MeasureError::NotInteractive)),Err(MeasureError::Output));
        out.fail.store(false,Ordering::SeqCst);out.flush_fail.store(false,Ordering::SeqCst);assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Output));assert_eq!(h.end(),Err(MeasureError::Output));
    }}
    #[test]fn measurement_wrong_or_stale_ack_never_probes_or_kills_current(){
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let old=page(&h,"main",&i,0,1,Content::Nonempty);let fresh=h.start_model("main").unwrap();let r=page(&h,"main",&fresh,0,1,Content::Nonempty);
        let mut forged=r.clone();forged.receipt="99999".into();let mut wrong_session=r.clone();wrong_session.session=Some("b".repeat(32));
        for invalid in [old,forged,wrong_session]{assert!(h.ack("main",&invalid,false,||panic!("obsolete ACK must not probe")).is_err());}
        assert!(h.ack("quick-panel",&r,true,||panic!("wrong window must not probe")).is_err());
        h.native_ready(true,good).unwrap();h.ack("main",&r,false,good).unwrap();assert!(!events(&out).iter().any(|e|e["event"]=="invalid"));
    }
    #[test]fn measurement_shared_strict_receipts(){let f:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/contracts/measurement.json")).unwrap();let parse=|v:serde_json::Value|serde_json::from_value::<Receipt>(v).ok().filter(|r|r.validate().is_ok());assert!(parse(f["receipt"].clone()).is_some());let i:Identity=serde_json::from_value(f["identity"].clone()).unwrap();i.validate().unwrap();for change in f["invalid_overrides"].as_array().unwrap(){let mut value=f["receipt"].clone();for(k,v)in change.as_object().unwrap(){value[k]=v.clone();}assert!(parse(value).is_none(),"{change}");}for key in f["receipt"].as_object().unwrap().keys(){let mut value=f["receipt"].clone();value.as_object_mut().unwrap().remove(key);assert!(parse(value).is_none(),"missing {key}");}}
    #[test]fn measurement_rejects_initial_empty_forged_and_old_versions(){
        let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let f:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/contracts/measurement.json")).unwrap();let fabricated:Receipt=serde_json::from_value(f["receipt"].clone()).unwrap();assert!(h.ack("main",&fabricated,false,good).is_err());
        let r=page(&h,"main",&i,0,1,Content::Nonempty);h.invalidate("main",&version(&i,0,2)).unwrap();assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Stale));assert!(h.begin_list("main",&version(&i,0,1)).is_err());assert!(h.finish_list("main",&version(&i,0,1),Content::Empty).is_err());assert!(events(&out).iter().all(|e|e["event"]!="main_interactive_ready"));
    }
    #[test]fn measurement_native_and_commit_both_orders_and_duplicate_ack(){for native_first in [false,true]{let(h,clock,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);if native_first{h.native_ready(true,good).unwrap();}h.ack("main",&r,false,good).unwrap();if !native_first{assert!(!events(&out).iter().any(|e|e["event"]=="main_interactive_ready"));clock.0.store(12,Ordering::SeqCst);h.native_ready(true,good).unwrap();}assert!(h.ack("main",&r,false,good).is_err());let e=events(&out);let ready=e.iter().filter(|e|e["event"]=="main_interactive_ready").collect::<Vec<_>>();assert_eq!(ready.len(),1);assert_eq!(ready[0]["input_focused"],false);if !native_first{assert_eq!(ready[0]["qpc"],"12");}h.end().unwrap();h.end().unwrap();assert_eq!(events(&out).iter().filter(|e|e["event"]=="run_end").count(),1);}}
    #[test]fn measurement_pending_ack_invalidated_before_native_ready(){for reason in ["hidden","destroyed","stopping","query_failed"]{let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Empty);h.ack("main",&r,true,good).unwrap();h.invalidate_window("main",reason).unwrap();h.native_ready(true,good).unwrap();assert!(!events(&out).iter().any(|e|e["event"]=="main_interactive_ready"));}}
    #[test]fn measurement_native_probe_hidden_minimized_unfocused_unknown_rejected(){for n in [NativeState{exists:false,visible:true,minimized:false,focused:true},NativeState{exists:true,visible:false,minimized:false,focused:true},NativeState{exists:true,visible:true,minimized:true,focused:true},NativeState{exists:true,visible:true,minimized:false,focused:false}]{let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);assert_eq!(h.ack("main",&r,false,||Ok(n)),Err(MeasureError::NotInteractive));assert!(!events(&out).iter().any(|e|e["event"]=="react_commit_ack"));}let(h,_,_)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Empty);assert_eq!(h.ack("main",&r,false,||Err(MeasureError::NotInteractive)),Err(MeasureError::NotInteractive));}
    #[test]fn measurement_models_and_query_replacements_do_not_reuse_receipts(){let(h,_,_)=fixture();let i=h.start_model("main").unwrap();let old=page(&h,"main",&i,0,1,Content::Nonempty);h.stop_model("main",&i).unwrap();let fresh=h.start_model("main").unwrap();assert_ne!(fresh.model,i.model);assert!(h.ack("main",&old,false,good).is_err());assert!(h.invalidate("main",&version(&i,1,2)).is_err());let v=version(&fresh,0,1);h.invalidate("main",&v).unwrap();h.begin_list("main",&v).unwrap();assert!(h.begin_list("main",&v).is_err());h.invalidate("main",&version(&fresh,1,2)).unwrap();assert!(h.finish_list("main",&v,Content::Nonempty).is_err());let r=page(&h,"main",&fresh,1,3,Content::Empty);assert!(h.finish_list("main",&version(&fresh,1,3),Content::Nonempty).is_err());assert_eq!(r.content,Content::Empty);}
    #[test]fn measurement_every_hotkey_has_attempt_and_coalesced_busy_outcome(){let(h,_,out)=fixture();let id=h.hotkey(10,None).unwrap();h.hotkey(10,Some("coalesced")).unwrap();h.hotkey(10,Some("dispatch_busy")).unwrap();h.panel_open(&id,10,"a".repeat(32)).unwrap();let i=h.start_model("quick-panel").unwrap();let r=page(&h,"quick-panel",&i,0,1,Content::Nonempty);assert!(h.ack("main",&r,true,good).is_err());let mut wrong=r.clone();wrong.session=Some("b".repeat(32));assert!(h.ack("quick-panel",&wrong,true,good).is_err());assert_eq!(h.ack("quick-panel",&r,false,good),Err(MeasureError::NotInteractive));assert_eq!(h.ack("quick-panel",&r,true,good),Err(MeasureError::Stale));let next=h.hotkey(10,None).unwrap();h.panel_open(&next,10,"c".repeat(32)).unwrap();let fresh=h.start_model("quick-panel").unwrap();let accepted=page(&h,"quick-panel",&fresh,0,1,Content::Nonempty);h.ack("quick-panel",&accepted,true,good).unwrap();let e=events(&out);assert_eq!(e.iter().filter(|e|e["event"]=="native_hotkey_dequeued").count(),4);assert_eq!(e.iter().filter(|e|e["reason"]=="coalesced"||e["reason"]=="dispatch_busy").count(),2);assert_eq!(e.iter().filter(|e|e["reason"]=="input_not_focused"&&e["attempt"]==id).count(),1);assert_eq!(e.iter().filter(|e|e["event"]=="panel_interactive_ready"&&e["attempt"]==id).count(),0);assert_eq!(e.iter().filter(|e|e["event"]=="panel_interactive_ready"&&e["attempt"]==next).count(),1);}
    #[test]fn measurement_write_and_flush_failures_are_sticky_and_no_success(){for flush in [false,true]{let(h,_,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);h.native_ready(true,good).unwrap();if flush{out.flush_fail.store(true,Ordering::SeqCst);}else{out.fail.store(true,Ordering::SeqCst);}assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Output));out.fail.store(false,Ordering::SeqCst);out.flush_fail.store(false,Ordering::SeqCst);assert_eq!(h.end(),Err(MeasureError::Output));assert!(!events(&out).iter().any(|e|e["event"]=="main_interactive_ready"||e["event"]=="run_end"));}}
    #[test]fn measurement_deadlines_limits_clock_and_partial_start_fail_closed(){
        let(h,c,out)=fixture();let i=h.start_model("main").unwrap();let r=page(&h,"main",&i,0,1,Content::Nonempty);c.0.store(302,Ordering::SeqCst);assert_eq!(h.ack("main",&r,false,good),Err(MeasureError::Expired));assert!(events(&out).iter().any(|e|e["reason"]=="attempt_timeout"));
        let(h,c,out)=fixture();c.0.store(18002,Ordering::SeqCst);assert_eq!(h.start_model("main"),Err(MeasureError::Expired));assert_eq!(events(&out).last().unwrap()["complete"],false);
        let(h,c,_)=fixture();c.0.store(9,Ordering::SeqCst);assert_eq!(h.start_model("main"),Err(MeasureError::Clock));assert!(h.end().is_err());
        let(h,_,out)=fixture();for _ in 0..2100{if h.hotkey(10,Some("coalesced")).is_err(){break;}}let e=events(&out);assert!(e.len()<=4096);assert_eq!(e.last().unwrap()["complete"],false);assert!(h.hotkey(10,None).is_err());
        let(_,c,out)=fixture();out.fail.store(true,Ordering::SeqCst);assert!(MeasureHost::new(MeasureArgs{run_id:"test".into(),output:PathBuf::new(),cold:1,frequency:10},Arc::new(c),Box::new(out)).is_err());
    }
    #[test]fn measurement_default_cli_groups_and_worker_boundary(){use crate::config::{parse_args,LaunchMode};let base=["--data-dir",r"D:\Synthetic\Isolated"];
        assert!(matches!(parse_args(base.map(String::from)).unwrap(),LaunchMode::Host(args) if args.measurement.is_none()));assert!(Measurement::default().enabled().is_err());assert_eq!(Measurement::default().script(),"");
        let flags=["--measure-run-id","test","--measure-output",r"D:\Synthetic\run.ndjson","--measure-cold-start-qpc","1","--measure-qpc-frequency","10"];
        let all=base.into_iter().chain(["--start-paused"]).chain(flags).map(String::from).collect::<Vec<_>>();assert!(matches!(parse_args(all.clone()).unwrap(),LaunchMode::Host(args) if args.measurement.is_some()));
        for index in 0..4{let mut args=all.clone();args.drain(3+index*2..5+index*2);assert!(parse_args(args).is_err());}let mut duplicate=all.clone();duplicate.extend(["--measure-run-id".into(),"again".into()]);assert!(parse_args(duplicate).is_err());let mut worker=all.clone();worker.push(crate::config::WORKER_MODE_FLAG.into());assert!(parse_args(worker).is_err());let mut unpaused=all;unpaused.remove(2);assert!(parse_args(unpaused).is_err());for value in ["0","01","+1"," 1","18446744073709551616"]{assert!(decimal(value,false).is_err());}
    }
    #[test]fn measurement_create_new_path_clock_validation_no_unintended_output(){
        use crate::measurement::Clock;let root=PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("isolated root"));let root=root.join(format!("measurement-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let data=root.join("data");std::fs::create_dir(&data).unwrap();let clock=Qpc;let args=MeasureArgs{run_id:"test".into(),output:root.join("new.ndjson"),cold:clock.now().unwrap(),frequency:clock.frequency().unwrap()};
        let mut bad=args.clone();bad.frequency+=1;assert!(bad.open(&data,&clock).is_err());assert!(!args.output.exists());bad=args.clone();bad.cold=u64::MAX;assert!(bad.open(&data,&clock).is_err());assert!(!args.output.exists());bad=args.clone();bad.output=data.join("forbidden.ndjson");assert!(bad.open(&data,&clock).is_err());assert!(!bad.output.exists());
        let host=args.open(&data,&clock).unwrap();host.end().unwrap();let before=std::fs::read(&args.output).unwrap();assert!(args.open(&data,&clock).is_err());assert_eq!(std::fs::read(&args.output).unwrap(),before);
        assert!(MeasureArgs::parse("test",r"D:\Synthetic\..\run.ndjson","1","10").is_err());assert!(MeasureArgs::parse("test","relative.ndjson","1","10").is_err());assert!(MeasureArgs::parse("private query",r"D:\Synthetic\run.ndjson","1","10").is_err());
    }
    #[cfg(feature="test-fixtures")]
    #[test]fn measurement_real_host_query_adapter_uses_original_parser_store_and_page_dto(){
        use crate::{host::HostRuntime,store::{Store,Snapshot,Payload,SourceConfidence}};let root=PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").unwrap()).join(format!("measurement-adapter-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let path=root.join("new.sqlite3");{let mut store=Store::open(&path).unwrap();store.insert(&Snapshot{capture_id:"synthetic".into(),captured_at_ms:1,source_process:None,source_confidence:SourceConfidence::Unknown,omitted_formats:vec![],payloads:vec![Payload{format_name:"CF_UNICODETEXT".into(),data:"synthetic body".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()}]}).unwrap();}
        let runtime=HostRuntime::for_test_file(path,Arc::new(|_,_|{})).unwrap();let(h,_,out)=fixture();let i=h.start_model("main").unwrap();for(seq,query,content)in[(1,"",Content::Nonempty),(2,"absent",Content::Empty)]{let v=version(&i,0,seq);h.invalidate("main",&v).unwrap();let mut request=serde_json::to_value(&i).unwrap();request["generation"]="0".into();request["sequence"]=seq.to_string().into();request["query_request"]=serde_json::json!({"request_id":"actual","query":query,"limit":50,"cursor":null});let page=h.list("main",serde_json::from_value(request).unwrap(),&runtime).unwrap();crate::contracts::parse_list_clips_result(&serde_json::to_value(&page.result).unwrap()).unwrap();assert_eq!(page.receipt.unwrap().content,content);}
        runtime.shutdown(std::time::Duration::from_secs(2)).unwrap();assert!(!String::from_utf8(out.bytes.lock().unwrap().clone()).unwrap().contains("synthetic body"));
    }
}
