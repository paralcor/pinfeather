#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match pinfeather_lib::config::parse_args(std::env::args().skip(1)) {
        Ok(pinfeather_lib::config::LaunchMode::Worker) => {
            eprintln!(
                "pinfeather: 内部抓取 worker 模式（--capture-worker-v1）尚未实现；\
                 本进程不会启动 WebView，也不会输出快照"
            );
            std::process::exit(2);
        }
        Ok(pinfeather_lib::config::LaunchMode::Host(args)) => {
            pinfeather_lib::run(args);
        }
        Err(message) => {
            eprintln!("pinfeather: {message}");
            std::process::exit(1);
        }
    }
}
