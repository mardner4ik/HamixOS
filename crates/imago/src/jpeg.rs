use alloc::vec::Vec;

use crate::{argb, check_size, Bitmap, Error, Result};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52,
    45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
const FAST: u32 = 9;

#[derive(Clone)]
struct Huffman {
    fast: [u8; 1 << FAST],
    fast_ac: [i16; 1 << FAST],
    code: [u16; 256],
    values: [u8; 256],
    size: [u8; 257],
    maxcode: [u32; 18],
    delta: [i32; 17],
}

impl Huffman {
    fn build(counts: &[u8; 16], values: &[u8]) -> Result<Huffman> {
        let mut h = Huffman { fast: [255; 1 << FAST], fast_ac: [0; 1 << FAST], code: [0; 256], values: [0; 256], size: [0; 257], maxcode: [0; 18], delta: [0; 17] };
        let mut k = 0usize;
        for (i, &c) in counts.iter().enumerate() {
            for _ in 0..c {
                if k >= 256 {
                    return Err(Error::Corrupt("jpeg: huffman table too large"));
                }
                h.size[k] = (i + 1) as u8;
                k += 1;
            }
        }
        h.size[k] = 0;
        if values.len() < k {
            return Err(Error::Truncated);
        }
        h.values[..k].copy_from_slice(&values[..k]);
        let mut code = 0u32;
        let mut k = 0usize;
        for j in 1..=16u32 {
            h.delta[j as usize] = k as i32 - code as i32;
            if h.size[k] as u32 == j {
                while h.size[k] as u32 == j {
                    h.code[k] = code as u16;
                    code += 1;
                    k += 1;
                }
                if code - 1 >= 1 << j {
                    return Err(Error::Corrupt("jpeg: bad huffman code lengths"));
                }
            }
            h.maxcode[j as usize] = code << (16 - j);
            code <<= 1;
        }
        h.maxcode[17] = u32::MAX;
        for i in 0..k {
            let s = h.size[i] as u32;
            if s <= FAST {
                let c = (h.code[i] as u32) << (FAST - s);
                let m = 1 << (FAST - s);
                for j in 0..m {
                    h.fast[(c + j) as usize] = i as u8;
                }
            }
        }
        for i in 0..(1usize << FAST) {
            let k = h.fast[i];
            if k == 255 {
                continue;
            }
            let rs = h.values[k as usize] as i32;
            let (run, mag) = ((rs >> 4) & 15, rs & 15);
            let len = h.size[k as usize] as i32;
            if mag != 0 && len + mag <= FAST as i32 {
                let bits = ((i as i32) << len) & ((1 << FAST) - 1);
                let raw = bits >> (FAST as i32 - mag);
                let m = 1 << (mag - 1);
                let value = if raw < m { raw - (1 << mag) + 1 } else { raw };
                if (-128..=127).contains(&value) {
                    h.fast_ac[i] = ((value * 256) + (run * 16) + (len + mag)) as i16;
                }
            }
        }
        Ok(h)
    }
}

struct Component {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    dc: usize,
    ac: usize,
    pred: i32,
    bw: usize,
    bh: usize,
    coeffs: Vec<i16>,
    plane: Vec<u8>,
    pw: usize,
    ph: usize,
    eob_run: u32,
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u32,
    count: i32,
    marker: Option<u8>,
}

impl<'a> Bits<'a> {
    #[inline(always)]
    fn fill(&mut self) {
        while self.count <= 24 {
            let mut byte = 0u32;
            if self.marker.is_none() {
                if let Some(&b) = self.data.get(self.pos) {
                    if b == 0xFF {
                        let next = self.data.get(self.pos + 1).copied().unwrap_or(0xD9);
                        if next == 0 {
                            self.pos += 2;
                            byte = 0xFF;
                        } else if next == 0xFF {
                            self.pos += 1;
                            continue;
                        } else {
                            self.marker = Some(next);
                        }
                    } else {
                        self.pos += 1;
                        byte = b as u32;
                    }
                } else {
                    self.marker = Some(0xD9);
                }
            }
            self.buffer |= byte << (24 - self.count);
            self.count += 8;
        }
    }

    #[inline(always)]
    fn decode(&mut self, h: &Huffman) -> Result<u8> {
        if self.count < 16 {
            self.fill();
        }
        let k = h.fast[(self.buffer >> (32 - FAST)) as usize];
        if k < 255 {
            let s = h.size[k as usize] as i32;
            if s > self.count {
                return Err(Error::Corrupt("jpeg: huffman overrun"));
            }
            self.buffer <<= s;
            self.count -= s;
            return Ok(h.values[k as usize]);
        }
        let temp = self.buffer >> 16;
        let mut k = FAST as usize + 1;
        while k < 17 && temp >= h.maxcode[k] {
            k += 1;
        }
        if k == 17 || k as i32 > self.count {
            self.count -= 16;
            return Err(Error::Corrupt("jpeg: bad huffman code"));
        }
        let c = ((self.buffer >> (32 - k)) as i32 + h.delta[k]) as usize;
        if c >= 256 {
            return Err(Error::Corrupt("jpeg: bad huffman code"));
        }
        self.buffer <<= k;
        self.count -= k as i32;
        Ok(h.values[c])
    }

    #[inline(always)]
    fn receive(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        if self.count < n as i32 {
            self.fill();
        }
        let v = (self.buffer >> (32 - n)) as i32;
        self.buffer <<= n;
        self.count -= n as i32;
        v
    }

    #[inline(always)]
    fn extend(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        let v = self.receive(n);
        if v < 1 << (n - 1) { v - (1 << n) + 1 } else { v }
    }

    fn bit(&mut self) -> i32 {
        self.receive(1)
    }

    fn reset(&mut self) {
        self.buffer = 0;
        self.count = 0;
        self.marker = None;
    }

    fn restart(&mut self) -> Result<()> {
        if self.marker.is_none() {
            while self.pos + 1 < self.data.len() && !(self.data[self.pos] == 0xFF && (0xD0..=0xD7).contains(&self.data[self.pos + 1])) {
                self.pos += 1;
            }
        }
        if self.pos + 1 < self.data.len() && self.data[self.pos] == 0xFF && (0xD0..=0xD7).contains(&self.data[self.pos + 1]) {
            self.pos += 2;
        }
        self.reset();
        Ok(())
    }
}

#[inline(always)]
fn clamp(v: i32) -> u8 {
    if v as u32 > 255 { if v < 0 { 0 } else { 255 } } else { v as u8 }
}

const fn fix(x: f64) -> i32 {
    (x * 4096.0 + 0.5) as i32
}

const C0: i32 = fix(0.5411961);
const C1: i32 = fix(-1.847759065);
const C2: i32 = fix(0.765366865);
const C3: i32 = fix(1.175875602);
const C4: i32 = fix(0.298631336);
const C5: i32 = fix(2.053119869);
const C6: i32 = fix(3.072711026);
const C7: i32 = fix(1.501321110);
const C8: i32 = fix(-0.899976223);
const C9: i32 = fix(-2.562915447);
const C10: i32 = fix(-1.961570560);
const C11: i32 = fix(-0.390180644);

#[inline(always)]
fn idct_1d(s: [i32; 8]) -> ([i32; 4], [i32; 4]) {
    let p1 = (s[2] + s[6]) * C0;
    let t2 = p1 + s[6] * C1;
    let t3 = p1 + s[2] * C2;
    let t0 = (s[0] + s[4]) << 12;
    let t1 = (s[0] - s[4]) << 12;
    let x = [t0 + t3, t1 + t2, t1 - t2, t0 - t3];
    let (a0, a1, a2, a3) = (s[7], s[5], s[3], s[1]);
    let p3 = a0 + a2;
    let p4 = a1 + a3;
    let p1 = a0 + a3;
    let p2 = a1 + a2;
    let p5 = (p3 + p4) * C3;
    let p1 = p5 + p1 * C8;
    let p2 = p5 + p2 * C9;
    let p3 = p3 * C10;
    let p4 = p4 * C11;
    let t = [a0 * C4 + p1 + p3, a1 * C5 + p2 + p4, a2 * C6 + p2 + p3, a3 * C7 + p1 + p4];
    (x, t)
}

fn idct(input: &[i16; 64], q: &[u16; 64], out: &mut [u8], stride: usize) {
    if input[1..].iter().all(|v| *v == 0) {
        let v = clamp(((input[0] as i32 * q[0] as i32 + 4) >> 3) + 128);
        for r in 0..8 {
            out[r * stride..r * stride + 8].fill(v);
        }
        return;
    }
    let mut d = [0i32; 64];
    for i in 0..64 {
        d[i] = input[i] as i32 * q[i] as i32;
    }
    let mut tmp = [0i32; 64];
    for i in 0..8 {
        if d[8 + i] == 0 && d[16 + i] == 0 && d[24 + i] == 0 && d[32 + i] == 0 && d[40 + i] == 0 && d[48 + i] == 0 && d[56 + i] == 0 {
            let dc = d[i] << 2;
            for r in 0..8 {
                tmp[r * 8 + i] = dc;
            }
            continue;
        }
        let (x, t) = idct_1d([d[i], d[8 + i], d[16 + i], d[24 + i], d[32 + i], d[40 + i], d[48 + i], d[56 + i]]);
        let x = [x[0] + 512, x[1] + 512, x[2] + 512, x[3] + 512];
        tmp[i] = (x[0] + t[3]) >> 10;
        tmp[56 + i] = (x[0] - t[3]) >> 10;
        tmp[8 + i] = (x[1] + t[2]) >> 10;
        tmp[48 + i] = (x[1] - t[2]) >> 10;
        tmp[16 + i] = (x[2] + t[1]) >> 10;
        tmp[40 + i] = (x[2] - t[1]) >> 10;
        tmp[24 + i] = (x[3] + t[0]) >> 10;
        tmp[32 + i] = (x[3] - t[0]) >> 10;
    }
    const BIAS: i32 = 65536 + (128 << 17);
    for r in 0..8 {
        let v: [i32; 8] = tmp[r * 8..r * 8 + 8].try_into().unwrap();
        let (x, t) = idct_1d(v);
        let x = [x[0] + BIAS, x[1] + BIAS, x[2] + BIAS, x[3] + BIAS];
        let o: &mut [u8; 8] = (&mut out[r * stride..r * stride + 8]).try_into().unwrap();
        o[0] = clamp((x[0] + t[3]) >> 17);
        o[7] = clamp((x[0] - t[3]) >> 17);
        o[1] = clamp((x[1] + t[2]) >> 17);
        o[6] = clamp((x[1] - t[2]) >> 17);
        o[2] = clamp((x[2] + t[1]) >> 17);
        o[5] = clamp((x[2] - t[1]) >> 17);
        o[3] = clamp((x[3] + t[0]) >> 17);
        o[4] = clamp((x[3] - t[0]) >> 17);
    }
}


fn reduce_block(input: &[i16; 64], q: &[u16; 64], out: &mut [u8], stride: usize, scale: usize) {
    if scale == 8 {
        out[0] = clamp(((input[0] as i32 * q[0] as i32) >> 3) + 128);
        return;
    }
    let n = 8 / scale;
    if input[1..].iter().all(|v| *v == 0) {
        let v = clamp(((input[0] as i32 * q[0] as i32 + 4) >> 3) + 128);
        for y in 0..n {
            out[y * stride..y * stride + n].fill(v);
        }
        return;
    }
    let mut full = [0u8; 64];
    idct(input, q, &mut full, 8);
    let area = (scale * scale) as u32;
    for y in 0..n {
        for x in 0..n {
            let mut sum = 0u32;
            for dy in 0..scale {
                for dx in 0..scale {
                    sum += full[(y * scale + dy) * 8 + x * scale + dx] as u32;
                }
            }
            out[y * stride + x] = ((sum + area / 2) / area) as u8;
        }
    }
}

struct Decoder<'a> {
    data: &'a [u8],
    quant: [[u16; 64]; 4],
    dc: [Option<Huffman>; 4],
    ac: [Option<Huffman>; 4],
    comps: Vec<Component>,
    width: usize,
    height: usize,
    hmax: usize,
    vmax: usize,
    mcux: usize,
    mcuy: usize,
    progressive: bool,
    restart: usize,
    adobe: Option<u8>,
    orientation: u8,
    scale: usize,
    frame_seen: bool,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8], scale: usize) -> Decoder<'a> {
        Decoder {
            data,
            quant: [[1; 64]; 4],
            dc: [None, None, None, None],
            ac: [None, None, None, None],
            comps: Vec::new(),
            width: 0,
            height: 0,
            hmax: 1,
            vmax: 1,
            mcux: 0,
            mcuy: 0,
            progressive: false,
            restart: 0,
            adobe: None,
            orientation: 1,
            scale,
            frame_seen: false,
        }
    }

    fn segment(&self, pos: usize) -> Result<&'a [u8]> {
        let len = u16::from_be_bytes([*self.data.get(pos).ok_or(Error::Truncated)?, *self.data.get(pos + 1).ok_or(Error::Truncated)?]) as usize;
        if len < 2 {
            return Err(Error::Corrupt("jpeg: bad segment length"));
        }
        self.data.get(pos + 2..pos + len).ok_or(Error::Truncated)
    }

    fn frame(&mut self, s: &[u8], progressive: bool, header_only: bool) -> Result<()> {
        if s.len() < 6 {
            return Err(Error::Truncated);
        }
        if s[0] != 8 {
            return Err(Error::Unsupported("jpeg: only 8-bit samples"));
        }
        self.height = u16::from_be_bytes([s[1], s[2]]) as usize;
        self.width = u16::from_be_bytes([s[3], s[4]]) as usize;
        check_size(self.width as u32, self.height as u32)?;
        let n = s[5] as usize;
        if !matches!(n, 1 | 3 | 4) || s.len() < 6 + n * 3 {
            return Err(Error::Unsupported("jpeg: component count"));
        }
        self.progressive = progressive;
        self.frame_seen = true;
        if header_only {
            return Ok(());
        }
        for i in 0..n {
            let c = &s[6 + i * 3..9 + i * 3];
            let (h, v) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
            if !(1..=4).contains(&h) || !(1..=4).contains(&v) || c[2] > 3 {
                return Err(Error::Corrupt("jpeg: bad sampling factors"));
            }
            self.comps.push(Component { id: c[0], h, v, tq: c[2] as usize, dc: 0, ac: 0, pred: 0, bw: 0, bh: 0, coeffs: Vec::new(), plane: Vec::new(), pw: 0, ph: 0, eob_run: 0 });
        }
        self.hmax = self.comps.iter().map(|c| c.h).max().unwrap_or(1);
        self.vmax = self.comps.iter().map(|c| c.v).max().unwrap_or(1);
        self.mcux = self.width.div_ceil(8 * self.hmax);
        self.mcuy = self.height.div_ceil(8 * self.vmax);
        let unit = 8 / self.scale;
        for c in self.comps.iter_mut() {
            c.bw = self.mcux * c.h;
            c.bh = self.mcuy * c.v;
            c.pw = c.bw * unit;
            c.ph = c.bh * unit;
            c.plane = alloc::vec![0u8; c.pw * c.ph];
            if self.progressive {
                c.coeffs = alloc::vec![0i16; c.bw * c.bh * 64];
            }
        }
        Ok(())
    }

    fn tables(&mut self, s: &[u8]) -> Result<()> {
        let mut i = 0;
        while i < s.len() {
            let info = s[i];
            let class = info >> 4;
            let id = (info & 15) as usize;
            if id > 3 {
                return Err(Error::Corrupt("jpeg: huffman table id"));
            }
            let counts: [u8; 16] = s.get(i + 1..i + 17).ok_or(Error::Truncated)?.try_into().unwrap();
            let total: usize = counts.iter().map(|c| *c as usize).sum();
            let values = s.get(i + 17..i + 17 + total).ok_or(Error::Truncated)?;
            let table = Huffman::build(&counts, values)?;
            if class == 0 {
                self.dc[id] = Some(table);
            } else {
                self.ac[id] = Some(table);
            }
            i += 17 + total;
        }
        Ok(())
    }

    fn quantization(&mut self, s: &[u8]) -> Result<()> {
        let mut i = 0;
        while i < s.len() {
            let precision = s[i] >> 4;
            let id = (s[i] & 15) as usize;
            if id > 3 {
                return Err(Error::Corrupt("jpeg: quantization table id"));
            }
            i += 1;
            for k in 0..64 {
                let v = if precision == 0 {
                    let v = *s.get(i).ok_or(Error::Truncated)? as u16;
                    i += 1;
                    v
                } else {
                    let v = u16::from_be_bytes([*s.get(i).ok_or(Error::Truncated)?, *s.get(i + 1).ok_or(Error::Truncated)?]);
                    i += 2;
                    v
                };
                self.quant[id][ZIGZAG[k]] = v;
            }
        }
        Ok(())
    }

    fn exif(&mut self, s: &[u8]) {
        if !s.starts_with(b"Exif\0\0") || s.len() < 14 {
            return;
        }
        let t = &s[6..];
        let le = t.starts_with(b"II");
        let u16at = |o: usize| t.get(o..o + 2).map(|b| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) });
        let u32at = |o: usize| t.get(o..o + 4).map(|b| if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) });
        let Some(ifd) = u32at(4) else {
            return;
        };
        let Some(count) = u16at(ifd as usize) else {
            return;
        };
        for e in 0..count as usize {
            let at = ifd as usize + 2 + e * 12;
            if u16at(at) == Some(0x0112) {
                if let Some(v) = u16at(at + 8) {
                    if (1..=8).contains(&v) {
                        self.orientation = v as u8;
                    }
                }
            }
        }
    }

    fn block_baseline(&mut self, bits: &mut Bits, ci: usize, bx: usize, by: usize) -> Result<()> {
        let mut block = [0i16; 64];
        let c = &mut self.comps[ci];
        let dc = self.dc[c.dc].as_ref().ok_or(Error::Corrupt("jpeg: missing DC table"))?;
        let ac = self.ac[c.ac].as_ref().ok_or(Error::Corrupt("jpeg: missing AC table"))?;
        let t = bits.decode(dc)? as u32;
        if t > 16 {
            return Err(Error::Corrupt("jpeg: bad DC size"));
        }
        let diff = bits.extend(t);
        c.pred += diff;
        block[0] = c.pred as i16;
        let mut k = 1;
        while k < 64 {
            if bits.count < 16 {
                bits.fill();
            }
            let fast = ac.fast_ac[(bits.buffer >> (32 - FAST)) as usize];
            if fast != 0 {
                let len = (fast & 15) as i32;
                bits.buffer <<= len;
                bits.count -= len;
                k += ((fast >> 4) & 15) as usize;
                if k > 63 {
                    return Err(Error::Corrupt("jpeg: AC overrun"));
                }
                block[ZIGZAG[k]] = fast >> 8;
                k += 1;
                continue;
            }
            let rs = bits.decode(ac)?;
            let (r, s) = ((rs >> 4) as usize, (rs & 15) as u32);
            if s == 0 {
                if r != 15 {
                    break;
                }
                k += 16;
                continue;
            }
            k += r;
            if k > 63 {
                return Err(Error::Corrupt("jpeg: AC overrun"));
            }
            block[ZIGZAG[k]] = bits.extend(s) as i16;
            k += 1;
        }
        let unit = 8 / self.scale;
        let stride = c.pw;
        let offset = by * unit * stride + bx * unit;
        let q = &self.quant[c.tq];
        if self.scale == 1 {
            idct(&block, q, &mut c.plane[offset..], stride);
        } else {
            reduce_block(&block, q, &mut c.plane[offset..], stride, self.scale);
        }
        Ok(())
    }

    fn block_progressive(&mut self, bits: &mut Bits, ci: usize, bx: usize, by: usize, ss: usize, se: usize, ah: u32, al: u32) -> Result<()> {
        let c = &mut self.comps[ci];
        let base = (by * c.bw + bx) * 64;
        let block = &mut c.coeffs[base..base + 64];
        if ss == 0 {
            if ah == 0 {
                let dc = self.dc[c.dc].as_ref().ok_or(Error::Corrupt("jpeg: missing DC table"))?;
                let t = bits.decode(dc)? as u32;
                if t > 16 {
                    return Err(Error::Corrupt("jpeg: bad DC size"));
                }
                c.pred += bits.extend(t);
                block[0] = (c.pred << al) as i16;
            } else if bits.bit() != 0 {
                block[0] |= (1 << al) as i16;
            }
            return Ok(());
        }
        let ac = self.ac[c.ac].as_ref().ok_or(Error::Corrupt("jpeg: missing AC table"))?;
        if ah == 0 {
            if c.eob_run > 0 {
                c.eob_run -= 1;
                return Ok(());
            }
            let mut k = ss;
            while k <= se {
                let rs = bits.decode(ac)?;
                let (r, s) = ((rs >> 4) as u32, (rs & 15) as u32);
                if s == 0 {
                    if r < 15 {
                        c.eob_run = (1 << r) - 1;
                        if r > 0 {
                            c.eob_run += bits.receive(r) as u32;
                        }
                        break;
                    }
                    k += 16;
                    continue;
                }
                k += r as usize;
                if k > 63 {
                    return Err(Error::Corrupt("jpeg: AC overrun"));
                }
                block[ZIGZAG[k]] = (bits.extend(s) * (1 << al)) as i16;
                k += 1;
            }
            return Ok(());
        }
        let bit = (1i32 << al) as i16;
        let mut k = ss;
        if c.eob_run == 0 {
            while k <= se {
                let rs = bits.decode(ac)?;
                let (mut r, s) = ((rs >> 4) as i32, (rs & 15) as u32);
                let mut value = 0i16;
                if s == 0 {
                    if r < 15 {
                        c.eob_run = (1 << r) as u32;
                        if r > 0 {
                            c.eob_run += bits.receive(r as u32) as u32;
                        }
                        break;
                    }
                } else {
                    if s != 1 {
                        return Err(Error::Corrupt("jpeg: bad refinement size"));
                    }
                    value = if bits.bit() != 0 { bit } else { -bit };
                }
                while k <= se {
                    let z = ZIGZAG[k];
                    k += 1;
                    if block[z] != 0 {
                        if bits.bit() != 0 && block[z] & bit == 0 {
                            block[z] += if block[z] > 0 { bit } else { -bit };
                        }
                    } else {
                        if r == 0 {
                            if value != 0 {
                                block[z] = value;
                            }
                            break;
                        }
                        r -= 1;
                    }
                }
            }
        }
        if c.eob_run > 0 {
            while k <= se {
                let z = ZIGZAG[k];
                k += 1;
                if block[z] != 0 && bits.bit() != 0 && block[z] & bit == 0 {
                    block[z] += if block[z] > 0 { bit } else { -bit };
                }
            }
            c.eob_run -= 1;
        }
        Ok(())
    }

    fn scan(&mut self, s: &[u8], mut pos: usize) -> Result<usize> {
        let n = *s.first().ok_or(Error::Truncated)? as usize;
        if n == 0 || n > 4 || s.len() < 1 + n * 2 + 3 {
            return Err(Error::Corrupt("jpeg: bad scan header"));
        }
        let mut order = Vec::new();
        for i in 0..n {
            let id = s[1 + i * 2];
            let tables = s[2 + i * 2];
            let ci = self.comps.iter().position(|c| c.id == id).ok_or(Error::Corrupt("jpeg: unknown component in scan"))?;
            self.comps[ci].dc = ((tables >> 4) & 3) as usize;
            self.comps[ci].ac = (tables & 3) as usize;
            order.push(ci);
        }
        let ss = s[1 + n * 2] as usize;
        let se = s[2 + n * 2] as usize;
        let ah = (s[3 + n * 2] >> 4) as u32;
        let al = (s[3 + n * 2] & 15) as u32;
        if self.progressive && (ss > se || se > 63 || (ss == 0 && se != 0) || (ss > 0 && n != 1)) {
            return Err(Error::Corrupt("jpeg: bad progressive scan"));
        }
        for c in self.comps.iter_mut() {
            c.pred = 0;
            c.eob_run = 0;
        }
        let mut bits = Bits { data: self.data, pos, buffer: 0, count: 0, marker: None };
        let mut todo = if self.restart > 0 { self.restart } else { usize::MAX };
        let single = n == 1;
        let (w, h) = if single {
            let c = &self.comps[order[0]];
            ((self.width * c.h).div_ceil(8 * self.hmax), (self.height * c.v).div_ceil(8 * self.vmax))
        } else {
            (self.mcux, self.mcuy)
        };
        for my in 0..h {
            for mx in 0..w {
                if single {
                    let ci = order[0];
                    if self.progressive {
                        self.block_progressive(&mut bits, ci, mx, my, ss, se, ah, al)?;
                    } else {
                        self.block_baseline(&mut bits, ci, mx, my)?;
                    }
                } else {
                    for &ci in order.iter() {
                        let (ch, cv) = (self.comps[ci].h, self.comps[ci].v);
                        for by in 0..cv {
                            for bx in 0..ch {
                                let (x, y) = (mx * ch + bx, my * cv + by);
                                if self.progressive {
                                    self.block_progressive(&mut bits, ci, x, y, ss, se, ah, al)?;
                                } else {
                                    self.block_baseline(&mut bits, ci, x, y)?;
                                }
                            }
                        }
                    }
                }
                todo -= 1;
                if todo == 0 {
                    todo = self.restart;
                    bits.restart()?;
                    for c in self.comps.iter_mut() {
                        c.pred = 0;
                        c.eob_run = 0;
                    }
                }
            }
        }
        pos = bits.pos;
        while pos + 1 < self.data.len() && !(self.data[pos] == 0xFF && self.data[pos + 1] != 0 && !(0xD0..=0xD7).contains(&self.data[pos + 1])) {
            pos += 1;
        }
        Ok(pos)
    }

    fn finish_progressive(&mut self) {
        let unit = 8 / self.scale;
        for c in self.comps.iter_mut() {
            let q = self.quant[c.tq];
            for by in 0..c.bh {
                for bx in 0..c.bw {
                    let base = (by * c.bw + bx) * 64;
                    let block: [i16; 64] = c.coeffs[base..base + 64].try_into().unwrap();
                    let offset = by * unit * c.pw + bx * unit;
                    if self.scale == 1 {
                        idct(&block, &q, &mut c.plane[offset..], c.pw);
                    } else {
                        reduce_block(&block, &q, &mut c.plane[offset..], c.pw, self.scale);
                    }
                }
            }
            c.coeffs = Vec::new();
        }
    }

    fn run(&mut self, header_only: bool) -> Result<()> {
        if !self.data.starts_with(&[0xFF, 0xD8]) {
            return Err(Error::Unknown);
        }
        let mut pos = 2;
        loop {
            while self.data.get(pos) == Some(&0xFF) && self.data.get(pos + 1) == Some(&0xFF) {
                pos += 1;
            }
            if self.data.get(pos) != Some(&0xFF) {
                return if self.frame_seen && !header_only { Ok(()) } else { Err(Error::Corrupt("jpeg: expected a marker")) };
            }
            let marker = *self.data.get(pos + 1).ok_or(Error::Truncated)?;
            pos += 2;
            match marker {
                0xD8 | 0x01 | 0xD0..=0xD7 => continue,
                0xD9 => return Ok(()),
                _ => {}
            }
            let segment = self.segment(pos)?;
            let next = pos + 2 + segment.len();
            match marker {
                0xC0 | 0xC1 | 0xC2 => {
                    self.frame(segment, marker == 0xC2, header_only)?;
                    if header_only {
                        return Ok(());
                    }
                }
                0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(Error::Unsupported("jpeg: lossless or arithmetic coding")),
                0xC4 => self.tables(segment)?,
                0xDB => self.quantization(segment)?,
                0xDD => {
                    if segment.len() >= 2 {
                        self.restart = u16::from_be_bytes([segment[0], segment[1]]) as usize;
                    }
                }
                0xE1 => self.exif(segment),
                0xEE => {
                    if segment.starts_with(b"Adobe") && segment.len() >= 12 {
                        self.adobe = Some(segment[11]);
                    }
                }
                0xDA => {
                    if !self.frame_seen {
                        return Err(Error::Corrupt("jpeg: scan before frame"));
                    }
                    pos = self.scan(segment, next)?;
                    continue;
                }
                _ => {}
            }
            pos = next;
        }
    }

    fn plane(&self, ci: usize, w: usize, h: usize) -> alloc::borrow::Cow<'_, [u8]> {
        let c = &self.comps[ci];
        if c.h == self.hmax && c.v == self.vmax {
            if c.pw == w {
                return alloc::borrow::Cow::Borrowed(&c.plane[..w * h]);
            }
            let mut out = alloc::vec![0u8; w * h];
            for y in 0..h {
                out[y * w..(y + 1) * w].copy_from_slice(&c.plane[y * c.pw..y * c.pw + w]);
            }
            return alloc::borrow::Cow::Owned(out);
        }
        let (fx, fy) = (self.hmax / c.h, self.vmax / c.v);
        let mut out = alloc::vec![0u8; w * h];
        let cw = c.pw;
        let ch = c.ph;
        if (fx == 2 || fx == 1) && (fy == 2 || fy == 1) && self.hmax % c.h == 0 && self.vmax % c.v == 0 {
            let mut t = alloc::vec![0u16; cw];
            for y in 0..h {
                let near = (y / fy).min(ch - 1);
                if fy == 2 {
                    let far = if y % 2 == 0 { near.saturating_sub(1) } else { (near + 1).min(ch - 1) };
                    let (rn, rf) = (&c.plane[near * cw..near * cw + cw], &c.plane[far * cw..far * cw + cw]);
                    for i in 0..cw {
                        t[i] = 3 * rn[i] as u16 + rf[i] as u16;
                    }
                } else {
                    for (i, v) in c.plane[near * cw..near * cw + cw].iter().enumerate() {
                        t[i] = *v as u16 * 4;
                    }
                }
                let row = &mut out[y * w..(y + 1) * w];
                if fx == 2 {
                    let pairs = (w / 2).min(cw);
                    for (sx, pair) in row.chunks_exact_mut(2).take(pairs).enumerate() {
                        let mid = 3 * t[sx] as u32 + 8;
                        let left = t[sx.saturating_sub(1)] as u32;
                        let right = t[(sx + 1).min(cw - 1)] as u32;
                        pair[0] = ((mid + left) >> 4) as u8;
                        pair[1] = ((mid + right) >> 4) as u8;
                    }
                    for x in pairs * 2..w {
                        let sx = (x / 2).min(cw - 1);
                        let other = if x % 2 == 0 { sx.saturating_sub(1) } else { (sx + 1).min(cw - 1) };
                        row[x] = ((3 * t[sx] as u32 + t[other] as u32 + 8) >> 4) as u8;
                    }
                } else {
                    for (x, o) in row.iter_mut().enumerate() {
                        *o = ((t[x.min(cw - 1)] as u32 + 2) >> 2) as u8;
                    }
                }
            }
            return alloc::borrow::Cow::Owned(out);
        }
        for y in 0..h {
            let sy = (y * c.v / self.vmax).min(ch - 1);
            for x in 0..w {
                out[y * w + x] = c.plane[sy * cw + (x * c.h / self.hmax).min(cw - 1)];
            }
        }
        alloc::borrow::Cow::Owned(out)
    }

    fn output(&self) -> Result<Bitmap> {
        let unit = 8 / self.scale;
        let w = (self.width * unit).div_ceil(8).max(1);
        let h = (self.height * unit).div_ceil(8).max(1);
        let mut out = Bitmap::new(w as u32, h as u32)?;
        let n = self.comps.len();
        let planes: Vec<alloc::borrow::Cow<'_, [u8]>> = (0..n).map(|ci| self.plane(ci, w, h)).collect();
        match (n, self.adobe) {
            (1, _) | (2, _) => {
                for (o, &g) in out.pixels.iter_mut().zip(planes[0].iter()) {
                    *o = 0xFF00_0000 | (g as u32) * 0x01_0101;
                }
            }
            (3, Some(0)) => {
                for i in 0..w * h {
                    out.pixels[i] = argb(255, planes[0][i], planes[1][i], planes[2][i]);
                }
            }
            (3, _) => {
                let (yp, cbp, crp) = (&planes[0], &planes[1], &planes[2]);
                for (i, o) in out.pixels.iter_mut().enumerate() {
                    *o = ycc(yp[i] as i32, cbp[i] as i32 - 128, crp[i] as i32 - 128);
                }
            }
            (_, Some(2)) => {
                for i in 0..w * h {
                    let rgb = ycc(planes[0][i] as i32, planes[1][i] as i32 - 128, planes[2][i] as i32 - 128);
                    out.pixels[i] = cmyk(255 - ((rgb >> 16) & 255), 255 - ((rgb >> 8) & 255), 255 - (rgb & 255), planes[3][i] as u32);
                }
            }
            _ => {
                let inverted = self.adobe.is_some();
                for i in 0..w * h {
                    let v = |ci: usize| if inverted { planes[ci][i] as u32 } else { 255 - planes[ci][i] as u32 };
                    out.pixels[i] = cmyk(255 - v(0), 255 - v(1), 255 - v(2), v(3));
                }
            }
        }
        Ok(orient(out, self.orientation))
    }
}

#[inline(always)]
fn ycc(y: i32, cb: i32, cr: i32) -> u32 {
    let y = (y << 16) + 32768;
    let r = (y + cr * 91881) >> 16;
    let g = (y - cb * 22554 - cr * 46802) >> 16;
    let b = (y + cb * 116130) >> 16;
    argb(255, clamp(r), clamp(g), clamp(b))
}

fn cmyk(c: u32, m: u32, y: u32, k: u32) -> u32 {
    let ch = |v: u32| ((255 - v) * k / 255) as u8;
    argb(255, ch(c), ch(m), ch(y))
}

fn orient(src: Bitmap, orientation: u8) -> Bitmap {
    if orientation <= 1 {
        return src;
    }
    let (w, h) = (src.width as usize, src.height as usize);
    let swap = orientation >= 5;
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let mut out = Bitmap { width: ow as u32, height: oh as u32, pixels: alloc::vec![0; w * h] };
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                _ => (y, w - 1 - x),
            };
            out.pixels[ny * ow + nx] = src.pixels[y * w + x];
        }
    }
    out
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let mut d = Decoder::new(data, 1);
    d.run(true)?;
    if !d.frame_seen {
        return Err(Error::Corrupt("jpeg: no frame"));
    }
    Ok((d.width as u32, d.height as u32))
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    decode_scaled(data, 0, 0)
}

pub fn decode_scaled(data: &[u8], width: u32, height: u32) -> Result<Bitmap> {
    let scale = if width == 0 && height == 0 {
        1
    } else {
        let (w, h) = dimensions(data)?;
        let mut s = 1usize;
        while s < 8 && (w as usize / (s * 2)) >= width.max(1) as usize && (h as usize / (s * 2)) >= height.max(1) as usize {
            s *= 2;
        }
        s
    };
    let mut d = Decoder::new(data, scale);
    d.run(false)?;
    if !d.frame_seen || d.comps.is_empty() {
        return Err(Error::Corrupt("jpeg: no image"));
    }
    if d.progressive {
        d.finish_progressive();
    }
    d.output()
}
