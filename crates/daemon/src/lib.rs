use directories::ProjectDirs;
use keyforge_config::{
    ActionResult, ActionStatus, Profile, Recovery, RecoveryAction, RepositoryError, Settings,
    SettingsRepository, validate,
};
use keyforge_engine::{CompileError, CompiledRules};
pub use keyforge_platform_windows::KeyboardDeviceInfo;
use keyforge_platform_windows::{
    DeviceInventoryError, HookService, LaunchAtLoginError, LaunchAtLoginRegistration,
    keyboard_matches_selector, list_connected_keyboards, restore_launch_at_login,
    set_launch_at_login, snapshot_launch_at_login,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use thiserror::Error;

const MAX_ACTIVITY: usize = 200;
const DEVICE_POLL_INTERVAL: Duration = Duration::from_secs(2);

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

trait KeyboardInventoryProvider: Send + Sync {
    fn list(&self) -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError>;
}

struct WindowsLaunchAtLoginRegistrar;
struct WindowsKeyboardInventoryProvider;

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

impl KeyboardInventoryProvider for WindowsKeyboardInventoryProvider {
    fn list(&self) -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
        list_connected_keyboards()
    }
}

struct DeviceMonitor {
    stop: mpsc::Sender<()>,
    join: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct ActiveProfileCandidate {
    index: usize,
    profile: Profile,
    conditional: bool,
    specificity: usize,
}

pub struct AppService {
    repository: Arc<SettingsRepository>,
    settings: Arc<RwLock<Settings>>,
    hook: Arc<Mutex<Option<HookService>>>,
    hook_error: Arc<RwLock<Option<String>>>,
    activity: Mutex<VecDeque<ActionResult>>,
    operation: Arc<Mutex<()>>,
    launch_at_login: Arc<dyn LaunchAtLoginRegistrar>,
    inventory: Arc<dyn KeyboardInventoryProvider>,
    connected_keyboards: Arc<RwLock<Vec<KeyboardDeviceInfo>>>,
    active_profile_count: Arc<AtomicUsize>,
    device_monitor: Mutex<Option<DeviceMonitor>>,
}

impl AppService {
    pub fn new_default() -> anyhow::Result<Self> {
        let project = ProjectDirs::from("com", "KeyForge", "KeyForge")
            .ok_or_else(|| anyhow::anyhow!("unable to resolve LOCALAPPDATA"))?;
        let path = project.data_local_dir().join("settings.json");
        Self::new(path, true)
    }

    pub fn new(path: PathBuf, enable_hooks: bool) -> anyhow::Result<Self> {
        Self::new_with_dependencies(
            path,
            enable_hooks,
            Arc::new(WindowsLaunchAtLoginRegistrar),
            Arc::new(WindowsKeyboardInventoryProvider),
            cfg!(windows),
        )
    }

    fn new_with_dependencies(
        path: PathBuf,
        enable_hooks: bool,
        launch_at_login: Arc<dyn LaunchAtLoginRegistrar>,
        inventory: Arc<dyn KeyboardInventoryProvider>,
        monitor_connected_devices: bool,
    ) -> anyhow::Result<Self> {
        let repository = Arc::new(SettingsRepository::new(path));
        let mut settings = repository.load_or_default()?;
        if !repository.path().exists() {
            settings = repository.save(settings, 0)?;
        }
        let connected_keyboards = inventory.list().unwrap_or_default();
        let (compiled, active_profile_count) =
            compile_runtime_rules(&settings, &connected_keyboards)?;
        let (hook, hook_error) = if enable_hooks {
            match HookService::start(compiled) {
                Ok(hook) => (Some(hook), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, Some("input hooks disabled for this service".into()))
        };
        let service = Self {
            repository,
            settings: Arc::new(RwLock::new(settings)),
            hook: Arc::new(Mutex::new(hook)),
            hook_error: Arc::new(RwLock::new(hook_error)),
            activity: Mutex::new(VecDeque::new()),
            operation: Arc::new(Mutex::new(())),
            launch_at_login,
            inventory,
            connected_keyboards: Arc::new(RwLock::new(connected_keyboards)),
            active_profile_count: Arc::new(AtomicUsize::new(active_profile_count)),
            device_monitor: Mutex::new(None),
        };
        if monitor_connected_devices {
            service.start_device_monitor();
        }
        Ok(service)
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
            active_profile_count: self.active_profile_count.load(Ordering::Acquire),
            hook_installed: installed,
            capabilities: vec![
                "keyboard_remap".into(),
                "mouse_remap".into(),
                "atomic_settings".into(),
                "backup_restore".into(),
                "activity_feed".into(),
                "device_inventory".into(),
                "connected_profile_activation".into(),
            ],
        }
    }

    pub fn connected_keyboards(&self) -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
        let keyboards = self.inventory.list()?;
        self.apply_connected_keyboards_snapshot(keyboards.clone());
        Ok(keyboards)
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
        if let Some(mut monitor) = self.device_monitor.lock().take() {
            let _ = monitor.stop.send(());
            if let Some(join) = monitor.join.take() {
                let _ = join.join();
            }
        }
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
        validate_runtime_profiles(&draft).map_err(compile_error_to_repository)?;
        let saved = self.repository.save(draft, expected_revision)?;
        let connected_keyboards = self.connected_keyboards.read().clone();
        let (compiled, active_profile_count) = compile_runtime_rules(&saved, &connected_keyboards)
            .map_err(compile_error_to_repository)?;
        let hook_applied = self.apply_compiled_rules(compiled, active_profile_count);
        *self.settings.write() = saved.clone();

        let mut result = if hook_applied {
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

    fn start_device_monitor(&self) {
        let inventory = self.inventory.clone();
        let operation = self.operation.clone();
        let settings = self.settings.clone();
        let hook = self.hook.clone();
        let hook_error = self.hook_error.clone();
        let connected_keyboards = self.connected_keyboards.clone();
        let active_profile_count = self.active_profile_count.clone();
        let (stop_tx, stop_rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("keyforge-device-monitor".into())
            .spawn(move || {
                loop {
                    match stop_rx.recv_timeout(DEVICE_POLL_INTERVAL) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    let Ok(keyboards) = inventory.list() else {
                        continue;
                    };
                    let _operation = operation.lock();
                    if keyboards == *connected_keyboards.read() {
                        continue;
                    }
                    let settings = settings.read().clone();
                    match compile_runtime_rules(&settings, &keyboards) {
                        Ok((compiled, count)) => {
                            *connected_keyboards.write() = keyboards;
                            active_profile_count.store(count, Ordering::Release);
                            if let Some(installed_hook) =
                                hook.lock().as_ref().filter(|hook| hook.is_installed())
                            {
                                installed_hook.update_rules(compiled);
                            }
                        }
                        Err(error) => {
                            *hook_error.write() = Some(error.to_string());
                        }
                    }
                }
            })
            .ok();
        if let Some(join) = join {
            *self.device_monitor.lock() = Some(DeviceMonitor {
                stop: stop_tx,
                join: Some(join),
            });
        }
    }

    fn apply_connected_keyboards_snapshot(&self, keyboards: Vec<KeyboardDeviceInfo>) {
        let _operation = self.operation.lock();
        let settings = self.settings.read().clone();
        match compile_runtime_rules(&settings, &keyboards) {
            Ok((compiled, count)) => {
                *self.connected_keyboards.write() = keyboards;
                self.apply_compiled_rules(compiled, count);
            }
            Err(error) => {
                *self.hook_error.write() = Some(error.to_string());
            }
        }
    }

    fn apply_compiled_rules(&self, compiled: CompiledRules, active_profile_count: usize) -> bool {
        self.active_profile_count
            .store(active_profile_count, Ordering::Release);
        if let Some(hook) = self.hook.lock().as_ref().filter(|hook| hook.is_installed()) {
            hook.update_rules(compiled);
            true
        } else {
            false
        }
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

fn compile_runtime_rules(
    settings: &Settings,
    connected_keyboards: &[KeyboardDeviceInfo],
) -> Result<(CompiledRules, usize), CompileError> {
    validate_runtime_profiles(settings)?;
    let mut candidates = settings
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(index, profile)| {
            activation_specificity(profile, connected_keyboards).map(|specificity| {
                ActiveProfileCandidate {
                    index,
                    profile: profile.clone(),
                    conditional: !profile.activation.always_active(),
                    specificity,
                }
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .conditional
            .cmp(&left.conditional)
            .then(right.specificity.cmp(&left.specificity))
            .then(left.index.cmp(&right.index))
    });
    let active_profile_count = candidates.len();
    let mut effective = settings.clone();
    effective.profiles.clear();
    for candidate in candidates {
        let mut filtered_profile = candidate.profile.clone();
        filtered_profile.rules.clear();
        for rule in candidate.profile.rules.iter().filter(|rule| rule.enabled) {
            filtered_profile.rules.push(rule.clone());
            let mut attempt = effective.clone();
            attempt.profiles.push(filtered_profile.clone());
            match CompiledRules::compile(&attempt) {
                Ok(_) => {}
                Err(CompileError::Conflict { .. }) => {
                    filtered_profile.rules.pop();
                }
                Err(error) => return Err(error),
            }
        }
        effective.profiles.push(filtered_profile);
    }
    CompiledRules::compile(&effective).map(|compiled| (compiled, active_profile_count))
}

fn validate_runtime_profiles(settings: &Settings) -> Result<(), CompileError> {
    for profile in settings
        .profiles
        .iter()
        .filter(|profile| profile.enabled && !profile.archived)
    {
        let mut isolated = settings.clone();
        isolated.profiles = vec![profile.clone()];
        CompiledRules::compile(&isolated)?;
    }
    Ok(())
}

fn activation_specificity(
    profile: &Profile,
    connected_keyboards: &[KeyboardDeviceInfo],
) -> Option<usize> {
    if !profile.enabled || profile.archived {
        return None;
    }
    if profile.activation.always_active() {
        return Some(0);
    }
    profile
        .activation
        .connected_keyboards
        .iter()
        .filter(|selector| {
            connected_keyboards
                .iter()
                .any(|keyboard| keyboard_matches_selector(keyboard, selector))
        })
        .map(|selector| selector.specificity())
        .max()
}

fn compile_error_to_repository(error: CompileError) -> RepositoryError {
    RepositoryError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        error.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyforge_config::{
        Action, ConditionGroup, ConditionOperator, DeviceSelector, MatchCondition, Profile,
        ProfileActivation, ProfileScope, Rule, Settings, TextOperator,
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

    struct FakeKeyboardInventoryProvider {
        keyboards: Mutex<Vec<KeyboardDeviceInfo>>,
    }

    impl FakeLaunchAtLoginRegistrar {
        fn new(command: Option<&str>) -> Self {
            Self {
                registration: Mutex::new(LaunchAtLoginRegistration {
                    command: command.map(str::to_owned),
                }),
                fail_set: AtomicBool::new(false),
                set_calls: AtomicUsize::new(0),
            }
        }

        fn registration(&self) -> LaunchAtLoginRegistration {
            self.registration.lock().clone()
        }
    }

    impl FakeKeyboardInventoryProvider {
        fn new(keyboards: Vec<KeyboardDeviceInfo>) -> Self {
            Self {
                keyboards: Mutex::new(keyboards),
            }
        }

        fn set(&self, keyboards: Vec<KeyboardDeviceInfo>) {
            *self.keyboards.lock() = keyboards;
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

    impl KeyboardInventoryProvider for FakeKeyboardInventoryProvider {
        fn list(&self) -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
            Ok(self.keyboards.lock().clone())
        }
    }

    fn sample_keyboard(vendor_id: &str, product_id: &str, name: &str) -> KeyboardDeviceInfo {
        KeyboardDeviceInfo {
            id: format!("rawkbd-{vendor_id}{product_id}"),
            name: name.into(),
            device_path: format!(r"\\?\HID#VID_{vendor_id}&PID_{product_id}&MI_00#7&1234&0&0000"),
            manufacturer: Some("Example Devices".into()),
            instance_id: Some(format!(
                r"HID\VID_{vendor_id}&PID_{product_id}&MI_00\7&1234&0&0000"
            )),
            container_id: Some(format!("{{{vendor_id}-{product_id}}}")),
            hardware_ids: vec!["HID_DEVICE_SYSTEM_KEYBOARD".into()],
            location_paths: vec![r"PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(3)".into()],
            vendor_id: Some(vendor_id.into()),
            product_id: Some(product_id.into()),
            interface_id: Some("00".into()),
            keyboard_type: 4,
            keyboard_sub_type: 0,
            keyboard_mode: 1,
            function_key_count: 12,
            indicator_count: 3,
            total_key_count: 104,
            is_virtual: false,
            source: "raw_input".into(),
        }
    }

    fn service_with_dependencies(
        path: PathBuf,
        registrar: Arc<FakeLaunchAtLoginRegistrar>,
        inventory: Arc<FakeKeyboardInventoryProvider>,
    ) -> AppService {
        AppService::new_with_dependencies(path, false, registrar, inventory, false).unwrap()
    }

    fn profile_with_connected_keyboard_rule(name: &str, selector: DeviceSelector) -> Profile {
        let mut profile = Profile::new(name);
        profile.activation = ProfileActivation {
            connected_keyboards: vec![selector],
        };
        profile.rules.push(Rule::key_remap("CapsLock", "Escape"));
        profile
    }

    #[test]
    fn launch_at_login_commits_settings_and_registry_as_one_operation() {
        let dir = tempdir().unwrap();
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        let service = service_with_dependencies(
            dir.path().join("settings.json"),
            registrar.clone(),
            inventory,
        );
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
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        registrar.fail_set.store(true, Ordering::SeqCst);
        let service = service_with_dependencies(
            dir.path().join("settings.json"),
            registrar.clone(),
            inventory,
        );
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
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        let service = service_with_dependencies(path.clone(), registrar.clone(), inventory);
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
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        let service = service_with_dependencies(
            dir.path().join("settings.json"),
            registrar.clone(),
            inventory,
        );
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
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        let service =
            service_with_dependencies(dir.path().join("settings.json"), registrar, inventory);
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
    fn connected_keyboard_activation_enables_matching_profiles() {
        let dir = tempdir().unwrap();
        let keyboard = sample_keyboard("046D", "C31C", "회사 키보드");
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(vec![keyboard]));
        let service =
            service_with_dependencies(dir.path().join("settings.json"), registrar, inventory);
        let mut draft = service.bootstrap().settings;
        draft.profiles.push(profile_with_connected_keyboard_rule(
            "회사 프로필",
            DeviceSelector {
                vendor_id: Some("046D".into()),
                product_id: Some("C31C".into()),
                ..DeviceSelector::default()
            },
        ));
        let saved = service.save_and_apply(draft, 1).unwrap();
        assert_eq!(saved.settings.revision, 2);
        assert_eq!(service.runtime_state().active_profile_count, 1);
    }

    #[test]
    fn connected_keyboard_activation_turns_off_when_device_disappears() {
        let dir = tempdir().unwrap();
        let keyboard = sample_keyboard("046D", "C31C", "회사 키보드");
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(vec![keyboard]));
        let service = service_with_dependencies(
            dir.path().join("settings.json"),
            registrar,
            inventory.clone(),
        );
        let mut draft = service.bootstrap().settings;
        draft.profiles.push(profile_with_connected_keyboard_rule(
            "회사 프로필",
            DeviceSelector {
                vendor_id: Some("046D".into()),
                product_id: Some("C31C".into()),
                ..DeviceSelector::default()
            },
        ));
        service.save_and_apply(draft, 1).unwrap();
        assert_eq!(service.runtime_state().active_profile_count, 1);

        inventory.set(Vec::new());
        service.connected_keyboards().unwrap();
        assert_eq!(service.runtime_state().active_profile_count, 0);
    }

    #[test]
    fn connected_keyboard_profile_rules_override_global_conflicts() {
        use keyforge_engine::{EventOrigin, KeyEvent, KeyPhase, MatchContext, RuntimeEngine};

        let keyboard = sample_keyboard("046D", "C31C", "회사 키보드");
        let mut settings = Settings::default();
        let mut global = Profile::new("전역 프로필");
        global.rules.push(Rule::key_remap("CapsLock", "Escape"));
        let mut conditional = profile_with_connected_keyboard_rule(
            "회사 프로필",
            DeviceSelector {
                vendor_id: Some("046D".into()),
                product_id: Some("C31C".into()),
                ..DeviceSelector::default()
            },
        );
        conditional.rules.clear();
        conditional.rules.push(Rule::key_remap("CapsLock", "Enter"));
        settings.profiles = vec![global, conditional];

        let (compiled, active_count) = compile_runtime_rules(&settings, &[keyboard]).unwrap();
        assert_eq!(active_count, 2);
        assert_eq!(compiled.len(), 1);

        let mut engine = RuntimeEngine::new(compiled);
        let dispatch = engine.process(
            &KeyEvent {
                key: "CapsLock".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(dispatch.suppress_original);
        assert!(matches!(
            &dispatch.actions[0].action,
            Action::SendKeys { chord } if chord == &vec!["Enter"]
        ));
    }
    #[test]
    fn selectorless_profiles_remain_active_without_devices() {
        let dir = tempdir().unwrap();
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        let service =
            service_with_dependencies(dir.path().join("settings.json"), registrar, inventory);
        assert_eq!(service.runtime_state().active_profile_count, 0);

        let mut draft = service.bootstrap().settings;
        draft.profiles.push(Profile::new("항상 활성"));
        service.save_and_apply(draft, 1).unwrap();
        assert_eq!(service.runtime_state().active_profile_count, 1);
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
        let registrar = Arc::new(FakeLaunchAtLoginRegistrar::new(None));
        let inventory = Arc::new(FakeKeyboardInventoryProvider::new(Vec::new()));
        let service =
            service_with_dependencies(dir.path().join("settings.json"), registrar, inventory);
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
