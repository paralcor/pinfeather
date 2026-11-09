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

fn measurement_caller(window:&tauri::WebviewWindow)->crate::measurement::Result<()> {authorize(window).map_err(|_|crate::measurement::MeasureError::WrongWindow)}
fn measurement_request<T:serde::de::DeserializeOwned>(value:serde_json::Value)->crate::measurement::Result<T>{serde_json::from_value(value).map_err(|_|crate::measurement::MeasureError::InvalidContract)}
#[tauri::command]
pub fn measure_start_model(window:tauri::WebviewWindow,measurement:tauri::State<'_,crate::measurement::Measurement>,request:serde_json::Value)->crate::measurement::Result<crate::measurement::Identity>{
    measurement_caller(&window)?;let request:crate::measurement::StartRequest=measurement_request(request)?;if request.schema!=1{return Err(crate::measurement::MeasureError::InvalidContract);}
    let host=measurement.enabled()?;
    if window.label()=="quick-panel"{use tauri::Manager;let panel=window.try_state::<Arc<crate::manual_panel::PanelHost>>().ok_or(crate::measurement::MeasureError::Stale)?;let snapshot=panel.sessions.snapshot();let session=snapshot.session_id.ok_or(crate::measurement::MeasureError::Stale)?;panel.sessions.admit(&session,||host.start_model(window.label())).map_err(|_|crate::measurement::MeasureError::Stale)?}else{host.start_model(window.label())}
}
#[tauri::command]
pub fn measure_invalidate(window:tauri::WebviewWindow,measurement:tauri::State<'_,crate::measurement::Measurement>,request:serde_json::Value)->crate::measurement::Result<()>{measurement_caller(&window)?;measurement.enabled()?.invalidate(window.label(),&measurement_request(request)?)}
#[tauri::command]
pub fn measure_stop_model(window:tauri::WebviewWindow,measurement:tauri::State<'_,crate::measurement::Measurement>,request:serde_json::Value)->crate::measurement::Result<()>{measurement_caller(&window)?;measurement.enabled()?.stop_model(window.label(),&measurement_request(request)?)}
#[tauri::command]
pub async fn measure_list_clips(window:tauri::WebviewWindow,measurement:tauri::State<'_,crate::measurement::Measurement>,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->crate::measurement::Result<crate::measurement::MeasuredPage>{
    measurement_caller(&window)?;let host=measurement.enabled()?.clone();let label=window.label().to_string();let request=measurement_request(request)?;let runtime=runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move||host.list(&label,request,&runtime)).await.map_err(|_|crate::measurement::MeasureError::Stopped)?
}
#[tauri::command]
pub fn measure_ready(window:tauri::WebviewWindow,measurement:tauri::State<'_,crate::measurement::Measurement>,request:serde_json::Value)->crate::measurement::Result<()>{
    measurement_caller(&window)?;let request:crate::measurement::ReadyRequest=measurement_request(request)?;let receipt=request.receipt();let host=measurement.enabled()?;
    if window.label()=="quick-panel"{use tauri::Manager;let panel=window.try_state::<Arc<crate::manual_panel::PanelHost>>().ok_or(crate::measurement::MeasureError::Stale)?;let session=receipt.session.as_deref().ok_or(crate::measurement::MeasureError::Stale)?;panel.sessions.admit(session,||host.ack(window.label(),&receipt,request.input_focused,||crate::measurement::native_state(&window))).map_err(|_|crate::measurement::MeasureError::Stale)?}else{host.ack(window.label(),&receipt,request.input_focused,||crate::measurement::native_state(&window))}
}

#[tauri::command]
pub fn get_settings_state(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>)->ContractResult<SettingsSnapshot>{authorize(&window)?;Ok(runtime.settings())}
fn settings_request(label:&str,url:&tauri::Url,request:&serde_json::Value)->ContractResult<Vec<String>>{validate_main_caller(label,url)?;parse_set_excluded_apps_request(request)}
#[tauri::command]
pub async fn apply_excluded_apps(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<SettingsSnapshot>{
    let url=window.url().map_err(|_|ContractError::new(ErrorCode::PermissionUnknown,"caller is not allowed"))?;
    let paths=settings_request(window.label(),&url,&request)?;let runtime=runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move||runtime.apply_exclusions(paths)).await.map_err(|_|join_failed())?
}

#[tauri::command]
pub fn get_residency_state(window:tauri::WebviewWindow,owner:tauri::State<'_,Arc<crate::residency::DesktopResidency>>)->ContractResult<crate::residency::ResidencySnapshot>{authorize(&window)?;Ok(owner.coordinator.snapshot())}
#[tauri::command]
pub fn request_exit(window:tauri::WebviewWindow,owner:tauri::State<'_,Arc<crate::residency::DesktopResidency>>)->ContractResult<crate::residency::ResidencySnapshot>{authorize_main(&window)?;owner.coordinator.request_exit(owner.effects.clone())}
#[tauri::command]
pub async fn retry_exit_saving(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>)->ContractResult<StatusResult>{authorize_main(&window)?;let runtime=runtime.inner().clone();tauri::async_runtime::spawn_blocking(move||runtime.retry_saving()).await.map_err(|_|join_failed())?}

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
    #[cfg(feature="test-fixtures")]
    #[test]
    fn m2_settings_unauthorized_updates_have_zero_policy_and_event_side_effects(){
        use std::sync::atomic::{AtomicUsize,Ordering};let events=Arc::new(AtomicUsize::new(0));let count=events.clone();let runtime=HostRuntime::for_test_memory(Arc::new(move|_,_|{count.fetch_add(1,Ordering::SeqCst);})).unwrap();
        let before=serde_json::to_value(runtime.settings()).unwrap();let observed=events.load(Ordering::SeqCst);
        for(label,url)in[("quick-panel","http://tauri.localhost/"),("main","https://example.invalid/"),("forged-main","http://tauri.localhost/")]{let result=settings_request(label,&tauri::Url::parse(url).unwrap(),&serde_json::json!({"paths":[r"C:\Private\vault.exe"]})).and_then(|paths|runtime.apply_exclusions(paths));assert_eq!(result.unwrap_err().code,ErrorCode::PermissionUnknown);}
        assert_eq!(serde_json::to_value(runtime.settings()).unwrap(),before);assert_eq!(events.load(Ordering::SeqCst),observed);runtime.shutdown(std::time::Duration::from_secs(1)).unwrap();
    }
    #[test]fn m2_management_actions_require_real_local_main_authority(){let local=tauri::Url::parse("http://tauri.localhost/").unwrap();assert!(validate_caller("quick-panel",&local).is_ok());assert!(validate_main_caller("quick-panel",&local).is_err());for(label,url)in[("main","https://example.invalid/"),("main","http://tauri.localhost:7/"),("forged-main","http://tauri.localhost/")]{assert!(validate_main_caller(label,&tauri::Url::parse(url).unwrap()).is_err());}assert!(validate_main_caller("main",&local).is_ok());}
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
