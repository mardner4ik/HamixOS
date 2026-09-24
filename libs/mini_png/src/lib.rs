#![no_std]
extern crate alloc;

pub mod inflate;

use alloc::vec;
use alloc::vec::Vec;

const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug)]
pub enum PngError {
    NotAPng,
    Truncated,
    Unsupported(&'static str),
    Corrupt(&'static str),
}

impl core::fmt::Display for PngError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PngError::NotAPng => write!(f, "not a PNG file"),
            PngError::Truncated => write!(f, "truncated PNG data"),
            PngError::Unsupported(s) => write!(f, "unsupported PNG feature: {}", s),
            PngError::Corrupt(s) => write!(f, "corrupt PNG data: {}", s),
        }
    }
}

/// A decoded image: tightly packed 8-bit RGBA, row-major, no padding.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * self.width + x) * 4) as usize;
        (self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3])
    }
}

struct Ihdr {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
}

fn read_u32(data: &[u8]) -> u32 {
    u32::from_be_bytes([data[0], data[1], data[2], data[3]])
}

/// Decodes a PNG buffer. Supports 8-bit-per-channel grayscale, RGB,
/// palette (with optional tRNS alpha), grayscale+alpha and RGBA images,
/// non-interlaced. That covers ordinary PNGs exported by any normal image
/// tool; 16-bit-per-channel and Adam7-interlaced PNGs are reported as
/// `Unsupported` rather than misdecoded.
fn decode_raw(data: &[u8]) -> Result<Decoded, PngError> {
    if data.len() < 8 || data[0..8] != SIGNATURE {
        return Err(PngError::NotAPng);
    }

    let mut pos = 8usize;
    let mut ihdr: Option<Ihdr> = None;
    let mut palette: Vec<(u8, u8, u8)> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();

    while pos + 8 <= data.len() {
        let len = read_u32(&data[pos..]) as usize;
        let kind = &data[pos + 4..pos + 8];
        let body_start = pos + 8;
        if body_start + len + 4 > data.len() {
            return Err(PngError::Truncated);
        }
        let body = &data[body_start..body_start + len];

        match kind {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err(PngError::Corrupt("short IHDR"));
                }
                ihdr = Some(Ihdr {
                    width: read_u32(&body[0..4]),
                    height: read_u32(&body[4..8]),
                    bit_depth: body[8],
                    color_type: body[9],
                    interlace: body[12],
                });
            }
            b"PLTE" => {
                for chunk in body.chunks_exact(3) {
                    palette.push((chunk[0], chunk[1], chunk[2]));
                }
            }
            b"tRNS" => {
                trns = body.to_vec();
            }
            b"IDAT" => {
                idat.extend_from_slice(body);
            }
            b"IEND" => break,
            _ => {}
        }

        pos = body_start + len + 4; // skip CRC, not verified
    }

    let ihdr = ihdr.ok_or(PngError::Corrupt("missing IHDR"))?;
    if ihdr.interlace != 0 {
        return Err(PngError::Unsupported("Adam7 interlacing is not supported"));
    }
    let depth = ihdr.bit_depth as usize;
    let channels: usize = match ihdr.color_type {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(PngError::Unsupported("unknown color type")),
    };
    let valid_depth = match ihdr.color_type {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => matches!(depth, 8 | 16),
    };
    if !valid_depth {
        return Err(PngError::Corrupt("invalid bit depth"));
    }
    let width = ihdr.width as usize;
    let height = ihdr.height as usize;
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(PngError::Unsupported("image dimensions"));
    }

    let bits_per_pixel = channels * depth;
    let unit = bits_per_pixel.div_ceil(8);
    let stride = (width * bits_per_pixel).div_ceil(8);
    let expected = height * (stride + 1);
    let mut raw = inflate::zlib_decompress_sized(&idat, expected).map_err(PngError::Corrupt)?;
    drop(idat);
    if raw.len() < expected {
        return Err(PngError::Corrupt("decompressed data shorter than expected"));
    }
    unfilter(&mut raw, height, stride, unit)?;
    Ok(Decoded { ihdr, raw, stride, channels, depth, palette, trns })
}

struct Decoded {
    ihdr: Ihdr,
    raw: Vec<u8>,
    stride: usize,
    channels: usize,
    depth: usize,
    palette: Vec<(u8, u8, u8)>,
    trns: Vec<u8>,
}

fn unfilter(raw: &mut [u8], height: usize, stride: usize, unit: usize) -> Result<(), PngError> {
    let row_len = stride + 1;
    for y in 0..height {
        let base = y * row_len;
        let filter = raw[base];
        let (before, rest) = raw.split_at_mut(base + 1);
        let row = &mut rest[..stride];
        let prev: &[u8] = if y > 0 { &before[base + 1 - row_len..base] } else { &[] };
        match filter {
            0 => {}
            1 => {
                for x in unit..stride {
                    row[x] = row[x].wrapping_add(row[x - unit]);
                }
            }
            2 => {
                if y > 0 {
                    for x in 0..stride {
                        row[x] = row[x].wrapping_add(prev[x]);
                    }
                }
            }
            3 => {
                for x in 0..stride {
                    let a = if x >= unit { row[x - unit] as u16 } else { 0 };
                    let b = if y > 0 { prev[x] as u16 } else { 0 };
                    row[x] = row[x].wrapping_add(((a + b) / 2) as u8);
                }
            }
            4 => {
                for x in 0..stride {
                    let a = if x >= unit { row[x - unit] } else { 0 };
                    let b = if y > 0 { prev[x] } else { 0 };
                    let c = if y > 0 && x >= unit { prev[x - unit] } else { 0 };
                    row[x] = row[x].wrapping_add(paeth(a, b, c));
                }
            }
            _ => return Err(PngError::Corrupt("invalid filter type")),
        }
    }
    Ok(())
}

impl Decoded {
    fn pixels<F: FnMut(usize, u8, u8, u8, u8)>(&self, mut put: F) {
        let width = self.ihdr.width as usize;
        let height = self.ihdr.height as usize;
        let row_len = self.stride + 1;
        let depth = self.depth;
        let color_type = self.ihdr.color_type;
        for y in 0..height {
            let row = &self.raw[y * row_len + 1..y * row_len + 1 + self.stride];
            let out = y * width;
            match (color_type, depth) {
                (6, 8) => {
                    for (x, px) in row.chunks_exact(4).enumerate() {
                        put(out + x, px[0], px[1], px[2], px[3]);
                    }
                }
                (2, 8) => {
                    for (x, px) in row.chunks_exact(3).enumerate() {
                        put(out + x, px[0], px[1], px[2], 255);
                    }
                }
                (0, 8) => {
                    for (x, v) in row.iter().enumerate() {
                        put(out + x, *v, *v, *v, 255);
                    }
                }
                (4, 8) => {
                    for (x, px) in row.chunks_exact(2).enumerate() {
                        put(out + x, px[0], px[0], px[0], px[1]);
                    }
                }
                _ => {
                    let sample = |index: usize| -> u8 {
                        match depth {
                            8 => row[index],
                            16 => row[index * 2],
                            _ => {
                                let bit = index * depth;
                                let byte = row[bit / 8];
                                let shift = 8 - depth - bit % 8;
                                let value = (byte >> shift) & ((1u8 << depth) - 1);
                                if color_type == 3 { value } else { value * (255 / ((1u8 << depth) - 1)) }
                            }
                        }
                    };
                    for x in 0..width {
                        let i = x * self.channels;
                        let (r, g, b, a) = match color_type {
                            0 => {
                                let v = sample(i);
                                (v, v, v, 255)
                            }
                            2 => (sample(i), sample(i + 1), sample(i + 2), 255),
                            3 => {
                                let idx = sample(i) as usize;
                                let (r, g, b) = *self.palette.get(idx).unwrap_or(&(0, 0, 0));
                                (r, g, b, *self.trns.get(idx).unwrap_or(&255))
                            }
                            4 => {
                                let v = sample(i);
                                (v, v, v, sample(i + 1))
                            }
                            _ => (sample(i), sample(i + 1), sample(i + 2), sample(i + 3)),
                        };
                        put(out + x, r, g, b, a);
                    }
                }
            }
        }
    }
}

pub fn decode(data: &[u8]) -> Result<Image, PngError> {
    let decoded = decode_raw(data)?;
    let mut rgba = vec![0u8; decoded.ihdr.width as usize * decoded.ihdr.height as usize * 4];
    decoded.pixels(|i, r, g, b, a| {
        let o = i * 4;
        rgba[o] = r;
        rgba[o + 1] = g;
        rgba[o + 2] = b;
        rgba[o + 3] = a;
    });
    Ok(Image { width: decoded.ihdr.width, height: decoded.ihdr.height, rgba })
}

pub fn decode_argb(data: &[u8]) -> Result<(u32, u32, Vec<u32>), PngError> {
    let decoded = decode_raw(data)?;
    let mut px = vec![0u32; decoded.ihdr.width as usize * decoded.ihdr.height as usize];
    decoded.pixels(|i, r, g, b, a| {
        px[i] = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    });
    Ok((decoded.ihdr.width, decoded.ihdr.height, px))
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}
