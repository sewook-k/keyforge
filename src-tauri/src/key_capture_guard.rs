//! Defense-in-depth native guard used while the UI is listening for a chord.
//!
//! WebView keyboard events can prevent browser defaults, but Windows still
//! handles a few system accelerators outside the DOM. The low-level capture
//! queue is the primary input path. This guard also covers same-thread WebView
//! child windows and reconstructs `Alt+Space` if Windows has already translated
//! it into a top-level system-menu command.

#[cfg(windows)]
use keyforge_daemon::{force_end_active_capture, record_window_system_key};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[cfg(windows)]
use tauri::{AppHandle, Manager, Runtime};

#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LMENU, VK_RMENU},
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::{
            EnumChildWindows, GetWindowThreadProcessId, SC_KEYMENU, WM_ACTIVATEAPP, WM_CONTEXTMENU,
            WM_MENUCHAR, WM_SYSCHAR, WM_SYSCOMMAND, WM_SYSDEADCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

#[cfg(windows)]
const CAPTURE_GUARD_SUBCLASS_ID: usize = 0x4B46_4347; // "KFCG"

#[cfg(windows)]
static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_ALT_SIDE: AtomicU8 = AtomicU8::new(0);

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AltSide {
    Left = 1,
    Right = 2,
}

#[cfg(windows)]
impl AltSide {
    fn from_message(message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<Self> {
        if !matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP) {
            return None;
        }
        let virtual_key = wparam.0 as u32;
        let extended = lparam.0 as usize & (1 << 24) != 0;
        match virtual_key {
            key if key == u32::from(VK_LMENU.0) => Some(Self::Left),
            key if key == u32::from(VK_RMENU.0) => Some(Self::Right),
            0x12 => Some(if extended { Self::Right } else { Self::Left }),
            _ => None,
        }
    }

    fn from_cached(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Left),
            2 => Some(Self::Right),
            _ => None,
        }
    }
}

#[cfg(windows)]
fn remember_alt_side(message: u32, wparam: WPARAM, lparam: LPARAM) {
    if let Some(side) = AltSide::from_message(message, wparam, lparam) {
        LAST_ALT_SIDE.store(side as u8, Ordering::Release);
    }
}

#[cfg(windows)]
fn clear_remembered_alt_side() {
    LAST_ALT_SIDE.store(0, Ordering::Release);
}

#[cfg(windows)]
fn preferred_alt_side() -> AltSide {
    if let Some(side) = AltSide::from_cached(LAST_ALT_SIDE.load(Ordering::Acquire)) {
        return side;
    }
    let right_alt_down = unsafe { GetAsyncKeyState(VK_RMENU.0.into()) } < 0;
    let left_alt_down = unsafe { GetAsyncKeyState(VK_LMENU.0.into()) } < 0;
    if right_alt_down && !left_alt_down {
        AltSide::Right
    } else {
        AltSide::Left
    }
}

/// Installs the guard on the KeyForge top-level window and every descendant
/// owned by the same UI thread (including the WebView2 host window).
///
/// The subclass stays installed for the lifetime of the window and is gated
/// by an atomic flag.  This avoids changing a window procedure from a Tauri
/// command thread while still allowing the UI to enable the guard just before
/// opening the capture dialog.
#[cfg(windows)]
pub fn install<R: Runtime, M: Manager<R>>(
    app: &M,
    window_label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let window = app.get_webview_window(window_label).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "KeyForge main window was not available for keyboard capture setup",
        )
    })?;
    let hwnd = window.hwnd()?;
    let installed = unsafe { install_window_tree(hwnd) };
    if installed.top_level {
        Ok(())
    } else {
        Err(Box::new(std::io::Error::last_os_error()))
    }
}

/// Enables capture immediately, then refreshes descendant subclasses on the
/// owning UI thread. WebView2 may recreate child HWNDs after navigation, so a
/// setup-only enumeration is not sufficient.
#[cfg(windows)]
pub fn activate<R: Runtime>(
    app: &AppHandle<R>,
    window_label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let window = app.get_webview_window(window_label).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "KeyForge main window was not available for keyboard capture activation",
        )
    })?;
    let hwnd_value = window.hwnd()?.0 as isize;
    CAPTURE_ACTIVE.store(true, Ordering::Release);
    clear_remembered_alt_side();
    window.run_on_main_thread(move || unsafe {
        let _ = install_window_tree(HWND(hwnd_value as *mut _));
    })?;
    Ok(())
}

#[cfg(windows)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct InstallResult {
    top_level: bool,
    descendants: usize,
}

#[cfg(windows)]
unsafe fn install_window_tree(hwnd: HWND) -> InstallResult {
    let mut result = InstallResult {
        top_level: unsafe {
            SetWindowSubclass(
                hwnd,
                Some(capture_guard_subclass_proc),
                CAPTURE_GUARD_SUBCLASS_ID,
                0,
            )
            .as_bool()
        },
        descendants: 0,
    };
    let owner_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
    let mut context = DescendantInstallContext {
        owner_thread,
        installed: &mut result.descendants,
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(install_descendant_subclass),
            LPARAM((&raw mut context) as isize),
        );
    }
    result
}

#[cfg(windows)]
struct DescendantInstallContext<'a> {
    owner_thread: u32,
    installed: &'a mut usize,
}

#[cfg(windows)]
unsafe extern "system" fn install_descendant_subclass(
    hwnd: HWND,
    lparam: LPARAM,
) -> windows::core::BOOL {
    let context = unsafe { &mut *(lparam.0 as *mut DescendantInstallContext<'_>) };
    // SetWindowSubclass cannot cross thread boundaries. WebView2 renderer
    // process windows are deliberately skipped; the same-thread host HWND is
    // enough to intercept messages routed through the app UI thread.
    if unsafe { GetWindowThreadProcessId(hwnd, None) } == context.owner_thread
        && unsafe {
            SetWindowSubclass(
                hwnd,
                Some(capture_guard_subclass_proc),
                CAPTURE_GUARD_SUBCLASS_ID,
                0,
            )
            .as_bool()
        }
    {
        *context.installed += 1;
    }
    true.into()
}

/// Enables or disables the native window-menu guard.
#[cfg(windows)]
pub fn set_active(active: bool) {
    CAPTURE_ACTIVE.store(active, Ordering::Release);
    if !active {
        clear_remembered_alt_side();
    }
}

#[cfg(windows)]
unsafe extern "system" fn capture_guard_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if should_end_capture_for_app_deactivation(message, wparam) {
        CAPTURE_ACTIVE.store(false, Ordering::Release);
        clear_remembered_alt_side();
        force_end_active_capture();
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    }

    remember_alt_side(message, wparam, lparam);
    let capture_active = CAPTURE_ACTIVE.load(Ordering::Acquire);
    if is_alt_space_system_command(capture_active, message, wparam, lparam) {
        record_alt_space_fallback();
        return LRESULT(0);
    }
    if should_capture_system_key(capture_active, message) {
        let message_bits = lparam.0 as usize;
        let scan_code = ((message_bits >> 16) & 0xFF) as u32;
        let extended = message_bits & (1 << 24) != 0;
        let _ = record_window_system_key(
            wparam.0 as u32,
            scan_code,
            extended,
            message == WM_SYSKEYDOWN,
        );
        // Returning zero prevents Tao/DefWindowProc from turning Alt+Space
        // into a system-menu command. The event was written to the same
        // bounded native queue that the renderer drains.
        return LRESULT(0);
    }
    if should_swallow_message(capture_active, message) {
        return LRESULT(0);
    }

    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// When the low-level hook is unavailable, WebView2 can let DefWindowProc
/// translate Alt+Space into `WM_SYSCOMMAND(SC_KEYMENU, ' ')`. Recover the
/// physical chord from that final, reliable top-level message before consuming
/// it. Duplicate key-down events are harmless because the renderer stores held
/// keys in a set.
#[cfg(windows)]
fn is_alt_space_system_command(
    capture_active: bool,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> bool {
    capture_active
        && message == WM_SYSCOMMAND
        && (wparam.0 as u32 & 0xFFF0) == SC_KEYMENU
        && (lparam.0 as u32 & 0xFFFF) == 0x20
}

#[cfg(windows)]
fn record_alt_space_fallback() {
    let (alt_vk, alt_scan, alt_extended) = match preferred_alt_side() {
        AltSide::Right => (u32::from(VK_RMENU.0), 0x38, true),
        AltSide::Left => (u32::from(VK_LMENU.0), 0x38, false),
    };
    clear_remembered_alt_side();
    let _ = record_window_system_key(alt_vk, alt_scan, alt_extended, true);
    let _ = record_window_system_key(0x20, 0x39, false, true);
    let _ = record_window_system_key(0x20, 0x39, false, false);
    let _ = record_window_system_key(alt_vk, alt_scan, alt_extended, false);
}

/// Captures system key transitions before Tao/DefWindowProc can process a
/// native accelerator such as Alt+Space.
#[cfg(windows)]
fn should_capture_system_key(capture_active: bool, message: u32) -> bool {
    capture_active && matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP)
}

/// `WM_ACTIVATEAPP(FALSE)` is a top-level application deactivation, unlike a
/// WebView2 child `Focused(false)` notification. It is safe to use as the
/// final guard against swallowing keys after the user switches applications.
#[cfg(windows)]
fn should_end_capture_for_app_deactivation(message: u32, wparam: WPARAM) -> bool {
    message == WM_ACTIVATEAPP && wparam.0 == 0
}

/// Consumes late system-menu messages while a native capture session is active.
#[cfg(windows)]
fn should_swallow_message(capture_active: bool, message: u32) -> bool {
    capture_active
        && matches!(
            message,
            WM_SYSCOMMAND | WM_SYSCHAR | WM_SYSDEADCHAR | WM_MENUCHAR | WM_CONTEXTMENU
        )
}

#[cfg(not(windows))]
pub fn install<R: tauri::Runtime, M: tauri::Manager<R>>(
    _app: &M,
    _window_label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(not(windows))]
pub fn activate<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _window_label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(not(windows))]
pub fn set_active(_active: bool) {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{WM_ACTIVATEAPP, WM_SYSKEYDOWN, WM_SYSKEYUP};

    #[test]
    fn capture_blocks_and_records_system_key_messages_while_active() {
        assert!(should_capture_system_key(true, WM_SYSKEYDOWN));
        assert!(should_capture_system_key(true, WM_SYSKEYUP));
        assert!(!should_capture_system_key(false, WM_SYSKEYDOWN));
        assert!(should_end_capture_for_app_deactivation(
            WM_ACTIVATEAPP,
            WPARAM(0)
        ));
        assert!(!should_end_capture_for_app_deactivation(
            WM_ACTIVATEAPP,
            WPARAM(1)
        ));
        assert!(should_swallow_message(true, WM_SYSCOMMAND));
        assert!(should_swallow_message(true, WM_SYSCHAR));
        assert!(should_swallow_message(true, WM_MENUCHAR));
        assert!(should_swallow_message(true, WM_CONTEXTMENU));
        assert!(!should_swallow_message(true, WM_SYSKEYDOWN));
        assert!(!should_swallow_message(false, WM_SYSCOMMAND));
        assert!(is_alt_space_system_command(
            true,
            WM_SYSCOMMAND,
            WPARAM(SC_KEYMENU as usize),
            LPARAM(0x20)
        ));
        assert!(!is_alt_space_system_command(
            false,
            WM_SYSCOMMAND,
            WPARAM(SC_KEYMENU as usize),
            LPARAM(0x20)
        ));
        assert!(!is_alt_space_system_command(
            true,
            WM_SYSCOMMAND,
            WPARAM(SC_KEYMENU as usize),
            LPARAM('f' as isize)
        ));
    }

    #[test]
    fn remembers_left_and_right_alt_side_from_system_messages() {
        clear_remembered_alt_side();
        remember_alt_side(WM_SYSKEYDOWN, WPARAM(0x12), LPARAM((0x38 << 16) as isize));
        assert_eq!(preferred_alt_side(), AltSide::Left);

        clear_remembered_alt_side();
        remember_alt_side(
            WM_SYSKEYDOWN,
            WPARAM(0x12),
            LPARAM(((0x38 << 16) | (1 << 24)) as isize),
        );
        assert_eq!(preferred_alt_side(), AltSide::Right);

        clear_remembered_alt_side();
    }
}
