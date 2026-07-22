use crate::pnp_metadata::{keyboard_metadata_by_path, normalize_device_path};
use crate::{
    DeviceInventoryError, KeyCaptureDrain, KeyCaptureEvent, KeyCaptureSession, KeyboardDeviceInfo,
};
use arc_swap::{ArcSwap, ArcSwapOption};
use keyforge_config::{Action, MouseButton};
use keyforge_engine::{
    CompiledRules, DispatchAction, DispatchActionPhase, EventOrigin, KeyEvent, KeyPhase,
    MatchContext, RuntimeEngine,
};
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    mem::{size_of, zeroed},
    ptr::null_mut,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::{
            GetRawInputDeviceInfoW, GetRawInputDeviceList,
            KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
                KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
                MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput,
            },
            RAWINPUTDEVICELIST, RID_DEVICE_INFO, RIDI_DEVICEINFO, RIDI_DEVICENAME,
            RIM_TYPEKEYBOARD,
        },
        WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
            PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
            WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
            WM_MBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
            WM_XBUTTONDOWN, WM_XBUTTONUP,
        },
    },
};

const LLKHF_INJECTED_FLAG: u32 = 0x10;
const LLKHF_EXTENDED_FLAG: u32 = 0x01;
const LLMHF_INJECTED_FLAG: u32 = 0x01;
const INPUT_MARKER: usize = 0x4B46_0001;
const XBUTTON1_VALUE: u16 = 1;
const XBUTTON2_VALUE: u16 = 2;
const CAPTURE_QUEUE_CAPACITY: usize = 256;
const WM_RELEASE_RUNTIME_FOR_CAPTURE: u32 = 0x8001;

pub fn list_connected_keyboards() -> Result<Vec<KeyboardDeviceInfo>, DeviceInventoryError> {
    let devices = unsafe { raw_input_device_list()? };
    // PnP metadata is optional enrichment. SetupAPI or one of its providers can
    // fail independently of Raw Input, so never make that failure hide a
    // connected input endpoint.
    let mut pnp_metadata = keyboard_metadata_by_path().unwrap_or_default();
    let mut keyboards = devices
        .into_iter()
        .filter(|device| device.dwType == RIM_TYPEKEYBOARD)
        .filter_map(|device| unsafe { keyboard_device_info(device).ok() })
        .collect::<Vec<_>>();
    for keyboard in &mut keyboards {
        let Some(metadata) = pnp_metadata.remove(&normalize_device_path(&keyboard.device_path))
        else {
            continue;
        };
        if let Some(display_name) = metadata.display_name {
            keyboard.name = display_name;
        }
        keyboard.manufacturer = metadata.manufacturer;
        keyboard.instance_id = metadata.instance_id;
        keyboard.container_id = metadata.container_id;
        keyboard.hardware_ids = metadata.hardware_ids;
        keyboard.location_paths = metadata.location_paths;
    }
    keyboards.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.device_path.cmp(&right.device_path))
    });
    keyboards.dedup_by(|left, right| left.device_path.eq_ignore_ascii_case(&right.device_path));
    Ok(keyboards)
}

unsafe fn raw_input_device_list() -> Result<Vec<RAWINPUTDEVICELIST>, DeviceInventoryError> {
    let item_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    for _ in 0..3 {
        let mut count = 0u32;
        let result = unsafe { GetRawInputDeviceList(null_mut(), &mut count, item_size) };
        if result == u32::MAX {
            return Err(DeviceInventoryError::Windows(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
        let result = unsafe { GetRawInputDeviceList(devices.as_mut_ptr(), &mut count, item_size) };
        if result != u32::MAX {
            devices.truncate(result as usize);
            return Ok(devices);
        }
    }
    Err(DeviceInventoryError::Windows(
        "the raw input device list changed repeatedly while it was being read".into(),
    ))
}

unsafe fn keyboard_device_info(
    device: RAWINPUTDEVICELIST,
) -> Result<KeyboardDeviceInfo, DeviceInventoryError> {
    let mut path_size = 0u32;
    let result = unsafe {
        GetRawInputDeviceInfoW(device.hDevice, RIDI_DEVICENAME, null_mut(), &mut path_size)
    };
    if result == u32::MAX || path_size == 0 {
        return Err(DeviceInventoryError::Windows(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let mut path_buffer = vec![0u16; path_size as usize + 1];
    let result = unsafe {
        GetRawInputDeviceInfoW(
            device.hDevice,
            RIDI_DEVICENAME,
            path_buffer.as_mut_ptr().cast(),
            &mut path_size,
        )
    };
    if result == u32::MAX {
        return Err(DeviceInventoryError::Windows(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let path_end = path_buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(result as usize);
    let device_path = String::from_utf16_lossy(&path_buffer[..path_end]);

    let mut info = RID_DEVICE_INFO {
        cbSize: size_of::<RID_DEVICE_INFO>() as u32,
        ..RID_DEVICE_INFO::default()
    };
    let mut info_size = size_of::<RID_DEVICE_INFO>() as u32;
    let info_result = unsafe {
        GetRawInputDeviceInfoW(
            device.hDevice,
            RIDI_DEVICEINFO,
            (&mut info as *mut RID_DEVICE_INFO).cast(),
            &mut info_size,
        )
    };
    let keyboard = if info_result == u32::MAX || info.dwType != RIM_TYPEKEYBOARD {
        Default::default()
    } else {
        unsafe { info.Anonymous.keyboard }
    };
    let vendor_id = path_component(&device_path, "VID_", 4);
    let product_id = path_component(&device_path, "PID_", 4);
    let interface_id = path_component(&device_path, "MI_", 2);
    let is_virtual = is_virtual_device_path(&device_path);
    Ok(KeyboardDeviceInfo {
        id: inventory_id(&device_path),
        name: keyboard_display_name(vendor_id.as_deref(), product_id.as_deref(), is_virtual),
        device_path,
        manufacturer: None,
        instance_id: None,
        container_id: None,
        hardware_ids: Vec::new(),
        location_paths: Vec::new(),
        vendor_id,
        product_id,
        interface_id,
        keyboard_type: keyboard.dwType,
        keyboard_sub_type: keyboard.dwSubType,
        keyboard_mode: keyboard.dwKeyboardMode,
        function_key_count: keyboard.dwNumberOfFunctionKeys,
        indicator_count: keyboard.dwNumberOfIndicators,
        total_key_count: keyboard.dwNumberOfKeysTotal,
        is_virtual,
        source: "raw_input".into(),
    })
}

fn path_component(path: &str, marker: &str, length: usize) -> Option<String> {
    let upper = path.to_ascii_uppercase();
    let start = upper.find(marker)? + marker.len();
    let value = upper.get(start..start + length)?;
    value
        .chars()
        .all(|character| character.is_ascii_hexdigit())
        .then(|| value.into())
}

fn is_virtual_device_path(path: &str) -> bool {
    let upper = path.to_ascii_uppercase();
    ["RDP_KBD", "ROOT#", "VIRTUAL", "VMWARE", "VMBUS"]
        .iter()
        .any(|marker| upper.contains(marker))
}

fn keyboard_display_name(
    vendor: Option<&str>,
    product: Option<&str>,
    virtual_device: bool,
) -> String {
    if virtual_device {
        return "가상 또는 원격 키보드".into();
    }
    match (vendor, product) {
        (Some(vendor), Some(product)) => format!("HID 키보드 · VID_{vendor} / PID_{product}"),
        _ => "Windows 키보드".into(),
    }
}

fn inventory_id(path: &str) -> String {
    let hash = path
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("rawkbd-{hash:016x}")
}

#[derive(Debug, Error, Clone)]
pub enum HookError {
    #[error("another KeyForge hook service is already registered in this process")]
    AlreadyStarted,
    #[error("failed to install Windows input hook: {0}")]
    Install(String),
    #[error("hook thread stopped before initialization")]
    ThreadStopped,
    #[error("the KeyForge capture window is unavailable")]
    CaptureWindowUnavailable,
    #[error("the Windows input hook is unavailable")]
    CaptureUnavailable,
}

struct CaptureState {
    active: AtomicBool,
    window_configured: AtomicBool,
    session_id: AtomicU64,
    overflowed: AtomicBool,
    release_pending: AtomicBool,
    sender: mpsc::SyncSender<KeyCaptureEvent>,
    receiver: Mutex<mpsc::Receiver<KeyCaptureEvent>>,
}

impl CaptureState {
    fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
        Self {
            active: AtomicBool::new(false),
            window_configured: AtomicBool::new(false),
            session_id: AtomicU64::new(0),
            overflowed: AtomicBool::new(false),
            release_pending: AtomicBool::new(false),
            sender,
            receiver: Mutex::new(receiver),
        }
    }

    fn set_owner_window(&self, hwnd: isize) {
        self.window_configured.store(hwnd != 0, Ordering::Release);
    }

    fn begin(&self) -> Result<KeyCaptureSession, HookError> {
        if !self.window_configured.load(Ordering::Acquire) {
            return Err(HookError::CaptureWindowUnavailable);
        }

        self.force_end();
        let session_id = self.session_id.fetch_add(1, Ordering::AcqRel) + 1;
        self.overflowed.store(false, Ordering::Release);
        self.release_pending.store(true, Ordering::Release);
        self.active.store(true, Ordering::Release);
        Ok(KeyCaptureSession { session_id })
    }

    fn end(&self, session_id: u64) -> bool {
        if self.session_id.load(Ordering::Acquire) != session_id {
            return false;
        }
        self.force_end();
        true
    }

    fn force_end(&self) {
        self.active.store(false, Ordering::Release);
        self.release_pending.store(false, Ordering::Release);
        self.overflowed.store(false, Ordering::Release);
        self.clear_events();
    }

    fn drain(&self, session_id: u64) -> KeyCaptureDrain {
        let current_session = self.session_id.load(Ordering::Acquire);
        if current_session != session_id {
            // A delayed poll from an old modal must never consume a newer
            // session's queue. Returning before locking keeps the current
            // session's physical events intact.
            return KeyCaptureDrain {
                session_id: current_session,
                active: false,
                overflowed: false,
                events: Vec::new(),
            };
        }
        let active = self.active.load(Ordering::Acquire);
        let overflowed = if current_session == session_id {
            self.overflowed.swap(false, Ordering::AcqRel)
        } else {
            false
        };
        let events = self
            .receiver
            .lock()
            .try_iter()
            .filter(|event| event.session_id == session_id)
            .collect();
        KeyCaptureDrain {
            session_id: current_session,
            active,
            overflowed,
            events,
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn take_release_pending(&self) -> bool {
        self.release_pending.swap(false, Ordering::AcqRel)
    }

    fn enqueue(&self, key: String, phase: KeyPhase) -> bool {
        let event = KeyCaptureEvent {
            session_id: self.session_id.load(Ordering::Acquire),
            key,
            phase,
        };
        match self.sender.try_send(event) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                self.overflowed.store(true, Ordering::Release);
                true
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.active.store(false, Ordering::Release);
                false
            }
        }
    }

    fn enqueue_window_system_key(
        &self,
        virtual_key: u32,
        scan_code: u32,
        extended: bool,
        is_key_down: bool,
    ) -> bool {
        if !self.active.load(Ordering::Acquire) {
            return false;
        }
        let flags = if extended { LLKHF_EXTENDED_FLAG } else { 0 };
        let phase = if is_key_down {
            KeyPhase::Down
        } else {
            KeyPhase::Up
        };
        self.enqueue(vk_name(virtual_key, scan_code, flags), phase)
    }

    fn clear_events(&self) {
        let receiver = self.receiver.lock();
        while receiver.try_recv().is_ok() {}
    }
}

/// Records a system-key message received by the KeyForge top-level window.
///
/// This is a fallback for cases where WebView2/Tao routes `WM_SYSKEYDOWN`
/// directly to the native window before the low-level hook path can provide a
/// renderer event. The window itself is the target, so it is safe to use the
/// active session without a foreground-HWND comparison. `WM_*` messages do not
/// carry low-level injection provenance, so this path cannot distinguish
/// physical input from injected input and must not be treated as authoritative.
pub fn record_window_system_key(
    virtual_key: u32,
    scan_code: u32,
    extended: bool,
    is_key_down: bool,
) -> bool {
    let Some(shared) = SHARED.load_full() else {
        return false;
    };
    shared
        .capture
        .enqueue_window_system_key(virtual_key, scan_code, extended, is_key_down)
}

/// Ends the active capture from a reliable top-level activation lifecycle
/// message. This is intentionally separate from WebView2 child focus events.
pub fn force_end_active_capture() {
    if let Some(shared) = SHARED.load_full() {
        shared.capture.force_end();
    }
}

struct Shared {
    rules: ArcSwap<CompiledRules>,
    paused: AtomicBool,
    installed: AtomicBool,
    thread_id: AtomicU32,
    capture: CaptureState,
}

static SHARED: LazyLock<ArcSwapOption<Shared>> = LazyLock::new(ArcSwapOption::empty);
static LIFECYCLE: LazyLock<parking_lot::Mutex<()>> = LazyLock::new(|| parking_lot::Mutex::new(()));
thread_local! {
    static RUNTIME: RefCell<Option<RuntimeEngine>> = const { RefCell::new(None) };
}

pub struct HookService {
    shared: Arc<Shared>,
    join: Option<JoinHandle<()>>,
}

impl HookService {
    pub fn start(rules: CompiledRules) -> Result<Self, HookError> {
        let _lifecycle = LIFECYCLE.lock();
        if SHARED.load_full().is_some() {
            return Err(HookError::AlreadyStarted);
        }
        let shared = Arc::new(Shared {
            rules: ArcSwap::from_pointee(rules),
            paused: AtomicBool::new(false),
            installed: AtomicBool::new(false),
            thread_id: AtomicU32::new(0),
            capture: CaptureState::new(),
        });
        SHARED.store(Some(shared.clone()));

        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread_shared = shared.clone();
        let join = match thread::Builder::new()
            .name("keyforge-input-hook".into())
            .spawn(move || {
                let result = unsafe { run_hook_loop(&thread_shared) };
                let _ = ready_tx.send(result.clone());
                if result.is_err() {
                    return;
                }
                unsafe { message_loop(&thread_shared) };
            }) {
            Ok(join) => join,
            Err(error) => {
                SHARED.store(None);
                return Err(HookError::Install(error.to_string()));
            }
        };

        let ready = ready_rx.recv().map_err(|_| HookError::ThreadStopped);
        match ready {
            Ok(Ok(())) => Ok(Self {
                shared,
                join: Some(join),
            }),
            Ok(Err(error)) => {
                SHARED.store(None);
                let _ = join.join();
                Err(error)
            }
            Err(error) => {
                SHARED.store(None);
                let _ = join.join();
                Err(error)
            }
        }
    }

    pub fn update_rules(&self, rules: CompiledRules) {
        self.shared.rules.store(Arc::new(rules));
    }
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Release);
    }
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Acquire)
    }
    pub fn is_installed(&self) -> bool {
        self.shared.installed.load(Ordering::Acquire)
    }

    pub fn set_key_capture_window(&self, hwnd: isize) {
        self.shared.capture.set_owner_window(hwnd);
    }

    pub fn begin_key_capture(&self) -> Result<KeyCaptureSession, HookError> {
        if !self.is_installed() {
            return Err(HookError::CaptureUnavailable);
        }
        let session = self.shared.capture.begin()?;
        // The runtime is hook-thread-local, so release its held output there
        // before the user supplies the first capture key.
        let id = self.shared.thread_id.load(Ordering::Acquire);
        if id != 0 {
            unsafe {
                PostThreadMessageW(id, WM_RELEASE_RUNTIME_FOR_CAPTURE, 0, 0);
            }
        }
        Ok(session)
    }

    pub fn end_key_capture(&self, session_id: u64) -> bool {
        self.shared.capture.end(session_id)
    }

    pub fn force_end_key_capture(&self) {
        self.shared.capture.force_end();
    }

    pub fn drain_key_capture_events(&self, session_id: u64) -> KeyCaptureDrain {
        self.shared.capture.drain(session_id)
    }

    pub fn stop(&mut self) {
        let _lifecycle = LIFECYCLE.lock();
        if self.join.is_none() {
            return;
        }
        self.force_end_key_capture();
        let id = self.shared.thread_id.load(Ordering::Acquire);
        if id != 0 {
            unsafe {
                PostThreadMessageW(id, WM_QUIT, 0, 0);
            }
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        if SHARED
            .load_full()
            .is_some_and(|current| Arc::ptr_eq(&current, &self.shared))
        {
            SHARED.store(None);
        }
    }
}

impl Drop for HookService {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn run_hook_loop(shared: &Arc<Shared>) -> Result<(), HookError> {
    shared
        .thread_id
        .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    let module = unsafe { GetModuleHandleW(std::ptr::null()) } as HINSTANCE;
    let keyboard = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
    if keyboard.is_null() {
        return Err(HookError::Install(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
    if mouse.is_null() {
        unsafe {
            UnhookWindowsHookEx(keyboard);
        }
        return Err(HookError::Install(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    HOOKS.with(|hooks| *hooks.borrow_mut() = Some((keyboard, mouse)));
    shared.installed.store(true, Ordering::Release);
    Ok(())
}

thread_local! {
    static HOOKS: RefCell<Option<(*mut core::ffi::c_void, *mut core::ffi::c_void)>> = const { RefCell::new(None) };
}

unsafe fn message_loop(shared: &Arc<Shared>) {
    let mut message: MSG = unsafe { zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        if message.message == WM_RELEASE_RUNTIME_FOR_CAPTURE && shared.capture.is_active() {
            release_pending_runtime_outputs_for_capture(&shared.capture, inject_action);
        }
    }
    release_runtime_outputs_for_shutdown(inject_action);
    HOOKS.with(|hooks| {
        if let Some((keyboard, mouse)) = hooks.borrow_mut().take() {
            unsafe {
                UnhookWindowsHookEx(keyboard);
                UnhookWindowsHookEx(mouse);
            }
        }
    });
    shared.installed.store(false, Ordering::Release);
}

fn release_runtime_outputs_for_shutdown(mut inject: impl FnMut(&DispatchAction) -> bool) -> usize {
    let releases = RUNTIME.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        let releases = runtime
            .as_mut()
            .map(|engine| engine.set_paused(true))
            .unwrap_or_default();
        *runtime = None;
        releases
    });
    let release_count = releases.len();
    for action in &releases {
        let _ = inject(action);
    }
    release_count
}

fn release_runtime_outputs_for_capture(mut inject: impl FnMut(&DispatchAction) -> bool) {
    let releases = RUNTIME.with(|runtime| {
        runtime
            .borrow_mut()
            .as_mut()
            .map(|engine| engine.set_paused(true))
            .unwrap_or_default()
    });
    for action in &releases {
        let _ = inject(action);
    }
}
fn release_pending_runtime_outputs_for_capture(
    capture: &CaptureState,
    inject: impl FnMut(&DispatchAction) -> bool,
) -> bool {
    if !capture.take_release_pending() {
        return false;
    }
    release_runtime_outputs_for_capture(inject);
    true
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let data = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let origin = if data.flags & LLKHF_INJECTED_FLAG != 0 {
        if data.dwExtraInfo == INPUT_MARKER {
            EventOrigin::InjectedSelf
        } else {
            EventOrigin::InjectedOther
        }
    } else {
        EventOrigin::Physical
    };
    if origin != EventOrigin::Physical {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let phase = match wparam as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => KeyPhase::Down,
        WM_KEYUP | WM_SYSKEYUP => KeyPhase::Up,
        _ => return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) },
    };
    let key = vk_name(data.vkCode, data.scanCode, data.flags);
    if let Some(shared) = SHARED.load_full()
        && shared.capture.is_active()
        && shared.capture.enqueue(key.clone(), phase)
    {
        // A physical key can win the race with the posted release request.
        // Its event is already queued, so it remains intact while we release.
        release_pending_runtime_outputs_for_capture(&shared.capture, inject_action);
        return 1;
    }
    let event = KeyEvent {
        key,
        phase,
        origin,
        repeat: false,
    };
    if process_event(event) {
        1
    } else {
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let data = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
    let origin = if data.flags & LLMHF_INJECTED_FLAG != 0 {
        if data.dwExtraInfo == INPUT_MARKER {
            EventOrigin::InjectedSelf
        } else {
            EventOrigin::InjectedOther
        }
    } else {
        EventOrigin::Physical
    };
    if origin != EventOrigin::Physical {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let (key, phase) = match wparam as u32 {
        WM_LBUTTONDOWN => ("mouseleft", KeyPhase::Down),
        WM_LBUTTONUP => ("mouseleft", KeyPhase::Up),
        WM_RBUTTONDOWN => ("mouseright", KeyPhase::Down),
        WM_RBUTTONUP => ("mouseright", KeyPhase::Up),
        WM_MBUTTONDOWN => ("mousemiddle", KeyPhase::Down),
        WM_MBUTTONUP => ("mousemiddle", KeyPhase::Up),
        WM_XBUTTONDOWN => (
            if (data.mouseData >> 16) as u16 == XBUTTON1_VALUE {
                "mousex1"
            } else {
                "mousex2"
            },
            KeyPhase::Down,
        ),
        WM_XBUTTONUP => (
            if (data.mouseData >> 16) as u16 == XBUTTON1_VALUE {
                "mousex1"
            } else {
                "mousex2"
            },
            KeyPhase::Up,
        ),
        _ => return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) },
    };
    if process_event(KeyEvent {
        key: key.into(),
        phase,
        origin,
        repeat: false,
    }) {
        1
    } else {
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }
}

fn process_event(event: KeyEvent) -> bool {
    process_event_with_injector(event, inject_action)
}

fn process_event_with_injector(
    event: KeyEvent,
    mut inject: impl FnMut(&DispatchAction) -> bool,
) -> bool {
    if event.origin != EventOrigin::Physical {
        return false;
    }
    let Some(shared) = SHARED.load_full() else {
        return false;
    };
    let snapshot = shared.rules.load_full();
    let (pause_releases, dispatch) = RUNTIME.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        let engine = runtime.get_or_insert_with(|| RuntimeEngine::new((*snapshot).clone()));
        if engine.revision() != snapshot.revision() {
            engine.replace_rules((*snapshot).clone());
        }
        let pause_releases = engine.set_paused(shared.paused.load(Ordering::Acquire));
        let dispatch = engine.process(&event, &MatchContext::default());
        (pause_releases, dispatch)
    });

    // SendInput can synchronously invoke this low-level hook again. Never keep
    // the RefCell-backed runtime mutably borrowed while injecting output.
    for action in &pause_releases {
        let _ = inject(action);
    }
    let mut injected = true;
    for action in &dispatch.actions {
        injected &= inject(action);
    }
    if dispatch.emergency_stop {
        shared.paused.store(true, Ordering::Release);
        return true;
    }
    let releases_held_output = dispatch
        .actions
        .iter()
        .any(|action| action.phase == DispatchActionPhase::Up);
    if dispatch.suppress_original
        && !dispatch.actions.is_empty()
        && !injected
        && event.phase == KeyPhase::Down
    {
        RUNTIME.with(|runtime| {
            if let Some(engine) = runtime.borrow_mut().as_mut() {
                engine.cancel_consumed(&event.key);
            }
        });
    }
    dispatch.suppress_original && (dispatch.actions.is_empty() || injected || releases_held_output)
}

fn inject_action(dispatch: &DispatchAction) -> bool {
    match (&dispatch.action, dispatch.phase) {
        (Action::SendKeys { chord }, phase) => inject_keys(chord, phase),
        (Action::SendMouse { button }, DispatchActionPhase::Invoke) => inject_mouse(*button),
        _ => false,
    }
}

fn inject_keys(chord: &[String], phase: DispatchActionPhase) -> bool {
    let Some(keys) = chord
        .iter()
        .map(|key| name_to_key_spec(key))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let planned = plan_key_events(&keys, phase);
    let inputs: Vec<_> = planned
        .iter()
        .map(|&(key, flags)| key_input(key, flags))
        .collect();
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent == inputs.len() as u32 {
        true
    } else {
        let cleanup: Vec<_> = keys
            .iter()
            .rev()
            .map(|&key| key_input(key, KEYEVENTF_KEYUP))
            .collect();
        unsafe {
            SendInput(
                cleanup.len() as u32,
                cleanup.as_ptr(),
                size_of::<INPUT>() as i32,
            );
        }
        false
    }
}

fn plan_key_events(keys: &[KeySpec], phase: DispatchActionPhase) -> Vec<(KeySpec, u32)> {
    match phase {
        DispatchActionPhase::Invoke => keys
            .iter()
            .copied()
            .map(|key| (key, 0))
            .chain(keys.iter().rev().copied().map(|key| (key, KEYEVENTF_KEYUP)))
            .collect(),
        DispatchActionPhase::Down => keys.iter().copied().map(|key| (key, 0)).collect(),
        DispatchActionPhase::Up => keys
            .iter()
            .rev()
            .copied()
            .map(|key| (key, KEYEVENTF_KEYUP))
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeySpec {
    vk: u16,
    scan: u16,
    flags: u32,
}

impl KeySpec {
    const fn vk(vk: u16) -> Self {
        Self {
            vk,
            scan: 0,
            flags: 0,
        }
    }

    const fn extended(vk: u16) -> Self {
        Self {
            vk,
            scan: 0,
            flags: KEYEVENTF_EXTENDEDKEY,
        }
    }

    const fn scan(scan: u16) -> Self {
        Self {
            vk: 0,
            scan,
            flags: KEYEVENTF_SCANCODE,
        }
    }
}

fn key_input(key: KeySpec, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key.vk,
                wScan: key.scan,
                dwFlags: key.flags | flags,
                time: 0,
                dwExtraInfo: INPUT_MARKER,
            },
        },
    }
}

fn inject_mouse(button: MouseButton) -> bool {
    let (down, up, data) = match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
        MouseButton::X1 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON1_VALUE as u32),
        MouseButton::X2 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON2_VALUE as u32),
    };
    let inputs = [mouse_input(down, data), mouse_input(up, data)];
    let sent = unsafe { SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent == 2 {
        true
    } else {
        let cleanup = mouse_input(up, data);
        unsafe {
            SendInput(1, &cleanup, size_of::<INPUT>() as i32);
        }
        false
    }
}

fn mouse_input(flags: u32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INPUT_MARKER,
            },
        },
    }
}

fn vk_name(vk: u32, scan_code: u32, flags: u32) -> String {
    let extended = flags & LLKHF_EXTENDED_FLAG != 0;
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D if extended => "NumpadEnter".into(),
        0x0D => "Enter".into(),
        0x10 if scan_code == 0x36 => "ShiftRight".into(),
        0x10 => "ShiftLeft".into(),
        0x11 if extended => "ControlRight".into(),
        0x11 => "ControlLeft".into(),
        0x12 if extended => "AltRight".into(),
        0x12 => "AltLeft".into(),
        0x13 => "Pause".into(),
        0x14 => "CapsLock".into(),
        0x15 => "Lang1".into(),
        0x17 => "Lang3".into(),
        0x18 => "Lang4".into(),
        0x19 => "Lang2".into(),
        0x1A => "Lang5".into(),
        0x1B => "Escape".into(),
        0x1C => "Convert".into(),
        0x1D => "NonConvert".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "ArrowLeft".into(),
        0x26 => "ArrowUp".into(),
        0x27 => "ArrowRight".into(),
        0x28 => "ArrowDown".into(),
        0x2C => "PrintScreen".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).unwrap().to_string(),
        0x5B => "MetaLeft".into(),
        0x5C => "MetaRight".into(),
        0x5D => "ContextMenu".into(),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x6A => "NumpadMultiply".into(),
        0x6B => "NumpadAdd".into(),
        0x6C => "NumpadComma".into(),
        0x6D => "NumpadSubtract".into(),
        0x6E => "NumpadDecimal".into(),
        0x6F => "NumpadDivide".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x90 => "NumLock".into(),
        0x91 => "ScrollLock".into(),
        0x92 => "NumpadEqual".into(),
        0xA0 => "ShiftLeft".into(),
        0xA1 => "ShiftRight".into(),
        0xA2 => "ControlLeft".into(),
        0xA3 => "ControlRight".into(),
        0xA4 => "AltLeft".into(),
        0xA5 => "AltRight".into(),
        0xA6 => "BrowserBack".into(),
        0xA7 => "BrowserForward".into(),
        0xA8 => "BrowserRefresh".into(),
        0xA9 => "BrowserStop".into(),
        0xAA => "BrowserSearch".into(),
        0xAB => "BrowserFavorites".into(),
        0xAC => "BrowserHome".into(),
        0xAD => "AudioVolumeMute".into(),
        0xAE => "AudioVolumeDown".into(),
        0xAF => "AudioVolumeUp".into(),
        0xB0 => "MediaTrackNext".into(),
        0xB1 => "MediaTrackPrevious".into(),
        0xB2 => "MediaStop".into(),
        0xB3 => "MediaPlayPause".into(),
        0xB4 => "LaunchMail".into(),
        0xB5 => "LaunchMediaPlayer".into(),
        0xB6 => "LaunchApp1".into(),
        0xB7 => "LaunchApp2".into(),
        0xBA => "Semicolon".into(),
        0xBB => "Equal".into(),
        0xBC => "Comma".into(),
        0xBD => "Minus".into(),
        0xBE => "Period".into(),
        0xBF => "Slash".into(),
        0xC0 => "Backquote".into(),
        0xDB => "BracketLeft".into(),
        0xDC if scan_code == 0x7D => "IntlYen".into(),
        0xDC => "Backslash".into(),
        0xDD => "BracketRight".into(),
        0xDE => "Quote".into(),
        0xE2 if scan_code == 0x73 => "IntlRo".into(),
        0xE2 if scan_code == 0x7D => "IntlYen".into(),
        0xE2 => "IntlBackslash".into(),
        _ => format!("VK_{vk:02X}"),
    }
}

fn name_to_key_spec(name: &str) -> Option<KeySpec> {
    let lower = name.trim().to_ascii_lowercase().replace([' ', '-'], "");
    if lower.len() == 1 {
        let byte = lower.as_bytes()[0];
        if byte.is_ascii_alphabetic() {
            return Some(KeySpec::vk(byte.to_ascii_uppercase() as u16));
        }
        if byte.is_ascii_digit() {
            return Some(KeySpec::vk(byte as u16));
        }
    }
    match lower.as_str() {
        "backspace" => Some(KeySpec::vk(0x08)),
        "tab" => Some(KeySpec::vk(0x09)),
        "enter" => Some(KeySpec::vk(0x0D)),
        "numpadenter" => Some(KeySpec::extended(0x0D)),
        "shift" => Some(KeySpec::vk(0x10)),
        "shiftleft" => Some(KeySpec::vk(0xA0)),
        "shiftright" => Some(KeySpec::vk(0xA1)),
        "control" | "ctrl" => Some(KeySpec::vk(0x11)),
        "controlleft" => Some(KeySpec::vk(0xA2)),
        "controlright" => Some(KeySpec::extended(0xA3)),
        "alt" => Some(KeySpec::vk(0x12)),
        "altleft" => Some(KeySpec::vk(0xA4)),
        "altright" => Some(KeySpec::extended(0xA5)),
        "metaleft" => Some(KeySpec::extended(0x5B)),
        "metaright" => Some(KeySpec::extended(0x5C)),
        "pause" => Some(KeySpec::vk(0x13)),
        "capslock" => Some(KeySpec::vk(0x14)),
        "lang1" | "kanamode" => Some(KeySpec::vk(0x15)),
        "lang3" => Some(KeySpec::vk(0x17)),
        "lang4" => Some(KeySpec::vk(0x18)),
        "lang2" => Some(KeySpec::vk(0x19)),
        "lang5" => Some(KeySpec::vk(0x1A)),
        "escape" | "esc" => Some(KeySpec::vk(0x1B)),
        "convert" => Some(KeySpec::vk(0x1C)),
        "nonconvert" => Some(KeySpec::vk(0x1D)),
        "space" => Some(KeySpec::vk(0x20)),
        "pageup" => Some(KeySpec::extended(0x21)),
        "pagedown" => Some(KeySpec::extended(0x22)),
        "end" => Some(KeySpec::extended(0x23)),
        "home" => Some(KeySpec::extended(0x24)),
        "arrowleft" => Some(KeySpec::extended(0x25)),
        "arrowup" => Some(KeySpec::extended(0x26)),
        "arrowright" => Some(KeySpec::extended(0x27)),
        "arrowdown" => Some(KeySpec::extended(0x28)),
        "printscreen" => Some(KeySpec::extended(0x2C)),
        "insert" => Some(KeySpec::extended(0x2D)),
        "delete" => Some(KeySpec::extended(0x2E)),
        "contextmenu" => Some(KeySpec::extended(0x5D)),
        value if value.starts_with("numpad") && value.len() == 7 => value[6..]
            .parse::<u16>()
            .ok()
            .filter(|n| *n <= 9)
            .map(|n| KeySpec::vk(0x60 + n)),
        "numpadmultiply" => Some(KeySpec::vk(0x6A)),
        "numpadadd" => Some(KeySpec::vk(0x6B)),
        "numpadcomma" => Some(KeySpec::vk(0x6C)),
        "numpadsubtract" => Some(KeySpec::vk(0x6D)),
        "numpaddecimal" => Some(KeySpec::vk(0x6E)),
        "numpaddivide" => Some(KeySpec::extended(0x6F)),
        value if value.starts_with('f') => value[1..]
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=24).contains(n))
            .map(|n| KeySpec::vk(0x6F + n)),
        "numlock" => Some(KeySpec::extended(0x90)),
        "scrolllock" => Some(KeySpec::vk(0x91)),
        "numpadequal" => Some(KeySpec::vk(0x92)),
        "browserback" => Some(KeySpec::extended(0xA6)),
        "browserforward" => Some(KeySpec::extended(0xA7)),
        "browserrefresh" => Some(KeySpec::extended(0xA8)),
        "browserstop" => Some(KeySpec::extended(0xA9)),
        "browsersearch" => Some(KeySpec::extended(0xAA)),
        "browserfavorites" => Some(KeySpec::extended(0xAB)),
        "browserhome" => Some(KeySpec::extended(0xAC)),
        "audiovolumemute" => Some(KeySpec::extended(0xAD)),
        "audiovolumedown" => Some(KeySpec::extended(0xAE)),
        "audiovolumeup" => Some(KeySpec::extended(0xAF)),
        "mediatracknext" => Some(KeySpec::extended(0xB0)),
        "mediatrackprevious" => Some(KeySpec::extended(0xB1)),
        "mediastop" => Some(KeySpec::extended(0xB2)),
        "mediaplaypause" => Some(KeySpec::extended(0xB3)),
        "launchmail" => Some(KeySpec::extended(0xB4)),
        "launchmediaplayer" => Some(KeySpec::extended(0xB5)),
        "launchapp1" => Some(KeySpec::extended(0xB6)),
        "launchapp2" => Some(KeySpec::extended(0xB7)),
        "semicolon" => Some(KeySpec::vk(0xBA)),
        "equal" => Some(KeySpec::vk(0xBB)),
        "comma" => Some(KeySpec::vk(0xBC)),
        "minus" => Some(KeySpec::vk(0xBD)),
        "period" => Some(KeySpec::vk(0xBE)),
        "slash" => Some(KeySpec::vk(0xBF)),
        "backquote" => Some(KeySpec::vk(0xC0)),
        "bracketleft" => Some(KeySpec::vk(0xDB)),
        "backslash" => Some(KeySpec::vk(0xDC)),
        "bracketright" => Some(KeySpec::vk(0xDD)),
        "quote" => Some(KeySpec::vk(0xDE)),
        "intlbackslash" => Some(KeySpec::scan(0x56)),
        "intlro" => Some(KeySpec::scan(0x73)),
        "intlyen" => Some(KeySpec::scan(0x7D)),
        value if value.starts_with("vk_") => u16::from_str_radix(&value[3..], 16)
            .ok()
            .filter(|vk| *vk <= 0xFF)
            .map(KeySpec::vk),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_common_key_names() {
        assert_eq!(name_to_key_spec("A"), Some(KeySpec::vk(0x41)));
        assert_eq!(name_to_key_spec("Escape"), Some(KeySpec::vk(0x1B)));
        assert_eq!(name_to_key_spec("F24"), Some(KeySpec::vk(0x87)));
        assert_eq!(
            name_to_key_spec("ControlRight"),
            Some(KeySpec::extended(0xA3))
        );
        assert_eq!(
            name_to_key_spec("NumpadEnter"),
            Some(KeySpec::extended(0x0D))
        );
        assert_eq!(
            name_to_key_spec("MediaPlayPause"),
            Some(KeySpec::extended(0xB3))
        );
        assert_eq!(name_to_key_spec("IntlRo"), Some(KeySpec::scan(0x73)));
    }
    #[test]
    fn unknown_key_is_not_injected() {
        assert_eq!(name_to_key_spec("definitely-not-a-key"), None);
    }

    #[test]
    fn distinguishes_physical_key_locations() {
        assert_eq!(vk_name(0x11, 0x1D, 0), "ControlLeft");
        assert_eq!(vk_name(0x11, 0x1D, LLKHF_EXTENDED_FLAG), "ControlRight");
        assert_eq!(vk_name(0x0D, 0x1C, LLKHF_EXTENDED_FLAG), "NumpadEnter");
        assert_eq!(vk_name(0xE2, 0x73, 0), "IntlRo");
    }

    #[test]
    fn capture_queue_preserves_alt_space_down_and_up_for_its_session() {
        let capture = CaptureState::new();
        capture.session_id.store(7, Ordering::Release);
        capture.active.store(true, Ordering::Release);

        assert!(capture.enqueue("AltLeft".into(), KeyPhase::Down));
        assert!(capture.enqueue("Space".into(), KeyPhase::Down));
        assert!(capture.enqueue("Space".into(), KeyPhase::Up));
        assert!(capture.enqueue("AltLeft".into(), KeyPhase::Up));

        let drain = capture.drain(7);
        assert!(drain.active);
        assert!(!drain.overflowed);
        assert_eq!(
            drain.events,
            vec![
                KeyCaptureEvent {
                    session_id: 7,
                    key: "AltLeft".into(),
                    phase: KeyPhase::Down,
                },
                KeyCaptureEvent {
                    session_id: 7,
                    key: "Space".into(),
                    phase: KeyPhase::Down,
                },
                KeyCaptureEvent {
                    session_id: 7,
                    key: "Space".into(),
                    phase: KeyPhase::Up,
                },
                KeyCaptureEvent {
                    session_id: 7,
                    key: "AltLeft".into(),
                    phase: KeyPhase::Up,
                },
            ]
        );
    }

    #[test]
    fn window_system_key_fallback_records_alt_space_when_the_hook_path_is_missing() {
        let capture = CaptureState::new();
        capture.session_id.store(11, Ordering::Release);
        capture.active.store(true, Ordering::Release);

        assert!(capture.enqueue_window_system_key(0x12, 0x38, false, true));
        assert!(capture.enqueue_window_system_key(0x20, 0x39, false, true));
        assert!(capture.enqueue_window_system_key(0x20, 0x39, false, false));
        assert!(capture.enqueue_window_system_key(0x12, 0x38, false, false));

        assert_eq!(
            capture.drain(11).events,
            vec![
                KeyCaptureEvent {
                    session_id: 11,
                    key: "AltLeft".into(),
                    phase: KeyPhase::Down,
                },
                KeyCaptureEvent {
                    session_id: 11,
                    key: "Space".into(),
                    phase: KeyPhase::Down,
                },
                KeyCaptureEvent {
                    session_id: 11,
                    key: "Space".into(),
                    phase: KeyPhase::Up,
                },
                KeyCaptureEvent {
                    session_id: 11,
                    key: "AltLeft".into(),
                    phase: KeyPhase::Up,
                },
            ]
        );
    }

    #[test]
    fn window_system_key_fallback_rejects_inactive_capture() {
        let capture = CaptureState::new();
        capture.session_id.store(12, Ordering::Release);

        assert!(!capture.enqueue_window_system_key(0x20, 0x39, false, true));
        let drain = capture.drain(12);
        assert!(!drain.active);
        assert!(!drain.overflowed);
        assert!(drain.events.is_empty());
    }
    #[test]
    fn capture_end_rejects_stale_sessions_and_clears_events() {
        let capture = CaptureState::new();
        capture.session_id.store(4, Ordering::Release);
        capture.active.store(true, Ordering::Release);
        assert!(capture.enqueue("F10".into(), KeyPhase::Down));

        assert!(!capture.end(3));
        assert!(capture.end(4));
        let drain = capture.drain(4);
        assert!(!drain.active);
        assert!(drain.events.is_empty());
    }

    #[test]
    fn capture_stays_active_without_a_per_key_foreground_query() {
        let capture = CaptureState::new();
        capture.active.store(true, Ordering::Release);

        assert!(capture.is_active());
        assert!(capture.enqueue("AltLeft".into(), KeyPhase::Down));
        assert!(capture.is_active());
    }

    #[test]
    fn capture_begin_does_not_require_an_exact_foreground_window_handle() {
        let capture = CaptureState::new();
        capture.window_configured.store(true, Ordering::Release);

        let session = capture.begin().expect("configured capture should begin");

        assert_eq!(session.session_id, 1);
        assert!(capture.active.load(Ordering::Acquire));
    }
    #[test]
    fn capture_begin_releases_runtime_before_the_first_ordinary_key() {
        use keyforge_config::{Profile, Rule, Settings};

        let mut settings = Settings::default();
        let mut profile = Profile::new("capture release");
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        settings.profiles = vec![profile];
        let mut engine = RuntimeEngine::new(CompiledRules::compile(&settings).unwrap());
        let dispatch = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert_eq!(dispatch.actions.len(), 1);
        RUNTIME.with(|runtime| *runtime.borrow_mut() = Some(engine));

        let capture = CaptureState::new();
        capture.set_owner_window(1);
        let session = capture.begin().expect("configured capture should begin");

        let mut releases = Vec::new();
        assert!(release_pending_runtime_outputs_for_capture(
            &capture,
            |action| {
                assert!(RUNTIME.with(|runtime| runtime.try_borrow_mut().is_ok()));
                releases.push(action.clone());
                true
            }
        ));
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].phase, DispatchActionPhase::Up);
        assert!(matches!(
            &releases[0].action,
            Action::SendKeys { chord } if chord == &vec!["MetaLeft"]
        ));

        assert!(capture.enqueue("A".into(), KeyPhase::Down));
        assert!(capture.enqueue("A".into(), KeyPhase::Up));
        assert_eq!(
            capture.drain(session.session_id).events,
            vec![
                KeyCaptureEvent {
                    session_id: session.session_id,
                    key: "A".into(),
                    phase: KeyPhase::Down,
                },
                KeyCaptureEvent {
                    session_id: session.session_id,
                    key: "A".into(),
                    phase: KeyPhase::Up,
                },
            ]
        );

        RUNTIME.with(|runtime| *runtime.borrow_mut() = None);
    }

    #[test]
    fn delayed_capture_release_preserves_the_first_ordinary_key_pair() {
        let capture = CaptureState::new();
        capture.set_owner_window(1);

        let session = capture.begin().expect("configured capture should begin");
        assert!(capture.enqueue("A".into(), KeyPhase::Down));
        assert!(release_pending_runtime_outputs_for_capture(
            &capture,
            |_| true
        ));
        assert!(capture.enqueue("A".into(), KeyPhase::Up));

        assert_eq!(
            capture.drain(session.session_id).events,
            vec![
                KeyCaptureEvent {
                    session_id: session.session_id,
                    key: "A".into(),
                    phase: KeyPhase::Down,
                },
                KeyCaptureEvent {
                    session_id: session.session_id,
                    key: "A".into(),
                    phase: KeyPhase::Up,
                },
            ]
        );
    }

    #[test]
    fn stale_capture_drain_does_not_consume_the_current_session_queue() {
        let capture = CaptureState::new();
        capture.session_id.store(9, Ordering::Release);
        capture.active.store(true, Ordering::Release);
        assert!(capture.enqueue("F10".into(), KeyPhase::Down));

        let stale = capture.drain(8);
        assert_eq!(stale.session_id, 9);
        assert!(!stale.active);
        assert!(stale.events.is_empty());

        let current = capture.drain(9);
        assert!(current.active);
        assert_eq!(
            current.events,
            vec![KeyCaptureEvent {
                session_id: 9,
                key: "F10".into(),
                phase: KeyPhase::Down,
            }]
        );
    }

    #[test]
    fn parses_raw_input_keyboard_identity_components() {
        let path = r"\\?\HID#VID_046D&PID_C31C&MI_00#7&1234&0&0000";
        assert_eq!(path_component(path, "VID_", 4).as_deref(), Some("046D"));
        assert_eq!(path_component(path, "PID_", 4).as_deref(), Some("C31C"));
        assert_eq!(path_component(path, "MI_", 2).as_deref(), Some("00"));
        assert!(!is_virtual_device_path(path));
        assert_eq!(
            keyboard_display_name(Some("046D"), Some("C31C"), false),
            "HID 키보드 · VID_046D / PID_C31C"
        );
    }

    #[test]
    fn classifies_virtual_keyboards_and_builds_deterministic_inventory_ids() {
        let path = r"\\?\ROOT#RDP_KBD#0000";
        assert!(is_virtual_device_path(path));
        assert_eq!(
            keyboard_display_name(None, None, true),
            "가상 또는 원격 키보드"
        );
        assert_eq!(inventory_id(path), inventory_id(path));
        assert_ne!(inventory_id(path), inventory_id("another-device"));
    }

    #[test]
    fn connected_keyboard_inventory_returns_well_formed_entries() {
        let keyboards = list_connected_keyboards().unwrap();
        for keyboard in &keyboards {
            assert!(!keyboard.id.is_empty());
            assert!(!keyboard.name.is_empty());
            assert!(!keyboard.device_path.is_empty());
            assert_eq!(keyboard.source, "raw_input");
            assert!(
                keyboard
                    .manufacturer
                    .as_ref()
                    .is_none_or(|value| !value.trim().is_empty())
            );
            assert!(
                keyboard
                    .instance_id
                    .as_ref()
                    .is_none_or(|value| !value.trim().is_empty())
            );
            assert!(
                keyboard
                    .container_id
                    .as_ref()
                    .is_none_or(|value| !value.trim().is_empty())
            );
            assert!(
                keyboard
                    .hardware_ids
                    .iter()
                    .all(|value| !value.trim().is_empty())
            );
            assert!(
                keyboard
                    .location_paths
                    .iter()
                    .all(|value| !value.trim().is_empty())
            );
        }
        for (index, keyboard) in keyboards.iter().enumerate() {
            assert!(!keyboards[index + 1..].iter().any(|other| {
                keyboard
                    .device_path
                    .eq_ignore_ascii_case(&other.device_path)
            }));
        }
        eprintln!("connected keyboards: {keyboards:#?}");
    }

    #[test]
    fn plans_modifier_remap_down_and_up_as_separate_batches() {
        let meta = name_to_key_spec("MetaLeft").unwrap();
        assert_eq!(meta, KeySpec::extended(0x5B));

        assert_eq!(
            plan_key_events(&[meta], DispatchActionPhase::Down),
            vec![(meta, 0)]
        );
        assert_eq!(
            plan_key_events(&[meta], DispatchActionPhase::Up),
            vec![(meta, KEYEVENTF_KEYUP)]
        );
        assert_eq!(
            plan_key_events(&[meta], DispatchActionPhase::Invoke),
            vec![(meta, 0), (meta, KEYEVENTF_KEYUP)]
        );
    }

    #[test]
    fn injected_event_fast_path_never_reborrows_runtime() {
        let _lifecycle = LIFECYCLE.lock();
        let compiled = CompiledRules::compile(&keyforge_config::Settings::default()).unwrap();
        let shared = Arc::new(Shared {
            rules: ArcSwap::from_pointee(compiled),
            paused: AtomicBool::new(false),
            installed: AtomicBool::new(false),
            thread_id: AtomicU32::new(0),
            capture: CaptureState::new(),
        });
        SHARED.store(Some(shared));

        RUNTIME.with(|runtime| {
            let _borrow = runtime.borrow_mut();
            assert!(!process_event(KeyEvent {
                key: "MetaLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::InjectedSelf,
                repeat: false,
            }));
        });

        SHARED.store(None);
    }

    #[test]
    fn output_injection_runs_after_runtime_borrow_is_released() {
        use keyforge_config::{Profile, Rule, Settings};

        let _lifecycle = LIFECYCLE.lock();
        let mut settings = Settings::default();
        let mut profile = Profile::new("reentrant modifier remap");
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        settings.profiles = vec![profile];
        let compiled = CompiledRules::compile(&settings).unwrap();
        let shared = Arc::new(Shared {
            rules: ArcSwap::from_pointee(compiled),
            paused: AtomicBool::new(false),
            installed: AtomicBool::new(false),
            thread_id: AtomicU32::new(0),
            capture: CaptureState::new(),
        });
        SHARED.store(Some(shared));
        RUNTIME.with(|runtime| *runtime.borrow_mut() = None);

        let suppressed = process_event_with_injector(
            KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            |action| {
                assert_eq!(action.phase, DispatchActionPhase::Down);
                assert!(RUNTIME.with(|runtime| runtime.try_borrow_mut().is_ok()));
                assert!(!process_event(KeyEvent {
                    key: "MetaLeft".into(),
                    phase: KeyPhase::Down,
                    origin: EventOrigin::InjectedSelf,
                    repeat: false,
                }));
                true
            },
        );
        assert!(suppressed);

        let released = process_event_with_injector(
            KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            |action| {
                assert_eq!(action.phase, DispatchActionPhase::Up);
                true
            },
        );
        assert!(released);

        RUNTIME.with(|runtime| *runtime.borrow_mut() = None);
        SHARED.store(None);
    }

    #[test]
    fn shutdown_releases_held_modifier_once_after_dropping_runtime_borrow() {
        use keyforge_config::{Profile, Rule, Settings};

        let mut settings = Settings::default();
        let mut profile = Profile::new("shutdown modifier remap");
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        settings.profiles = vec![profile];
        let mut engine = RuntimeEngine::new(CompiledRules::compile(&settings).unwrap());
        let dispatch = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert_eq!(dispatch.actions.len(), 1);
        assert_eq!(dispatch.actions[0].phase, DispatchActionPhase::Down);
        RUNTIME.with(|runtime| *runtime.borrow_mut() = Some(engine));

        let mut releases = Vec::new();
        assert_eq!(
            release_runtime_outputs_for_shutdown(|action| {
                assert!(RUNTIME.with(|runtime| runtime.try_borrow_mut().is_ok()));
                releases.push(action.clone());
                true
            }),
            1
        );
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].phase, DispatchActionPhase::Up);
        assert!(matches!(
            &releases[0].action,
            Action::SendKeys { chord } if chord == &vec!["MetaLeft"]
        ));
        assert!(RUNTIME.with(|runtime| runtime.borrow().is_none()));

        assert_eq!(release_runtime_outputs_for_shutdown(|_| true), 0);
    }
}
