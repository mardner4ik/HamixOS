use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::{Queue, Transport};
use crate::net::dma::DmaRegion;

const EVENTS: usize = 64;
const EV_SYN: u16 = 0;
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
const CFG_ID_NAME: u8 = 1;
const CFG_ABS_INFO: u8 = 0x12;

struct Device {
    transport: Box<dyn Transport>,
    queue: Queue,
    buffers: DmaRegion,
    name: String,
    abs_max: (u32, u32),
    dx: i32,
    dy: i32,
    wheel: i32,
    abs: Option<(u32, u32)>,
    buttons: u8,
    moved: bool,
}

unsafe impl Send for Device {}

static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());

fn extended(code: u16) -> Option<u8> {
    Some(match code {
        96 => 0x1C,
        97 => 0x1D,
        98 => 0x35,
        99 => 0x37,
        100 => 0x38,
        102 => 0x47,
        103 => 0x48,
        104 => 0x49,
        105 => 0x4B,
        106 => 0x4D,
        107 => 0x4F,
        108 => 0x50,
        109 => 0x51,
        110 => 0x52,
        111 => 0x53,
        113 => 0x20,
        114 => 0x2E,
        115 => 0x30,
        125 => 0x5B,
        126 => 0x5C,
        127 => 0x5D,
        _ => return None,
    })
}

fn key(code: u16, value: u32) {
    use crate::drivers::input::keyboard::inject_scancode;
    let release = if value == 0 { 0x80 } else { 0 };
    if let Some(scan) = extended(code) {
        inject_scancode(0xE0);
        inject_scancode(scan | release);
    } else if (1..=88).contains(&code) {
        inject_scancode(code as u8 | release);
    }
}

fn config_string(transport: &dyn Transport, select: u8, subsel: u8) -> String {
    transport.write_config8(0, select);
    transport.write_config8(1, subsel);
    let size = transport.config8(2) as usize;
    (0..size.min(128)).map(|i| transport.config8(8 + i) as char).collect()
}

fn abs_max(transport: &dyn Transport, axis: u8) -> u32 {
    transport.write_config8(0, CFG_ABS_INFO);
    transport.write_config8(1, axis);
    if transport.config8(2) == 0 {
        return 0;
    }
    transport.config32(12)
}

impl Device {
    fn post(&mut self, slot: usize) {
        let phys = self.buffers.phys + (slot * 8) as u64;
        self.queue.push(&[(phys, 8, true)]);
    }

    fn event(&mut self, kind: u16, code: u16, value: u32) {
        match kind {
            EV_KEY => match code {
                BTN_LEFT | BTN_RIGHT | BTN_MIDDLE => {
                    let bit = match code {
                        BTN_LEFT => 1,
                        BTN_RIGHT => 2,
                        _ => 4,
                    };
                    if value != 0 {
                        self.buttons |= bit;
                    } else {
                        self.buttons &= !bit;
                    }
                    self.moved = true;
                }
                _ => key(code, value),
            },
            EV_REL => {
                match code {
                    REL_X => self.dx += value as i32,
                    REL_Y => self.dy += value as i32,
                    REL_WHEEL => self.wheel -= value as i32,
                    _ => {}
                }
                self.moved = true;
            }
            EV_ABS => {
                let (x, y) = self.abs.unwrap_or((0, 0));
                match code {
                    ABS_X => self.abs = Some((value, y)),
                    ABS_Y => self.abs = Some((x, value)),
                    _ => {}
                }
                self.moved = true;
            }
            EV_SYN => self.sync(),
            _ => {}
        }
    }

    fn sync(&mut self) {
        if !self.moved {
            return;
        }
        self.moved = false;
        match self.abs.take() {
            Some((x, y)) => {
                let (w, h) = crate::drivers::input::mouse::bounds();
                let (max_x, max_y) = (self.abs_max.0.max(1) as i64, self.abs_max.1.max(1) as i64);
                let px = (x as i64 * (w as i64 - 1).max(0) / max_x) as i32;
                let py = (y as i64 * (h as i64 - 1).max(0) / max_y) as i32;
                crate::drivers::input::mouse::inject_absolute(px, py, self.buttons, self.wheel);
            }
            None => crate::drivers::input::mouse::inject(self.dx, self.dy, self.buttons, self.wheel),
        }
        self.dx = 0;
        self.dy = 0;
        self.wheel = 0;
    }

    fn drain(&mut self) -> bool {
        let mut any = false;
        while let Some((head, _)) = self.queue.pop() {
            let slot = ((self.queue.address_of(head).saturating_sub(self.buffers.phys)) / 8) as usize;
            let raw = self.buffers.slice(slot * 8, 8);
            let kind = u16::from_le_bytes([raw[0], raw[1]]);
            let code = u16::from_le_bytes([raw[2], raw[3]]);
            let value = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
            self.event(kind, code, value);
            self.post(slot.min(EVENTS - 1));
            any = true;
        }
        if any {
            self.queue.kick(self.transport.as_ref());
        }
        any
    }
}

pub fn init() -> Vec<String> {
    let mut devices = DEVICES.lock();
    for found in super::take(super::ID_INPUT) {
        let mut transport = found.transport;
        if super::negotiate(transport.as_mut(), 0).is_none() {
            continue;
        }
        let Some(queue) = Queue::new(transport.as_mut(), 0, EVENTS as u16) else {
            continue;
        };
        let Some(buffers) = DmaRegion::new(EVENTS * 8) else {
            continue;
        };
        let name = config_string(transport.as_ref(), CFG_ID_NAME, 0);
        let abs = (abs_max(transport.as_ref(), 0), abs_max(transport.as_ref(), 1));
        let mut device = Device { transport, queue, buffers, name, abs_max: abs, dx: 0, dy: 0, wheel: 0, abs: None, buttons: 0, moved: false };
        let count = EVENTS.min(device.queue.size as usize);
        for slot in 0..count {
            device.post(slot);
        }
        super::finish(device.transport.as_mut());
        device.queue.kick(device.transport.as_ref());
        devices.push(device);
    }
    devices.iter().map(|d| d.name.clone()).collect()
}

pub fn poll() {
    let Some(mut devices) = DEVICES.try_lock() else {
        return;
    };
    for device in devices.iter_mut() {
        if device.queue.has_used() {
            device.transport.ack_interrupt();
            device.drain();
        }
    }
}

pub fn present() -> bool {
    !DEVICES.lock().is_empty()
}
