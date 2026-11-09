//! Synthesize `Ctrl+V` via `SendInput`.
//!
//! The sink only calls this when the current foreground still matches the
//! HWND we captured at hotkey-press time. Blind input injection is explicitly
//! forbidden by the spec.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_HOME, VK_LEFT, VK_SHIFT,
};

const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);
const VK_A: VIRTUAL_KEY = VIRTUAL_KEY(0x41);
const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);
const VK_X: VIRTUAL_KEY = VIRTUAL_KEY(0x58);
const VK_Y: VIRTUAL_KEY = VIRTUAL_KEY(0x59);
const VK_Z: VIRTUAL_KEY = VIRTUAL_KEY(0x5A);

/// Modifier flags for `send_combo`. Bits can be OR-ed.
#[derive(Debug, Clone, Copy)]
pub struct Mods(pub u8);

impl Mods {
    pub const NONE: Mods = Mods(0);
    pub const CTRL: Mods = Mods(1 << 0);
    pub const SHIFT: Mods = Mods(1 << 1);

    fn has(self, other: Mods) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for Mods {
    type Output = Mods;
    fn bitor(self, rhs: Mods) -> Mods {
        Mods(self.0 | rhs.0)
    }
}

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
    send_combo(VK_V, Mods::CTRL)
}

/// Send one `<mods>+<key>` chord. Press modifiers, press key, release key,
/// release modifiers — the order every mainstream Windows app expects.
pub fn send_combo(vk: VIRTUAL_KEY, mods: Mods) -> Result<(), PasteError> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(8);
    if mods.has(Mods::CTRL) {
        inputs.push(key_event(VK_CONTROL, false));
    }
    if mods.has(Mods::SHIFT) {
        inputs.push(key_event(VK_SHIFT, false));
    }
    inputs.push(key_event(vk, false));
    inputs.push(key_event(vk, true));
    if mods.has(Mods::SHIFT) {
        inputs.push(key_event(VK_SHIFT, true));
    }
    if mods.has(Mods::CTRL) {
        inputs.push(key_event(VK_CONTROL, true));
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    let expected = inputs.len() as u32;
    if sent != expected {
        return Err(PasteError::PartialSend { sent, expected });
    }
    Ok(())
}

/// Virtual-key constants exposed for the selection-command dispatcher.
pub mod keys {
    use super::*;
    pub const SELECT_LEFT: VIRTUAL_KEY = VK_LEFT;
    pub const BACKSPACE: VIRTUAL_KEY = VK_BACK;
    pub const DELETE: VIRTUAL_KEY = VK_DELETE;
    pub const HOME: VIRTUAL_KEY = VK_HOME;
    pub const END: VIRTUAL_KEY = VK_END;
    pub const A: VIRTUAL_KEY = VK_A;
    pub const C: VIRTUAL_KEY = VK_C;
    pub const X: VIRTUAL_KEY = VK_X;
    pub const Y: VIRTUAL_KEY = VK_Y;
    pub const Z: VIRTUAL_KEY = VK_Z;
}
