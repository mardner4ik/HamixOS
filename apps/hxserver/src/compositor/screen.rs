use alloc::vec::Vec;
use hamix_std::display::gpu;
use hamix_std::sys;
use vellum::{Area, Painter};

pub struct Screen {
    pub width: i32,
    pub height: i32,
    pub fb: *mut u8,
    pub pitch: usize,
    pub bpp: usize,
    pub frame: Vec<u32>,
    pub clip: Area,
    pub generation: u64,
    pub scanout: bool,
    pub buffers: u32,
    pub back: u32,
    pub stride: usize,
    pub frame_damage: Vec<Area>,
    pub previous_damage: Vec<Area>,
    pub force_flip: bool,
}

fn flip_wanted(force: bool) -> bool {
    let caps = gpu::caps();
    caps & gpu::CAP_FLIP != 0 && gpu::buffers() >= 2 && (force || caps & gpu::CAP_FLUSH == 0)
}

impl Screen {
    pub fn take() -> Option<Screen> {
        let mut info = sys::HamixFbInfo::default();
        if sys::fbmap(&mut info) != 0 {
            return None;
        }
        let (width, height) = (info.width as i32, info.height as i32);
        Some(Screen {
            width,
            height,
            fb: info.addr as *mut u8,
            pitch: info.pitch as usize,
            bpp: (info.bpp / 8).max(1) as usize,
            frame: alloc::vec![0; (width * height) as usize],
            clip: Area::new(0, 0, width, height),
            generation: hamix_std::display::generation(),
            scanout: gpu::caps() & gpu::CAP_FLUSH != 0,
            buffers: 1,
            back: 1,
            stride: 0,
            frame_damage: Vec::new(),
            previous_damage: Vec::new(),
            force_flip: false,
        })
    }

    pub fn configure_flip(&mut self, force: bool) {
        self.force_flip = force;
        self.buffers = if flip_wanted(force) { 2 } else { 1 };
        self.stride = (self.pitch * self.height as usize).div_ceil(4096) * 4096;
        self.back = 1;
        self.frame_damage.clear();
        self.previous_damage.clear();
        if self.buffers == 2 {
            gpu::flip(0);
        }
    }

    pub fn flipping(&self) -> bool {
        self.buffers == 2
    }

    pub fn remap(&mut self) -> Option<bool> {
        let mut info = sys::HamixFbInfo::default();
        if sys::fbmap(&mut info) != 0 {
            return None;
        }
        let (width, height) = (info.width as i32, info.height as i32);
        self.fb = info.addr as *mut u8;
        self.pitch = info.pitch as usize;
        self.bpp = (info.bpp / 8).max(1) as usize;
        let resized = width != self.width || height != self.height;
        if resized {
            self.width = width;
            self.height = height;
            self.frame = alloc::vec![0; (width * height) as usize];
        }
        self.generation = hamix_std::display::generation();
        self.clip = self.bounds();
        self.scanout = gpu::caps() & gpu::CAP_FLUSH != 0;
        let force = self.force_flip;
        self.configure_flip(force);
        Some(resized)
    }

    pub fn bounds(&self) -> Area {
        Area::new(0, 0, self.width, self.height)
    }

    pub fn stale(&self) -> bool {
        hamix_std::display::generation() != self.generation
    }

    pub fn painter(&mut self) -> Painter<'_> {
        let (w, h) = (self.width, self.height);
        Painter::new(&mut self.frame, w, h)
    }

    pub fn painter_clipped(&mut self) -> Painter<'_> {
        let clip = self.clip;
        let mut p = self.painter();
        p.set_clip(clip);
        p
    }

    pub fn present(&mut self, area: Area) {
        if self.flipping() {
            let target = unsafe { self.fb.add(self.back as usize * self.stride) };
            present(&self.frame, self.width, area, target, self.pitch, self.bpp);
            self.frame_damage.push(area);
            return;
        }
        present(&self.frame, self.width, area, self.fb, self.pitch, self.bpp);
        if self.scanout {
            gpu::flush(area.x, area.y, area.w, area.h);
        }
    }

    pub fn finish_frame(&mut self) {
        if !self.flipping() || self.frame_damage.is_empty() {
            return;
        }
        let target = unsafe { self.fb.add(self.back as usize * self.stride) };
        let previous = core::mem::take(&mut self.previous_damage);
        for area in previous.iter() {
            let area = area.intersect(&self.bounds());
            if !area.is_empty() {
                present(&self.frame, self.width, area, target, self.pitch, self.bpp);
            }
        }
        gpu::wait_vblank(20);
        if gpu::flip(self.back) == 0 {
            self.back ^= 1;
        }
        self.previous_damage = core::mem::take(&mut self.frame_damage);
    }
}

pub fn present(frame: &[u32], width: i32, area: Area, fb: *mut u8, pitch: usize, bpp: usize) {
    for y in area.y..area.bottom() {
        let src = &frame[(y * width + area.x) as usize..(y * width + area.right()) as usize];
        unsafe {
            let row = fb.add(y as usize * pitch + area.x as usize * bpp);
            match bpp {
                4 => core::ptr::copy_nonoverlapping(src.as_ptr(), row as *mut u32, src.len()),
                3 => {
                    let mut q = row;
                    for v in src {
                        *q = *v as u8;
                        *q.add(1) = (*v >> 8) as u8;
                        *q.add(2) = (*v >> 16) as u8;
                        q = q.add(3);
                    }
                }
                2 => {
                    let mut q = row as *mut u16;
                    for v in src {
                        *q = (((v >> 8) & 0xF800) | ((v >> 5) & 0x07E0) | ((v >> 3) & 0x001F)) as u16;
                        q = q.add(1);
                    }
                }
                _ => {}
            }
        }
    }
}
