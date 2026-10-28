use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;

use crate::style::Color;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixmap {
    width: u32,
    height: u32,
    pixels: Rc<Vec<u32>>,
}

fn blend(dst: u32, src: u32) -> u32 {
    let a = src >> 24;
    if a == 255 {
        return src;
    }
    if a == 0 {
        return dst;
    }
    let inv = 255 - a;
    let channel = |shift: u32| (((src >> shift) & 0xff) * a + ((dst >> shift) & 0xff) * inv) / 255;
    let out_a = a + ((dst >> 24) * inv) / 255;
    out_a << 24 | channel(16) << 16 | channel(8) << 8 | channel(0)
}

impl Pixmap {
    pub fn new(width: u32, height: u32) -> Pixmap {
        Pixmap { width, height, pixels: Rc::new(vec![0; width as usize * height as usize]) }
    }

    pub fn release(&mut self) {
        self.width = 0;
        self.height = 0;
        self.pixels = Rc::new(Vec::new());
    }

    pub fn filled(width: u32, height: u32, color: Color) -> Pixmap {
        Pixmap { width, height, pixels: Rc::new(vec![color.0; width as usize * height as usize]) }
    }

    pub fn from_argb(width: u32, height: u32, pixels: Vec<u32>) -> Option<Pixmap> {
        (pixels.len() == width as usize * height as usize).then_some(Pixmap { width, height, pixels: Rc::new(pixels) })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        Rc::make_mut(&mut self.pixels).as_mut_slice()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.pixels = Rc::new(vec![0; width as usize * height as usize]);
        }
    }

    pub fn clear(&mut self, color: Color) {
        Rc::make_mut(&mut self.pixels).fill(color.0);
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(Color(self.pixels[y as usize * self.width as usize + x as usize]))
    }

    pub fn put(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = y as usize * self.width as usize + x as usize;
        let pixels = Rc::make_mut(&mut self.pixels);
        pixels[i] = blend(pixels[i], color.0);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = self.width as usize;
        let opaque = color.alpha() == 255;
        let pixels = Rc::make_mut(&mut self.pixels);
        for row in y0..y1 {
            let line = &mut pixels[row as usize * stride + x0 as usize..row as usize * stride + x1 as usize];
            if opaque {
                line.fill(color.0);
            } else {
                for px in line.iter_mut() {
                    *px = blend(*px, color.0);
                }
            }
        }
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y + 1, 1, h - 2, color);
        self.fill_rect(x + w - 1, y + 1, 1, h - 2, color);
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.put(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        for dy in -r..=r {
            let span = isqrt(r * r - dy * dy);
            self.fill_rect(cx - span, cy + dy, span * 2 + 1, 1, color);
        }
    }

    pub fn blit(&mut self, src: &Pixmap, x: i32, y: i32) {
        for sy in 0..src.height as i32 {
            for sx in 0..src.width as i32 {
                self.put(x + sx, y + sy, Color(src.pixels[sy as usize * src.width as usize + sx as usize]));
            }
        }
    }

    pub fn scaled(&self, width: u32, height: u32) -> Pixmap {
        let mut out = Pixmap::new(width, height);
        if self.width == 0 || self.height == 0 {
            return out;
        }
        let target = Rc::make_mut(&mut out.pixels);
        for y in 0..height {
            let sy = (y as u64 * self.height as u64 / height.max(1) as u64) as usize;
            for x in 0..width {
                let sx = (x as u64 * self.width as u64 / width.max(1) as u64) as usize;
                target[y as usize * width as usize + x as usize] = self.pixels[sy * self.width as usize + sx];
            }
        }
        out
    }
}

fn isqrt(v: i32) -> i32 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}
