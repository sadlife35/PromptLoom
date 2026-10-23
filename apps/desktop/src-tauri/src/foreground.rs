//! Snapshot the foreground window at hotkey-press time so we can refuse
//! delivery if the user tabs away mid-recording.

use promptloom_core::TargetWindow;

#[cfg(windows)]
pub fn capture_foreground() -> Option<TargetWindow> {
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

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

        let exe = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
        .ok()
        .and_then(|handle| {
            let mut buf = [0u16; 260];
            let len = GetModuleBaseNameW(handle, None, &mut buf);
            let _ = CloseHandle(handle);
            if len == 0 {
                None
            } else {
                Some(String::from_utf16_lossy(&buf[..len as usize]))
            }
        })
        .unwrap_or_default();

        Some(TargetWindow {
            hwnd: hwnd.0 as u64,
            process_id: pid,
            executable: exe,
        })
    }
}

#[cfg(not(windows))]
pub fn capture_foreground() -> Option<TargetWindow> {
    // Non-Windows builds (CI, dev) only need the stub.
    Some(TargetWindow {
        hwnd: 0,
        process_id: 0,
        executable: "unknown".into(),
    })
}
