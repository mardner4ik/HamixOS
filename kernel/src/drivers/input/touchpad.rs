use alloc::format;
use alloc::string::String;
use spin::Mutex;

use super::mouse::{aux_command, read_data, AUX_ACK};

const SET_SCALE11: u8 = 0xE6;
const SET_RESOLUTION: u8 = 0xE8;
const STATUS_REQUEST: u8 = 0xE9;
const SET_RATE: u8 = 0xF3;
const DISABLE_REPORTING: u8 = 0xF5;

const ETP_CUSTOM: u8 = 0xF8;
const ETP_FW_ID_QUERY: u8 = 0x00;
const ETP_FW_VERSION_QUERY: u8 = 0x01;
const ETP_REGISTER_READWRITE: u8 = 0x00;
const ETP_REGISTER_WRITE: u8 = 0x11;

const SYN_IDENTIFY: u8 = 0x00;
const SYN_CAPABILITIES: u8 = 0x02;
const SYN_MODE_ABSOLUTE: u8 = 0x80;
const SYN_MODE_HIGH_RATE: u8 = 0x40;
const SYN_MODE_W: u8 = 0x01;
const SYN_CAP_EXTENDED: u32 = 1 << 23;
const SYN_CAP_MULTIFINGER: u32 = 1 << 1;
const SYN_MIN_PRESSURE: u8 = 25;

const SCREEN_TRAVEL: i32 = 1100;
const SCROLL_STEPS_PER_PAD: i32 = 24;
const TAP_MAX_MS: u64 = 200;
const TAP_MAX_TRAVEL: i32 = 40;

#[derive(Clone, Copy, PartialEq)]
enum Model {
    Synaptics { multifinger: bool },
    ElantechV2 { pressure: bool },
    ElantechV3 { crc: bool },
    ElantechV4 { crc: bool },
}

#[derive(Clone, Copy)]
struct Contact {
    x: i32,
    y: i32,
}

struct Pad {
    model: Model,
    width: i32,
    buf: [u8; 6],
    len: usize,
    fingers: u8,
    last: Option<Contact>,
    touch_started: u64,
    touch_travel: i32,
    touch_fingers: u8,
    touching: bool,
    rest_x: i32,
    rest_y: i32,
    scroll_rest: i32,
    buttons: u8,
    v4_fingers: u8,
    v4_contact: [Contact; 5],
    v4_primary: usize,
}

static PAD: Mutex<Option<Pad>> = Mutex::new(None);

fn ok(command: u8) -> bool {
    aux_command(command) == AUX_ACK
}

fn status() -> [u8; 3] {
    if !ok(STATUS_REQUEST) {
        return [0; 3];
    }
    [read_data(), read_data(), read_data()]
}

fn sliced(value: u8) -> bool {
    if !ok(SET_SCALE11) {
        return false;
    }
    for shift in [6u8, 4, 2, 0] {
        if !ok(SET_RESOLUTION) || !ok((value >> shift) & 3) {
            return false;
        }
    }
    true
}

fn sliced_query(value: u8) -> Option<[u8; 3]> {
    if !sliced(value) {
        return None;
    }
    Some(status())
}

fn custom_query(value: u8) -> Option<[u8; 3]> {
    if !ok(ETP_CUSTOM) || !ok(value) {
        return None;
    }
    Some(status())
}

fn custom_sequence(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| ok(*b))
}

fn elantech_write(model: Model, reg: u8, value: u8) -> bool {
    match model {
        Model::ElantechV2 { .. } => custom_sequence(&[ETP_CUSTOM, ETP_REGISTER_WRITE, ETP_CUSTOM, reg, ETP_CUSTOM, value, SET_SCALE11]),
        Model::ElantechV3 { .. } => custom_sequence(&[ETP_CUSTOM, ETP_REGISTER_READWRITE, ETP_CUSTOM, reg, ETP_CUSTOM, value, SET_SCALE11]),
        Model::ElantechV4 { .. } => custom_sequence(&[
            ETP_CUSTOM,
            ETP_REGISTER_READWRITE,
            ETP_CUSTOM,
            reg,
            ETP_CUSTOM,
            ETP_REGISTER_READWRITE,
            ETP_CUSTOM,
            value,
            SET_SCALE11,
        ]),
        Model::Synaptics { .. } => false,
    }
}

fn detect_elantech() -> Option<(Model, i32, String)> {
    if !ok(DISABLE_REPORTING) || !ok(SET_SCALE11) || !ok(SET_SCALE11) || !ok(SET_SCALE11) {
        return None;
    }
    let magic = status();
    if magic[0] != 0x3C || magic[1] != 0x03 || (magic[2] != 0xC8 && magic[2] != 0x00) {
        return None;
    }
    let version = sliced_query(ETP_FW_VERSION_QUERY)?;
    let fw = (version[0] as u32) << 16 | (version[1] as u32) << 8 | version[2] as u32;
    let family = (fw >> 16) & 0x0F;
    let crc = fw & 0x4000 == 0x4000;
    let model = if fw < 0x020030 || fw == 0x020600 {
        return Some((Model::ElantechV2 { pressure: false }, 0, format!("Elantech touchpad v1 (firmware {:06x}) is not supported, using it as a mouse", fw)));
    } else {
        match family {
            2 | 4 => Model::ElantechV2 { pressure: fw >= 0x020800 },
            5 => Model::ElantechV3 { crc },
            6..=15 => Model::ElantechV4 { crc },
            _ => return Some((Model::ElantechV2 { pressure: false }, 0, format!("Elantech touchpad with unknown firmware {:06x}, using it as a mouse", fw))),
        }
    };
    let width = match model {
        Model::ElantechV2 { .. } => 1152,
        _ => custom_query(ETP_FW_ID_QUERY).map(|id| ((id[0] as i32 & 0x0F) << 8) | id[1] as i32).filter(|w| *w > 100).unwrap_or(1470),
    };
    let written = match model {
        Model::ElantechV2 { .. } => elantech_write(model, 0x10, 0x54) && elantech_write(model, 0x11, 0x88) && elantech_write(model, 0x21, 0x60),
        Model::ElantechV3 { .. } => elantech_write(model, 0x10, 0x0B),
        Model::ElantechV4 { .. } => elantech_write(model, 0x07, 0x01),
        Model::Synaptics { .. } => false,
    };
    if !written {
        return Some((model, 0, format!("Elantech touchpad (firmware {:06x}) refused absolute mode, using it as a mouse", fw)));
    }
    let name = match model {
        Model::ElantechV2 { .. } => "v2",
        Model::ElantechV3 { .. } => "v3",
        _ => "v4",
    };
    Some((model, width, format!("Elantech touchpad {} (firmware {:06x}), two-finger scrolling and tap-to-click", name, fw)))
}

fn detect_synaptics() -> Option<(Model, i32, String)> {
    let id = sliced_query(SYN_IDENTIFY)?;
    if id[1] != 0x47 {
        return None;
    }
    let caps = sliced_query(SYN_CAPABILITIES)?;
    let capabilities = (caps[0] as u32) << 16 | (caps[1] as u32) << 8 | caps[2] as u32;
    let extended = capabilities & SYN_CAP_EXTENDED != 0;
    let multifinger = extended && capabilities & SYN_CAP_MULTIFINGER != 0;
    let mut mode = SYN_MODE_ABSOLUTE | SYN_MODE_HIGH_RATE;
    if extended {
        mode |= SYN_MODE_W;
    }
    if !sliced(mode) || !ok(SET_RATE) || !ok(0x14) {
        return Some((Model::Synaptics { multifinger }, 0, String::from("Synaptics touchpad refused absolute mode, using it as a mouse")));
    }
    let scroll = if multifinger { "two-finger scrolling" } else { "edge scrolling" };
    Some((
        Model::Synaptics { multifinger },
        4000,
        format!("Synaptics touchpad {}.{} ({}, tap-to-click)", id[2] & 0x0F, id[0], scroll),
    ))
}

pub fn detect() -> Option<String> {
    let found = detect_elantech().or_else(|| {
        ok(super::mouse::AUX_SET_DEFAULTS);
        detect_synaptics()
    });
    let Some((model, width, description)) = found else {
        ok(super::mouse::AUX_SET_DEFAULTS);
        return None;
    };
    if width == 0 {
        ok(super::mouse::AUX_SET_DEFAULTS);
        crate::drivers::klog::log(&format!("mouse: {}", description));
        return None;
    }
    *PAD.lock() = Some(Pad {
        model,
        width,
        buf: [0; 6],
        len: 0,
        fingers: 0,
        last: None,
        touch_started: 0,
        touch_travel: 0,
        touch_fingers: 0,
        touching: false,
        rest_x: 0,
        rest_y: 0,
        scroll_rest: 0,
        buttons: 0,
        v4_fingers: 0,
        v4_contact: [Contact { x: 0, y: 0 }; 5],
        v4_primary: 0,
    });
    Some(description)
}

pub fn on_byte(byte: u8) {
    let mut guard = PAD.lock();
    let Some(pad) = guard.as_mut() else {
        return;
    };
    pad.buf[pad.len] = byte;
    pad.len += 1;
    if pad.len < 6 {
        return;
    }
    pad.len = 0;
    let packet = pad.buf;
    if !pad.handle(&packet) {
        pad.buf.copy_within(1..6, 0);
        pad.len = 5;
    }
}

struct Frame {
    fingers: u8,
    contact: Option<Contact>,
    buttons: u8,
}

impl Pad {
    fn handle(&mut self, p: &[u8; 6]) -> bool {
        let frame = match self.model {
            Model::Synaptics { multifinger } => {
                if p[0] & 0xC8 != 0x80 || p[3] & 0xC8 != 0xC0 {
                    return false;
                }
                let w = ((p[0] & 0x30) >> 2) | ((p[0] & 0x04) >> 1) | ((p[3] & 0x04) >> 2);
                if w == 2 {
                    return true;
                }
                let x = ((p[3] as i32 & 0x10) << 8) | ((p[1] as i32 & 0x0F) << 8) | p[4] as i32;
                let y = ((p[3] as i32 & 0x20) << 7) | ((p[1] as i32 & 0xF0) << 4) | p[5] as i32;
                let z = p[2];
                let fingers = if z < SYN_MIN_PRESSURE {
                    0
                } else if multifinger && w == 0 {
                    2
                } else if multifinger && w == 1 {
                    3
                } else {
                    1
                };
                let buttons = p[0] & 0x03;
                let fingers = if !multifinger && fingers == 1 && x > 5472 - 4000 / 12 { 2 } else { fingers };
                Frame { fingers, contact: Some(Contact { x, y: -y }), buttons }
            }
            Model::ElantechV2 { pressure } => {
                if p == &[0x84, 0xFF, 0xFF, 0x02, 0xFF, 0xFF] || p == &[0xC4, 0xFF, 0xFF, 0x02, 0xFF, 0xFF] {
                    return true;
                }
                let valid = if pressure {
                    p[0] & 0x0C == 0x04 && p[3] & 0x0F == 0x02
                } else if p[0] & 0xC0 == 0x80 {
                    p[0] & 0x0C == 0x0C && p[3] & 0x0E == 0x08
                } else {
                    p[0] & 0x3C == 0x3C && p[1] & 0xF0 == 0 && p[3] & 0x3E == 0x38 && p[4] & 0xF0 == 0
                };
                if !valid {
                    return false;
                }
                let fingers = (p[0] & 0xC0) >> 6;
                let contact = match fingers {
                    1 | 3 => Some(Contact { x: ((p[1] as i32 & 0x0F) << 8) | p[2] as i32, y: -(((p[4] as i32 & 0x0F) << 8) | p[5] as i32) }),
                    2 => Some(Contact { x: (((p[0] as i32 & 0x10) << 4) | p[1] as i32) << 2, y: -((((p[0] as i32 & 0x20) << 3) | p[2] as i32) << 2) }),
                    _ => None,
                };
                Frame { fingers, contact, buttons: p[0] & 0x03 }
            }
            Model::ElantechV3 { crc } => {
                if p == &[0xC4, 0xFF, 0xFF, 0x02, 0xFF, 0xFF] {
                    return true;
                }
                let (head, tail) = if crc {
                    (p[3] & 0x09 == 0x08, p[3] & 0x09 == 0x09)
                } else {
                    (p[0] & 0x0C == 0x04 && p[3] & 0xCF == 0x02, p[0] & 0x0C == 0x0C && p[3] & 0xCE == 0x0C)
                };
                if tail {
                    return true;
                }
                if !head {
                    return !crc && p[3] & 0x0F == 0x06;
                }
                let fingers = (p[0] & 0xC0) >> 6;
                let contact = Contact { x: ((p[1] as i32 & 0x0F) << 8) | p[2] as i32, y: -(((p[4] as i32 & 0x0F) << 8) | p[5] as i32) };
                Frame { fingers, contact: if fingers > 0 { Some(contact) } else { None }, buttons: p[0] & 0x03 }
            }
            Model::ElantechV4 { crc } => {
                let sane = if crc { p[3] & 0x08 == 0 } else { p[0] & 0x08 == 0 && p[3] & 0x1C == 0x10 };
                if !sane {
                    return false;
                }
                let buttons = p[0] & 0x03;
                match p[3] & 0x03 {
                    0 => {
                        let mask = p[1] & 0x1F;
                        self.v4_fingers = mask.count_ones() as u8;
                        if mask != 0 && mask & (1 << self.v4_primary) == 0 {
                            self.v4_primary = mask.trailing_zeros() as usize;
                            self.last = None;
                        }
                        let contact = if mask != 0 { Some(self.v4_contact[self.v4_primary]) } else { None };
                        Frame { fingers: self.v4_fingers, contact, buttons }
                    }
                    1 => {
                        let id = ((p[3] & 0xE0) >> 5) as usize;
                        if id == 0 || id > 5 {
                            return true;
                        }
                        self.v4_contact[id - 1] = Contact { x: ((p[1] as i32 & 0x0F) << 8) | p[2] as i32, y: -(((p[4] as i32 & 0x0F) << 8) | p[5] as i32) };
                        Frame { fingers: self.v4_fingers, contact: Some(self.v4_contact[self.v4_primary]), buttons }
                    }
                    2 => {
                        let weight = if p[0] & 0x10 != 0 { 5 } else { 1 };
                        for (id_byte, dx, dy) in [(p[0], p[1], p[2]), (p[3], p[4], p[5])] {
                            let id = ((id_byte & 0xE0) >> 5) as usize;
                            if id == 0 || id > 5 {
                                continue;
                            }
                            self.v4_contact[id - 1].x += dx as i8 as i32 * weight;
                            self.v4_contact[id - 1].y -= dy as i8 as i32 * weight;
                        }
                        Frame { fingers: self.v4_fingers, contact: Some(self.v4_contact[self.v4_primary]), buttons }
                    }
                    _ => return true,
                }
            }
        };
        self.apply(frame);
        true
    }

    fn apply(&mut self, frame: Frame) {
        let now = crate::task::uptime_ms();
        let fingers = if frame.contact.is_none() { 0 } else { frame.fingers.max(1) };
        let mut dx = 0;
        let mut dy = 0;
        let mut wheel = 0;
        if fingers == 0 {
            if self.touching {
                let quick = now.saturating_sub(self.touch_started) <= TAP_MAX_MS;
                let still = self.touch_travel * 1000 / self.width.max(1) <= TAP_MAX_TRAVEL;
                if quick && still && frame.buttons == 0 && self.buttons == 0 {
                    let button = if self.touch_fingers >= 2 { super::mouse::BUTTON_RIGHT } else { super::mouse::BUTTON_LEFT };
                    super::mouse::inject(0, 0, button, 0);
                    super::mouse::inject(0, 0, 0, 0);
                }
            }
            self.touching = false;
            self.last = None;
        } else {
            if !self.touching {
                self.touching = true;
                self.touch_started = now;
                self.touch_travel = 0;
                self.touch_fingers = 0;
            }
            self.touch_fingers = self.touch_fingers.max(fingers);
            let contact = frame.contact.unwrap();
            if fingers != self.fingers {
                self.last = None;
                self.scroll_rest = 0;
            }
            if let Some(last) = self.last {
                let rx = contact.x - last.x;
                let ry = contact.y - last.y;
                self.touch_travel += rx.abs() + ry.abs();
                if fingers >= 2 {
                    self.scroll_rest -= ry * SCROLL_STEPS_PER_PAD;
                    let step = self.width;
                    wheel = self.scroll_rest / step;
                    self.scroll_rest -= wheel * step;
                } else {
                    let speed = rx.abs() + ry.abs();
                    let gain = if speed * 1000 / self.width.max(1) > 12 { 3 } else { 2 };
                    self.rest_x += rx * SCREEN_TRAVEL * gain / 2;
                    self.rest_y += ry * SCREEN_TRAVEL * gain / 2;
                    dx = self.rest_x / self.width;
                    dy = self.rest_y / self.width;
                    self.rest_x -= dx * self.width;
                    self.rest_y -= dy * self.width;
                }
            }
            self.last = Some(contact);
        }
        self.fingers = fingers;
        if dx != 0 || dy != 0 || wheel != 0 || frame.buttons != self.buttons {
            self.buttons = frame.buttons;
            super::mouse::inject(dx, dy, frame.buttons, wheel);
        }
    }
}
