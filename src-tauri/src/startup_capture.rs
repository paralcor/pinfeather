use std::path::{Path, PathBuf};

use crate::config::{read_settings, replace_settings, stage_settings, ReplaceError};
use crate::contracts::{ContractError, ContractResult, ErrorCode};

pub(crate) const FILE_NAME: &str = "startup-capture.json";
pub(crate) const CHANGED_EVENT: &str = "startup_capture_changed";
const UNREADABLE: &str = "启动恢复配置不可用。原文件已保留，请退出后核对 startup-capture.json。";
const EXTERNAL_CHANGE: &str = "启动恢复配置已在外部更改或无法核对。原文件已保留，请退出后核对 startup-capture.json。";
const SAVE_FAILED: &str = "未能保存启动恢复设置。请核对保存状态，或用 --start-paused 强制暂停启动。";
const SAVE_UNKNOWN: &str = "无法确认启动恢复设置是否已保存。请退出后核对 startup-capture.json。";

#[derive(Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Preference { Disabled, Paused, Listening }

impl Preference {
    pub(crate) fn manual(enabled: bool) -> Self {
        if enabled { Self::Listening } else { Self::Paused }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    last_manual_state: Preference,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartupOutcome { DefaultPaused, ForcedPaused, RememberedPaused, Restored, Blocked }

#[derive(Debug, serde::Serialize)]
pub(crate) struct StartupCaptureSnapshot {
    pub(crate) revision: String,
    pub(crate) enabled: bool,
    pub(crate) blocked: bool,
    pub(crate) outcome: StartupOutcome,
    pub(crate) reason: Option<String>,
    pub(crate) error: Option<String>,
}

// Guarded by CaptureState so live changes and admission to saving are atomic.
#[derive(Default)]
pub(crate) struct StartupCaptureWrites {
    pub(crate) saving: usize,
    pub(crate) pending_start: bool,
    pub(crate) recovery_queued: bool,
    pub(crate) last_manual: Option<(u64, bool)>,
}

impl StartupCaptureWrites {
    pub(crate) fn record_manual_choice(&mut self, enabled: bool) {
        let generation = self.last_manual.map_or(1, |(previous, _)| previous + 1);
        self.last_manual = Some((generation, enabled));
        self.pending_start = false;
    }
}

#[derive(Clone)]
pub(crate) struct StartupCapture {
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
    preference: Preference,
    outcome: StartupOutcome,
    reason: Option<String>,
    error: Option<String>,
    blocked: bool,
    revision: u64,
}

impl StartupCapture {
    pub(crate) fn without_file(force_paused: bool) -> Self {
        Self {
            path: None, expected: None, preference: Preference::Disabled,
            outcome: if force_paused { StartupOutcome::ForcedPaused } else { StartupOutcome::DefaultPaused },
            reason: None, error: None, blocked: false, revision: 0,
        }
    }

    pub(crate) fn load(
        directory: Option<&Path>, force_paused: bool,
        read: impl FnOnce(&Path) -> std::io::Result<Option<Vec<u8>>>,
    ) -> Self {
        let Some(path) = directory.map(|dir| dir.join(FILE_NAME)) else { return Self::without_file(force_paused); };
        let loaded = read(&path);
        let preference = match &loaded {
            Ok(None) => Some(Preference::Disabled),
            Ok(Some(bytes)) => serde_json::from_slice::<Document>(bytes).ok()
                .filter(|doc| doc.schema_version == 1).map(|doc| doc.last_manual_state),
            Err(_) => None,
        };
        let outcome = match (force_paused, preference) {
            (true, _) => StartupOutcome::ForcedPaused,
            (false, None) => StartupOutcome::Blocked,
            (false, Some(Preference::Paused)) => StartupOutcome::RememberedPaused,
            _ => StartupOutcome::DefaultPaused,
        };
        Self {
            path: Some(path), expected: loaded.ok().flatten(), preference: preference.unwrap_or(Preference::Disabled),
            outcome, error: preference.is_none().then(|| UNREADABLE.into()), reason: None,
            blocked: preference.is_none(), revision: 0,
        }
    }

    pub(crate) fn read_timed_out(&mut self) {
        if self.outcome != StartupOutcome::ForcedPaused { self.outcome = StartupOutcome::Blocked; }
        self.reason = None;
        self.error = Some("启动时核对历史存储或启动恢复配置超时。片羽保持暂停，请重新启动。".into());
        self.blocked = true;
    }

    pub(crate) fn should_restore(&self) -> bool {
        self.preference == Preference::Listening && !self.blocked && self.outcome != StartupOutcome::ForcedPaused
    }

    pub(crate) fn restored(&mut self, reason: Option<String>) {
        self.outcome = if reason.is_some() { StartupOutcome::Blocked } else { StartupOutcome::Restored };
        self.reason = reason;
    }

    pub(crate) fn clear_restore_block(&mut self) {
        if !self.blocked && self.reason.take().is_some() {
            self.outcome = StartupOutcome::DefaultPaused;
            self.revision += 1;
        }
    }

    pub(crate) fn startup_file_current(&mut self, read: impl FnOnce(&Path) -> std::io::Result<Option<Vec<u8>>>) -> bool {
        if self.path.as_deref().is_none_or(|path| matches!(read(path), Ok(current) if current == self.expected)) {
            return true;
        }
        self.blocked = true;
        self.error = Some(EXTERNAL_CHANGE.into());
        false
    }

    pub(crate) fn remembers_manual_state(&self) -> bool {
        self.preference != Preference::Disabled && !self.blocked
    }

    pub(crate) fn revision(&self) -> u64 { self.revision }

    pub(crate) fn snapshot(&self) -> StartupCaptureSnapshot {
        StartupCaptureSnapshot {
            revision: self.revision.to_string(), enabled: self.preference != Preference::Disabled,
            blocked: self.blocked, outcome: self.outcome, reason: self.reason.clone(), error: self.error.clone(),
        }
    }

    pub(crate) fn save(&mut self, preference: Preference) -> ContractResult<()> {
        if self.blocked {
            return Err(ContractError::new(ErrorCode::StorageUnavailable, self.error.as_deref().unwrap_or(UNREADABLE)));
        }
        let result = self.save_file(preference);
        let (error, blocked) = match result {
            Ok(()) => {
                if self.preference != preference || self.error.take().is_some() {
                    self.revision += 1;
                }
                self.preference = preference;
                self.error = None;
                return Ok(());
            }
            Err(ReplaceError::Retained) => (SAVE_FAILED, false),
            Err(ReplaceError::Changed) => (EXTERNAL_CHANGE, true),
            Err(ReplaceError::Indeterminate) => (SAVE_UNKNOWN, true),
        };
        self.blocked = blocked;
        self.error = Some(error.into());
        self.revision += 1;
        Err(ContractError::new(ErrorCode::StorageUnavailable, error))
    }

    fn save_file(&mut self, preference: Preference) -> Result<(), ReplaceError> {
        let Some(path) = self.path.as_deref() else { return Ok(()); };
        if preference == self.preference {
            return if matches!(read_settings(path), Ok(current) if current == self.expected) {
                Ok(())
            } else {
                Err(ReplaceError::Changed)
            };
        }
        let bytes = serde_json::to_vec_pretty(&Document { schema_version: 1, last_manual_state: preference })
            .map_err(|_| ReplaceError::Retained)?;
        let staged = stage_settings(path, "startup-capture", bytes).map_err(|_| ReplaceError::Retained)?;
        replace_settings(path, &mut self.expected, &staged)
    }
}
