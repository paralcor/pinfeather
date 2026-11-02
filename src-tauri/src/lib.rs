pub mod clipboard;
pub mod config;
pub mod contracts;
pub mod platform;
pub mod store;
pub mod runtime;
pub mod host;
pub mod commands;
pub mod manual_panel;
pub mod residency;

use config::HostArgs;

/// 导航允许范围：本地应用协议与（仅调试构建）明确的 Vite 开发源，不开放任意远程源。
/// 应用源仅接受默认端口，防止同主机其它端口的本地服务被当作应用导航目标。
fn navigation_allowed(url: &tauri::Url, allow_dev_origin: bool) -> bool {
    let scheme = url.scheme();
    let host = url.host_str();
    match (scheme, host) {
        ("tauri", Some("localhost")) => url.port().is_none(),
        ("http" | "https", Some("tauri.localhost")) => url.port().is_none(),
        ("http", Some("127.0.0.1") | Some("localhost")) => {
            allow_dev_origin && url.port() == Some(1420)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::navigation_allowed;

    fn allowed(url: &str, dev: bool) -> bool {
        navigation_allowed(&tauri::Url::parse(url).unwrap(), dev)
    }

    #[test]
    fn allows_local_app_origins() {
        assert!(allowed("http://tauri.localhost/index.html", false));
        assert!(allowed("https://tauri.localhost/index.html", false));
        assert!(allowed("tauri://localhost/index.html", false));
    }

    #[test]
    fn allows_dev_origin_only_in_dev() {
        assert!(allowed("http://127.0.0.1:1420/index.html", true));
        assert!(allowed("http://localhost:1420/", true));
        assert!(!allowed("http://127.0.0.1:1420/index.html", false));
    }

    #[test]
    fn rejects_remote_and_unknown_targets() {
        assert!(!allowed("https://example.com/", false));
        assert!(!allowed("https://example.com/", true));
        assert!(!allowed("http://tauri.localhost.evil.com/", false));
        assert!(!allowed("https://tauri.localhost:8443/", false));
        assert!(!allowed("http://127.0.0.1:9999/", true));
        assert!(!allowed("http://127.0.0.1.evil.com:1420/", true));
        assert!(!allowed("file:///C:/Windows/System32/calc.exe", false));
        assert!(!allowed("javascript:alert(1)", false));
    }
}

pub fn run(args: HostArgs) {
    use tauri::{Manager,Emitter};
    let prepared = match config::prepare_data_dir(&args.data_dir) {
        Ok(prepared) => prepared,
        Err(message) => {
            eprintln!("pinfeather: {message}");
            std::process::exit(1);
        }
    };
    let data_dir=prepared.data_dir;
    let webview_dir = prepared.webview_dir;

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::get_status,commands::list_clips,commands::get_clip_text,commands::copy_clip_text,commands::set_capture_enabled,commands::get_manual_panel_state,commands::open_manual_panel,commands::ready_manual_panel,commands::hide_manual_panel,commands::copy_manual_panel_text,commands::get_residency_state,commands::request_exit,commands::retry_exit_saving,commands::get_settings_state,commands::apply_excluded_apps])
        .setup(move |app| {
            let handle=app.handle().clone();
            let runtime=std::sync::Arc::new(host::HostRuntime::start(data_dir.clone(),std::sync::Arc::new(move|name,revision|{
                let _=handle.emit(name,serde_json::json!({"revision":revision.to_string()}));
                let target=handle.clone();let _=handle.run_on_main_thread(move||{if let Some(owner)=target.try_state::<std::sync::Arc<residency::DesktopResidency>>(){owner.refresh_tray();}});
            })).map_err(|e|std::io::Error::other(e.message))?);
            // Collection always requires an explicit user action in this isolated run.
            // --start-paused remains accepted; no implicit remembered enable exists.
            app.manage(runtime.clone());
            let window=tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("片羽")
            .inner_size(720.0, 520.0)
            .data_directory(webview_dir.clone())
            .on_navigation(|url| navigation_allowed(url, cfg!(debug_assertions)))
            .build()?;
            let quick=tauri::WebviewWindowBuilder::new(app,"quick-panel",tauri::WebviewUrl::App("index.html?panel=1".into()))
                .title("片羽 · 快捷面板").inner_size(620.0,560.0).visible(false).data_directory(webview_dir)
                .on_navigation(|url|navigation_allowed(url,cfg!(debug_assertions))).build();
            let panel=std::sync::Arc::new(manual_panel::PanelHost::new(app.handle().clone(),quick.is_ok()));
            app.manage(panel.clone());
            if let Ok(quick)=quick {
                let p=panel.clone();
                quick.on_window_event(move|event|match event{
                    tauri::WindowEvent::CloseRequested{api,..}=>{api.prevent_close();p.sessions.dismiss(&p.window);},
                    tauri::WindowEvent::Focused(false)=>{if p.sessions.snapshot().phase=="ready"&&p.window.0.get_webview_window("quick-panel").and_then(|w|w.is_focused().ok())==Some(false){p.sessions.dismiss(&p.window);}},
                    tauri::WindowEvent::Destroyed=>{let _=p.stop();},
                    _=>{},
                });
                panel.start_hotkey();
            }
            let residency=residency::DesktopResidency::new(app.handle().clone(),runtime,panel);
            app.manage(residency.clone());residency.install_tray();
            window.on_window_event(move|event|{
                if let tauri::WindowEvent::CloseRequested{api,..}=event {
                    if residency.coordinator.exited(){return;}
                    api.prevent_close();
                    residency.coordinator.close_main(residency.effects.as_ref());
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("pinfeather: Tauri 运行失败")
        .run(|handle,event|if let tauri::RunEvent::ExitRequested{api,..}=event{
            if let Some(owner)=handle.try_state::<std::sync::Arc<residency::DesktopResidency>>(){
                if !owner.coordinator.exited(){api.prevent_exit();let _=owner.coordinator.request_exit(owner.effects.clone());}
            }
        });
}
