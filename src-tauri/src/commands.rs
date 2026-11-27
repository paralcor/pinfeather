//! The caller is the Tauri-injected window, never a client-supplied label.
use std::sync::Arc;
use crate::{contracts::*,host::HostRuntime};
use crate::startup_capture::StartupCaptureSnapshot;

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
pub(crate) async fn get_startup_capture(
    window: tauri::WebviewWindow, runtime: tauri::State<'_, Arc<HostRuntime>>,
) -> ContractResult<StartupCaptureSnapshot> {
    authorize_main(&window)?;
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.startup_capture()).await.map_err(|_| join_failed())
}

fn startup_capture_request(label: &str, url: &tauri::Url, request: &serde_json::Value) -> ContractResult<bool> {
    validate_main_caller(label, url)?;
    parse_set_capture_enabled_request(request)
}

#[tauri::command]
pub(crate) async fn set_startup_capture(
    window: tauri::WebviewWindow, runtime: tauri::State<'_, Arc<HostRuntime>>, request: serde_json::Value,
) -> ContractResult<StartupCaptureSnapshot> {
    let url = window.url().map_err(|_| ContractError::new(ErrorCode::PermissionUnknown, "caller is not allowed"))?;
    let enabled = startup_capture_request(window.label(), &url, &request)?;
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.set_startup_capture(enabled)).await.map_err(|_| join_failed())?
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
#[derive(serde::Serialize)]
pub struct PngPreviewResult {
    request_id: String,
    clip_id: String,
    #[serde(flatten)]
    thumbnail: crate::png_clip::Thumbnail,
}

fn png_preview_request(
    label: &str, url: &tauri::Url, value: &serde_json::Value,
) -> ContractResult<(Option<String>, CopyClipTextRequest)> {
    if label == "main" {
        validate_main_caller(label, url)?;
        Ok((None, parse_copy_clip_text_request(value)?))
    } else {
        validate_panel_caller(label, url)?;
        let (session, request) = parse_panel_copy(value)?;
        Ok((Some(session), request))
    }
}

#[tauri::command]
pub async fn get_clip_png_preview(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<HostRuntime>>,
    panel: tauri::State<'_, Arc<crate::manual_panel::PanelHost>>,
    request: serde_json::Value,
) -> ContractResult<PngPreviewResult> {
    static PREVIEW: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let url = window.url().map_err(|_| ContractError::new(ErrorCode::PermissionUnknown, "caller is not allowed"))?;
    let (session, request) = png_preview_request(window.label(), &url, &request)?;
    let runtime = runtime.inner().clone();
    let panel = panel.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = match PREVIEW.try_lock() {
            Ok(permit) => permit,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err(ContractError::new(ErrorCode::Busy, "PNG preview is pending")),
        };
        let session = session.as_deref().map(|id| (&panel.sessions, id));
        let data = runtime.get_png_bytes(positive_id_as_i64(&request.clip_id), session)?;
        let thumbnail = crate::png_clip::thumbnail(&data)?;
        if let Some((sessions, id)) = session { sessions.admit(id, || ())?; }
        Ok(PngPreviewResult { request_id: request.request_id, clip_id: request.clip_id, thumbnail })
    }).await.map_err(|_| join_failed())?
}
#[tauri::command]
pub async fn copy_clip_text(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<CopyClipTextResult>{
    authorize_main(&window)?;let request=parse_copy_clip_text_request(&request)?;let runtime=runtime.inner().clone();
    let uncertain=CopyClipTextResult::new(&request,CopyOutcome::Unknown,Some(ErrorCode::CleanupFailed));
    tauri::async_runtime::spawn_blocking(move||runtime.copy_clip_text(request)).await.unwrap_or(Ok(uncertain))
}
#[tauri::command]
pub async fn copy_clip_png(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<HostRuntime>>,
    request: serde_json::Value,
) -> ContractResult<CopyClipTextResult> {
    authorize_main(&window)?;
    let request = parse_copy_clip_text_request(&request)?;
    let uncertain = CopyClipTextResult::new(&request, CopyOutcome::Unknown, Some(ErrorCode::CleanupFailed));
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.copy_clip(request, crate::clipboard::origin::WriteFormat::Png))
        .await.unwrap_or(Ok(uncertain))
}
pub(crate) fn set_capture_enabled(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    use tauri::ipc::{CommandArg, CommandItem, Invoke, InvokeError};

    let Invoke { message, resolver, acl } = invoke;
    let argument = |key| CommandItem { plugin: None, name: "set_capture_enabled", key, message: &message, acl: &acl };
    let enqueue = || -> Result<_, InvokeError> {
        let window = tauri::WebviewWindow::from_command(argument("window"))?;
        let runtime = tauri::State::<Arc<HostRuntime>>::from_command(argument("runtime"))?;
        let request = serde_json::Value::from_command(argument("request"))?;
        authorize_main(&window)?;
        let enabled = parse_set_capture_enabled_request(&request)?;
        let runtime = runtime.inner().clone();
        // Number valid requests before Tauri schedules any async or blocking task.
        let sequence = runtime.reserve_manual_capture_sequence();
        Ok(tauri::async_runtime::spawn_blocking(move || runtime.set_enabled_in_order(sequence, enabled)))
    };
    match enqueue() {
        Ok(pending) => resolver.respond_async(async move {
            pending.await.map_err(|_| join_failed())?.map_err(Into::into)
        }),
        Err(error) => resolver.invoke_error(error),
    }
    true
}

#[tauri::command]
pub fn get_manual_panel_state(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize(&window)?;Ok(panel.sessions.snapshot())}
#[tauri::command]
pub fn open_manual_panel(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize_main(&window)?;panel.sessions.open(&panel.window)}
#[tauri::command]
pub fn ready_manual_panel(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>,request:serde_json::Value)->ContractResult<crate::manual_panel::PanelSnapshot>{authorize_panel(&window)?;let session=parse_panel_session(&request)?;panel.sessions.ready(&session,&panel.window)}
#[tauri::command]
pub fn hide_manual_panel(
    window: tauri::WebviewWindow,
    panel: tauri::State<'_, Arc<crate::manual_panel::PanelHost>>,
    request: serde_json::Value,
    after_copy: Option<bool>,
) -> ContractResult<crate::manual_panel::PanelSnapshot> {
    authorize_panel(&window)?;
    let session = parse_panel_session(&request)?;
    panel.sessions.hide(&session, &panel.window, after_copy.unwrap_or(false))
}
#[derive(serde::Serialize)]
pub struct PanelCopyResult {session_id:String,result:CopyClipTextResult}
#[tauri::command]
pub async fn copy_manual_panel_text(window:tauri::WebviewWindow,panel:tauri::State<'_,Arc<crate::manual_panel::PanelHost>>,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<PanelCopyResult>{
    authorize_panel(&window)?;let(session,request)=parse_panel_copy(&request)?;let panel=panel.inner().clone();let runtime=runtime.inner().clone();let original=session.clone();let uncertain=CopyClipTextResult::new(&request,CopyOutcome::Unknown,Some(ErrorCode::CleanupFailed));
    let result=tauri::async_runtime::spawn_blocking(move||runtime.copy_panel_text(&panel.sessions,&session,request)).await.unwrap_or(Ok(uncertain))?;Ok(PanelCopyResult{session_id:original,result})
}

#[tauri::command]
pub async fn copy_manual_panel_png(
    window: tauri::WebviewWindow,
    panel: tauri::State<'_, Arc<crate::manual_panel::PanelHost>>,
    runtime: tauri::State<'_, Arc<HostRuntime>>,
    request: serde_json::Value,
) -> ContractResult<PanelCopyResult> {
    authorize_panel(&window)?;
    let (session, request) = parse_panel_copy(&request)?;
    let panel = panel.inner().clone();
    let runtime = runtime.inner().clone();
    let session_id = session.clone();
    let uncertain = CopyClipTextResult::new(&request, CopyOutcome::Unknown, Some(ErrorCode::CleanupFailed));
    let result = tauri::async_runtime::spawn_blocking(move || {
        runtime.copy_panel(&panel.sessions, &session, request, crate::clipboard::origin::WriteFormat::Png)
    }).await.unwrap_or(Ok(uncertain))?;
    Ok(PanelCopyResult { session_id, result })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature="test-fixtures")]
    #[test]
    fn startup_capture_rejects_unauthorized_windows_without_side_effects() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let events = Arc::new(AtomicUsize::new(0));
        let tracked = events.clone();
        let runtime = HostRuntime::for_test_memory(Arc::new(move |name, _| {
            if name == crate::startup_capture::CHANGED_EVENT { tracked.fetch_add(1, Ordering::SeqCst); }
        })).unwrap();
        let before = serde_json::to_value(runtime.startup_capture()).unwrap();
        let payload = serde_json::json!({"enabled": true});
        for (label, url) in [
            ("quick-panel", "http://tauri.localhost/"),
            ("main", "https://example.invalid/"),
            ("other", "http://tauri.localhost/"),
        ] {
            let result = startup_capture_request(label, &tauri::Url::parse(url).unwrap(), &payload)
                .and_then(|enabled| runtime.set_startup_capture(enabled));
            assert_eq!(result.unwrap_err().code, ErrorCode::PermissionUnknown);
        }
        let local = tauri::Url::parse("http://tauri.localhost/").unwrap();
        for invalid in [serde_json::json!({"enabled": true, "extra": false}), serde_json::json!({"enabled": "true"})] {
            assert!(startup_capture_request("main", &local, &invalid).is_err());
        }
        assert_eq!(serde_json::to_value(runtime.startup_capture()).unwrap(), before);
        assert_eq!(events.load(Ordering::SeqCst), 0);
        let enabled = startup_capture_request("main", &local, &payload).unwrap();
        assert!(runtime.set_startup_capture(enabled).unwrap().enabled);
        assert_eq!(events.load(Ordering::SeqCst), 1);
        runtime.shutdown(std::time::Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn png_preview_requires_the_matching_window_request_shape() {
        let local = tauri::Url::parse("http://tauri.localhost/").unwrap();
        let remote = tauri::Url::parse("https://example.invalid/").unwrap();
        let request = serde_json::json!({"request_id":"png-1","clip_id":"2"});
        let envelope = serde_json::json!({"session_id":"a".repeat(32),"request":request});
        assert!(png_preview_request("main", &local, &request).unwrap().0.is_none());
        assert_eq!(png_preview_request("quick-panel", &local, &envelope).unwrap().0, Some("a".repeat(32)));
        assert!(png_preview_request("main", &local, &envelope).is_err());
        assert!(png_preview_request("quick-panel", &local, &request).is_err());
        assert_eq!(png_preview_request("main", &remote, &request).unwrap_err().code, ErrorCode::PermissionUnknown);
    }
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

#[tauri::command]
pub async fn manage_history(window:tauri::WebviewWindow,runtime:tauri::State<'_,Arc<HostRuntime>>,request:serde_json::Value)->ContractResult<crate::host::HistoryManagementResult>{
    authorize_main(&window)?;
    let request:crate::host::HistoryManagement=serde_json::from_value(request).map_err(|_|ContractError::invalid_input("invalid management request"))?;
    let runtime=runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move||runtime.manage_history(request)).await.map_err(|_|join_failed())?
}
