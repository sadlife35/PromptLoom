# ADR 0005 — Project vocabulary and settings persistence

Status: accepted.

## Context

M4 must let the user point PromptLoom at one project at a time, load its
committed `.promptloom/terms.yml`, apply deterministic corrections to each
transcript, and remember the choice across launches. The spec forbids any
automatic repository crawling, team sync, or multi-project state.

## Decision

- **Settings file**: `%APPDATA%\com.promptloom.desktop\settings.json`.
  Contains the chosen `project_path` (nullable) and a cached `terms_count`
  for the Settings UI. Serde JSON, pretty-printed, no secrets.
- **One project at a time.** The user picks a folder via a native dialog
  (`tauri-plugin-dialog`). PromptLoom reads **only** `<project>/.promptloom/terms.yml`
  — nothing else is scanned.
- **Hot-swap on save.** Changing the project folder reloads the vocabulary
  and swaps it into the running orchestrator via `Orchestrator::components()`
  without restarting the app or interrupting audio capture.
- **Boot behaviour.** On launch, if the saved `project_path` still exists
  and contains `terms.yml`, PromptLoom loads it before accepting the first
  dictation. If the folder is gone or the file is invalid, the empty
  vocabulary is used and a notice surfaces in the Settings UI.
- **Correction semantics are already defined** by `crates/vocabulary/`:
  ASCII-case-insensitive whole-phrase replacement, longest source first,
  destination spelling preserved exactly.

## Non-goals

- No automatic project detection from the focused window's working dir.
- No multi-project blending.
- No fuzzy matching.
- No per-language rule sets.

## Consequences

- The settings file is readable; it holds only a path, not credentials.
- Vocabulary swap is observable from the UI (notice "vocabulary reloaded:
  N terms"), so the user can validate the file parsed without dictating.
- A malformed `terms.yml` logs a warn and falls back to the empty
  vocabulary — the app never refuses to launch over a broken rules file.
