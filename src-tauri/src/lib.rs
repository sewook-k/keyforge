use keyforge_config::{ActionResult, Recovery, RecoveryAction, RepositoryError, Settings};
use keyforge_daemon::{AppService, Bootstrap, KeyboardDeviceInfo, SaveResponse, ServiceError};
use serde::Serialize;
use std::sync::Arc;
use tauri::{
    AppHandle, Emitter, Manager, Runtime, State,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

type Service = Arc<AppService>;

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "keyforge-tray";
const TRAY_OPEN_ID: &str = "keyforge-open";
const TRAY_QUIT_ID: &str = "keyforge-quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowCloseAction {
    HideToTray,
    AllowClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayAction {
    Open,
    Quit,
}

fn window_close_action(window_label: &str, close_to_tray: bool) -> WindowCloseAction {
    if window_label == MAIN_WINDOW_LABEL && close_to_tray {
        WindowCloseAction::HideToTray
    } else {
        WindowCloseAction::AllowClose
    }
}

fn effective_close_to_tray(saved_preference: Option<bool>) -> bool {
    saved_preference.unwrap_or(true)
}

fn tray_action(menu_id: &str) -> Option<TrayAction> {
    match menu_id {
        TRAY_OPEN_ID => Some(TrayAction::Open),
        TRAY_QUIT_ID => Some(TrayAction::Quit),
        _ => None,
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, TRAY_OPEN_ID, "KeyForge 열기", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "KeyForge 종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &separator, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("KeyForge · 키 매핑 실행 중")
        .on_menu_event(|app, event| match tray_action(event.id().as_ref()) {
            Some(TrayAction::Open) => show_main_window(app),
            Some(TrayAction::Quit) => app.exit(0),
            None => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: String,
    message: String,
    result: ActionResult,
}

fn command_error(action_type: &str, user_message: &str, error: ServiceError) -> CommandError {
    let code = match &error {
        ServiceError::Repository(repository_error) => match repository_error {
            RepositoryError::StaleRevision { .. } => "stale_revision",
            RepositoryError::Validation(_) => "validation_failed",
            RepositoryError::VerificationFailed => "verification_failed",
            RepositoryError::NoBackup => "no_backup",
            RepositoryError::Io(_) => "io_error",
            RepositoryError::Json(_) => "invalid_json",
        },
        ServiceError::Startup(_) | ServiceError::StartupRollback { .. } => {
            "startup_registration_failed"
        }
    };
    let message = error.to_string();
    let result = ActionResult::error(
        action_type,
        user_message,
        Recovery {
            attempted: false,
            succeeded: None,
            message: Some(message.clone()),
            actions: vec![
                RecoveryAction::Retry,
                RecoveryAction::RestoreBackup,
                RecoveryAction::OpenLogs,
            ],
        },
    );
    CommandError {
        code: code.into(),
        message,
        result,
    }
}

#[tauri::command]
fn bootstrap(service: State<'_, Service>) -> Bootstrap {
    service.bootstrap()
}

#[tauri::command]
fn save_settings(
    service: State<'_, Service>,
    app: AppHandle,
    draft: Settings,
    expected_revision: u64,
) -> Result<SaveResponse, Box<CommandError>> {
    let response = match service.save_and_apply(draft, expected_revision) {
        Ok(response) => response,
        Err(error) => {
            let command_error = command_error("save_settings", "설정을 저장하지 못했습니다", error);
            service.record_result(command_error.result.clone());
            let _ = app.emit("keyforge:action-result", &command_error.result);
            return Err(Box::new(command_error));
        }
    };
    let _ = app.emit("keyforge:action-result", &response.result);
    let _ = app.emit("keyforge:runtime-changed", service.runtime_state());
    Ok(response)
}

#[tauri::command]
fn set_engine_paused(service: State<'_, Service>, app: AppHandle, paused: bool) -> ActionResult {
    let result = service.set_engine_paused(paused);
    let _ = app.emit("keyforge:action-result", &result);
    let _ = app.emit("keyforge:runtime-changed", service.runtime_state());
    result
}

#[tauri::command]
fn create_backup(service: State<'_, Service>, app: AppHandle) -> ActionResult {
    let result = service.create_backup();
    let _ = app.emit("keyforge:action-result", &result);
    result
}

#[tauri::command]
fn restore_backup(
    service: State<'_, Service>,
    app: AppHandle,
    expected_revision: u64,
) -> Result<SaveResponse, Box<CommandError>> {
    let response = match service.restore_backup(expected_revision) {
        Ok(response) => response,
        Err(error) => {
            let command_error =
                command_error("restore_backup", "백업을 복원하지 못했습니다", error);
            service.record_result(command_error.result.clone());
            let _ = app.emit("keyforge:action-result", &command_error.result);
            return Err(Box::new(command_error));
        }
    };
    let _ = app.emit("keyforge:action-result", &response.result);
    let _ = app.emit("keyforge:runtime-changed", service.runtime_state());
    Ok(response)
}

#[tauri::command]
fn get_activity(service: State<'_, Service>) -> Vec<ActionResult> {
    service.activity()
}

#[tauri::command]
fn list_connected_keyboards(
    service: State<'_, Service>,
) -> Result<Vec<KeyboardDeviceInfo>, String> {
    service
        .connected_keyboards()
        .map_err(|error| error.to_string())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let service = AppService::new_default().map_err(Box::<dyn std::error::Error>::from)?;
            let start_minimized = service.start_minimized();
            app.manage(Arc::new(service));
            setup_tray(app)?;
            if !start_minimized {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = effective_close_to_tray(
                    window
                        .app_handle()
                        .try_state::<Service>()
                        .map(|service| service.close_to_tray()),
                );
                if window_close_action(window.label(), close_to_tray)
                    == WindowCloseAction::HideToTray
                {
                    api.prevent_close();
                    if window.hide().is_err() {
                        let _ = window.show();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_settings,
            set_engine_paused,
            create_backup,
            restore_backup,
            get_activity,
            list_connected_keyboards,
        ])
        .build(tauri::generate_context!())
        .expect("error while building KeyForge");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit)
            && let Some(service) = app.try_state::<Service>()
        {
            service.shutdown();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_main_window_with_the_saved_preference_hides_to_tray() {
        assert_eq!(
            window_close_action(MAIN_WINDOW_LABEL, true),
            WindowCloseAction::HideToTray
        );
        assert_eq!(
            window_close_action(MAIN_WINDOW_LABEL, false),
            WindowCloseAction::AllowClose
        );
        assert_eq!(
            window_close_action("diagnostics", true),
            WindowCloseAction::AllowClose
        );
        assert!(effective_close_to_tray(None));
        assert!(effective_close_to_tray(Some(true)));
        assert!(!effective_close_to_tray(Some(false)));
    }

    #[test]
    fn tray_menu_ids_have_explicit_actions() {
        assert_eq!(tray_action(TRAY_OPEN_ID), Some(TrayAction::Open));
        assert_eq!(tray_action(TRAY_QUIT_ID), Some(TrayAction::Quit));
        assert_eq!(tray_action("unknown"), None);
    }
}
