//! The caller is the Tauri-injected window, never a client-supplied label.
use std::sync::Arc;
use crate::{contracts::*,host::HostRuntime};

pub fn validate_caller(label:&str,url:&tauri::Url)->ContractResult<()> {
    if !matches!(label,"main"|"quick-panel")||!crate::navigation_allowed(url,cfg!(debug_assertions)) {
        return Err(ContractError::new(ErrorCode::PermissionUnknown,"caller is not allowed"));
    }
    Ok(())
}
fn authorize(window:&tauri::WebviewWindow)->ContractResult<()> {
    validate_caller(window.label(),&window.url().map_err(|_|ContractError::new(ErrorCode::PermissionUnknown,"caller is not allowed"))?)
}
fn join_failed()->ContractError{ContractError::new(ErrorCode::CleanupFailed,"runtime request failed")}

#[tauri::command]
pub async fn get_status(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>)->ContractResult<StatusResult>{
    authorize(&window)?;let runtime=runtime.inner().clone();tauri::async_runtime::spawn_blocking(move||runtime.status()).await.map_err(|_|join_failed())
}
#[tauri::command]
pub async fn list_clips(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<ListClipsResult>{
    authorize(&window)?;let request=parse_list_clips_request(&request)?;let runtime=runtime.inner().clone();tauri::async_runtime::spawn_blocking(move||runtime.list(request)).await.map_err(|_|join_failed())?
}
#[tauri::command]
pub async fn set_capture_enabled(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<StatusResult>{
    authorize(&window)?;let enabled=parse_set_capture_enabled_request(&request)?;let runtime=runtime.inner().clone();tauri::async_runtime::spawn_blocking(move||runtime.set_enabled(enabled)).await.map_err(|_|join_failed())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_local_windows_can_call_runtime(){
        for label in ["main","quick-panel"]{assert!(validate_caller(label,&tauri::Url::parse("http://tauri.localhost/").unwrap()).is_ok());}
        for (label,url) in [("evil","http://tauri.localhost/"),("main","https://example.invalid/"),("main","http://tauri.localhost:7777/")]{assert_eq!(validate_caller(label,&tauri::Url::parse(url).unwrap()).unwrap_err().code,ErrorCode::PermissionUnknown);}
    }
}
