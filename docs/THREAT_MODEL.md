# Threat model

- Injected input is tagged and ignored to prevent recursion.
- `Ctrl+Alt+Pause` bypasses user rules and stops the engine.
- Rule repetitions and execution durations are bounded.
- Clipboard values and captured keystrokes are never written to activity logs.
- Files and URLs are validated and never launched during configuration preview.
- Settings are validated before writing and read back before a success result.
- Secure Desktop, sign-in screens, elevated applications, and anti-cheat games
  are outside the supported trust boundary.
