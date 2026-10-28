use alloc::string::String;
use alloc::vec::Vec;

use crate::sys::syscall3;

pub const SYS_DISPLAY_INFO: u64 = 9160;
pub const SYS_DISPLAY_SET: u64 = 9161;
pub const SYS_DISPLAY_CONFIRM: u64 = 9162;
pub const SYS_DISPLAY_REVERT: u64 = 9163;
pub const SYS_FB_GENERATION: u64 = 9164;
pub const SYS_DISPLAY_EDID: u64 = 9165;
pub const SYS_DISPLAY_OUTPUT: u64 = 9166;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
    pub native: bool,
}

#[derive(Clone, Default)]
pub struct Connector {
    pub id: u32,
    pub name: String,
    pub kind: String,
    pub connected: bool,
    pub native: Mode,
    pub primary: bool,
    pub edid_len: u32,
    pub monitor: String,
    pub vendor: String,
    pub size_mm: String,
    pub modes: Vec<Mode>,
}

#[derive(Clone, Default)]
pub struct Info {
    pub backend: String,
    pub output: String,
    pub can_set: bool,
    pub current: Mode,
    pub native: Mode,
    pub modes: Vec<Mode>,
    pub revert_in: Option<u32>,
    pub connectors: Vec<Connector>,
}

pub fn info() -> Info {
    let mut size = 4096usize;
    let text = loop {
        let mut buf = alloc::vec![0u8; size];
        let n = unsafe { syscall3(SYS_DISPLAY_INFO, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
        if n < 0 {
            return Info::default();
        }
        if (n as usize) <= size {
            buf.truncate(n as usize);
            break String::from_utf8_lossy(&buf).into_owned();
        }
        size = n as usize;
    };
    let mut info = Info::default();
    let mode = |f: &[&str]| Mode {
        width: f.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
        height: f.get(2).and_then(|v| v.parse().ok()).unwrap_or(0),
        refresh: f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0),
        native: f.get(4).map(|v| *v == "native").unwrap_or(false),
    };
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("backend") => {
                info.backend = String::from(f.get(1).copied().unwrap_or(""));
                info.can_set = f.get(2).map(|v| *v == "settable").unwrap_or(false);
            }
            Some("output") => info.output = String::from(f.get(1).copied().unwrap_or("")),
            Some("current") => info.current = mode(&f),
            Some("native") => info.native = mode(&f),
            Some("mode") => info.modes.push(mode(&f)),
            Some("pending") => info.revert_in = f.get(1).and_then(|v| v.parse().ok()),
            Some("connector") => {
                let num = |i: usize| f.get(i).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                info.connectors.push(Connector {
                    id: num(1),
                    name: String::from(f.get(2).copied().unwrap_or("")),
                    kind: String::from(f.get(3).copied().unwrap_or("")),
                    connected: f.get(4).map(|v| *v == "connected").unwrap_or(false),
                    native: Mode { width: num(5), height: num(6), refresh: num(7), native: true },
                    primary: f.get(8).map(|v| *v == "primary").unwrap_or(false),
                    edid_len: num(9),
                    ..Connector::default()
                });
            }
            Some("monitor") => {
                let id = f.get(1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(u32::MAX);
                if let Some(c) = info.connectors.iter_mut().find(|c| c.id == id) {
                    c.monitor = String::from(f.get(2).copied().unwrap_or(""));
                    c.vendor = String::from(f.get(3).copied().unwrap_or(""));
                    c.size_mm = String::from(f.get(4).copied().unwrap_or(""));
                }
            }
            Some("cmode") => {
                let id = f.get(1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(u32::MAX);
                let m = Mode {
                    width: f.get(2).and_then(|v| v.parse().ok()).unwrap_or(0),
                    height: f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0),
                    refresh: f.get(4).and_then(|v| v.parse().ok()).unwrap_or(0),
                    native: false,
                };
                if let Some(c) = info.connectors.iter_mut().find(|c| c.id == id) {
                    c.modes.push(m);
                }
            }
            _ => {}
        }
    }
    info
}

pub fn set_mode(width: u32, height: u32, refresh: u32, trial: bool) -> i64 {
    unsafe { syscall3(SYS_DISPLAY_SET, ((width as u64) << 32) | height as u64, refresh as u64, trial as u64) }
}

pub fn set_output(connector: u32, width: u32, height: u32) -> i64 {
    unsafe { syscall3(SYS_DISPLAY_OUTPUT, connector as u64, ((width as u64) << 32) | height as u64, 0) }
}

pub fn edid(connector: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 1024];
    let n = unsafe { syscall3(SYS_DISPLAY_EDID, connector as u64, buf.as_mut_ptr() as u64, buf.len() as u64) };
    if n <= 0 {
        return Vec::new();
    }
    buf.truncate(n as usize);
    buf
}

pub fn confirm() -> i64 {
    unsafe { syscall3(SYS_DISPLAY_CONFIRM, 0, 0, 0) }
}

pub fn revert() -> i64 {
    unsafe { syscall3(SYS_DISPLAY_REVERT, 0, 0, 0) }
}

pub fn generation() -> u64 {
    let r = unsafe { syscall3(SYS_FB_GENERATION, 0, 0, 0) };
    if r < 0 { 0 } else { r as u64 }
}

pub mod gpu {
    use crate::sys::syscall3;

    pub const SYS_GPU_CAPS: u64 = 9180;
    pub const SYS_GPU_FLUSH: u64 = 9181;
    pub const SYS_GPU_FILL: u64 = 9182;
    pub const SYS_GPU_COPY: u64 = 9183;
    pub const SYS_GPU_CURSOR_SET: u64 = 9184;
    pub const SYS_GPU_CURSOR_MOVE: u64 = 9185;
    pub const SYS_GPU_CURSOR_HIDE: u64 = 9186;

    pub const CAP_FLUSH: u32 = 1 << 0;
    pub const CAP_FILL: u32 = 1 << 1;
    pub const CAP_COPY: u32 = 1 << 2;
    pub const CAP_MODESET: u32 = 1 << 3;
    pub const CAP_CURSOR: u32 = 1 << 4;
    pub const CAP_REFRESH: u32 = 1 << 5;
    pub const CAP_SCALE: u32 = 1 << 6;
    pub const CAP_FLIP: u32 = 1 << 7;
    pub const CAP_VBLANK: u32 = 1 << 8;
    pub const CAP_CONNECTORS: u32 = 1 << 9;
    pub const CAP_HOTPLUG: u32 = 1 << 10;
    pub const SYS_GPU_FLIP: u64 = 9187;
    pub const SYS_GPU_VBLANK: u64 = 9188;
    pub const SYS_GPU_BUFFERS: u64 = 9189;

    pub const CURSOR_MAX: u32 = 64;

    fn pack(a: u32, b: u32) -> u64 {
        ((a as u64) << 32) | b as u64
    }

    pub fn caps() -> u32 {
        let r = unsafe { syscall3(SYS_GPU_CAPS, 0, 0, 0) };
        if r < 0 { 0 } else { r as u32 }
    }

    pub fn flush(x: i32, y: i32, w: i32, h: i32) -> i64 {
        if w <= 0 || h <= 0 {
            return 0;
        }
        unsafe { syscall3(SYS_GPU_FLUSH, pack(x.max(0) as u32, y.max(0) as u32), pack(w as u32, h as u32), 0) }
    }

    pub fn fill(x: i32, y: i32, w: i32, h: i32, color: u32) -> i64 {
        if w <= 0 || h <= 0 {
            return 0;
        }
        unsafe { syscall3(SYS_GPU_FILL, pack(x.max(0) as u32, y.max(0) as u32), pack(w as u32, h as u32), color as u64) }
    }

    pub fn copy(sx: i32, sy: i32, dx: i32, dy: i32, w: i32, h: i32) -> i64 {
        if w <= 0 || h <= 0 {
            return 0;
        }
        unsafe { syscall3(SYS_GPU_COPY, pack(sx.max(0) as u32, sy.max(0) as u32), pack(dx.max(0) as u32, dy.max(0) as u32), pack(w as u32, h as u32)) }
    }

    pub fn cursor_set(pixels: &[u32], width: u32, height: u32, hot_x: u32, hot_y: u32) -> i64 {
        if width == 0 || height == 0 || width > CURSOR_MAX || height > CURSOR_MAX {
            return -22;
        }
        if pixels.len() < (width * height) as usize {
            return -22;
        }
        unsafe { syscall3(SYS_GPU_CURSOR_SET, pixels.as_ptr() as u64, pack(width, height), pack(hot_x, hot_y)) }
    }

    pub fn cursor_move(x: i32, y: i32) -> i64 {
        unsafe { syscall3(SYS_GPU_CURSOR_MOVE, pack(x.max(0) as u32, y.max(0) as u32), 0, 0) }
    }

    pub fn cursor_hide() -> i64 {
        unsafe { syscall3(SYS_GPU_CURSOR_HIDE, 0, 0, 0) }
    }

    pub fn buffers() -> u32 {
        let r = unsafe { syscall3(SYS_GPU_BUFFERS, 0, 0, 0) };
        if r < 1 { 1 } else { r as u32 }
    }

    pub fn flip(index: u32) -> i64 {
        unsafe { syscall3(SYS_GPU_FLIP, index as u64, 0, 0) }
    }

    pub fn wait_vblank(timeout_ms: u32) -> i64 {
        unsafe { syscall3(SYS_GPU_VBLANK, timeout_ms as u64, 0, 0) }
    }
}
