use alloc::collections::VecDeque;
use alloc::format;
use alloc::vec::Vec;

use crate::sys::{self, MouseState, O_CLOEXEC, O_NONBLOCK};

const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_WHEEL: u16 = 8;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const KEY_A: usize = 30;
const EVENT_SIZE: usize = 24;

const MOD_SHIFT: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_CTRL: u8 = 4;

fn ioc(dir: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (0x45 << 8) | nr
}

fn eviocgbit(ev: u64, len: u64) -> u64 {
    ioc(2, 0x20 + ev, len)
}

fn eviocgabs(axis: u64) -> u64 {
    ioc(2, 0x40 + axis, 24)
}

fn has(bits: &[u8], bit: usize) -> bool {
    bits.get(bit / 8).map(|b| b & (1 << (bit % 8)) != 0).unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Mouse,
    Tablet,
}

struct Device {
    fd: u64,
    kind: Kind,
    abs_max: (i32, i32),
}

pub struct Input {
    devices: Vec<Device>,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    buttons: u32,
    presses: [u32; 3],
    wheel: i32,
    shift: bool,
    caps: bool,
    ctrl: bool,
    alt: bool,
    super_down: bool,
    super_used: bool,
    keys: VecDeque<i64>,
}

const LOWER: &[u8; 58] = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
const UPPER: &[u8; 58] = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

impl Input {
    pub fn open(width: i32, height: i32) -> Option<Input> {
        let mut devices = Vec::new();
        for n in 0..32 {
            let fd = sys::open_with(&format!("/dev/input/event{}", n), O_NONBLOCK | O_CLOEXEC);
            if fd < 0 {
                continue;
            }
            let fd = fd as u64;
            let mut ev = [0u8; 8];
            let mut keys = [0u8; 96];
            let mut rel = [0u8; 8];
            let mut abs = [0u8; 8];
            if sys::ioctl(fd, eviocgbit(0, 8), ev.as_mut_ptr()) < 0 {
                sys::close(fd);
                continue;
            }
            sys::ioctl(fd, eviocgbit(EV_KEY as u64, 96), keys.as_mut_ptr());
            sys::ioctl(fd, eviocgbit(EV_REL as u64, 8), rel.as_mut_ptr());
            sys::ioctl(fd, eviocgbit(EV_ABS as u64, 8), abs.as_mut_ptr());
            let kind = if has(&ev, EV_KEY as usize) && has(&keys, KEY_A) {
                Kind::Keyboard
            } else if has(&ev, EV_REL as usize) && has(&rel, REL_X as usize) {
                Kind::Mouse
            } else if has(&ev, EV_ABS as usize) && has(&abs, ABS_X as usize) && has(&keys, BTN_LEFT as usize) {
                Kind::Tablet
            } else if has(&ev, EV_REL as usize) {
                Kind::Mouse
            } else {
                sys::close(fd);
                continue;
            };
            let mut abs_max = (0, 0);
            if kind == Kind::Tablet {
                for (axis, slot) in [(ABS_X, 0), (ABS_Y, 1)] {
                    let mut info = [0u8; 24];
                    if sys::ioctl(fd, eviocgabs(axis as u64), info.as_mut_ptr()) == 0 {
                        let max = i32::from_le_bytes(info[8..12].try_into().unwrap());
                        if slot == 0 {
                            abs_max.0 = max;
                        } else {
                            abs_max.1 = max;
                        }
                    }
                }
            }
            devices.push(Device { fd, kind, abs_max });
        }
        if !devices.iter().any(|d| d.kind == Kind::Keyboard) {
            for d in devices {
                sys::close(d.fd);
            }
            return None;
        }
        Some(Input {
            devices,
            width: width.max(1),
            height: height.max(1),
            x: width / 2,
            y: height / 2,
            buttons: 0,
            presses: [0; 3],
            wheel: 0,
            shift: false,
            caps: false,
            ctrl: false,
            alt: false,
            super_down: false,
            super_used: false,
            keys: VecDeque::new(),
        })
    }

    pub fn fds(&self) -> Vec<u64> {
        self.devices.iter().map(|d| d.fd).collect()
    }

    pub fn set_bounds(&mut self, width: i32, height: i32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.x = self.x.clamp(0, self.width - 1);
        self.y = self.y.clamp(0, self.height - 1);
    }

    pub fn pump(&mut self) {
        let mut buf = [0u8; EVENT_SIZE * 64];
        for i in 0..self.devices.len() {
            loop {
                let n = sys::read(self.devices[i].fd, &mut buf);
                if n <= 0 {
                    break;
                }
                for chunk in buf[..n as usize].chunks_exact(EVENT_SIZE) {
                    let kind = u16::from_le_bytes([chunk[16], chunk[17]]);
                    let code = u16::from_le_bytes([chunk[18], chunk[19]]);
                    let value = i32::from_le_bytes(chunk[20..24].try_into().unwrap());
                    self.event(i, kind, code, value);
                }
                if (n as usize) < buf.len() {
                    break;
                }
            }
        }
    }

    fn event(&mut self, device: usize, kind: u16, code: u16, value: i32) {
        match kind {
            EV_KEY if matches!(code, BTN_LEFT | BTN_RIGHT | BTN_MIDDLE) => self.button(code, value),
            EV_KEY => self.key(code, value),
            EV_REL => match code {
                REL_X => self.x = (self.x + value).clamp(0, self.width - 1),
                REL_Y => self.y = (self.y + value).clamp(0, self.height - 1),
                REL_WHEEL => self.wheel -= value,
                _ => {}
            },
            EV_ABS => {
                let (mx, my) = self.devices[device].abs_max;
                match code {
                    ABS_X if mx > 0 => self.x = (value as i64 * (self.width - 1) as i64 / mx as i64) as i32,
                    ABS_Y if my > 0 => self.y = (value as i64 * (self.height - 1) as i64 / my as i64) as i32,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn button(&mut self, code: u16, value: i32) {
        let (bit, index) = match code {
            BTN_LEFT => (sys::MOUSE_LEFT, 0),
            BTN_RIGHT => (sys::MOUSE_RIGHT, 1),
            _ => (sys::MOUSE_MIDDLE, 2),
        };
        if value != 0 {
            if self.buttons & bit == 0 {
                self.presses[index] += 1;
            }
            self.buttons |= bit;
        } else {
            self.buttons &= !bit;
        }
    }

    pub fn mouse(&mut self, state: &mut MouseState) {
        state.x = self.x;
        state.y = self.y;
        state.buttons = self.buttons | self.presses[0].min(15) << 8 | self.presses[1].min(15) << 12 | self.presses[2].min(15) << 16;
        state.wheel = self.wheel;
        self.presses = [0; 3];
        self.wheel = 0;
    }

    pub fn warp(&mut self, x: i32, y: i32) {
        self.x = x.clamp(0, self.width - 1);
        self.y = y.clamp(0, self.height - 1);
    }

    fn mods(&self) -> u8 {
        let mut m = 0;
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.alt {
            m |= MOD_ALT;
        }
        if self.ctrl {
            m |= MOD_CTRL;
        }
        m
    }

    fn push(&mut self, code: i64, char_like: bool) {
        let mods = if char_like { self.mods() & MOD_ALT } else { self.mods() };
        self.keys.push_back(if mods == 0 { code } else { ((mods as i64) << 32) | (code as u32 as i64) });
    }

    fn key(&mut self, code: u16, value: i32) {
        let down = value != 0;
        match code {
            42 | 54 => {
                self.shift = down;
                return;
            }
            29 | 97 => {
                self.ctrl = down;
                return;
            }
            56 | 100 => {
                self.alt = down;
                return;
            }
            58 => {
                if value == 1 {
                    self.caps = !self.caps;
                }
                return;
            }
            125 | 126 => {
                if value == 1 {
                    self.super_down = true;
                    self.super_used = false;
                } else if value == 0 {
                    self.super_down = false;
                    if !core::mem::replace(&mut self.super_used, false) {
                        self.push(-21, false);
                    }
                }
                return;
            }
            _ => {}
        }
        if !down {
            return;
        }
        let special = match code {
            103 => Some(-1),
            108 => Some(-2),
            105 => Some(-3),
            106 => Some(-4),
            102 => Some(-5),
            107 => Some(-6),
            111 => Some(-7),
            104 => Some(-8),
            109 => Some(-9),
            110 => Some(-53),
            _ => None,
        };
        if let Some(key) = special {
            if self.super_down {
                self.super_used = true;
                let snap = match key {
                    -3 => Some(-23),
                    -4 => Some(-24),
                    -1 => Some(-25),
                    -2 => Some(-26),
                    _ => None,
                };
                if let Some(s) = snap {
                    self.push(s, false);
                    return;
                }
            }
            self.push(key, false);
            return;
        }
        if self.alt && code == 15 {
            self.push(-20, false);
            return;
        }
        if self.alt && code == 62 {
            self.push(-22, false);
            return;
        }
        match code {
            59..=68 => {
                self.push(-40 - (code as i64 - 58), false);
                return;
            }
            87 => {
                self.push(-51, false);
                return;
            }
            88 => {
                self.push(-52, false);
                return;
            }
            96 => {
                self.push(10, false);
                return;
            }
            98 => {
                self.push(b'/' as i64, true);
                return;
            }
            _ => {}
        }
        if self.super_down {
            self.super_used = true;
        }
        let index = code as usize;
        if index >= LOWER.len() {
            return;
        }
        let letter = LOWER[index].is_ascii_alphabetic();
        let upper = if letter { self.shift ^ self.caps } else { self.shift };
        let ch = if upper { UPPER[index] } else { LOWER[index] };
        match ch {
            0 => {}
            b'\n' => self.push(10, false),
            0x08 => self.push(8, false),
            b'\t' => self.push(9, false),
            0x1b => self.push(27, false),
            c if self.ctrl && c.is_ascii_alphabetic() => self.push((c.to_ascii_lowercase() & 0x1f) as i64, false),
            c => self.push(c as i64, true),
        }
    }

    pub fn next_key(&mut self) -> Option<i64> {
        self.keys.pop_front()
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        for d in self.devices.iter() {
            sys::close(d.fd);
        }
    }
}
