use keyforge_engine::KeyPhase;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod startup;

pub use startup::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardDeviceInfo {
    pub id: String,
    pub name: String,
    pub device_path: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub container_id: Option<String>,
    #[serde(default)]
    pub hardware_ids: Vec<String>,
    #[serde(default)]
    pub location_paths: Vec<String>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub interface_id: Option<String>,
    pub keyboard_type: u32,
    pub keyboard_sub_type: u32,
    pub keyboard_mode: u32,
    pub function_key_count: u32,
    pub indicator_count: u32,
    pub total_key_count: u32,
    pub is_virtual: bool,
    pub source: String,
}

/// A native physical key event collected while KeyForge is recording a chord.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyCaptureEvent {
    pub session_id: u64,
    pub key: String,
    pub phase: KeyPhase,
}

/// Identifies one short-lived native key-capture session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyCaptureSession {
    pub session_id: u64,
}

/// Bounded native capture data drained by the Tauri UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyCaptureDrain {
    pub session_id: u64,
    pub active: bool,
    pub overflowed: bool,
    pub events: Vec<KeyCaptureEvent>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DeviceInventoryError {
    #[error("connected keyboard inventory is only available on Windows")]
    Unsupported,
    #[error("failed to enumerate connected keyboards: {0}")]
    Windows(String),
}

#[cfg(windows)]
mod pnp_metadata;

#[cfg(windows)]
mod windows_impl;

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(windows))]
mod portable_stub {
    use super::{DeviceInventoryError, KeyCaptureDrain, KeyCaptureSession, KeyboardDeviceInfo};
    use keyforge_engine::CompiledRules;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("global input hooks are only available on Windows")]
    pub struct HookError;

    pub struct HookService;
    impl HookService {
        pub fn start(_: CompiledRules) -> Result<Self, HookError> {
            Err(HookError)
        }
        pub fn update_rules(&self, _: CompiledRules) {}
        pub fn set_paused(&self, _: bool) {}
        pub fn is_paused(&self) -> bool {
            true
        }
        pub fn is_installed(&self) -> bool {
            false
        }
        pub fn set_key_capture_window(&self, _: isize) {}
        pub fn begin_key_capture(&self) -> Result<KeyCaptureSession, HookError> {
            Err(HookError)
        }
        pub fn end_key_capture(&self, _: u64) -> bool {
            false
        }
        pub fn force_end_key_capture(&self) {}
        pub fn drain_key_capture_events(&self, session_id: u64) -> KeyCaptureDrain {
            KeyCaptureDrain {
                session_id,
                active: false,
                overflowed: false,
                events: Vec::new(),
            }
        }
        pub fn stop(&mut self) {}
    }

    pub fn list_connected_keyboards() -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
        Err(DeviceInventoryError::Unsupported)
    }

    pub fn record_window_system_key(_: u32, _: u32, _: bool, _: bool) -> bool {
        false
    }

    pub fn force_end_active_capture() {}
}

#[cfg(not(windows))]
pub use portable_stub::*;
