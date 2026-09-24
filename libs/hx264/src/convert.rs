use alloc::vec::Vec;

use crate::Planes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMatrix {
    Bt601,
    Bt709,
}

impl ColorMatrix {
    pub fn for_size(width: u32, height: u32) -> ColorMatrix {
        if width >= 1280 || height >= 720 { ColorMatrix::Bt709 } else { ColorMatrix::Bt601 }
    }

    fn coefficients(self) -> [i16; 4] {
        match self {
            ColorMatrix::Bt601 => [102, 25, 52, 129],
            ColorMatrix::Bt709 => [115, 14, 34, 135],
        }
    }
}

pub struct Scaler {
    key: (usize, usize, usize, usize),
    x_index: Vec<u32>,
    x_frac: Vec<u16>,
    xc_index: Vec<u32>,
    luma: Vec<i16>,
    cb: Vec<i16>,
    cr: Vec<i16>,
    rows: [Vec<u16>; 2],
    row_ids: [usize; 2],
}

impl Default for Scaler {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn to_u8(v: i16) -> u8 {
    (v >> 6).clamp(0, 255) as u8
}

#[cfg(target_arch = "x86_64")]
fn pack_row(luma: &[i16], cb: &[i16], cr: &[i16], out: &mut [u32], coefficients: [i16; 4]) {
    use core::arch::x86_64::*;
    let [kr, kgu, kgv, kb] = coefficients;
    let n = out.len();
    let mut i = 0;
    unsafe {
        let vkr = _mm_set1_epi16(kr);
        let vkgu = _mm_set1_epi16(kgu);
        let vkgv = _mm_set1_epi16(kgv);
        let vkb = _mm_set1_epi16(kb);
        let zero = _mm_setzero_si128();
        while i + 8 <= n {
            let y = _mm_loadu_si128(luma.as_ptr().add(i) as *const __m128i);
            let u = _mm_loadu_si128(cb.as_ptr().add(i) as *const __m128i);
            let v = _mm_loadu_si128(cr.as_ptr().add(i) as *const __m128i);
            let r = _mm_srai_epi16(_mm_adds_epi16(y, _mm_mullo_epi16(vkr, v)), 6);
            let g = _mm_srai_epi16(_mm_subs_epi16(y, _mm_add_epi16(_mm_mullo_epi16(vkgu, u), _mm_mullo_epi16(vkgv, v))), 6);
            let b = _mm_srai_epi16(_mm_adds_epi16(y, _mm_mullo_epi16(vkb, u)), 6);
            let rb = _mm_packus_epi16(r, r);
            let gb = _mm_packus_epi16(g, g);
            let bb = _mm_packus_epi16(b, b);
            let bg = _mm_unpacklo_epi8(bb, gb);
            let ra = _mm_unpacklo_epi8(rb, zero);
            _mm_storeu_si128(out.as_mut_ptr().add(i) as *mut __m128i, _mm_unpacklo_epi16(bg, ra));
            _mm_storeu_si128(out.as_mut_ptr().add(i + 4) as *mut __m128i, _mm_unpackhi_epi16(bg, ra));
            i += 8;
        }
    }
    while i < n {
        let (y, u, v) = (luma[i], cb[i], cr[i]);
        let r = to_u8(y.saturating_add(kr * v)) as u32;
        let g = to_u8(y.saturating_sub(kgu * u + kgv * v)) as u32;
        let b = to_u8(y.saturating_add(kb * u)) as u32;
        out[i] = (r << 16) | (g << 8) | b;
        i += 1;
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn pack_row(luma: &[i16], cb: &[i16], cr: &[i16], out: &mut [u32], coefficients: [i16; 4]) {
    let [kr, kgu, kgv, kb] = coefficients;
    for i in 0..out.len() {
        let (y, u, v) = (luma[i], cb[i], cr[i]);
        let r = to_u8(y.saturating_add(kr * v)) as u32;
        let g = to_u8(y.saturating_sub(kgu * u + kgv * v)) as u32;
        let b = to_u8(y.saturating_add(kb * u)) as u32;
        out[i] = (r << 16) | (g << 8) | b;
    }
}

fn scale_row(line: &[u8], index: &[u32], frac: &[u16], out: &mut [u16]) {
    for ((slot, &xi), &fx) in out.iter_mut().zip(index.iter()).zip(frac.iter()) {
        let i = xi as usize;
        *slot = if fx == 0 { line[i] as u16 * 256 } else { line[i] as u16 * (256 - fx) + line[i + 1] as u16 * fx };
    }
}

impl Scaler {
    pub fn new() -> Scaler {
        Scaler {
            key: (0, 0, 0, 0),
            x_index: Vec::new(),
            x_frac: Vec::new(),
            xc_index: Vec::new(),
            luma: Vec::new(),
            cb: Vec::new(),
            cr: Vec::new(),
            rows: [Vec::new(), Vec::new()],
            row_ids: [usize::MAX; 2],
        }
    }

    fn prepare(&mut self, src_w: usize, src_h: usize, dst_w: usize, dst_h: usize) {
        let key = (src_w, src_h, dst_w, dst_h);
        if self.key == key {
            return;
        }
        self.key = key;
        self.x_index.clear();
        self.x_frac.clear();
        self.xc_index.clear();
        let step = ((src_w as u64) << 16) / dst_w.max(1) as u64;
        for x in 0..dst_w {
            let pos = ((x as u64 * step) + (step >> 1)).saturating_sub(1 << 15);
            let index = ((pos >> 16) as usize).min(src_w.saturating_sub(1));
            let frac = if index + 1 < src_w { ((pos & 0xFFFF) >> 8) as u16 } else { 0 };
            self.x_index.push(index as u32);
            self.x_frac.push(frac);
            let center = ((x as u64 * step + (step >> 1)) >> 17) as usize;
            self.xc_index.push(center.min((src_w / 2).saturating_sub(1)) as u32);
        }
        let padded = dst_w + 8;
        for buffer in [&mut self.luma, &mut self.cb, &mut self.cr] {
            buffer.clear();
            buffer.resize(padded, 0);
        }
        for buffer in self.rows.iter_mut() {
            buffer.clear();
            buffer.resize(padded, 0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, planes: &Planes, matrix: ColorMatrix, dst: &mut [u32], dst_stride: usize, dst_x: usize, dst_y: usize, dst_w: usize, dst_h: usize) {
        if dst_w == 0 || dst_h == 0 || planes.width < 2 || planes.height < 2 {
            return;
        }
        if dst.len() < (dst_y + dst_h - 1) * dst_stride + dst_x + dst_w {
            return;
        }
        let (src_w, src_h) = (planes.width, planes.height);
        self.prepare(src_w, src_h, dst_w, dst_h);
        self.row_ids = [usize::MAX; 2];
        let coefficients = matrix.coefficients();
        let step_y = ((src_h as u64) << 16) / dst_h as u64;
        let chroma_w = src_w / 2;
        for oy in 0..dst_h {
            let pos = ((oy as u64 * step_y) + (step_y >> 1)).saturating_sub(1 << 15);
            let y0 = ((pos >> 16) as usize).min(src_h - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fy = ((pos & 0xFFFF) >> 8) as u32;
            let cy = (((oy as u64 * step_y + (step_y >> 1)) >> 17) as usize).min((src_h / 2).saturating_sub(1));
            let urow = &planes.u[cy * planes.stride_c..cy * planes.stride_c + chroma_w];
            let vrow = &planes.v[cy * planes.stride_c..cy * planes.stride_c + chroma_w];
            for (slot, source) in [y0, y1].into_iter().enumerate() {
                if self.row_ids[slot] == source {
                    continue;
                }
                let other = 1 - slot;
                if self.row_ids[other] == source {
                    self.rows.swap(0, 1);
                    self.row_ids.swap(0, 1);
                    continue;
                }
                let line = &planes.y[source * planes.stride_y..source * planes.stride_y + src_w];
                scale_row(line, &self.x_index, &self.x_frac, &mut self.rows[slot][..dst_w]);
                self.row_ids[slot] = source;
            }
            let (top, bottom) = (&self.rows[0], &self.rows[1]);
            let inv_fy = 256 - fy;
            if fy == 0 {
                for (out, &t) in self.luma[..dst_w].iter_mut().zip(top.iter()) {
                    *out = (((t as u32 + 128) >> 8) as i16 - 16) * 74;
                }
            } else {
                for ((out, &t), &b) in self.luma[..dst_w].iter_mut().zip(top.iter()).zip(bottom.iter()) {
                    let value = (t as u32 * inv_fy + b as u32 * fy + 32_768) >> 16;
                    *out = (value as i16 - 16) * 74;
                }
            }
            for ((u, v), &ci) in self.cb[..dst_w].iter_mut().zip(self.cr[..dst_w].iter_mut()).zip(self.xc_index.iter()) {
                let c = ci as usize;
                *u = urow[c] as i16 - 128;
                *v = vrow[c] as i16 - 128;
            }
            let out_start = (dst_y + oy) * dst_stride + dst_x;
            pack_row(&self.luma, &self.cb, &self.cr, &mut dst[out_start..out_start + dst_w], coefficients);
        }
    }
}
