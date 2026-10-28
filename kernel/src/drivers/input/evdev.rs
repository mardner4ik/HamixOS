use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::arch::without_interrupts;

pub const KEYBOARD: usize = 0;
pub const MOUSE: usize = 1;
pub const TABLET: usize = 2;
pub const COUNT: usize = 3;
pub const MAJOR: u32 = 13;
pub const FIRST_MINOR: u32 = 64;
pub const TABLET_MAX: i32 = 32767;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const EV_MSC: u16 = 4;
const EV_LED: u16 = 0x11;
const EV_REP: u16 = 0x14;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_WHEEL: u16 = 8;
const REL_WHEEL_HI_RES: u16 = 11;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const MSC_SCAN: u16 = 4;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const KEY_CNT: usize = 0x300;
const RING: usize = 512;
const EVENT_SIZE: usize = 24;

use crate::errno::EINVAL;
use crate::errno::ENOTTY;
use crate::errno::EAGAIN;
use crate::errno::EBUSY;
use crate::errno::EFAULT;

pub struct Info {
    pub name: &'static str,
    pub phys: &'static str,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

pub const DEVICES: [Info; COUNT] = [
    Info { name: "AT Translated Set 2 keyboard", phys: "isa0060/serio0/input0", bus: 0x11, vendor: 1, product: 1, version: 0xab41 },
    Info { name: "ImPS/2 Generic Wheel Mouse", phys: "isa0060/serio1/input0", bus: 0x11, vendor: 2, product: 3, version: 0 },
    Info { name: "HamixOS Absolute Pointer", phys: "hamix/tablet0/input0", bus: 0x06, vendor: 0x627, product: 1, version: 1 },
];

fn set(bits: &mut [u64], bit: usize) {
    bits[bit / 64] |= 1 << (bit % 64);
}

pub fn ev_bits(device: usize) -> u64 {
    match device {
        KEYBOARD => (1 << EV_SYN) | (1 << EV_KEY) | (1 << EV_MSC) | (1 << EV_LED) | (1 << EV_REP),
        MOUSE => (1 << EV_SYN) | (1 << EV_KEY) | (1 << EV_REL),
        _ => (1 << EV_SYN) | (1 << EV_KEY) | (1 << EV_ABS),
    }
}

pub fn key_bits(device: usize) -> [u64; KEY_CNT / 64] {
    let mut bits = [0u64; KEY_CNT / 64];
    if device == KEYBOARD {
        for key in 1..=88 {
            set(&mut bits, key);
        }
        for key in [96, 97, 98, 99, 100, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 113, 114, 115, 116, 119, 125, 126, 127, 142, 143, 158, 163, 164, 165, 172] {
            set(&mut bits, key);
        }
    } else {
        for key in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
            set(&mut bits, key as usize);
        }
    }
    bits
}

pub fn rel_bits(device: usize) -> u64 {
    if device == MOUSE { (1 << REL_X) | (1 << REL_Y) | (1 << REL_WHEEL) | (1 << REL_WHEEL_HI_RES) } else { 0 }
}

pub fn abs_bits(device: usize) -> u64 {
    if device == TABLET { (1 << ABS_X) | (1 << ABS_Y) } else { 0 }
}

fn msc_bits(device: usize) -> u64 {
    if device == KEYBOARD { 1 << MSC_SCAN } else { 0 }
}

fn led_bits(device: usize) -> u64 {
    if device == KEYBOARD { 0x7 } else { 0 }
}

pub fn hex_words(words: &[u64]) -> String {
    let mut last = words.len();
    while last > 1 && words[last - 1] == 0 {
        last -= 1;
    }
    let parts: Vec<String> = words[..last].iter().rev().map(|w| format!("{:x}", w)).collect();
    parts.join(" ")
}

struct Client {
    device: usize,
    vt: usize,
    grab: bool,
    realtime: bool,
    ring: Box<[[u8; EVENT_SIZE]; RING]>,
    head: usize,
    len: usize,
}

impl Client {
    fn push(&mut self, event: [u8; EVENT_SIZE]) {
        if self.len == RING {
            self.head = 0;
            self.len = 0;
            let mut dropped = event;
            dropped[16..24].fill(0);
            dropped[16..18].copy_from_slice(&EV_SYN.to_le_bytes());
            dropped[18..20].copy_from_slice(&SYN_DROPPED.to_le_bytes());
            self.ring[0] = dropped;
            self.len = 1;
        }
        let at = (self.head + self.len) % RING;
        self.ring[at] = event;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<[u8; EVENT_SIZE]> {
        if self.len == 0 {
            return None;
        }
        let e = self.ring[self.head];
        self.head = (self.head + 1) % RING;
        self.len -= 1;
        Some(e)
    }
}

static CLIENTS: Mutex<BTreeMap<u32, Client>> = Mutex::new(BTreeMap::new());
static KEYS: Mutex<[u64; KEY_CNT / 64]> = Mutex::new([0; KEY_CNT / 64]);
static TABLET_POS: Mutex<(i32, i32)> = Mutex::new((0, 0));
static REPEAT: Mutex<(i32, i32)> = Mutex::new((250, 33));
static LEDS: AtomicU64 = AtomicU64::new(0);
static EXTENDED: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut BTreeMap<u32, Client>) -> R) -> R {
    without_interrupts(|| f(&mut CLIENTS.lock()))
}

fn encode(realtime: bool, kind: u16, code: u16, value: i32) -> [u8; EVENT_SIZE] {
    let ms = crate::task::uptime_ms();
    let secs = if realtime { crate::drivers::rtc::boot_epoch() + ms / 1000 } else { ms / 1000 };
    let mut e = [0u8; EVENT_SIZE];
    e[0..8].copy_from_slice(&secs.to_le_bytes());
    e[8..16].copy_from_slice(&((ms % 1000) * 1000).to_le_bytes());
    e[16..18].copy_from_slice(&kind.to_le_bytes());
    e[18..20].copy_from_slice(&code.to_le_bytes());
    e[20..24].copy_from_slice(&value.to_le_bytes());
    e
}

fn emit(device: usize, events: &[(u16, u16, i32)]) {
    let active = crate::tty::vt::foreground();
    let delivered = without_interrupts(|| {
        let mut clients = CLIENTS.lock();
        let grabbed = clients.values().any(|c| c.device == device && c.grab);
        let mut any = false;
        for client in clients.values_mut() {
            if client.device != device || client.vt != active || (grabbed && !client.grab) {
                continue;
            }
            for &(kind, code, value) in events {
                client.push(encode(client.realtime, kind, code, value));
            }
            client.push(encode(client.realtime, EV_SYN, SYN_REPORT, 0));
            any = true;
        }
        any
    });
    if delivered {
        crate::task::notify_input();
    }
}

pub fn claimed(device: usize) -> bool {
    let active = crate::tty::vt::foreground();
    without_interrupts(|| CLIENTS.lock().values().any(|c| c.device == device && c.vt == active))
}

fn extended_key(make: u8) -> Option<u16> {
    Some(match make {
        0x1C => 96,
        0x1D => 97,
        0x35 => 98,
        0x37 => 99,
        0x38 => 100,
        0x47 => 102,
        0x48 => 103,
        0x49 => 104,
        0x4B => 105,
        0x4D => 106,
        0x4F => 107,
        0x50 => 108,
        0x51 => 109,
        0x52 => 110,
        0x53 => 111,
        0x20 => 113,
        0x2E => 114,
        0x30 => 115,
        0x5E => 116,
        0x5B => 125,
        0x5C => 126,
        0x5D => 127,
        0x6C => 142,
        0x5F => 142,
        0x63 => 143,
        0x6A => 158,
        0x19 => 163,
        0x22 => 164,
        0x10 => 165,
        0x32 => 172,
        _ => return None,
    })
}

pub fn on_scancode(scancode: u8) {
    if scancode == 0xE0 {
        EXTENDED.store(true, Ordering::Relaxed);
        return;
    }
    if scancode == 0xE1 {
        return;
    }
    let extended = EXTENDED.swap(false, Ordering::Relaxed);
    let release = scancode & 0x80 != 0;
    let make = scancode & 0x7F;
    let key = if extended {
        if make == 0x2A || make == 0x36 {
            return;
        }
        match extended_key(make) {
            Some(k) => k,
            None => return,
        }
    } else {
        if make == 0 || make > 0x58 {
            return;
        }
        make as u16
    };
    let value = without_interrupts(|| {
        let mut keys = KEYS.lock();
        let word = key as usize / 64;
        let bit = 1u64 << (key as usize % 64);
        let down = keys[word] & bit != 0;
        if release {
            keys[word] &= !bit;
            0
        } else {
            keys[word] |= bit;
            if down { 2 } else { 1 }
        }
    });
    let scan = if extended { 0xE000 | make as i32 } else { make as i32 };
    emit(KEYBOARD, &[(EV_MSC, MSC_SCAN, scan), (EV_KEY, key, value)]);
}

struct Batch {
    items: [(u16, u16, i32); 8],
    len: usize,
}

impl Batch {
    fn new() -> Self {
        Self { items: [(0, 0, 0); 8], len: 0 }
    }

    fn push(&mut self, item: (u16, u16, i32)) {
        if self.len < self.items.len() {
            self.items[self.len] = item;
            self.len += 1;
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn events(&self) -> &[(u16, u16, i32)] {
        &self.items[..self.len]
    }
}

fn button_events(previous: u8, buttons: u8, out: &mut Batch) {
    for (bit, code) in [(1u8, BTN_LEFT), (2, BTN_RIGHT), (4, BTN_MIDDLE)] {
        if (previous ^ buttons) & bit != 0 {
            out.push((EV_KEY, code, (buttons & bit != 0) as i32));
        }
    }
}

pub fn on_relative(dx: i32, dy: i32, previous: u8, buttons: u8, wheel: i32) {
    let mut events = Batch::new();
    if dx != 0 {
        events.push((EV_REL, REL_X, dx));
    }
    if dy != 0 {
        events.push((EV_REL, REL_Y, dy));
    }
    if wheel != 0 {
        events.push((EV_REL, REL_WHEEL, -wheel));
        events.push((EV_REL, REL_WHEEL_HI_RES, -wheel * 120));
    }
    button_events(previous, buttons, &mut events);
    if !events.is_empty() {
        emit(MOUSE, events.events());
    }
}

pub fn on_absolute(x: i32, y: i32, width: i32, height: i32, previous: u8, buttons: u8, wheel: i32) {
    let ax = (x.clamp(0, width.max(1) - 1) as i64 * TABLET_MAX as i64 / (width.max(2) - 1) as i64) as i32;
    let ay = (y.clamp(0, height.max(1) - 1) as i64 * TABLET_MAX as i64 / (height.max(2) - 1) as i64) as i32;
    let mut events = Batch::new();
    let moved = without_interrupts(|| {
        let mut pos = TABLET_POS.lock();
        let moved = *pos != (ax, ay);
        *pos = (ax, ay);
        moved
    });
    if moved {
        events.push((EV_ABS, ABS_X, ax));
        events.push((EV_ABS, ABS_Y, ay));
    }
    button_events(previous, buttons, &mut events);
    if !events.is_empty() {
        emit(TABLET, events.events());
    }
    if wheel != 0 {
        emit(MOUSE, &[(EV_REL, REL_WHEEL, -wheel), (EV_REL, REL_WHEEL_HI_RES, -wheel * 120)]);
    }
}

pub fn opened(id: u32, device: usize) {
    let vt = crate::task::current_vt();
    let client = Client { device, vt, grab: false, realtime: false, ring: Box::new([[0u8; EVENT_SIZE]; RING]), head: 0, len: 0 };
    with(|c| c.insert(id, client));
}

pub fn closed(id: u32) {
    let client = with(|c| c.remove(&id));
    drop(client);
}

pub fn device_of(id: u32) -> Option<usize> {
    with(|c| c.get(&id).map(|c| c.device))
}

pub fn readable(id: u32) -> bool {
    with(|c| c.get(&id).map(|c| c.len > 0).unwrap_or(false))
}

pub fn read(fd: u64, id: u32, buf: u64, count: u64) -> i64 {
    if (count as usize) < EVENT_SIZE {
        return EINVAL;
    }
    loop {
        let max = count as usize / EVENT_SIZE;
        let events: Vec<[u8; EVENT_SIZE]> = with(|c| {
            let mut out = Vec::new();
            if let Some(client) = c.get_mut(&id) {
                while out.len() < max {
                    match client.pop() {
                        Some(e) => out.push(e),
                        None => break,
                    }
                }
            }
            out
        });
        if !events.is_empty() {
            let bytes: Vec<u8> = events.iter().flat_map(|e| e.iter().copied()).collect();
            let r = crate::syscall::copy_out(buf, bytes.len() as u64, &bytes);
            return if r < 0 { r } else { bytes.len() as i64 };
        }
        if crate::syscall::fd_nonblock(fd) {
            return EAGAIN;
        }
        if crate::syscall::signal::pending() {
            return crate::syscall::signal::EINTR;
        }
        crate::tty::sleep_for_events(crate::task::TICK_HZ / 10);
    }
}

pub fn write(id: u32, data: &[u8]) -> i64 {
    if device_of(id).is_none() {
        return EINVAL;
    }
    for chunk in data.chunks_exact(EVENT_SIZE) {
        let kind = u16::from_le_bytes([chunk[16], chunk[17]]);
        let code = u16::from_le_bytes([chunk[18], chunk[19]]);
        let value = i32::from_le_bytes(chunk[20..24].try_into().unwrap());
        if kind == EV_LED && code < 3 {
            if value != 0 {
                LEDS.fetch_or(1 << code, Ordering::Relaxed);
            } else {
                LEDS.fetch_and(!(1 << code), Ordering::Relaxed);
            }
        }
    }
    (data.len() / EVENT_SIZE * EVENT_SIZE) as i64
}

fn put_bits(arg: u64, len: usize, words: &[u64]) -> i64 {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let n = len.min(bytes.len());
    let mut out = alloc::vec![0u8; len];
    out[..n].copy_from_slice(&bytes[..n]);
    if len > 0 && crate::syscall::copy_out(arg, len as u64, &out) < 0 {
        return EFAULT;
    }
    n as i64
}

fn put_string(arg: u64, len: usize, text: &str) -> i64 {
    let mut bytes = Vec::from(text.as_bytes());
    bytes.push(0);
    let n = len.min(bytes.len());
    if n > 0 && crate::syscall::copy_out(arg, n as u64, &bytes[..n]) < 0 {
        return EFAULT;
    }
    n as i64
}

fn read_i32(arg: u64) -> Result<i32, i64> {
    let raw = crate::syscall::user_slice(arg, 4)?;
    Ok(i32::from_le_bytes((&*raw).try_into().unwrap()))
}

fn absinfo(device: usize, axis: u16) -> Option<[i32; 6]> {
    if device != TABLET || axis > ABS_Y {
        return None;
    }
    let (x, y) = without_interrupts(|| *TABLET_POS.lock());
    let (w, h) = crate::drivers::input::mouse::bounds();
    let size = if axis == ABS_X { w } else { h };
    let millimetres = (size.max(1) as i64 * 265 / 1000).max(1);
    let resolution = ((TABLET_MAX as i64 + 1) / millimetres) as i32;
    Some([if axis == ABS_X { x } else { y }, 0, TABLET_MAX, 0, 0, resolution.max(1)])
}

pub fn ioctl(id: u32, request: u64, arg: u64) -> i64 {
    let Some(device) = device_of(id) else {
        return EINVAL;
    };
    if (request >> 8) & 0xFF != 0x45 {
        return ENOTTY;
    }
    let nr = (request & 0xFF) as u16;
    let len = ((request >> 16) & 0x3FFF) as usize;
    let write = (request >> 30) & 1 != 0;
    let read = (request >> 30) & 2 != 0;
    let info = &DEVICES[device];
    match nr {
        0x01 => crate::syscall::copy_out(arg, 4, &0x010001i32.to_le_bytes()).min(0),
        0x02 => {
            let mut raw = [0u8; 8];
            for (i, v) in [info.bus, info.vendor, info.product, info.version].iter().enumerate() {
                raw[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
            crate::syscall::copy_out(arg, 8, &raw).min(0)
        }
        0x03 if write => {
            let raw = match crate::syscall::user_slice(arg, 8) {
                Ok(b) => b.to_vec(),
                Err(e) => return e,
            };
            let delay = i32::from_le_bytes(raw[0..4].try_into().unwrap());
            let period = i32::from_le_bytes(raw[4..8].try_into().unwrap());
            without_interrupts(|| *REPEAT.lock() = (delay, period));
            0
        }
        0x03 => {
            if device != KEYBOARD {
                return EINVAL;
            }
            let (delay, period) = without_interrupts(|| *REPEAT.lock());
            let mut raw = [0u8; 8];
            raw[0..4].copy_from_slice(&delay.to_le_bytes());
            raw[4..8].copy_from_slice(&period.to_le_bytes());
            crate::syscall::copy_out(arg, 8, &raw).min(0)
        }
        0x06 => put_string(arg, len, info.name),
        0x07 => put_string(arg, len, info.phys),
        0x08 => put_string(arg, len, ""),
        0x09 => put_bits(arg, len, &[0]),
        0x18 => {
            if device == KEYBOARD {
                let keys = without_interrupts(|| *KEYS.lock());
                put_bits(arg, len, &keys)
            } else {
                put_bits(arg, len, &[0u64; KEY_CNT / 64])
            }
        }
        0x19 => put_bits(arg, len, &[if device == KEYBOARD { LEDS.load(Ordering::Relaxed) } else { 0 }]),
        0x1A | 0x1B => put_bits(arg, len, &[0]),
        0x20..=0x3F => {
            let ev = nr - 0x20;
            match ev {
                0 => put_bits(arg, len, &[ev_bits(device)]),
                1 => put_bits(arg, len, &key_bits(device)),
                2 => put_bits(arg, len, &[rel_bits(device)]),
                3 => put_bits(arg, len, &[abs_bits(device), 0]),
                4 => put_bits(arg, len, &[msc_bits(device)]),
                0x11 => put_bits(arg, len, &[led_bits(device)]),
                _ => put_bits(arg, len, &[0, 0]),
            }
        }
        0x40..=0x7F if read => match absinfo(device, nr - 0x40) {
            Some(values) => {
                let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
                let n = len.min(bytes.len());
                crate::syscall::copy_out(arg, n as u64, &bytes[..n]).min(0)
            }
            None => EINVAL,
        },
        0xC0..=0xFF if write => {
            if absinfo(device, nr - 0xC0).is_some() { 0 } else { EINVAL }
        }
        0x80 | 0x81 => EINVAL,
        0x84 => crate::syscall::copy_out(arg, 4, &0i32.to_le_bytes()).min(0),
        0x90 => {
            let grab = arg != 0;
            with(|c| {
                if grab && c.iter().any(|(k, v)| *k != id && v.device == device && v.grab) {
                    return EBUSY;
                }
                if let Some(client) = c.get_mut(&id) {
                    client.grab = grab;
                }
                0
            })
        }
        0x91 => {
            with(|c| {
                if let Some(client) = c.get_mut(&id) {
                    client.vt = usize::MAX;
                    client.len = 0;
                }
            });
            0
        }
        0x92 | 0x93 => 0,
        0xA0 => {
            let clock = match read_i32(arg) {
                Ok(v) => v,
                Err(e) => return e,
            };
            if !matches!(clock, 0 | 1 | 7) {
                return EINVAL;
            }
            with(|c| {
                if let Some(client) = c.get_mut(&id) {
                    client.realtime = clock == 0;
                }
            });
            0
        }
        _ => EINVAL,
    }
}
