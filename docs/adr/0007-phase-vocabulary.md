# ADR 0007 — Phase vocabulary surfaced to the UI

Status: accepted (Alpha A2).

## Context

Alpha A2 requires that users can always tell whether they are **Ready**,
**Recording**, **Processing locally**, text was **Delivered** / **Copied
only**, dictation was **Cancelled**, or hit an **Error**. The internal
state machine is finer-grained than that — it distinguishes `Idle`,
`Recording`, `Finalizing`, `Transcribing`, `Correcting`, `Delivering`,
plus notice variants (`Delivered(Pasted|CopyOnly|Failed)`, `Cancelled`,
`TooShort`, `Error(ErrorKind)`).

## Decision

- Introduce a user-visible **`Phase`** record emitted on
  `promptloom:phase`. Fields: `kind` (one of seven kebab-case strings),
  `label` (plain English, 1–3 words), `detail` (one sentence with a
  recovery hint for errors).
- The mapping lives in `main.rs::phase_for(state, notices)` — one function,
  easy to amend. Notices take precedence over the mechanical state because
  they encode what just happened, which is what the user cares about.
- HUD stays visible through processing and the terminal confirmation,
  **not** just during recording. It auto-hides after a per-kind dwell
  (900 ms for Delivered, 1.6 s for Copied only, 2.4 s for Error, 600 ms
  for Cancelled).
- Tray tooltip is updated best-effort via `app.tray_by_id("main")` so the
  user can glance at the taskbar for current state.
- Error states include a plain-language recovery hint:
  - *No microphone* → "Plug one in or check Windows Sound settings."
  - *Model not installed* → "Open Settings and reinstall the local speech
    model."
  - *Transcription failed* → "Try again. Speak closer to the mic."
  - *No window to paste into* → "Click back into the app you want to
    dictate into."
  - *Could not paste* → "Text is on your clipboard — paste it manually
    with Ctrl+V."

## Non-goals

- No animations beyond the state transitions.
- No fake progress indicator — "Processing locally" is a label, not a
  spinner with a fake percentage.
- No history view of past dictations.

## Consequences

- Any new error kind added to `ErrorKind` must also get a plain-language
  entry in `error_phase`, or it will label as the enum variant name — a
  compiler-visible miss (match is exhaustive).
- The HUD now briefly stays visible after a successful paste; users who
  found the old quick-hide behaviour less obtrusive can argue for a config
  toggle later. For A2 we chose clarity over invisibility.
- The orchestrator still runs the whisper subprocess synchronously inside
  the hotkey-release handler. UI responsiveness during long transcriptions
  is unaffected (we flash "Processing locally" before the block), but no
  other backend command can run during that window. Left as a known
  limitation; not required by A2.
