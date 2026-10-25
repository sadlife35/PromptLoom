//! Synthesize `Ctrl+V` via `SendInput`.
//!
//! The sink only calls this when the current foreground still matches the
//! HWND we captured at hotkey-press time. Blind input injection is explicitly
//! forbidden by the spec.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL,
};

const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

#[derive(Debug, thiserror::Error)]
pub enum PasteError {
    #[error("SendInput reported {sent} of {expected} events delivered")]
    PartialSend { sent: u32, expected: u32 },
}

fn key_event(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn send_ctrl_v() -> Result<(), PasteError> {
    let inputs = [
        key_event(VK_CONTROL, false),
        key_event(VK_V, false),
        key_event(VK_V, true),
        key_event(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    let expected = inputs.len() as u32;
    if sent != expected {
        return Err(PasteError::PartialSend { sent, expected });
    }
    Ok(())
}
