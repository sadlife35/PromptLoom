# ADR 0006 — First-run wizard inside the Settings window

Status: accepted (Alpha A1).

## Context

Alpha A1 requires a first-run experience covering privacy, microphone,
model, shortcut, vocabulary, and a "try it in Notepad" test. Options:

1. A dedicated Tauri window that closes on completion.
2. An inline wizard inside the existing Settings window that is replaced
   by the normal Settings UI once onboarding completes.

## Decision

Choice 2: inline inside the Settings window.

- Reuses existing Tauri plumbing, Vite entry, and permission capabilities.
- Avoids a second window label + lifecycle (open-on-first-run,
  close-on-done, re-open-on-reset) that would need separate state tracking.
- The user cannot accidentally dismiss onboarding and leave the main
  Settings in an unusable half-configured state — the main UI only renders
  after `needs_onboarding` returns false.
- Smaller code surface (<400 lines of React) and no new window config.

Backend adds a single boolean (`onboarding_complete`) plus a `needs_onboarding`
command that also returns `true` when the model is missing or SHA-mismatched
on disk, so a deleted model naturally re-routes the user to the model step
without a second code path.

## Non-goals

- No separate "about" or "walkthrough" surface outside this one wizard.
- No arbitrary hotkey recording — only a small safe preset (`F8`, `F9`,
  `F10`, `F2`) is offered (see ADR 0002).

## Consequences

- Resetting the user's progress is a single toggle: set
  `onboarding_complete` false in `settings.json` (or delete the model file,
  which gets the user back to the model step automatically on next launch).
- The Settings main UI loses its model-download card redundancy; the main
  UI assumes the model is already installed. A missing model after
  completion returns the user to the wizard rather than offering a fix
  inline. Acceptable for A1; a Models section (A4) will restore inline
  management.
