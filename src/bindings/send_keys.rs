use windows::Win32::UI::Input::KeyboardAndMouse::*;
use std::{ mem::size_of, sync::Arc };

use crate::{
    bindings::{ self, key_code::{ KeyCode, SendInputMouseButton } },
    log,
    logger::ActionLog,
};

const XBUTTON1: u32 = 0x0001;
const XBUTTON2: u32 = 0x0002;

fn send_key_down(key: u16, extended: bool) -> INPUT {
    let flags = if extended {
        KEYEVENTF_EXTENDEDKEY | KEYEVENTF_SCANCODE
    } else {
        KEYEVENTF_SCANCODE
    };

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: key,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_key_up(key: u16, extended: bool) -> INPUT {
    let flags = if extended {
        KEYEVENTF_EXTENDEDKEY | KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP
    };

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: key,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn send_keyboard_input(
    logger: Arc<dyn ActionLog>,
    key: KeyCode,
    modifiers: &bindings::Modifier
) {
    let (extended, scan) = match key.keycode_to_scancode() {
        Some((ext, sc)) => (ext, sc),
        None => {
            log!(logger, "❌ No scancode mapping for key code: {:?}", key);
            return;
        }
    };

    let modifier_keys = modifiers.to_key_codes();
    for modifier in &modifier_keys {
        let (mod_extended, mod_scan) = match modifier.keycode_to_scancode() {
            Some((ext, sc)) => (ext, sc),
            None => {
                log!(logger, "❌ No scancode mapping for modifier: {:?}", modifier);
                continue;
            }
        };

        let down_input = send_key_down(mod_scan, mod_extended.is_some());
        if let Err(e) = send_input_event(down_input) {
            log!(logger, "❌ Failed to send key down input for modifier {:?}: {}", modifier, e);
        }
    }

    let down_input = send_key_down(scan, extended.is_some());
    if let Err(e) = send_input_event(down_input) {
        log!(logger, "❌ Failed to send key down input for key {:?}: {}", key, e);
    }

    let up_input = send_key_up(scan, extended.is_some());
    if let Err(e) = send_input_event(up_input) {
        log!(logger, "❌ Failed to send key up input for key {:?}: {}", key, e);
    }

    for modifier in modifier_keys.iter().rev() {
        let (mod_extended, mod_scan) = match modifier.keycode_to_scancode() {
            Some((ext, sc)) => (ext, sc),
            None => {
                log!(logger, "❌ No scancode mapping for modifier: {:?}", modifier);
                continue;
            }
        };

        let up_input = send_key_up(mod_scan, mod_extended.is_some());
        if let Err(e) = send_input_event(up_input) {
            log!(logger, "❌ Failed to send key up input for modifier {:?}: {}", modifier, e);
        }
    }
}

fn create_scancode_input(scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_input_event(input: INPUT) -> Result<(), String> {
    let inputs = [input];
    let result = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if result == 0 {
        Err("SendInput failed".to_string())
    } else {
        Ok(())
    }
}

pub fn send_mouse_input(logger: Arc<dyn ActionLog>, button: SendInputMouseButton) {
    let (down_flag, up_flag, mouse_data) = match button {
        SendInputMouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
        SendInputMouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
        SendInputMouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
        SendInputMouseButton::XButton(x) => {
            let data = match x {
                1 => XBUTTON1,
                2 => XBUTTON2,
                _ => {
                    log!(logger, "⚠️ Unsupported XButton: {}", x);
                    return;
                }
            };
            (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, data)
        }
    };

    let down_input = create_mouse_input(down_flag, mouse_data);
    if let Err(e) = send_input_event(down_input) {
        log!(logger, "❌ Failed to send mouse down input: {}", e);
    }

    let up_input = create_mouse_input(up_flag, mouse_data);
    if let Err(e) = send_input_event(up_input) {
        log!(logger, "❌ Failed to send mouse up input: {}", e);
    }
}

fn create_mouse_input(flags: MOUSE_EVENT_FLAGS, mouse_data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: mouse_data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
