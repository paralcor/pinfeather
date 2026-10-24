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
                data_dir = Some(normalize_data_dir(value)?);
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

/// 按状态契约的 data_dir 规则规范化；末尾分隔符先去掉（盘根除外），其余不合规一律拒绝。
fn normalize_data_dir(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim_end_matches(['\\', '/']);
    let is_drive = trimmed.len() == 2 && trimmed.ends_with(':');
    let candidate = if trimmed.is_empty() || is_drive {
        value
    } else {
        trimmed
    };
    crate::contracts::normalize_windows_dir_path(candidate)
        .map(PathBuf::from)
        .map_err(|e| {
            format!(
                "--data-dir 必须是规范的 Windows 完整目录路径（{}），收到：{value}",
                e.message
            )
        })
}

pub struct PreparedDataDir {
    pub data_dir: PathBuf,
    pub webview_dir: PathBuf,
    pub history_path: PathBuf,
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
        history_path: data_dir.join("history.sqlite3"),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_args, LaunchMode};
    use std::path::PathBuf;

    fn data_dir(value: &str) -> Result<PathBuf, String> {
        match parse_args(["--data-dir", value].map(String::from))? {
            LaunchMode::Host(args) => Ok(args.data_dir),
            LaunchMode::Worker => Err(String::from("unexpected worker mode")),
        }
    }

    #[test]
    fn data_dir_is_stored_in_contract_canonical_form() {
        for (input, expected) in [
            (r"D:\pf\run1\", r"D:\pf\run1"),
            ("d:/pf/run1/", r"D:\pf\run1"),
            (r"D:\pf\old\..\run1", r"D:\pf\run1"),
            (r"D:\", r"D:\"),
            (r"\\server\share\", r"\\server\share\"),
            (r"\\server\share\pf", r"\\server\share\pf"),
        ] {
            // 按字符串比较：PathBuf 相等忽略末尾分隔符，而状态契约不忽略。
            assert_eq!(
                data_dir(input).unwrap().to_str(),
                Some(expected),
                "input {input}"
            );
        }
    }

    #[test]
    fn data_dir_rejects_paths_the_status_contract_rejects() {
        for input in [
            r"\\?\D:\pf",
            r"\\.\pipe\pf",
            "C:pf",
            r"\pf",
            r"pf\run1",
            r"D:\..\pf",
            r"D:\pf\a*b",
        ] {
            assert!(data_dir(input).is_err(), "must reject {input}");
        }
    }
}
