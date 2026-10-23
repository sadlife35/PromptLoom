# ADR 0002 — Hold-to-talk hotkey = F8; cancel = Escape

Status: accepted.

## Context

The spec mandates a global hold-to-talk hotkey and Escape cancellation but
does not pick concrete keys. The hotkey must (a) rarely collide with other
applications, (b) be a single key rather than a chord (hold ergonomics),
(c) survive focus changes to a protected or elevated window.

## Decision

- **Hold-to-talk: F8.** Function keys are rarely rebound by productivity
  apps, F8 isn't commonly reserved by Windows itself (unlike F10/F11), and
  it is on every laptop keyboard without a Fn-layer compromise. The user
  can rebind it later from Settings in M4.
- **Cancel: Escape.** Universal cancel semantics. Registered globally so
  it works even when the HUD does not hold focus.
- Both are registered via `tauri-plugin-global-shortcut` v2 with
  `ShortcutState::Pressed`/`Released` dispatched to the orchestrator.

## Consequences

- Users with F8-bound tools (e.g. some IDEs' step-over) can override via
  Settings in M4.
- Registering a global Escape may feel intrusive outside active recording;
  the handler ignores Escape when the state machine is in `Idle`.
