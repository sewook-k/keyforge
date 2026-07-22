use directories::ProjectDirs;
use keyforge_config::{
    ActionResult, ActionStatus, Recovery, RecoveryAction, RepositoryError, Settings,
    SettingsRepository, validate,
};
use keyforge_engine::CompiledRules;
use keyforge_platform_windows::{
    DeviceInventoryError, HookService, LaunchAtLoginError, LaunchAtLoginRegistration,
    list_connected_keyboards, restore_launch_at_login, set_launch_at_login,
    snapshot_launch_at_login,
};
pub use keyforge_platform_windows::{
    KeyCaptureDrain, KeyCaptureSession, KeyboardDeviceInfo, force_end_active_capture,
    record_window_system_key,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::VecDeque, path::PathBuf, sync::Arc};
use thiserror::Error;

const MAX_ACTIVITY: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub engine_state: EngineState,
    pub active_profile_count: usize,
    pub hook_installed: bool,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    Running,
    Paused,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub settings: Settings,
    pub runtime: RuntimeState,
    pub activity: Vec<ActionResult>,
    pub settings_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResponse {
    pub settings: Settings,
    pub result: ActionResult,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("Windows startup registration could not be updated: {0}")]
    Startup(#[from] LaunchAtLoginError),
    #[error(
        "settings were not saved and the previous Windows startup registration could not be restored: {original}; rollback: {rollback}"
    )]
    StartupRollback {
        original: RepositoryError,
        rollback: LaunchAtLoginError,
    },
}

trait LaunchAtLoginRegistrar: Send + Sync {
    fn snapshot(&self) -> Result<LaunchAtLoginRegistration, LaunchAtLoginError>;
    fn set_enabled(&self, enabled: bool) -> Result<(), LaunchAtLoginError>;
    fn restore(&self, registration: &LaunchAtLoginRegistration) -> Result<(), LaunchAtLoginError>;
}

struct WindowsLaunchAtLoginRegistrar;

impl LaunchAtLoginRegistrar for WindowsLaunchAtLoginRegistrar {
    fn snapshot(&self) -> Result<LaunchAtLoginRegistration, LaunchAtLoginError> {
        snapshot_launch_at_login()
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), LaunchAtLoginError> {
        set_launch_at_login(enabled)
    }

    fn restore(&self, registration: &LaunchAtLoginRegistration) -> Result<(), LaunchAtLoginError> {
        restore_launch_at_login(registration)
    }
}

pub struct AppService {
    repository: Arc<SettingsRepository>,
    settings: RwLock<Settings>,
    hook: Mutex<Option<HookService>>,
    hook_error: RwLock<Option<String>>,
    activity: Mutex<VecDeque<ActionResult>>,
    operation: Mutex<()>,
    launch_at_login: Arc<dyn LaunchAtLoginRegistrar>,
}

impl AppService {
    pub fn new_default() -> anyhow::Result<Self> {
        let project = ProjectDirs::from("com", "KeyForge", "KeyForge")
            .ok_or_else(|| anyhow::anyhow!("unable to resolve LOCALAPPDATA"))?;
        let path = project.data_local_dir().join("settings.json");
        Self::new(path, true)
    }

    pub fn new(path: PathBuf, enable_hooks: bool) -> anyhow::Result<Self> {
        Self::new_with_launch_at_login_registrar(
            path,
            enable_hooks,
            Arc::new(WindowsLaunchAtLoginRegistrar),
        )
    }

    fn new_with_launch_at_login_registrar(
        path: PathBuf,
        enable_hooks: bool,
        launch_at_login: Arc<dyn LaunchAtLoginRegistrar>,
    ) -> anyhow::Result<Self> {
        let repository = Arc::new(SettingsRepository::new(path));
        let mut settings = repository.load_or_default()?;
        if !repository.path().exists() {
            settings = repository.save(settings, 0)?;
        }
        let compiled = CompiledRules::compile(&settings)?;
        let (hook, hook_error) = if enable_hooks {
            match HookService::start(compiled) {
                Ok(hook) => (Some(hook), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, Some("input hooks disabled for this service".into()))
        };
        Ok(Self {
            repository,
            settings: RwLock::new(settings),
            hook: Mutex::new(hook),
            hook_error: RwLock::new(hook_error),
            activity: Mutex::new(VecDeque::new()),
            operation: Mutex::new(()),
            launch_at_login,
        })
    }

    pub fn bootstrap(&self) -> Bootstrap {
        Bootstrap {
            settings: self.settings.read().clone(),
            runtime: self.runtime_state(),
            activity: self.activity.lock().iter().cloned().collect(),
            settings_path: self.repository.path().display().to_string(),
        }
    }

    pub fn runtime_state(&self) -> RuntimeState {
        let settings = self.settings.read();
        let hook = self.hook.lock();
        let installed = hook.as_ref().is_some_and(HookService::is_installed);
        let paused = hook.as_ref().is_some_and(HookService::is_paused);
        RuntimeState {
            engine_state: if !installed {
                EngineState::Error
            } else if paused {
                EngineState::Paused
            } else {
                EngineState::Running
            },
            active_profile_count: settings
                .profiles
                .iter()
                .filter(|profile| profile.enabled && !profile.archived)
                .count(),
            hook_installed: installed,
            capabilities: vec![
                "keyboard_remap".into(),
                "mouse_remap".into(),
                "atomic_settings".into(),
                "backup_restore".into(),
                "activity_feed".into(),
                "device_inventory".into(),
            ],
        }
    }

    pub fn connected_keyboards(&self) -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
        list_connected_keyboards()
    }

    pub fn set_key_capture_window(&self, hwnd: isize) {
        if let Some(hook) = self.hook.lock().as_ref() {
            hook.set_key_capture_window(hwnd);
        }
    }

    pub fn begin_key_capture(&self) -> Result<KeyCaptureSession, String> {
        if let Some(hook) = self.hook.lock().as_ref().filter(|hook| hook.is_installed()) {
            return hook.begin_key_capture().map_err(|error| error.to_string());
        }
        Err(self
            .hook_error
            .read()
            .clone()
            .unwrap_or_else(|| "Windows input hook is unavailable".into()))
    }

    pub fn end_key_capture(&self, session_id: u64) -> bool {
        self.hook
            .lock()
            .as_ref()
            .is_some_and(|hook| hook.end_key_capture(session_id))
    }

    pub fn force_end_key_capture(&self) {
        if let Some(hook) = self.hook.lock().as_ref() {
            hook.force_end_key_capture();
        }
    }

    pub fn drain_key_capture_events(&self, session_id: u64) -> KeyCaptureDrain {
        self.hook
            .lock()
            .as_ref()
            .map(|hook| hook.drain_key_capture_events(session_id))
            .unwrap_or(KeyCaptureDrain {
                session_id,
                active: false,
                overflowed: false,
                events: Vec::new(),
            })
    }

    pub fn close_to_tray(&self) -> bool {
        self.settings.read().preferences.close_to_tray
    }

    pub fn start_minimized(&self) -> bool {
        self.settings.read().preferences.start_minimized
    }

    pub fn launch_at_login_enabled(&self) -> bool {
        self.settings.read().preferences.launch_at_login
    }

    pub fn shutdown(&self) {
        if let Some(mut hook) = self.hook.lock().take() {
            hook.stop();
        }
    }

    pub fn save_and_apply(
        &self,
        draft: Settings,
        expected_revision: u64,
    ) -> Result<SaveResponse, ServiceError> {
        let _operation = self.operation.lock();
        validate(&draft).map_err(RepositoryError::from)?;

        let current = self.settings.read().clone();
        if current.revision != expected_revision {
            return Err(RepositoryError::StaleRevision {
                expected: expected_revision,
                current: current.revision,
            }
            .into());
        }

        let startup_changed =
            draft.preferences.launch_at_login != current.preferences.launch_at_login;
        let startup_snapshot = if startup_changed {
            Some(self.launch_at_login.snapshot()?)
        } else {
            None
        };
        if startup_changed {
            self.launch_at_login
                .set_enabled(draft.preferences.launch_at_login)?;
        }

        let mut response = match self.save_and_apply_locked(draft, expected_revision) {
            Ok(response) => response,
            Err(original) => {
                if let Some(snapshot) = startup_snapshot
                    && let Err(rollback) = self.launch_at_login.restore(&snapshot)
                {
                    return Err(ServiceError::StartupRollback { original, rollback });
                }
                return Err(original.into());
            }
        };

        if startup_changed {
            response.result.action_type = "set_launch_at_login".into();
            response.result.message = if response.settings.preferences.launch_at_login {
                "Windows 시작 프로그램에 KeyForge를 등록하고 설정을 적용했습니다."
            } else {
                "Windows 시작 프로그램에서 KeyForge 등록을 해제하고 설정을 적용했습니다."
            }
            .into();
            response.result.details = Some(json!({
                "stage": "verified",
                "settingsPath": self.repository.path(),
                "launchAtLogin": response.settings.preferences.launch_at_login,
                "startupRegistrationChanged": true,
            }));
            self.replace_activity(response.result.clone());
        }

        Ok(response)
    }

    fn save_and_apply_locked(
        &self,
        draft: Settings,
        expected_revision: u64,
    ) -> Result<SaveResponse, RepositoryError> {
        let compiled = CompiledRules::compile(&draft).map_err(|error| {
            RepositoryError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                error.to_string(),
            ))
        })?;
        let saved = self.repository.save(draft, expected_revision)?;
        let mut compiled = compiled;
        if compiled.revision() != saved.revision {
            compiled = CompiledRules::compile(&saved).map_err(|error| {
                RepositoryError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    error.to_string(),
                ))
            })?;
        }

        let mut result =
            if let Some(hook) = self.hook.lock().as_ref().filter(|hook| hook.is_installed()) {
                hook.update_rules(compiled);
                ActionResult::success(
                    "save_settings",
                    "프로필을 저장하고 적용했습니다",
                    Some(saved.revision),
                )
            } else {
                ActionResult {
                    action_id: uuid::Uuid::new_v4(),
                    action_type: "save_settings".into(),
                    status: ActionStatus::Warning,
                    message: "설정은 저장했지만 입력 엔진에 적용하지 못했습니다".into(),
                    revision: Some(saved.revision),
                    timestamp: chrono::Utc::now(),
                    recovery: Some(Recovery {
                        attempted: false,
                        succeeded: None,
                        message: self.hook_error.read().clone(),
                        actions: vec![RecoveryAction::Retry, RecoveryAction::OpenLogs],
                    }),
                    details: None,
                }
            };
        result.details = Some(json!({"stage": "verified", "settingsPath": self.repository.path()}));
        *self.settings.write() = saved.clone();
        self.push_activity(result.clone());
        Ok(SaveResponse {
            settings: saved,
            result,
        })
    }

    pub fn set_engine_paused(&self, paused: bool) -> ActionResult {
        let result = if let Some(hook) = self.hook.lock().as_ref() {
            hook.set_paused(paused);
            ActionResult::success(
                if paused {
                    "pause_engine"
                } else {
                    "resume_engine"
                },
                if paused {
                    "모든 입력 규칙을 일시정지했습니다"
                } else {
                    "입력 엔진을 다시 시작했습니다"
                },
                Some(self.settings.read().revision),
            )
        } else {
            ActionResult::error(
                "pause_engine",
                "입력 엔진을 사용할 수 없습니다",
                Recovery {
                    attempted: false,
                    succeeded: None,
                    message: self.hook_error.read().clone(),
                    actions: vec![RecoveryAction::OpenLogs],
                },
            )
        };
        self.push_activity(result.clone());
        result
    }

    pub fn create_backup(&self) -> ActionResult {
        let _operation = self.operation.lock();
        let result = match self.repository.create_backup() {
            Ok(path) => {
                let mut result = ActionResult::success(
                    "create_backup",
                    "설정 백업을 만들었습니다",
                    Some(self.settings.read().revision),
                );
                result.details = Some(json!({"path": path}));
                result
            }
            Err(error) => ActionResult::error(
                "create_backup",
                "설정 백업을 만들지 못했습니다",
                Recovery {
                    attempted: false,
                    succeeded: None,
                    message: Some(error.to_string()),
                    actions: vec![RecoveryAction::Retry, RecoveryAction::OpenLogs],
                },
            ),
        };
        self.push_activity(result.clone());
        result
    }

    pub fn restore_backup(&self, expected_revision: u64) -> Result<SaveResponse, ServiceError> {
        let previous_launch_at_login = self.launch_at_login_enabled();
        let mut backup = self.repository.load_backup()?;
        backup.revision = expected_revision;
        let mut response = self.save_and_apply(backup, expected_revision)?;
        let startup_changed =
            previous_launch_at_login != response.settings.preferences.launch_at_login;
        response.result.action_type = "restore_backup".into();
        response.result.message = if startup_changed {
            "백업을 복원하고 Windows 시작 프로그램 설정을 적용했습니다."
        } else {
            "백업을 복원하고 적용했습니다."
        }
        .into();
        response.result.details = Some(json!({
            "stage": "verified",
            "settingsPath": self.repository.path(),
            "launchAtLogin": response.settings.preferences.launch_at_login,
            "startupRegistrationChanged": startup_changed,
        }));
        self.replace_activity(response.result.clone());
        Ok(response)
    }

    pub fn activity(&self) -> Vec<ActionResult> {
        self.activity.lock().iter().cloned().collect()
    }

    pub fn record_result(&self, result: ActionResult) {
        self.push_activity(result);
    }

    fn push_activity(&self, result: ActionResult) {
        let mut activity = self.activity.lock();
        activity.push_front(result);
        activity.truncate(MAX_ACTIVITY);
    }

    fn replace_activity(&self, result: ActionResult) {
        let mut activity = self.activity.lock();
        if let Some(existing) = activity
            .iter_mut()
            .find(|item| item.action_id == result.action_id)
        {
            *existing = result;
        } else {
            activity.push_front(result);
            activity.truncate(MAX_ACTIVITY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyforge_config::{
        Action, ConditionGroup, ConditionOperator, MatchCondition, Profile, ProfileScope, Rule,
        Settings, TextOperator,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tempfile::tempdir;

    struct FakeLaunchAtLoginRegistrar {
        registration: Mutex<LaunchAtLoginRegistration>,
        fail_set: AtomicBool,
        set_calls: AtomicUsize,
    }

    impl FakeLaunchAtLoginRegistrar {
        fn new(command: Option<&str>) -> Self {
            Self {
                registration: Mutex::new(LaunchAtLoginRegistration {
                    command: command.map(str::to_owned),
                    startup_approved: None,
                }),
                fail_set: AtomicBool::new(false),
                set_calls: AtomicUsize::new(0),
            }
        }

        fn registration(&self) -> LaunchAtLoginRegistration {
            self.registration.lock().clone()
        }
    }

    impl LaunchAtLoginRegistrar for FakeLaunchAtLoginRegistrar {
        fn snapshot(&self) -> Result<LaunchAtLoginRegistration, LaunchAtLoginError> {
            Ok(self.registration())
        }

        fn set_enabled(&self, enabled: bool) -> Result<(), LaunchAtLoginError> {
            if self.fail_set.load(Ordering::SeqCst) {
                return Err(LaunchAtLoginError::Registry(
                    "injected registry failure".into(),
                ));
            }
            self.set_calls.fetch_add(1, Ordering::SeqCst);
            self.registration.lock().command = enabled.then(|| "\"fake-keyforge.exe\"".into());
            Ok(())
        }

        fn restore(
            &self,
            registration: &LaunchAtLoginRegistration,
        ) -> Result<(), LaunchAtLoginError> {
            *self.registration.lock() = registration.clone();
            Ok(())
        }
    }

    fn service_with_registrar(
        path: PathBuf,
        registrar: Arc<FakeLaunchAtLoginRegistrar>,
    ) -> AppService {
        AppService::new_with_launch_at_login_registrar(path, false, registrar).unwrap()
    }

    #[test]
    fn launch_at_login_commits_settings_and_registry_as_one_operation() {
        let dir = tempdir().unwrap();
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let service = service_with_registrar(dir.path().join("settings.json"), registrar.clone());
        let initial = service.bootstrap();
        let mut draft = initial.settings.clone();
        draft.preferences.launch_at_login = true;

        let response = service
            .save_and_apply(draft, initial.settings.revision)
            .unwrap();

        assert!(response.settings.preferences.launch_at_login);
        assert_eq!(response.result.action_type, "set_launch_at_login");
        assert_eq!(
            registrar.registration().command.as_deref(),
            Some("\"fake-keyforge.exe\"")
        );
        assert_eq!(registrar.set_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn launch_at_login_registry_failure_leaves_settings_unchanged() {
        let dir = tempdir().unwrap();
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        registrar.fail_set.store(true, Ordering::SeqCst);
        let service = service_with_registrar(dir.path().join("settings.json"), registrar.clone());
        let initial = service.bootstrap();
        let mut draft = initial.settings.clone();
        draft.preferences.launch_at_login = true;

        let error = service
            .save_and_apply(draft, initial.settings.revision)
            .unwrap_err();

        assert!(matches!(
            error,
            ServiceError::Startup(LaunchAtLoginError::Registry(_))
        ));
        assert_eq!(service.bootstrap().settings, initial.settings);
        assert_eq!(registrar.registration().command, None);
        assert_eq!(registrar.set_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stale_settings_save_restores_the_previous_startup_registration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(Some("\"before.exe\"")));
        let service = service_with_registrar(path.clone(), registrar.clone());
        let initial = service.bootstrap();

        let competing_repository = SettingsRepository::new(&path);
        let mut competing_draft = initial.settings.clone();
        competing_draft.profiles[0].name = "External update".into();
        competing_repository
            .save(competing_draft, initial.settings.revision)
            .unwrap();

        let mut draft = initial.settings.clone();
        draft.preferences.launch_at_login = true;
        let error = service
            .save_and_apply(draft, initial.settings.revision)
            .unwrap_err();

        assert!(matches!(
            error,
            ServiceError::Repository(RepositoryError::StaleRevision { .. })
        ));
        assert_eq!(
            registrar.registration().command.as_deref(),
            Some("\"before.exe\"")
        );
    }

    #[test]
    fn backup_restore_applies_the_backed_up_startup_setting() {
        let dir = tempdir().unwrap();
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let service = service_with_registrar(dir.path().join("settings.json"), registrar.clone());
        let initial = service.bootstrap();
        assert_eq!(service.create_backup().status, ActionStatus::Success);

        let mut enabled = initial.settings.clone();
        enabled.preferences.launch_at_login = true;
        let enabled = service
            .save_and_apply(enabled, initial.settings.revision)
            .unwrap();
        assert!(service.launch_at_login_enabled());

        let restored = service.restore_backup(enabled.settings.revision).unwrap();
        assert!(!restored.settings.preferences.launch_at_login);
        assert_eq!(restored.result.action_type, "restore_backup");
        assert_eq!(registrar.registration().command, None);
    }

    #[test]
    fn save_without_hook_is_warning_but_verified_and_revisioned() {
        let dir = tempdir().unwrap();
        let service = AppService::new(dir.path().join("settings.json"), false).unwrap();
        let initial = service.bootstrap();
        let mut draft = initial.settings.clone();
        draft.profiles.push(Profile::new("Global"));
        let response = service
            .save_and_apply(draft, initial.settings.revision)
            .unwrap();
        assert_eq!(response.settings.revision, initial.settings.revision + 1);
        assert_eq!(response.result.status, ActionStatus::Warning);
        assert_eq!(service.activity().len(), 1);
        assert!(matches!(
            response.settings.profiles.last().unwrap().scope,
            keyforge_config::ProfileScope::Global
        ));
    }

    #[test]
    fn legacy_device_scoped_modifier_remap_boots_as_global() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings {
            schema_version: 1,
            ..Settings::default()
        };
        let mut profile = Profile::new("독거미 AULA 84");
        profile.scope = ProfileScope::Device {
            conditions: ConditionGroup {
                operator: ConditionOperator::And,
                conditions: vec![MatchCondition::DeviceId {
                    operator: TextOperator::Contains,
                    value: "VID_3434&PID_01A0".into(),
                }],
            },
        };
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        settings.profiles = vec![profile];
        std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();

        let service = AppService::new(path, cfg!(windows)).unwrap();
        let bootstrap = service.bootstrap();
        #[cfg(windows)]
        {
            assert!(bootstrap.runtime.hook_installed);
            assert_eq!(bootstrap.runtime.engine_state, EngineState::Running);
        }
        let migrated = bootstrap.settings;
        assert_eq!(
            migrated.schema_version,
            keyforge_config::CURRENT_SCHEMA_VERSION
        );
        assert!(matches!(migrated.profiles[0].scope, ProfileScope::Global));
        assert!(matches!(
            &migrated.profiles[0].rules[0].action,
            Action::SendKeys { chord } if chord == &vec!["MetaLeft"]
        ));
    }

    #[test]
    fn close_to_tray_preference_is_live_and_shutdown_is_idempotent() {
        let dir = tempdir().unwrap();
        let service = AppService::new(dir.path().join("settings.json"), false).unwrap();
        let initial = service.bootstrap();
        assert!(service.close_to_tray());
        assert!(!service.start_minimized());

        let mut draft = initial.settings.clone();
        draft.preferences.close_to_tray = false;
        draft.preferences.start_minimized = true;
        service
            .save_and_apply(draft, initial.settings.revision)
            .unwrap();

        assert!(!service.close_to_tray());
        assert!(service.start_minimized());
        service.shutdown();
        service.shutdown();
    }
}
