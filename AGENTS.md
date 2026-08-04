# Repository Guidelines

## Project Overview

KeyForge is a clean-room, Windows-first global keyboard and mouse remapper. Rust owns settings, rule evaluation, Windows hooks/injection, and service orchestration; a Tauri 2 shell exposes that backend to a React 19/TypeScript UI. The current feature boundary is documented in `docs/KNOWN_LIMITATIONS.md`: remapping is global-only, and application/device scopes remain disabled until reliable native matching exists.

## Architecture & Data Flow

- `crates/config` defines the serialized settings contract, validation, migration, optimistic revisions, and atomic backup/recovery.
- `crates/engine` is platform-independent. It compiles enabled rules and turns normalized input events into `Dispatch` values without calling OS APIs.
- `crates/platform-windows` owns low-level hooks, Raw Input inventory, SetupAPI metadata, `SendInput`, launch-at-login, and capture queues. Keep unsafe Win32 code isolated here.
- `crates/daemon::AppService` coordinates settings, compiled rules, hook lifecycle, activity, backup/restore, startup registration, and capture sessions.
- `src-tauri/src/lib.rs` is the desktop adapter: Tauri commands, managed `Arc<AppService>`, events, tray/single-instance behavior, and shutdown.
- `apps/ui/src/lib/bridge.ts` is the only UI/backend boundary. Components must not invoke Tauri directly. Its browser-development mock uses `localStorage`; keep mock and native revision/error behavior aligned.

Typical flow: UI callback → typed bridge → Tauri command → `AppService` → validation/compile/atomic save → hook rule-snapshot swap → Tauri event/UI update. Runtime input flows hook → normalized `KeyEvent` → `RuntimeEngine` → `DispatchAction` → `SendInput`. Injected events carry a private marker and are ignored to prevent recursion.

Preserve fail-open handling, emergency stop (`Ctrl+Alt+Pause`), bounded execution, held-output cleanup, stale-revision checks, and rollback on persistence/registry failure.

## Key Directories

- `apps/ui/src/` — React UI, pages, reusable controls, bridge, shared types, and colocated tests.
- `crates/config/` — settings schema, validation, persistence, migration, and recovery.
- `crates/engine/` — pure rule compilation and runtime state machine.
- `crates/platform-windows/` — Windows hooks, injection, devices, PnP, and startup integration.
- `crates/daemon/` — application service and orchestration boundary.
- `src-tauri/` — desktop executable, IPC commands, tray/window lifecycle, capabilities, and packaging.
- `scripts/regression.ps1` — automated and release regression gates.
- `docs/` — architecture decision, limitations, threat model, and physical-Windows QA plan/results.

## Development Commands

Run commands from `keyforge/` using PowerShell on Windows.

```powershell
npm install                         # install the root npm workspace
npm run dev                         # Vite UI only, fixed port 1420
npm run tauri dev                   # desktop development app
npm run build:ui                    # tsc -b, then Vite production build
npm run build                       # UI plus packaged Tauri build
cargo test --workspace              # all Rust tests
npm run test:ui                     # all Vitest tests
npm run test:regression             # fmt, Clippy, tests, UI build, audit
npm run test:release-gate           # release build plus required manual results
```

Focused examples:

```powershell
cargo test -p keyforge-engine injected_input_is_never_processed
npm --workspace @keyforge/ui run test -- src/App.test.tsx
npm --workspace @keyforge/ui run test -- src/App.test.tsx -t "pauses and resumes the engine"
```

The full Rust quality gates are `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`. There is no ESLint, Prettier, or standalone npm lint/format command; TypeScript diagnostics run during `npm run build:ui`.

## Code Conventions & Common Patterns

### Rust

- Use `thiserror` for typed domain/library errors and transparent conversions. Reserve `anyhow::Result` for composition boundaries such as constructors and binary `main`.
- Keep wire structs `camelCase`; tagged enums use `#[serde(tag = "kind", rename_all = "snake_case")]`. JSON shape is a compatibility contract.
- Keep engine decisions pure and synchronous. Backend commands and persistence are currently blocking; Windows hooks run on a dedicated thread with bounded `sync_channel`s.
- Make dependencies/state explicit with `Arc`, `Mutex`/`RwLock`, traits at testable OS boundaries, and deliberate atomics/`ArcSwap` in hook paths.
- Never hold the thread-local runtime borrow across `SendInput`, which can synchronously re-enter the hook.
- Preserve RAII cleanup around hooks and Win32 resources. Do not write settings JSON or startup registry state outside their repository/service transaction boundaries.

### TypeScript/React

- Use strict TypeScript ESM, functional components, hooks, discriminated unions, explicit `import type`, and typed callback props.
- Follow existing style: 2-space indentation, single quotes, semicolons, trailing commas in multiline constructs, immutable updates, and early returns.
- `App.tsx` is the UI composition/state root. Pass data down and mutations up; keep page-only state local.
- Reuse `components/common.tsx`, `ProfileEditor.tsx`, `keyCatalog.ts`, `types.ts`, and `lib/bridge.ts` rather than creating parallel abstractions.
- Report actionable backend errors; do not fabricate fallback devices or silently hide native failures.

## Important Files

- `README.md` — prerequisites, development quick start, and current product summary.
- `Cargo.toml` / `package.json` — Rust and npm workspace definitions and scripts.
- `crates/config/src/model.rs` — persisted and IPC schema.
- `crates/config/src/validation.rs` — configuration invariants and feature limits.
- `crates/config/src/repository.rs` — atomic persistence, revisions, migration, and recovery.
- `crates/engine/src/lib.rs` — compiler and runtime dispatch semantics.
- `crates/platform-windows/src/windows_impl.rs` — hook, capture, inventory, and injection implementation.
- `crates/daemon/src/lib.rs` — central `AppService`.
- `src-tauri/src/lib.rs` / `src-tauri/src/main.rs` — desktop adapter and executable entry point.
- `src-tauri/src/key_capture_guard.rs` — native defense for system-key capture such as Alt+Space.
- `apps/ui/src/App.tsx` / `apps/ui/src/lib/bridge.ts` — UI root and IPC boundary.
- `src-tauri/tauri.conf.json` — dev/build commands, product metadata, window, and NSIS configuration.
- `docs/ADR-0001-architecture.md` — required subsystem boundaries.

## Runtime/Tooling Preferences

Use Rust stable with edition 2024 support, Node.js 20+, npm, Microsoft C++ Build Tools, and WebView2. Use Cargo for Rust and npm from the repository root for JavaScript; both lockfiles are committed. Do not substitute Bun, Yarn, or pnpm. Tauri development expects Vite at exactly `http://localhost:1420`.

Keep version `0.1.14` (or its successor) synchronized across the Cargo workspace, root/UI package manifests, and `src-tauri/tauri.conf.json`; the regression script enforces this. Packaging targets current-user NSIS on Windows.

## Testing & QA

- Rust tests are inline `#[cfg(test)]` modules throughout the crates and `src-tauri`; select them with `cargo test -p <package> <filter>`.
- UI tests are colocated under `apps/ui/src` and use Vitest, jsdom, Testing Library, `userEvent`, and `jest-dom`. Prefer roles, accessible names, focus, dialogs, and visible outcomes over implementation details.
- Cover native and browser bridge paths when changing backend-sensitive behavior. Test structured errors, revisions, persistence, capture interruption, and state retention after refresh failures.
- Add or strengthen the regression test for a failed matrix item before fixing the defect.
- `npm run test:regression` is automated evidence only. Physical-keyboard P0/P1 checks in `docs/REGRESSION_TEST_PLAN.md` are required for release because injected input cannot validate the physical hook path.
- Record hardware, build, time, result, and evidence in `docs/REGRESSION_MANUAL_RESULTS.md`. `PENDING` rows mean manual QA has not passed; never claim otherwise.
- Windows-specific tests may touch HKCU startup registration or live device enumeration. Keep those environmental assumptions explicit when selecting focused tests.
