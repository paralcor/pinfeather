//! Win32 只读实现；系统 HGLOBAL 永不释放，所有复制都在一次剪贴板锁内。
use super::capture::{ClipboardApi,CaptureReason,Owner,MAX_FORMATS,EXCLUDE,HISTORY,CLOUD,IGNORE,INTERNAL};
use windows_sys::Win32::{Foundation::{GetLastError,SetLastError},System::{DataExchange::*,Memory::*}};

#[derive(Default)]
pub struct WindowsClipboard { opened: bool, registered:Option<RegisteredFormats> }

/// Runtime identifiers, not the spelling returned by an arbitrary producer.
pub struct RegisteredFormats(Vec<(u32,&'static str)>);
impl RegisteredFormats {
    pub fn id_for(&self,name:&str)->Option<u32>{if name=="CF_UNICODETEXT"{Some(13)}else{self.0.iter().find(|(_,n)|*n==name).map(|(id,_)|*id)}}
    pub fn register(mut register:impl FnMut(&str)->u32)->Result<Self,CaptureReason>{
        let mut formats=Vec::new();
        for name in ["HTML Format","PNG",EXCLUDE,HISTORY,CLOUD,IGNORE,INTERNAL] {
            let id=register(name);
            if !(0xc000..=0xffff).contains(&id)||formats.iter().any(|(key,_)|*key==id){return Err(CaptureReason::ReadFailed);}
            formats.push((id,name));
        }
        Ok(Self(formats))
    }
    pub fn canonical_name(&self,id:u32)->Option<&'static str>{if id==13 {Some("CF_UNICODETEXT")}else{self.0.iter().find(|(key,_)|*key==id).map(|(_,name)|*name)}}
}

pub struct WindowsWriter {formats:RegisteredFormats,opened:bool}
fn write_failed()->crate::contracts::ContractError{crate::contracts::ContractError::new(crate::contracts::ErrorCode::InjectionFailed,"clipboard write failed")}
impl WindowsWriter {
    pub fn new()->crate::contracts::ContractResult<Self>{
        let formats=RegisteredFormats::register(|name|{let wide:Vec<u16>=name.encode_utf16().chain([0]).collect();unsafe{RegisterClipboardFormatW(wide.as_ptr())}}).map_err(|_|write_failed())?;
        Ok(Self{formats,opened:false})
    }
}
impl Drop for WindowsWriter {fn drop(&mut self){if self.opened{unsafe{CloseClipboard();}}}}
impl super::origin::WriteApi for WindowsWriter {
    fn open(&mut self,owner:usize)->crate::contracts::ContractResult<()>{if owner==0||self.opened||unsafe{OpenClipboard(owner as _)}==0{return Err(write_failed());}self.opened=true;Ok(())}
    fn empty(&mut self)->crate::contracts::ContractResult<()>{if !self.opened||unsafe{EmptyClipboard()}==0{Err(write_failed())}else{Ok(())}}
    fn allocate(&mut self,bytes:&[u8])->crate::contracts::ContractResult<usize>{unsafe{
        let handle=GlobalAlloc(GMEM_MOVEABLE|GMEM_ZEROINIT,bytes.len());if handle.is_null(){return Err(write_failed());}
        let ptr=GlobalLock(handle);if ptr.is_null(){windows_sys::Win32::Foundation::GlobalFree(handle);return Err(write_failed());}
        std::ptr::copy_nonoverlapping(bytes.as_ptr(),ptr as *mut u8,bytes.len());GlobalUnlock(handle);Ok(handle as usize)
    }}
    fn set(&mut self,name:&str,handle:usize)->crate::contracts::ContractResult<()>{let id=self.formats.id_for(name).ok_or_else(write_failed)?;if !self.opened||unsafe{SetClipboardData(id,handle as _)}.is_null(){Err(write_failed())}else{Ok(())}}
    fn free(&mut self,handle:usize){unsafe{windows_sys::Win32::Foundation::GlobalFree(handle as _);}}
    fn close(&mut self)->crate::contracts::ContractResult<()>{if self.opened{if unsafe{CloseClipboard()}==0{return Err(write_failed());}self.opened=false;}Ok(())}
    fn sequence(&mut self)->u32{unsafe{GetClipboardSequenceNumber()}}
}

impl Drop for WindowsClipboard {
    fn drop(&mut self) {if self.opened {unsafe {CloseClipboard();}}}
}

impl ClipboardApi for WindowsClipboard {
    fn sequence(&mut self)->u32 {unsafe {GetClipboardSequenceNumber()}}
    fn open(&mut self)->bool {
        if self.opened {return false;}
        self.opened=unsafe {OpenClipboard(std::ptr::null_mut())!=0};self.opened
    }
    fn close(&mut self)->Result<(),CaptureReason>{
        if self.opened {
            if unsafe{CloseClipboard()}==0 {return Err(CaptureReason::CloseFailed);}
            self.opened=false;
        }
        Ok(())
    }
    fn owner(&mut self)->Owner {
        use windows_sys::Win32::{Foundation::CloseHandle,System::Threading::{OpenProcess,QueryFullProcessImageNameW,PROCESS_QUERY_LIMITED_INFORMATION},UI::WindowsAndMessaging::GetWindowThreadProcessId};
        if !self.opened {return Owner::default();}
        unsafe {
            let hwnd=GetClipboardOwner();if hwnd.is_null(){return Owner::default();}
            let mut pid=0;GetWindowThreadProcessId(hwnd,&mut pid);
            let mut result=Owner{hwnd:Some((hwnd as usize).to_string()),pid:if pid==0{None}else{Some(pid)},process:None};
            if pid!=0 {
                let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);
                if !process.is_null(){
                    let mut path=vec![0u16;1025];let mut len=path.len() as u32;
                    if QueryFullProcessImageNameW(process,0,path.as_mut_ptr(),&mut len)!=0 && len<=1024 {
                        result.process=String::from_utf16(&path[..len as usize]).ok();
                    }
                    CloseHandle(process);
                }
            }
            let mut after_pid=0;
            GetWindowThreadProcessId(hwnd,&mut after_pid);
            if GetClipboardOwner()!=hwnd || after_pid!=pid {return Owner::default();}
            result
        }
    }
    fn formats(&mut self)->Result<Vec<(u32,String)>,CaptureReason>{
        if !self.opened{return Err(CaptureReason::ReadFailed);}
        if self.registered.is_none(){
            self.registered=Some(RegisteredFormats::register(|name|{let wide:Vec<u16>=name.encode_utf16().chain([0]).collect();unsafe{RegisterClipboardFormatW(wide.as_ptr())}})?);
        }
        let mut result=Vec::new();let mut id=0;
        loop {
            unsafe {SetLastError(0);id=EnumClipboardFormats(id);}
            if id==0 {if unsafe{GetLastError()}!=0{return Err(CaptureReason::ReadFailed);}break;}
            if result.len()>=MAX_FORMATS{return Err(CaptureReason::ReadFailed);}
            let name=if let Some(name)=self.registered.as_ref().unwrap().canonical_name(id) {name.into()}else if id>=0xc000 {
                let mut name=[0u16;257];let len=unsafe{GetClipboardFormatNameW(id,name.as_mut_ptr(),name.len() as i32)};
                if len<=0 || len>256{return Err(CaptureReason::ReadFailed);}
                let name=String::from_utf16(&name[..len as usize]).map_err(|_|CaptureReason::ReadFailed)?;
                if name=="CF_UNICODETEXT" {"registered:CF_UNICODETEXT".into()}else{name}
            }else{format!("win32:{id}")};
            result.push((id,name));
        }
        Ok(result)
    }
    fn read(&mut self,id:u32,limit:usize)->Result<Vec<u8>,CaptureReason>{
        if !self.opened{return Err(CaptureReason::ReadFailed);}
        unsafe {
            let handle=GetClipboardData(id);if handle.is_null(){return Err(CaptureReason::ReadFailed);}
            let size=GlobalSize(handle);
            if size>limit {return Err(CaptureReason::InvalidPayload);}
            // A zero-sized HGLOBAL can also be a failed/invalid handle.
            if size==0 {return Err(CaptureReason::ReadFailed);}
            let ptr=GlobalLock(handle);if ptr.is_null(){return Err(CaptureReason::ReadFailed);}
            let data=std::slice::from_raw_parts(ptr as *const u8,size).to_vec();
            GlobalUnlock(handle);
            Ok(data)
        }
    }
}
