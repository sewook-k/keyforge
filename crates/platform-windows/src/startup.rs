use thiserror::Error;

/// A snapshot of the values owned by KeyForge in the current user's Windows
/// startup registry keys. It is intentionally opaque to callers other than the
/// daemon's transaction/rollback flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAtLoginRegistration {
    pub command: Option<String>,
    pub startup_approved: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum LaunchAtLoginError {
    #[error("Windows login startup registration is only available on Windows")]
    Unsupported,
    #[error("could not determine the KeyForge executable path: {0}")]
    ExecutablePath(String),
    #[error(
        "the KeyForge startup command is too long for the Windows Run key ({length} UTF-16 characters; maximum is {maximum})"
    )]
    CommandTooLong { length: usize, maximum: usize },
    #[error("Windows startup registry operation failed: {0}")]
    Registry(String),
}

/// Captures the KeyForge-owned startup value before changing it. The daemon
/// uses this to compensate if the atomic settings write fails afterwards.
pub fn snapshot_launch_at_login() -> Result<LaunchAtLoginRegistration, LaunchAtLoginError> {
    imp::snapshot()
}

/// Updates only the current user's `Run\\KeyForge` value. This is deliberately
/// invoked by an explicit user setting change, never during process startup.
pub fn set_launch_at_login(enabled: bool) -> Result<(), LaunchAtLoginError> {
    imp::set_enabled(enabled)
}

/// Restores a snapshot created by [`snapshot_launch_at_login`].
pub fn restore_launch_at_login(
    registration: &LaunchAtLoginRegistration,
) -> Result<(), LaunchAtLoginError> {
    imp::restore(registration)
}

fn quote_windows_command_argument(argument: &str) -> String {
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;

    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(character);
                backslashes = 0;
            }
        }
    }

    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(windows)]
mod imp {
    use super::{LaunchAtLoginError, LaunchAtLoginRegistration, quote_windows_command_argument};
    use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA},
        System::Registry::{
            HKEY_CURRENT_USER, REG_BINARY, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
            RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
        },
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const STARTUP_APPROVED_RUN_KEY: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
    const VALUE_NAME: &str = "KeyForge";
    const MAX_RUN_COMMAND_UTF16: usize = 260;

    const STARTUP_APPROVED_ENABLED: [u8; 12] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    pub(super) fn snapshot() -> Result<LaunchAtLoginRegistration, LaunchAtLoginError> {
        Ok(LaunchAtLoginRegistration {
            command: read_string_value(RUN_KEY, VALUE_NAME)?,
            startup_approved: read_binary_value(STARTUP_APPROVED_RUN_KEY, VALUE_NAME)?,
        })
    }

    pub(super) fn set_enabled(enabled: bool) -> Result<(), LaunchAtLoginError> {
        if enabled {
            let command = command_for_current_executable()?;
            set_enabled_for_value_names(VALUE_NAME, VALUE_NAME, true, Some(&command))
        } else {
            set_enabled_for_value_names(VALUE_NAME, VALUE_NAME, false, None)
        }
    }

    pub(super) fn restore(
        registration: &LaunchAtLoginRegistration,
    ) -> Result<(), LaunchAtLoginError> {
        match &registration.command {
            Some(command) => write_string_value(RUN_KEY, VALUE_NAME, command)?,
            None => delete_value(RUN_KEY, VALUE_NAME)?,
        }
        match &registration.startup_approved {
            Some(value) => write_binary_value(STARTUP_APPROVED_RUN_KEY, VALUE_NAME, value),
            None => delete_value(STARTUP_APPROVED_RUN_KEY, VALUE_NAME),
        }
    }

    fn set_enabled_for_value_names(
        run_value_name: &str,
        startup_approved_value_name: &str,
        enabled: bool,
        command: Option<&str>,
    ) -> Result<(), LaunchAtLoginError> {
        if enabled {
            let command = command.expect("enabled launch-at-login requires a startup command");
            write_string_value(RUN_KEY, run_value_name, command)?;
            write_binary_value(
                STARTUP_APPROVED_RUN_KEY,
                startup_approved_value_name,
                &STARTUP_APPROVED_ENABLED,
            )
        } else {
            delete_value(RUN_KEY, run_value_name)?;
            delete_value(STARTUP_APPROVED_RUN_KEY, startup_approved_value_name)
        }
    }

    fn command_for_current_executable() -> Result<String, LaunchAtLoginError> {
        let executable = std::env::current_exe()
            .map_err(|error| LaunchAtLoginError::ExecutablePath(error.to_string()))?;
        command_for_executable(&executable)
    }

    fn command_for_executable(executable: &Path) -> Result<String, LaunchAtLoginError> {
        let command = quote_windows_command_argument(&executable.to_string_lossy());
        let length = command.encode_utf16().count();
        if length > MAX_RUN_COMMAND_UTF16 {
            return Err(LaunchAtLoginError::CommandTooLong {
                length,
                maximum: MAX_RUN_COMMAND_UTF16,
            });
        }
        Ok(command)
    }

    fn read_string_value(
        key_path: &str,
        value_name: &str,
    ) -> Result<Option<String>, LaunchAtLoginError> {
        let key_path = wide(key_path);
        let value_name = wide(value_name);
        let mut bytes = 0_u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                value_name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                null_mut(),
                &mut bytes,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != 0 {
            return Err(registry_error("read startup registration", status));
        }

        let mut buffer = vec![0_u16; (bytes as usize).div_ceil(size_of::<u16>()) + 1];
        loop {
            let mut buffer_bytes =
                u32::try_from(buffer.len() * size_of::<u16>()).map_err(|_| {
                    LaunchAtLoginError::Registry("startup registration value is too large".into())
                })?;
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    key_path.as_ptr(),
                    value_name.as_ptr(),
                    RRF_RT_REG_SZ,
                    null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut buffer_bytes,
                )
            };
            if status == ERROR_MORE_DATA {
                let required = (buffer_bytes as usize)
                    .div_ceil(size_of::<u16>())
                    .max(buffer.len() * 2);
                buffer.resize(required + 1, 0);
                continue;
            }
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if status != 0 {
                return Err(registry_error("read startup registration", status));
            }
            let length = buffer
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(buffer.len());
            let command = String::from_utf16(&buffer[..length]).map_err(|error| {
                LaunchAtLoginError::Registry(format!(
                    "startup registration has invalid UTF-16: {error}"
                ))
            })?;
            return Ok(Some(command));
        }
    }

    fn read_binary_value(
        key_path: &str,
        value_name: &str,
    ) -> Result<Option<Vec<u8>>, LaunchAtLoginError> {
        let key_path = wide(key_path);
        let value_name = wide(value_name);
        let mut bytes = 0_u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                value_name.as_ptr(),
                RRF_RT_REG_BINARY,
                null_mut(),
                null_mut(),
                &mut bytes,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != 0 {
            return Err(registry_error("read startup approval", status));
        }

        let mut buffer = vec![0_u8; bytes as usize];
        loop {
            let mut buffer_bytes = u32::try_from(buffer.len()).map_err(|_| {
                LaunchAtLoginError::Registry("startup approval value is too large".into())
            })?;
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    key_path.as_ptr(),
                    value_name.as_ptr(),
                    RRF_RT_REG_BINARY,
                    null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut buffer_bytes,
                )
            };
            if status == ERROR_MORE_DATA {
                buffer.resize((buffer_bytes as usize).max(buffer.len() * 2), 0);
                continue;
            }
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if status != 0 {
                return Err(registry_error("read startup approval", status));
            }
            buffer.truncate(buffer_bytes as usize);
            return Ok(Some(buffer));
        }
    }

    fn write_string_value(
        key_path: &str,
        value_name: &str,
        command: &str,
    ) -> Result<(), LaunchAtLoginError> {
        let key_path = wide(key_path);
        let value_name = wide(value_name);
        let command = wide(command);
        let command_bytes = u32::try_from(command.len() * size_of::<u16>()).map_err(|_| {
            LaunchAtLoginError::Registry("startup registration command is too large".into())
        })?;
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                value_name.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                command_bytes,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(registry_error("write startup registration", status))
        }
    }

    fn write_binary_value(
        key_path: &str,
        value_name: &str,
        value: &[u8],
    ) -> Result<(), LaunchAtLoginError> {
        let key_path = wide(key_path);
        let value_name = wide(value_name);
        let value_bytes = u32::try_from(value.len()).map_err(|_| {
            LaunchAtLoginError::Registry("startup approval value is too large".into())
        })?;
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                value_name.as_ptr(),
                REG_BINARY,
                value.as_ptr().cast(),
                value_bytes,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(registry_error("write startup approval", status))
        }
    }

    fn delete_value(key_path: &str, value_name: &str) -> Result<(), LaunchAtLoginError> {
        let key_path = wide(key_path);
        let value_name = wide(value_name);
        let status = unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key_path.as_ptr(), value_name.as_ptr())
        };
        if status == 0 || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(registry_error("remove startup registration", status))
        }
    }

    fn registry_error(operation: &str, status: u32) -> LaunchAtLoginError {
        LaunchAtLoginError::Registry(format!(
            "{operation}: {} (code {status})",
            std::io::Error::from_raw_os_error(status as i32)
        ))
    }

    fn wide(value: &str) -> Vec<u16> {
        Path::new(value)
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Cleanup {
            run_value_name: String,
            startup_approved_value_name: String,
        }

        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_value(RUN_KEY, &self.run_value_name);
                let _ = delete_value(STARTUP_APPROVED_RUN_KEY, &self.startup_approved_value_name);
            }
        }

        fn unique_value_name() -> String {
            format!(
                "KeyForge-CodexTest-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )
        }

        #[test]
        fn run_value_round_trips_with_a_unique_test_value() {
            let value_name = unique_value_name();
            let _cleanup = Cleanup {
                run_value_name: value_name.clone(),
                startup_approved_value_name: value_name.clone(),
            };
            let command = "\"C:\\Users\\Key Forge\\KeyForge.exe\"";

            write_string_value(RUN_KEY, &value_name, command).unwrap();
            assert_eq!(
                read_string_value(RUN_KEY, &value_name).unwrap().as_deref(),
                Some(command)
            );
            delete_value(RUN_KEY, &value_name).unwrap();
            assert_eq!(read_string_value(RUN_KEY, &value_name).unwrap(), None);
        }

        #[test]
        fn startup_approved_value_round_trips_with_a_unique_test_value() {
            let value_name = unique_value_name();
            let _cleanup = Cleanup {
                run_value_name: value_name.clone(),
                startup_approved_value_name: value_name.clone(),
            };
            let approval = vec![0x03, 0x00, 0x00, 0x00, 0x12, 0x34, 0x56, 0x78];

            write_binary_value(STARTUP_APPROVED_RUN_KEY, &value_name, &approval).unwrap();
            assert_eq!(
                read_binary_value(STARTUP_APPROVED_RUN_KEY, &value_name).unwrap(),
                Some(approval.clone())
            );
            delete_value(STARTUP_APPROVED_RUN_KEY, &value_name).unwrap();
            assert_eq!(
                read_binary_value(STARTUP_APPROVED_RUN_KEY, &value_name).unwrap(),
                None
            );
        }

        #[test]
        fn enabling_launch_at_login_marks_startup_as_explicitly_enabled() {
            let value_name = unique_value_name();
            let _cleanup = Cleanup {
                run_value_name: value_name.clone(),
                startup_approved_value_name: value_name.clone(),
            };
            let command = "\"C:\\Program Files\\KeyForge\\KeyForge.exe\"";
            let disabled = vec![0x03, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD];

            write_binary_value(STARTUP_APPROVED_RUN_KEY, &value_name, &disabled).unwrap();
            set_enabled_for_value_names(&value_name, &value_name, true, Some(command)).unwrap();

            assert_eq!(
                read_string_value(RUN_KEY, &value_name).unwrap().as_deref(),
                Some(command)
            );
            assert_eq!(
                read_binary_value(STARTUP_APPROVED_RUN_KEY, &value_name).unwrap(),
                Some(STARTUP_APPROVED_ENABLED.to_vec())
            );
        }

        #[test]
        fn rejects_a_command_that_exceeds_the_windows_run_limit() {
            let path = std::path::PathBuf::from(format!("C:\\{}", "x".repeat(300)));
            assert!(matches!(
                command_for_executable(&path),
                Err(LaunchAtLoginError::CommandTooLong { maximum: 260, .. })
            ));
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{LaunchAtLoginError, LaunchAtLoginRegistration};

    pub(super) fn snapshot() -> Result<LaunchAtLoginRegistration, LaunchAtLoginError> {
        Err(LaunchAtLoginError::Unsupported)
    }

    pub(super) fn set_enabled(_: bool) -> Result<(), LaunchAtLoginError> {
        Err(LaunchAtLoginError::Unsupported)
    }

    pub(super) fn restore(_: &LaunchAtLoginRegistration) -> Result<(), LaunchAtLoginError> {
        Err(LaunchAtLoginError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::quote_windows_command_argument;

    #[test]
    fn quotes_spaces_trailing_backslashes_and_quotes_for_windows() {
        assert_eq!(
            quote_windows_command_argument(r"C:\\Program Files\\KeyForge.exe"),
            r#""C:\\Program Files\\KeyForge.exe""#
        );
        assert_eq!(
            quote_windows_command_argument(r"C:\\portable\\"),
            r#""C:\\portable\\\\""#
        );
        assert_eq!(
            quote_windows_command_argument(r#"C:\\name\"quoted\".exe"#),
            r#""C:\\name\\\"quoted\\\".exe""#
        );
    }

    #[test]
    fn preserves_unicode_paths_when_quoting() {
        assert_eq!(
            quote_windows_command_argument(r"C:\\사용자\\키포지.exe"),
            r#""C:\\사용자\\키포지.exe""#
        );
    }
}
