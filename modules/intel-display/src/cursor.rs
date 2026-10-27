use crate::regs::{self, Gen, Mmio};

pub const SIDE: u32 = 64;
pub const BYTES: usize = (SIDE * SIDE * 4) as usize;

pub struct Plane {
    pipe: usize,
    aperture: *mut u8,
    ggtt: u32,
    hot: (u32, u32),
    armed: bool,
}

impl Plane {
    pub fn attach(mmio: &Mmio, chip: Gen, pipe: usize, fb_addr: u64, fb_bytes: u64, stolen_bytes: u64) -> Option<Plane> {
        let surface = mmio.read(regs::plane(pipe) + regs::PLANE_SURF) & !0xFFF;
        if surface == 0 {
            return None;
        }
        let offset = (fb_bytes + 0xFFF) & !0xFFF;
        if offset + BYTES as u64 > stolen_bytes {
            return None;
        }
        let ggtt = surface.checked_add(offset as u32)?;
        let _ = chip;
        Some(Plane { pipe, aperture: (fb_addr + offset) as *mut u8, ggtt, hot: (0, 0), armed: false })
    }

    pub fn upload(&mut self, mmio: &Mmio, chip: Gen, pixels: &[u32], width: u32, height: u32, hot_x: u32, hot_y: u32) -> bool {
        if width == 0 || height == 0 || width > SIDE || height > SIDE {
            return false;
        }
        if pixels.len() < (width * height) as usize {
            return false;
        }
        let target = unsafe { core::slice::from_raw_parts_mut(self.aperture as *mut u32, (SIDE * SIDE) as usize) };
        for value in target.iter_mut() {
            *value = 0;
        }
        for row in 0..height as usize {
            for column in 0..width as usize {
                target[row * SIDE as usize + column] = pixels[row * width as usize + column];
            }
        }
        self.hot = (hot_x.min(SIDE - 1), hot_y.min(SIDE - 1));
        let mut control = regs::CURSOR_MODE_64_ARGB;
        if matches!(chip, Gen::Gen6 | Gen::Gen7) && self.pipe == 1 {
            control |= regs::CURSOR_PIPE_SELECT;
        }
        let base = regs::cursor(self.pipe);
        mmio.write(base, control);
        mmio.write(base + (regs::CUR_BASE - regs::CUR_CTL), self.ggtt);
        self.armed = true;
        true
    }

    pub fn moveto(&self, mmio: &Mmio, x: i32, y: i32) {
        if !self.armed {
            return;
        }
        let px = x - self.hot.0 as i32;
        let py = y - self.hot.1 as i32;
        let encode = |value: i32| -> u32 {
            if value < 0 {
                ((1u32 << 15) | ((-value) as u32 & 0xFFF)) as u32
            } else {
                value as u32 & 0xFFF
            }
        };
        let base = regs::cursor(self.pipe);
        mmio.write(base + (regs::CUR_POS - regs::CUR_CTL), encode(px) | (encode(py) << 16));
        mmio.write(base + (regs::CUR_BASE - regs::CUR_CTL), self.ggtt);
    }

    pub fn hide(&mut self, mmio: &Mmio) {
        let base = regs::cursor(self.pipe);
        mmio.write(base, 0);
        mmio.write(base + (regs::CUR_BASE - regs::CUR_CTL), 0);
        self.armed = false;
    }
}
