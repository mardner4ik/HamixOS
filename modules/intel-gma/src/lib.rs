#![no_std]

extern crate alloc;

mod display;
mod gmbus;
mod models;

use alloc::format;
use hamix_kpi::io::Mmio;
use hamix_kpi::{self as kpi, ConnectorDesc, DisplayOps, FbDesc, PciHandle};

use display::{Cursor, Failure, Output, Port};
use models::MODELS;

const VENDOR: u16 = 0x8086;
const GMCH_CONTROL: u8 = 0x52;

const SIZES: [(u32, u32); 14] = [
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
];

struct Device {
    mmio: Mmio,
    output: Output,
    fb: FbDesc,
    scan_bytes: u64,
    current: (u32, u32),
    refresh: u32,
    cursor: Option<Cursor>,
    connector: ConnectorDesc,
    edid: [u8; gmbus::EDID_LEN],
    edid_len: usize,
}

static mut DEVICE: Option<Device> = None;

fn device() -> Option<&'static mut Device> {
    unsafe { (&mut *(&raw mut DEVICE)).as_mut() }
}

fn stolen_bytes(gmch: u16) -> u64 {
    const MB: u64 = 1 << 20;
    match (gmch >> 4) & 0xF {
        0x1 => MB,
        0x2 => 4 * MB,
        0x3 => 8 * MB,
        0x4 => 16 * MB,
        0x5 => 32 * MB,
        0x6 => 48 * MB,
        0x7 => 64 * MB,
        0x8 => 128 * MB,
        0x9 => 256 * MB,
        0xA => 96 * MB,
        0xB => 160 * MB,
        0xC => 224 * MB,
        0xD => 352 * MB,
        _ => 8 * MB,
    }
}

fn host_stolen() -> u64 {
    match kpi::find_class(0x06, 0x00, 0x00, 0) {
        Some(host) if host.vendor == VENDOR => stolen_bytes(unsafe { kpi::hamix_pci_read16(&host, GMCH_CONTROL) }),
        _ => 8 << 20,
    }
}

fn fb_bytes(fb: &FbDesc) -> u64 {
    fb.pitch as u64 * fb.height as u64
}

fn scanout_capacity(handle: &PciHandle, fb: &FbDesc, surface: u32, stolen: u64) -> u64 {
    let (aperture, aperture_len) = kpi::bar(handle, 2);
    if aperture == 0 || fb.addr < aperture || fb.addr >= aperture + aperture_len {
        return fb_bytes(fb);
    }
    let reserve = display::CURSOR_BYTES + 4096;
    let stolen_room = stolen.saturating_sub(surface as u64).saturating_sub(reserve);
    let aperture_room = (aperture + aperture_len - fb.addr).saturating_sub(reserve);
    (stolen_room.min(aperture_room) & !0xFFF).max(fb_bytes(fb))
}

fn fits(device: &Device, width: u32, height: u32) -> bool {
    display::pitch_for(width) as u64 * height as u64 <= device.scan_bytes
}

fn mode_options(device: &Device) -> [(u32, u32); 16] {
    let mut out = [(0u32, 0u32); 16];
    let native = (device.output.native.hactive, device.output.native.vactive);
    if !device.output.settable() {
        out[0] = device.current;
        return out;
    }
    let mut at = 0usize;
    if fits(device, native.0, native.1) {
        out[at] = native;
        at += 1;
    }
    for (width, height) in SIZES {
        if at >= out.len() {
            break;
        }
        if width > native.0 || height > native.1 || out.contains(&(width, height)) || !fits(device, width, height) {
            continue;
        }
        out[at] = (width, height);
        at += 1;
    }
    if !out.contains(&device.current) && at < out.len() {
        out[at] = device.current;
    }
    out
}

fn refresh_options(device: &Device) -> [u32; 4] {
    let native = device.output.native_hz();
    let mut out = [native, 0, 0, 0];
    if !device.output.settable() || device.output.clock_khz == 0 {
        return out;
    }
    let mut at = 1usize;
    for candidate in [50u32, 48, 40] {
        if candidate < native && display::vtotal_for(&device.output, candidate).is_some() && at < out.len() {
            out[at] = candidate;
            at += 1;
        }
    }
    out
}

fn map_scanout(device: &Device, pitch: u32, height: u32) -> bool {
    let bytes = pitch as u64 * height as u64;
    if bytes > device.scan_bytes.max(fb_bytes(&device.fb)) {
        return false;
    }
    bytes <= fb_bytes(&device.fb) || !unsafe { kpi::hamix_ioremap(device.fb.addr, bytes as usize) }.is_null()
}

fn failure_text(failure: &Failure) -> &'static str {
    match failure {
        Failure::PipeStuck => "the display pipe did not respond",
        Failure::TooLarge => "the mode is larger than the panel",
        Failure::BadRefresh => "the panel cannot use this refresh rate",
    }
}

fn program(device: &mut Device, width: u32, height: u32, refresh: u32) -> i32 {
    let pitch = display::pitch_for(width);
    if !map_scanout(device, pitch, height) {
        return -12;
    }
    match display::apply(&device.mmio, &device.output, width, height, refresh) {
        Ok(pitch) => {
            let resized = (width, height) != device.current;
            device.current = (width, height);
            device.refresh = refresh;
            if resized && !kpi::display_changed(&FbDesc { addr: device.fb.addr, pitch, width, height, bpp: 32 }) {
                return -5;
            }
            0
        }
        Err(failure) => {
            kpi::dev_warn(failure_text(&failure));
            let (w, h) = device.current;
            let _ = display::apply(&device.mmio, &device.output, w, h, device.refresh);
            -5
        }
    }
}

extern "C" fn set_mode(_context: u64, width: u32, height: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if (width, height) == device.current {
        return 0;
    }
    if !mode_options(device).contains(&(width, height)) {
        return -22;
    }
    let refresh = device.refresh;
    program(device, width, height, refresh)
}

extern "C" fn set_refresh(_context: u64, refresh_hz: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let wanted = if refresh_hz == 0 { device.output.native_hz() } else { refresh_hz };
    if wanted == device.refresh {
        return 0;
    }
    if !refresh_options(device).contains(&wanted) {
        return -22;
    }
    let (w, h) = device.current;
    program(device, w, h, wanted)
}

extern "C" fn mode_list(_context: u64, out: *mut u32, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if out.is_null() || max == 0 {
        return -22;
    }
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 2) };
    let mut count = 0usize;
    for (width, height) in mode_options(device) {
        if width == 0 || count >= max as usize {
            continue;
        }
        target[count * 2] = width;
        target[count * 2 + 1] = height;
        count += 1;
    }
    count as i32
}

extern "C" fn refresh_list(_context: u64, out: *mut u32, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if out.is_null() || max == 0 {
        return -22;
    }
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize) };
    let mut count = 0usize;
    for value in refresh_options(device) {
        if value == 0 || count >= target.len() {
            continue;
        }
        target[count] = value;
        count += 1;
    }
    count as i32
}

extern "C" fn list_connectors(_context: u64, out: *mut ConnectorDesc, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if out.is_null() || max == 0 {
        return -22;
    }
    unsafe { out.write(device.connector) };
    1
}

extern "C" fn connector_modes(_context: u64, id: u32, out: *mut u32, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if id != 0 || out.is_null() || max == 0 {
        return -22;
    }
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 3) };
    let native = device.output.native_hz();
    let mut count = 0usize;
    for (w, h) in mode_options(device) {
        if w == 0 || count >= max as usize {
            continue;
        }
        target[count * 3] = w;
        target[count * 3 + 1] = h;
        target[count * 3 + 2] = native;
        count += 1;
    }
    count as i32
}

extern "C" fn read_edid(_context: u64, id: u32, out: *mut u8, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if id != 0 || out.is_null() {
        return -22;
    }
    let n = device.edid_len.min(max as usize);
    unsafe { core::ptr::copy_nonoverlapping(device.edid.as_ptr(), out, n) };
    n as i32
}

extern "C" fn set_output(_context: u64, id: u32, width: u32, height: u32) -> i32 {
    if id == 0 { set_mode(0, width, height) } else { -95 }
}

extern "C" fn wait_vblank(_context: u64, timeout_ms: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if display::wait_vblank(&device.mmio, device.output.pipe, timeout_ms) { 0 } else { -110 }
}

extern "C" fn cursor_set(_context: u64, pixels: *const u32, width: u32, height: u32, hot_x: u32, hot_y: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if pixels.is_null() {
        return -22;
    }
    let source = unsafe { core::slice::from_raw_parts(pixels, (width * height) as usize) };
    let mmio = &device.mmio;
    match device.cursor.as_mut() {
        Some(cursor) => {
            if cursor.upload(mmio, source, width, height, hot_x, hot_y) { 0 } else { -22 }
        }
        None => -95,
    }
}

extern "C" fn cursor_move(_context: u64, x: i32, y: i32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    match device.cursor.as_ref() {
        Some(cursor) => {
            cursor.moveto(&device.mmio, x, y);
            0
        }
        None => -95,
    }
}

extern "C" fn cursor_hide(_context: u64) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let mmio = &device.mmio;
    match device.cursor.as_mut() {
        Some(cursor) => {
            cursor.hide(mmio);
            0
        }
        None => -95,
    }
}

fn connector_for(output: &Output) -> ConnectorDesc {
    let (kind, name) = match output.port {
        Port::Lvds => (kpi::CONNECTOR_LVDS, "LVDS-1"),
        Port::Vga => (kpi::CONNECTOR_VGA, "VGA-1"),
        Port::Digital => (kpi::CONNECTOR_DVI, "DVI-1"),
        Port::Unknown => (kpi::CONNECTOR_UNKNOWN, "Unknown-1"),
    };
    let mut desc = ConnectorDesc::named(kind, 0, name);
    desc.status = 1;
    desc.primary = 1;
    desc.native_w = output.native.hactive;
    desc.native_h = output.native.vactive;
    desc.refresh = output.native_hz();
    desc
}

fn init() -> i32 {
    if unsafe { kpi::hamix_display_abi() } != kpi::DISPLAY_ABI {
        kpi::printk("intel-gma: kernel display ABI does not match this module");
        return -1;
    }
    let mut handle = PciHandle::default();
    let Some(model) = MODELS.iter().find(|m| unsafe { kpi::hamix_pci_find(VENDOR, m.device, &mut handle) } == 0) else {
        kpi::printk("intel-gma: no gen4 Intel graphics device");
        return -1;
    };
    kpi::display_describe(model.name, model.codename, "gen4");
    let Some(fb) = kpi::current_framebuffer() else {
        kpi::printk("intel-gma: no framebuffer to take over");
        return -1;
    };
    let (mmio_base, mmio_len) = kpi::bar(&handle, 0);
    if mmio_base == 0 || mmio_len < 0x80000 {
        kpi::printk("intel-gma: register window is missing or too small");
        return -1;
    }
    let Some(mmio) = Mmio::map(mmio_base, mmio_len.min(0x80000)) else {
        kpi::printk("intel-gma: cannot map the register window");
        return -1;
    };
    if mmio.read32(0x70008) == 0xFFFF_FFFF {
        kpi::printk("intel-gma: registers read back all ones, the device is powered down");
        return -1;
    }
    let Some(output) = display::detect(&mmio) else {
        kpi::printk("intel-gma: no active display pipe");
        return -1;
    };
    let stolen = host_stolen();
    let surface = display::surface(&mmio, &output);
    let scan_bytes = scanout_capacity(&handle, &fb, surface, stolen);
    let mut edid = [0u8; gmbus::EDID_LEN];
    let edid_len = if kpi::param("edid", 1) != 0 {
        let pin = if output.port == Port::Vga { gmbus::PIN_VGA } else { gmbus::PIN_PANEL };
        if gmbus::read_edid(&mmio, pin, &mut edid) { gmbus::EDID_LEN } else { 0 }
    } else {
        0
    };
    let mut connector = connector_for(&output);
    connector.edid_len = edid_len as u32;
    let cursor_offset = (scan_bytes + 0xFFF) & !0xFFF;
    let cursor = if kpi::param("cursor", 0) != 0 && surface != 0 && surface as u64 + cursor_offset + display::CURSOR_BYTES <= stolen {
        Some(Cursor { pipe: output.pipe, g4x: model.g4x, aperture: (fb.addr + cursor_offset) as *mut u8, ggtt: surface + cursor_offset as u32, hot: (0, 0), armed: false })
    } else {
        None
    };
    let current = (fb.width, fb.height);
    let refresh = output.native_hz();
    let device = Device { mmio, output, fb, scan_bytes, current, refresh, cursor, connector, edid, edid_len };
    kpi::printk(&format!(
        "intel-gma: {} pipe {} {} native {}x{} source {}x{} clock {} kHz {} Hz, stolen {} MiB, scanout room {} MiB{}",
        model.name,
        if output.pipe == 0 { 'A' } else { 'B' },
        match output.port {
            Port::Lvds => "LVDS",
            Port::Vga => "VGA",
            Port::Digital => "SDVO",
            Port::Unknown => "unknown port",
        },
        output.native.hactive,
        output.native.vactive,
        output.source.0,
        output.source.1,
        output.clock_khz,
        output.native_hz(),
        stolen >> 20,
        scan_bytes >> 20,
        if edid_len > 0 { ", EDID" } else { "" }
    ));
    let has_cursor = device.cursor.is_some();
    let settable = device.output.settable();
    unsafe { *(&raw mut DEVICE) = Some(device) };

    let mut caps = kpi::CAP_CONNECTORS | kpi::CAP_VBLANK;
    if settable {
        caps |= kpi::CAP_MODESET | kpi::CAP_SCALE;
        if output.clock_khz > 0 {
            caps |= kpi::CAP_REFRESH;
        }
    }
    if has_cursor {
        caps |= kpi::CAP_CURSOR;
    }
    let mut ops = DisplayOps::new(caps, 0);
    ops.connectors = Some(list_connectors);
    ops.connector_modes = Some(connector_modes);
    ops.edid = Some(read_edid);
    ops.wait_vblank = Some(wait_vblank);
    ops.mode_list = Some(mode_list);
    ops.refresh_list = Some(refresh_list);
    if settable {
        ops.set_mode = Some(set_mode);
        ops.set_output = Some(set_output);
        if output.clock_khz > 0 {
            ops.set_refresh = Some(set_refresh);
        }
    }
    if has_cursor {
        ops.cursor_set = Some(cursor_set);
        ops.cursor_move = Some(cursor_move);
        ops.cursor_hide = Some(cursor_hide);
    }
    if !kpi::register_display("intel-gma", &ops, &fb) {
        kpi::printk("intel-gma: the kernel refused the display registration");
        unsafe { *(&raw mut DEVICE) = None };
        return -1;
    }
    if !kpi::claim(kpi::CLASS_DISPLAY, "intel-gma", &handle, None, core::ptr::null_mut()) {
        return -1;
    }
    0
}

fn exit() {
    if let Some(device) = device() {
        let mmio = &device.mmio;
        if let Some(cursor) = device.cursor.as_mut() {
            cursor.hide(mmio);
        }
        let (w, h) = device.output.source;
        if device.current != (w, h) || device.refresh != device.output.native_hz() {
            let _ = display::apply(&device.mmio, &device.output, w, h, device.output.native_hz());
        }
    }
    unsafe { *(&raw mut DEVICE) = None };
}

hamix_kpi::kernel_heap!();

hamix_kpi::module!(init = init, exit = exit, version = "1.0");
