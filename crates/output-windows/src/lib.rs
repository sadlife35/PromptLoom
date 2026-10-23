//! promptloom-output-windows: Windows clipboard transaction + paste fallback.
//!
//! The [`WindowsClipboardSink`] implements the spec's delivery contract:
//!
//! 1. Save prior clipboard contents.
//! 2. Set the dictated text as Unicode.
//! 3. If the current foreground still matches the captured target, send
//!    `Ctrl+V` to it. Otherwise, leave the text on the clipboard and
//!    surface a `CopyOnly` outcome (never injects keystrokes blindly).
//! 4. Restore the prior clipboard in a background thread AFTER a short
//!    delay — but only if the clipboard still contains the text we set
//!    (someone else writing to the clipboard in the meantime wins).

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod foreground;
#[cfg(windows)]
pub mod paste;
#[cfg(windows)]
pub mod sink;

#[cfg(windows)]
pub use sink::WindowsClipboardSink;

use promptloom_core::{DeliveryOutcome, OutputSink, TargetWindow};

/// Determine whether to paste or copy-only based on captured vs. current
/// target window identity. Shared by sinks and tests.
pub fn should_paste(captured: &TargetWindow, current: Option<&TargetWindow>) -> bool {
    match current {
        Some(now) => now.hwnd == captured.hwnd && now.process_id == captured.process_id,
        None => false,
    }
}

/// Null sink retained for cross-platform tests. Records last request and
/// returns a configurable outcome.
pub struct NullOutputSink {
    pub last_text: Option<String>,
    pub last_target: Option<TargetWindow>,
    pub outcome: DeliveryOutcome,
}

impl Default for NullOutputSink {
    fn default() -> Self {
        Self {
            last_text: None,
            last_target: None,
            outcome: DeliveryOutcome::CopyOnly {
                reason: "null sink; no OS integration",
            },
        }
    }
}

impl OutputSink for NullOutputSink {
    fn deliver(&mut self, text: &str, target: &TargetWindow) -> DeliveryOutcome {
        self.last_text = Some(text.to_string());
        self.last_target = Some(target.clone());
        self.outcome.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(hwnd: u64, pid: u32) -> TargetWindow {
        TargetWindow {
            hwnd,
            process_id: pid,
            executable: "x.exe".into(),
        }
    }

    #[test]
    fn paste_when_target_matches() {
        let a = w(1, 10);
        let b = w(1, 10);
        assert!(should_paste(&a, Some(&b)));
    }

    #[test]
    fn copy_only_when_target_changed() {
        let a = w(1, 10);
        let b = w(2, 11);
        assert!(!should_paste(&a, Some(&b)));
    }

    #[test]
    fn copy_only_when_no_current() {
        let a = w(1, 10);
        assert!(!should_paste(&a, None));
    }

    #[test]
    fn copy_only_when_pid_matches_but_hwnd_differs() {
        // Same process, different window (e.g. new browser tab).
        let a = w(1, 10);
        let b = w(2, 10);
        assert!(!should_paste(&a, Some(&b)));
    }

    #[test]
    fn copy_only_when_hwnd_matches_but_pid_differs() {
        // Rare — HWND reused for a new process. Also copy-only.
        let a = w(1, 10);
        let b = w(1, 11);
        assert!(!should_paste(&a, Some(&b)));
    }
}
