use crate::{argb, check_size, Bitmap, Error, Result};

struct Header {
    kind: u8,
    width: u32,
    height: u32,
    maxval: u32,
    depth: u32,
    data: usize,
}

fn token(data: &[u8], pos: &mut usize) -> Result<u32> {
    loop {
        match data.get(*pos) {
            Some(b'#') => {
                while data.get(*pos).map(|b| *b != b'\n').unwrap_or(false) {
                    *pos += 1;
                }
            }
            Some(b) if b.is_ascii_whitespace() => *pos += 1,
            Some(_) => break,
            None => return Err(Error::Truncated),
        }
    }
    let mut v: u32 = 0;
    let mut any = false;
    while let Some(b) = data.get(*pos).filter(|b| b.is_ascii_digit()) {
        v = v.checked_mul(10).and_then(|v| v.checked_add((b - b'0') as u32)).ok_or(Error::Corrupt("pnm: number too large"))?;
        *pos += 1;
        any = true;
    }
    if !any {
        return Err(Error::Corrupt("pnm: expected a number"));
    }
    Ok(v)
}

fn word<'a>(data: &'a [u8], pos: &mut usize) -> &'a [u8] {
    while data.get(*pos).map(|b| b.is_ascii_whitespace()).unwrap_or(false) {
        *pos += 1;
    }
    let start = *pos;
    while data.get(*pos).map(|b| !b.is_ascii_whitespace()).unwrap_or(false) {
        *pos += 1;
    }
    &data[start..*pos]
}

fn header(data: &[u8]) -> Result<Header> {
    if data.len() < 3 || data[0] != b'P' {
        return Err(Error::Unknown);
    }
    let kind = data[1] - b'0';
    let mut pos = 2;
    if kind == 7 {
        let (mut width, mut height, mut depth, mut maxval) = (0, 0, 0, 0);
        loop {
            let key = word(data, &mut pos);
            match key {
                b"WIDTH" => width = token(data, &mut pos)?,
                b"HEIGHT" => height = token(data, &mut pos)?,
                b"DEPTH" => depth = token(data, &mut pos)?,
                b"MAXVAL" => maxval = token(data, &mut pos)?,
                b"TUPLTYPE" => {
                    word(data, &mut pos);
                }
                b"ENDHDR" => break,
                b"" => return Err(Error::Truncated),
                _ => {}
            }
        }
        while data.get(pos).map(|b| *b != b'\n').unwrap_or(false) {
            pos += 1;
        }
        check_size(width, height)?;
        if !(1..=4).contains(&depth) || maxval == 0 || maxval > 65535 {
            return Err(Error::Unsupported("pam: depth or maxval"));
        }
        return Ok(Header { kind, width, height, maxval, depth, data: pos + 1 });
    }
    let width = token(data, &mut pos)?;
    let height = token(data, &mut pos)?;
    let maxval = if kind == 1 || kind == 4 { 1 } else { token(data, &mut pos)? };
    check_size(width, height)?;
    if maxval == 0 || maxval > 65535 {
        return Err(Error::Corrupt("pnm: bad maxval"));
    }
    let depth = match kind {
        3 | 6 => 3,
        _ => 1,
    };
    Ok(Header { kind, width, height, maxval, depth, data: pos + 1 })
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let h = header(data)?;
    Ok((h.width, h.height))
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    let h = header(data)?;
    let mut out = Bitmap::new(h.width, h.height)?;
    let count = (h.width * h.height) as usize;
    let scale = |v: u32| ((v.min(h.maxval) * 255 + h.maxval / 2) / h.maxval) as u8;
    let ascii = h.kind <= 3;
    let mut pos = h.data;
    if h.kind == 4 {
        let stride = (h.width as usize).div_ceil(8);
        let bits = data.get(pos..pos + stride * h.height as usize).ok_or(Error::Truncated)?;
        for y in 0..h.height as usize {
            for x in 0..h.width as usize {
                let on = bits[y * stride + x / 8] >> (7 - x % 8) & 1 == 1;
                out.pixels[y * h.width as usize + x] = if on { 0xFF00_0000 } else { 0xFFFF_FFFF };
            }
        }
        return Ok(out);
    }
    let wide = h.maxval > 255;
    let next = |pos: &mut usize| -> Result<u32> {
        if ascii {
            if h.kind == 1 {
                loop {
                    match data.get(*pos) {
                        Some(b'0') => {
                            *pos += 1;
                            return Ok(1);
                        }
                        Some(b'1') => {
                            *pos += 1;
                            return Ok(0);
                        }
                        Some(_) => *pos += 1,
                        None => return Err(Error::Truncated),
                    }
                }
            }
            return token(data, pos);
        }
        if wide {
            let v = data.get(*pos..*pos + 2).ok_or(Error::Truncated)?;
            *pos += 2;
            Ok(u16::from_be_bytes([v[0], v[1]]) as u32)
        } else {
            let v = *data.get(*pos).ok_or(Error::Truncated)?;
            *pos += 1;
            Ok(v as u32)
        }
    };
    for i in 0..count {
        let px = match h.depth {
            1 => {
                let g = scale(next(&mut pos)?);
                argb(255, g, g, g)
            }
            2 => {
                let g = scale(next(&mut pos)?);
                argb(scale(next(&mut pos)?), g, g, g)
            }
            3 => argb(255, scale(next(&mut pos)?), scale(next(&mut pos)?), scale(next(&mut pos)?)),
            _ => {
                let (r, g, b) = (scale(next(&mut pos)?), scale(next(&mut pos)?), scale(next(&mut pos)?));
                argb(scale(next(&mut pos)?), r, g, b)
            }
        };
        out.pixels[i] = px;
    }
    Ok(out)
}
