use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::arch::io::{inw, outw};
use super::gpu;
use crate::drivers::pci;
use crate::memory::{FramebufferInfo, FRAMEBUFFER};

const BGA_INDEX: u16 = 0x01CE;
const BGA_DATA: u16 = 0x01CF;
const BGA_XRES: u16 = 1;
const BGA_YRES: u16 = 2;
const BGA_BPP: u16 = 3;
const BGA_ENABLE: u16 = 4;
const BGA_VIRT_WIDTH: u16 = 6;
const BGA_VIRT_HEIGHT: u16 = 7;
const BGA_X_OFFSET: u16 = 8;
const BGA_Y_OFFSET: u16 = 9;
const BGA_VIDEO_MEMORY_64K: u16 = 0xA;
const TRIAL_MS: u64 = 15_000;
const CONFIG_PATH: &str = "/etc/hamix/display.conf";

const STANDARD: [(u32, u32); 16] = [
    (640, 480),
    (800, 600),
    (1024, 600),
    (1024, 768),
    (1152, 864),
    (1280, 720),
    (1280, 768),
    (1280, 800),
    (1280, 1024),
    (1360, 768),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1680, 1050),
    (1920, 1080),
    (1920, 1200),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
}

enum Backend {
    None,
    Bga { vram: u64 },
    Gpu { provider: String, limit: u64 },
}

struct State {
    backend: Backend,
    current: Option<Mode>,
    previous: Option<Mode>,
    deadline: u64,
}

static STATE: Mutex<State> = Mutex::new(State { backend: Backend::None, current: None, previous: None, deadline: 0 });
static GENERATION: AtomicU64 = AtomicU64::new(1);

fn bga_write(index: u16, value: u16) {
    outw(BGA_INDEX, index);
    outw(BGA_DATA, value);
}

fn bga_read(index: u16) -> u16 {
    outw(BGA_INDEX, index);
    inw(BGA_DATA)
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

pub fn bump_generation() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn backend_name() -> &'static str {
    match &STATE.lock().backend {
        Backend::None => "firmware framebuffer",
        Backend::Bga { .. } => "Bochs VBE mode setting",
        Backend::Gpu { .. } => "module mode setting",
    }
}

pub fn use_gpu(provider: &str, limit: u64, width: u32, height: u32, refresh: u32) {
    let mut state = STATE.lock();
    state.backend = Backend::Gpu { provider: String::from(provider), limit };
    state.current = Some(Mode { width, height, refresh: if refresh == 0 { 60 } else { refresh } });
    state.previous = None;
    state.deadline = 0;
}

pub fn init() {
    let fb = *FRAMEBUFFER.lock();
    let Some(fb) = fb else {
        return;
    };
    let devices = pci::devices();
    let mut state = STATE.lock();
    if crate::arch::io::PORTS && devices.iter().any(|d| d.vendor == 0x1234 && d.device == 0x1111) && bga_read(0) >= 0xB0C2 {
        let vram = bga_read(BGA_VIDEO_MEMORY_64K) as u64 * 64 * 1024;
        state.backend = Backend::Bga { vram: if vram == 0 { 16 << 20 } else { vram } };
        state.current = Some(Mode { width: fb.width, height: fb.height, refresh: 60 });
    }
    if state.current.is_none() {
        state.current = Some(Mode { width: fb.width, height: fb.height, refresh: 60 });
    }
}

fn available(state: &State) -> Vec<(Mode, bool)> {
    let mut modes = Vec::new();
    match &state.backend {
        Backend::None => {
            if let Some(m) = state.current {
                modes.push((m, true));
            }
        }
        Backend::Bga { vram } => {
            for (w, h) in STANDARD {
                if (w as u64 * h as u64 * 4) <= *vram {
                    modes.push((Mode { width: w, height: h, refresh: 60 }, false));
                }
            }
        }
        Backend::Gpu { limit, .. } => {
            let current = state.current;
            let caps = gpu::caps();
            let rates = gpu::refresh_rates();
            let mut listed = if caps & gpu::CAP_MODESET != 0 { gpu::mode_list() } else { Vec::new() };
            if listed.is_empty() && caps & gpu::CAP_CONNECTORS != 0 && caps & gpu::CAP_MODESET != 0 {
                let primary = gpu::connectors().into_iter().find(|c| c.primary != 0).map(|c| c.id).unwrap_or(0);
                for (w, h, _) in gpu::connector_modes(primary) {
                    if (w as u64 * h as u64 * 4) * gpu::buffers() as u64 <= *limit && !listed.contains(&(w, h)) {
                        listed.push((w, h));
                    }
                }
            }
            let mut sizes: Vec<((u32, u32), bool)> = Vec::new();
            if !listed.is_empty() {
                for (index, (w, h)) in listed.iter().copied().enumerate() {
                    sizes.push(((w, h), index == 0));
                }
            } else if caps & gpu::CAP_MODESET == 0 {
                let Some(m) = current else {
                    return modes;
                };
                sizes.push(((m.width, m.height), true));
            } else {
                for (w, h) in STANDARD {
                    if (w as u64 * h as u64 * 4) <= *limit {
                        let native = current.map(|m| m.width == w && m.height == h).unwrap_or(false);
                        sizes.push(((w, h), native));
                    }
                }
                if let Some(m) = current {
                    if !sizes.iter().any(|((w, h), _)| *w == m.width && *h == m.height) {
                        sizes.push(((m.width, m.height), true));
                    }
                }
            }
            let native_refresh = rates.last().copied().or(current.map(|m| m.refresh)).unwrap_or(60);
            for ((w, h), native) in sizes {
                if rates.is_empty() {
                    modes.push((Mode { width: w, height: h, refresh: native_refresh }, native));
                    continue;
                }
                for rate in rates.iter().copied() {
                    modes.push((Mode { width: w, height: h, refresh: rate }, native && rate == native_refresh));
                }
            }
        }
    }
    modes
}

pub fn info_text() -> String {
    let state = STATE.lock();
    let mut out = String::new();
    let (name, settable, output) = match &state.backend {
        Backend::None => ("firmware framebuffer", false, String::from("set by the bootloader")),
        Backend::Bga { vram } => ("Bochs VBE", true, format!("virtual display, {} MiB video memory", vram >> 20)),
        Backend::Gpu { provider, limit } => (
            provider.as_str(),
            gpu::caps() & (gpu::CAP_MODESET | gpu::CAP_REFRESH) != 0,
            format!("accelerated scanout, up to {} MiB of scanout memory", limit >> 20),
        ),
    };
    out.push_str(&format!("backend\t{}\t{}\n", name, if settable { "settable" } else { "fixed" }));
    out.push_str(&format!("output\t{}\n", output));
    if let Some(m) = state.current {
        out.push_str(&format!("current\t{}\t{}\t{}\n", m.width, m.height, m.refresh));
    }
    if matches!(state.backend, Backend::Gpu { .. }) {
        if let Some((w, h)) = gpu::mode_list().first().copied() {
            let refresh = gpu::refresh_rates().last().copied().or(state.current.map(|m| m.refresh)).unwrap_or(60);
            out.push_str(&format!("native\t{}\t{}\t{}\n", w, h, refresh));
        }
    }
    for (m, native) in available(&state) {
        out.push_str(&format!("mode\t{}\t{}\t{}\t{}\n", m.width, m.height, m.refresh, if native { "native" } else { "scaled" }));
    }
    if matches!(state.backend, Backend::Gpu { .. }) && gpu::caps() & gpu::CAP_CONNECTORS != 0 {
        for c in gpu::connectors() {
            out.push_str(&format!(
                "connector\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                c.id,
                c.name(),
                c.kind_name(),
                if c.status != 0 { "connected" } else { "disconnected" },
                c.native_w,
                c.native_h,
                c.refresh,
                if c.primary != 0 { "primary" } else { "secondary" },
                c.edid_len
            ));
            let edid = if c.edid_len > 0 { super::edid::parse(&gpu::edid(c.id)) } else { None };
            if let Some(e) = edid.as_ref().filter(|e| !e.name.is_empty()) {
                out.push_str(&format!("monitor\t{}\t{}\t{}{}{}\t{}x{} mm\n", c.id, e.name, e.manufacturer[0] as char, e.manufacturer[1] as char, e.manufacturer[2] as char, e.width_mm, e.height_mm));
            }
            for (w, h, hz) in gpu::connector_modes(c.id) {
                out.push_str(&format!("cmode\t{}\t{}\t{}\t{}\n", c.id, w, h, hz));
            }
        }
    }
    if state.deadline > 0 {
        let left = state.deadline.saturating_sub(crate::task::uptime_ms()).div_ceil(1000);
        out.push_str(&format!("pending\t{}\n", left));
    }
    out
}

fn program(state: &mut State, mode: Mode) -> Result<(u32, u32, u32), &'static str> {
    match &state.backend {
        Backend::None => Err("mode setting is not supported on this graphics adapter"),
        Backend::Gpu { .. } => Err("the display module programs the mode itself"),
        Backend::Bga { .. } => {
            bga_write(BGA_ENABLE, 0);
            bga_write(BGA_XRES, mode.width as u16);
            bga_write(BGA_YRES, mode.height as u16);
            bga_write(BGA_BPP, 32);
            bga_write(BGA_VIRT_WIDTH, mode.width as u16);
            bga_write(BGA_VIRT_HEIGHT, mode.height as u16);
            bga_write(BGA_X_OFFSET, 0);
            bga_write(BGA_Y_OFFSET, 0);
            bga_write(BGA_ENABLE, 0x41);
            if bga_read(BGA_XRES) as u32 != mode.width || bga_read(BGA_YRES) as u32 != mode.height {
                return Err("the adapter refused the mode");
            }
            Ok((mode.width, mode.height, mode.width * 4))
        }
    }
}

fn framebuffer_changed(width: u32, height: u32, pitch: u32) {
    let updated = {
        let mut fb = FRAMEBUFFER.lock();
        let Some(info) = fb.as_mut() else {
            return;
        };
        *info = FramebufferInfo { addr: info.addr, pitch, width, height, bpp: 32 };
        *info
    };
    crate::arch::paging::enable_kernel_write_combining(updated.addr, updated.byte_len());
    crate::drivers::video::text_mode::cache_framebuffer();
    if !crate::drivers::video::text_mode::graphics_owned() {
        crate::drivers::video::text_mode::fb_clear_full();
        crate::drivers::video::text_mode::fb_redraw_all();
    }
    crate::drivers::input::mouse::set_bounds(width as i32, height as i32);
    crate::task::display::refresh_owner();
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

fn apply_locked(state: &mut State, mode: Mode) -> Result<(), &'static str> {
    if !available(state).iter().any(|(m, _)| *m == mode) {
        return Err("unsupported mode");
    }
    if matches!(state.backend, Backend::Gpu { .. }) {
        let current = state.current;
        let same_size = current.map(|m| m.width == mode.width && m.height == mode.height).unwrap_or(false);
        if !same_size && crate::drivers::video::gpu::set_mode(mode.width, mode.height) != 0 {
            return Err("the display module refused the mode");
        }
        if current.map(|m| m.refresh != mode.refresh).unwrap_or(true) && crate::drivers::video::gpu::caps() & crate::drivers::video::gpu::CAP_REFRESH != 0 {
            if crate::drivers::video::gpu::set_refresh(mode.refresh) != 0 {
                if let Some(previous) = current {
                    let _ = crate::drivers::video::gpu::set_refresh(previous.refresh);
                }
                return Err("the display module refused the refresh rate");
            }
        }
        state.current = Some(mode);
        return Ok(());
    }
    let (w, h, pitch) = program(state, mode)?;
    state.current = Some(mode);
    framebuffer_changed(w, h, pitch);
    Ok(())
}

pub fn set_mode(width: u32, height: u32, refresh: u32, trial: bool) -> i64 {
    let mut state = STATE.lock();
    let mode = Mode { width, height, refresh };
    if state.current == Some(mode) {
        return 0;
    }
    let previous = state.current;
    match apply_locked(&mut state, mode) {
        Ok(()) => {
            if trial {
                if state.deadline == 0 {
                    state.previous = previous;
                }
                state.deadline = crate::task::uptime_ms() + TRIAL_MS;
            } else {
                state.previous = None;
                state.deadline = 0;
            }
            crate::drivers::klog::log(&format!("video: mode {}x{}@{}{}", width, height, refresh, if trial { " (trial)" } else { "" }));
            0
        }
        Err(e) => {
            crate::drivers::klog::log(&format!("video: mode {}x{}@{} failed: {}", width, height, refresh, e));
            -22
        }
    }
}

pub fn confirm() -> i64 {
    let mode = {
        let mut state = STATE.lock();
        state.deadline = 0;
        state.previous = None;
        state.current
    };
    if let Some(m) = mode {
        let text = format!("mode={}x{}@{}\n", m.width, m.height, m.refresh);
        if let Some(vfs) = crate::fs::VFS.lock().as_mut() {
            let _ = vfs.write(0, CONFIG_PATH, text.as_bytes(), false, 0);
        }
        crate::fs::request_sync();
    }
    0
}

pub fn revert() -> i64 {
    let mut state = STATE.lock();
    let Some(previous) = state.previous.take() else {
        state.deadline = 0;
        return 0;
    };
    state.deadline = 0;
    match apply_locked(&mut state, previous) {
        Ok(()) => 0,
        Err(_) => -22,
    }
}

pub fn tick() {
    let expired = {
        let state = STATE.lock();
        state.deadline > 0 && crate::task::uptime_ms() >= state.deadline
    };
    if expired {
        crate::drivers::klog::log("video: trial mode not confirmed, reverting");
        revert();
    }
}

pub fn apply_saved() -> Option<Result<String, &'static str>> {
    let text = crate::fs::VFS.lock().as_mut().and_then(|v| v.read(0, CONFIG_PATH).ok())?;
    let text = String::from_utf8_lossy(&text).into_owned();
    let value = String::from(text.lines().find_map(|l| l.trim().strip_prefix("mode="))?.trim());
    let (size, refresh) = value.split_once('@').unwrap_or((value.as_str(), "60"));
    let (w, h) = size.split_once('x')?;
    let mode = Mode { width: w.parse().ok()?, height: h.parse().ok()?, refresh: refresh.parse().ok()? };
    let mut state = STATE.lock();
    if state.current == Some(mode) {
        return Some(Ok(format!("{}x{}@{} (already active)", mode.width, mode.height, mode.refresh)));
    }
    Some(apply_locked(&mut state, mode).map(|_| format!("{}x{}@{}", mode.width, mode.height, mode.refresh)))
}
