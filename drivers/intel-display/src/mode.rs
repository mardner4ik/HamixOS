use hamix_kpi as kpi;

use crate::regs::{self, Gen, Mmio};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Timing {
    pub hactive: u32,
    pub htotal: u32,
    pub vactive: u32,
    pub vtotal: u32,
    pub vblank_active: u32,
    pub vblank_total: u32,
    pub vsync_start: u32,
    pub vsync_end: u32,
}

#[derive(Clone, Copy)]
pub struct Output {
    pub pipe: usize,
    pub transcoder: usize,
    pub native: Timing,
    pub source: (u32, u32),
    pub refresh_mhz: u32,
    pub pixel_khz: u64,
    pub measured: bool,
}

fn read_timing(mmio: &Mmio, base: u32) -> Timing {
    let (hactive, htotal) = regs::pair(mmio.read(base + regs::HTOTAL));
    let (vactive, vtotal) = regs::pair(mmio.read(base + regs::VTOTAL));
    let (vblank_active, vblank_total) = regs::pair(mmio.read(base + regs::VBLANK));
    let (vsync_start, vsync_end) = regs::pair(mmio.read(base + regs::VSYNC));
    Timing { hactive, htotal, vactive, vtotal, vblank_active, vblank_total, vsync_start, vsync_end }
}

fn plausible(t: &Timing) -> bool {
    t.hactive >= 320 && t.hactive <= 8192 && t.vactive >= 200 && t.vactive <= 8192 && t.htotal > t.hactive && t.vtotal > t.vactive && t.vtotal <= 8191
}

pub fn measure_refresh_mhz(mmio: &Mmio, pipe: usize) -> u32 {
    let reg = regs::frames(pipe);
    let first = mmio.read(reg);
    let start = unsafe { kpi::hamix_uptime_ms() };
    for _ in 0..2500 {
        unsafe { kpi::hamix_udelay(200) };
    }
    let second = mmio.read(reg);
    let elapsed = match unsafe { kpi::hamix_uptime_ms() }.saturating_sub(start) {
        0 => 500,
        value => value,
    };
    let frames = second.wrapping_sub(first) as u64;
    if frames == 0 || frames > 1000 {
        return 0;
    }
    (frames * 1_000_000 / elapsed) as u32
}

fn edp_pipe(mmio: &Mmio) -> usize {
    match (mmio.read(regs::TRANS_DDI_FUNC_CTL_EDP) >> 12) & 0x7 {
        5 => 1,
        6 => 2,
        _ => 0,
    }
}

fn output_for(mmio: &Mmio, chip: Gen, index: usize, pipe: usize, native: Timing) -> Output {
    let (sh, sw) = regs::pair(mmio.read(regs::pipe_src(pipe)));
    let source = if sw >= 320 && sh >= 200 && sw <= native.hactive && sh <= native.vactive { (sw, sh) } else { (native.hactive, native.vactive) };
    let measured = measure_refresh_mhz(mmio, pipe);
    let refresh_mhz = if measured > 0 { measured } else { 60_000 };
    let pixel_khz = native.htotal as u64 * native.vtotal as u64 * refresh_mhz as u64 / 1_000_000;
    let _ = chip;
    Output { pipe, transcoder: index, native, source, refresh_mhz, pixel_khz, measured: measured > 0 }
}

pub fn detect(mmio: &Mmio, chip: Gen) -> Option<Output> {
    let count = if chip.has_edp_transcoder() { 4 } else { 3 };
    let mut order = [3usize, 0, 1, 2];
    if count == 3 {
        order = [0, 1, 2, 3];
    }
    for index in order {
        if index >= count {
            continue;
        }
        if mmio.read(regs::conf(chip, index)) & regs::ENABLE == 0 {
            continue;
        }
        let pipe = if index >= 3 { edp_pipe(mmio) } else { index };
        if mmio.read(regs::plane(pipe)) & regs::ENABLE == 0 && index < 3 {
            continue;
        }
        let native = read_timing(mmio, regs::transcoder(chip, index));
        if !plausible(&native) {
            continue;
        }
        return Some(output_for(mmio, chip, index, pipe, native));
    }
    for pipe in 0..3usize {
        if mmio.read(regs::plane(pipe)) & regs::ENABLE == 0 {
            continue;
        }
        let mut candidates = [pipe, 3, 0];
        if !chip.has_edp_transcoder() {
            candidates[1] = pipe;
        }
        for index in candidates {
            let native = read_timing(mmio, regs::transcoder(chip, index));
            if plausible(&native) {
                return Some(output_for(mmio, chip, index, pipe, native));
            }
        }
    }
    None
}

pub fn vtotal_for_refresh(output: &Output, refresh_hz: u32) -> Option<u32> {
    if output.pixel_khz == 0 || output.native.htotal == 0 || refresh_hz == 0 {
        return None;
    }
    let wanted = output.pixel_khz * 1_000_000 / (output.native.htotal as u64 * refresh_hz as u64 * 1000);
    if wanted < output.native.vactive as u64 + 2 || wanted > 8191 {
        return None;
    }
    Some(wanted as u32)
}

pub fn set_vtotal(mmio: &Mmio, chip: Gen, output: &Output, vtotal: u32) -> bool {
    if vtotal <= output.native.vactive || vtotal > 8191 {
        return false;
    }
    let base = regs::transcoder(chip, output.transcoder);
    let extra = vtotal as i64 - output.native.vtotal as i64;
    let blank_total = (output.native.vblank_total as i64 + extra).clamp(1, 8191) as u32;
    if output.native.vsync_end > blank_total || output.native.vblank_active >= blank_total {
        return false;
    }
    mmio.write(base + regs::VTOTAL, regs::encode(output.native.vactive, vtotal));
    mmio.write(base + regs::VBLANK, regs::encode(output.native.vblank_active, blank_total));
    let check = regs::pair(mmio.read(base + regs::VTOTAL));
    check.1 == vtotal
}

pub fn restore(mmio: &Mmio, chip: Gen, output: &Output) {
    let base = regs::transcoder(chip, output.transcoder);
    mmio.write(base + regs::VTOTAL, regs::encode(output.native.vactive, output.native.vtotal));
    mmio.write(base + regs::VBLANK, regs::encode(output.native.vblank_active, output.native.vblank_total));
}

pub fn pitch_for(width: u32) -> u32 {
    (width * 4 + 63) & !63
}

fn second_scaler_off(mmio: &Mmio, pipe: usize) {
    let step = pipe.min(2) as u32 * regs::PS_STRIDE;
    if mmio.read(regs::PS_CTRL_A + regs::PS_SECOND + step) & regs::PS_ENABLE != 0 {
        mmio.write(regs::PS_CTRL_A + regs::PS_SECOND + step, 0);
        mmio.write(regs::PS_WIN_SZ_A + regs::PS_SECOND + step, 0);
    }
}

fn scaler_off(mmio: &Mmio, chip: Gen, pipe: usize) {
    let step = pipe.min(2) as u32;
    if chip.pipe_scaler() {
        mmio.write(regs::PS_CTRL_A + step * regs::PS_STRIDE, 0);
        mmio.write(regs::PS_WIN_SZ_A + step * regs::PS_STRIDE, 0);
        if pipe < 2 {
            second_scaler_off(mmio, pipe);
        }
    } else {
        mmio.write(regs::PF_CTL_A + step * regs::PF_STRIDE, 0);
        mmio.write(regs::PF_WIN_SZ_A + step * regs::PF_STRIDE, 0);
    }
}

fn scaler_on(mmio: &Mmio, chip: Gen, pipe: usize, width: u32, height: u32) {
    let step = pipe.min(2) as u32;
    if chip.pipe_scaler() {
        if pipe < 2 {
            second_scaler_off(mmio, pipe);
        }
        mmio.write(regs::PS_CTRL_A + step * regs::PS_STRIDE, regs::PS_ENABLE | regs::PS_FILTER_MED);
        mmio.write(regs::PS_WIN_POS_A + step * regs::PS_STRIDE, 0);
        mmio.write(regs::PS_WIN_SZ_A + step * regs::PS_STRIDE, (width << 16) | height);
    } else {
        let mut control = regs::PF_ENABLE | regs::PF_FILTER_MED;
        if chip.scaler_pipe_select() {
            control |= step << regs::PF_PIPE_SEL_IVB;
        }
        mmio.write(regs::PF_CTL_A + step * regs::PF_STRIDE, control);
        mmio.write(regs::PF_WIN_POS_A + step * regs::PF_STRIDE, 0);
        mmio.write(regs::PF_WIN_SZ_A + step * regs::PF_STRIDE, (width << 16) | height);
    }
}

pub fn scalable(output: &Output) -> bool {
    output.native.hactive >= 640 && output.native.vactive >= 480
}

pub fn set_source(mmio: &Mmio, chip: Gen, output: &Output, width: u32, height: u32, pitch: u32) -> bool {
    if width < 320 || height < 200 || width > output.native.hactive || height > output.native.vactive {
        return false;
    }
    let plane = regs::plane(output.pipe);
    let src = regs::pipe_src(output.pipe);
    mmio.write(src, regs::encode(height, width));
    if chip.pipe_scaler() {
        mmio.write(plane + regs::PLANE_STRIDE, pitch / 64);
        mmio.write(plane + regs::PLANE_SIZE, ((height - 1) << 16) | (width - 1));
        mmio.write(plane + regs::PLANE_POS, 0);
        mmio.write(plane + regs::PLANE_OFFSET, 0);
    } else {
        mmio.write(plane + regs::PLANE_STRIDE, pitch);
        mmio.write(plane + regs::PLANE_LINOFF, 0);
        mmio.write(plane + regs::PLANE_OFFSET, 0);
    }
    if width == output.native.hactive && height == output.native.vactive {
        scaler_off(mmio, chip, output.pipe);
    } else {
        scaler_on(mmio, chip, output.pipe, output.native.hactive, output.native.vactive);
    }
    let surface = mmio.read(plane + regs::PLANE_SURF);
    mmio.write(plane + regs::PLANE_SURF, surface);
    unsafe { kpi::hamix_mdelay(20) };
    let (back_h, back_w) = regs::pair(mmio.read(src));
    back_w == width && back_h == height
}
