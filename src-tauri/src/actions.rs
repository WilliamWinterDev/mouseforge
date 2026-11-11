//! Synthesising keyboard / mouse input with SendInput.

use crate::config::MacroStep;
use std::mem::size_of;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

/// Our injected events carry this tag so the hooks can ignore them.
pub const MAGIC: usize = 0x4D46_4F52; // "MFOR"

/// Parse a key name like "a", "F13", "ctrl", "pagedown", "volume_up" into a virtual key code.
pub fn vk_from_name(name: &str) -> Option<u16> {
    let n = name.trim().to_lowercase();
    let n = n.as_str();
    if n.len() == 1 {
        let c = n.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            return Some(c as u16);
        }
    }
    if let Some(num) = n.strip_prefix('f').and_then(|r| r.parse::<u16>().ok()) {
        if (1..=24).contains(&num) {
            return Some(VK_F1 + num - 1);
        }
    }
    if let Some(num) = n.strip_prefix("num").and_then(|r| r.parse::<u16>().ok()) {
        if num <= 9 {
            return Some(VK_NUMPAD0 + num);
        }
    }
    Some(match n {
        "ctrl" | "control" => VK_CONTROL,
        "shift" => VK_SHIFT,
        "alt" => VK_MENU,
        "win" | "meta" | "super" => VK_LWIN,
        "enter" | "return" => VK_RETURN,
        "esc" | "escape" => VK_ESCAPE,
        "tab" => VK_TAB,
        "space" => VK_SPACE,
        "backspace" => VK_BACK,
        "delete" | "del" => VK_DELETE,
        "insert" | "ins" => VK_INSERT,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" | "pgup" => VK_PRIOR,
        "pagedown" | "pgdn" => VK_NEXT,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "printscreen" | "prtsc" => VK_SNAPSHOT,
        "capslock" => VK_CAPITAL,
        "scrolllock" => VK_SCROLL,
        "pause" => VK_PAUSE,
        "menu" | "apps" => VK_APPS,
        "minus" | "-" => VK_OEM_MINUS,
        "equals" | "=" => VK_OEM_PLUS,
        "[" => VK_OEM_4,
        "]" => VK_OEM_6,
        ";" => VK_OEM_1,
        "'" => VK_OEM_7,
        "," => VK_OEM_COMMA,
        "." => VK_OEM_PERIOD,
        "/" => VK_OEM_2,
        "\\" => VK_OEM_5,
        "`" => VK_OEM_3,
        "nummultiply" => VK_MULTIPLY,
        "numadd" => VK_ADD,
        "numsubtract" => VK_SUBTRACT,
        "numdecimal" => VK_DECIMAL,
        "numdivide" => VK_DIVIDE,
        "play_pause" => VK_MEDIA_PLAY_PAUSE,
        "next" => VK_MEDIA_NEXT_TRACK,
        "prev" => VK_MEDIA_PREV_TRACK,
        "stop" => VK_MEDIA_STOP,
        "volume_up" => VK_VOLUME_UP,
        "volume_down" => VK_VOLUME_DOWN,
        "mute" => VK_VOLUME_MUTE,
        _ => return None,
    })
}

/// "Ctrl+Shift+T" -> [VK_CONTROL, VK_SHIFT, 'T']. Splits on '+', but a trailing
/// "+" (as in "Ctrl++") means the plus/equals key.
pub fn parse_combo(combo: &str) -> Vec<u16> {
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    if combo.trim_end().ends_with("++") {
        parts.retain(|p| !p.is_empty());
        parts.push("=");
    }
    parts.into_iter().filter(|p| !p.is_empty()).filter_map(vk_from_name).collect()
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        VK_INSERT | VK_DELETE | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_UP | VK_DOWN | VK_LEFT
            | VK_RIGHT | VK_LWIN | VK_RWIN | VK_APPS | VK_DIVIDE | VK_SNAPSHOT
            | VK_MEDIA_PLAY_PAUSE | VK_MEDIA_NEXT_TRACK | VK_MEDIA_PREV_TRACK | VK_MEDIA_STOP
            | VK_VOLUME_UP | VK_VOLUME_DOWN | VK_VOLUME_MUTE
    )
}

fn key_input(vk: u16, up: bool) -> INPUT {
    let mut flags = 0;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: MAGIC },
        },
    }
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
    }
}

pub fn keys_down(vks: &[u16]) {
    send(&vks.iter().map(|&v| key_input(v, false)).collect::<Vec<_>>());
}

pub fn keys_up(vks: &[u16]) {
    send(&vks.iter().rev().map(|&v| key_input(v, true)).collect::<Vec<_>>());
}

pub fn tap_combo(combo: &str) {
    let vks = parse_combo(combo);
    keys_down(&vks);
    keys_up(&vks);
}

pub fn type_text(text: &str) {
    let mut inputs = Vec::new();
    for unit in text.replace("\r\n", "\n").encode_utf16() {
        if unit == '\n' as u16 {
            inputs.push(key_input(VK_RETURN, false));
            inputs.push(key_input(VK_RETURN, true));
            continue;
        }
        for up in [false, true] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                        time: 0,
                        dwExtraInfo: MAGIC,
                    },
                },
            });
        }
    }
    send(&inputs);
}

pub fn mouse_button(button: &str, up: bool) {
    let (down_flag, up_flag, data) = match button {
        "left" => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
        "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
        "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
        "back" => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, 1),
        "forward" => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, 2),
        _ => return,
    };
    send(&[INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: if up { up_flag } else { down_flag },
                time: 0,
                dwExtraInfo: MAGIC,
            },
        },
    }]);
}

pub fn run_macro(steps: &[MacroStep]) {
    let mut held: Vec<u16> = Vec::new();
    for step in steps {
        match step {
            MacroStep::Tap { combo } => tap_combo(combo),
            MacroStep::Down { key } => {
                if let Some(vk) = vk_from_name(key) {
                    keys_down(&[vk]);
                    held.push(vk);
                }
            }
            MacroStep::Up { key } => {
                if let Some(vk) = vk_from_name(key) {
                    keys_up(&[vk]);
                    held.retain(|&h| h != vk);
                }
            }
            MacroStep::Delay { ms } => std::thread::sleep(std::time::Duration::from_millis((*ms).min(60_000))),
            MacroStep::Text { text } => type_text(text),
        }
    }
    // Never leave keys stuck down.
    keys_up(&held);
}

pub fn launch(path: &str, args: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (op, file, params) = (wide("open"), wide(path.trim().trim_matches('"')), wide(args));
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            if args.is_empty() { std::ptr::null() } else { params.as_ptr() },
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}
