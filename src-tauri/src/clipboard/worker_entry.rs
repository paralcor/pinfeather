//! Same-executable worker pipe loop. No WebView, storage or input injection.
use std::io::{Read,Write};
use super::{capture::{capture_request,Clock},protocol::{HELLO_BODY,encode_frame,parse_frame_prefix,parse_worker_message_json,encode_snapshot,CONTROL_FRAME_MAX_BYTES},windows::WindowsClipboard};

struct MonotonicClock(std::time::Instant);
impl Clock for MonotonicClock {
    fn now_ms(&self)->u64 {self.0.elapsed().as_millis().min(u64::MAX as u128) as u64}
    fn sleep_ms(&mut self,ms:u64){std::thread::sleep(std::time::Duration::from_millis(ms));}
}

/// Callable only from internal worker mode; callers own process termination for
/// delayed-rendering hangs. EOF at a frame boundary is clean; partial EOF fails.
pub fn run()->Result<(), &'static str>{
    let mut input=std::io::stdin().lock();let mut output=std::io::stdout().lock();
    output.write_all(&encode_frame(HELLO_BODY.as_bytes(),CONTROL_FRAME_MAX_BYTES).map_err(|_|"worker hello failed")?).map_err(|_|"worker pipe failed")?;
    output.flush().map_err(|_|"worker pipe failed")?;
    let mut api=WindowsClipboard::default();let mut clock=MonotonicClock(std::time::Instant::now());
    loop {
        let mut prefix=[0u8;4];let mut n=0;
        while n<4 {
            let got=input.read(&mut prefix[n..]).map_err(|_|"worker request failed")?;
            if got==0 {return if n==0 {Ok(())}else{Err("partial worker request")};}n+=got;
        }
        let len=parse_frame_prefix(prefix,crate::contracts::WORKER_REQUEST_MAX_BYTES).map_err(|_|"worker request exceeds limit")?;
        let mut bytes=vec![0;len];input.read_exact(&mut bytes).map_err(|_|"partial worker request")?;
        let req=parse_worker_message_json(std::str::from_utf8(&bytes).map_err(|_|"invalid worker request")?).map_err(|_|"invalid worker request")?;
        let response=capture_request(&req,&mut api,&mut clock);
        output.write_all(&encode_snapshot(&response).map_err(|_|"worker response failed")?).map_err(|_|"worker pipe failed")?;
        output.flush().map_err(|_|"worker pipe failed")?;
        if response.reason==Some(super::capture::CaptureReason::CloseFailed){return Err("worker clipboard close failed");}
    }
}
