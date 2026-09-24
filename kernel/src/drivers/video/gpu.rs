use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::memory::{FramebufferInfo, FRAMEBUFFER};

pub const ABI: u32 = 3;
pub const ABI_V2: u32 = 2;

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

pub const CONNECTOR_UNKNOWN: u32 = 0;
pub const CONNECTOR_VGA: u32 = 1;
pub const CONNECTOR_DVI: u32 = 2;
pub const CONNECTOR_HDMI: u32 = 3;
pub const CONNECTOR_DP: u32 = 4;
pub const CONNECTOR_EDP: u32 = 5;
pub const CONNECTOR_LVDS: u32 = 6;
pub const CONNECTOR_VIRTUAL: u32 = 7;

pub const CURSOR_MAX: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DisplayOpsV2 {
    pub abi: u32,
    pub caps: u32,
    pub context: u64,
    pub flush: Option<extern "C" fn(u64, u32, u32, u32, u32) -> i32>,
    pub fill: Option<extern "C" fn(u64, u32, u32, u32, u32, u32) -> i32>,
    pub copy: Option<extern "C" fn(u64, u32, u32, u32, u32, u32, u32) -> i32>,
    pub set_mode: Option<extern "C" fn(u64, u32, u32) -> i32>,
    pub set_refresh: Option<extern "C" fn(u64, u32) -> i32>,
    pub refresh_list: Option<extern "C" fn(u64, *mut u32, u32) -> i32>,
    pub mode_list: Option<extern "C" fn(u64, *mut u32, u32) -> i32>,
    pub cursor_set: Option<extern "C" fn(u64, *const u32, u32, u32, u32, u32) -> i32>,
    pub cursor_move: Option<extern "C" fn(u64, i32, i32) -> i32>,
    pub cursor_hide: Option<extern "C" fn(u64) -> i32>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DisplayOps {
    pub abi: u32,
    pub caps: u32,
    pub context: u64,
    pub flush: Option<extern "C" fn(u64, u32, u32, u32, u32) -> i32>,
    pub fill: Option<extern "C" fn(u64, u32, u32, u32, u32, u32) -> i32>,
    pub copy: Option<extern "C" fn(u64, u32, u32, u32, u32, u32, u32) -> i32>,
    pub set_mode: Option<extern "C" fn(u64, u32, u32) -> i32>,
    pub set_refresh: Option<extern "C" fn(u64, u32) -> i32>,
    pub refresh_list: Option<extern "C" fn(u64, *mut u32, u32) -> i32>,
    pub mode_list: Option<extern "C" fn(u64, *mut u32, u32) -> i32>,
    pub cursor_set: Option<extern "C" fn(u64, *const u32, u32, u32, u32, u32) -> i32>,
    pub cursor_move: Option<extern "C" fn(u64, i32, i32) -> i32>,
    pub cursor_hide: Option<extern "C" fn(u64) -> i32>,
    pub connectors: Option<extern "C" fn(u64, *mut ConnectorDesc, u32) -> i32>,
    pub connector_modes: Option<extern "C" fn(u64, u32, *mut u32, u32) -> i32>,
    pub edid: Option<extern "C" fn(u64, u32, *mut u8, u32) -> i32>,
    pub set_output: Option<extern "C" fn(u64, u32, u32, u32) -> i32>,
    pub page_flip: Option<extern "C" fn(u64, u32) -> i32>,
    pub wait_vblank: Option<extern "C" fn(u64, u32) -> i32>,
    pub buffers: u32,
    pub reserved: u32,
}

impl DisplayOps {
    pub const fn empty(caps: u32) -> DisplayOps {
        DisplayOps {
            abi: ABI,
            caps,
            context: 0,
            flush: None,
            fill: None,
            copy: None,
            set_mode: None,
            set_refresh: None,
            refresh_list: None,
            mode_list: None,
            cursor_set: None,
            cursor_move: None,
            cursor_hide: None,
            connectors: None,
            connector_modes: None,
            edid: None,
            set_output: None,
            page_flip: None,
            wait_vblank: None,
            buffers: 1,
            reserved: 0,
        }
    }
}

impl From<DisplayOpsV2> for DisplayOps {
    fn from(v2: DisplayOpsV2) -> DisplayOps {
        DisplayOps {
            abi: ABI_V2,
            caps: v2.caps & !(CAP_FLIP | CAP_VBLANK | CAP_CONNECTORS | CAP_HOTPLUG),
            context: v2.context,
            flush: v2.flush,
            fill: v2.fill,
            copy: v2.copy,
            set_mode: v2.set_mode,
            set_refresh: v2.set_refresh,
            refresh_list: v2.refresh_list,
            mode_list: v2.mode_list,
            cursor_set: v2.cursor_set,
            cursor_move: v2.cursor_move,
            cursor_hide: v2.cursor_hide,
            ..DisplayOps::empty(0)
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ConnectorDesc {
    pub kind: u32,
    pub id: u32,
    pub status: u32,
    pub native_w: u32,
    pub native_h: u32,
    pub edid_len: u32,
    pub x: i32,
    pub y: i32,
    pub refresh: u32,
    pub primary: u32,
    pub name: [u8; 16],
}

impl ConnectorDesc {
    pub fn name(&self) -> String {
        let end = self.name.iter().position(|b| *b == 0).unwrap_or(self.name.len());
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            CONNECTOR_VGA => "VGA",
            CONNECTOR_DVI => "DVI",
            CONNECTOR_HDMI => "HDMI",
            CONNECTOR_DP => "DP",
            CONNECTOR_EDP => "eDP",
            CONNECTOR_LVDS => "LVDS",
            CONNECTOR_VIRTUAL => "Virtual",
            _ => "Unknown",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FbDesc {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
}

struct Provider {
    name: String,
    ops: DisplayOps,
}

unsafe impl Send for Provider {}

static PROVIDER: Mutex<Option<Provider>> = Mutex::new(None);
static IDENTITY: Mutex<Option<(String, String, String)>> = Mutex::new(None);

pub fn set_identity(model: String, codename: String, generation: String) {
    *IDENTITY.lock() = Some((model, codename, generation));
}

pub fn identity() -> Option<(String, String, String)> {
    IDENTITY.lock().clone()
}
static CALL: Mutex<()> = Mutex::new(());
static CAPS: AtomicU32 = AtomicU32::new(0);
static CURSOR_OWNED: AtomicBool = AtomicBool::new(false);
static FLUSHES: AtomicU64 = AtomicU64::new(0);
static PIXELS: AtomicU64 = AtomicU64::new(0);
static FB_W: AtomicU32 = AtomicU32::new(0);
static FB_H: AtomicU32 = AtomicU32::new(0);
static DIRTY_X0: AtomicU32 = AtomicU32::new(u32::MAX);
static DIRTY_Y0: AtomicU32 = AtomicU32::new(u32::MAX);
static DIRTY_X1: AtomicU32 = AtomicU32::new(0);
static DIRTY_Y1: AtomicU32 = AtomicU32::new(0);

pub fn active() -> bool {
    CAPS.load(Ordering::Relaxed) != 0
}

pub fn caps() -> u32 {
    CAPS.load(Ordering::Relaxed)
}

pub fn stats() -> (u64, u64) {
    (FLUSHES.load(Ordering::Relaxed), PIXELS.load(Ordering::Relaxed))
}

pub fn name() -> String {
    PROVIDER.lock().as_ref().map(|p| p.name.clone()).unwrap_or_default()
}

fn ops() -> Option<DisplayOps> {
    PROVIDER.lock().as_ref().map(|p| p.ops)
}

pub fn register(name: &str, ops: DisplayOps, fb: FbDesc) -> Result<(), &'static str> {
    if ops.abi != ABI && ops.abi != ABI_V2 {
        return Err("gpu: module was built against a different display ABI");
    }
    if fb.addr == 0 || fb.width == 0 || fb.height == 0 || fb.bpp != 32 {
        return Err("gpu: module offered an unusable framebuffer");
    }
    if fb.pitch < fb.width * 4 {
        return Err("gpu: module offered a framebuffer whose pitch is too small");
    }
    if PROVIDER.lock().is_some() {
        return Err("gpu: a display provider is already registered");
    }
    *PROVIDER.lock() = Some(Provider { name: name.to_string(), ops });
    CAPS.store(ops.caps, Ordering::SeqCst);
    BUFFERS.store(ops.buffers.clamp(1, 2), Ordering::SeqCst);
    FRONT.store(0, Ordering::SeqCst);
    adopt(FramebufferInfo { addr: fb.addr, pitch: fb.pitch, width: fb.width, height: fb.height, bpp: 32 });
    if ops.caps & (CAP_MODESET | CAP_REFRESH) != 0 {
        let resident = (fb.pitch as u64 * fb.height as u64).div_ceil(4096) * 4096 * ops.buffers.clamp(1, 2) as u64;
        let headroom = if crate::module::kpi::loading() {
            crate::module::budget_left().max(0) as u64 + resident
        } else {
            (crate::memory::frame::memory_info().0 as u64 / 4).max(resident)
        };
        let native = refresh_rates().last().copied().unwrap_or(60);
        super::modes::use_gpu(name, headroom, fb.width, fb.height, native);
    }
    Ok(())
}

pub fn adopt(fb: FramebufferInfo) {
    FB_W.store(fb.width, Ordering::SeqCst);
    FB_H.store(fb.height, Ordering::SeqCst);
    *FRAMEBUFFER.lock() = Some(fb);
    super::text_mode::cache_framebuffer();
    if !super::text_mode::graphics_owned() {
        super::text_mode::fb_clear_full();
        super::text_mode::fb_redraw_all();
    }
    crate::drivers::input::mouse::set_bounds(fb.width as i32, fb.height as i32);
    crate::task::display::refresh_owner();
    super::modes::bump_generation();
    mark_dirty(0, 0, fb.width, fb.height);
}

fn clamp(x: u32, y: u32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let (fw, fh) = (FB_W.load(Ordering::Relaxed), FB_H.load(Ordering::Relaxed));
    if fw == 0 || fh == 0 || x >= fw || y >= fh || w == 0 || h == 0 {
        return None;
    }
    Some((x, y, w.min(fw - x), h.min(fh - y)))
}

pub fn mark_dirty(x: u32, y: u32, w: u32, h: u32) {
    if !active() {
        return;
    }
    let Some((x, y, w, h)) = clamp(x, y, w, h) else {
        return;
    };
    DIRTY_X0.fetch_min(x, Ordering::Relaxed);
    DIRTY_Y0.fetch_min(y, Ordering::Relaxed);
    DIRTY_X1.fetch_max(x + w, Ordering::Relaxed);
    DIRTY_Y1.fetch_max(y + h, Ordering::Relaxed);
}

fn take_dirty() -> Option<(u32, u32, u32, u32)> {
    let x0 = DIRTY_X0.swap(u32::MAX, Ordering::Relaxed);
    let y0 = DIRTY_Y0.swap(u32::MAX, Ordering::Relaxed);
    let x1 = DIRTY_X1.swap(0, Ordering::Relaxed);
    let y1 = DIRTY_Y1.swap(0, Ordering::Relaxed);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

fn invoke<F: FnOnce(&DisplayOps) -> i32>(f: F) -> i32 {
    let Some(ops) = ops() else {
        return -19;
    };
    let _guard = CALL.lock();
    f(&ops)
}

pub fn flush(x: u32, y: u32, w: u32, h: u32) -> i32 {
    let Some((x, y, w, h)) = clamp(x, y, w, h) else {
        return 0;
    };
    let result = invoke(|ops| match ops.flush {
        Some(call) => call(ops.context, x, y, w, h),
        None => -95,
    });
    if result == 0 {
        FLUSHES.fetch_add(1, Ordering::Relaxed);
        PIXELS.fetch_add(w as u64 * h as u64, Ordering::Relaxed);
    }
    result
}

pub fn flush_pending() {
    if let Some((x, y, w, h)) = take_dirty() {
        flush(x, y, w, h);
    }
}

pub fn fill(x: u32, y: u32, w: u32, h: u32, color: u32) -> i32 {
    let Some((x, y, w, h)) = clamp(x, y, w, h) else {
        return 0;
    };
    let r = invoke(|ops| match ops.fill {
        Some(call) => call(ops.context, x, y, w, h, color),
        None => -95,
    });
    if r == 0 {
        mark_dirty(x, y, w, h);
    }
    r
}

pub fn copy(sx: u32, sy: u32, dx: u32, dy: u32, w: u32, h: u32) -> i32 {
    if clamp(sx, sy, w, h).is_none() {
        return 0;
    }
    let Some((dx, dy, w, h)) = clamp(dx, dy, w, h) else {
        return 0;
    };
    let r = invoke(|ops| match ops.copy {
        Some(call) => call(ops.context, sx, sy, dx, dy, w, h),
        None => -95,
    });
    if r == 0 {
        mark_dirty(dx, dy, w, h);
    }
    r
}

pub fn set_mode(width: u32, height: u32) -> i32 {
    invoke(|ops| match ops.set_mode {
        Some(call) => call(ops.context, width, height),
        None => -95,
    })
}

pub fn set_refresh(hz: u32) -> i32 {
    invoke(|ops| match ops.set_refresh {
        Some(call) => call(ops.context, hz),
        None => -95,
    })
}

pub fn refresh_rates() -> Vec<u32> {
    let mut out = [0u32; 8];
    let count = invoke(|ops| match ops.refresh_list {
        Some(call) => call(ops.context, out.as_mut_ptr(), out.len() as u32),
        None => -95,
    });
    if count <= 0 {
        return Vec::new();
    }
    let mut list: Vec<u32> = out[..(count as usize).min(out.len())].iter().copied().filter(|v| *v > 0).collect();
    list.sort_unstable();
    list.dedup();
    list
}

pub fn mode_list() -> Vec<(u32, u32)> {
    let mut out = [0u32; 32];
    let count = invoke(|ops| match ops.mode_list {
        Some(call) => call(ops.context, out.as_mut_ptr(), (out.len() / 2) as u32),
        None => -95,
    });
    if count <= 0 {
        return Vec::new();
    }
    let mut list = Vec::new();
    for pair in out.chunks_exact(2).take(count as usize) {
        if pair[0] >= 320 && pair[1] >= 200 && !list.contains(&(pair[0], pair[1])) {
            list.push((pair[0], pair[1]));
        }
    }
    list
}

pub fn cursor_set(pixels: &[u32], width: u32, height: u32, hot_x: u32, hot_y: u32) -> i32 {
    if width == 0 || height == 0 || width > CURSOR_MAX || height > CURSOR_MAX {
        return -22;
    }
    if pixels.len() < (width * height) as usize {
        return -22;
    }
    let r = invoke(|ops| match ops.cursor_set {
        Some(call) => call(ops.context, pixels.as_ptr(), width, height, hot_x, hot_y),
        None => -95,
    });
    if r == 0 {
        CURSOR_OWNED.store(true, Ordering::SeqCst);
    }
    r
}

pub fn cursor_move(x: i32, y: i32) -> i32 {
    invoke(|ops| match ops.cursor_move {
        Some(call) => call(ops.context, x, y),
        None => -95,
    })
}

pub fn cursor_hide() -> i32 {
    CURSOR_OWNED.store(false, Ordering::SeqCst);
    invoke(|ops| match ops.cursor_hide {
        Some(call) => call(ops.context),
        None => -95,
    })
}

pub fn cursor_owned() -> bool {
    CURSOR_OWNED.load(Ordering::Relaxed)
}

pub fn connectors() -> Vec<ConnectorDesc> {
    let mut out = [ConnectorDesc::default(); 8];
    let count = invoke(|ops| match ops.connectors {
        Some(call) => call(ops.context, out.as_mut_ptr(), out.len() as u32),
        None => -95,
    });
    if count <= 0 {
        return Vec::new();
    }
    out[..(count as usize).min(out.len())].to_vec()
}

pub fn connector_modes(id: u32) -> Vec<(u32, u32, u32)> {
    let mut out = [0u32; 96];
    let count = invoke(|ops| match ops.connector_modes {
        Some(call) => call(ops.context, id, out.as_mut_ptr(), (out.len() / 3) as u32),
        None => -95,
    });
    if count <= 0 {
        return Vec::new();
    }
    out.chunks_exact(3).take(count as usize).map(|m| (m[0], m[1], m[2])).filter(|m| m.0 >= 320 && m.1 >= 200).collect()
}

pub fn edid(id: u32) -> Vec<u8> {
    let mut out = alloc::vec![0u8; 1024];
    let count = invoke(|ops| match ops.edid {
        Some(call) => call(ops.context, id, out.as_mut_ptr(), out.len() as u32),
        None => -95,
    });
    if count <= 0 {
        return Vec::new();
    }
    out.truncate(count as usize);
    out
}

pub fn set_output(id: u32, width: u32, height: u32) -> i32 {
    let result = invoke(|ops| match ops.set_output {
        Some(call) => call(ops.context, id, width, height),
        None => -95,
    });
    if result == 0 {
        hotplug();
    }
    result
}

static FRONT: AtomicU32 = AtomicU32::new(0);
static BUFFERS: AtomicU32 = AtomicU32::new(1);

pub fn buffers() -> u32 {
    if caps() & CAP_FLIP == 0 { 1 } else { BUFFERS.load(Ordering::Relaxed).max(1) }
}

pub fn buffer_stride(fb: &FramebufferInfo) -> u64 {
    (fb.pitch as u64 * fb.height as u64).div_ceil(4096) * 4096
}

pub fn page_flip(index: u32) -> i32 {
    if index >= buffers() {
        return -22;
    }
    let result = invoke(|ops| match ops.page_flip {
        Some(call) => call(ops.context, index),
        None => -95,
    });
    if result == 0 {
        FRONT.store(index, Ordering::Relaxed);
    }
    result
}

pub fn front() -> u32 {
    FRONT.load(Ordering::Relaxed)
}

pub fn wait_vblank(timeout_ms: u32) -> i32 {
    let Some(ops) = ops() else {
        return -19;
    };
    match ops.wait_vblank {
        Some(call) => call(ops.context, timeout_ms),
        None => -95,
    }
}

static HOTPLUGS: AtomicU64 = AtomicU64::new(0);

pub fn hotplug() {
    HOTPLUGS.fetch_add(1, Ordering::Relaxed);
    super::modes::bump_generation();
    if let Some(owner) = crate::task::display::owner() {
        let message = 11u32.to_le_bytes();
        crate::task::ipc::send(0, owner, &message);
    }
}

pub fn hotplugs() -> u64 {
    HOTPLUGS.load(Ordering::Relaxed)
}

pub fn cap_names(caps: u32) -> Vec<&'static str> {
    let mut list = Vec::new();
    for (bit, text) in [
        (CAP_FLUSH, "flush"),
        (CAP_FILL, "fill"),
        (CAP_COPY, "copy"),
        (CAP_MODESET, "modeset"),
        (CAP_CURSOR, "cursor"),
        (CAP_REFRESH, "refresh"),
        (CAP_SCALE, "scale"),
        (CAP_FLIP, "flip"),
        (CAP_VBLANK, "vblank"),
        (CAP_CONNECTORS, "connectors"),
        (CAP_HOTPLUG, "hotplug"),
    ] {
        if caps & bit != 0 {
            list.push(text);
        }
    }
    list
}

pub fn describe() -> String {
    if !active() {
        return match crate::module::display_failure() {
            Some(reason) => alloc::format!("provider\tnone\nreason\t{}\n", reason),
            None => String::from("provider\tnone\n"),
        };
    }
    let (flushes, pixels) = stats();
    let mut out = alloc::format!(
        "provider\t{}\nabi\t{}\ncaps\t{}\nflushes\t{}\npixels\t{}\ncursor\t{}\nbuffers\t{}\nhotplugs\t{}\n",
        name(),
        PROVIDER.lock().as_ref().map(|p| p.ops.abi).unwrap_or(0),
        cap_names(caps()).join(","),
        flushes,
        pixels,
        if cursor_owned() { "hardware" } else { "software" },
        buffers(),
        hotplugs()
    );
    for c in connectors() {
        out.push_str(&alloc::format!(
            "connector\t{}\t{}\t{}\t{}x{}\t{} Hz\tedid {} bytes\n",
            c.id,
            c.name(),
            if c.status != 0 { "connected" } else { "disconnected" },
            c.native_w,
            c.native_h,
            c.refresh,
            c.edid_len
        ));
    }
    out
}

extern "C" fn daemon(_: u64) -> ! {
    loop {
        if crate::task::display::owner().is_none() || crate::task::display::suspended() {
            flush_pending();
        }
        crate::drivers::virtio::gpu::poll_events();
        crate::task::sleep_ticks((crate::task::TICK_HZ / 30).max(1));
    }
}

pub fn start_daemon() {
    if active() {
        crate::task::spawn_kernel_thread("gpud", 0, daemon, 0);
    }
}
