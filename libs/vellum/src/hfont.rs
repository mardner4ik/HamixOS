use alloc::vec::Vec;

use crate::gfx::{blend, Painter};

#[derive(Clone, Copy, Default)]
pub struct Glyph {
    pub codepoint: u32,
    pub x: u16,
    pub y: u16,
    pub w: u8,
    pub h: u8,
    pub bearing_x: i8,
    pub bearing_y: i8,
    pub advance: u16,
}

pub struct Font {
    pub size: i32,
    pub ascent: i32,
    pub descent: i32,
    pub line_height: i32,
    glyphs: Vec<Glyph>,
    ascii: [u16; 128],
    atlas: Vec<u8>,
    atlas_w: usize,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Font {
    pub fn parse(bytes: &[u8]) -> Option<Font> {
        if bytes.len() < 24 || &bytes[0..4] != b"HFNT" {
            return None;
        }
        let size = u16_at(bytes, 6) as i32;
        let ascent = u16_at(bytes, 8) as i16 as i32;
        let descent = u16_at(bytes, 10) as i16 as i32;
        let line_height = u16_at(bytes, 12) as i32;
        let count = u32_at(bytes, 14) as usize;
        let atlas_w = u16_at(bytes, 18) as usize;
        let atlas_h = u16_at(bytes, 20) as usize;
        let records = 22;
        let atlas_start = records + count * 16;
        if bytes.len() < atlas_start + atlas_w * atlas_h {
            return None;
        }
        let mut glyphs = Vec::with_capacity(count);
        for i in 0..count {
            let r = &bytes[records + i * 16..records + i * 16 + 16];
            glyphs.push(Glyph {
                codepoint: u32_at(r, 0),
                x: u16_at(r, 4),
                y: u16_at(r, 6),
                w: r[8],
                h: r[9],
                bearing_x: r[10] as i8,
                bearing_y: r[11] as i8,
                advance: u16_at(r, 12),
            });
        }
        glyphs.sort_by_key(|g| g.codepoint);
        let mut ascii = [u16::MAX; 128];
        for (i, g) in glyphs.iter().enumerate() {
            if g.codepoint < 128 {
                ascii[g.codepoint as usize] = i as u16;
            }
        }
        Some(Font {
            size,
            ascent,
            descent,
            line_height,
            glyphs,
            ascii,
            atlas: bytes[atlas_start..atlas_start + atlas_w * atlas_h].to_vec(),
            atlas_w,
        })
    }

    pub fn glyph(&self, ch: char) -> Option<&Glyph> {
        let cp = ch as u32;
        if cp < 128 {
            let i = self.ascii[cp as usize];
            if i != u16::MAX {
                return self.glyphs.get(i as usize);
            }
        }
        match self.glyphs.binary_search_by_key(&cp, |g| g.codepoint) {
            Ok(i) => self.glyphs.get(i),
            Err(_) => {
                if ch == '\t' {
                    return self.glyph(' ');
                }
                if cp < 128 { None } else { self.glyph('?') }
            }
        }
    }

    pub fn advance(&self, ch: char) -> i32 {
        if ch == '\t' {
            return self.advance(' ') * 4;
        }
        self.glyph(ch).map(|g| g.advance as i32).unwrap_or(0)
    }

    pub fn measure(&self, text: &str) -> i32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    pub fn height(&self) -> i32 {
        self.ascent + self.descent
    }

    pub fn draw(&self, p: &mut Painter, x: i32, y: i32, text: &str, color: u32, alpha: u32) -> i32 {
        let baseline = y + self.ascent;
        let mut pen = x;
        let clip = p.clip;
        for ch in text.chars() {
            if ch == '\t' {
                pen += self.advance(' ') * 4;
                continue;
            }
            let Some(g) = self.glyph(ch) else {
                continue;
            };
            let gx = pen + g.bearing_x as i32;
            let gy = baseline - g.bearing_y as i32;
            pen += g.advance as i32;
            if g.w == 0 || gx >= clip.right() || gy >= clip.bottom() || gx + g.w as i32 <= clip.x || gy + g.h as i32 <= clip.y {
                continue;
            }
            for row in 0..g.h as i32 {
                let ty = gy + row;
                if ty < clip.y || ty >= clip.bottom() {
                    continue;
                }
                let src_row = (g.y as usize + row as usize) * self.atlas_w + g.x as usize;
                let dst_row = (ty * p.w) as usize;
                for col in 0..g.w as i32 {
                    let tx = gx + col;
                    if tx < clip.x || tx >= clip.right() {
                        continue;
                    }
                    let cov = self.atlas[src_row + col as usize] as u32;
                    if cov == 0 {
                        continue;
                    }
                    let a = cov * alpha / 255;
                    let i = dst_row + tx as usize;
                    p.buf[i] = blend(p.buf[i], color, a);
                }
            }
        }
        pen - x
    }
}
