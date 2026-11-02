//! 启动参数与隔离数据目录。缺少 `--data-dir` 时明确报错，无生产目录回落。

use std::path::{Path, PathBuf};
use std::io::{Read, Write};
use crate::contracts::{ContractError, ContractResult, ErrorCode};

const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocument { schema_version: u32, excluded_apps: Vec<String> }

pub(crate) fn normalize_exclusions(paths: Vec<String>) -> ContractResult<Vec<String>> {
    let paths = crate::contracts::parse_set_excluded_apps_request(&serde_json::json!({"paths":paths}))?;
    let mut unique: Vec<String> = Vec::new();
    for path in paths {
        let mut duplicate=false;
        for old in &unique { if crate::contracts::compare_windows_paths(old, &path)? { duplicate=true;break; } }
        if !duplicate { unique.push(path); }
    }
    Ok(unique)
}

pub(crate) fn same_exclusions(left: &[String], right: &[String]) -> ContractResult<bool> {
    if left.len()!=right.len(){return Ok(false);}
    for path in left {
        let mut found=false;
        for other in right { if crate::contracts::compare_windows_paths(path,other)?{found=true;break;} }
        if !found{return Ok(false);}
    }
    Ok(true)
}

fn settings_error() -> ContractError { ContractError::new(ErrorCode::StorageUnavailable, "设置文件不可用；已保留文件。请退出后修正 settings.json，再重新启动。") }
fn read_settings(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new(); file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES { return Err(std::io::Error::other("settings file too large")); }
    Ok(Some(bytes))
}

/// Owns the persisted format and exact bytes expected before a replacement.
pub(crate) struct SettingsFile { path: PathBuf, expected: Option<Vec<u8>> }
pub(crate) struct SettingsWrite { path: PathBuf, bytes: Vec<u8> }
impl Drop for SettingsWrite {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.path); }
}
pub(crate) struct SettingsSaveError { pub error: ContractError, pub uncertain: bool }
impl SettingsFile {
    pub fn load(path: PathBuf) -> (Self, ContractResult<Vec<String>>) {
        let loaded = read_settings(&path);
        let result = match &loaded {
            Ok(None) => Ok(Vec::new()),
            Ok(Some(bytes)) => serde_json::from_slice::<SettingsDocument>(bytes).map_err(|_| settings_error()).and_then(|doc| {
                if doc.schema_version != 1 { return Err(settings_error()); }
                normalize_exclusions(doc.excluded_apps).map_err(|_| settings_error())
            }),
            Err(_) => Err(settings_error()),
        };
        (Self { path, expected: loaded.ok().flatten() }, result)
    }
    pub fn prepare(&self, paths: &[String]) -> ContractResult<SettingsWrite> {
        let bytes = serde_json::to_vec_pretty(&SettingsDocument { schema_version: 1, excluded_apps: paths.to_vec() }).map_err(|_| settings_error())?;
        let path = self.path.with_file_name(format!(".settings-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|_| settings_error())?;
        let staged = SettingsWrite { path, bytes };
        let written=file.write_all(&staged.bytes).and_then(|_| file.sync_all());drop(file);written.map_err(|_| settings_error())?;
        Ok(staged)
    }
    /// Caller holds the capture commit gate; never delete the existing destination.
    pub fn replace(&mut self, staged: &SettingsWrite) -> Result<(), SettingsSaveError> {
        let current = read_settings(&self.path).map_err(|_| SettingsSaveError { error: settings_error(), uncertain: true })?;
        if current != self.expected { return Err(SettingsSaveError { error: settings_error(), uncertain: true }); }
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, MoveFileExW, MOVEFILE_WRITE_THROUGH};
        let destination: Vec<u16> = self.path.as_os_str().encode_wide().chain([0]).collect();
        let source: Vec<u16> = staged.path.as_os_str().encode_wide().chain([0]).collect();
        let replaced = unsafe {
            if self.expected.is_some() { ReplaceFileW(destination.as_ptr(), source.as_ptr(), std::ptr::null(), 0, std::ptr::null(), std::ptr::null()) }
            else { MoveFileExW(source.as_ptr(), destination.as_ptr(), MOVEFILE_WRITE_THROUGH) }
        } != 0;
        let after = read_settings(&self.path);
        if replaced && matches!(&after, Ok(Some(bytes)) if *bytes == staged.bytes) {
            self.expected = Some(staged.bytes.clone()); return Ok(());
        }
        let retained = matches!(&after, Ok(bytes) if *bytes == self.expected);
        Err(SettingsSaveError { error: ContractError::new(ErrorCode::StorageUnavailable, if retained { "设置保存失败，磁盘与当前排除规则保持原样。可重试应用。" } else { "无法确认设置最终状态；收录已阻止。请退出后核对 settings.json，再重新启动。" }), uncertain: !retained })
    }
}

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

#[cfg(test)]
mod settings_tests {
    use super::*;
    fn directory() -> PathBuf {
        let root=PathBuf::from(std::env::var_os("PINFEATHER_TEST_DATA_ROOT").expect("isolated test root required"));assert!(root.is_absolute());
        let dir=root.join(format!("config-settings-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&dir).unwrap();dir
    }
    #[test]
    fn m2_settings_missing_and_strict_documents_preserve_bytes() {
        let dir=directory();let path=dir.join("settings.json");
        let(_,loaded)=SettingsFile::load(path.clone());assert!(loaded.unwrap().is_empty());assert!(!path.exists());
        for bytes in [b"{".as_slice(),br#"{"schema_version":2,"excluded_apps":[]}"#,br#"{"schema_version":1,"excluded_apps":[],"capture_enabled":true}"#,br#"{"schema_version":1,"excluded_apps":["vault.exe"]}"#,br#"{"excluded_apps":[]}"#,br#"{"schema_version":1,"schema_version":1,"excluded_apps":[]}"#] {
            std::fs::write(&path,bytes).unwrap();let(_,loaded)=SettingsFile::load(path.clone());assert!(loaded.is_err());assert_eq!(std::fs::read(&path).unwrap(),bytes);
        }
    }
    #[test]
    fn m2_settings_normalization_ordinal_duplicates_and_limits() {
        assert_eq!(normalize_exclusions(vec!["c:/Private/old/../Ä.exe".into(),r"C:\PRIVATE\ä.EXE".into()]).unwrap(),vec![r"C:\Private\Ä.exe"]);
        let paths:Vec<_>=(0..32).map(|i|format!(r"C:\Private\app{i}.exe")).collect();assert_eq!(normalize_exclusions(paths.clone()).unwrap().len(),32);
        let mut too_many=paths;too_many.push(r"C:\Private\extra.exe".into());assert!(normalize_exclusions(too_many).is_err());
        for path in [r"C:\..\vault.exe",r"C:\Private\*.exe",r"C:\Private\vault.dll","vault.exe",r"\\?\C:\Private\vault.exe"]{assert!(normalize_exclusions(vec![path.into()]).is_err());}
    }
    #[test]
    fn m2_settings_shared_requests_and_snapshot() {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tests/fixtures/contracts/settings.json")).unwrap();
        for case in fixture["requests"].as_array().unwrap(){let parsed=crate::contracts::parse_set_excluded_apps_request(&case["value"]);if case["expected"].is_null(){assert!(parsed.is_err());}else{assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(),case["expected"]);}}
        let wire=crate::contracts::SettingsSnapshot{revision:"7".into(),schema_version:1,excluded_apps:vec![r"C:\Private\vault.exe".into()],saving:false,blocked:false,error:None,last_skip:Some("unknown_owner".into())};
        assert_eq!(serde_json::to_value(wire).unwrap(),fixture["snapshot"]);
    }
    #[test]
    fn m2_settings_external_change_is_indeterminate_and_preserved() {
        let dir=directory();let path=dir.join("settings.json");let(mut file,loaded)=SettingsFile::load(path.clone());loaded.unwrap();
        let staged=file.prepare(&[r"C:\Private\vault.exe".into()]).unwrap();let external=br#"{"schema_version":1,"excluded_apps":[]}"#;std::fs::write(&path,external).unwrap();
        assert!(file.replace(&staged).unwrap_err().uncertain);assert_eq!(std::fs::read(path).unwrap(),external);drop(staged);
        assert!(!std::fs::read_dir(dir).unwrap().any(|entry|entry.unwrap().file_name().to_string_lossy().starts_with(".settings-")));
    }
}
