//! `WindowsClipboardSink` — the real M3 delivery path.

use std::time::Duration;

use promptloom_core::{DeliveryOutcome, OutputSink, TargetWindow};

use crate::clipboard::ClipboardOps;
use crate::foreground::current_foreground;
use crate::paste::send_ctrl_v;
use crate::should_paste;

pub struct WindowsClipboardSink {
    /// How long to wait after sending Ctrl+V before attempting to restore
    /// the prior clipboard. The target app needs this window to consume the
    /// paste; too short and we restore before it's read; too long and the
    /// user sees PromptLoom text in their next unrelated paste.
    pub restore_delay: Duration,
}

impl Default for WindowsClipboardSink {
    fn default() -> Self {
        Self {
            restore_delay: Duration::from_millis(450),
        }
    }
}

impl OutputSink for WindowsClipboardSink {
    fn deliver(&mut self, text: &str, target: &TargetWindow) -> DeliveryOutcome {
        if text.is_empty() {
            return DeliveryOutcome::CopyOnly {
                reason: "empty transcript",
            };
        }

        // 1. Save prior clipboard text (if any). We only save Unicode text;
        //    richer formats (CF_HDROP, CF_DIB, etc.) are left untouched by
        //    this transaction and will survive it via EmptyClipboard being
        //    scoped — see note below.
        let prior = ClipboardOps::read_text();

        // 2. Set the dictated text as the new Unicode clipboard content.
        if let Err(e) = ClipboardOps::write_text(text) {
            tracing::warn!(?e, "clipboard write failed; delivery failed");
            return DeliveryOutcome::Failed;
        }

        // 3. Confirm foreground still matches the captured target. If not,
        //    the user tabbed away during transcription — never inject
        //    keystrokes blindly. Leave the transcript on the clipboard so
        //    the user can paste it themselves.
        let current = current_foreground();
        let aligned = should_paste(target, current.as_ref());
        if !aligned {
            tracing::info!(
                captured_hwnd = target.hwnd,
                current_hwnd = ?current.as_ref().map(|c| c.hwnd),
                "target focus changed; copy-only (clipboard preserved for manual paste)"
            );
            return DeliveryOutcome::CopyOnly {
                reason: if current.is_none() {
                    "no foreground window at delivery time"
                } else {
                    "focus changed since recording"
                },
            };
        }

        // 4. Send Ctrl+V. If the OS reports partial injection, fall back to
        //    copy-only rather than retrying (another app may have grabbed
        //    focus between our check and the SendInput call).
        if let Err(e) = send_ctrl_v() {
            tracing::warn!(?e, "SendInput Ctrl+V failed; copy-only");
            return DeliveryOutcome::CopyOnly {
                reason: "paste injection failed",
            };
        }

        // 5. Restore prior clipboard in a background thread, but ONLY if the
        //    clipboard still contains the text we set. If anything else has
        //    written to the clipboard since (another app, the user, …) we
        //    leave it alone — their write wins.
        let text_owned = text.to_string();
        let prior_owned = prior;
        let delay = self.restore_delay;
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            match ClipboardOps::read_text() {
                Some(now) if now == text_owned => {
                    if let Some(prior_text) = prior_owned {
                        let _ = ClipboardOps::write_text(&prior_text);
                    }
                    // else: there was no prior text; leaving our text is
                    // acceptable (it's what the user just dictated).
                }
                _ => {
                    // Clipboard changed between paste and restore: do
                    // nothing. Spec: "restore only if it still contains
                    // PromptLoom's temporary content".
                }
            }
        });

        DeliveryOutcome::Pasted
    }
}
