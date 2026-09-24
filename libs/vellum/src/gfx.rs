use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }

    pub fn intersect(&self, other: &Area) -> Area {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Area::new(x, y, (r - x).max(0), (b - y).max(0))
    }

    pub fn overlaps(&self, other: &Area) -> bool {
        !self.intersect(other).is_empty()
    }

    pub fn union(&self, other: &Area) -> Area {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let r = self.right().max(other.right());
        let b = self.bottom().max(other.bottom());
        Area::new(x, y, r - x, b - y)
    }

    pub fn expand(&self, by: i32) -> Area {
        Area::new(self.x - by, self.y - by, self.w + by * 2, self.h + by * 2)
    }

    pub fn inset(&self, by: i32) -> Area {
        self.expand(-by)
    }

    pub fn center_y(&self) -> i32 {
        self.y + self.h / 2
    }

    pub fn center_x(&self) -> i32 {
        self.x + self.w / 2
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[inline(always)]
pub fn blend(dst: u32, src: u32, alpha: u32) -> u32 {
    if alpha >= 255 {
        return src & 0xFFFFFF;
    }
    if alpha == 0 {
        return dst;
    }
    let inv = 255 - alpha;
    let r = (((dst >> 16) & 0xFF) * inv + ((src >> 16) & 0xFF) * alpha + 128) / 255;
    let g = (((dst >> 8) & 0xFF) * inv + ((src >> 8) & 0xFF) * alpha + 128) / 255;
    let b = ((dst & 0xFF) * inv + (src & 0xFF) * alpha + 128) / 255;
    (r << 16) | (g << 8) | b
}

pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    blend(a, b, t.min(255))
}

pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Image {
    pub fn from_png(bytes: &[u8]) -> Option<Image> {
        let (w, h, px) = mini_png::decode_argb(bytes).ok()?;
        Some(Image { w: w as i32, h: h as i32, px })
    }

    pub fn solid(w: i32, h: i32, color: u32) -> Image {
        Image { w, h, px: alloc::vec![0xFF00_0000 | color; (w.max(0) * h.max(0)) as usize] }
    }

    #[inline(always)]
    pub fn sample(&self, fx: i32, fy: i32) -> u32 {
        let x0 = (fx >> 8).clamp(0, self.w - 1);
        let y0 = (fy >> 8).clamp(0, self.h - 1);
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let tx = (fx & 0xFF) as u32;
        let ty = (fy & 0xFF) as u32;
        let row0 = (y0 * self.w) as usize;
        let row1 = (y1 * self.w) as usize;
        let a = self.px[row0 + x0 as usize];
        let b = self.px[row0 + x1 as usize];
        let c = self.px[row1 + x0 as usize];
        let d = self.px[row1 + x1 as usize];
        if a == b && c == d && a == c {
            return a;
        }
        let w00 = (256 - tx) * (256 - ty);
        let w10 = tx * (256 - ty);
        let w01 = (256 - tx) * ty;
        let w11 = tx * ty;
        let mix = |shift: u32| -> u32 { ((((a >> shift) & 0xFF) * w00 + ((b >> shift) & 0xFF) * w10 + ((c >> shift) & 0xFF) * w01 + ((d >> shift) & 0xFF) * w11) >> 16) << shift };
        mix(0) | mix(8) | mix(16) | mix(24)
    }

    pub fn scaled(&self, w: i32, h: i32) -> Image {
        let w = w.max(1);
        let h = h.max(1);
        let mut px = Vec::with_capacity((w * h) as usize);
        if w < self.w / 2 || h < self.h / 2 {
            let spans = |dst: i32, src: i32| -> Vec<(i32, i32, i32)> {
                (0..dst)
                    .map(|i| {
                        let s0 = (i as i64 * src as i64 / dst as i64) as i32;
                        let s1 = (((i + 1) as i64 * src as i64 / dst as i64) as i32).clamp(s0 + 1, src);
                        (s0, s1, ((s1 - s0) / 4).max(1))
                    })
                    .collect()
            };
            let xs = spans(w, self.w);
            let ys = spans(h, self.h);
            for &(sy0, sy1, step_y) in ys.iter() {
                for &(sx0, sx1, step_x) in xs.iter() {
                    let (mut a, mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
                    let mut sy = sy0;
                    while sy < sy1 {
                        let row = (sy * self.w) as usize;
                        let mut sx = sx0;
                        while sx < sx1 {
                            let p = self.px[row + sx as usize];
                            a += p >> 24;
                            r += (p >> 16) & 0xFF;
                            g += (p >> 8) & 0xFF;
                            b += p & 0xFF;
                            n += 1;
                            sx += step_x;
                        }
                        sy += step_y;
                    }
                    let n = n.max(1);
                    px.push(((a / n) << 24) | ((r / n) << 16) | ((g / n) << 8) | (b / n));
                }
            }
        } else {
            let step_x = ((self.w as i64) << 8) / w as i64;
            let step_y = ((self.h as i64) << 8) / h as i64;
            for y in 0..h {
                let fy = ((y as i64 * step_y) + step_y / 2 - 128).max(0) as i32;
                for x in 0..w {
                    let fx = ((x as i64 * step_x) + step_x / 2 - 128).max(0) as i32;
                    px.push(self.sample(fx, fy));
                }
            }
        }
        Image { w, h, px }
    }
}

pub struct Painter<'a> {
    pub buf: &'a mut [u32],
    pub w: i32,
    pub h: i32,
    pub clip: Area,
    pub origin: (i32, i32),
}

impl<'a> Painter<'a> {
    pub fn new(buf: &'a mut [u32], w: i32, h: i32) -> Painter<'a> {
        Painter { buf, w, h, clip: Area::new(0, 0, w, h), origin: (0, 0) }
    }

    pub fn bounds(&self) -> Area {
        Area::new(0, 0, self.w, self.h)
    }

    pub fn set_clip(&mut self, area: Area) {
        self.clip = area.intersect(&self.bounds());
    }

    pub fn reset_clip(&mut self) {
        self.clip = self.bounds();
    }

    #[inline(always)]
    fn put(&mut self, x: i32, y: i32, color: u32, alpha: u32) {
        if self.clip.contains(x, y) {
            let i = (y * self.w + x) as usize;
            self.buf[i] = blend(self.buf[i], color, alpha);
        }
    }

    pub fn fill(&mut self, area: Area, color: u32) {
        let a = area.intersect(&self.clip);
        if a.is_empty() {
            return;
        }
        let color = color & 0xFFFFFF;
        for y in a.y..a.bottom() {
            let start = (y * self.w + a.x) as usize;
            self.buf[start..start + a.w as usize].fill(color);
        }
    }

    pub fn blend_fill(&mut self, area: Area, color: u32, alpha: u32) {
        if alpha >= 255 {
            return self.fill(area, color);
        }
        let a = area.intersect(&self.clip);
        if a.is_empty() || alpha == 0 {
            return;
        }
        let inv = 255 - alpha;
        let pr = ((color >> 16) & 0xFF) * alpha + 128;
        let pg = ((color >> 8) & 0xFF) * alpha + 128;
        let pb = (color & 0xFF) * alpha + 128;
        for y in a.y..a.bottom() {
            let start = (y * self.w + a.x) as usize;
            for p in &mut self.buf[start..start + a.w as usize] {
                let d = *p;
                let r = (((d >> 16) & 0xFF) * inv + pr) / 255;
                let g = (((d >> 8) & 0xFF) * inv + pg) / 255;
                let b = ((d & 0xFF) * inv + pb) / 255;
                *p = (r << 16) | (g << 8) | b;
            }
        }
    }

    pub fn gradient_v(&mut self, area: Area, top: u32, bottom: u32, alpha: u32) {
        let a = area.intersect(&self.clip);
        if a.is_empty() {
            return;
        }
        for y in a.y..a.bottom() {
            let t = (((y - area.y) * 255) / area.h.max(1)) as u32;
            let color = blend(top, bottom, t);
            self.blend_fill(Area::new(a.x, y, a.w, 1), color, alpha);
        }
    }

    #[inline(always)]
    fn corner_coverage(dx: f32, dy: f32, r: f32) -> f32 {
        let d = fast_sqrt(dx * dx + dy * dy);
        (r - d + 0.5).clamp(0.0, 1.0)
    }

    pub fn rounded(&mut self, area: Area, radius: i32, color: u32, alpha: u32) {
        if area.is_empty() {
            return;
        }
        let r = radius.min(area.w / 2).min(area.h / 2).max(0);
        if r == 0 {
            return self.blend_fill(area, color, alpha);
        }
        self.blend_fill(Area::new(area.x, area.y + r, area.w, area.h - 2 * r), color, alpha);
        self.blend_fill(Area::new(area.x + r, area.y, area.w - 2 * r, r), color, alpha);
        self.blend_fill(Area::new(area.x + r, area.bottom() - r, area.w - 2 * r, r), color, alpha);
        let rf = r as f32;
        for cy in 0..r {
            for cx in 0..r {
                let dx = rf - cx as f32 - 0.5;
                let dy = rf - cy as f32 - 0.5;
                let cov = Self::corner_coverage(dx, dy, rf);
                if cov <= 0.0 {
                    continue;
                }
                let a = (alpha as f32 * cov) as u32;
                self.put(area.x + cx, area.y + cy, color, a);
                self.put(area.right() - 1 - cx, area.y + cy, color, a);
                self.put(area.x + cx, area.bottom() - 1 - cy, color, a);
                self.put(area.right() - 1 - cx, area.bottom() - 1 - cy, color, a);
            }
        }
    }

    pub fn rounded_border(&mut self, area: Area, radius: i32, color: u32, alpha: u32) {
        if area.w < 2 || area.h < 2 {
            return;
        }
        let r = radius.min(area.w / 2).min(area.h / 2).max(0);
        self.blend_fill(Area::new(area.x + r, area.y, area.w - 2 * r, 1), color, alpha);
        self.blend_fill(Area::new(area.x + r, area.bottom() - 1, area.w - 2 * r, 1), color, alpha);
        self.blend_fill(Area::new(area.x, area.y + r, 1, area.h - 2 * r), color, alpha);
        self.blend_fill(Area::new(area.right() - 1, area.y + r, 1, area.h - 2 * r), color, alpha);
        if r == 0 {
            return;
        }
        let rf = r as f32;
        for cy in 0..r {
            for cx in 0..r {
                let dx = rf - cx as f32 - 0.5;
                let dy = rf - cy as f32 - 0.5;
                let d = fast_sqrt(dx * dx + dy * dy);
                let cov = (1.0 - (d - (rf - 0.5)).abs()).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let a = (alpha as f32 * cov) as u32;
                self.put(area.x + cx, area.y + cy, color, a);
                self.put(area.right() - 1 - cx, area.y + cy, color, a);
                self.put(area.x + cx, area.bottom() - 1 - cy, color, a);
                self.put(area.right() - 1 - cx, area.bottom() - 1 - cy, color, a);
            }
        }
    }

    pub fn shadow(&mut self, area: Area, radius: i32, size: i32, alpha: u32, offset_y: i32) {
        if area.is_empty() || size <= 0 || alpha == 0 {
            return;
        }
        let bounds = shadow_bounds(area, size, offset_y).intersect(&self.clip);
        if bounds.is_empty() {
            return;
        }
        let inset = size / 4;
        let body = Area::new(area.x + inset, area.y + offset_y + inset, (area.w - 2 * inset).max(1), (area.h - 2 * inset).max(1));
        let r = (radius as f32).min(body.w as f32 / 2.0).min(body.h as f32 / 2.0).max(0.0);
        let cx = body.x as f32 + body.w as f32 / 2.0;
        let cy = body.y as f32 + body.h as f32 / 2.0;
        let hx = body.w as f32 / 2.0 - r;
        let hy = body.h as f32 / 2.0 - r;
        let spread = size as f32;
        let rr = radius.min(area.w / 2).min(area.h / 2).max(0);
        let mut lut = [0u32; 257];
        for (i, v) in lut.iter_mut().enumerate() {
            let t = i as f32 / 256.0;
            let eased = t * t * (3.0 - 2.0 * t);
            *v = (alpha as f32 * eased * eased) as u32;
        }
        for y in bounds.y..bounds.bottom() {
            let py = y as f32 + 0.5 - cy;
            let qy = py.abs() - hy;
            let row = (y * self.w) as usize;
            let in_rows = y >= area.y && y < area.bottom();
            let cross_row = y >= area.y + rr && y < area.bottom() - rr;
            let (skip_from, skip_to) = if cross_row { (area.x, area.right()) } else { (area.x + rr, area.right() - rr) };
            let mut x = bounds.x;
            while x < bounds.right() {
                if in_rows && x >= skip_from && x < skip_to {
                    x = skip_to;
                    continue;
                }
                let px = x as f32 + 0.5 - cx;
                let qx = px.abs() - hx;
                let ox = qx.max(0.0);
                let oy = qy.max(0.0);
                let d = if ox > 0.0 && oy > 0.0 { fast_sqrt(ox * ox + oy * oy) } else { ox.max(oy) } + qx.max(qy).min(0.0) - r;
                let t = ((spread * 0.5 - d) / spread).clamp(0.0, 1.0);
                let a = lut[(t * 256.0) as usize];
                if a > 0 {
                    let i = row + x as usize;
                    self.buf[i] = blend(self.buf[i], 0, a);
                }
                x += 1;
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: u32, alpha: u32) {
        let x0 = (cx - r - 1.0) as i32;
        let x1 = (cx + r + 1.0) as i32;
        let y0 = (cy - r - 1.0) as i32;
        let y1 = (cy + r + 1.0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let cov = (r - fast_sqrt(dx * dx + dy * dy) + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.put(x, y, color, (alpha as f32 * cov) as u32);
                }
            }
        }
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, color: u32, alpha: u32) {
        let x0 = (cx - r - 1.0) as i32;
        let x1 = (cx + r + 1.0) as i32;
        let y0 = (cy - r - 1.0) as i32;
        let y1 = (cy + r + 1.0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let d = fast_sqrt(dx * dx + dy * dy);
                let outer = (r - d + 0.5).clamp(0.0, 1.0);
                let inner = (r - width - d + 0.5).clamp(0.0, 1.0);
                let cov = outer - inner;
                if cov > 0.0 {
                    self.put(x, y, color, (alpha as f32 * cov) as u32);
                }
            }
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: u32, alpha: u32) {
        let minx = (x0.min(x1) - width - 1.0) as i32;
        let maxx = (x0.max(x1) + width + 1.0) as i32;
        let miny = (y0.min(y1) - width - 1.0) as i32;
        let maxy = (y0.max(y1) + width + 1.0) as i32;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len2 = (dx * dx + dy * dy).max(0.0001);
        let half = width / 2.0;
        for y in miny..=maxy {
            if y < self.clip.y || y >= self.clip.bottom() {
                continue;
            }
            for x in minx..=maxx {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
                let qx = x0 + t * dx - px;
                let qy = y0 + t * dy - py;
                let d = fast_sqrt(qx * qx + qy * qy);
                let cov = (half - d + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.put(x, y, color, (alpha as f32 * cov) as u32);
                }
            }
        }
    }

    pub fn image(&mut self, img: &Image, x: i32, y: i32, alpha: u32) {
        let target = Area::new(x, y, img.w, img.h).intersect(&self.clip);
        if target.is_empty() {
            return;
        }
        for ty in target.y..target.bottom() {
            let sy = ty - y;
            let row = (ty * self.w) as usize;
            let src = (sy * img.w - x) as isize;
            for tx in target.x..target.right() {
                let p = img.px[(src + tx as isize) as usize];
                let pa = p >> 24;
                if pa == 0 {
                    continue;
                }
                let i = row + tx as usize;
                if pa == 255 && alpha >= 255 {
                    self.buf[i] = p & 0xFFFFFF;
                    continue;
                }
                let a = (pa * alpha) / 255;
                self.buf[i] = blend(self.buf[i], p, a);
            }
        }
    }

    pub fn image_tinted(&mut self, img: &Image, x: i32, y: i32, color: u32, alpha: u32) {
        let target = Area::new(x, y, img.w, img.h).intersect(&self.clip);
        if target.is_empty() {
            return;
        }
        for ty in target.y..target.bottom() {
            let sy = ty - y;
            for tx in target.x..target.right() {
                let p = img.px[(sy * img.w + tx - x) as usize];
                let a = ((p >> 24) * alpha) / 255;
                if a == 0 {
                    continue;
                }
                let i = (ty * self.w + tx) as usize;
                self.buf[i] = blend(self.buf[i], color, a);
            }
        }
    }

    pub fn image_scaled(&mut self, img: &Image, dst: Area, alpha: u32) {
        if img.w == 0 || img.h == 0 || dst.is_empty() {
            return;
        }
        let target = dst.intersect(&self.clip);
        if target.is_empty() {
            return;
        }
        let step_x = ((img.w as i64) << 8) / dst.w as i64;
        let step_y = ((img.h as i64) << 8) / dst.h as i64;
        let smooth = dst.w >= img.w / 2;
        for ty in target.y..target.bottom() {
            let fy = (((ty - dst.y) as i64 * step_y) + step_y / 2 - 128).max(0) as i32;
            for tx in target.x..target.right() {
                let fx = (((tx - dst.x) as i64 * step_x) + step_x / 2 - 128).max(0) as i32;
                let p = if smooth { img.sample(fx, fy) } else { img.px[(((fy >> 8).min(img.h - 1)) * img.w + (fx >> 8).min(img.w - 1)) as usize] };
                let a = ((p >> 24) * alpha) / 255;
                if a == 0 {
                    continue;
                }
                let i = (ty * self.w + tx) as usize;
                self.buf[i] = blend(self.buf[i], p, a);
            }
        }
    }

    pub fn blit(&mut self, src: &[u32], sw: i32, sh: i32, x: i32, y: i32) {
        let target = Area::new(x, y, sw, sh).intersect(&self.clip);
        if target.is_empty() || src.len() < (sw * sh) as usize {
            return;
        }
        for ty in target.y..target.bottom() {
            let s = ((ty - y) * sw + target.x - x) as usize;
            let d = (ty * self.w + target.x) as usize;
            self.buf[d..d + target.w as usize].copy_from_slice(&src[s..s + target.w as usize]);
        }
    }

    pub fn copy_area(&mut self, src: &[u32], area: Area) {
        let a = area.intersect(&self.clip);
        if a.is_empty() || src.len() < self.buf.len() {
            return;
        }
        for y in a.y..a.bottom() {
            let start = (y * self.w + a.x) as usize;
            self.buf[start..start + a.w as usize].copy_from_slice(&src[start..start + a.w as usize]);
        }
    }

    pub fn text(&mut self, font: &crate::hfont::Font, x: i32, y: i32, text: &str, color: u32) -> i32 {
        font.draw(self, x, y, text, color, 255)
    }

    pub fn text_alpha(&mut self, font: &crate::hfont::Font, x: i32, y: i32, text: &str, color: u32, alpha: u32) -> i32 {
        font.draw(self, x, y, text, color, alpha)
    }

    pub fn coverage(&mut self, x: i32, y: i32, color: u32, alpha: u32) {
        self.put(x, y, color, alpha);
    }
}

#[inline(always)]
pub fn fast_sqrt(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::{_mm_cvtss_f32, _mm_set_ss, _mm_sqrt_ss};
        _mm_cvtss_f32(_mm_sqrt_ss(_mm_set_ss(v)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let mut x = if v > 1.0 { v / 2.0 } else { 1.0 };
        for _ in 0..8 {
            x = 0.5 * (x + v / x);
        }
        x
    }
}

#[inline(always)]
pub fn libm_sqrt(v: f32) -> f32 {
    fast_sqrt(v)
}

pub fn shadow_bounds(area: Area, size: i32, offset_y: i32) -> Area {
    let inset = size / 4;
    let reach = size / 2 + 1 - inset;
    Area::new(area.x - reach, area.y + offset_y - reach, area.w + 2 * reach, area.h + 2 * reach)
}

pub fn ellipsize(font: &crate::hfont::Font, text: &str, max: i32) -> String {
    if font.measure(text) <= max {
        return String::from(text);
    }
    let dots = font.measure("…");
    let mut out = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let w = font.advance(ch);
        if width + w + dots > max {
            break;
        }
        width += w;
        out.push(ch);
    }
    out.push('…');
    out
}
