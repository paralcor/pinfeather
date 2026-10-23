#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match pinfeather_lib::config::parse_args(std::env::args().skip(1)) {
        Ok(pinfeather_lib::config::LaunchMode::Worker) => {
            if let Err(message) = pinfeather_lib::clipboard::worker_entry::run() {
                eprintln!("pinfeather: {message}");
                std::process::exit(2);
            }
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
