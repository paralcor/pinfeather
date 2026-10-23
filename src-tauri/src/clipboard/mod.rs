//! 内部抓取：协议、安全只读快照、消息监听与 worker 生命周期。

pub mod protocol;
pub mod worker;
pub mod capture;
pub mod windows;
pub mod worker_entry;
pub mod listener;
pub mod origin;
