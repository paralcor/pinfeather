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
pub fn validate_main_caller(label:&str,url:&tauri::Url)->ContractResult<()> {validate_caller(label,url)?;if label!="main"{return Err(ContractError::new(ErrorCode::PermissionUnknown,"main window required"));}Ok(())}
fn authorize_main(window:&tauri::WebviewWindow)->ContractResult<()>{validate_main_caller(window.label(),&window.url().map_err(|_|ContractError::new(ErrorCode::PermissionUnknown,"caller is not allowed"))?)}
pub fn validate_panel_caller(label:&str,url:&tauri::Url)->ContractResult<()>{validate_caller(label,url)?;if label!="quick-panel"{return Err(ContractError::new(ErrorCode::PermissionUnknown,"panel window required"));}Ok(())}
fn authorize_panel(window:&tauri::WebviewWindow)->ContractResult<()>{validate_panel_caller(window.label(),&window.url().map_err(|_|ContractError::new(ErrorCode::PermissionUnknown,"caller is not allowed"))?)}
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
pub async fn get_clip_text(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<GetClipTextResult>{
    authorize(&window)?;let request=parse_get_clip_text_request(&request)?;let runtime=runtime.inner().clone();tauri::async_runtime::spawn_blocking(move||runtime.get_clip_text(request)).await.map_err(|_|join_failed())?
}
#[tauri::command]
pub async fn copy_clip_text(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<CopyClipTextResult>{
    authorize_main(&window)?;let request=parse_copy_clip_text_request(&request)?;let runtime=runtime.inner().clone();
    let uncertain=CopyClipTextResult::new(&request,CopyOutcome::Unknown,Some(ErrorCode::CleanupFailed));
    tauri::async_runtime::spawn_blocking(move||runtime.copy_clip_text(request)).await.unwrap_or(Ok(uncertain))
}
#[tauri::command]
pub async fn set_capture_enabled(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<StatusResult>{
    authorize_main(&window)?;let enabled=parse_set_capture_enabled_request(&request)?;let runtime=runtime.inner().clone();tauri::async_runtime::spawn_blocking(move||runtime.set_enabled(enabled)).await.map_err(|_|join_failed())?
}

#[tauri::command]
pub fn get_manual_panel_state(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize(&window)?;Ok(panel.sessions.snapshot())}
#[tauri::command]
pub fn open_manual_panel(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize_main(&window)?;panel.sessions.open(&panel.window)}
#[tauri::command]
pub fn ready_manual_panel(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>,request:serde_json::Value)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize_panel(&window)?;let session=parse_panel_session(&request)?;panel.sessions.ready(&session,&panel.window)}
#[tauri::command]
pub fn hide_manual_panel(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>,request:serde_json::Value)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize_panel(&window)?;let session=parse_panel_session(&request)?;panel.sessions.hide(&session,&panel.window)}
#[derive(serde::Serialize)]
pub struct PanelCopyResult {session_id:String,result:CopyClipTextResult}
#[tauri::command]
pub async fn copy_manual_panel_text(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<PanelCopyResult>{
    authorize_panel(&window)?;let(session,request)=parse_panel_copy(&request)?;let panel=panel.inner().clone();let runtime=runtime.inner().clone();let original=session.clone();let uncertain=CopyClipTextResult::new(&request,CopyOutcome::Unknown,Some(ErrorCode::CleanupFailed));
    let result=tauri::async_runtime::spawn_blocking(move||runtime.copy_panel_text(&panel.sessions,&session,request)).await.unwrap_or(Ok(uncertain))?;Ok(PanelCopyResult{session_id:original,result})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn manual_panel_generic_copy_is_main_only(){assert!(validate_main_caller("quick-panel",&tauri::Url::parse("http://tauri.localhost/").unwrap()).is_err());}
    #[test]fn manual_panel_authority_rejects_unknown_external_and_cross_window_actions(){
        let local=tauri::Url::parse("http://tauri.localhost/index.html?panel=1").unwrap();assert!(validate_panel_caller("quick-panel",&local).is_ok());assert!(validate_panel_caller("main",&local).is_err());
        for(label,url)in[("unknown","http://tauri.localhost/"),("quick-panel","https://example.invalid/"),("quick-panel","http://tauri.localhost:8888/"),("quick-panel","file:///C:/index.html")]{let url=tauri::Url::parse(url).unwrap();assert!(validate_panel_caller(label,&url).is_err());assert!(validate_main_caller(label,&url).is_err());}
    }
    #[test]
    fn only_known_local_windows_can_call_runtime(){
        for label in ["main","quick-panel"]{assert!(validate_caller(label,&tauri::Url::parse("http://tauri.localhost/").unwrap()).is_ok());}
        for (label,url) in [("evil","http://tauri.localhost/"),("main","https://example.invalid/"),("main","http://tauri.localhost:7777/")]{assert_eq!(validate_caller(label,&tauri::Url::parse(url).unwrap()).unwrap_err().code,ErrorCode::PermissionUnknown);}
    }
}
