#![no_std]
extern crate alloc;

mod inflate;

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
pub fn decode(data: &[u8]) -> Result<Image, PngError> {
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
    if ihdr.bit_depth != 8 {
        return Err(PngError::Unsupported("only 8-bit channels are supported"));
    }
    if ihdr.interlace != 0 {
        return Err(PngError::Unsupported("Adam7 interlacing is not supported"));
    }

    let channels: usize = match ihdr.color_type {
        0 => 1, // grayscale
        2 => 3, // RGB
        3 => 1, // palette index
        4 => 2, // grayscale + alpha
        6 => 4, // RGBA
        _ => return Err(PngError::Unsupported("unknown color type")),
    };

    let raw = inflate::zlib_decompress(&idat).map_err(PngError::Corrupt)?;

    let width = ihdr.width as usize;
    let height = ihdr.height as usize;
    let stride = width * channels;
    let expected = height * (stride + 1);
    if raw.len() < expected {
        return Err(PngError::Corrupt("decompressed data shorter than expected"));
    }

    // Undo the per-scanline filters (PNG spec section 6).
    let mut unfiltered = vec![0u8; height * stride];
    let mut src = 0usize;
    for y in 0..height {
        let filter = raw[src];
        src += 1;
        let row_start = y * stride;
        for x in 0..stride {
            let raw_byte = raw[src + x];
            let a = if x >= channels { unfiltered[row_start + x - channels] } else { 0 };
            let b = if y > 0 { unfiltered[row_start - stride + x] } else { 0 };
            let c = if y > 0 && x >= channels { unfiltered[row_start - stride + x - channels] } else { 0 };
            let recon = match filter {
                0 => raw_byte,
                1 => raw_byte.wrapping_add(a),
                2 => raw_byte.wrapping_add(b),
                3 => raw_byte.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => raw_byte.wrapping_add(paeth(a, b, c)),
                _ => return Err(PngError::Corrupt("invalid filter type")),
            };
            unfiltered[row_start + x] = recon;
        }
        src += stride;
    }

    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let si = y * stride + x * channels;
            let di = (y * width + x) * 4;
            let (r, g, b, a) = match ihdr.color_type {
                0 => (unfiltered[si], unfiltered[si], unfiltered[si], 255),
                2 => (unfiltered[si], unfiltered[si + 1], unfiltered[si + 2], 255),
                3 => {
                    let idx = unfiltered[si] as usize;
                    let (r, g, b) = *palette.get(idx).unwrap_or(&(0, 0, 0));
                    let a = *trns.get(idx).unwrap_or(&255);
                    (r, g, b, a)
                }
                4 => (unfiltered[si], unfiltered[si], unfiltered[si], unfiltered[si + 1]),
                6 => (unfiltered[si], unfiltered[si + 1], unfiltered[si + 2], unfiltered[si + 3]),
                _ => unreachable!(),
            };
            rgba[di] = r;
            rgba[di + 1] = g;
            rgba[di + 2] = b;
            rgba[di + 3] = a;
        }
    }

    Ok(Image { width: ihdr.width, height: ihdr.height, rgba })
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
