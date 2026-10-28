use alloc::vec::Vec;

use crate::{bmp, png, Bitmap, Error, Reader, Result};

struct Entry {
    width: u32,
    height: u32,
    bits: u16,
    offset: usize,
    size: usize,
}

fn entries(data: &[u8]) -> Result<Vec<Entry>> {
    let mut r = Reader::new(data);
    r.skip(4)?;
    let count = r.u16le()? as usize;
    if count == 0 {
        return Err(Error::Corrupt("ico: no images"));
    }
    let mut list = Vec::new();
    for _ in 0..count {
        let w = r.u8()?;
        let h = r.u8()?;
        r.skip(4)?;
        let bits = r.u16le()?;
        let size = r.u32le()? as usize;
        let offset = r.u32le()? as usize;
        list.push(Entry { width: if w == 0 { 256 } else { w as u32 }, height: if h == 0 { 256 } else { h as u32 }, bits, offset, size });
    }
    Ok(list)
}

fn pick(list: &[Entry], wanted: u32) -> Option<&Entry> {
    let wanted = if wanted == 0 { u32::MAX } else { wanted };
    let score = |e: &Entry| {
        let size = e.width.max(e.height);
        let fit = if size >= wanted { (size - wanted) as i64 } else { (wanted - size) as i64 * 4 };
        (fit, -(e.bits as i64))
    };
    list.iter().min_by_key(|e| score(e))
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let list = entries(data)?;
    let e = pick(&list, 0).ok_or(Error::Corrupt("ico: no images"))?;
    Ok((e.width, e.height))
}

pub fn decode_best(data: &[u8], wanted: u32) -> Result<Bitmap> {
    let list = entries(data)?;
    let e = pick(&list, wanted).ok_or(Error::Corrupt("ico: no images"))?;
    let body = data.get(e.offset..e.offset.saturating_add(e.size)).ok_or(Error::Truncated)?;
    if body.starts_with(b"\x89PNG") {
        return png::decode(body);
    }
    let dib = bmp::dib_header(body, true)?;
    let at = dib.header_size;
    bmp::decode_dib(body, &dib, at, true)
}
