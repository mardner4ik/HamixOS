#![no_std]

mod aux;
mod connectors;
mod cursor;
mod edid;
mod mode;
mod regs;

use hamix_kpi::{self as kpi, ConnectorDesc, DisplayOps, FbDesc, PciHandle};

use mode::Output;
use regs::{Gen, Mmio};

const VENDOR: u16 = 0x8086;
const GMCH_CONTROL: u8 = 0x50;

struct Device {
    mmio: Mmio,
    chip: Gen,
    output: Output,
    limits: Option<edid::Limits>,
    cursor: Option<cursor::Plane>,
    stolen: u64,
    applied_vtotal: u32,
    fb: FbDesc,
    scan_bytes: u64,
    current: (u32, u32),
    scalable: bool,
    connectors: [connectors::Connector; connectors::MAX],
    connector_count: usize,
    surface: u32,
    flip: bool,
    front: u32,
}

const SIZES: [(u32, u32); 16] = [
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

fn mode_options(device: &Device) -> [(u32, u32); 16] {
    let mut out = [(0u32, 0u32); 16];
    out[0] = device.output.source;
    let native = (device.output.native.hactive, device.output.native.vactive);
    if native != out[0] && mode::pitch_for(native.0) as u64 * native.1 as u64 <= device.scan_bytes {
        out[0] = native;
    }
    if !device.scalable {
        return out;
    }
    let mut at = 1usize;
    for (width, height) in SIZES {
        if at >= out.len() {
            break;
        }
        if width > device.output.native.hactive || height > device.output.native.vactive {
            continue;
        }
        if out.contains(&(width, height)) {
            continue;
        }
        if mode::pitch_for(width) as u64 * height as u64 > device.scan_bytes {
            continue;
        }
        out[at] = (width, height);
        at += 1;
    }
    out
}

fn map_scanout(device: &Device, pitch: u32, height: u32) -> bool {
    let bytes = pitch as u64 * height as u64;
    if bytes > device.scan_bytes.max(device.fb.pitch as u64 * device.fb.height as u64) {
        return false;
    }
    bytes <= device.fb.pitch as u64 * device.fb.height as u64 || !unsafe { kpi::hamix_ioremap(device.fb.addr, bytes as usize) }.is_null()
}

static mut DEVICE: Option<Device> = None;

fn device() -> Option<&'static mut Device> {
    unsafe { (&mut *(&raw mut DEVICE)).as_mut() }
}

fn stolen_bytes(igd: &PciHandle, chip: Gen) -> u64 {
    const MB: u64 = 1 << 20;
    let gmch = unsafe { kpi::hamix_pci_read16(igd, GMCH_CONTROL) } as u64;
    let bytes = match chip {
        Gen::Gen6 | Gen::Gen7 | Gen::Gen7_5 => ((gmch >> 3) & 0x1F) * 32 * MB,
        Gen::Gen8 => ((gmch >> 8) & 0xFF) * 32 * MB,
        Gen::Gen9 => {
            let gms = (gmch >> 8) & 0xFF;
            if gms < 0xF0 { gms * 32 * MB } else { (gms - 0xF0 + 1) * 4 * MB }
        }
    };
    if bytes == 0 { 32 * MB } else { bytes }
}

fn scanout_capacity(handle: &PciHandle, fb: &FbDesc, surface: u32, stolen: u64) -> u64 {
    let fb_bytes = fb.pitch as u64 * fb.height as u64;
    let (aperture, aperture_len) = kpi::bar(handle, 2);
    if aperture == 0 || fb.addr < aperture || fb.addr >= aperture + aperture_len {
        return fb_bytes;
    }
    let reserve = (cursor::BYTES as u64).div_ceil(4096) * 4096 + 4096;
    let stolen_room = stolen.saturating_sub(surface as u64).saturating_sub(reserve);
    let aperture_room = (aperture + aperture_len - fb.addr).saturating_sub(reserve);
    let room = stolen_room.min(aperture_room) & !0xFFF;
    room.max(fb_bytes)
}

fn refresh_options(device: &Device) -> [u32; 6] {
    let native = (device.output.refresh_mhz + 500) / 1000;
    let mut out = [0u32; 6];
    out[0] = native;
    let floor = device.limits.map(|l| l.min_vertical_hz).filter(|v| *v >= 20).unwrap_or(0);
    let mut at = 1usize;
    for candidate in [50u32, 48, 40, 30] {
        if !device.output.measured {
            break;
        }
        if candidate >= native || at >= out.len() {
            continue;
        }
        if floor > 0 && candidate < floor {
            continue;
        }
        if mode::vtotal_for_refresh(&device.output, candidate).is_some() {
            out[at] = candidate;
            at += 1;
        }
    }
    out
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
    let pitch = mode::pitch_for(width);
    if !map_scanout(device, pitch, height) {
        return -12;
    }
    if !mode::set_source(&device.mmio, device.chip, &device.output, width, height, pitch) {
        let previous = device.current;
        mode::set_source(&device.mmio, device.chip, &device.output, previous.0, previous.1, mode::pitch_for(previous.0));
        return -5;
    }
    device.current = (width, height);
    let desc = FbDesc { addr: device.fb.addr, pitch, width, height, bpp: 32 };
    if !kpi::display_changed(&desc) {
        return -5;
    }
    0
}

extern "C" fn mode_list(_context: u64, out: *mut u32, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if out.is_null() || max == 0 {
        return -22;
    }
    let options = mode_options(device);
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 2) };
    let mut count = 0usize;
    for (width, height) in options {
        if width == 0 || height == 0 || count >= max as usize {
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
    let options = refresh_options(device);
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize) };
    let mut count = 0usize;
    for value in options {
        if value == 0 || count >= target.len() {
            continue;
        }
        target[count] = value;
        count += 1;
    }
    count as i32
}

extern "C" fn set_refresh(_context: u64, refresh_hz: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let native = (device.output.refresh_mhz + 500) / 1000;
    if refresh_hz == 0 || refresh_hz == native {
        mode::restore(&device.mmio, device.chip, &device.output);
        device.applied_vtotal = device.output.native.vtotal;
        return 0;
    }
    if !refresh_options(device).contains(&refresh_hz) {
        return -22;
    }
    let Some(vtotal) = mode::vtotal_for_refresh(&device.output, refresh_hz) else {
        return -22;
    };
    if !mode::set_vtotal(&device.mmio, device.chip, &device.output, vtotal) {
        mode::restore(&device.mmio, device.chip, &device.output);
        return -5;
    }
    device.applied_vtotal = vtotal;
    0
}

extern "C" fn cursor_set(_context: u64, pixels: *const u32, width: u32, height: u32, hot_x: u32, hot_y: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if pixels.is_null() {
        return -22;
    }
    let source = unsafe { core::slice::from_raw_parts(pixels, (width * height) as usize) };
    let chip = device.chip;
    let mmio_ptr = &device.mmio as *const Mmio;
    let Some(plane) = device.cursor.as_mut() else {
        return -95;
    };
    let mmio = unsafe { &*mmio_ptr };
    if plane.upload(mmio, chip, source, width, height, hot_x, hot_y) { 0 } else { -22 }
}

extern "C" fn cursor_move(_context: u64, x: i32, y: i32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let mmio_ptr = &device.mmio as *const Mmio;
    let Some(plane) = device.cursor.as_ref() else {
        return -95;
    };
    plane.moveto(unsafe { &*mmio_ptr }, x, y);
    0
}

extern "C" fn cursor_hide(_context: u64) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let mmio_ptr = &device.mmio as *const Mmio;
    let Some(plane) = device.cursor.as_mut() else {
        return -95;
    };
    plane.hide(unsafe { &*mmio_ptr });
    0
}

struct Line {
    bytes: [u8; 192],
    at: usize,
}

impl Line {
    fn new() -> Line {
        Line { bytes: [0; 192], at: 0 }
    }

    fn text(&mut self, text: &str) {
        self.bytes_text(text.as_bytes());
    }

    fn bytes_text(&mut self, text: &[u8]) {
        for byte in text {
            if self.at < self.bytes.len() {
                self.bytes[self.at] = *byte;
                self.at += 1;
            }
        }
    }

    fn number(&mut self, value: u32) {
        let mut digits = [0u8; 10];
        let mut count = 0usize;
        let mut value = value;
        if value == 0 {
            digits[0] = b'0';
            count = 1;
        }
        while value > 0 && count < digits.len() {
            digits[count] = b'0' + (value % 10) as u8;
            value /= 10;
            count += 1;
        }
        while count > 0 {
            count -= 1;
            if self.at < self.bytes.len() {
                self.bytes[self.at] = digits[count];
                self.at += 1;
            }
        }
    }

    fn emit(&self) {
        unsafe { kpi::hamix_printk(self.bytes.as_ptr(), self.at) };
    }
}

fn report(device: &Device) {
    let native = (device.output.refresh_mhz + 500) / 1000;
    let mut line = Line::new();
    line.text("intel-display: ");
    line.text(device.chip.as_str());
    line.text(" pipe ");
    line.number(device.output.pipe as u32);
    line.text(" ");
    line.number(device.output.source.0);
    line.text("x");
    line.number(device.output.source.1);
    line.text(" at ");
    line.number(native);
    line.text(" Hz");
    match device.limits {
        Some(limits) if limits.max_vertical_hz > 0 => {
            line.text(", panel ");
            line.number(limits.min_vertical_hz);
            line.text("-");
            line.number(limits.max_vertical_hz);
            line.text(" Hz");
        }
        _ => line.text(", no EDID"),
    }
    if device.cursor.is_some() {
        line.text(", hardware cursor");
    }
    if device.scalable {
        line.text(if device.chip.pipe_scaler() { ", pipe scaler" } else { ", panel fitter" });
    }
    line.emit();
    for connector in device.connectors.iter().take(device.connector_count) {
        let mut line = Line::new();
        line.text("intel-display: connector ");
        let end = connector.desc.name.iter().position(|b| *b == 0).unwrap_or(16);
        line.bytes_text(&connector.desc.name[..end]);
        line.text(if connector.desc.status != 0 { " connected" } else { " disconnected" });
        if connector.edid_len > 0 {
            line.text(", EDID ");
            line.number(connector.edid_len as u32);
            line.text(" bytes");
        }
        line.emit();
    }
}

extern "C" fn list_connectors(_context: u64, out: *mut ConnectorDesc, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if out.is_null() {
        return -22;
    }
    let count = device.connector_count.min(max as usize);
    for i in 0..count {
        unsafe { out.add(i).write(device.connectors[i].desc) };
    }
    count as i32
}

extern "C" fn connector_modes(_context: u64, id: u32, out: *mut u32, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let Some(connector) = device.connectors.get(id as usize).filter(|_| (id as usize) < device.connector_count) else {
        return -22;
    };
    if out.is_null() || max == 0 {
        return -22;
    }
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 3) };
    if connector.active {
        let native = (device.output.refresh_mhz + 500) / 1000;
        let options = mode_options(device);
        let mut count = 0usize;
        for (w, h) in options {
            if w == 0 || count >= max as usize {
                continue;
            }
            target[count * 3] = w;
            target[count * 3 + 1] = h;
            target[count * 3 + 2] = native;
            count += 1;
        }
        return count as i32;
    }
    if connector.edid_len < 128 {
        return 0;
    }
    kpi::edid_modes(&connector.edid[..connector.edid_len], target) as i32
}

extern "C" fn read_edid(_context: u64, id: u32, out: *mut u8, max: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let Some(connector) = device.connectors.get(id as usize).filter(|_| (id as usize) < device.connector_count) else {
        return -22;
    };
    if out.is_null() {
        return -22;
    }
    let n = connector.edid_len.min(max as usize);
    unsafe { core::ptr::copy_nonoverlapping(connector.edid.as_ptr(), out, n) };
    n as i32
}

extern "C" fn set_output(_context: u64, id: u32, width: u32, height: u32) -> i32 {
    if id == 0 {
        return set_mode(0, width, height);
    }
    -95
}

extern "C" fn wait_vblank(_context: u64, timeout_ms: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    let reg = regs::frames(device.output.pipe);
    let first = device.mmio.read(reg);
    let mut waited = 0u64;
    while waited < timeout_ms.max(1) as u64 * 1000 {
        if device.mmio.read(reg) != first {
            return 0;
        }
        unsafe { kpi::hamix_udelay(100) };
        waited += 100;
    }
    -110
}

extern "C" fn page_flip(_context: u64, index: u32) -> i32 {
    let Some(device) = device() else {
        return -19;
    };
    if !device.flip || index > 1 {
        return -95;
    }
    let (w, h) = device.current;
    let stride = ((mode::pitch_for(w) as u64 * h as u64).div_ceil(4096) * 4096) as u32;
    let plane = regs::plane(device.output.pipe);
    device.mmio.write(plane + regs::PLANE_SURF, device.surface + index * stride);
    device.front = index;
    0
}

fn init() -> i32 {
    if unsafe { kpi::hamix_display_abi() } != kpi::DISPLAY_ABI {
        kpi::printk("intel-display: kernel display ABI does not match this module");
        return -1;
    }
    let mut handle = PciHandle::default();
    let mut found = None;
    for model in MODELS {
        if unsafe { kpi::hamix_pci_find(VENDOR, model.device, &mut handle) } == 0 {
            found = Gen::of(model.device).map(|chip| (chip, model));
            break;
        }
    }
    let Some((chip, model)) = found else {
        kpi::printk("intel-display: no supported Intel graphics device");
        return -1;
    };
    kpi::display_describe(model.name, model.codename, chip.generation());
    let Some(fb) = kpi::current_framebuffer() else {
        kpi::printk("intel-display: no framebuffer to take over");
        return -1;
    };
    let (mmio_base, mmio_len) = kpi::bar(&handle, 0);
    if mmio_base == 0 || mmio_len < 0x80000 {
        kpi::printk("intel-display: register window is missing or too small");
        return -1;
    }
    let mmio = Mmio::new(unsafe { kpi::hamix_ioremap(mmio_base, mmio_len as usize) }, mmio_len);
    if !mmio.alive() {
        kpi::printk("intel-display: registers read back all ones, the device is powered down");
        return -1;
    }
    let Some(output) = mode::detect(&mmio, chip) else {
        kpi::printk("intel-display: no active display pipe");
        return -1;
    };
    if !output.measured {
        kpi::printk("intel-display: the pipe frame counter is not advancing, assuming 60 Hz and keeping the refresh rate fixed");
    }

    let stolen = stolen_bytes(&handle, chip);
    let allow_edid = kpi::param("edid", 1) != 0;
    let (connectors, connector_count) = connectors::detect(&mmio, chip, &output, allow_edid);
    let limits = if connectors[0].edid_len >= 128 {
        Some(edid::limits_of(&connectors[0].edid))
    } else if allow_edid {
        edid::read(&mmio)
    } else {
        None
    };
    let fb_bytes = fb.pitch as u64 * fb.height as u64;
    let stride = fb_bytes.div_ceil(4096) * 4096;
    let surface = mmio.read(regs::plane(output.pipe) + regs::PLANE_SURF) & !0xFFF;
    let flip = kpi::param("flip", 0) != 0 && stride * 2 + 64 * 1024 <= stolen;
    let scan_bytes = if flip || kpi::param("grow", 1) == 0 { fb_bytes } else { scanout_capacity(&handle, &fb, surface, stolen) };
    let cursor_offset = if flip { stride * 2 } else { scan_bytes.max(fb_bytes) };
    let cursor = cursor::Plane::attach(&mmio, chip, output.pipe, fb.addr, cursor_offset, stolen.saturating_sub(surface as u64));

    let applied_vtotal = output.native.vtotal;
    let current = (fb.width, fb.height);
    let scalable = mode::scalable(&output) && kpi::param("scaler", 1) != 0;
    let device = Device {
        mmio,
        chip,
        output,
        limits,
        cursor,
        stolen,
        applied_vtotal,
        fb,
        scan_bytes,
        current,
        scalable,
        connectors,
        connector_count,
        surface,
        flip,
        front: 0,
    };
    let mut device = device;
    let mut desc = FbDesc { addr: fb.addr, pitch: fb.pitch, width: fb.width, height: fb.height, bpp: 32 };
    let preferred = mode_options(&device)[0];
    if kpi::param("native", 1) != 0 && preferred != device.current && preferred.0 > 0 {
        let pitch = mode::pitch_for(preferred.0);
        if map_scanout(&device, pitch, preferred.1) && mode::set_source(&device.mmio, device.chip, &device.output, preferred.0, preferred.1, pitch) {
            device.current = preferred;
            desc = FbDesc { addr: fb.addr, pitch, width: preferred.0, height: preferred.1, bpp: 32 };
            clear_scanout(&desc);
        } else {
            let source = device.output.source;
            mode::set_source(&device.mmio, device.chip, &device.output, source.0, source.1, fb.pitch);
        }
    }
    report(&device);
    let has_cursor = device.cursor.is_some();
    unsafe { *(&raw mut DEVICE) = Some(device) };

    let measured = device_measured();
    let mut caps = kpi::CAP_MODESET | kpi::CAP_CONNECTORS | kpi::CAP_VBLANK;
    if measured {
        caps |= kpi::CAP_REFRESH;
    }
    if flip {
        caps |= kpi::CAP_FLIP;
    }
    if scalable {
        caps |= kpi::CAP_SCALE;
    }
    if has_cursor {
        caps |= kpi::CAP_CURSOR;
    }
    let mut ops = DisplayOps::new(caps, 0);
    ops.set_mode = Some(set_mode);
    ops.refresh_list = Some(refresh_list);
    ops.mode_list = Some(mode_list);
    ops.connectors = Some(list_connectors);
    ops.connector_modes = Some(connector_modes);
    ops.edid = Some(read_edid);
    ops.set_output = Some(set_output);
    ops.wait_vblank = Some(wait_vblank);
    if flip {
        ops.page_flip = Some(page_flip);
        ops.buffers = 2;
    }
    if measured {
        ops.set_refresh = Some(set_refresh);
    }
    if has_cursor {
        ops.cursor_set = Some(cursor_set);
        ops.cursor_move = Some(cursor_move);
        ops.cursor_hide = Some(cursor_hide);
    }
    if !kpi::register_display("intel-display", &ops, &desc) {
        kpi::printk("intel-display: the kernel refused the display registration");
        return -1;
    }
    if !kpi::claim(kpi::CLASS_DISPLAY, "intel-display", &handle, None, core::ptr::null_mut()) {
        return -1;
    }
    0
}

fn clear_scanout(desc: &FbDesc) {
    let words = (desc.pitch as usize / 4) * desc.height as usize;
    let pixels = unsafe { core::slice::from_raw_parts_mut(desc.addr as *mut u32, words) };
    for pixel in pixels.iter_mut() {
        *pixel = 0;
    }
}

fn device_measured() -> bool {
    device().map(|d| d.output.measured).unwrap_or(false)
}

fn exit() {
    if let Some(device) = device() {
        if device.front != 0 {
            let plane = regs::plane(device.output.pipe);
            device.mmio.write(plane + regs::PLANE_SURF, device.surface);
        }
        let native = device.output.source;
        mode::set_source(&device.mmio, device.chip, &device.output, native.0, native.1, device.fb.pitch);
        mode::restore(&device.mmio, device.chip, &device.output);
        let mmio_ptr = &device.mmio as *const Mmio;
        if let Some(plane) = device.cursor.as_mut() {
            plane.hide(unsafe { &*mmio_ptr });
        }
        let _ = device.stolen;
        let _ = device.applied_vtotal;
    }
}

include!("ids.rs");

hamix_kpi::module!(init = init, exit = exit, version = "1.1");
