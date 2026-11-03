//! Residency decisions and one retained cleanup worker, independent of native effects.
use std::{sync::{Arc,Mutex,mpsc},thread::JoinHandle,time::{Duration,Instant}};
use crate::{contracts::{ContractError,ContractResult,ErrorCode},host::HostRuntime,manual_panel::PanelHost};
use tauri::{Emitter,Manager};

#[derive(Debug,Clone,serde::Serialize)]
pub struct ResidencySnapshot {pub revision:String,pub phase:String,pub tray_available:bool,pub error:Option<String>}
struct State {revision:u64,phase:&'static str,tray:bool,error:Option<String>,terminal_failure:bool}
impl State {fn record_error(&mut self,error:String){self.error=Some(match self.error.take(){Some(primary) if !primary.contains(&error)=>format!("{primary}; {error}"),Some(primary)=>primary,None=>error});}}
#[derive(Debug)]
pub struct CleanupError {pub message:String,pub terminal:bool}
pub trait ResidencyEffects:Send+Sync+'static {
    fn tray_available(&self)->Result<bool,String>;
    fn show_main(&self)->Result<(),String>;
    fn hide_main(&self)->Result<(),String>;
    fn seal(&self,deadline:Instant);
    fn cleanup(&self,deadline:Instant)->Result<(),CleanupError>;
    fn publish(&self,state:&ResidencySnapshot);
    fn exit(&self);
}
struct CleanupOwner {requests:mpsc::SyncSender<Instant>,_thread:JoinHandle<()>}
pub struct Residency {state:Mutex<State>,worker:Mutex<Option<CleanupOwner>>}
impl Residency {
    pub fn new()->Arc<Self>{Arc::new(Self{state:Mutex::new(State{revision:0,phase:"running",tray:false,error:None,terminal_failure:false}),worker:Mutex::new(None)})}
    pub fn snapshot(&self)->ResidencySnapshot{let s=self.state.lock().unwrap_or_else(|e|e.into_inner());ResidencySnapshot{revision:s.revision.to_string(),phase:s.phase.into(),tray_available:s.tray,error:s.error.clone()}}
    fn update(&self,edit:impl FnOnce(&mut State),effects:&dyn ResidencyEffects){
        {let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());edit(&mut s);s.revision+=1;}effects.publish(&self.snapshot());
    }
    pub fn running(&self)->bool{self.state.lock().unwrap_or_else(|e|e.into_inner()).phase=="running"}
    pub fn exited(&self)->bool{self.state.lock().unwrap_or_else(|e|e.into_inner()).phase=="exited"}
    pub fn tray_result(&self,result:Result<(),String>,effects:&dyn ResidencyEffects){
        let failed=result.is_err();self.update(|s|{s.tray=!failed;if let Err(error)=result{s.record_error(error);}},effects);
        if failed{self.show(effects);}
    }
    pub fn show(&self,effects:&dyn ResidencyEffects){if let Err(error)=effects.show_main(){self.update(|s|s.record_error(error),effects);}}
    pub fn close_main(&self,effects:&dyn ResidencyEffects){
        // The native probe can dispatch to the main thread; hold no state lock while waiting.
        let tray=if self.running(){effects.tray_available()}else{Ok(false)};
        // Keep the eligibility check and hide effect in the exit admission order.
        let (hidden,observed)={
            let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
            if s.phase=="running"{
                s.tray=matches!(tray,Ok(true));s.revision+=1;
                if let Err(error)=tray{s.record_error(error);}
                if s.tray{(Some(effects.hide_main()),true)}else{s.record_error("托盘不可用，请使用退出片羽。".into());(None,true)}
            }else{(None,false)}
        };
        if observed{effects.publish(&self.snapshot());}
        match hidden {
            None=>self.show(effects),
            Some(Err(error))=>{self.update(|s|s.record_error(error),effects);self.show(effects);},
            Some(Ok(()))=>{},
        }
    }
    pub fn request_exit(self:&Arc<Self>,effects:Arc<dyn ResidencyEffects>)->ContractResult<ResidencySnapshot>{
        let deadline=Instant::now()+Duration::from_secs(3);
        {let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());if matches!(s.phase,"stopping"|"exited"){drop(s);return Ok(self.snapshot());}s.phase="stopping";s.revision+=1;}
        // Close admission before any asynchronous cleanup or late callback.
        effects.seal(deadline);effects.publish(&self.snapshot());
        if let Err(error)=effects.show_main(){self.update(|s|{s.phase="failed";s.record_error(error);},effects.as_ref());return Ok(self.snapshot());}
        let mut worker=self.worker.lock().unwrap_or_else(|e|e.into_inner());
        if worker.is_none(){
            let(tx,rx)=mpsc::sync_channel(1);let weak=Arc::downgrade(self);let port=effects.clone();
            let thread=std::thread::Builder::new().name("residency-cleanup".into()).spawn(move||{
                while let Ok(deadline)=rx.recv(){
                    let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||port.cleanup(deadline))).unwrap_or_else(|_|Err(CleanupError{message:"退出任务异常，资源回收未确认。".into(),terminal:true}));
                    let Some(owner)=weak.upgrade()else{break;};
                    let success=result.is_ok()&&!owner.state.lock().unwrap_or_else(|e|e.into_inner()).terminal_failure;
                    owner.update(|s|{s.phase=if success{"exited"}else{"failed"};if let Err(error)=result{s.terminal_failure|=error.terminal;s.record_error(error.message);}},port.as_ref());
                    if success{port.exit();break;}else{owner.show(port.as_ref());}
                }
            }).map_err(|_|ContractError::new(ErrorCode::CleanupFailed,"cleanup thread unavailable"));
            match thread{Ok(thread)=>*worker=Some(CleanupOwner{requests:tx,_thread:thread}),Err(error)=>{drop(worker);self.update(|s|{s.phase="failed";s.record_error(error.message.clone());},effects.as_ref());return Err(error);}}
        }
        if worker.as_ref().unwrap().requests.try_send(deadline).is_err(){drop(worker);self.update(|s|{s.phase="failed";s.record_error("退出资源所有者不可用。".into());},effects.as_ref());return Err(ContractError::new(ErrorCode::CleanupFailed,"cleanup owner unavailable"));}
        drop(worker);Ok(self.snapshot())
    }
}

pub struct TauriEffects {pub handle:tauri::AppHandle,pub runtime:Arc<HostRuntime>,pub panel:Arc<PanelHost>}
impl ResidencyEffects for TauriEffects {
    fn tray_available(&self)->Result<bool,String>{
        let Some(tray)=self.handle.tray_by_id("pinfeather-residency")else{return Ok(false);};
        tray.rect().map(|rect|rect.is_some()).map_err(|_|"托盘状态查询失败，请使用主窗口处理。".into())
    }
    fn show_main(&self)->Result<(),String>{let w=self.handle.get_webview_window("main").ok_or("主窗口不可用。")?;w.show().and_then(|_|w.set_focus()).map_err(|_|"主窗口显示失败，请保留现有入口并重试退出。".into())}
    fn hide_main(&self)->Result<(),String>{self.handle.get_webview_window("main").ok_or("主窗口不可用。")?.hide().map_err(|_|"主窗口隐藏失败，窗口将保留。".into())}
    fn seal(&self,deadline:Instant){self.panel.sessions.seal();self.runtime.begin_shutdown(deadline);}
    fn cleanup(&self,deadline:Instant)->Result<(),CleanupError>{
        cleanup_owners(deadline,|until|self.panel.stop_until(until),|until|self.runtime.shutdown_until(until))
    }
    fn publish(&self,state:&ResidencySnapshot){let _=self.handle.emit("residency_changed",state);let handle=self.handle.clone();let _=self.handle.run_on_main_thread(move||{if let Some(owner)=handle.try_state::<Arc<DesktopResidency>>(){owner.refresh_tray();}});}
    fn exit(&self){self.handle.exit(0);}
}
pub(crate) fn cleanup_owners(deadline:Instant,panel:impl FnOnce(Instant)->ContractResult<()>,runtime:impl FnOnce(Instant)->ContractResult<()>)->Result<(),CleanupError>{
    let panel=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||panel(deadline)));let panel_panic=panel.is_err();let panel=panel.unwrap_or_else(|_|Err(ContractError::new(ErrorCode::CleanupFailed,"panel cleanup panicked")));
    let runtime=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||runtime(deadline))).unwrap_or_else(|_|Err(ContractError::new(ErrorCode::CleanupFailed,"runtime cleanup panicked")));
    let terminal=panel_panic||runtime.as_ref().is_err_and(|e|e.code==ErrorCode::CleanupFailed);
    let errors=[panel.err().map(|e|format!("面板：{}",e.message)),runtime.err().map(|e|format!("宿主：{}",e.message))].into_iter().flatten().collect::<Vec<_>>();
    if errors.is_empty(){Ok(())}else{Err(CleanupError{message:errors.join("；"),terminal})}
}
pub struct DesktopResidency {
    pub coordinator:Arc<Residency>,pub effects:Arc<TauriEffects>,
    tray:Mutex<Option<tauri::tray::TrayIcon>>,
    menus:Mutex<Option<(tauri::menu::MenuItem<tauri::Wry>,tauri::menu::MenuItem<tauri::Wry>,tauri::menu::MenuItem<tauri::Wry>,tauri::menu::MenuItem<tauri::Wry>)>>,
}
impl DesktopResidency {
    pub fn new(handle:tauri::AppHandle,runtime:Arc<HostRuntime>,panel:Arc<PanelHost>)->Arc<Self>{Arc::new(Self{coordinator:Residency::new(),effects:Arc::new(TauriEffects{handle,runtime,panel}),tray:Mutex::new(None),menus:Mutex::new(None)})}
    pub fn install_tray(self:&Arc<Self>){
        let result=(||->tauri::Result<()>{
            use tauri::{menu::{Menu,MenuItem},tray::TrayIconBuilder};let h=&self.effects.handle;
            let main=MenuItem::with_id(h,"residency-main","打开主窗口",true,None::<&str>)?;
            let panel=MenuItem::with_id(h,"residency-panel","打开快捷面板",true,None::<&str>)?;
            let status=MenuItem::with_id(h,"residency-status","收录状态：已暂停",false,None::<&str>)?;
            let start=MenuItem::with_id(h,"residency-start","开始收录",true,None::<&str>)?;
            let pause=MenuItem::with_id(h,"residency-pause","暂停收录",false,None::<&str>)?;
            let exit=MenuItem::with_id(h,"residency-exit","退出片羽",true,None::<&str>)?;
            let menu=Menu::with_items(h,&[&main,&panel,&status,&start,&pause,&exit])?;
            let weak=Arc::downgrade(self);let icon=h.default_window_icon().ok_or_else(||tauri::Error::AssetNotFound("tray icon".into()))?;
            let builder=TrayIconBuilder::with_id("pinfeather-residency").tooltip("片羽 · 已暂停").icon(icon.clone()).menu(&menu).on_menu_event(move|_,event|{if let Some(owner)=weak.upgrade(){owner.menu(event.id.as_ref());}});
            let tray=builder.build(h)?;*self.tray.lock().unwrap_or_else(|e|e.into_inner())=Some(tray);*self.menus.lock().unwrap_or_else(|e|e.into_inner())=Some((status,start,pause,panel));Ok(())
        })();
        self.coordinator.tray_result(result.map_err(|_|"托盘创建失败，请使用主窗口的退出片羽。".into()),self.effects.as_ref());
        self.refresh_tray();
    }
    pub fn refresh_tray(&self){
        if !self.coordinator.snapshot().tray_available{return;}
        let status=self.effects.runtime.status();let settings=self.effects.runtime.settings();let running=self.coordinator.running();
        let text=if !running{"正在退出：打开主窗口处理"}else if settings.blocked{"设置异常：收录已阻止"}else{match status.state.as_str(){"listening"=>"正在收录","paused"=>"已暂停","overloaded"=>"已停止新增：打开主窗口处理","failed"=>"服务异常：打开主窗口处理",_=>"正在退出：打开主窗口处理"}};
        let result=(||->tauri::Result<()>{
            if let Some((label,start,pause,panel))=self.menus.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){label.set_text(format!("收录状态：{text}"))?;start.set_enabled(running&&!settings.blocked&&!settings.saving&&status.services.storage!="failed"&&(!status.capture.user_enabled||status.state=="failed"||status.state=="overloaded"))?;pause.set_enabled(running&&status.capture.user_enabled)?;panel.set_enabled(running)?;}
            if let Some(tray)=self.tray.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){tray.set_tooltip(Some(format!("片羽 · {text}")))?;}Ok(())
        })();
        if result.is_err(){self.coordinator.tray_result(Err("托盘更新失败，请使用主窗口处理。".into()),self.effects.as_ref());}
    }
    fn menu(self:&Arc<Self>,id:&str){match id{
        "residency-main"=>self.coordinator.show(self.effects.as_ref()),
        "residency-panel" if self.coordinator.running()=>{if self.effects.panel.sessions.open(&self.effects.panel.window).is_err(){self.coordinator.show(self.effects.as_ref());}},
        "residency-start"|"residency-pause" if self.coordinator.running()=>{let enabled=id=="residency-start";let owner=self.clone();tauri::async_runtime::spawn_blocking(move||{if owner.effects.runtime.set_enabled(enabled).is_err(){owner.coordinator.show(owner.effects.as_ref());}owner.effects.publish(&owner.coordinator.snapshot());});},
        "residency-exit"=>{let _=self.coordinator.request_exit(self.effects.clone());},_=>{}
    }}
}

#[cfg(all(test,feature="test-fixtures"))]
mod tests {
    use super::*;
    use std::{collections::VecDeque,sync::atomic::{AtomicBool,AtomicUsize,Ordering}};
    struct Effects {
        tray:Mutex<Result<bool,String>>,probes:AtomicUsize,probe_entered:Mutex<Option<mpsc::Sender<()>>>,probe_gate:Mutex<Option<mpsc::Receiver<()>>>,
        show_fail:AtomicBool,hide_fail:AtomicBool,hides:AtomicUsize,shows:AtomicUsize,seals:AtomicUsize,exits:AtomicUsize,
        updates:mpsc::Sender<ResidencySnapshot>,entered:mpsc::Sender<()>,gate:Mutex<Option<mpsc::Receiver<()>>>,
        results:Mutex<VecDeque<Result<(),CleanupError>>>,threads:Mutex<Vec<std::thread::ThreadId>>,deadlines:Mutex<Vec<Instant>>,sealed:Mutex<Vec<Instant>>,
        hide_gate:Mutex<Option<mpsc::Receiver<()>>>,hide_entered:Mutex<Option<mpsc::Sender<()>>>,hidden_after_seal:AtomicBool,
    }
    impl ResidencyEffects for Effects {
        fn tray_available(&self)->Result<bool,String>{self.probes.fetch_add(1,Ordering::SeqCst);if let Some(tx)=self.probe_entered.lock().unwrap().take(){tx.send(()).unwrap();}if let Some(rx)=self.probe_gate.lock().unwrap().take(){rx.recv_timeout(Duration::from_secs(3)).unwrap();}self.tray.lock().unwrap().clone()}
        fn show_main(&self)->Result<(),String>{self.shows.fetch_add(1,Ordering::SeqCst);if self.show_fail.load(Ordering::SeqCst){Err("show failed".into())}else{Ok(())}}
        fn hide_main(&self)->Result<(),String>{if let Some(tx)=self.hide_entered.lock().unwrap().take(){tx.send(()).unwrap();}if let Some(rx)=self.hide_gate.lock().unwrap().take(){rx.recv_timeout(Duration::from_secs(3)).unwrap();}self.hidden_after_seal.store(self.seals.load(Ordering::SeqCst)>0,Ordering::SeqCst);self.hides.fetch_add(1,Ordering::SeqCst);if self.hide_fail.load(Ordering::SeqCst){Err("hide failed".into())}else{Ok(())}}
        fn seal(&self,deadline:Instant){self.seals.fetch_add(1,Ordering::SeqCst);self.sealed.lock().unwrap().push(deadline);}
        fn cleanup(&self,deadline:Instant)->Result<(),CleanupError>{self.threads.lock().unwrap().push(std::thread::current().id());self.deadlines.lock().unwrap().push(deadline);self.entered.send(()).unwrap();if let Some(gate)=self.gate.lock().unwrap().take(){gate.recv_timeout(Duration::from_secs(3)).unwrap();}self.results.lock().unwrap().pop_front().unwrap_or(Ok(()))}
        fn publish(&self,state:&ResidencySnapshot){let _=self.updates.send(state.clone());}
        fn exit(&self){self.exits.fetch_add(1,Ordering::SeqCst);}
    }
    fn fixture()->(Arc<Residency>,Arc<Effects>,mpsc::Receiver<ResidencySnapshot>,mpsc::Receiver<()>){
        let(tx,rx)=mpsc::channel();let(entered,ready)=mpsc::channel();let effects=Arc::new(Effects{tray:Mutex::new(Ok(true)),probes:AtomicUsize::new(0),probe_entered:Mutex::new(None),probe_gate:Mutex::new(None),show_fail:AtomicBool::new(false),hide_fail:AtomicBool::new(false),hides:AtomicUsize::new(0),shows:AtomicUsize::new(0),seals:AtomicUsize::new(0),exits:AtomicUsize::new(0),updates:tx,entered,gate:Mutex::new(None),results:Mutex::new(VecDeque::new()),threads:Mutex::new(vec![]),deadlines:Mutex::new(vec![]),sealed:Mutex::new(vec![]),hide_gate:Mutex::new(None),hide_entered:Mutex::new(None),hidden_after_seal:AtomicBool::new(false)});
        (Residency::new(),effects,rx,ready)
    }
    fn phase(rx:&mpsc::Receiver<ResidencySnapshot>,expected:&str){while rx.recv_timeout(Duration::from_secs(2)).unwrap().phase!=expected{}}
    fn assert_lost_tray_keeps_entry(observation:Result<bool,String>){
        let(owner,effects,_,_)=fixture();owner.tray_result(Ok(()),effects.as_ref());assert!(owner.snapshot().tray_available);
        *effects.tray.lock().unwrap()=observation;owner.close_main(effects.as_ref());
        assert_eq!(effects.hides.load(Ordering::SeqCst),0,"cached tray success must not hide main after current tray loss");
        assert_eq!(effects.probes.load(Ordering::SeqCst),1);assert_eq!(effects.shows.load(Ordering::SeqCst),1);
        assert!(!owner.snapshot().tray_available);assert!(owner.snapshot().error.unwrap().contains("托盘"));
        assert!(owner.running());assert_eq!(effects.seals.load(Ordering::SeqCst),0);assert_eq!(effects.exits.load(Ordering::SeqCst),0);assert!(effects.threads.lock().unwrap().is_empty());
    }
    #[test]
    fn m2_main_x_rechecks_missing_or_unknown_tray_after_cached_success(){assert_lost_tray_keeps_entry(Ok(false));}
    #[test]
    fn m2_main_x_rechecks_tray_query_error_after_cached_success(){assert_lost_tray_keeps_entry(Err("托盘查询失败".into()));}
    #[test]
    fn m2_main_x_uses_recovered_tray_without_clearing_other_errors(){
        let(owner,effects,_,_)=fixture();owner.update(|s|s.record_error("primary worker failure".into()),effects.as_ref());
        *effects.tray.lock().unwrap()=Ok(false);owner.close_main(effects.as_ref());assert_eq!(effects.hides.load(Ordering::SeqCst),0);assert!(!owner.snapshot().tray_available);
        *effects.tray.lock().unwrap()=Ok(true);owner.close_main(effects.as_ref());
        assert_eq!(effects.probes.load(Ordering::SeqCst),2);assert_eq!(effects.hides.load(Ordering::SeqCst),1);assert!(owner.snapshot().tray_available);assert!(owner.snapshot().error.unwrap().contains("primary worker failure"));
        assert!(owner.running());assert_eq!(effects.seals.load(Ordering::SeqCst),0);assert!(effects.threads.lock().unwrap().is_empty());
    }
    #[test]
    fn m2_exit_can_seal_during_native_probe_and_late_success_never_hides(){
        let(owner,effects,updates,entered)=fixture();owner.tray_result(Ok(()),effects.as_ref());
        let(probing,probe_started)=mpsc::channel();let(release_probe,probe_gate)=mpsc::channel();*effects.probe_entered.lock().unwrap()=Some(probing);*effects.probe_gate.lock().unwrap()=Some(probe_gate);
        let(release_cleanup,cleanup_gate)=mpsc::channel();*effects.gate.lock().unwrap()=Some(cleanup_gate);
        let o=owner.clone();let e=effects.clone();let closing=std::thread::spawn(move||o.close_main(e.as_ref()));probe_started.recv_timeout(Duration::from_secs(1)).unwrap();
        let(done,done_rx)=mpsc::channel();let o=owner.clone();let e=effects.clone();let exiting=std::thread::spawn(move||{done.send(o.request_exit(e)).unwrap();});
        let admitted=done_rx.recv_timeout(Duration::from_secs(1));release_probe.send(()).unwrap();closing.join().unwrap();exiting.join().unwrap();entered.recv_timeout(Duration::from_secs(1)).unwrap();release_cleanup.send(()).unwrap();phase(&updates,"exited");
        assert!(admitted.is_ok(),"the native probe must not hold the exit admission lock");assert_eq!(admitted.unwrap().unwrap().phase,"stopping");
        assert_eq!(effects.hides.load(Ordering::SeqCst),0);assert_eq!(effects.seals.load(Ordering::SeqCst),1);assert_eq!(effects.probes.load(Ordering::SeqCst),1);assert!(!effects.hidden_after_seal.load(Ordering::SeqCst));
    }
    #[test]
    fn m2_tray_failure_x_retains_entry_and_hide_never_shutdown(){
        let(owner,effects,_,_)=fixture();*effects.tray.lock().unwrap()=Ok(false);owner.tray_result(Err("tray failed".into()),effects.as_ref());owner.close_main(effects.as_ref());assert_eq!(effects.hides.load(Ordering::SeqCst),0);assert_eq!(effects.seals.load(Ordering::SeqCst),0);
        *effects.tray.lock().unwrap()=Ok(true);owner.tray_result(Ok(()),effects.as_ref());owner.close_main(effects.as_ref());assert_eq!(effects.hides.load(Ordering::SeqCst),1);assert!(owner.running());assert!(effects.threads.lock().unwrap().is_empty());
        effects.hide_fail.store(true,Ordering::SeqCst);owner.close_main(effects.as_ref());assert!(owner.snapshot().error.unwrap().contains("hide"));assert!(effects.shows.load(Ordering::SeqCst)>=3);
        owner.tray_result(Err("tray update failed".into()),effects.as_ref());assert!(!owner.snapshot().tray_available);
    }
    #[test]
    fn m2_main_show_failure_closes_admission_without_invisible_cleanup(){
        let(owner,effects,rx,_)=fixture();effects.show_fail.store(true,Ordering::SeqCst);owner.request_exit(effects.clone()).unwrap();phase(&rx,"failed");assert_eq!(effects.seals.load(Ordering::SeqCst),1);assert!(effects.threads.lock().unwrap().is_empty());assert!(!owner.running());
    }
    #[test]
    fn m2_duplicate_exit_and_late_close_coalesce_then_retry_same_worker(){
        let(owner,effects,rx,entered)=fixture();let(release,gate)=mpsc::channel();*effects.gate.lock().unwrap()=Some(gate);effects.results.lock().unwrap().push_back(Err(CleanupError{message:"primary pending".into(),terminal:false}));
        owner.tray_result(Ok(()),effects.as_ref());owner.request_exit(effects.clone()).unwrap();entered.recv_timeout(Duration::from_secs(1)).unwrap();
        for _ in 0..10{owner.request_exit(effects.clone()).unwrap();owner.close_main(effects.as_ref());}
        assert_eq!(effects.seals.load(Ordering::SeqCst),1);assert_eq!(effects.hides.load(Ordering::SeqCst),0);assert!(!owner.running());release.send(()).unwrap();phase(&rx,"failed");
        owner.request_exit(effects.clone()).unwrap();entered.recv_timeout(Duration::from_secs(1)).unwrap();phase(&rx,"exited");
        let threads=effects.threads.lock().unwrap();assert_eq!(threads.len(),2);assert_eq!(threads[0],threads[1]);assert_eq!(*effects.deadlines.lock().unwrap(),*effects.sealed.lock().unwrap());assert!(owner.snapshot().error.unwrap().contains("primary pending"));
        owner.request_exit(effects.clone()).unwrap();assert_eq!(effects.seals.load(Ordering::SeqCst),2);
    }
    #[test]
    fn m2_confirmed_cleanup_failure_stays_failed_after_later_success(){
        let(owner,effects,rx,entered)=fixture();effects.results.lock().unwrap().push_back(Err(CleanupError{message:"confirmed panic".into(),terminal:true}));owner.request_exit(effects.clone()).unwrap();entered.recv_timeout(Duration::from_secs(1)).unwrap();phase(&rx,"failed");owner.request_exit(effects.clone()).unwrap();entered.recv_timeout(Duration::from_secs(1)).unwrap();phase(&rx,"failed");assert_eq!(effects.exits.load(Ordering::SeqCst),0);assert!(!owner.exited());
    }
    #[test]
    fn m2_residency_contract_fixture_shape(){
        let document:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/contracts/residency.json")).unwrap();
        let(owner,_,_,_)=fixture();let wire=serde_json::to_value(owner.snapshot()).unwrap();assert_eq!(wire,document["valid"][0]);
    }
    #[test]
    fn m2_cleanup_aggregates_panel_and_terminal_runtime_failures(){
        let error=cleanup_owners(Instant::now(),|_|Err(ContractError::new(ErrorCode::CleanupFailed,"hide failed")),|_|Err(ContractError::new(ErrorCode::CleanupFailed,"worker panic; listener cleanup failed; storage failed"))).unwrap_err();
        assert!(error.terminal);for cause in ["hide failed","worker panic","listener cleanup failed","storage failed"]{assert!(error.message.contains(cause));}
    }
    #[test]
    fn m2_failed_exit_preserves_primary_when_showing_main_also_fails(){
        let(owner,effects,_,_)=fixture();owner.update(|s|{s.phase="failed";s.error=Some("primary worker failure".into());},effects.as_ref());effects.show_fail.store(true,Ordering::SeqCst);owner.show(effects.as_ref());let error=owner.snapshot().error.unwrap();assert!(error.contains("primary worker failure"));assert!(error.contains("show failed"));
    }
    #[test]
    fn m2_main_x_and_exit_serialize_so_late_hide_cannot_hide_exit_progress(){
        let(owner,effects,updates,entered)=fixture();owner.tray_result(Ok(()),effects.as_ref());let(hiding,hidden_started)=mpsc::channel();let(release_hide,hide_gate)=mpsc::channel();*effects.hide_entered.lock().unwrap()=Some(hiding);*effects.hide_gate.lock().unwrap()=Some(hide_gate);
        let(release_cleanup,cleanup_gate)=mpsc::channel();*effects.gate.lock().unwrap()=Some(cleanup_gate);let o=owner.clone();let e=effects.clone();let hiding=std::thread::spawn(move||o.close_main(e.as_ref()));hidden_started.recv_timeout(Duration::from_secs(1)).unwrap();
        let(attempting,attempted)=mpsc::channel();let(done,done_rx)=mpsc::channel();let o=owner.clone();let e=effects.clone();let exiting=std::thread::spawn(move||{attempting.send(()).unwrap();let result=o.request_exit(e);done.send(result).unwrap();});attempted.recv_timeout(Duration::from_secs(1)).unwrap();
        let premature=done_rx.recv_timeout(Duration::from_millis(100));release_hide.send(()).unwrap();hiding.join().unwrap();if premature.is_err(){done_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();}exiting.join().unwrap();entered.recv_timeout(Duration::from_secs(1)).unwrap();release_cleanup.send(()).unwrap();phase(&updates,"exited");
        assert!(premature.is_err(),"exit must serialize after an already-admitted hide effect");assert!(!effects.hidden_after_seal.load(Ordering::SeqCst),"no hide effect may complete after closing admission");
    }
}
