# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

KeyForge is a clean-room, Windows-first global keyboard/mouse remapper. Rust owns settings, rule evaluation, Windows hooks/injection, and service orchestration; a Tauri 2 shell exposes that backend to a React 19/TypeScript UI. Remapping is currently global-only — application and device scopes are disabled until reliable native matching exists (see `docs/KNOWN_LIMITATIONS.md`).

The default emergency stop is `Ctrl+Alt+Pause`.

## Development commands

Run all commands from this `keyforge/` directory using PowerShell on Windows.

```powershell
npm install                         # install the root npm workspace
npm run dev                         # Vite UI only, fixed port 1420
npm run tauri dev                   # desktop development app
npm run build:ui                    # tsc -b, then Vite production build
npm run build                       # UI plus packaged Tauri build
cargo test --workspace              # all Rust tests
npm run test:ui                     # all Vitest tests
npm run test:regression             # fmt, Clippy, tests, UI build, audit (scripts/regression.ps1)
npm run test:release-gate           # release build plus required manual QA results
```

Focused/single-test examples:

```powershell
cargo test -p keyforge-engine injected_input_is_never_processed
npm --workspace @keyforge/ui run test -- src/App.test.tsx
npm --workspace @keyforge/ui run test -- src/App.test.tsx -t "pauses and resumes the engine"
```

Rust quality gates: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`. There is no ESLint/Prettier/standalone lint script; TypeScript diagnostics run during `npm run build:ui`.

Tauri dev expects Vite at exactly `http://localhost:1420`. Use Cargo and npm (from the repo root) only — do not substitute Bun, Yarn, or pnpm; both lockfiles are committed.

Keep the version (`0.1.14` or its successor) synchronized across the Cargo workspace, root/UI package manifests, and `src-tauri/tauri.conf.json` — `scripts/regression.ps1` enforces this.

## Architecture & data flow

- `crates/config` — the serialized settings contract: validation, migration, optimistic revisions, atomic backup/recovery. Schema shape is a compatibility contract; JSON wire structs are `camelCase`, tagged enums use `#[serde(tag = "kind", rename_all = "snake_case")]`.
- `crates/engine` — platform-independent. Compiles enabled rules and turns normalized input events into `Dispatch` values without calling any OS API. Keep decisions pure and synchronous.
- `crates/platform-windows` — low-level hooks, Raw Input inventory, SetupAPI metadata, `SendInput`, launch-at-login. All unsafe Win32 code stays isolated here.
- `crates/daemon::AppService` — coordinates settings, compiled rules, hook lifecycle, activity, backup/restore, startup registration, and capture sessions. This is the central service object.
- `src-tauri/src/lib.rs` — the desktop adapter: Tauri commands, managed `Arc<AppService>`, events, tray/single-instance behavior, shutdown. `src-tauri/src/key_capture_guard.rs` is native defense for system-key capture (e.g. `Alt+Space`).
- `apps/ui/src/lib/bridge.ts` — the **only** UI/backend boundary; components must never call Tauri directly. Its browser-dev mock uses `localStorage` — keep mock and native revision/error behavior aligned.

Typical UI-driven flow: UI callback → typed bridge → Tauri command → `AppService` → validation/compile → atomic save → hook rule-snapshot swap → Tauri event → UI update.

Typical runtime flow: hook → normalized `KeyEvent` → `RuntimeEngine` → `DispatchAction` → `SendInput`. Injected events carry a private marker and are ignored, to prevent recursion into the hook.

Preserve across changes: fail-open handling, the `Ctrl+Alt+Pause` emergency stop, bounded execution, held-output cleanup, stale-revision checks, and rollback on persistence/registry failure. Never hold the thread-local runtime borrow across `SendInput`, since it can synchronously re-enter the hook.

## Key directories

- `apps/ui/src/` — React UI: `App.tsx` is the composition/state root (data flows down, mutations flow up; keep page-only state local); `components/` holds `ProfileEditor.tsx` and `common.tsx`; `keyCatalog.ts`, `types.ts`, `lib/bridge.ts` are shared and should be reused rather than duplicated.
- `crates/config/`, `crates/engine/`, `crates/platform-windows/`, `crates/daemon/` — see above.
- `src-tauri/` — desktop executable, IPC commands, tray/window lifecycle, capabilities, packaging (`tauri.conf.json` holds dev/build commands, product metadata, window, and NSIS config).
- `scripts/regression.ps1` — automated and release regression gates.
- `docs/` — `ADR-0001-architecture.md` (subsystem boundaries), `KNOWN_LIMITATIONS.md` (exact native feature boundary for the current version), `THREAT_MODEL.md`, `REGRESSION_TEST_PLAN.md` / `REGRESSION_MANUAL_RESULTS.md` (physical-Windows QA).

## Code conventions

**Rust**: use `thiserror` for typed domain/library errors and transparent conversions; reserve `anyhow::Result` for composition boundaries (constructors, binary `main`). Windows hooks run on a dedicated thread with bounded `sync_channel`s; backend commands and persistence are currently blocking. Make dependencies/state explicit with `Arc`, `Mutex`/`RwLock`, traits at testable OS boundaries, and deliberate atomics/`ArcSwap` in hook paths. Preserve RAII cleanup around hooks and Win32 resources; never write settings JSON or startup registry state outside the repository/service transaction boundaries.

**TypeScript/React**: strict TypeScript ESM, functional components, hooks, discriminated unions, explicit `import type`, typed callback props. 2-space indent, single quotes, semicolons, trailing commas in multiline constructs, immutable updates, early returns. Report actionable backend errors — do not fabricate fallback devices or silently hide native failures.

## Testing & QA

- Rust tests are inline `#[cfg(test)]` modules throughout the crates and `src-tauri`; select with `cargo test -p <package> <filter>`.
- UI tests are colocated under `apps/ui/src`, using Vitest, jsdom, Testing Library, `userEvent`, `jest-dom`. Prefer roles, accessible names, focus, dialogs, and visible outcomes over implementation details.
- When changing backend-sensitive behavior, cover both native and browser bridge paths (structured errors, revisions, persistence, capture interruption, state retention after refresh failures).
- Add or strengthen a regression test for a failed matrix item before fixing the defect.
- `npm run test:regression` is automated evidence only — physical-keyboard P0/P1 checks in `docs/REGRESSION_TEST_PLAN.md` are additionally required for release, since injected input cannot validate the physical hook path. Results are recorded in `docs/REGRESSION_MANUAL_RESULTS.md`; `PENDING` rows mean manual QA has not passed — never claim otherwise.
- Some Windows-specific tests touch HKCU startup registration or live device enumeration; keep those environmental assumptions explicit when selecting focused tests.

## Important files

- `crates/config/src/model.rs` — persisted and IPC schema.
- `crates/config/src/validation.rs` — configuration invariants and feature limits.
- `crates/config/src/repository.rs` — atomic persistence, revisions, migration, recovery.
- `crates/engine/src/lib.rs` — compiler and runtime dispatch semantics.
- `crates/platform-windows/src/windows_impl.rs` — hook, capture, inventory, injection implementation.
- `crates/daemon/src/lib.rs` — central `AppService`.
- `src-tauri/src/lib.rs` / `src-tauri/src/main.rs` — desktop adapter and executable entry point.
- `src-tauri/src/key_capture_guard.rs` — native defense for system-key capture (e.g. Alt+Space).
- `apps/ui/src/App.tsx` / `apps/ui/src/lib/bridge.ts` — UI root and IPC boundary.
- `src-tauri/tauri.conf.json` — dev/build commands, product metadata, window, NSIS configuration.
- `docs/ADR-0001-architecture.md` — required subsystem boundaries.
