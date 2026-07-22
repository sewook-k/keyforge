use crate::{Settings, ValidationError, validate};
use chrono::Utc;
use parking_lot::Mutex;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("stale revision: expected {expected}, current {current}")]
    StaleRevision { expected: u64, current: u64 },
    #[error("saved settings verification failed")]
    VerificationFailed,
    #[error("no settings backup exists")]
    NoBackup,
}

pub struct SettingsRepository {
    path: PathBuf,
    writer: Mutex<()>,
}

impl SettingsRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            writer: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn backup_path(&self) -> PathBuf {
        self.path.with_extension("json.bak")
    }

    pub fn load_or_default(&self) -> Result<Settings, RepositoryError> {
        let _guard = self.writer.lock();
        if !self.path.exists() {
            return Ok(Settings::default());
        }
        match read_settings(&self.path) {
            Ok(settings) => Ok(settings),
            Err(primary_error) if self.backup_path().exists() => {
                let backup = self.backup_path();
                let settings = read_settings(&backup).map_err(|_| primary_error)?;
                self.repair_primary_from_backup(&backup)?;
                Ok(settings)
            }
            Err(error) => Err(error),
        }
    }

    pub fn save(
        &self,
        mut draft: Settings,
        expected_revision: u64,
    ) -> Result<Settings, RepositoryError> {
        let _guard = self.writer.lock();
        validate(&draft)?;

        let current = if self.path.exists() {
            read_settings(&self.path)?.revision
        } else {
            0
        };
        if current != expected_revision {
            return Err(RepositoryError::StaleRevision {
                expected: expected_revision,
                current,
            });
        }
        draft.revision = current + 1;
        draft.updated_at = Utc::now();
        validate(&draft)?;

        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temp_path = parent.join(format!(".settings-{}.tmp", Uuid::new_v4()));
        let bytes = serde_json::to_vec_pretty(&draft)?;

        let result = (|| {
            let mut temp = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            temp.write_all(&bytes)?;
            temp.write_all(b"\n")?;
            temp.sync_all()?;
            drop(temp);

            if self.path.exists() {
                atomic_copy(&self.path, &self.backup_path())?;
            }
            atomic_replace(&temp_path, &self.path)?;
            match read_settings(&self.path) {
                Ok(verified) if verified == draft => Ok(verified),
                _ => {
                    if self.backup_path().exists() {
                        atomic_copy(&self.backup_path(), &self.path)?;
                        let _ = read_settings(&self.path)?;
                    } else {
                        let _ = fs::remove_file(&self.path);
                    }
                    Err(RepositoryError::VerificationFailed)
                }
            }
        })();

        if temp_path.exists() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    pub fn create_backup(&self) -> Result<PathBuf, RepositoryError> {
        let _guard = self.writer.lock();
        let settings = read_settings(&self.path)?;
        validate(&settings)?;
        let backup = self.backup_path();
        atomic_copy(&self.path, &backup)?;
        Ok(backup)
    }

    pub fn load_backup(&self) -> Result<Settings, RepositoryError> {
        let _guard = self.writer.lock();
        let backup = self.backup_path();
        if !backup.exists() {
            return Err(RepositoryError::NoBackup);
        }
        read_settings(&backup)
    }

    pub fn restore_backup(&self, expected_revision: u64) -> Result<Settings, RepositoryError> {
        let mut settings = self.load_backup()?;
        settings.revision = expected_revision;
        self.save(settings, expected_revision)
    }

    fn repair_primary_from_backup(&self, backup: &Path) -> Result<(), RepositoryError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let corrupt = self.path.with_extension("json.corrupt");
        let _ = fs::copy(&self.path, corrupt);
        let temp_path = parent.join(format!(".recovery-{}.tmp", Uuid::new_v4()));
        fs::copy(backup, &temp_path)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temp_path)?
            .sync_all()?;
        let result = atomic_replace(&temp_path, &self.path).map_err(RepositoryError::Io);
        if temp_path.exists() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
}

fn read_settings(path: &Path) -> Result<Settings, RepositoryError> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    let mut value: serde_json::Value = serde_json::from_slice(&data)?;
    migrate_settings_value(&mut value);
    let settings: Settings = serde_json::from_value(value)?;
    validate(&settings)?;
    Ok(settings)
}

fn migrate_settings_value(value: &mut serde_json::Value) {
    const REMOVED_ACTIONS: &[&str] = &[
        "macro",
        "send_text",
        "auto_click",
        "launch",
        "clipboard_slot",
        "window",
        "switch_profile",
    ];
    let schema_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let Some(profiles) = value
        .get_mut("profiles")
        .and_then(|value| value.as_array_mut())
    else {
        if schema_version < crate::CURRENT_SCHEMA_VERSION as u64 {
            value["schemaVersion"] = crate::CURRENT_SCHEMA_VERSION.into();
        }
        return;
    };
    for profile in profiles {
        if let Some(rules) = profile
            .get_mut("rules")
            .and_then(|value| value.as_array_mut())
        {
            rules.retain(|rule| {
                rule.pointer("/action/kind")
                    .and_then(|kind| kind.as_str())
                    .is_none_or(|kind| !REMOVED_ACTIONS.contains(&kind))
            });
        }
        if schema_version < crate::CURRENT_SCHEMA_VERSION as u64 {
            profile["scope"] = serde_json::json!({ "kind": "global" });
        }
    }
    if schema_version < crate::CURRENT_SCHEMA_VERSION as u64 {
        value["schemaVersion"] = crate::CURRENT_SCHEMA_VERSION.into();
    }
}

fn atomic_copy(source: &Path, target: &Path) -> std::io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".copy-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        fs::copy(source, &temp)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temp)?
            .sync_all()?;
        atomic_replace(&temp, target)
    })();
    if temp.exists() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    fn wide(path: &Path) -> Vec<u16> {
        OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    let source = wide(source);
    let target = wide(target);
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConditionGroup, ConditionOperator, MatchCondition, Profile, ProfileScope, TextOperator,
    };
    use tempfile::tempdir;

    #[test]
    fn save_increments_revision_and_preserves_backup() {
        let dir = tempdir().unwrap();
        let repo = SettingsRepository::new(dir.path().join("settings.json"));
        let first = repo.save(Settings::default(), 0).unwrap();
        assert_eq!(first.revision, 1);
        let mut second_draft = first.clone();
        second_draft.profiles[0].name = "Changed".into();
        let second = repo.save(second_draft, 1).unwrap();
        assert_eq!(second.revision, 2);
        let backup = read_settings(&repo.backup_path()).unwrap();
        assert_eq!(backup.revision, 1);
    }

    #[test]
    fn removed_action_rules_are_migrated_without_losing_supported_rules() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let repo = SettingsRepository::new(&path);
        let mut settings = Settings::default();
        for (input, output) in [("A", "B"), ("C", "D")] {
            settings.profiles[0]
                .rules
                .push(crate::Rule::key_remap(input, output));
        }
        let mut value = serde_json::to_value(settings).unwrap();
        let template = value["profiles"][0]["rules"][0].clone();
        let removed = [
            "macro",
            "send_text",
            "auto_click",
            "launch",
            "clipboard_slot",
            "window",
            "switch_profile",
        ];
        let rules = value["profiles"][0]["rules"].as_array_mut().unwrap();
        rules.remove(0);
        for kind in removed {
            let mut rule = template.clone();
            rule["id"] = serde_json::json!(Uuid::new_v4());
            rule["action"] = serde_json::json!({ "kind": kind });
            rules.push(rule);
        }
        value["schemaVersion"] = 2.into();
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = repo.load_or_default().unwrap();
        assert_eq!(loaded.schema_version, crate::CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.profiles[0].rules.len(), 1);
        assert!(matches!(
            &loaded.profiles[0].rules[0].action,
            crate::Action::SendKeys { chord } if chord == &vec!["D"]
        ));
    }

    #[test]
    fn version_one_conditional_scope_migrates_to_global() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let repo = SettingsRepository::new(&path);
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = 1.into();
        value["profiles"][0]["scope"] = serde_json::json!({
            "kind": "device",
            "conditions": {
                "operator": "and",
                "conditions": [{
                    "kind": "device_id",
                    "operator": "contains",
                    "value": "VID_3434&PID_01A0"
                }]
            }
        });
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = repo.load_or_default().unwrap();
        assert_eq!(loaded.schema_version, crate::CURRENT_SCHEMA_VERSION);
        assert!(matches!(loaded.profiles[0].scope, ProfileScope::Global));
    }

    #[test]
    fn rejects_stale_writer() {
        let dir = tempdir().unwrap();
        let repo = SettingsRepository::new(dir.path().join("settings.json"));
        repo.save(Settings::default(), 0).unwrap();
        let error = repo.save(Settings::default(), 0).unwrap_err();
        assert!(matches!(error, RepositoryError::StaleRevision { .. }));
    }

    #[test]
    fn corrupt_primary_recovers_from_backup() {
        let dir = tempdir().unwrap();
        let repo = SettingsRepository::new(dir.path().join("settings.json"));
        let first = repo.save(Settings::default(), 0).unwrap();
        repo.create_backup().unwrap();
        fs::write(repo.path(), b"{not json").unwrap();
        assert_eq!(repo.load_or_default().unwrap(), first);
        assert_eq!(read_settings(repo.path()).unwrap(), first);
        assert!(repo.path().with_extension("json.corrupt").exists());
    }

    #[test]
    fn default_profile_is_global() {
        assert!(matches!(
            Settings::default().profiles[0].scope,
            ProfileScope::Global
        ));
    }

    #[test]
    fn legacy_preferences_default_launch_at_login_to_false() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let repo = SettingsRepository::new(&path);
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["preferences"]
            .as_object_mut()
            .unwrap()
            .remove("launchAtLogin");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = repo.load_or_default().unwrap();
        assert!(!loaded.preferences.launch_at_login);
    }

    #[test]
    fn empty_condition_is_rejected() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("Scoped");
        profile.scope = ProfileScope::Application {
            conditions: ConditionGroup {
                operator: ConditionOperator::And,
                conditions: vec![MatchCondition::ProcessName {
                    operator: TextOperator::Equals,
                    value: String::new(),
                }],
            },
        };
        settings.profiles.push(profile);
        assert!(validate(&settings).is_err());
    }
}
