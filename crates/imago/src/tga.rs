use crate::{argb, check_size, Bitmap, Error, Reader, Result};

struct Header {
    id_len: usize,
    map_type: u8,
    kind: u8,
    map_first: u16,
    map_len: u16,
    map_bits: u8,
    width: u32,
    height: u32,
    bits: u8,
    descriptor: u8,
}

fn header(data: &[u8]) -> Result<Header> {
    let mut r = Reader::new(data);
    let id_len = r.u8()? as usize;
    let map_type = r.u8()?;
    let kind = r.u8()?;
    let map_first = r.u16le()?;
    let map_len = r.u16le()?;
    let map_bits = r.u8()?;
    r.skip(4)?;
    let width = r.u16le()? as u32;
    let height = r.u16le()? as u32;
    let bits = r.u8()?;
    let descriptor = r.u8()?;
    Ok(Header { id_len, map_type, kind, map_first, map_len, map_bits, width, height, bits, descriptor })
}

pub fn looks_like(data: &[u8]) -> bool {
    let Ok(h) = header(data) else {
        return false;
    };
    matches!(h.kind, 1 | 2 | 3 | 9 | 10 | 11)
        && h.map_type <= 1
        && matches!(h.bits, 8 | 15 | 16 | 24 | 32)
        && h.width > 0
        && h.height > 0
        && (h.map_type == 1) == matches!(h.kind, 1 | 9)
        && h.descriptor & 0xC0 == 0
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let h = header(data)?;
    check_size(h.width, h.height)?;
    Ok((h.width, h.height))
}

fn color(bytes: &[u8], bits: u8, alpha_bits: u8) -> u32 {
    match bits {
        8 => argb(255, bytes[0], bytes[0], bytes[0]),
        15 | 16 => {
            let v = u16::from_le_bytes([bytes[0], bytes[1]]);
            let c = |s: u16| (((v >> s) & 31) * 255 / 31) as u8;
            let a = if bits == 16 && alpha_bits > 0 && v & 0x8000 == 0 { 0 } else { 255 };
            argb(a, c(10), c(5), c(0))
        }
        24 => argb(255, bytes[2], bytes[1], bytes[0]),
        _ => argb(if alpha_bits > 0 { bytes[3] } else { 255 }, bytes[2], bytes[1], bytes[0]),
    }
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    let h = header(data)?;
    check_size(h.width, h.height)?;
    let mut r = Reader::at(data, 18 + h.id_len);
    let mut palette = alloc::vec::Vec::new();
    if h.map_type == 1 {
        let entry = (h.map_bits as usize).div_ceil(8);
        for _ in 0..h.map_len {
            palette.push(color(r.bytes(entry)?, h.map_bits, 1));
        }
    }
    let alpha_bits = h.descriptor & 15;
    let size = (h.bits as usize).div_ceil(8);
    let count = (h.width * h.height) as usize;
    let mut pixels = alloc::vec::Vec::with_capacity(count);
    let read_one = |r: &mut Reader| -> Result<u32> {
        let bytes = r.bytes(size)?;
        if matches!(h.kind, 1 | 9) {
            let index = if size == 1 { bytes[0] as usize } else { u16::from_le_bytes([bytes[0], bytes[1]]) as usize };
            Ok(*palette.get(index.wrapping_sub(h.map_first as usize)).ok_or(Error::Corrupt("tga: palette index"))?)
        } else {
            Ok(color(bytes, h.bits, alpha_bits))
        }
    };
    if h.kind >= 9 {
        while pixels.len() < count {
            let packet = r.u8()?;
            let n = (packet & 0x7F) as usize + 1;
            if packet & 0x80 != 0 {
                let px = read_one(&mut r)?;
                for _ in 0..n.min(count - pixels.len()) {
                    pixels.push(px);
                }
            } else {
                for _ in 0..n.min(count - pixels.len()) {
                    pixels.push(read_one(&mut r)?);
                }
            }
        }
    } else {
        for _ in 0..count {
            pixels.push(read_one(&mut r)?);
        }
    }
    let (w, ht) = (h.width as usize, h.height as usize);
    let mut out = Bitmap::new(h.width, h.height)?;
    let top_down = h.descriptor & 0x20 != 0;
    let right_left = h.descriptor & 0x10 != 0;
    for y in 0..ht {
        let sy = if top_down { y } else { ht - 1 - y };
        for x in 0..w {
            let sx = if right_left { w - 1 - x } else { x };
            out.pixels[y * w + x] = pixels[sy * w + sx];
        }
    }
    if alpha_bits > 0 && out.pixels.iter().all(|p| p >> 24 == 0) {
        for p in out.pixels.iter_mut() {
            *p |= 0xFF00_0000;
        }
    }
    Ok(out)
}
