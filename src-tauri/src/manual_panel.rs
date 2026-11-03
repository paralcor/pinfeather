//! One session authority. The session lock is held through queue admission, never reply waiting.
use std::sync::Mutex;
use crate::contracts::{ContractError,ContractResult,ErrorCode};
use serde::Serialize;

#[derive(Debug,Clone,Serialize)]
pub struct PanelSnapshot { pub revision:String,pub session_id:Option<String>,pub phase:String,pub hotkey:String,pub error:Option<String> }
#[derive(Clone,Copy,PartialEq,Eq)]
enum Phase { Hidden,Waiting,Ready,Stopping,Unavailable }
struct State { revision:u64,session:Option<String>,phase:Phase,hotkey:String,error:Option<String> }
pub trait PanelWindow:Send+Sync {
    fn hide(&self)->Result<(),String>;
    fn show(&self)->Result<(),String>;
    fn focus(&self)->Result<(),String>;
    fn publish(&self,snapshot:&PanelSnapshot)->Result<(),String>;
}
pub struct PanelSessions { state:Mutex<State> }
fn denied()->ContractError { ContractError::new(ErrorCode::PermissionUnknown,"panel session is not current and ready") }
impl PanelSessions {
    pub fn new(available:bool)->Self { Self{state:Mutex::new(State{revision:0,session:None,phase:if available{Phase::Hidden}else{Phase::Unavailable},hotkey:"not_started".into(),error:if available{None}else{Some("快捷面板创建失败，主窗口仍可使用。".into())}})} }
    pub fn snapshot(&self)->PanelSnapshot { self.state.lock().unwrap_or_else(|e|e.into_inner()).snapshot() }
    pub fn hotkey(&self,status:&str,error:Option<String>,window:&dyn PanelWindow) { let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.hotkey=status.into();s.error=error;s.revision+=1;let _=window.publish(&s.snapshot()); }
    pub fn open(&self,window:&dyn PanelWindow)->ContractResult<PanelSnapshot> {
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        if matches!(s.phase,Phase::Stopping|Phase::Unavailable){return Err(denied());}
        s.session=Some(uuid::Uuid::new_v4().simple().to_string());s.phase=Phase::Waiting;s.error=None;s.revision+=1;
        if let Err(error)=window.hide().and_then(|_|window.publish(&s.snapshot())) {s.fail(error);let _=window.publish(&s.snapshot());return Err(ContractError::new(ErrorCode::CleanupFailed,"panel open failed"));}
        Ok(s.snapshot())
    }
    pub fn ready(&self,session:&str,window:&dyn PanelWindow)->ContractResult<PanelSnapshot> {
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        if s.session.as_deref()!=Some(session){return Err(denied());}
        if s.phase==Phase::Ready{return Ok(s.snapshot());}
        if s.phase!=Phase::Waiting{return Err(denied());}
        s.phase=Phase::Ready;s.revision+=1;
        if let Err(error)=window.show().and_then(|_|window.focus()) {s.fail(error);let _=window.hide();let _=window.publish(&s.snapshot());return Err(ContractError::new(ErrorCode::CleanupFailed,"panel presentation failed"));}
        if let Err(error)=window.publish(&s.snapshot()){s.fail(error);let _=window.hide();return Err(ContractError::new(ErrorCode::CleanupFailed,"panel notification failed"));}
        Ok(s.snapshot())
    }
    pub fn hide(&self,session:&str,window:&dyn PanelWindow)->ContractResult<PanelSnapshot> {
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        if s.session.as_deref()!=Some(session)||matches!(s.phase,Phase::Stopping|Phase::Unavailable){return Err(denied());}
        s.session=None;s.phase=Phase::Hidden;s.revision+=1;
        if let Err(error)=window.hide(){s.error=Some(error);let _=window.publish(&s.snapshot());return Err(ContractError::new(ErrorCode::CleanupFailed,"panel hide failed"));}
        let _=window.publish(&s.snapshot());Ok(s.snapshot())
    }
    pub fn dismiss(&self,window:&dyn PanelWindow) { if let Some(session)=self.snapshot().session_id {let _=self.hide(&session,window);} }
    pub fn stop(&self,window:&dyn PanelWindow)->ContractResult<()> {let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.session=None;s.phase=Phase::Stopping;s.revision+=1;let result=window.hide();if let Err(error)=&result{s.error=Some(error.clone());}let _=window.publish(&s.snapshot());result.map_err(|_|ContractError::new(ErrorCode::CleanupFailed,"panel hide incomplete"))}
    pub fn seal(&self){let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.session=None;s.phase=Phase::Stopping;s.revision+=1;}
    pub fn admit<T>(&self,session:&str,admission:impl FnOnce()->T)->ContractResult<T> {
        let s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        if s.phase!=Phase::Ready||s.session.as_deref()!=Some(session){return Err(denied());}
        Ok(admission())
    }
}

/// Tauri is only the effects adapter; all session decisions above are shared with tests.
pub struct TauriPanelWindow(pub tauri::AppHandle);
impl PanelWindow for TauriPanelWindow {
    fn hide(&self)->Result<(),String>{use tauri::Manager;if let Some(w)=self.0.get_webview_window("quick-panel"){w.hide().map_err(|_|"快捷面板隐藏失败。".into())}else{Ok(())}}
    fn show(&self)->Result<(),String>{use tauri::Manager;self.0.get_webview_window("quick-panel").ok_or("快捷面板不可用。".to_string())?.show().map_err(|_|"快捷面板显示失败。".into())}
    fn focus(&self)->Result<(),String>{
        use tauri::Manager;
        let window=self.0.get_webview_window("quick-panel").ok_or("快捷面板不可用。".to_string())?;
        window.set_focus().map_err(|_|"快捷面板聚焦失败。".to_string())?;
        let webview:&tauri::Webview=window.as_ref();
        webview.set_focus().map_err(|_|"快捷面板输入焦点请求失败。".into())
    }
    fn publish(&self,snapshot:&PanelSnapshot)->Result<(),String>{use tauri::Emitter;self.0.emit("manual_panel_changed",snapshot).map_err(|_|"快捷面板状态通知失败。".into())}
}
pub struct PanelHost {
    pub sessions:PanelSessions,
    pub window:TauriPanelWindow,
    pub hotkey:Mutex<Option<crate::platform::hotkey::HotkeyOwner>>,
    dispatch_pending:std::sync::atomic::AtomicBool,
}
impl PanelHost {
    pub fn new(handle:tauri::AppHandle,available:bool)->Self{Self{sessions:PanelSessions::new(available),window:TauriPanelWindow(handle),hotkey:Mutex::new(None),dispatch_pending:std::sync::atomic::AtomicBool::new(false)}}
    pub fn start_hotkey(self:&std::sync::Arc<Self>){
        use std::sync::{Arc,atomic::Ordering};let weak=Arc::downgrade(self);let report=weak.clone();
        let(owner,result)=crate::platform::hotkey::HotkeyOwner::start(Box::new(crate::platform::hotkey::WindowsHotkey),Arc::new(move||{
            if let Some(panel)=weak.upgrade(){if panel.dispatch_pending.swap(true,Ordering::AcqRel){return;}let target=panel.clone();if panel.window.0.run_on_main_thread(move||{let _=target.sessions.open(&target.window);target.dispatch_pending.store(false,Ordering::Release);}).is_err(){panel.dispatch_pending.store(false,Ordering::Release);panel.sessions.hotkey("failed",Some("快捷键派发失败，可使用主窗口入口。".into()),&panel.window);}}
        }),Arc::new(move|error|{if let Some(panel)=report.upgrade(){panel.sessions.hotkey("failed",Some(error),&panel.window);}}));
        *self.hotkey.lock().unwrap_or_else(|e|e.into_inner())=Some(owner);
        self.sessions.hotkey(if result.is_ok(){"ready"}else{"failed"},result.err(),&self.window);
    }
    pub fn stop(&self)->ContractResult<()> {
        self.stop_until(std::time::Instant::now()+std::time::Duration::from_secs(3))
    }
    pub fn stop_until(&self,deadline:std::time::Instant)->ContractResult<()> {
        let hidden=self.sessions.stop(&self.window);
        if let Some(owner)=self.hotkey.lock().unwrap_or_else(|e|e.into_inner()).as_mut(){if let Err(error)=owner.stop_until(deadline){self.sessions.hotkey("failed",Some(error.clone()),&self.window);return Err(ContractError::new(ErrorCode::CleanupFailed,&format!("{}hotkey: {error}",hidden.err().map(|e|format!("{}; ",e.message)).unwrap_or_default())));}}
        hidden?;self.sessions.hotkey("stopped",None,&self.window);Ok(())
    }
}
impl State {
    fn fail(&mut self,error:String){self.session=None;self.phase=Phase::Hidden;self.error=Some(error);self.revision+=1;}
    fn snapshot(&self)->PanelSnapshot {PanelSnapshot{revision:self.revision.to_string(),session_id:self.session.clone(),phase:match self.phase{Phase::Hidden=>"hidden",Phase::Waiting=>"waiting",Phase::Ready=>"ready",Phase::Stopping=>"stopping",Phase::Unavailable=>"unavailable"}.into(),hotkey:self.hotkey.clone(),error:self.error.clone()}}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};
    #[derive(Default)]struct Window {fail:AtomicUsize,shown:AtomicUsize,focused:AtomicUsize}
    impl PanelWindow for Window {
        fn hide(&self)->Result<(),String>{if self.fail.load(Ordering::SeqCst)==1{Err("hide failed".into())}else{Ok(())}}
        fn show(&self)->Result<(),String>{self.shown.fetch_add(1,Ordering::SeqCst);if self.fail.load(Ordering::SeqCst)==2{Err("show failed".into())}else{Ok(())}}
        fn focus(&self)->Result<(),String>{self.focused.fetch_add(1,Ordering::SeqCst);if self.fail.load(Ordering::SeqCst)==3{Err("focus failed".into())}else{Ok(())}}
        fn publish(&self,_:&PanelSnapshot)->Result<(),String>{if self.fail.load(Ordering::SeqCst)==4{Err("publish failed".into())}else{Ok(())}}
    }
    #[test]fn manual_panel_session_rejects_missing_old_forged_not_ready_hidden_and_stopping(){
        let p=PanelSessions::new(true);let w=Window::default();let writes=AtomicUsize::new(0);
        let attempt=|session:&str|p.admit(session,||writes.fetch_add(1,Ordering::SeqCst));
        assert!(attempt("forged").is_err());
        let a=p.open(&w).unwrap().session_id.unwrap();assert!(attempt(&a).is_err());
        p.ready(&a,&w).unwrap();assert!(attempt("forged").is_err());assert!(attempt(&a).is_ok());
        let b=p.open(&w).unwrap().session_id.unwrap();assert_ne!(a,b);assert!(attempt(&a).is_err());assert!(p.ready(&a,&w).is_err());
        p.ready(&b,&w).unwrap();assert!(p.hide(&a,&w).is_err());p.hide(&b,&w).unwrap();assert!(attempt(&b).is_err());
        let c=p.open(&w).unwrap().session_id.unwrap();p.stop(&w).unwrap();assert!(p.ready(&c,&w).is_err());assert!(attempt(&c).is_err());assert!(p.open(&w).is_err());assert_eq!(writes.load(Ordering::SeqCst),1);
    }
    #[test]fn manual_panel_show_focus_hide_and_notification_fail_closed(){for fail in 1..=4{let p=PanelSessions::new(true);let w=Window::default();let session=p.open(&w).unwrap().session_id.unwrap();w.fail.store(fail,Ordering::SeqCst);if fail==1{assert!(p.hide(&session,&w).is_err());}else{assert!(p.ready(&session,&w).is_err());}assert!(p.admit(&session,||()).is_err());assert!(p.snapshot().error.is_some());}}
    #[test]fn manual_panel_late_ready_never_shows_or_focuses(){let p=PanelSessions::new(true);let w=Window::default();let a=p.open(&w).unwrap().session_id.unwrap();let b=p.open(&w).unwrap().session_id.unwrap();assert!(p.ready(&a,&w).is_err());assert_eq!(w.shown.load(Ordering::SeqCst),0);p.hide(&b,&w).unwrap();assert!(p.ready(&b,&w).is_err());assert_eq!(w.focused.load(Ordering::SeqCst),0);}
    #[test]fn manual_panel_shared_strict_contract(){let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/contracts/manual_panel.json")).unwrap();for c in fixture["sessions"].as_array().unwrap(){assert_eq!(crate::contracts::parse_panel_session(&c["value"]).is_ok(),c["valid"].as_bool().unwrap());}for c in fixture["copies"].as_array().unwrap(){assert_eq!(crate::contracts::parse_panel_copy(&c["value"]).is_ok(),c["valid"].as_bool().unwrap());}}
    #[test]fn manual_panel_unavailable_and_failed_stop_keep_closed_authority(){let p=PanelSessions::new(false);let w=Window::default();assert!(p.snapshot().error.is_some());assert!(p.open(&w).is_err());assert!(p.admit("forged",||panic!("unavailable")).is_err());let p=PanelSessions::new(true);let id=p.open(&w).unwrap().session_id.unwrap();p.ready(&id,&w).unwrap();p.ready(&id,&w).unwrap();assert_eq!(w.focused.load(Ordering::SeqCst),1,"duplicate ready must not steal focus");w.fail.store(1,Ordering::SeqCst);assert!(p.stop(&w).is_err());assert!(p.admit(&id,||panic!("stopped")).is_err());assert!(p.open(&w).is_err());w.fail.store(0,Ordering::SeqCst);p.stop(&w).unwrap();p.stop(&w).unwrap();}
    #[test]fn manual_panel_admission_and_hide_share_one_lock(){let p=std::sync::Arc::new(PanelSessions::new(true));let w=std::sync::Arc::new(Window::default());let a=p.open(&*w).unwrap().session_id.unwrap();p.ready(&a,&*w).unwrap();let(entered,rx)=std::sync::mpsc::channel();let(release,go)=std::sync::mpsc::channel();let pp=p.clone();let aa=a.clone();let copy=std::thread::spawn(move||pp.admit(&aa,||{entered.send(()).unwrap();go.recv().unwrap();}));rx.recv().unwrap();assert!(p.state.try_lock().is_err());release.send(()).unwrap();copy.join().unwrap().unwrap();p.hide(&a,&*w).unwrap();assert!(p.admit(&a,||panic!("no late side effect")).is_err());}
}
