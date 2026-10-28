#![no_std]

extern crate alloc;

pub mod bmp;
pub mod gif;
pub mod ico;
pub mod inflate;
pub mod jpeg;
pub mod png;
pub mod pnm;
pub mod qoi;
pub mod svg;
pub mod tga;

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Ico,
    Qoi,
    Tga,
    Pnm,
    Svg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unknown,
    Truncated,
    Corrupt(&'static str),
    Unsupported(&'static str),
    TooLarge,
}

impl From<&'static str> for Error {
    fn from(message: &'static str) -> Error {
        Error::Corrupt(message)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub const MAX_PIXELS: u64 = 1 << 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Bitmap {
    pub fn new(width: u32, height: u32) -> Result<Bitmap> {
        check_size(width, height)?;
        Ok(Bitmap { width, height, pixels: alloc::vec![0; width as usize * height as usize] })
    }

    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn opaque(&self) -> bool {
        self.pixels.iter().all(|p| p >> 24 == 255)
    }

    pub fn premultiplied(mut self) -> Bitmap {
        for p in self.pixels.iter_mut() {
            let a = *p >> 24;
            if a == 255 {
                continue;
            }
            if a == 0 {
                *p = 0;
                continue;
            }
            let scale = |c: u32| (c * a + 127) / 255;
            *p = a << 24 | scale((*p >> 16) & 0xFF) << 16 | scale((*p >> 8) & 0xFF) << 8 | scale(*p & 0xFF);
        }
        self
    }

    pub fn resized(&self, width: u32, height: u32) -> Bitmap {
        let mut out = Bitmap { width, height, pixels: alloc::vec![0; width as usize * height as usize] };
        if self.width == 0 || self.height == 0 || width == 0 || height == 0 {
            return out;
        }
        if width <= self.width && height <= self.height {
            box_filter(self, &mut out);
        } else {
            bilinear(self, &mut out);
        }
        out
    }
}

fn box_filter(src: &Bitmap, out: &mut Bitmap) {
    let (sw, sh, dw, dh) = (src.width as u64, src.height as u64, out.width as u64, out.height as u64);
    for dy in 0..dh {
        let y0 = dy * sh / dh;
        let y1 = ((dy + 1) * sh / dh).max(y0 + 1);
        for dx in 0..dw {
            let x0 = dx * sw / dw;
            let x1 = ((dx + 1) * sw / dw).max(x0 + 1);
            let (mut a, mut r, mut g, mut b) = (0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                let row = &src.pixels[(y * sw) as usize..((y + 1) * sw) as usize];
                for &p in &row[x0 as usize..x1 as usize] {
                    let pa = (p >> 24) as u64;
                    a += pa;
                    r += ((p >> 16) & 0xFF) as u64 * pa;
                    g += ((p >> 8) & 0xFF) as u64 * pa;
                    b += (p & 0xFF) as u64 * pa;
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            out.pixels[(dy * dw + dx) as usize] = if a == 0 {
                0
            } else {
                ((a / n) as u32) << 24 | ((r / a) as u32) << 16 | ((g / a) as u32) << 8 | (b / a) as u32
            };
        }
    }
}

fn bilinear(src: &Bitmap, out: &mut Bitmap) {
    let (sw, sh) = (src.width as i64, src.height as i64);
    let step_x = (sw << 16) / out.width as i64;
    let step_y = (sh << 16) / out.height as i64;
    for dy in 0..out.height as i64 {
        let fy = (dy * step_y + step_y / 2 - 32768).max(0);
        let y0 = (fy >> 16).min(sh - 1);
        let y1 = (y0 + 1).min(sh - 1);
        let ty = ((fy >> 8) & 0xFF) as u32;
        for dx in 0..out.width as i64 {
            let fx = (dx * step_x + step_x / 2 - 32768).max(0);
            let x0 = (fx >> 16).min(sw - 1);
            let x1 = (x0 + 1).min(sw - 1);
            let tx = ((fx >> 8) & 0xFF) as u32;
            let p = |x: i64, y: i64| src.pixels[(y * sw + x) as usize];
            let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
            let mut v = 0u32;
            for shift in [0u32, 8, 16, 24] {
                let ch = |q: u32| (q >> shift) & 0xFF;
                let top = ch(a) * (256 - tx) + ch(b) * tx;
                let bottom = ch(c) * (256 - tx) + ch(d) * tx;
                v |= (((top * (256 - ty) + bottom * ty) >> 16) & 0xFF) << shift;
            }
            out.pixels[(dy * out.width as i64 + dx) as usize] = v;
        }
    }
}

pub fn check_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::Corrupt("image has no pixels"));
    }
    if width as u64 * height as u64 > MAX_PIXELS || width > 65535 || height > 65535 {
        return Err(Error::TooLarge);
    }
    Ok(())
}

pub fn detect(data: &[u8]) -> Option<Format> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some(Format::Gif);
    }
    if data.starts_with(b"BM") && data.len() > 26 {
        return Some(Format::Bmp);
    }
    if data.starts_with(b"qoif") {
        return Some(Format::Qoi);
    }
    if data.len() > 6 && data[0] == 0 && data[1] == 0 && (data[2] == 1 || data[2] == 2) && data[3] == 0 && (data[4] != 0 || data[5] != 0) {
        return Some(Format::Ico);
    }
    if data.len() > 2 && data[0] == b'P' && (b'1'..=b'7').contains(&data[1]) && data[2].is_ascii_whitespace() {
        return Some(Format::Pnm);
    }
    if svg::looks_like(data) {
        return Some(Format::Svg);
    }
    if tga::looks_like(data) {
        return Some(Format::Tga);
    }
    None
}

pub fn info(data: &[u8]) -> Result<(Format, u32, u32)> {
    let format = detect(data).ok_or(Error::Unknown)?;
    let (w, h) = match format {
        Format::Png => png::dimensions(data)?,
        Format::Jpeg => jpeg::dimensions(data)?,
        Format::Gif => gif::dimensions(data)?,
        Format::Bmp => bmp::dimensions(data)?,
        Format::Ico => ico::dimensions(data)?,
        Format::Qoi => qoi::dimensions(data)?,
        Format::Tga => tga::dimensions(data)?,
        Format::Pnm => pnm::dimensions(data)?,
        Format::Svg => svg::dimensions(data)?,
    };
    Ok((format, w, h))
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    decode_sized(data, 0, 0)
}

pub fn decode_sized(data: &[u8], width: u32, height: u32) -> Result<Bitmap> {
    match detect(data).ok_or(Error::Unknown)? {
        Format::Png => png::decode(data),
        Format::Jpeg => jpeg::decode_scaled(data, width, height),
        Format::Gif => gif::decode(data),
        Format::Bmp => bmp::decode(data),
        Format::Ico => ico::decode_best(data, width.max(height)),
        Format::Qoi => qoi::decode(data),
        Format::Tga => tga::decode(data),
        Format::Pnm => pnm::decode(data),
        Format::Svg => svg::render(data, width, height),
    }
}

pub fn extension_format(name: &str) -> Option<Format> {
    let ext = name.rsplit('.').next()?;
    let mut lower = [0u8; 8];
    if ext.len() > lower.len() {
        return None;
    }
    for (i, b) in ext.bytes().enumerate() {
        lower[i] = b.to_ascii_lowercase();
    }
    Some(match &lower[..ext.len()] {
        b"png" => Format::Png,
        b"jpg" | b"jpeg" | b"jpe" | b"jfif" => Format::Jpeg,
        b"gif" => Format::Gif,
        b"bmp" | b"dib" => Format::Bmp,
        b"ico" | b"cur" => Format::Ico,
        b"qoi" => Format::Qoi,
        b"tga" | b"icb" | b"vda" => Format::Tga,
        b"pbm" | b"pgm" | b"ppm" | b"pnm" | b"pam" => Format::Pnm,
        b"svg" => Format::Svg,
        _ => return None,
    })
}

#[inline(always)]
pub fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> Reader<'a> {
        Reader { data, pos }
    }

    pub fn u8(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or(Error::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    pub fn u16le(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }

    pub fn u32le(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn u32be(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(Error::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
}
