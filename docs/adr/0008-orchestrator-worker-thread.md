# ADR 0008 — Orchestrator runs on its own worker thread

Status: accepted (post-A5 fix).

## Context

Before this change, `main.rs` held the orchestrator in `Mutex<Option<Orch>>`.
Every hotkey event, Tauri command, and vocabulary swap had to acquire that
mutex. Because `orch.release()` runs the whole pipeline synchronously —
including the ~1–2 s `whisper-cli.exe` subprocess call — the Settings
window and all other Tauri commands blocked for the duration of each
dictation.

The UI felt broken even though it worked. Called out as a known limitation
in `RELEASE_NOTES.md`.

## Decision

Move the orchestrator to a dedicated worker thread. Communicate via a
bounded (32-slot) `std::sync::mpsc::sync_channel<WorkerCmd>`:

```
enum WorkerCmd {
    HotkeyPressed(TargetWindow),
    HotkeyReleased,
    EscapePressed,
    ReloadVocabulary(Vocabulary),
    SwapTranscriber(ActiveTranscriber),
}
```

Rules:
- The worker thread owns `Orch` exclusively — no other code can touch it.
- Tauri command handlers only ever `try_send` to the worker. They never
  block. If the channel is full, the oldest hotkey press loses rather than
  the UI freezing.
- Current state is published as an `Arc<Mutex<State>>` snapshot that the
  worker updates after every event. UI commands read the snapshot without
  touching the worker.
- The "Processing locally" phase emit now happens inside the worker, right
  before `orch.release()` blocks. The UI sees it immediately because the
  Tauri command thread returned the moment the message was sent.

## Consequences

- **UI never freezes during whisper.** `get_settings`, `audit_privacy`,
  `model_status`, `current_state` all return instantly even mid-dictation.
- **`current_state` is a snapshot**, not a live read. There is a tiny
  window between a hotkey event arriving and the worker publishing the new
  state. The settings UI is tolerant of this (it only matters for the
  `set_dictation_mode` Idle check, and the worst-case loss is "mode change
  rejected during a dictation that just finished a millisecond ago" —
  user retries).
- **`try_send` can drop events** if the channel fills up. Capacity 32 is
  enough for any realistic human-hotkey burst; dropping is better than
  blocking the global shortcut handler, which can cause Windows to think
  the handler is unresponsive.
- **Vocabulary and transcriber swaps are asynchronous.** Settings
  responses to the user ("vocabulary reloaded: 5 terms") are optimistic —
  they report what the UI asked for, not what the worker has processed.
  Fine in practice; swaps run in milliseconds behind the channel.
- The emission ordering changes slightly: the Processing phase is emitted
  by the worker, not the handler. All existing phase listeners (HUD,
  Settings) treat events as unordered, so no UI change is needed.

## Non-goals

- Not a full async rewrite. The worker is a plain OS thread with blocking
  calls. We don't want tokio in the orchestrator path.
- Not a multi-worker pool. A single orchestrator serializes dictations by
  design; concurrent dictations would be semantically wrong (two Ctrl+V
  pastes racing into the focused window).
