use alloc::vec::Vec;

use crate::{Modes, MouseEncoding, MouseMode};

pub const MOD_SHIFT: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_CTRL: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    F(u8),
}

fn push_num(out: &mut Vec<u8>, n: usize) {
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut v = n;
    loop {
        digits[len] = b'0' + (v % 10) as u8;
        len += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for i in (0..len).rev() {
        out.push(digits[i]);
    }
}

fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '@' | ' ' | '2' | '`' => 0,
        '[' | '3' | '{' => 27,
        '\\' | '4' | '|' => 28,
        ']' | '5' | '}' => 29,
        '^' | '6' | '~' => 30,
        '_' | '/' | '-' | '7' => 31,
        '8' | '?' => 127,
        _ => return None,
    })
}

fn csi_final(out: &mut Vec<u8>, modes: &Modes, mods: u8, final_byte: u8) {
    if mods == 0 {
        out.extend_from_slice(if modes.app_cursor { b"\x1bO" } else { b"\x1b[" });
    } else {
        out.extend_from_slice(b"\x1b[1;");
        push_num(out, mods as usize + 1);
    }
    out.push(final_byte);
}

fn csi_tilde(out: &mut Vec<u8>, mods: u8, code: usize) {
    out.extend_from_slice(b"\x1b[");
    push_num(out, code);
    if mods != 0 {
        out.push(b';');
        push_num(out, mods as usize + 1);
    }
    out.push(b'~');
}

pub fn encode_key(key: Key, mods: u8, modes: &Modes) -> Vec<u8> {
    let mut out = Vec::new();
    let alt = mods & MOD_ALT != 0;
    let ctrl = mods & MOD_CTRL != 0;
    let shift = mods & MOD_SHIFT != 0;
    match key {
        Key::Char(c) => {
            if alt {
                out.push(0x1b);
            }
            if ctrl {
                if let Some(b) = ctrl_byte(c) {
                    out.push(b);
                    return out;
                }
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        Key::Enter => {
            if alt {
                out.push(0x1b);
            }
            out.extend_from_slice(if modes.newline { b"\r\n" } else { b"\r" });
        }
        Key::Backspace => {
            if alt {
                out.push(0x1b);
            }
            out.push(if ctrl { 0x08 } else { 0x7f });
        }
        Key::Tab => {
            if shift {
                out.extend_from_slice(b"\x1b[Z");
            } else {
                if alt {
                    out.push(0x1b);
                }
                out.push(b'\t');
            }
        }
        Key::Escape => {
            if alt {
                out.push(0x1b);
            }
            out.push(0x1b);
        }
        Key::Up => csi_final(&mut out, modes, mods, b'A'),
        Key::Down => csi_final(&mut out, modes, mods, b'B'),
        Key::Right => csi_final(&mut out, modes, mods, b'C'),
        Key::Left => csi_final(&mut out, modes, mods, b'D'),
        Key::Home => csi_final(&mut out, modes, mods, b'H'),
        Key::End => csi_final(&mut out, modes, mods, b'F'),
        Key::Insert => csi_tilde(&mut out, mods, 2),
        Key::Delete => csi_tilde(&mut out, mods, 3),
        Key::PageUp => csi_tilde(&mut out, mods, 5),
        Key::PageDown => csi_tilde(&mut out, mods, 6),
        Key::F(n @ 1..=4) => {
            if mods == 0 {
                out.extend_from_slice(b"\x1bO");
            } else {
                out.extend_from_slice(b"\x1b[1;");
                push_num(&mut out, mods as usize + 1);
            }
            out.push(b'P' + n - 1);
        }
        Key::F(n) => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => return out,
            };
            csi_tilde(&mut out, mods, code);
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseEvent {
    Press(MouseButton),
    Release(MouseButton),
    Move(Option<MouseButton>),
    WheelUp,
    WheelDown,
}

fn button_code(button: MouseButton) -> usize {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

pub fn encode_mouse(modes: &Modes, event: MouseEvent, col: usize, row: usize, mods: u8) -> Option<Vec<u8>> {
    let mode = modes.mouse;
    if mode == MouseMode::Off {
        return None;
    }
    let (mut code, release) = match event {
        MouseEvent::Press(b) => (button_code(b), false),
        MouseEvent::Release(b) => {
            if mode == MouseMode::X10 {
                return None;
            }
            (if modes.mouse_encoding == MouseEncoding::Sgr { button_code(b) } else { 3 }, true)
        }
        MouseEvent::Move(held) => {
            match (mode, held) {
                (MouseMode::Any, _) | (MouseMode::Button, Some(_)) => {}
                _ => return None,
            }
            (32 + held.map(button_code).unwrap_or(3), false)
        }
        MouseEvent::WheelUp => {
            if mode == MouseMode::X10 {
                return None;
            }
            (64, false)
        }
        MouseEvent::WheelDown => {
            if mode == MouseMode::X10 {
                return None;
            }
            (65, false)
        }
    };
    if mode != MouseMode::X10 {
        if mods & MOD_SHIFT != 0 {
            code += 4;
        }
        if mods & MOD_ALT != 0 {
            code += 8;
        }
        if mods & MOD_CTRL != 0 {
            code += 16;
        }
    }
    let x = col + 1;
    let y = row + 1;
    let mut out = Vec::new();
    match modes.mouse_encoding {
        MouseEncoding::Sgr => {
            out.extend_from_slice(b"\x1b[<");
            push_num(&mut out, code);
            out.push(b';');
            push_num(&mut out, x);
            out.push(b';');
            push_num(&mut out, y);
            out.push(if release { b'm' } else { b'M' });
        }
        MouseEncoding::Urxvt => {
            out.extend_from_slice(b"\x1b[");
            push_num(&mut out, code + 32);
            out.push(b';');
            push_num(&mut out, x);
            out.push(b';');
            push_num(&mut out, y);
            out.push(b'M');
        }
        MouseEncoding::Utf8 => {
            out.extend_from_slice(b"\x1b[M");
            for v in [code + 32, x + 32, y + 32] {
                let ch = char::from_u32(v.min(2047) as u32).unwrap_or(' ');
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
        MouseEncoding::Default => {
            if x > 223 || y > 223 {
                return None;
            }
            out.extend_from_slice(b"\x1b[M");
            out.push((code + 32) as u8);
            out.push((x + 32) as u8);
            out.push((y + 32) as u8);
        }
    }
    Some(out)
}

pub fn encode_focus(modes: &Modes, focused: bool) -> Option<&'static [u8]> {
    if !modes.focus_events {
        return None;
    }
    Some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}

pub fn encode_paste(modes: &Modes, text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    if modes.bracketed_paste {
        out.extend_from_slice(b"\x1b[200~");
    }
    for b in text.bytes() {
        match b {
            b'\n' => out.push(b'\r'),
            0x1b if modes.bracketed_paste => {}
            other => out.push(other),
        }
    }
    if modes.bracketed_paste {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}
