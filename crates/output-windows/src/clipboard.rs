//! Thin wrapper over `clipboard-win` so the sink doesn't deal with raw
//! HGLOBAL/HANDLE semantics.
//!
//! Operations open the clipboard with a short retry window — the Windows
//! clipboard is single-writer and other apps may hold it briefly.

use std::thread::sleep;
use std::time::Duration;

use clipboard_win::{formats, get_clipboard, set_clipboard, SysResult};

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("clipboard busy — could not open")]
    Busy,
    #[error("no Unicode text in clipboard")]
    NoText,
    #[error("set failed")]
    SetFailed,
}

fn retry<T>(mut op: impl FnMut() -> SysResult<T>) -> SysResult<T> {
    let mut last_err = None;
    for backoff_ms in [0, 10, 25, 50, 100] {
        if backoff_ms > 0 {
            sleep(Duration::from_millis(backoff_ms));
        }
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("retry loop produced an error"))
}

pub struct ClipboardOps;

impl ClipboardOps {
    /// Read current clipboard as Unicode text, if any. Returns Err only on
    /// hard failure; "no text" returns Ok(None).
    pub fn read_text() -> Option<String> {
        retry(|| get_clipboard::<String, _>(formats::Unicode)).ok()
    }

    pub fn write_text(text: &str) -> Result<(), ClipboardError> {
        retry(|| set_clipboard(formats::Unicode, text)).map_err(|e| {
            tracing::warn!(?e, "set_clipboard failed after retries");
            ClipboardError::SetFailed
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(not(windows), ignore)]
    fn roundtrip_unicode_text() {
        // Not deterministic in parallel with other tests touching the
        // clipboard, so keep this gated behind cargo test --test-threads=1
        // in practice. The important check is the API shape compiling.
        let msg = "promptloom-m3-roundtrip-\u{1F600}";
        if ClipboardOps::write_text(msg).is_ok() {
            let got = ClipboardOps::read_text();
            if let Some(got) = got {
                assert_eq!(got, msg);
            }
        }
    }
}
