#![allow(clippy::too_many_arguments)]

use core::arch::x86_64::*;

#[inline(always)]
unsafe fn load8(p: *const u8) -> __m128i {
    unsafe { _mm_loadl_epi64(p as *const __m128i) }
}

#[inline(always)]
unsafe fn widen8(p: *const u8) -> __m128i {
    unsafe { _mm_unpacklo_epi8(load8(p), _mm_setzero_si128()) }
}

#[inline(always)]
unsafe fn fir6_u8(a: __m128i, b: __m128i, c: __m128i, d: __m128i, e: __m128i, f: __m128i) -> __m128i {
    unsafe {
        let cd = _mm_add_epi16(c, d);
        let be = _mm_add_epi16(b, e);
        let af = _mm_add_epi16(a, f);
        let cd20 = _mm_add_epi16(_mm_slli_epi16(cd, 4), _mm_slli_epi16(cd, 2));
        let be5 = _mm_add_epi16(_mm_slli_epi16(be, 2), be);
        _mm_add_epi16(_mm_sub_epi16(cd20, be5), af)
    }
}

#[inline(always)]
fn fir6_scalar(s: &[u8], i: usize) -> i32 {
    s[i] as i32 - 5 * s[i + 1] as i32 + 20 * s[i + 2] as i32 + 20 * s[i + 3] as i32 - 5 * s[i + 4] as i32 + s[i + 5] as i32
}

pub fn half_pel_h(src: &[u8], out: &mut [u8], w: usize) {
    assert!(src.len() >= w + 5 && out.len() >= w);
    let mut i = 0;
    unsafe {
        let round = _mm_set1_epi16(16);
        while i + 8 <= w {
            let p = src.as_ptr().add(i);
            let v = fir6_u8(widen8(p), widen8(p.add(1)), widen8(p.add(2)), widen8(p.add(3)), widen8(p.add(4)), widen8(p.add(5)));
            let r = _mm_srai_epi16(_mm_add_epi16(v, round), 5);
            let packed = _mm_packus_epi16(r, r);
            _mm_storel_epi64(out.as_mut_ptr().add(i) as *mut __m128i, packed);
            i += 8;
        }
    }
    while i < w {
        out[i] = ((fir6_scalar(src, i) + 16) >> 5).clamp(0, 255) as u8;
        i += 1;
    }
}

pub fn half_pel_v(rows: [&[u8]; 6], out: &mut [u8], w: usize) {
    assert!(rows.iter().all(|r| r.len() >= w) && out.len() >= w);
    let mut i = 0;
    unsafe {
        let round = _mm_set1_epi16(16);
        while i + 8 <= w {
            let v = fir6_u8(
                widen8(rows[0].as_ptr().add(i)),
                widen8(rows[1].as_ptr().add(i)),
                widen8(rows[2].as_ptr().add(i)),
                widen8(rows[3].as_ptr().add(i)),
                widen8(rows[4].as_ptr().add(i)),
                widen8(rows[5].as_ptr().add(i)),
            );
            let r = _mm_srai_epi16(_mm_add_epi16(v, round), 5);
            let packed = _mm_packus_epi16(r, r);
            _mm_storel_epi64(out.as_mut_ptr().add(i) as *mut __m128i, packed);
            i += 8;
        }
    }
    while i < w {
        let val = rows[0][i] as i32 - 5 * rows[1][i] as i32 + 20 * rows[2][i] as i32 + 20 * rows[3][i] as i32 - 5 * rows[4][i] as i32 + rows[5][i] as i32;
        out[i] = ((val + 16) >> 5).clamp(0, 255) as u8;
        i += 1;
    }
}

#[inline(always)]
unsafe fn sign_extend(x: __m128i) -> (__m128i, __m128i) {
    unsafe {
        let sign = _mm_srai_epi16(x, 15);
        (_mm_unpacklo_epi16(x, sign), _mm_unpackhi_epi16(x, sign))
    }
}

#[inline(always)]
unsafe fn fir6_i32(af: __m128i, be: __m128i, cd: __m128i) -> __m128i {
    unsafe {
        let cd20 = _mm_add_epi32(_mm_slli_epi32(cd, 4), _mm_slli_epi32(cd, 2));
        let be5 = _mm_add_epi32(_mm_slli_epi32(be, 2), be);
        _mm_add_epi32(_mm_sub_epi32(cd20, be5), af)
    }
}

pub fn half_pel_hv(rows: [&[u8]; 6], out: &mut [u8], w: usize) {
    assert!(rows.iter().all(|r| r.len() >= w + 5) && out.len() >= w);
    let mut i = 0;
    unsafe {
        let round = _mm_set1_epi32(512);
        while i + 8 <= w {
            let mut h = [_mm_setzero_si128(); 6];
            for (k, row) in rows.iter().enumerate() {
                let p = row.as_ptr().add(i);
                h[k] = fir6_u8(widen8(p), widen8(p.add(1)), widen8(p.add(2)), widen8(p.add(3)), widen8(p.add(4)), widen8(p.add(5)));
            }
            let af = _mm_add_epi16(h[0], h[5]);
            let be = _mm_add_epi16(h[1], h[4]);
            let cd = _mm_add_epi16(h[2], h[3]);
            let (af_lo, af_hi) = sign_extend(af);
            let (be_lo, be_hi) = sign_extend(be);
            let (cd_lo, cd_hi) = sign_extend(cd);
            let lo = _mm_srai_epi32(_mm_add_epi32(fir6_i32(af_lo, be_lo, cd_lo), round), 10);
            let hi = _mm_srai_epi32(_mm_add_epi32(fir6_i32(af_hi, be_hi, cd_hi), round), 10);
            let words = _mm_packs_epi32(lo, hi);
            let packed = _mm_packus_epi16(words, words);
            _mm_storel_epi64(out.as_mut_ptr().add(i) as *mut __m128i, packed);
            i += 8;
        }
    }
    while i < w {
        let h: [i32; 6] = core::array::from_fn(|k| fir6_scalar(rows[k], i));
        let val = h[0] - 5 * h[1] + 20 * h[2] + 20 * h[3] - 5 * h[4] + h[5];
        out[i] = ((val + 512) >> 10).clamp(0, 255) as u8;
        i += 1;
    }
}

pub fn average(a: &[u8], b: &[u8], out: &mut [u8], w: usize) {
    assert!(a.len() >= w && b.len() >= w && out.len() >= w);
    let mut i = 0;
    unsafe {
        while i + 16 <= w {
            let x = _mm_loadu_si128(a.as_ptr().add(i) as *const __m128i);
            let y = _mm_loadu_si128(b.as_ptr().add(i) as *const __m128i);
            _mm_storeu_si128(out.as_mut_ptr().add(i) as *mut __m128i, _mm_avg_epu8(x, y));
            i += 16;
        }
        while i + 8 <= w {
            let x = load8(a.as_ptr().add(i));
            let y = load8(b.as_ptr().add(i));
            _mm_storel_epi64(out.as_mut_ptr().add(i) as *mut __m128i, _mm_avg_epu8(x, y));
            i += 8;
        }
    }
    while i < w {
        out[i] = ((a[i] as u16 + b[i] as u16 + 1) >> 1) as u8;
        i += 1;
    }
}

pub fn chroma_bilinear(plane: &[u8], top: usize, stride: usize, w: usize, h: usize, out: &mut [u8], c00: u16, c01: u16, c10: u16, c11: u16) {
    assert!(top + h * stride + w + 1 <= plane.len() && out.len() >= w * h);
    unsafe {
        let k00 = _mm_set1_epi16(c00 as i16);
        let k01 = _mm_set1_epi16(c01 as i16);
        let k10 = _mm_set1_epi16(c10 as i16);
        let k11 = _mm_set1_epi16(c11 as i16);
        let round = _mm_set1_epi16(32);
        for r in 0..h {
            let t = plane.as_ptr().add(top + r * stride);
            let b = t.add(stride);
            let o = out.as_mut_ptr().add(r * w);
            let mut i = 0;
            while i + 8 <= w {
                let s = _mm_add_epi16(_mm_mullo_epi16(widen8(t.add(i)), k00), _mm_mullo_epi16(widen8(t.add(i + 1)), k01));
                let s = _mm_add_epi16(s, _mm_mullo_epi16(widen8(b.add(i)), k10));
                let s = _mm_add_epi16(s, _mm_mullo_epi16(widen8(b.add(i + 1)), k11));
                let v = _mm_srli_epi16(_mm_add_epi16(s, round), 6);
                _mm_storel_epi64(o.add(i) as *mut __m128i, _mm_packus_epi16(v, v));
                i += 8;
            }
            while i < w {
                let v = c00 as u32 * *t.add(i) as u32 + c01 as u32 * *t.add(i + 1) as u32 + c10 as u32 * *b.add(i) as u32 + c11 as u32 * *b.add(i + 1) as u32;
                *o.add(i) = ((v + 32) >> 6) as u8;
                i += 1;
            }
        }
    }
}

pub fn fir6_horizontal_i16(src: &[u8], out: &mut [i16], w: usize) {
    assert!(src.len() >= w + 5 && out.len() >= w);
    let mut i = 0;
    unsafe {
        while i + 8 <= w {
            let p = src.as_ptr().add(i);
            let v = fir6_u8(widen8(p), widen8(p.add(1)), widen8(p.add(2)), widen8(p.add(3)), widen8(p.add(4)), widen8(p.add(5)));
            _mm_storeu_si128(out.as_mut_ptr().add(i) as *mut __m128i, v);
            i += 8;
        }
    }
    while i < w {
        out[i] = fir6_scalar(src, i) as i16;
        i += 1;
    }
}

pub fn round_i16(src: &[i16], out: &mut [u8], w: usize) {
    assert!(src.len() >= w && out.len() >= w);
    let mut i = 0;
    unsafe {
        let round = _mm_set1_epi16(16);
        while i + 8 <= w {
            let v = _mm_loadu_si128(src.as_ptr().add(i) as *const __m128i);
            let r = _mm_srai_epi16(_mm_add_epi16(v, round), 5);
            let packed = _mm_packus_epi16(r, r);
            _mm_storel_epi64(out.as_mut_ptr().add(i) as *mut __m128i, packed);
            i += 8;
        }
    }
    while i < w {
        out[i] = ((src[i] as i32 + 16) >> 5).clamp(0, 255) as u8;
        i += 1;
    }
}

pub fn fir6_vertical_i16(rows: [&[i16]; 6], out: &mut [u8], w: usize) {
    assert!(rows.iter().all(|r| r.len() >= w) && out.len() >= w);
    let mut i = 0;
    unsafe {
        let round = _mm_set1_epi32(512);
        while i + 8 <= w {
            let load = |k: usize| _mm_loadu_si128(rows[k].as_ptr().add(i) as *const __m128i);
            let af = _mm_add_epi16(load(0), load(5));
            let be = _mm_add_epi16(load(1), load(4));
            let cd = _mm_add_epi16(load(2), load(3));
            let (af_lo, af_hi) = sign_extend(af);
            let (be_lo, be_hi) = sign_extend(be);
            let (cd_lo, cd_hi) = sign_extend(cd);
            let lo = _mm_srai_epi32(_mm_add_epi32(fir6_i32(af_lo, be_lo, cd_lo), round), 10);
            let hi = _mm_srai_epi32(_mm_add_epi32(fir6_i32(af_hi, be_hi, cd_hi), round), 10);
            let words = _mm_packs_epi32(lo, hi);
            _mm_storel_epi64(out.as_mut_ptr().add(i) as *mut __m128i, _mm_packus_epi16(words, words));
            i += 8;
        }
    }
    while i < w {
        let v = rows[0][i] as i32 - 5 * rows[1][i] as i32 + 20 * rows[2][i] as i32 + 20 * rows[3][i] as i32 - 5 * rows[4][i] as i32 + rows[5][i] as i32;
        out[i] = ((v + 512) >> 10).clamp(0, 255) as u8;
        i += 1;
    }
}
