pub mod clipboard;
pub mod config;
pub mod contracts;
pub mod platform;
pub mod store;
pub mod runtime;
pub mod host;
pub mod commands;

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
        .invoke_handler(tauri::generate_handler![commands::get_status,commands::list_clips,commands::get_clip_text,commands::set_capture_enabled])
        .setup(move |app| {
            let handle=app.handle().clone();
            let runtime=std::sync::Arc::new(host::HostRuntime::start(data_dir.clone(),std::sync::Arc::new(move|name,revision|{let _=handle.emit(name,serde_json::json!({"revision":revision.to_string()}));})).map_err(|e|std::io::Error::other(e.message))?);
            // Collection always requires an explicit user action in this isolated run.
            // --start-paused remains accepted; no implicit remembered enable exists.
            app.manage(runtime);
            let window=tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("片羽")
            .inner_size(720.0, 520.0)
            .data_directory(webview_dir)
            .on_navigation(|url| navigation_allowed(url, cfg!(debug_assertions)))
            .build()?;
            let owner=app.handle().clone();
            window.on_window_event(move|event|{
                if let tauri::WindowEvent::CloseRequested{api,..}=event {
                    let runtime=owner.state::<std::sync::Arc<host::HostRuntime>>().inner().clone();
                    if runtime.exit_ready.load(std::sync::atomic::Ordering::Acquire){return;}
                    api.prevent_close();
                    if runtime.exit_pending.swap(true,std::sync::atomic::Ordering::AcqRel){return;}
                    let handle=owner.clone();
                    tauri::async_runtime::spawn(async move{
                        let closing=runtime.clone();
                        let result=tauri::async_runtime::spawn_blocking(move||closing.shutdown(std::time::Duration::from_secs(3))).await;
                        if matches!(result,Ok(Ok(()))){handle.exit(0);}else{runtime.exit_pending.store(false,std::sync::atomic::Ordering::Release);runtime.exit_failed();}
                    });
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("pinfeather: Tauri 运行失败");
}
