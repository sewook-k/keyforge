use std::{
    collections::HashMap,
    io,
    mem::size_of,
    ptr::{addr_of, null, null_mut},
    slice,
};

use windows_sys::Win32::{
    Devices::{
        DeviceAndDriverInstallation::{
            DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
            SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
            SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
            SetupDiGetDevicePropertyW,
        },
        HumanInterfaceDevice::GUID_DEVINTERFACE_KEYBOARD,
        Properties::{
            DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_ContainerId,
            DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName, DEVPKEY_Device_HardwareIds,
            DEVPKEY_Device_InstanceId, DEVPKEY_Device_LocationPaths, DEVPKEY_Device_Manufacturer,
            DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_INDIRECT,
            DEVPROP_TYPE_STRING_LIST, DEVPROPTYPE,
        },
    },
    Foundation::{
        DEVPROPKEY, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, INVALID_HANDLE_VALUE,
    },
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PnpKeyboardMetadata {
    pub display_name: Option<String>,
    pub manufacturer: Option<String>,
    pub instance_id: Option<String>,
    pub container_id: Option<String>,
    pub hardware_ids: Vec<String>,
    pub location_paths: Vec<String>,
}

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

struct PropertyData {
    property_type: DEVPROPTYPE,
    bytes: Vec<u8>,
}

pub(crate) fn normalize_device_path(path: &str) -> String {
    path.to_ascii_lowercase()
}

pub(crate) fn keyboard_metadata_by_path() -> io::Result<HashMap<String, PnpKeyboardMetadata>> {
    let raw_set = unsafe {
        SetupDiGetClassDevsW(
            &GUID_DEVINTERFACE_KEYBOARD,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if raw_set == INVALID_HANDLE_VALUE as HDEVINFO {
        return Err(io::Error::last_os_error());
    }
    let set = DeviceInfoSet(raw_set);
    let mut result = HashMap::new();

    // A corrupt provider must not leave inventory refresh stuck forever. In
    // practice the keyboard interface count is tiny; this is only a guard.
    for index in 0..4096u32 {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..SP_DEVICE_INTERFACE_DATA::default()
        };
        let enumerated = unsafe {
            SetupDiEnumDeviceInterfaces(
                set.0,
                null(),
                &GUID_DEVINTERFACE_KEYBOARD,
                index,
                &mut interface,
            )
        };
        if enumerated == 0 {
            if last_error_code() == ERROR_NO_MORE_ITEMS {
                break;
            }
            // There is no interface record to skip safely. Keep metadata
            // already collected and let every unmatched Raw Input endpoint use
            // its existing fallback information.
            break;
        }

        let Some((device_path, device_info)) = interface_detail(set.0, &interface) else {
            continue;
        };
        result.insert(
            normalize_device_path(&device_path),
            read_metadata(set.0, &device_info),
        );
    }

    Ok(result)
}

fn interface_detail(
    set: HDEVINFO,
    interface: &SP_DEVICE_INTERFACE_DATA,
) -> Option<(String, SP_DEVINFO_DATA)> {
    let mut required_size = 0u32;
    unsafe {
        let _ = SetupDiGetDeviceInterfaceDetailW(
            set,
            interface,
            null_mut(),
            0,
            &mut required_size,
            null_mut(),
        );
    }
    if required_size < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 {
        return None;
    }

    // Vec<usize> provides pointer alignment for the variable-length SetupAPI
    // detail structure. A Vec<u8> cast here would not be sufficiently aligned.
    let byte_count = required_size as usize;
    let storage_units = byte_count.div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; storage_units];
    let detail = storage
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    unsafe {
        (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
    }
    let mut device_info = SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ..SP_DEVINFO_DATA::default()
    };
    let succeeded = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            set,
            interface,
            detail,
            required_size,
            &mut required_size,
            &mut device_info,
        )
    };
    if succeeded == 0 {
        return None;
    }

    let path_pointer = unsafe { addr_of!((*detail).DevicePath).cast::<u16>() };
    let path_offset = path_pointer as usize - detail as usize;
    if path_offset >= byte_count {
        return None;
    }
    let path_units = (byte_count - path_offset) / size_of::<u16>();
    let path = unsafe { slice::from_raw_parts(path_pointer, path_units) };
    decode_utf16_z(path).map(|path| (path, device_info))
}

fn read_metadata(set: HDEVINFO, device: &SP_DEVINFO_DATA) -> PnpKeyboardMetadata {
    let bus_reported_name = property_string(set, device, &DEVPKEY_Device_BusReportedDeviceDesc);
    let friendly_name = property_string(set, device, &DEVPKEY_Device_FriendlyName);
    let device_description = property_string(set, device, &DEVPKEY_Device_DeviceDesc);
    PnpKeyboardMetadata {
        display_name: preferred_display_name(&[
            bus_reported_name,
            friendly_name,
            device_description,
        ]),
        manufacturer: property_string(set, device, &DEVPKEY_Device_Manufacturer),
        instance_id: property_string(set, device, &DEVPKEY_Device_InstanceId),
        container_id: property_guid(set, device, &DEVPKEY_Device_ContainerId),
        hardware_ids: property_string_list(set, device, &DEVPKEY_Device_HardwareIds),
        location_paths: property_string_list(set, device, &DEVPKEY_Device_LocationPaths),
    }
}

fn preferred_display_name(candidates: &[Option<String>]) -> Option<String> {
    candidates
        .iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .cloned()
}

fn property_string(set: HDEVINFO, device: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
    let property = property_data(set, device, key)?;
    if !matches!(
        property.property_type,
        DEVPROP_TYPE_STRING | DEVPROP_TYPE_STRING_INDIRECT
    ) {
        return None;
    }
    decode_utf16_bytes(&property.bytes)
}

fn property_string_list(set: HDEVINFO, device: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Vec<String> {
    let Some(property) = property_data(set, device, key) else {
        return Vec::new();
    };
    match property.property_type {
        DEVPROP_TYPE_STRING_LIST => decode_multi_sz_bytes(&property.bytes),
        DEVPROP_TYPE_STRING => decode_utf16_bytes(&property.bytes).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn property_guid(set: HDEVINFO, device: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
    let property = property_data(set, device, key)?;
    (property.property_type == DEVPROP_TYPE_GUID)
        .then(|| decode_guid_bytes(&property.bytes))
        .flatten()
}

fn property_data(
    set: HDEVINFO,
    device: &SP_DEVINFO_DATA,
    key: &DEVPROPKEY,
) -> Option<PropertyData> {
    let mut property_type = 0;
    let mut required_size = 0u32;
    let first = unsafe {
        SetupDiGetDevicePropertyW(
            set,
            device,
            key,
            &mut property_type,
            null_mut(),
            0,
            &mut required_size,
            0,
        )
    };
    if first == 0 && last_error_code() != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    if required_size == 0 {
        return None;
    }

    let mut bytes = vec![0u8; required_size as usize];
    let second = unsafe {
        SetupDiGetDevicePropertyW(
            set,
            device,
            key,
            &mut property_type,
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            &mut required_size,
            0,
        )
    };
    if second == 0 {
        return None;
    }
    bytes.truncate((required_size as usize).min(bytes.len()));
    Some(PropertyData {
        property_type,
        bytes,
    })
}

fn last_error_code() -> u32 {
    io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default() as u32
}

fn utf16_units(bytes: &[u8]) -> Option<Vec<u16>> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect(),
    )
}

fn decode_utf16_z(units: &[u16]) -> Option<String> {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let value = String::from_utf16_lossy(&units[..end]);
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn decode_utf16_bytes(bytes: &[u8]) -> Option<String> {
    decode_utf16_z(&utf16_units(bytes)?)
}

fn decode_multi_sz_bytes(bytes: &[u8]) -> Vec<String> {
    let Some(units) = utf16_units(bytes) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut start = 0usize;
    while start < units.len() {
        let end = units[start..]
            .iter()
            .position(|unit| *unit == 0)
            .map(|relative| start + relative)
            .unwrap_or(units.len());
        if end == start {
            break;
        }
        if let Some(value) = decode_utf16_z(&units[start..end]) {
            values.push(value);
        }
        if end == units.len() {
            break;
        }
        start = end + 1;
    }
    values
}

fn decode_guid_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 16 {
        return None;
    }
    let data1 = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let data2 = u16::from_le_bytes(bytes[4..6].try_into().ok()?);
    let data3 = u16::from_le_bytes(bytes[6..8].try_into().ok()?);
    Some(format!(
        "{data1:08X}-{data2:04X}-{data3:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide_bytes(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_utf16_without_casting_unaligned_property_buffers() {
        let mut units = "기계식 키보드".encode_utf16().collect::<Vec<_>>();
        units.extend([0, b'X' as u16]);
        assert_eq!(
            decode_utf16_bytes(&wide_bytes(&units)).as_deref(),
            Some("기계식 키보드")
        );
        assert_eq!(decode_utf16_bytes(&[0x41]), None);
    }

    #[test]
    fn decodes_multi_sz_and_stops_at_double_null() {
        let mut units = Vec::new();
        units.extend("HID\\VID_3554&PID_FA09".encode_utf16());
        units.push(0);
        units.extend("HID_DEVICE_SYSTEM_KEYBOARD".encode_utf16());
        units.extend([0, 0]);
        units.extend("ignored".encode_utf16());
        assert_eq!(
            decode_multi_sz_bytes(&wide_bytes(&units)),
            vec![
                "HID\\VID_3554&PID_FA09".to_string(),
                "HID_DEVICE_SYSTEM_KEYBOARD".to_string()
            ]
        );
    }

    #[test]
    fn decodes_windows_guid_memory_layout() {
        let bytes = [
            0x06, 0xD2, 0x7E, 0x8C, 0x8A, 0x3F, 0x27, 0x48, 0xB3, 0xAB, 0xAE, 0x9E, 0x1F, 0xAE,
            0xFC, 0x6C,
        ];
        assert_eq!(
            decode_guid_bytes(&bytes).as_deref(),
            Some("8C7ED206-3F8A-4827-B3AB-AE9E1FAEFC6C")
        );
        assert_eq!(decode_guid_bytes(&bytes[..15]), None);
    }

    #[test]
    fn chooses_display_name_in_requested_pnp_priority_order() {
        assert_eq!(
            preferred_display_name(&[
                Some("Bus name".into()),
                Some("Friendly name".into()),
                Some("Device description".into())
            ])
            .as_deref(),
            Some("Bus name")
        );
        assert_eq!(
            preferred_display_name(&[None, Some("Friendly name".into()), None]).as_deref(),
            Some("Friendly name")
        );
    }

    #[test]
    fn normalizes_interface_paths_for_case_insensitive_matching() {
        assert_eq!(
            normalize_device_path(r"\\?\HID#VID_3554&PID_FA09"),
            normalize_device_path(r"\\?\hid#vid_3554&pid_fa09")
        );
    }

    #[test]
    fn setupapi_keyboard_enumeration_smoke_test() {
        let devices = keyboard_metadata_by_path().unwrap();
        for (path, metadata) in devices {
            assert!(!path.is_empty());
            assert!(
                metadata
                    .display_name
                    .as_ref()
                    .is_none_or(|name| !name.trim().is_empty())
            );
            assert!(
                metadata
                    .hardware_ids
                    .iter()
                    .all(|value| !value.trim().is_empty())
            );
            assert!(
                metadata
                    .location_paths
                    .iter()
                    .all(|value| !value.trim().is_empty())
            );
        }
    }
}
