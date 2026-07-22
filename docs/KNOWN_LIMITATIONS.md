# Known limitations in 0.1.17

KeyForge 0.1.17 intentionally focuses on stable global key and mouse remapping.

- Global single-gesture keyboard and mouse remaps are connected to native
  low-level hooks and `SendInput`.
- Single-key `SendKeys` remaps hold the output from source key-down through
  source key-up. Multi-key output actions emit a bounded tap.
- Hold and double-tap triggers remain represented in the schema but are rejected
  by the current rule compiler.
- Application, device, and combined scopes stay disabled. A low-level hook can
  suppress input but does not identify the physical device; Raw Input identifies
  the device but is a separate event stream. KeyForge does not correlate those
  streams heuristically.
- The read-only Devices tab lists only keyboard endpoints returned by Windows
  Raw Input and never substitutes mock devices. A physical keyboard can expose
  multiple HID collections, so the endpoint count is not a physical-device
  count. SetupAPI enriches endpoints with their current PnP name, manufacturer,
  instance ID, container ID, hardware IDs, and location paths. These values,
  the displayed session ID, and path-derived VID/PID/MI values are diagnostics,
  not persistent profile selectors.
- Macro, text automation, auto-click, coordinate click, file/URL launch,
  clipboard slots, profile-switch actions, window tools, and mock device lists
  are removed. Schema v2 settings drop only rules using those actions while
  retaining supported rules during migration to schema v3.
- The key inspector observes only events sent to its focused capture area. It
  shows browser `code`, `key`, location, and a legacy keyCode/VK reference; an
  exact Windows scan code requires a future native inspection channel.
- While a direct key-capture dialog is open, the native low-level keyboard
  hook creates a short capture session. Physical key-down/key-up records are
  placed in a bounded native queue before Windows or the WebView can process
  them, so `Alt+Space`, `Alt+F4`, `F10`, `Shift+F10`, and the Apps key do not
  open native menus or trigger mappings. If Tao/WebView2 routes a system key
  through a same-thread WebView host window, native subclasses record
  `WM_SYSKEYDOWN`/`WM_SYSKEYUP` in the same queue and return zero before
  `DefWindowProc` can open a system menu. WebView child subclasses are refreshed
  at each capture start. If Windows has already translated `Alt+Space` into
  `WM_SYSCOMMAND(SC_KEYMENU, Space)`, the top-level subclass reconstructs the
  complete left/right Alt plus Space chord in that queue before consuming the
  menu command. The session ends on Use, Cancel,
  dialog unmount, actual `WM_ACTIVATEAPP(FALSE)`, window hide, close, destroy,
  or app exit; a WebView2 child-focus transition does not end it. Secure
  Desktop and shell-reserved combinations such as `Ctrl+Alt+Delete` and `Win+L`
  remain unavailable to a normal user-mode app.
- Notifications are shown in the in-app toast and activity feed. Windows Action
  Center delivery is not connected yet.
- `launchAtLogin` is an explicit opt-in current-user Windows `Run` registration;
  it needs no administrator rights. Windows can delay or disable startup apps,
  and a portable EXE moved to a different path must be registered again by
  turning the setting off and on. KeyForge does not rewrite this registration
  while it is starting up.
- With `closeToTray` enabled, the window close button hides the main window and
  keeps the mapping hook alive. Use `KeyForge 종료` from the tray menu for a
  normal full exit. A forced process kill cannot run held-output cleanup.
- The binary is unsigned and Windows SmartScreen may show a warning.
- Secure Desktop, sign-in screens, higher-integrity applications, and anti-cheat
  games are outside the supported boundary.

The default emergency stop is `Ctrl+Alt+Pause`. Injection failures are fail-open
and cleanup key-up events are attempted before returning control to Windows.
