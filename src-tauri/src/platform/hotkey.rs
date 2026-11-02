//! A single owning thread registers, polls and unregisters the thread hotkey.
use std::{sync::{Arc,Mutex,mpsc::{self,SyncSender}},sync::atomic::{AtomicBool,Ordering},thread::JoinHandle,time::{Duration,Instant}};
pub trait HotkeyApi:Send {
    fn register(&mut self)->Result<(),String>;
    fn poll(&mut self)->Result<bool,String>;
    fn unregister(&mut self)->Result<(),String>;
}
struct OwnedThread { stop:SyncSender<mpsc::Sender<Result<(),String>>>,thread:JoinHandle<()>,stopping:Arc<AtomicBool> }
static RETAINED:Mutex<Vec<OwnedThread>>=Mutex::new(Vec::new());
pub struct HotkeyOwner { owned:Option<OwnedThread>,cleanup_failed:bool }
impl HotkeyOwner {
    pub fn start(api:Box<dyn HotkeyApi>,dispatch:Arc<dyn Fn()+Send+Sync>,failure:Arc<dyn Fn(String)+Send+Sync>)->(Self,Result<(),String>) {
        Self::start_with_budget(api,dispatch,failure,Duration::from_secs(3))
    }
    fn start_with_budget(mut api:Box<dyn HotkeyApi>,dispatch:Arc<dyn Fn()+Send+Sync>,failure:Arc<dyn Fn(String)+Send+Sync>,startup:Duration)->(Self,Result<(),String>) {
        let (stop,rx)=mpsc::sync_channel::<mpsc::Sender<Result<(),String>>>(1);let(tx,ready)=mpsc::sync_channel(1);let stopping=Arc::new(AtomicBool::new(false));let flag=stopping.clone();
        let thread=std::thread::Builder::new().name("manual-panel-hotkey".into()).spawn(move||{
            let registered=api.register();let _=tx.send(registered.clone());if registered.is_err(){return;}
            loop {
                match rx.recv_timeout(Duration::from_millis(15)) {
                    Ok(reply)=>{flag.store(true,Ordering::Release);let result=api.unregister();let done=result.is_ok();let _=reply.send(result);if done{return;}},
                    Err(mpsc::RecvTimeoutError::Disconnected)=>{flag.store(true,Ordering::Release);if api.unregister().is_ok(){return;}std::thread::sleep(Duration::from_millis(15));},
                    Err(mpsc::RecvTimeoutError::Timeout)=>{},
                }
                if !flag.load(Ordering::Acquire){match api.poll(){Ok(true)=>dispatch(),Err(error)=>{flag.store(true,Ordering::Release);failure(error);},_=>{}}}
            }
        });
        match thread {Ok(thread)=>{let owner=Self{owned:Some(OwnedThread{stop,thread,stopping}),cleanup_failed:false};let result=ready.recv_timeout(startup).unwrap_or_else(|_|Err("快捷键线程启动未确认。".into()));if result.is_err(){owner.owned.as_ref().unwrap().stopping.store(true,Ordering::Release);}(owner,result)},Err(_)=>(Self{owned:None,cleanup_failed:false},Err("快捷键线程创建失败。".into()))}
    }
    pub fn stop(&mut self,budget:Duration)->Result<(),String> {
        self.stop_until(Instant::now()+budget)
    }
    pub fn stop_until(&mut self,deadline:Instant)->Result<(),String> {
        if self.cleanup_failed{return Err("快捷键线程回收曾失败，保留退出失败状态。".into());}
        let Some(owned)=self.owned.as_ref() else{return Ok(());};owned.stopping.store(true,Ordering::Release);
        if !owned.thread.is_finished(){let(tx,rx)=mpsc::channel();match owned.stop.try_send(tx){
            Ok(())=>match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())){Ok(result)=>result?,Err(mpsc::RecvTimeoutError::Disconnected)=>{},Err(mpsc::RecvTimeoutError::Timeout)=>return Err("快捷键停止未完成。".into())},
            Err(mpsc::TrySendError::Disconnected(_))=>{},
            Err(mpsc::TrySendError::Full(_))=>return Err("快捷键停止请求未接纳。".into()),
        }}
        while !owned.thread.is_finished(){if Instant::now()>=deadline{return Err("快捷键线程仍归宿主所有，退出未完成。".into());}std::thread::sleep(Duration::from_millis(1));}
        self.owned.take().unwrap().thread.join().map_err(|_|{self.cleanup_failed=true;"快捷键线程异常退出。".to_string()})
    }
    #[cfg(test)]fn retains_thread(&self)->bool{self.owned.is_some()}
}
impl Drop for HotkeyOwner {fn drop(&mut self){if self.stop(Duration::from_millis(100)).is_err(){if let Some(owned)=self.owned.take(){RETAINED.lock().unwrap_or_else(|e|e.into_inner()).push(owned);}}}}

pub struct WindowsHotkey;
impl HotkeyApi for WindowsHotkey {
    fn register(&mut self)->Result<(),String>{use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey,MOD_CONTROL,MOD_ALT,MOD_NOREPEAT};if unsafe{RegisterHotKey(std::ptr::null_mut(),1,MOD_CONTROL|MOD_ALT|MOD_NOREPEAT,b'V' as u32)}==0{Err("Ctrl+Alt+V 注册失败，可能已被占用；可使用主窗口入口。".into())}else{Ok(())}}
    fn poll(&mut self)->Result<bool,String>{use windows_sys::Win32::UI::WindowsAndMessaging::{PeekMessageW,MSG,PM_REMOVE,WM_HOTKEY};let mut msg:MSG=unsafe{std::mem::zeroed()};let mut pressed=false;for _ in 0..32{if unsafe{PeekMessageW(&mut msg,std::ptr::null_mut(),0,0,PM_REMOVE)}==0{break;}if msg.message==WM_HOTKEY&&msg.wParam==1{pressed=true;}}Ok(pressed)}
    fn unregister(&mut self)->Result<(),String>{use windows_sys::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;if unsafe{UnregisterHotKey(std::ptr::null_mut(),1)}==0{Err("快捷键注销失败，保留线程以便重试退出。".into())}else{Ok(())}}
}

#[cfg(test)]mod tests {
    use super::*;use std::sync::atomic::AtomicUsize;
    struct Fake {register_fail:bool,unregister_fail:Arc<AtomicBool>,registered:Arc<AtomicBool>,polls:Arc<AtomicUsize>}
    impl HotkeyApi for Fake {
        fn register(&mut self)->Result<(),String>{if self.register_fail{return Err("conflict".into());}self.registered.store(true,Ordering::SeqCst);Ok(())}
        fn poll(&mut self)->Result<bool,String>{self.polls.fetch_add(1,Ordering::SeqCst);Ok(true)}
        fn unregister(&mut self)->Result<(),String>{if self.unregister_fail.load(Ordering::SeqCst){Err("retry".into())}else{self.registered.store(false,Ordering::SeqCst);Ok(())}}
    }
    fn start(fail:bool)->(HotkeyOwner,Result<(),String>,Arc<AtomicBool>,Arc<AtomicBool>){let unregister=Arc::new(AtomicBool::new(false));let registered=Arc::new(AtomicBool::new(false));let(owner,result)=HotkeyOwner::start(Box::new(Fake{register_fail:fail,unregister_fail:unregister.clone(),registered:registered.clone(),polls:Arc::new(AtomicUsize::new(0))}),Arc::new(||{}),Arc::new(|_|{}));(owner,result,unregister,registered)}
    #[test]fn manual_panel_hotkey_conflict_retains_fallback_and_reaps_thread(){let(mut owner,result,_,registered)=start(true);assert!(result.is_err());owner.stop(Duration::from_secs(1)).unwrap();assert!(!registered.load(Ordering::SeqCst));assert!(!owner.retains_thread());owner.stop(Duration::ZERO).unwrap();}
    #[test]fn manual_panel_hotkey_unregister_failure_retains_owner_and_retry(){let(mut owner,result,fail,registered)=start(false);result.unwrap();fail.store(true,Ordering::SeqCst);assert!(owner.stop(Duration::from_secs(1)).is_err());assert!(owner.retains_thread());assert!(registered.load(Ordering::SeqCst));fail.store(false,Ordering::SeqCst);owner.stop(Duration::from_secs(1)).unwrap();assert!(!registered.load(Ordering::SeqCst));assert!(!owner.retains_thread());owner.stop(Duration::ZERO).unwrap();}
    #[test]fn manual_panel_hotkey_panic_is_sticky_after_join(){struct PanicApi;impl HotkeyApi for PanicApi{fn register(&mut self)->Result<(),String>{panic!("synthetic startup panic")}fn poll(&mut self)->Result<bool,String>{unreachable!()}fn unregister(&mut self)->Result<(),String>{unreachable!()}}let(mut owner,result)=HotkeyOwner::start(Box::new(PanicApi),Arc::new(||{}),Arc::new(|_|{}));assert!(result.is_err());while !owner.owned.as_ref().unwrap().thread.is_finished(){std::thread::yield_now();}assert!(owner.stop(Duration::from_secs(1)).is_err());assert!(owner.stop(Duration::from_secs(1)).is_err(),"consumed failed join must remain failure");}
    #[test]fn manual_panel_hotkey_partial_start_timeout_keeps_thread_and_suppresses_late_registration(){
        struct Slow {release:mpsc::Receiver<()>,unregistered:Arc<AtomicBool>}
        impl HotkeyApi for Slow{fn register(&mut self)->Result<(),String>{self.release.recv().unwrap();Ok(())}fn poll(&mut self)->Result<bool,String>{Ok(true)}fn unregister(&mut self)->Result<(),String>{self.unregistered.store(true,Ordering::SeqCst);Ok(())}}
        let(tx,rx)=mpsc::channel();let unregistered=Arc::new(AtomicBool::new(false));let calls=Arc::new(AtomicUsize::new(0));let c=calls.clone();
        let(mut owner,result)=HotkeyOwner::start_with_budget(Box::new(Slow{release:rx,unregistered:unregistered.clone()}),Arc::new(move||{c.fetch_add(1,Ordering::SeqCst);}),Arc::new(|_|{}),Duration::ZERO);
        assert!(result.is_err());assert!(owner.retains_thread());assert!(owner.stop(Duration::ZERO).is_err());assert!(owner.retains_thread());tx.send(()).unwrap();
        // A pending timed-out stop may complete before this retry; both paths must reap it.
        let deadline=Instant::now()+Duration::from_secs(1);while !owner.owned.as_ref().unwrap().thread.is_finished(){assert!(Instant::now()<deadline);std::thread::yield_now();}
        owner.stop(Duration::from_secs(1)).unwrap();assert!(unregistered.load(Ordering::SeqCst));assert_eq!(calls.load(Ordering::SeqCst),0);assert!(!owner.retains_thread());
    }
}
