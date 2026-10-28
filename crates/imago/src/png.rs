use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::inflate;
use crate::{argb, check_size, Bitmap, Error, Reader, Result};

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const PASSES: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlaced: bool,
}

impl Header {
    fn channels(&self) -> usize {
        match self.color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        }
    }

    fn bits_per_pixel(&self) -> usize {
        self.channels() * self.depth as usize
    }

    fn row_bytes(&self, width: usize) -> usize {
        (width * self.bits_per_pixel()).div_ceil(8)
    }

    fn filter_step(&self) -> usize {
        self.bits_per_pixel().div_ceil(8).max(1)
    }
}

fn header(data: &[u8]) -> Result<Header> {
    if !data.starts_with(SIGNATURE) {
        return Err(Error::Unknown);
    }
    let mut r = Reader::at(data, 8);
    let len = r.u32be()?;
    if r.bytes(4)? != b"IHDR" || len < 13 {
        return Err(Error::Corrupt("png: first chunk is not IHDR"));
    }
    let width = r.u32be()?;
    let height = r.u32be()?;
    let depth = r.u8()?;
    let color = r.u8()?;
    let _compression = r.u8()?;
    let _filter = r.u8()?;
    let interlaced = r.u8()? == 1;
    let valid = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        _ => false,
    };
    if !valid {
        return Err(Error::Unsupported("png: invalid colour type and bit depth"));
    }
    check_size(width, height)?;
    Ok(Header { width, height, depth, color, interlaced })
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let h = header(data)?;
    Ok((h.width, h.height))
}

#[inline(always)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i16, b as i16, c as i16);
    let pa = (ib - ic).abs();
    let pb = (ia - ic).abs();
    let pc = (ia + ib - 2 * ic).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn unfilter_step<const N: usize>(kind: u8, row: &mut [u8], prev: &[u8]) {
    let mut left = [0u8; N];
    let mut corner = [0u8; N];
    match kind {
        1 => {
            for x in row.chunks_exact_mut(N) {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(left[k]);
                    left[k] = x[k];
                }
            }
        }
        3 => {
            for (x, p) in row.chunks_exact_mut(N).zip(prev.chunks_exact(N)) {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(((left[k] as u16 + p[k] as u16) >> 1) as u8);
                    left[k] = x[k];
                }
            }
        }
        _ => {
            for (x, p) in row.chunks_exact_mut(N).zip(prev.chunks_exact(N)) {
                for k in 0..N {
                    x[k] = x[k].wrapping_add(paeth(left[k], p[k], corner[k]));
                    left[k] = x[k];
                    corner[k] = p[k];
                }
            }
        }
    }
}

fn unfilter(kind: u8, row: &mut [u8], prev: &[u8], step: usize) -> Result<()> {
    match kind {
        0 => return Ok(()),
        2 => {
            for (x, p) in row.iter_mut().zip(prev.iter()) {
                *x = x.wrapping_add(*p);
            }
            return Ok(());
        }
        1 | 3 | 4 => {}
        _ => return Err(Error::Corrupt("png: unknown filter type")),
    }
    let whole = row.len() / step * step;
    let (body, tail) = row.split_at_mut(whole);
    match step {
        1 => unfilter_step::<1>(kind, body, prev),
        2 => unfilter_step::<2>(kind, body, prev),
        3 => unfilter_step::<3>(kind, body, prev),
        4 => unfilter_step::<4>(kind, body, prev),
        6 => unfilter_step::<6>(kind, body, prev),
        _ => unfilter_step::<8>(kind, body, prev),
    }
    if !tail.is_empty() {
        let n = whole;
        for i in 0..tail.len() {
            let a = body.get(n + i - step).copied().unwrap_or(0);
            let b = prev[n + i];
            let c = prev.get(n + i - step).copied().unwrap_or(0);
            tail[i] = tail[i].wrapping_add(match kind {
                1 => a,
                3 => ((a as u16 + b as u16) >> 1) as u8,
                _ => paeth(a, b, c),
            });
        }
    }
    Ok(())
}

struct Palette {
    colors: [u32; 256],
    key: Option<(u16, u16, u16)>,
}

fn sample(row: &[u8], depth: u8, index: usize) -> u16 {
    match depth {
        8 => row[index] as u16,
        16 => u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]),
        _ => {
            let bit = index * depth as usize;
            let byte = row[bit / 8];
            let shift = 8 - depth as usize - bit % 8;
            ((byte >> shift) & ((1u8 << depth) - 1)) as u16
        }
    }
}

fn scale8(v: u16, depth: u8) -> u8 {
    match depth {
        1 => if v != 0 { 255 } else { 0 },
        2 => (v * 85) as u8,
        4 => (v * 17) as u8,
        8 => v as u8,
        _ => (v >> 8) as u8,
    }
}

fn convert_row(h: &Header, pal: &Palette, row: &[u8], out: &mut [u32], xs: impl Iterator<Item = usize>) {
    let depth = h.depth;
    match (h.color, depth) {
        (6, 8) => {
            for (x, p) in xs.zip(row.chunks_exact(4)) {
                out[x] = (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
            }
        }
        (2, 8) if pal.key.is_none() => {
            for (x, p) in xs.zip(row.chunks_exact(3)) {
                out[x] = 0xFF00_0000 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
            }
        }
        (2, 8) => {
            for (i, x) in xs.enumerate() {
                let p = &row[i * 3..i * 3 + 3];
                let key = pal.key.map(|k| k == (p[0] as u16, p[1] as u16, p[2] as u16)).unwrap_or(false);
                out[x] = argb(if key { 0 } else { 255 }, p[0], p[1], p[2]);
            }
        }
        (3, _) => {
            for (i, x) in xs.enumerate() {
                out[x] = pal.colors[sample(row, depth, i) as usize];
            }
        }
        (0, _) => {
            for (i, x) in xs.enumerate() {
                let v = sample(row, depth, i);
                let g = scale8(v, depth);
                let key = pal.key.map(|k| k.0 == v).unwrap_or(false);
                out[x] = argb(if key { 0 } else { 255 }, g, g, g);
            }
        }
        (4, _) => {
            for (i, x) in xs.enumerate() {
                let g = scale8(sample(row, depth, i * 2), depth);
                let a = scale8(sample(row, depth, i * 2 + 1), depth);
                out[x] = argb(a, g, g, g);
            }
        }
        (2, _) => {
            for (i, x) in xs.enumerate() {
                let (r, g, b) = (sample(row, depth, i * 3), sample(row, depth, i * 3 + 1), sample(row, depth, i * 3 + 2));
                let key = pal.key.map(|k| k == (r, g, b)).unwrap_or(false);
                out[x] = argb(if key { 0 } else { 255 }, scale8(r, depth), scale8(g, depth), scale8(b, depth));
            }
        }
        _ => {
            for (i, x) in xs.enumerate() {
                let c = |k: usize| scale8(sample(row, depth, i * 4 + k), depth);
                out[x] = argb(c(3), c(0), c(1), c(2));
            }
        }
    }
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    let h = header(data)?;
    let mut pal = Palette { colors: [0xFF00_0000; 256], key: None };
    let mut idat: Cow<[u8]> = Cow::Borrowed(&[]);
    let mut idat_parts = 0;
    let mut r = Reader::at(data, 8);
    while r.remaining() >= 12 {
        let len = r.u32be()? as usize;
        let kind: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        let body = r.bytes(len)?;
        r.skip(4)?;
        match &kind {
            b"PLTE" => {
                for (i, c) in body.chunks_exact(3).take(256).enumerate() {
                    pal.colors[i] = argb(255, c[0], c[1], c[2]);
                }
            }
            b"tRNS" => match h.color {
                3 => {
                    for (i, &a) in body.iter().take(256).enumerate() {
                        pal.colors[i] = (pal.colors[i] & 0x00FF_FFFF) | (a as u32) << 24;
                    }
                }
                0 if body.len() >= 2 => {
                    let v = u16::from_be_bytes([body[0], body[1]]);
                    pal.key = Some((v, v, v));
                }
                2 if body.len() >= 6 => {
                    let c = |k: usize| u16::from_be_bytes([body[k], body[k + 1]]);
                    pal.key = Some((c(0), c(2), c(4)));
                }
                _ => {}
            },
            b"IDAT" => {
                idat_parts += 1;
                if idat_parts == 1 {
                    idat = Cow::Borrowed(body);
                } else {
                    idat.to_mut().extend_from_slice(body);
                }
            }
            b"IEND" => break,
            _ => {}
        }
    }
    if idat.is_empty() {
        return Err(Error::Corrupt("png: no image data"));
    }
    let (w, ht) = (h.width as usize, h.height as usize);
    let expected = if h.interlaced {
        PASSES
            .iter()
            .map(|&(x0, y0, dx, dy)| {
                let pw = (w + dx - 1 - x0) / dx;
                let ph = (ht + dy - 1 - y0) / dy;
                if pw == 0 || ph == 0 { 0 } else { (h.row_bytes(pw) + 1) * ph }
            })
            .sum()
    } else {
        (h.row_bytes(w) + 1) * ht
    };
    let mut raw = inflate::zlib_decompress_sized(&idat, expected).map_err(Error::Corrupt)?;
    if raw.len() < expected {
        return Err(Error::Truncated);
    }
    let mut out = Bitmap::new(h.width, h.height)?;
    let step = h.filter_step();
    if !h.interlaced {
        let stride = h.row_bytes(w);
        let zero = alloc::vec![0u8; stride];
        for y in 0..ht {
            let offset = y * (stride + 1);
            let (before, after) = raw.split_at_mut(offset);
            let prev = if y == 0 { &zero[..] } else { &before[offset - stride..offset] };
            let kind = after[0];
            let row = &mut after[1..1 + stride];
            unfilter(kind, row, prev, step)?;
            convert_row(&h, &pal, row, &mut out.pixels[y * w..(y + 1) * w], 0..w);
        }
        return Ok(out);
    }
    let mut offset = 0;
    for &(x0, y0, dx, dy) in PASSES.iter() {
        let pw = (w + dx - 1 - x0) / dx;
        let ph = (ht + dy - 1 - y0) / dy;
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = h.row_bytes(pw);
        let mut prev: Vec<u8> = alloc::vec![0u8; stride];
        for py in 0..ph {
            let kind = raw[offset];
            let row = &mut raw[offset + 1..offset + 1 + stride];
            unfilter(kind, row, &prev, step)?;
            let y = y0 + py * dy;
            convert_row(&h, &pal, row, &mut out.pixels[y * w..(y + 1) * w], (0..pw).map(|i| x0 + i * dx));
            prev.copy_from_slice(row);
            offset += stride + 1;
        }
    }
    Ok(out)
}
