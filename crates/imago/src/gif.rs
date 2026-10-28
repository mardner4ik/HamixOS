use alloc::vec::Vec;

use crate::{argb, check_size, Bitmap, Error, Reader, Result};

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return Err(Error::Unknown);
    }
    let mut r = Reader::at(data, 6);
    let w = r.u16le()? as u32;
    let h = r.u16le()? as u32;
    check_size(w, h)?;
    Ok((w, h))
}

fn read_palette(r: &mut Reader, bits: u8) -> Result<Vec<u32>> {
    let n = 1usize << (bits + 1);
    let raw = r.bytes(n * 3)?;
    Ok(raw.chunks_exact(3).map(|c| argb(255, c[0], c[1], c[2])).collect())
}

fn sub_blocks(r: &mut Reader) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let len = r.u8()? as usize;
        if len == 0 {
            return Ok(out);
        }
        out.extend_from_slice(r.bytes(len)?);
    }
}

fn skip_blocks(r: &mut Reader) -> Result<()> {
    loop {
        let len = r.u8()? as usize;
        if len == 0 {
            return Ok(());
        }
        r.skip(len)?;
    }
}

fn lzw(data: &[u8], min_size: u8, expected: usize) -> Result<Vec<u8>> {
    if !(2..=11).contains(&min_size) {
        return Err(Error::Corrupt("gif: bad LZW code size"));
    }
    let clear = 1u16 << min_size;
    let end = clear + 1;
    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    let mut first = [0u8; 4096];
    for i in 0..clear {
        suffix[i as usize] = i as u8;
        first[i as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(expected);
    let mut size = min_size as u32 + 1;
    let mut next = end + 1;
    let mut prev: Option<u16> = None;
    let (mut buffer, mut count, mut pos) = (0u32, 0u32, 0usize);
    let mut stack = [0u8; 4096];
    loop {
        while count < size {
            let Some(&b) = data.get(pos) else {
                return Ok(out);
            };
            buffer |= (b as u32) << count;
            count += 8;
            pos += 1;
        }
        let code = (buffer & ((1 << size) - 1)) as u16;
        buffer >>= size;
        count -= size;
        if code == clear {
            size = min_size as u32 + 1;
            next = end + 1;
            prev = None;
            continue;
        }
        if code == end {
            return Ok(out);
        }
        let Some(p) = prev else {
            if code >= clear {
                return Err(Error::Corrupt("gif: bad first code"));
            }
            out.push(code as u8);
            prev = Some(code);
            continue;
        };
        let emit = if code < next { code } else if code == next { p } else { return Err(Error::Corrupt("gif: bad code")) };
        let mut c = emit;
        let mut n = 0;
        loop {
            stack[n] = suffix[c as usize];
            n += 1;
            if c < clear {
                break;
            }
            c = prefix[c as usize];
            if n >= 4096 {
                return Err(Error::Corrupt("gif: code chain"));
            }
        }
        for i in (0..n).rev() {
            out.push(stack[i]);
        }
        if code == next {
            out.push(first[p as usize]);
        }
        if next < 4096 {
            prefix[next as usize] = p;
            suffix[next as usize] = first[emit as usize];
            first[next as usize] = first[p as usize];
            next += 1;
            if next == 1 << size && size < 12 {
                size += 1;
            }
        }
        prev = Some(code);
        if out.len() >= expected {
            return Ok(out);
        }
    }
}

pub struct Frame {
    pub bitmap: Bitmap,
    pub delay_ms: u32,
}

pub struct Animation<'a> {
    r: Reader<'a>,
    width: u32,
    height: u32,
    global: Vec<u32>,
    background: u32,
    canvas: Vec<u32>,
    previous: Vec<u32>,
    pending_dispose: Option<(u8, u32, u32, u32, u32)>,
    pub loops: Option<u16>,
    done: bool,
}

impl<'a> Animation<'a> {
    pub fn new(data: &'a [u8]) -> Result<Animation<'a>> {
        let (width, height) = dimensions(data)?;
        let mut r = Reader::at(data, 10);
        let flags = r.u8()?;
        let bg_index = r.u8()?;
        r.skip(1)?;
        let global = if flags & 0x80 != 0 { read_palette(&mut r, flags & 7)? } else { Vec::new() };
        let _ = bg_index;
        let count = (width * height) as usize;
        Ok(Animation { r, width, height, global, background: 0, canvas: alloc::vec![0; count], previous: Vec::new(), pending_dispose: None, loops: None, done: false })
    }

    fn dispose(&mut self) {
        let Some((method, x, y, w, h)) = self.pending_dispose.take() else {
            return;
        };
        match method {
            2 => {
                for row in y..(y + h).min(self.height) {
                    for col in x..(x + w).min(self.width) {
                        self.canvas[(row * self.width + col) as usize] = self.background;
                    }
                }
            }
            3 if self.previous.len() == self.canvas.len() => self.canvas.copy_from_slice(&self.previous),
            _ => {}
        }
    }

    pub fn next_frame(&mut self) -> Option<Result<Frame>> {
        if self.done {
            return None;
        }
        let result = self.read_frame();
        match result {
            Ok(Some(frame)) => Some(Ok(frame)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }

    fn read_frame(&mut self) -> Result<Option<Frame>> {
        let mut transparent: Option<u8> = None;
        let mut delay = 0u32;
        let mut disposal = 0u8;
        loop {
            let Ok(kind) = self.r.u8() else {
                return Ok(None);
            };
            match kind {
                0x21 => {
                    let label = self.r.u8()?;
                    if label == 0xF9 {
                        let len = self.r.u8()?;
                        let block = self.r.bytes(len as usize)?;
                        if block.len() >= 4 {
                            disposal = (block[0] >> 2) & 7;
                            delay = u16::from_le_bytes([block[1], block[2]]) as u32 * 10;
                            if block[0] & 1 != 0 {
                                transparent = Some(block[3]);
                            }
                        }
                        skip_blocks(&mut self.r)?;
                    } else if label == 0xFF {
                        let app = sub_blocks(&mut self.r)?;
                        if app.starts_with(b"NETSCAPE2.0") && app.len() >= 14 {
                            self.loops = Some(u16::from_le_bytes([app[12], app[13]]));
                        }
                    } else {
                        skip_blocks(&mut self.r)?;
                    }
                }
                0x2C => {
                    self.dispose();
                    let x = self.r.u16le()? as u32;
                    let y = self.r.u16le()? as u32;
                    let w = self.r.u16le()? as u32;
                    let h = self.r.u16le()? as u32;
                    let flags = self.r.u8()?;
                    let local = if flags & 0x80 != 0 { Some(read_palette(&mut self.r, flags & 7)?) } else { None };
                    let interlaced = flags & 0x40 != 0;
                    let min_size = self.r.u8()?;
                    let data = sub_blocks(&mut self.r)?;
                    let indices = lzw(&data, min_size, (w * h) as usize)?;
                    if disposal == 3 {
                        self.previous = self.canvas.clone();
                    }
                    let palette = local.as_ref().unwrap_or(&self.global);
                    let rows: Vec<u32> = if interlaced {
                        let mut order = Vec::with_capacity(h as usize);
                        for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                            let mut r = start;
                            while r < h {
                                order.push(r);
                                r += step;
                            }
                        }
                        order
                    } else {
                        (0..h).collect()
                    };
                    for (i, &row) in rows.iter().enumerate() {
                        let cy = y + row;
                        if cy >= self.height {
                            continue;
                        }
                        for col in 0..w {
                            let cx = x + col;
                            if cx >= self.width {
                                continue;
                            }
                            let Some(&index) = indices.get(i * w as usize + col as usize) else {
                                continue;
                            };
                            if Some(index) == transparent {
                                continue;
                            }
                            if let Some(&c) = palette.get(index as usize) {
                                self.canvas[(cy * self.width + cx) as usize] = c;
                            }
                        }
                    }
                    self.pending_dispose = Some((disposal, x, y, w, h));
                    let bitmap = Bitmap { width: self.width, height: self.height, pixels: self.canvas.clone() };
                    return Ok(Some(Frame { bitmap, delay_ms: if delay < 20 { 100 } else { delay } }));
                }
                0x3B => return Ok(None),
                _ => return Err(Error::Corrupt("gif: unknown block")),
            }
        }
    }
}

pub fn decode(data: &[u8]) -> Result<Bitmap> {
    let mut animation = Animation::new(data)?;
    match animation.next_frame() {
        Some(frame) => frame.map(|f| f.bitmap),
        None => Err(Error::Corrupt("gif: no frames")),
    }
}

pub fn frame_count(data: &[u8]) -> Result<usize> {
    let mut animation = Animation::new(data)?;
    let mut n = 0;
    while let Some(frame) = animation.next_frame() {
        frame?;
        n += 1;
    }
    Ok(n)
}
