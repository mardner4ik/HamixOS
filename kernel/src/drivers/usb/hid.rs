use crate::drivers::input::{keyboard, mouse};

pub fn on_boot_mouse_report(report: &[u8]) {
    if report.len() < 3 {
        return;
    }
    let buttons = report[0] & 0x07;
    let dx = report[1] as i8 as i32;
    let dy = report[2] as i8 as i32;
    let wheel = if report.len() >= 4 { -(report[3] as i8 as i32) } else { 0 };
    mouse::inject(dx, dy, buttons, wheel);
}

fn usage_to_set1(usage: u8) -> Option<(bool, u8)> {
    const TABLE: &[u8] = &[
        0, 0, 0, 0, 0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19,
        0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
        0x0A, 0x0B, 0x1C, 0x01, 0x0E, 0x0F, 0x39, 0x0C, 0x0D, 0x1A, 0x1B, 0x2B, 0x2B, 0x27, 0x28, 0x29, 0x33, 0x34,
        0x35, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x57, 0x58, 0, 0x46, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0x45, 0x35, 0x37, 0x4A, 0x4E, 0x1C, 0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48,
        0x49, 0x52, 0x53, 0x56, 0,
    ];
    match usage {
        0x49 => Some((true, 0x52)),
        0x4A => Some((true, 0x47)),
        0x4B => Some((true, 0x49)),
        0x4C => Some((true, 0x53)),
        0x4D => Some((true, 0x4F)),
        0x4E => Some((true, 0x51)),
        0x4F => Some((true, 0x4D)),
        0x50 => Some((true, 0x4B)),
        0x51 => Some((true, 0x50)),
        0x52 => Some((true, 0x48)),
        0x7F => Some((true, 0x20)),
        0x80 => Some((true, 0x30)),
        0x81 => Some((true, 0x2E)),
        u if (u as usize) < TABLE.len() && TABLE[u as usize] != 0 => Some((false, TABLE[u as usize])),
        _ => None,
    }
}

fn send(usage: u8, pressed: bool) {
    if let Some((extended, code)) = usage_to_set1(usage) {
        if extended {
            keyboard::inject_scancode(0xE0);
        }
        keyboard::inject_scancode(if pressed { code } else { code | 0x80 });
    }
}

const MODIFIERS: [u8; 8] = [0x1D, 0x2A, 0x38, 0x5B, 0x1D, 0x36, 0x38, 0x5C];

pub fn on_boot_keyboard_report(report: &[u8], previous: &mut [u8; 8]) {
    if report.len() < 8 || report[2] == 1 {
        return;
    }
    let changed = report[0] ^ previous[0];
    for bit in 0..8 {
        if changed & (1 << bit) != 0 {
            let pressed = report[0] & (1 << bit) != 0;
            let code = MODIFIERS[bit];
            if matches!(bit, 3 | 4 | 6 | 7) {
                keyboard::inject_scancode(0xE0);
            }
            keyboard::inject_scancode(if pressed { code } else { code | 0x80 });
        }
    }
    for &old in &previous[2..8] {
        if old != 0 && !report[2..8].contains(&old) {
            send(old, false);
        }
    }
    for &new in &report[2..8] {
        if new != 0 && !previous[2..8].contains(&new) {
            send(new, true);
        }
    }
    previous.copy_from_slice(&report[..8]);
}
