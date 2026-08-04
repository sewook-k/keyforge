use keyforge_config::DeviceSelector;
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

impl KeyboardDeviceInfo {
    pub fn canonical_selector(&self) -> DeviceSelector {
        DeviceSelector {
            vendor_id: normalized_hex(self.vendor_id.as_deref()),
            product_id: normalized_hex(self.product_id.as_deref()),
            interface_id: normalized_hex(self.interface_id.as_deref()),
            manufacturer_contains: normalized_text(self.manufacturer.as_deref()),
            name_contains: normalized_text(Some(&self.name)),
            is_virtual: Some(self.is_virtual),
        }
    }
}

pub fn keyboard_matches_selector(keyboard: &KeyboardDeviceInfo, selector: &DeviceSelector) -> bool {
    selector.vendor_id.as_deref().is_none_or(|expected| {
        keyboard
            .vendor_id
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    }) && selector.product_id.as_deref().is_none_or(|expected| {
        keyboard
            .product_id
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    }) && selector.interface_id.as_deref().is_none_or(|expected| {
        keyboard
            .interface_id
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    }) && selector
        .manufacturer_contains
        .as_deref()
        .is_none_or(|expected| {
            contains_ignore_ascii_case(keyboard.manufacturer.as_deref(), expected)
        })
        && selector
            .name_contains
            .as_deref()
            .is_none_or(|expected| contains_ignore_ascii_case(Some(&keyboard.name), expected))
        && selector
            .is_virtual
            .is_none_or(|expected| keyboard.is_virtual == expected)
}

fn contains_ignore_ascii_case(actual: Option<&str>, expected: &str) -> bool {
    actual.is_some_and(|actual| {
        actual
            .to_ascii_lowercase()
            .contains(&expected.to_ascii_lowercase())
    })
}

fn normalized_hex(value: Option<&str>) -> Option<String> {
    value.map(|value| value.trim().to_ascii_uppercase())
}

fn normalized_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
    use super::{DeviceInventoryError, KeyboardDeviceInfo};
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
        pub fn stop(&mut self) {}
    }

    pub fn list_connected_keyboards() -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
        Err(DeviceInventoryError::Unsupported)
    }
}

#[cfg(not(windows))]
pub use portable_stub::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_keyboard() -> KeyboardDeviceInfo {
        KeyboardDeviceInfo {
            id: "rawkbd-0123456789abcdef".into(),
            name: "테스트 기계식 키보드".into(),
            device_path: r"\\?\HID#VID_046D&PID_C31C&MI_00#7&1234&0&0000".into(),
            manufacturer: Some("Example Devices".into()),
            instance_id: Some(r"HID\VID_046D&PID_C31C&MI_00\7&1234&0&0000".into()),
            container_id: Some("{01234567-89ab-cdef-0123-456789abcdef}".into()),
            hardware_ids: vec!["HID_DEVICE_SYSTEM_KEYBOARD".into()],
            location_paths: vec![r"PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(3)".into()],
            vendor_id: Some("046d".into()),
            product_id: Some("c31c".into()),
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

    #[test]
    fn canonical_selector_uses_stable_non_path_fields() {
        let selector = sample_keyboard().canonical_selector();
        assert_eq!(selector.vendor_id.as_deref(), Some("046D"));
        assert_eq!(selector.product_id.as_deref(), Some("C31C"));
        assert_eq!(selector.interface_id.as_deref(), Some("00"));
        assert_eq!(
            selector.manufacturer_contains.as_deref(),
            Some("Example Devices")
        );
        assert_eq!(
            selector.name_contains.as_deref(),
            Some("테스트 기계식 키보드")
        );
        assert_eq!(selector.is_virtual, Some(false));
    }

    #[test]
    fn selector_matching_ignores_case_and_supports_partial_names() {
        let keyboard = sample_keyboard();
        assert!(keyboard_matches_selector(
            &keyboard,
            &DeviceSelector {
                vendor_id: Some("046D".into()),
                product_id: Some("C31C".into()),
                interface_id: Some("00".into()),
                manufacturer_contains: Some("example".into()),
                name_contains: Some("기계식".into()),
                is_virtual: Some(false),
            }
        ));
        assert!(!keyboard_matches_selector(
            &keyboard,
            &DeviceSelector {
                product_id: Some("FFFF".into()),
                ..DeviceSelector::default()
            }
        ));
    }
}
