# ADR-0001: Rust core with Tauri UI

## Decision

Use independent crates for configuration, rule evaluation, Windows input, and
the daemon. The Tauri shell owns commands and UI event delivery but not domain
logic. The UI is React and TypeScript.

Profiles default to global scope. Application, device, and combined scopes are
explicit variants and require valid conditions.

## Consequences

The configuration and engine can be tested without a GUI or global hooks.
Windows-only APIs remain isolated. Native integration tests must run on Windows.

