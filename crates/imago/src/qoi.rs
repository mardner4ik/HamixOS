use crate::{argb, check_size, Bitmap, Error, Reader, Result};

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    if !data.starts_with(b"qoif") {
        return Err(Error::Unknown);
    }
    let mut r = Reader::at(data, 4);
    let w = r.u32be()?;
    let h = r.u32be()?;
    check_size(w, h)?;
    Ok((w, h))
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    let (w, h) = dimensions(data)?;
    let mut out = Bitmap::new(w, h)?;
    let mut index = [0u32; 64];
    let mut px = 0xFF00_0000u32;
    let mut pos = 14;
    let mut run = 0u32;
    let hash = |p: u32| {
        let (a, r, g, b) = (p >> 24, (p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
        ((r * 3 + g * 5 + b * 7 + a * 11) % 64) as usize
    };
    for slot in out.pixels.iter_mut() {
        if run > 0 {
            run -= 1;
            *slot = px;
            continue;
        }
        let b0 = *data.get(pos).ok_or(Error::Truncated)?;
        pos += 1;
        let (a, mut r, mut g, mut b) = ((px >> 24) as u8, (px >> 16) as u8, (px >> 8) as u8, px as u8);
        match b0 {
            0xFE => {
                let c = data.get(pos..pos + 3).ok_or(Error::Truncated)?;
                px = argb(a, c[0], c[1], c[2]);
                pos += 3;
            }
            0xFF => {
                let c = data.get(pos..pos + 4).ok_or(Error::Truncated)?;
                px = argb(c[3], c[0], c[1], c[2]);
                pos += 4;
            }
            _ => match b0 >> 6 {
                0 => px = index[(b0 & 63) as usize],
                1 => {
                    r = r.wrapping_add(((b0 >> 4) & 3).wrapping_sub(2));
                    g = g.wrapping_add(((b0 >> 2) & 3).wrapping_sub(2));
                    b = b.wrapping_add((b0 & 3).wrapping_sub(2));
                    px = argb(a, r, g, b);
                }
                2 => {
                    let b1 = *data.get(pos).ok_or(Error::Truncated)?;
                    pos += 1;
                    let dg = (b0 & 63).wrapping_sub(32);
                    g = g.wrapping_add(dg);
                    r = r.wrapping_add(dg.wrapping_add((b1 >> 4).wrapping_sub(8)));
                    b = b.wrapping_add(dg.wrapping_add((b1 & 15).wrapping_sub(8)));
                    px = argb(a, r, g, b);
                }
                _ => run = (b0 & 63) as u32,
            },
        }
        index[hash(px)] = px;
        *slot = px;
    }
    Ok(out)
}
