use alloc::vec::Vec;

use crate::{argb, check_size, Bitmap, Error, Reader, Result};

pub(crate) struct Dib {
    pub width: u32,
    pub height: u32,
    top_down: bool,
    bits: u16,
    compression: u32,
    masks: [u32; 4],
    palette: Vec<u32>,
    pub header_size: usize,
}

pub(crate) fn dib_header(data: &[u8], icon: bool) -> Result<Dib> {
    let mut r = Reader::new(data);
    let header_size = r.u32le()? as usize;
    let (width, raw_height, bits, compression, colors) = if header_size == 12 {
        let w = r.u16le()? as i32;
        let h = r.u16le()? as i16 as i32;
        r.skip(2)?;
        (w, h, r.u16le()?, 0, 0)
    } else if header_size >= 16 {
        let w = r.u32le()? as i32;
        let h = r.u32le()? as i32;
        r.skip(2)?;
        let bits = r.u16le()?;
        let compression = if header_size >= 20 { r.u32le()? } else { 0 };
        let colors = if header_size >= 36 {
            r.skip(12)?;
            r.u32le()?
        } else {
            0
        };
        (w, h, bits, compression, colors)
    } else {
        return Err(Error::Corrupt("bmp: header too small"));
    };
    let mut height = raw_height;
    if icon {
        height /= 2;
    }
    let top_down = height < 0;
    let (width, height) = (width.unsigned_abs(), height.unsigned_abs());
    check_size(width, height)?;
    let mut masks = match bits {
        16 => [0x7C00, 0x03E0, 0x001F, 0],
        32 => [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0],
        _ => [0; 4],
    };
    let mut palette_at = header_size;
    if compression == 3 || compression == 6 {
        let mut m = Reader::at(data, 40);
        if header_size >= 52 {
            masks = [m.u32le()?, m.u32le()?, m.u32le()?, if header_size >= 56 { m.u32le()? } else { 0 }];
        } else {
            masks = [m.u32le()?, m.u32le()?, m.u32le()?, 0];
            palette_at = 40 + if compression == 6 { 16 } else { 12 };
        }
    } else if bits == 32 && header_size >= 56 && compression == 0 {
        masks[3] = 0xFF00_0000;
    }
    let mut palette = Vec::new();
    if bits <= 8 {
        let n = if colors == 0 { 1usize << bits } else { (colors as usize).min(256) };
        let entry = if header_size == 12 { 3 } else { 4 };
        let mut p = Reader::at(data, palette_at);
        for _ in 0..n {
            let c = p.bytes(entry)?;
            palette.push(argb(255, c[2], c[1], c[0]));
        }
    }
    let header_size = palette_at + palette.len() * if header_size == 12 { 3 } else { 4 };
    Ok(Dib { width, height, top_down, bits, compression, masks, palette, header_size })
}

fn channel(v: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 255;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    let x = (v & mask) >> shift;
    if bits >= 8 { (x >> (bits - 8)) as u8 } else { ((x * 255) / ((1 << bits) - 1)) as u8 }
}

fn rle(dib: &Dib, src: &[u8], out: &mut [u8]) -> Result<()> {
    let (w, h) = (dib.width as usize, dib.height as usize);
    let (mut x, mut y) = (0usize, 0usize);
    let mut i = 0;
    let four = dib.compression == 2;
    while i + 1 < src.len() && y < h {
        let (n, v) = (src[i] as usize, src[i + 1]);
        i += 2;
        if n > 0 {
            for k in 0..n {
                if x < w {
                    out[y * w + x] = if four { if k % 2 == 0 { v >> 4 } else { v & 15 } } else { v };
                }
                x += 1;
            }
            continue;
        }
        match v {
            0 => {
                x = 0;
                y += 1;
            }
            1 => break,
            2 => {
                x += *src.get(i).ok_or(Error::Truncated)? as usize;
                y += *src.get(i + 1).ok_or(Error::Truncated)? as usize;
                i += 2;
            }
            count => {
                let count = count as usize;
                let bytes = if four { count.div_ceil(2) } else { count };
                let run = src.get(i..i + bytes).ok_or(Error::Truncated)?;
                for k in 0..count {
                    let p = if four { if k % 2 == 0 { run[k / 2] >> 4 } else { run[k / 2] & 15 } } else { run[k] };
                    if x < w && y < h {
                        out[y * w + x] = p;
                    }
                    x += 1;
                }
                i += bytes.div_ceil(2) * 2;
            }
        }
    }
    Ok(())
}

pub(crate) fn decode_dib(data: &[u8], dib: &Dib, pixels_at: usize, icon: bool) -> Result<Bitmap> {
    let (w, h) = (dib.width as usize, dib.height as usize);
    let mut out = Bitmap::new(dib.width, dib.height)?;
    let src = data.get(pixels_at..).ok_or(Error::Truncated)?;
    let row_of = |y: usize| if dib.top_down { y } else { h - 1 - y };
    if dib.compression == 1 || dib.compression == 2 {
        let mut indices = alloc::vec![0u8; w * h];
        rle(dib, src, &mut indices)?;
        for y in 0..h {
            for x in 0..w {
                let c = *dib.palette.get(indices[y * w + x] as usize).unwrap_or(&0xFF00_0000);
                out.pixels[row_of(y) * w + x] = c;
            }
        }
        return Ok(out);
    }
    if dib.compression != 0 && dib.compression != 3 && dib.compression != 6 {
        return Err(Error::Unsupported("bmp: compression"));
    }
    let stride = (w * dib.bits as usize).div_ceil(32) * 4;
    if src.len() < stride * h {
        return Err(Error::Truncated);
    }
    let mut alpha_seen = false;
    for y in 0..h {
        let row = &src[y * stride..(y + 1) * stride];
        let dst = &mut out.pixels[row_of(y) * w..row_of(y) * w + w];
        match dib.bits {
            1 | 2 | 4 | 8 => {
                let bits = dib.bits as usize;
                for (x, d) in dst.iter_mut().enumerate() {
                    let bit = x * bits;
                    let index = (row[bit / 8] >> (8 - bits - bit % 8)) & ((1 << bits) - 1) as u8;
                    *d = *dib.palette.get(index as usize).unwrap_or(&0xFF00_0000);
                }
            }
            16 => {
                for (x, d) in dst.iter_mut().enumerate() {
                    let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]) as u32;
                    let a = if dib.masks[3] != 0 { channel(v, dib.masks[3]) } else { 255 };
                    *d = argb(a, channel(v, dib.masks[0]), channel(v, dib.masks[1]), channel(v, dib.masks[2]));
                }
            }
            24 => {
                for (x, d) in dst.iter_mut().enumerate() {
                    let p = &row[x * 3..x * 3 + 3];
                    *d = argb(255, p[2], p[1], p[0]);
                }
            }
            32 => {
                let alpha_mask = if icon { 0xFF00_0000 } else { dib.masks[3] };
                for (x, d) in dst.iter_mut().enumerate() {
                    let v = u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().unwrap());
                    let a = if alpha_mask != 0 { channel(v, alpha_mask) } else { 255 };
                    alpha_seen |= a != 0;
                    *d = argb(a, channel(v, dib.masks[0]), channel(v, dib.masks[1]), channel(v, dib.masks[2]));
                }
            }
            _ => return Err(Error::Unsupported("bmp: bit depth")),
        }
    }
    let has_alpha = dib.bits == 32 && (icon || dib.masks[3] != 0);
    if has_alpha && !alpha_seen {
        for p in out.pixels.iter_mut() {
            *p |= 0xFF00_0000;
        }
    }
    if icon && !(dib.bits == 32 && alpha_seen) {
        let mask_at = pixels_at + stride * h;
        let mask_stride = w.div_ceil(32) * 4;
        if let Some(mask) = data.get(mask_at..mask_at + mask_stride * h) {
            for y in 0..h {
                for x in 0..w {
                    if mask[y * mask_stride + x / 8] >> (7 - x % 8) & 1 == 1 {
                        out.pixels[row_of(y) * w + x] = 0;
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let dib = dib_header(data.get(14..).ok_or(Error::Truncated)?, false)?;
    Ok((dib.width, dib.height))
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    if !data.starts_with(b"BM") {
        return Err(Error::Unknown);
    }
    let offset = u32::from_le_bytes(data.get(10..14).ok_or(Error::Truncated)?.try_into().unwrap()) as usize;
    let body = data.get(14..).ok_or(Error::Truncated)?;
    let dib = dib_header(body, false)?;
    let at = if offset >= 14 { offset - 14 } else { dib.header_size };
    decode_dib(body, &dib, at, false)
}
