# KeyForge

KeyForge is a clean-room, Windows-first global key and mouse remapper.
Profiles work in every application by default. Application and device filters
remain disabled until their native match context is implemented reliably.

## Development

Prerequisites: Rust stable, Node.js 20+, Microsoft C++ Build Tools, WebView2.

```powershell
npm install
cargo test --workspace
npm run build:ui
npm run tauri dev
```

The default emergency stop is `Ctrl+Alt+Pause`.

## Project status

The first release implements versioned atomic settings, profile and rule
editing, activity notifications, Windows low-level keyboard and mouse hooks,
injected event suppression, backup/restore, a real read-only Windows keyboard
inventory, native tray lifecycle, optional user-level Windows sign-in startup,
native shortcut capture guarding, and a Fluent-inspired Tauri UI. Closing the
window keeps the process and input hook running in the notification area when
`closeToTray` is enabled; the tray menu provides the explicit quit path. Only
actions backed by native executors are exposed. Device-specific remapping
remains disabled until an authoritative device-aware interception layer is
validated.

See [Known limitations](docs/KNOWN_LIMITATIONS.md) for the exact native feature
boundary of version 0.1.17.
