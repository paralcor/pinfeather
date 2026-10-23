pub mod clipboard;
pub mod config;
pub mod contracts;
pub mod platform;

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

#[derive(serde::Serialize)]
struct ShellStatus {
    data_dir: String,
    start_paused: bool,
}

struct ShellState {
    data_dir: String,
    start_paused: bool,
}

#[tauri::command]
fn get_shell_status(state: tauri::State<'_, ShellState>) -> ShellStatus {
    ShellStatus {
        data_dir: state.data_dir.clone(),
        start_paused: state.start_paused,
    }
}

pub fn run(args: HostArgs) {
    let prepared = match config::prepare_data_dir(&args.data_dir) {
        Ok(prepared) => prepared,
        Err(message) => {
            eprintln!("pinfeather: {message}");
            std::process::exit(1);
        }
    };
    let state = ShellState {
        data_dir: prepared.data_dir.display().to_string(),
        start_paused: args.start_paused,
    };
    let webview_dir = prepared.webview_dir;

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![get_shell_status])
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("片羽")
            .inner_size(720.0, 520.0)
            .data_directory(webview_dir)
            .on_navigation(|url| navigation_allowed(url, cfg!(debug_assertions)))
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("pinfeather: Tauri 运行失败");
}
