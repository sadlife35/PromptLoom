# ADR 0001 — Tauri v2 shell, Rust backend, whisper.cpp ASR

Status: accepted (fixed by the build specification).

## Context

PromptLoom must ship a Windows desktop app that records only while a global
hotkey is held, transcribes locally, applies deterministic text corrections,
and inserts the result into the previously focused window — with raw audio
never touching disk.

## Decision

- Shell: **Tauri v2**. React/TypeScript for a tray menu, a settings window,
  and a non-focusable recording HUD. The webview owns no audio and no
  clipboard logic.
- Backend: **Rust**. All audio capture, state transitions, model invocation,
  and clipboard transactions live in Rust so the webview cannot leak audio.
- ASR: **whisper.cpp**, exactly one English-capable small/base GGUF model,
  SHA-256 verified at install time and kept warm after first use.
- Audio: **CPAL/WASAPI**, mono PCM16 at 16 kHz. Capture callback writes to a
  bounded in-memory channel; it never blocks, allocates heavily, or hits disk.
- Insertion: Windows clipboard set → Ctrl+V to the captured HWND → prior
  clipboard restore. Target identity captured before recording.
- Config: `serde_json` under the per-user application-data directory.
- License: Apache-2.0 for application code; model weights retain their own
  terms via `models/manifest.json`.

## Consequences

- The desktop crate (`apps/desktop/src-tauri`) depends on Tauri v2 and
  WebView2; the top-level Cargo workspace excludes it in M0 so the ship-gate
  `cargo test --workspace` runs without those prerequisites.
- Changing any of these decisions requires a new ADR in `docs/adr/`.
