# KeyForge 회귀 테스트 계획

이 문서는 지금까지 확정된 요구사항의 릴리스 기준이다. **자동 회귀 게이트와
P0/P1 물리 Windows 승인 항목이 모두 PASS일 때만 작업/릴리스를 완료로 선언한다.**
`SendInput`은 의도적으로 물리 훅에서 무시되므로 물리 키보드 E2E를 대체하지 못한다.

## 실행 방법

```powershell
npm run test:regression
npm run test:release-gate
```

- `test:regression`: 버전 동기화, 포맷, Clippy, Rust/UI 테스트, UI build, audit.
- `test:release-gate`: 위 검사와 Windows release build, PE/installer 버전 검증,
  그리고 수동 P0/P1 결과표의 PASS 여부를 모두 확인한다.
- 수동 결과는 [REGRESSION_MANUAL_RESULTS.md](REGRESSION_MANUAL_RESULTS.md)에
  실제 장비·빌드·시간·증거와 함께 기록한다. PENDING 또는 FAIL이 하나라도 있으면
  완료가 아니다.

## 자동 회귀 매트릭스

`keyforge/`에서 아래 명령을 실행한다. 자동 검증은 실제 키보드 훅·트레이·레지스트리를
대체하지 않으며, 마지막 열의 `WIN-*`은 별도 물리 Windows 승인 항목이다.

| ID | 요구사항 | 자동 명령·증거 경로 | 물리 Windows 승인 |
|---|---|---|---|
| AUTO-VERSION | UI/Cargo/Tauri/패키지 버전 및 상단 `vX.Y.Z` | `npm run test:regression` — `scripts/regression.ps1`, `apps/ui/src/App.test.tsx` | 새 빌드 버전 표기: WIN-PERSIST |
| AUTO-SETTINGS | 원자 저장, revision, 백업/손상 복구 | `cargo test -p keyforge-config save_increments_revision_and_preserves_backup`; `cargo test -p keyforge-config corrupt_primary_recovers_from_backup` — `crates/config/src/repository.rs` | 종료·재시작 복구: WIN-PERSIST |
| AUTO-GLOBAL | 앱 지정 없이 전역 기본 프로필, 비지원 scope 차단 | `cargo test -p keyforge-config version_one_conditional_scope_migrates_to_global` — `crates/config/src/repository.rs` | 해당 없음 |
| AUTO-MAPPING | 전역 키 매핑, 보조키 down/up, 주입 입력 재처리 차단 | `cargo test -p keyforge-engine injected_input_is_never_processed`; `cargo test -p keyforge-engine single_modifier_remap_holds_output_until_source_key_up` — `crates/engine/src/lib.rs` | WIN-MAP-BASIC |
| AUTO-MODIFIER-CYCLE | Alt→Ctrl→Meta→Alt 순환 및 다음 일반키 보호 | `cargo test -p keyforge-engine modifier_cycle_does_not_consume_the_next_ordinary_key` — `crates/engine/src/lib.rs` | WIN-MAP-CYCLE |
| AUTO-DIRECT-UI | 입력·출력 직접 누르기, 키 선택 목록, 좌/우 키 | `npm --workspace @keyforge/ui run test -- src/components/ProfileEditor.test.tsx` — `apps/ui/src/components/ProfileEditor.test.tsx` | WIN-CAP-ALTSPACE-IN, WIN-CAP-ALTSPACE-OUT |
| AUTO-CAPTURE-HANDOFF-SAVE | 입력 session A 종료 완료 뒤 출력 session B 시작, 출력 native drain chord를 규칙/프로필 저장까지 유지 | `npm --workspace @keyforge/ui run test -- src/components/ProfileEditor.test.tsx -t "waits for an input teardown before beginning native output capture and saves the output chord"` — `apps/ui/src/components/ProfileEditor.test.tsx` | WIN-CAP-ALTSPACE-IN, WIN-CAP-ALTSPACE-OUT, WIN-PERSIST |
| AUTO-ALTSPACE-QUEUE | `AltLeft + Space` native queue down/up 순서 | `cargo test -p keyforge-platform-windows capture_queue_preserves_alt_space_down_and_up_for_its_session` — `crates/platform-windows/src/windows_impl.rs` | WIN-CAP-ALTSPACE-IN, WIN-CAP-ALTSPACE-OUT |
| AUTO-ALTSPACE-WNDPROC | `WM_SYSKEYDOWN/UP` 차단 및 fallback queue 기록 | `cargo test -p keyforge-platform-windows window_system_key_fallback_records_alt_space_when_the_hook_path_is_missing` — `crates/platform-windows/src/windows_impl.rs` | WIN-CAP-SYSTEM |
| AUTO-ALTSPACE-SYSCOMMAND | WebView2 `SC_KEYMENU + Space` 뒤 chord 복원 | `cargo test -p keyforge-app alt_space_reconstruction_requires_an_unambiguous_alt_side` — `src-tauri/src/key_capture_guard.rs` | WIN-CAP-ALTSPACE-IN, WIN-CAP-ALTSPACE-OUT |
| AUTO-CAPTURE-LIFECYCLE | WebView focus false-positive, stale drain, cancel/end | `npm --workspace @keyforge/ui run test -- src/components/ProfileEditor.test.tsx`; `cargo test -p keyforge-platform-windows stale_capture_drain_does_not_consume_the_current_session_queue` | WIN-CAP-SYSTEM |
| AUTO-NO-AUTOMATION | 매크로/자동화/창 도구/가짜 장치 제거 | `npm --workspace @keyforge/ui run test -- src/App.test.tsx`; `cargo test -p keyforge-config removed_action_rules_are_migrated_without_losing_supported_rules` | 해당 없음 |
| AUTO-DEVICES | Raw Input/PnP 목록 파싱·오류 처리 | `npm --workspace @keyforge/ui run test -- src/DevicesPage.test.tsx`; `cargo test -p keyforge-platform-windows parses_raw_input_keyboard_identity_components` | WIN-DEVICE |
| AUTO-INSPECTOR | 키 확인 탭 정규화·상세값·초기 focus | `npm --workspace @keyforge/ui run test -- src/KeyInspectorPage.test.tsx` — `apps/ui/src/KeyInspectorPage.test.tsx` | WIN-INSPECTOR |
| AUTO-TRAY | close-to-tray, tray menu, shutdown idempotence | `cargo test -p keyforge-app tray_menu_ids_have_explicit_actions`; `cargo test -p keyforge-daemon close_to_tray_preference_is_live_and_shutdown_is_idempotent` | WIN-TRAY |
| AUTO-AUTOSTART | HKCU Run 등록, quoting, rollback, 복원 | `cargo test -p keyforge-platform-windows quotes_spaces_trailing_backslashes_and_quotes_for_windows`; `cargo test -p keyforge-daemon stale_settings_save_restores_the_previous_startup_registration` | WIN-AUTOSTART |
| AUTO-NOTIFY | 저장/일시정지 toast·activity | `npm --workspace @keyforge/ui run test -- src/App.test.tsx -t "pauses and resumes the engine"` — `apps/ui/src/App.test.tsx` | WIN-NOTIFY |
| AUTO-STABILITY | RefCell reentry, held output cleanup, injected input 무시 | `cargo test -p keyforge-platform-windows injected_event_fast_path_never_reborrows_runtime`; `cargo test -p keyforge-platform-windows shutdown_releases_held_modifier_once_after_dropping_runtime_borrow` | WIN-SOAK |
| AUTO-SOAK-PRECONDITION | 30~60분 반복 사용 전 자동 사전 회귀 | `npm run test:regression` — `scripts/regression.ps1` | 30~60분 실제 반복은 자동 대체 불가: WIN-SOAK |

## 물리 Windows 승인 매트릭스

| ID | 우선순위 | 실제 확인 항목 |
|---|---|---|
| WIN-CAP-ALTSPACE-IN | P0 | 입력 키 직접 누르기에서 `LeftAlt + Space`가 `AltLeft + Space`로 표시되고 시스템 메뉴가 열리지 않음 |
| WIN-CAP-ALTSPACE-OUT | P0 | 전송 키 직접 누르기에서도 같은 동작 |
| WIN-CAP-SYSTEM | P0 | `Alt+F4`, `F10`, `Shift+F10`, Apps 키를 입력/출력 양쪽에서 캡처하고, 취소 뒤 각 기본 메뉴/동작이 복귀 |
| WIN-MAP-BASIC | P0 | 다른 앱에서 `ControlLeft → MetaLeft` down/up, 일반키 조합, 원래 입력 전달을 확인 |
| WIN-MAP-CYCLE | P0 | Alt→Ctrl→Meta→Alt 세 규칙을 실제 키보드로 누름/뗌 후 stuck modifier 및 다음 일반키 오류 없음 |
| WIN-TRAY | P0 | X→트레이 숨김→매핑 지속→트레이 열기→트레이 종료→프로세스 종료 |
| WIN-PERSIST | P1 | 저장→완전 종료→재시작→규칙/설정/백업 복구 확인 |
| WIN-AUTOSTART | P1 | opt-in Run 값, 시작 최소화, 경로 변경 재등록 안내 확인 |
| WIN-DEVICE | P1 | 실제 장치 탭 refresh, 빈/부분 정보, 연결 장치 표기; device scope가 지원되지 않음을 확인 |
| WIN-INSPECTOR | P1 | 키 확인 탭의 좌/우 보조키·숫자패드·media 상세값과 clear/focus 확인 |
| WIN-NOTIFY | P1 | 저장/적용/오류가 toast와 활동 기록에 일치하게 표시 |
| WIN-SOAK | P1 | 30~60분 동안 매핑·capture open/cancel/use·hide/reopen 반복 후 crash, stuck modifier, 이벤트 로그 오류 없음 |

## 완료 규칙

1. `npm run test:release-gate`가 성공한다.
2. 수동 결과표의 모든 P0/P1이 PASS이며 증거가 있다.
3. 새 빌드의 좌측 상단 버전, PE FileVersion/ProductVersion, 설치본 버전이 일치한다.
4. 실패가 발생하면 해당 ID의 자동 회귀 테스트를 먼저 추가하거나 강화한 후 수정한다.
