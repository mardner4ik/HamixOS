use hamix_kpi::io::{delay_us, Mmio};

const DPLL_A: u32 = 0x6014;
const DPLL_B: u32 = 0x6018;
const FPA0: u32 = 0x6040;
const FPB0: u32 = 0x6048;
const PIPE_TIMING_A: u32 = 0x60000;
const PIPE_TIMING_B: u32 = 0x61000;
const HTOTAL: u32 = 0x00;
const HBLANK: u32 = 0x04;
const HSYNC: u32 = 0x08;
const VTOTAL: u32 = 0x0C;
const VBLANK: u32 = 0x10;
const VSYNC: u32 = 0x14;
const PIPESRC: u32 = 0x1C;
const PIPECONF_A: u32 = 0x70008;
const PIPECONF_B: u32 = 0x71008;
const PIPESTAT_A: u32 = 0x70024;
const PIPESTAT_B: u32 = 0x71024;
const DSPCNTR_A: u32 = 0x70180;
const DSPCNTR_B: u32 = 0x71180;
const DSP_LINOFF: u32 = 0x04;
const DSP_STRIDE: u32 = 0x08;
const DSP_POS: u32 = 0x0C;
const DSP_SIZE: u32 = 0x10;
const DSP_SURF: u32 = 0x1C;
const DSP_TILEOFF: u32 = 0x24;
const LVDS: u32 = 0x61180;
const ADPA: u32 = 0x61100;
const SDVOB: u32 = 0x61140;
const SDVOC: u32 = 0x61160;
const PFIT_CONTROL: u32 = 0x61230;
const PFIT_PGM_RATIOS: u32 = 0x61234;
const CURSOR_A: u32 = 0x70080;
const CURSOR_STRIDE: u32 = 0x40;
const CURSOR_BASE: u32 = 0x04;
const CURSOR_POS: u32 = 0x08;
const CURSOR_MODE_64_ARGB: u32 = 0x27;

pub const ENABLE: u32 = 1 << 31;
const PIPE_STATE: u32 = 1 << 30;
const VBLANK_STATUS: u32 = 1 << 1;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Timing {
    pub hactive: u32,
    pub htotal: u32,
    pub hblank_start: u32,
    pub hblank_end: u32,
    pub hsync_start: u32,
    pub hsync_end: u32,
    pub vactive: u32,
    pub vtotal: u32,
    pub vblank_start: u32,
    pub vblank_end: u32,
    pub vsync_start: u32,
    pub vsync_end: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Port {
    Lvds,
    Vga,
    Digital,
    Unknown,
}

#[derive(Clone, Copy)]
pub struct Output {
    pub pipe: usize,
    pub plane: usize,
    pub port: Port,
    pub native: Timing,
    pub source: (u32, u32),
    pub clock_khz: u32,
    pub refresh_mhz: u32,
    pub fitter: bool,
}

impl Output {
    pub fn native_hz(&self) -> u32 {
        ((self.refresh_mhz + 500) / 1000).max(1)
    }

    pub fn settable(&self) -> bool {
        self.port == Port::Lvds || self.fitter
    }
}

pub enum Failure {
    PipeStuck,
    TooLarge,
    BadRefresh,
}

fn pair(value: u32) -> (u32, u32) {
    ((value & 0xFFF) + 1, ((value >> 16) & 0xFFF) + 1)
}

fn encode(start: u32, end: u32) -> u32 {
    (start.saturating_sub(1) & 0xFFF) | ((end.saturating_sub(1) & 0xFFF) << 16)
}

fn pipe_base(pipe: usize) -> u32 {
    if pipe == 0 { PIPE_TIMING_A } else { PIPE_TIMING_B }
}

fn pipeconf(pipe: usize) -> u32 {
    if pipe == 0 { PIPECONF_A } else { PIPECONF_B }
}

fn pipestat(pipe: usize) -> u32 {
    if pipe == 0 { PIPESTAT_A } else { PIPESTAT_B }
}

fn plane_base(plane: usize) -> u32 {
    if plane == 0 { DSPCNTR_A } else { DSPCNTR_B }
}

pub fn pitch_for(width: u32) -> u32 {
    (width * 4 + 63) & !63
}

pub fn timing(mmio: &Mmio, pipe: usize) -> Timing {
    let base = pipe_base(pipe);
    let (hactive, htotal) = pair(mmio.read32(base + HTOTAL));
    let (hblank_start, hblank_end) = pair(mmio.read32(base + HBLANK));
    let (hsync_start, hsync_end) = pair(mmio.read32(base + HSYNC));
    let (vactive, vtotal) = pair(mmio.read32(base + VTOTAL));
    let (vblank_start, vblank_end) = pair(mmio.read32(base + VBLANK));
    let (vsync_start, vsync_end) = pair(mmio.read32(base + VSYNC));
    Timing { hactive, htotal, hblank_start, hblank_end, hsync_start, hsync_end, vactive, vtotal, vblank_start, vblank_end, vsync_start, vsync_end }
}

fn write_timing(mmio: &Mmio, pipe: usize, t: &Timing) {
    let base = pipe_base(pipe);
    mmio.write32(base + HTOTAL, encode(t.hactive, t.htotal));
    mmio.write32(base + HBLANK, encode(t.hblank_start, t.hblank_end));
    mmio.write32(base + HSYNC, encode(t.hsync_start, t.hsync_end));
    mmio.write32(base + VTOTAL, encode(t.vactive, t.vtotal));
    mmio.write32(base + VBLANK, encode(t.vblank_start, t.vblank_end));
    mmio.write32(base + VSYNC, encode(t.vsync_start, t.vsync_end));
}

fn clock_khz(mmio: &Mmio, pipe: usize) -> Option<u32> {
    let dpll = mmio.read32(if pipe == 0 { DPLL_A } else { DPLL_B });
    if dpll & ENABLE == 0 {
        return None;
    }
    let fp = mmio.read32(if pipe == 0 { FPA0 } else { FPB0 });
    let n = (fp >> 16) & 0x3F;
    let m1 = (fp >> 8) & 0x3F;
    let m2 = fp & 0x3F;
    let onehot = (dpll >> 16) & 0xFF;
    if onehot == 0 {
        return None;
    }
    let p1 = onehot.trailing_zeros() + 1;
    let mode = (dpll >> 26) & 3;
    let p2 = if mode == 2 {
        if (dpll >> 24) & 3 == 2 { 7 } else { 14 }
    } else if (dpll >> 24) & 1 != 0 {
        5
    } else {
        10
    };
    let refclk: u64 = if (dpll >> 13) & 3 == 3 { 100_000 } else { 96_000 };
    let m = 5 * (m1 + 2) + (m2 + 2);
    let vco = refclk * m as u64 / (n + 2) as u64;
    Some((vco / (p1 * p2) as u64) as u32)
}

fn port_of(mmio: &Mmio, pipe: usize) -> Port {
    let on_pipe = |reg: u32| {
        let value = mmio.read32(reg);
        value & ENABLE != 0 && ((value >> 30) & 1) as usize == pipe
    };
    if on_pipe(LVDS) {
        Port::Lvds
    } else if on_pipe(ADPA) {
        Port::Vga
    } else if on_pipe(SDVOB) || on_pipe(SDVOC) {
        Port::Digital
    } else {
        Port::Unknown
    }
}

pub fn detect(mmio: &Mmio) -> Option<Output> {
    let lvds = mmio.read32(LVDS);
    let lvds_pipe = if lvds & ENABLE != 0 { Some(if lvds & (1 << 30) != 0 { 1 } else { 0 }) } else { None };
    let mut found = None;
    for plane in [1usize, 0] {
        let cntr = mmio.read32(plane_base(plane));
        if cntr & ENABLE == 0 {
            continue;
        }
        let pipe = ((cntr >> 24) & 1) as usize;
        if mmio.read32(pipeconf(pipe)) & ENABLE == 0 {
            continue;
        }
        if found.is_none() || lvds_pipe == Some(pipe) {
            found = Some((plane, pipe));
        }
    }
    let (plane, pipe) = found?;
    let native = timing(mmio, pipe);
    if native.hactive < 320 || native.vactive < 200 || native.htotal <= native.hactive || native.vtotal <= native.vactive {
        return None;
    }
    let (h, w) = pair(mmio.read32(pipe_base(pipe) + PIPESRC));
    let clock_khz = clock_khz(mmio, pipe).unwrap_or(0);
    let refresh_mhz = if native.htotal > 0 && native.vtotal > 0 && clock_khz > 0 {
        (clock_khz as u64 * 1_000_000 / (native.htotal as u64 * native.vtotal as u64)) as u32
    } else {
        60_000
    };
    let fitter = mmio.read32(PFIT_CONTROL) & ENABLE != 0;
    Some(Output { pipe, plane, port: port_of(mmio, pipe), native, source: (w, h), clock_khz, refresh_mhz, fitter })
}

pub fn surface(mmio: &Mmio, output: &Output) -> u32 {
    mmio.read32(plane_base(output.plane) + DSP_SURF) & !0xFFF
}

fn wait_pipe(mmio: &Mmio, pipe: usize, on: bool) -> bool {
    for _ in 0..50 {
        if (mmio.read32(pipeconf(pipe)) & PIPE_STATE != 0) == on {
            return true;
        }
        delay_us(4000);
    }
    false
}

pub fn vtotal_for(output: &Output, refresh_hz: u32) -> Option<u32> {
    if output.clock_khz == 0 || output.native.htotal == 0 || refresh_hz == 0 {
        return None;
    }
    let wanted = (output.clock_khz as u64 * 1000 / (output.native.htotal as u64 * refresh_hz as u64)) as u32;
    if wanted < output.native.vtotal || wanted > 4095 {
        return None;
    }
    Some(wanted)
}

pub fn apply(mmio: &Mmio, output: &Output, width: u32, height: u32, refresh_hz: u32) -> Result<u32, Failure> {
    let native = output.native;
    if width > native.hactive || height > native.vactive || width < 320 || height < 200 {
        return Err(Failure::TooLarge);
    }
    let mut timing = native;
    if refresh_hz != 0 && refresh_hz != output.native_hz() {
        let vtotal = vtotal_for(output, refresh_hz).ok_or(Failure::BadRefresh)?;
        let extra = vtotal - native.vtotal;
        timing.vtotal = vtotal;
        timing.vblank_end = native.vblank_end + extra;
    }
    let pitch = pitch_for(width);
    let plane = plane_base(output.plane);
    let pipe = output.pipe;
    let surface = mmio.read32(plane + DSP_SURF);
    let cntr = mmio.read32(plane);
    mmio.write32(plane, cntr & !ENABLE);
    mmio.write32(plane + DSP_SURF, surface);
    delay_us(20_000);
    let conf = mmio.read32(pipeconf(pipe));
    mmio.write32(pipeconf(pipe), conf & !ENABLE);
    if !wait_pipe(mmio, pipe, false) {
        mmio.write32(pipeconf(pipe), conf);
        mmio.write32(plane, cntr);
        mmio.write32(plane + DSP_SURF, surface);
        return Err(Failure::PipeStuck);
    }
    write_timing(mmio, pipe, &timing);
    mmio.write32(pipe_base(pipe) + PIPESRC, ((width - 1) << 16) | (height - 1));
    let scaled = width != native.hactive || height != native.vactive;
    if output.port == Port::Lvds || output.fitter {
        if scaled {
            mmio.write32(PFIT_PGM_RATIOS, 0);
            mmio.write32(PFIT_CONTROL, ENABLE | ((pipe as u32) << 29));
        } else {
            mmio.write32(PFIT_CONTROL, 0);
        }
    }
    mmio.write32(pipeconf(pipe), conf | ENABLE);
    let started = wait_pipe(mmio, pipe, true);
    mmio.write32(plane + DSP_POS, 0);
    mmio.write32(plane + DSP_SIZE, ((height - 1) << 16) | (width - 1));
    mmio.write32(plane + DSP_STRIDE, pitch);
    mmio.write32(plane + DSP_LINOFF, 0);
    mmio.write32(plane + DSP_TILEOFF, 0);
    mmio.write32(plane, (cntr & !(0xF << 26) & !(1 << 10)) | (6 << 26) | ENABLE);
    mmio.write32(plane + DSP_SURF, surface);
    if started { Ok(pitch) } else { Err(Failure::PipeStuck) }
}

pub fn wait_vblank(mmio: &Mmio, pipe: usize, timeout_ms: u32) -> bool {
    let reg = pipestat(pipe);
    let keep = mmio.read32(reg) & 0xFFFF_0000;
    mmio.write32(reg, keep | VBLANK_STATUS);
    let mut waited = 0u64;
    while waited < timeout_ms.max(1) as u64 * 1000 {
        if mmio.read32(reg) & VBLANK_STATUS != 0 {
            return true;
        }
        delay_us(100);
        waited += 100;
    }
    false
}

pub struct Cursor {
    pub pipe: usize,
    pub g4x: bool,
    pub aperture: *mut u8,
    pub ggtt: u32,
    pub hot: (u32, u32),
    pub armed: bool,
}

pub const CURSOR_SIDE: u32 = 64;
pub const CURSOR_BYTES: u64 = (CURSOR_SIDE * CURSOR_SIDE * 4) as u64;

impl Cursor {
    fn base(&self) -> u32 {
        if self.g4x { CURSOR_A + self.pipe as u32 * CURSOR_STRIDE } else { CURSOR_A }
    }

    pub fn upload(&mut self, mmio: &Mmio, pixels: &[u32], width: u32, height: u32, hot_x: u32, hot_y: u32) -> bool {
        if width == 0 || height == 0 || width > CURSOR_SIDE || height > CURSOR_SIDE || pixels.len() < (width * height) as usize {
            return false;
        }
        let target = unsafe { core::slice::from_raw_parts_mut(self.aperture as *mut u32, (CURSOR_SIDE * CURSOR_SIDE) as usize) };
        for value in target.iter_mut() {
            *value = 0;
        }
        for row in 0..height as usize {
            for column in 0..width as usize {
                target[row * CURSOR_SIDE as usize + column] = pixels[row * width as usize + column];
            }
        }
        self.hot = (hot_x.min(CURSOR_SIDE - 1), hot_y.min(CURSOR_SIDE - 1));
        let mut control = CURSOR_MODE_64_ARGB;
        if !self.g4x {
            control |= (self.pipe as u32) << 28;
        }
        mmio.write32(self.base(), control);
        mmio.write32(self.base() + CURSOR_BASE, self.ggtt);
        self.armed = true;
        true
    }

    pub fn moveto(&self, mmio: &Mmio, x: i32, y: i32) {
        if !self.armed {
            return;
        }
        let encode = |value: i32| -> u32 {
            if value < 0 { (1u32 << 15) | ((-value) as u32 & 0xFFF) } else { value as u32 & 0xFFF }
        };
        let px = x - self.hot.0 as i32;
        let py = y - self.hot.1 as i32;
        mmio.write32(self.base() + CURSOR_POS, encode(px) | (encode(py) << 16));
        mmio.write32(self.base() + CURSOR_BASE, self.ggtt);
    }

    pub fn hide(&mut self, mmio: &Mmio) {
        mmio.write32(self.base(), 0);
        mmio.write32(self.base() + CURSOR_BASE, 0);
        self.armed = false;
    }
}
