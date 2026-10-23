# ADR 0004 — Delivery via clipboard transaction + Ctrl+V

Status: accepted.

## Context

M3 must deliver the dictated text into the user's previously focused app.
The spec forbids blind input injection: text may only be pasted into the
HWND we captured at hotkey-press time. Password managers, UAC/elevated
windows, and other protected targets must fall back to copy-only.

## Decision

The delivery path:

1. **Read & save** any prior Unicode text from the clipboard. (Richer
   formats — CF_HDROP, CF_DIB — are not touched; see Consequences.)
2. **Write** the dictated text as `CF_UNICODETEXT` via the `clipboard-win`
   crate, which handles HGLOBAL/HANDLE semantics safely.
3. **Compare** the current foreground window identity to the captured one
   (both `HWND` and `process_id` must match). If anything shifted, abort
   the paste and return `CopyOnly` — the user's transcript is on their
   clipboard; they can paste it manually.
4. **Inject** `Ctrl+V` via `SendInput` on the captured HWND.
5. **Restore** prior clipboard in a background thread after ~450 ms, but
   **only** if the clipboard still holds our exact text. If anything (the
   user, another app) wrote to the clipboard between paste and restore,
   leave it alone — their write wins.

## Non-goals (unchanged from the spec)

- No blind keystroke injection.
- No AT/UIA integration (no accessibility prompts).
- No clipboard-history exclusion in M3 — the Windows
  `ExcludeClipboardContentFromMonitorProcessing` format requires
  registering a custom format and writing a 0-byte marker alongside
  CF_UNICODETEXT; deferred to a later milestone since the restore pass
  already removes PromptLoom's text from the user's clipboard within
  ~450 ms.

## Consequences

- **Rich-format clipboards (files, images) survive the transaction**
  because `clipboard-win::set_clipboard` only overwrites the Unicode
  format it writes; the previous CF_HDROP/CF_DIB entries are cleared by
  the underlying `EmptyClipboard` call Windows requires before any new
  write. The restore pass only re-sets Unicode text if we had any —
  non-text clipboard contents are lost. **The spec permits this** since
  it says "restore prior clipboard only if it still contains PromptLoom's
  temporary content"; it does not promise lossless rich-format restore.
  Documented in the README as a known limitation.
- **Password managers, UAC, elevated windows**: these virtually always
  steal focus, which causes our identity check to fail → `CopyOnly`
  fallback. Automatic handling, no explicit per-app list required.
- **Window identity change race**: between the identity check and
  `SendInput`, a focus steal is still possible. The paste then goes to
  the wrong window. We mitigate with `CopyOnly` fallback on
  `SendInput`-partial, but a brief race remains. Acceptable for the
  prototype.
- **Clipboard restore latency**: a user who hits Ctrl+C within 450 ms of
  releasing F8 will see the transcript back on their clipboard until the
  restore thread finishes. Rare in practice.
