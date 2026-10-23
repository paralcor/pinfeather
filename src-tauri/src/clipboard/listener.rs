//! Hidden message window. The callback observes metadata only and never reads payloads.
use std::{sync::{Arc,Mutex,mpsc,atomic::{AtomicBool,AtomicU64,Ordering}},thread::JoinHandle,time::{Duration,Instant}};
use crate::contracts::{ContractError,ContractResult,ErrorCode};
use windows_sys::Win32::{Foundation::HWND,System::DataExchange::{AddClipboardFormatListener,RemoveClipboardFormatListener,GetClipboardSequenceNumber},UI::WindowsAndMessaging::*};

type Callback=Arc<dyn Fn(u32,u64)+Send+Sync>;
struct Context { callback:Callback, registered:bool, failed:Arc<AtomicBool>, started:Instant }
static RETAINED:Mutex<Vec<JoinHandle<()>>>=Mutex::new(Vec::new());
static NEXT:AtomicU64=AtomicU64::new(0);
static BROKEN:AtomicBool=AtomicBool::new(false);

fn failure()->ContractError {ContractError::new(ErrorCode::CleanupFailed,"clipboard listener unavailable")}

pub struct ClipboardListener {hwnd:usize,thread:Option<JoinHandle<()>>,failed:Arc<AtomicBool>}
impl ClipboardListener {
    pub fn owner_hwnd(&self)->usize{self.hwnd}
    pub fn start(callback:Callback)->ContractResult<Self>{Self::start_inner(callback,true)}
    fn start_inner(callback:Callback,register:bool)->ContractResult<Self>{
        if BROKEN.load(Ordering::Acquire)||!RETAINED.lock().unwrap_or_else(|e|e.into_inner()).is_empty(){return Err(failure());}
        let failed=Arc::new(AtomicBool::new(false));let thread_failed=failed.clone();let(tx,rx)=mpsc::sync_channel(1);
        let thread=std::thread::Builder::new().name("clipboard-listener".into()).spawn(move||{
            let result=unsafe{message_loop(callback,register,thread_failed.clone(),tx)};
            if result.is_err()||thread_failed.load(Ordering::Acquire){thread_failed.store(true,Ordering::Release);BROKEN.store(true,Ordering::Release);}
        }).map_err(|_|failure())?;
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(hwnd))=>Ok(Self{hwnd,thread:Some(thread),failed}),
            _=>{
                // Dropping rx makes a late startup abort before entering its pump.
                drop(rx);let mut listener=Self{hwnd:0,thread:Some(thread),failed};let _=listener.close();Err(failure())
            }
        }
    }
    pub fn healthy(&self)->bool {self.hwnd!=0&&!self.failed.load(Ordering::Acquire)&&self.thread.as_ref().is_some_and(|t|!t.is_finished())}
    pub fn close(&mut self)->ContractResult<()> {
        let Some(thread)=self.thread.as_ref() else{return Ok(());};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{System::Threading::{WaitForSingleObject,GetThreadId},Foundation::WAIT_OBJECT_0};
        // Address the owned pump thread, not a potentially recycled HWND after
        // a previous close request. Keep its native handle until join confirms exit.
        if self.hwnd!=0 && unsafe{WaitForSingleObject(thread.as_raw_handle(),0)}!=WAIT_OBJECT_0 {
            unsafe{PostThreadMessageW(GetThreadId(thread.as_raw_handle()),WM_QUIT,0,0);}
        }
        if unsafe{WaitForSingleObject(thread.as_raw_handle(),2000)}!=WAIT_OBJECT_0 {return Err(failure());}
        let joined=self.thread.take().unwrap().join();self.hwnd=0;
        if joined.is_err()||self.failed.load(Ordering::Acquire){Err(failure())}else{Ok(())}
    }
}
impl Drop for ClipboardListener {
    fn drop(&mut self){let _=self.close();if let Some(thread)=self.thread.take(){RETAINED.lock().unwrap_or_else(|e|e.into_inner()).push(thread);}}
}

unsafe extern "system" fn window_proc(hwnd:HWND,msg:u32,w:usize,l:isize)->isize {
    if msg==WM_NCCREATE {let create=&*(l as *const CREATESTRUCTW);SetWindowLongPtrW(hwnd,GWLP_USERDATA,create.lpCreateParams as isize);}
    let ctx=GetWindowLongPtrW(hwnd,GWLP_USERDATA) as *mut Context;
    if !ctx.is_null(){match msg {
        WM_CLIPBOARDUPDATE=>{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||((*ctx).callback)(GetClipboardSequenceNumber(),(*ctx).started.elapsed().as_millis().min(u64::MAX as u128) as u64)));
            if result.is_err(){(*ctx).failed.store(true,Ordering::Release);PostMessageW(hwnd,WM_CLOSE,0,0);}return 0;
        }
        WM_CLOSE=>{if (*ctx).registered {if RemoveClipboardFormatListener(hwnd)==0{(*ctx).failed.store(true,Ordering::Release);}(*ctx).registered=false;}DestroyWindow(hwnd);return 0;}
        WM_DESTROY=>{PostQuitMessage(0);return 0;}
        WM_NCDESTROY=>{SetWindowLongPtrW(hwnd,GWLP_USERDATA,0);}
        _=>{}
    }}
    DefWindowProcW(hwnd,msg,w,l)
}

unsafe fn message_loop(callback:Callback,register:bool,failed:Arc<AtomicBool>,ready:mpsc::SyncSender<Result<usize,()>>)->Result<(),()> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    let instance=GetModuleHandleW(std::ptr::null());
    let name:Vec<u16>=format!("Pinfeather.Listener.{}.{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)).encode_utf16().chain([0]).collect();
    let mut class:WNDCLASSW=std::mem::zeroed();class.lpfnWndProc=Some(window_proc);class.hInstance=instance;class.lpszClassName=name.as_ptr();
    if RegisterClassW(&class)==0 {let _=ready.send(Err(()));return Err(());}
    let mut ctx=Box::new(Context{callback,registered:false,failed,started:Instant::now()});
    let hwnd=CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,0,0,HWND_MESSAGE,std::ptr::null_mut(),instance,&mut *ctx as *mut Context as _);
    if hwnd.is_null(){UnregisterClassW(name.as_ptr(),instance);let _=ready.send(Err(()));return Err(());}
    let initialized=!register||AddClipboardFormatListener(hwnd)!=0;
    ctx.registered=register&&initialized;
    if !initialized || ready.send(Ok(hwnd as usize)).is_err() {
        if ctx.registered {RemoveClipboardFormatListener(hwnd);ctx.registered=false;}
        SetWindowLongPtrW(hwnd,GWLP_USERDATA,0);
        DestroyWindow(hwnd);UnregisterClassW(name.as_ptr(),instance);return Err(());
    }
    let mut msg:MSG=std::mem::zeroed();let mut result=loop {
        let got=GetMessageW(&mut msg,std::ptr::null_mut(),0,0);
        if got==0 {break Ok(());}if got<0 {break Err(());}TranslateMessage(&msg);DispatchMessageW(&msg);
    };
    if IsWindow(hwnd)!=0 {
        if ctx.registered && RemoveClipboardFormatListener(hwnd)==0 {result=Err(());}
        // Even if native destruction fails, no later teardown callback may
        // retain a pointer to the Rust context that is about to be dropped.
        SetWindowLongPtrW(hwnd,GWLP_USERDATA,0);
        if DestroyWindow(hwnd)==0 {result=Err(());}
    }
    if UnregisterClassW(name.as_ptr(),instance)==0{return Err(());}result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn message_thread_readiness_and_repeated_shutdown_without_clipboard_registration(){
        // Native window/pump/cleanup, deliberately no clipboard registration or query.
        for _ in 0..3 {let mut listener=ClipboardListener::start_inner(Arc::new(|_,_|panic!("no clipboard callbacks expected")),false).unwrap();assert!(listener.healthy());listener.close().unwrap();listener.close().unwrap();assert!(!listener.healthy());}
    }
}
