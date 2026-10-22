//! 启动参数与隔离数据目录。缺少 `--data-dir` 时明确报错，无生产目录回落。

use std::path::{Path, PathBuf};

pub const WORKER_MODE_FLAG: &str = "--capture-worker-v1";

pub enum LaunchMode {
    Host(HostArgs),
    Worker,
}

pub struct HostArgs {
    pub data_dir: PathBuf,
    pub start_paused: bool,
}

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<LaunchMode, String> {
    let args: Vec<String> = args.into_iter().collect();
    if args.iter().any(|arg| arg == WORKER_MODE_FLAG) {
        return Ok(LaunchMode::Worker);
    }

    let mut data_dir: Option<PathBuf> = None;
    let mut start_paused = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--data-dir" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| String::from("--data-dir 缺少路径参数"))?;
                if value.is_empty() {
                    return Err(String::from("--data-dir 路径参数为空"));
                }
                if !Path::new(value).is_absolute() {
                    return Err(format!("--data-dir 必须是绝对路径，收到：{value}"));
                }
                data_dir = Some(PathBuf::from(value));
                index += 2;
            }
            "--start-paused" => {
                start_paused = true;
                index += 1;
            }
            other => return Err(format!("未知参数：{other}")),
        }
    }

    let data_dir =
        data_dir.ok_or_else(|| String::from("缺少 --data-dir <绝对路径>；本程序不使用默认生产数据目录"))?;
    Ok(LaunchMode::Host(HostArgs {
        data_dir,
        start_paused,
    }))
}

pub struct PreparedDataDir {
    pub data_dir: PathBuf,
    pub webview_dir: PathBuf,
}

pub fn prepare_data_dir(data_dir: &Path) -> Result<PreparedDataDir, String> {
    if !data_dir.is_absolute() {
        return Err(format!("数据目录不是绝对路径：{}", data_dir.display()));
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("无法创建数据目录 {}：{e}", data_dir.display()))?;
    if !data_dir.is_dir() {
        return Err(format!("数据目录不是目录：{}", data_dir.display()));
    }
    let webview_dir = data_dir.join("webview");
    std::fs::create_dir_all(&webview_dir)
        .map_err(|e| format!("无法创建 webview 数据目录 {}：{e}", webview_dir.display()))?;
    Ok(PreparedDataDir {
        data_dir: data_dir.to_path_buf(),
        webview_dir,
    })
}
