//! Snapshot the CURRENT foreground window identity to compare against the
//! one captured at hotkey-press time.
//!
//! Mirrors `apps/desktop/src-tauri/src/foreground.rs` but lives here so the
//! sink doesn't need to call back into the desktop crate.

use promptloom_core::TargetWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub fn current_foreground() -> Option<TargetWindow> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        Some(TargetWindow {
            hwnd: hwnd.0 as u64,
            process_id: pid,
            // Executable isn't needed for the identity check; leave empty.
            executable: String::new(),
        })
    }
}
