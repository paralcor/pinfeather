//! Offline history restore. Does not initialize Tauri, clipboard, or capture.
fn main() {
    let args:Vec<_>=std::env::args_os().skip(1).collect();
    if args.len()!=2 { eprintln!("Usage: history_restore <backup.sqlite3> <new-absolute-directory>");std::process::exit(2); }
    if pinfeather_lib::store::Store::restore_to_new_directory(std::path::Path::new(&args[0]),std::path::Path::new(&args[1])).is_err(){eprintln!("Restore failed; source preserved. Do not reuse a partially created target directory.");std::process::exit(1);}
    println!("History restored to the new directory. Product was not started.");
}
