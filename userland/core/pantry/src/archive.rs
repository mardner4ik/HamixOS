use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Member<'a> {
    pub raw: &'a [u8],
    pub data: Vec<u8>,
}

fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    table
}

fn crc_update(table: &[u32; 256], crc: u32, data: &[u8]) -> u32 {
    let mut crc = crc;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

fn crc32(data: &[u8]) -> u32 {
    !crc_update(&crc_table(), !0u32, data)
}

fn deflate_start(rest: &[u8]) -> Result<usize, &'static str> {
    if rest.len() < 18 || rest[0] != 0x1f || rest[1] != 0x8b || rest[2] != 8 {
        return Err("not a gzip stream");
    }
    let flags = rest[3];
    let mut off = 10usize;
    if flags & 4 != 0 {
        let xlen = u16::from_le_bytes([*rest.get(off).ok_or("truncated gzip")?, *rest.get(off + 1).ok_or("truncated gzip")?]) as usize;
        off += 2 + xlen;
    }
    for flag in [8u8, 16] {
        if flags & flag != 0 {
            let end = rest.get(off..).ok_or("truncated gzip")?.iter().position(|b| *b == 0).ok_or("truncated gzip")?;
            off += end + 1;
        }
    }
    if flags & 2 != 0 {
        off += 2;
    }
    Ok(off)
}

pub fn gzip_stream(rest: &[u8], sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<usize, &'static str> {
    let off = deflate_start(rest)?;
    let deflate = rest.get(off..).ok_or("truncated gzip")?;
    let table = crc_table();
    let mut crc = !0u32;
    let mut forward = |chunk: &[u8]| {
        crc = crc_update(&table, crc, chunk);
        sink(chunk)
    };
    let (used, total) = mini_png::inflate::inflate_stream(deflate, &mut forward)?;
    let trailer = off + used;
    let footer = rest.get(trailer..trailer + 8).ok_or("truncated gzip trailer")?;
    let stored_crc = u32::from_le_bytes(footer[0..4].try_into().unwrap());
    let size = u32::from_le_bytes(footer[4..8].try_into().unwrap());
    if stored_crc != !crc || size != total as u32 {
        return Err("gzip checksum mismatch");
    }
    Ok(trailer + 8)
}

pub fn gzip_raw_members(bytes: &[u8], limit: usize) -> Result<Vec<&[u8]>, &'static str> {
    let mut members = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() && members.len() < limit {
        let rest = &bytes[pos..];
        let end = if members.len() + 1 == limit { rest.len() } else { gzip_stream(rest, &mut |_| true)? };
        members.push(&rest[..end]);
        pos += end;
    }
    Ok(members)
}

pub fn gzip_members(bytes: &[u8], limit: usize) -> Result<Vec<Member<'_>>, &'static str> {
    let mut members = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() && members.len() < limit {
        let rest = &bytes[pos..];
        if rest.len() < 18 || rest[0] != 0x1f || rest[1] != 0x8b || rest[2] != 8 {
            return Err("not a gzip stream");
        }
        let flags = rest[3];
        let mut off = 10usize;
        if flags & 4 != 0 {
            let xlen = u16::from_le_bytes([*rest.get(off).ok_or("truncated gzip")?, *rest.get(off + 1).ok_or("truncated gzip")?]) as usize;
            off += 2 + xlen;
        }
        for flag in [8u8, 16] {
            if flags & flag != 0 {
                let end = rest.get(off..).ok_or("truncated gzip")?.iter().position(|b| *b == 0).ok_or("truncated gzip")?;
                off += end + 1;
            }
        }
        if flags & 2 != 0 {
            off += 2;
        }
        let deflate = rest.get(off..).ok_or("truncated gzip")?;
        let (data, used) = mini_png::inflate::inflate_counted(deflate, 0)?;
        let trailer = off + used;
        let footer = rest.get(trailer..trailer + 8).ok_or("truncated gzip trailer")?;
        let crc = u32::from_le_bytes(footer[0..4].try_into().unwrap());
        let size = u32::from_le_bytes(footer[4..8].try_into().unwrap());
        if crc != crc32(&data) || size != data.len() as u32 {
            return Err("gzip checksum mismatch");
        }
        let end = trailer + 8;
        members.push(Member { raw: &rest[..end], data });
        pos += end;
    }
    Ok(members)
}

#[derive(Clone, PartialEq)]
pub enum Kind {
    File,
    Dir,
    Symlink(String),
    Hardlink(String),
    Other,
}

pub struct Entry<'a> {
    pub path: String,
    pub data: &'a [u8],
}

fn octal(field: &[u8]) -> u64 {
    if field.first().map(|b| b & 0x80 != 0).unwrap_or(false) {
        return field[1..].iter().fold(0u64, |acc, b| (acc << 8) | *b as u64);
    }
    field.iter().take_while(|b| **b != 0).filter(|b| (b'0'..=b'7').contains(b)).fold(0u64, |acc, b| acc * 8 + (b - b'0') as u64)
}

fn text(field: &[u8]) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

fn parse_pax(body: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut pos = 0usize;
    while pos < body.len() {
        let Some(space) = body[pos..].iter().position(|b| *b == b' ') else {
            break;
        };
        let len: usize = core::str::from_utf8(&body[pos..pos + space]).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        if len == 0 || pos + len > body.len() {
            break;
        }
        let record = &body[pos + space + 1..pos + len];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(eq) = record.iter().position(|b| *b == b'=') {
            map.insert(String::from_utf8_lossy(&record[..eq]).into_owned(), String::from_utf8_lossy(&record[eq + 1..]).into_owned());
        }
        pos += len;
    }
    map
}

fn clean(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

pub fn tar_entries(tar: &[u8]) -> Result<Vec<Entry<'_>>, &'static str> {
    let mut entries = Vec::new();
    let mut pos = 0usize;
    let mut pending_pax: BTreeMap<String, String> = BTreeMap::new();
    let mut long_name: Option<String> = None;
    let mut long_link: Option<String> = None;
    while pos + 512 <= tar.len() {
        let header = &tar[pos..pos + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let stored: u64 = octal(&header[148..156]);
        let sum: u64 = header.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { *b as u64 }).sum();
        if stored != sum {
            return Err("tar header checksum mismatch");
        }
        let size = octal(&header[124..136]) as usize;
        let body_start = pos + 512;
        let body = tar.get(body_start..body_start + size).ok_or("tar entry truncated")?;
        pos = body_start + size.div_ceil(512) * 512;
        let flag = header[156];
        match flag {
            b'x' => {
                pending_pax = parse_pax(body);
                continue;
            }
            b'g' => continue,
            b'L' => {
                long_name = Some(text(body));
                continue;
            }
            b'K' => {
                long_link = Some(text(body));
                continue;
            }
            _ => {}
        }
        let mut name = text(&header[0..100]);
        if &header[257..262] == b"ustar" {
            let prefix = text(&header[345..500]);
            if !prefix.is_empty() {
                name = alloc::format!("{}/{}", prefix, name);
            }
        }
        let pax = core::mem::take(&mut pending_pax);
        if let Some(p) = pax.get("path") {
            name = p.clone();
        }
        if let Some(l) = long_name.take() {
            name = l;
        }
        let mut link = text(&header[157..257]);
        if let Some(l) = pax.get("linkpath") {
            link = l.clone();
        }
        if let Some(l) = long_link.take() {
            link = l;
        }
        let kind = match flag {
            b'0' | 0 | b'7' => Kind::File,
            b'5' => Kind::Dir,
            b'2' => Kind::Symlink(link),
            b'1' => Kind::Hardlink(clean(&link)),
            _ => Kind::Other,
        };
        let path = clean(&name);
        if path.is_empty() && kind != Kind::Dir {
            continue;
        }
        entries.push(Entry { path, data: body });
    }
    Ok(entries)
}

pub struct StreamEntry {
    pub path: String,
    pub kind: Kind,
    pub mode: u32,
}

pub enum TarEvent<'a> {
    Begin(&'a StreamEntry),
    Data(&'a [u8]),
    End,
}

enum TarState {
    Header,
    Meta { flag: u8, size: usize, body: Vec<u8> },
    Body { remaining: usize, padding: usize },
    Padding(usize),
    Done,
}

pub struct TarStream {
    state: TarState,
    header: Vec<u8>,
    pax: BTreeMap<String, String>,
    long_name: Option<String>,
    long_link: Option<String>,
    current: Option<StreamEntry>,
    pub error: Option<&'static str>,
}

impl TarStream {
    pub fn new() -> TarStream {
        TarStream { state: TarState::Header, header: Vec::with_capacity(512), pax: BTreeMap::new(), long_name: None, long_link: None, current: None, error: None }
    }

    pub fn feed(&mut self, mut data: &[u8], on: &mut dyn FnMut(TarEvent) -> bool) -> bool {
        while !data.is_empty() {
            match &mut self.state {
                TarState::Done => return true,
                TarState::Padding(left) => {
                    let n = (*left).min(data.len());
                    *left -= n;
                    data = &data[n..];
                    if *left == 0 {
                        self.state = TarState::Header;
                    }
                }
                TarState::Body { remaining, padding } => {
                    let n = (*remaining).min(data.len());
                    if n > 0 && !on(TarEvent::Data(&data[..n])) {
                        return false;
                    }
                    *remaining -= n;
                    data = &data[n..];
                    if *remaining == 0 {
                        let pad = *padding;
                        if !on(TarEvent::End) {
                            return false;
                        }
                        self.current = None;
                        self.state = if pad > 0 { TarState::Padding(pad) } else { TarState::Header };
                    }
                }
                TarState::Meta { flag, size, body } => {
                    let n = (*size - body.len()).min(data.len());
                    body.extend_from_slice(&data[..n]);
                    data = &data[n..];
                    if body.len() == *size {
                        let flag = *flag;
                        let body = core::mem::take(body);
                        let pad = body.len().div_ceil(512) * 512 - body.len();
                        match flag {
                            b'x' => self.pax = parse_pax(&body),
                            b'L' => self.long_name = Some(text(&body)),
                            b'K' => self.long_link = Some(text(&body)),
                            _ => {}
                        }
                        self.state = if pad > 0 { TarState::Padding(pad) } else { TarState::Header };
                    }
                }
                TarState::Header => {
                    let n = (512 - self.header.len()).min(data.len());
                    self.header.extend_from_slice(&data[..n]);
                    data = &data[n..];
                    if self.header.len() < 512 {
                        continue;
                    }
                    let header = core::mem::take(&mut self.header);
                    if !self.start_entry(&header, on) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn start_entry(&mut self, header: &[u8], on: &mut dyn FnMut(TarEvent) -> bool) -> bool {
        if header.iter().all(|b| *b == 0) {
            self.state = TarState::Done;
            return true;
        }
        let stored: u64 = octal(&header[148..156]);
        let sum: u64 = header.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { *b as u64 }).sum();
        if stored != sum {
            self.error = Some("tar header checksum mismatch");
            return false;
        }
        let size = octal(&header[124..136]) as usize;
        let flag = header[156];
        if matches!(flag, b'x' | b'g' | b'L' | b'K') {
            self.state = TarState::Meta { flag, size, body: Vec::with_capacity(size.min(1 << 20)) };
            if size == 0 {
                self.state = TarState::Header;
            }
            return true;
        }
        let mut name = text(&header[0..100]);
        if &header[257..262] == b"ustar" {
            let prefix = text(&header[345..500]);
            if !prefix.is_empty() {
                name = alloc::format!("{}/{}", prefix, name);
            }
        }
        let pax = core::mem::take(&mut self.pax);
        if let Some(p) = pax.get("path") {
            name = p.clone();
        }
        if let Some(l) = self.long_name.take() {
            name = l;
        }
        let mut link = text(&header[157..257]);
        if let Some(l) = pax.get("linkpath") {
            link = l.clone();
        }
        if let Some(l) = self.long_link.take() {
            link = l;
        }
        let kind = match flag {
            b'0' | 0 | b'7' => Kind::File,
            b'5' => Kind::Dir,
            b'2' => Kind::Symlink(link),
            b'1' => Kind::Hardlink(clean(&link)),
            _ => Kind::Other,
        };
        let path = clean(&name);
        let padding = size.div_ceil(512) * 512 - size;
        let skip = path.is_empty() && kind != Kind::Dir;
        let entry = StreamEntry { path, kind, mode: (octal(&header[100..108]) & 0o7777) as u32 };
        if skip {
            self.state = if size + padding > 0 { TarState::Padding(size + padding) } else { TarState::Header };
            return true;
        }
        self.current = Some(entry);
        if !on(TarEvent::Begin(self.current.as_ref().unwrap())) {
            return false;
        }
        if size == 0 {
            self.current = None;
            if !on(TarEvent::End) {
                return false;
            }
            self.state = TarState::Header;
        } else {
            self.state = TarState::Body { remaining: size, padding };
        }
        true
    }
}
