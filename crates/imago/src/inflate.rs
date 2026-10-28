use alloc::vec;
use alloc::vec::Vec;

pub type InflateResult<T> = Result<T, &'static str>;

const PRIMARY_BITS: u32 = 10;
const WINDOW: usize = 32768;
const STREAM_CHUNK: usize = 1 << 18;

const KIND_LITERAL: u32 = 0;
const KIND_LENGTH: u32 = 1;
const KIND_END: u32 = 2;
const KIND_SUBTABLE: u32 = 3;

const LENGTH_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CODE_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

#[inline(always)]
fn entry(length: u32, extra: u32, kind: u32, value: u32) -> u32 {
    length | extra << 8 | kind << 12 | value << 16
}

struct Table {
    entries: Vec<u32>,
}

impl Table {
    fn build(lengths: &[u8], symbol: impl Fn(usize) -> (u32, u32, u32)) -> InflateResult<Table> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for len in 1..16 {
            left = (left << 1) - count[len] as i32;
            if left < 0 {
                return Err("inflate: oversubscribed huffman code");
            }
        }
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for len in 1..16 {
            code = (code + count[len - 1] as u32) << 1;
            next[len] = code;
        }
        let primary = 1usize << PRIMARY_BITS;
        let invalid = entry(1, 0, KIND_END, 0xFFFF);
        let mut entries = vec![invalid; primary];
        let mut codes: Vec<(u32, u32, usize)> = Vec::with_capacity(lengths.len());
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let len = len as u32;
            let c = next[len as usize];
            next[len as usize] += 1;
            let mut reversed = 0u32;
            for i in 0..len {
                reversed |= ((c >> i) & 1) << (len - 1 - i);
            }
            codes.push((reversed, len, sym));
        }
        let mut sub_bits = [0u32; 1 << PRIMARY_BITS];
        for &(rev, len, _) in codes.iter() {
            if len > PRIMARY_BITS {
                let slot = (rev & ((1 << PRIMARY_BITS) - 1)) as usize;
                sub_bits[slot] = sub_bits[slot].max(len - PRIMARY_BITS);
            }
        }
        for slot in 0..primary {
            let bits = sub_bits[slot];
            if bits > 0 {
                let offset = entries.len() as u32;
                entries.resize(entries.len() + (1usize << bits), invalid);
                entries[slot] = entry(PRIMARY_BITS, bits, KIND_SUBTABLE, offset);
            }
        }
        for &(rev, len, sym) in codes.iter() {
            let (extra, kind, value) = symbol(sym);
            if len <= PRIMARY_BITS {
                let e = entry(len, extra, kind, value);
                let mut i = rev as usize;
                while i < primary {
                    entries[i] = e;
                    i += 1 << len;
                }
            } else {
                let slot = (rev & ((1 << PRIMARY_BITS) - 1)) as usize;
                let head = entries[slot];
                let offset = (head >> 16) as usize;
                let bits = (head >> 8) & 15;
                let sub_len = len - PRIMARY_BITS;
                let e = entry(sub_len, extra, kind, value);
                let mut i = (rev >> PRIMARY_BITS) as usize;
                while i < (1usize << bits) {
                    entries[offset + i] = e;
                    i += 1 << sub_len;
                }
            }
        }
        Ok(Table { entries })
    }
}

fn literal_symbol(sym: usize) -> (u32, u32, u32) {
    match sym {
        0..=255 => (0, KIND_LITERAL, sym as u32),
        256 => (0, KIND_END, 0),
        257..=285 => (LENGTH_EXTRA[sym - 257] as u32, KIND_LENGTH, LENGTH_BASE[sym - 257] as u32),
        _ => (0, KIND_END, 0xFFFF),
    }
}

fn distance_symbol(sym: usize) -> (u32, u32, u32) {
    if sym < 30 { (DIST_EXTRA[sym] as u32, KIND_LENGTH, DIST_BASE[sym] as u32) } else { (0, KIND_END, 0xFFFF) }
}

fn plain_symbol(sym: usize) -> (u32, u32, u32) {
    (0, KIND_LITERAL, sym as u32)
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u64,
    count: u32,
    overrun: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0, buffer: 0, count: 0, overrun: 0 }
    }

    #[inline(always)]
    fn refill(&mut self) {
        if self.pos + 8 <= self.data.len() {
            let word = u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
            self.buffer |= word << self.count;
            self.pos += ((63 - self.count) >> 3) as usize;
            self.count |= 56;
            return;
        }
        while self.count <= 56 {
            let byte = if self.pos < self.data.len() {
                self.data[self.pos]
            } else {
                self.overrun += 1;
                0
            };
            self.pos += 1;
            self.buffer |= (byte as u64) << self.count;
            self.count += 8;
        }
    }

    #[inline(always)]
    fn peek(&self, n: u32) -> u32 {
        (self.buffer & ((1u64 << n) - 1)) as u32
    }

    #[inline(always)]
    fn consume(&mut self, n: u32) {
        self.buffer >>= n;
        self.count -= n;
    }

    #[inline(always)]
    fn take(&mut self, n: u32) -> u32 {
        if self.count < n {
            self.refill();
        }
        let v = self.peek(n);
        self.consume(n);
        v
    }

    fn exhausted(&self) -> bool {
        self.overrun > 8
    }

    fn align(&mut self) {
        let drop = self.count & 7;
        self.consume(drop);
    }

    fn consumed(&self) -> usize {
        (self.pos - (self.count / 8) as usize).min(self.data.len())
    }

    #[inline(always)]
    fn decode(&mut self, table: &Table) -> u32 {
        if self.count < 32 {
            self.refill();
        }
        let mut e = table.entries[self.peek(PRIMARY_BITS) as usize];
        if (e >> 12) & 15 == KIND_SUBTABLE {
            self.consume(PRIMARY_BITS);
            let bits = (e >> 8) & 15;
            let offset = (e >> 16) as usize;
            e = table.entries[offset + self.peek(bits) as usize];
        }
        self.consume(e & 0xFF);
        e
    }
}

struct Sink<'s> {
    out: Vec<u8>,
    emit: Option<&'s mut dyn FnMut(&[u8]) -> bool>,
    emitted: u64,
    failed: bool,
    limit: usize,
}

impl Sink<'_> {
    fn flush(&mut self, keep: usize) {
        let Some(emit) = self.emit.as_mut() else {
            return;
        };
        if self.out.len() <= keep {
            return;
        }
        let cut = self.out.len() - keep;
        if !self.failed && !emit(&self.out[..cut]) {
            self.failed = true;
        }
        self.emitted += cut as u64;
        self.out.drain(..cut);
    }

    #[inline(always)]
    fn check(&mut self) -> InflateResult<()> {
        if self.emit.is_some() {
            if self.out.len() >= STREAM_CHUNK + WINDOW {
                self.flush(WINDOW);
                if self.failed {
                    return Err("inflate: output rejected");
                }
            }
        } else if self.out.len() > self.limit {
            return Err("inflate: output too large");
        }
        Ok(())
    }

    #[inline(always)]
    fn copy(&mut self, distance: usize, length: usize) -> InflateResult<()> {
        let len = self.out.len();
        if distance == 0 || distance > len {
            return Err("inflate: distance before start of output");
        }
        let start = len - distance;
        if distance >= length {
            self.out.extend_from_within(start..start + length);
        } else if distance == 1 {
            let b = self.out[len - 1];
            self.out.resize(len + length, b);
        } else {
            self.out.reserve(length);
            let mut remaining = length;
            while remaining > 0 {
                let chunk = remaining.min(self.out.len() - start);
                self.out.extend_from_within(start..start + chunk);
                remaining -= chunk;
            }
        }
        Ok(())
    }
}

fn fixed_tables() -> InflateResult<(Table, Table)> {
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    Ok((Table::build(&lengths, literal_symbol)?, Table::build(&[5u8; 30], distance_symbol)?))
}

fn dynamic_tables(r: &mut Bits) -> InflateResult<(Table, Table)> {
    let hlit = r.take(5) as usize + 257;
    let hdist = r.take(5) as usize + 1;
    let hclen = r.take(4) as usize + 4;
    let mut code_lengths = [0u8; 19];
    for &index in CODE_ORDER.iter().take(hclen) {
        code_lengths[index] = r.take(3) as u8;
    }
    let codes = Table::build(&code_lengths, plain_symbol)?;
    let mut lengths = [0u8; 320];
    let mut i = 0;
    while i < hlit + hdist {
        let e = r.decode(&codes);
        let sym = e >> 16;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("inflate: repeat without previous length");
                }
                let prev = lengths[i - 1];
                let n = 3 + r.take(2) as usize;
                if i + n > hlit + hdist {
                    return Err("inflate: code lengths overflow");
                }
                lengths[i..i + n].fill(prev);
                i += n;
            }
            17 | 18 => {
                let n = if sym == 17 { 3 + r.take(3) as usize } else { 11 + r.take(7) as usize };
                if i + n > hlit + hdist {
                    return Err("inflate: code lengths overflow");
                }
                i += n;
            }
            _ => return Err("inflate: bad code length symbol"),
        }
        if r.exhausted() {
            return Err("inflate: ran out of input");
        }
    }
    if lengths[256] == 0 {
        return Err("inflate: missing end-of-block code");
    }
    Ok((Table::build(&lengths[..hlit], literal_symbol)?, Table::build(&lengths[hlit..hlit + hdist], distance_symbol)?))
}

fn block(r: &mut Bits, out: &mut Sink, lit: &Table, dist: &Table) -> InflateResult<()> {
    loop {
        let e = r.decode(lit);
        let kind = (e >> 12) & 15;
        if kind == KIND_LITERAL {
            out.out.push((e >> 16) as u8);
            if out.out.len() & 0xFFF == 0 {
                out.check()?;
                if r.exhausted() {
                    return Err("inflate: ran out of input");
                }
            }
            continue;
        }
        if kind == KIND_END {
            if e >> 16 != 0 {
                return Err("inflate: invalid literal/length symbol");
            }
            return Ok(());
        }
        let extra = (e >> 8) & 15;
        let length = (e >> 16) as usize + r.take(extra) as usize;
        let d = r.decode(dist);
        if (d >> 12) & 15 != KIND_LENGTH {
            return Err("inflate: invalid distance symbol");
        }
        let dextra = (d >> 8) & 15;
        let distance = (d >> 16) as usize + r.take(dextra) as usize;
        out.copy(distance, length)?;
        out.check()?;
        if r.exhausted() {
            return Err("inflate: ran out of input");
        }
    }
}

fn run(data: &[u8], out: &mut Sink) -> InflateResult<usize> {
    let mut r = Bits::new(data);
    let mut fixed: Option<(Table, Table)> = None;
    loop {
        let last = r.take(1);
        let kind = r.take(2);
        match kind {
            0 => {
                r.align();
                let len = r.take(16) as usize;
                let nlen = r.take(16) as usize;
                if len != !nlen & 0xFFFF {
                    return Err("inflate: stored block length mismatch");
                }
                let mut remaining = len;
                while remaining > 0 && r.count >= 8 {
                    out.out.push(r.take(8) as u8);
                    remaining -= 1;
                }
                let start = r.pos - (r.count / 8) as usize;
                if start + remaining > data.len() {
                    return Err("inflate: ran out of input");
                }
                out.out.extend_from_slice(&data[start..start + remaining]);
                r = Bits { data, pos: start + remaining, buffer: 0, count: 0, overrun: 0 };
                out.check()?;
            }
            1 => {
                if fixed.is_none() {
                    fixed = Some(fixed_tables()?);
                }
                let (lit, dist) = fixed.as_ref().unwrap();
                block(&mut r, out, lit, dist)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut r)?;
                block(&mut r, out, &lit, &dist)?;
            }
            _ => return Err("inflate: reserved block type"),
        }
        if r.exhausted() {
            return Err("inflate: ran out of input");
        }
        if last == 1 {
            break;
        }
    }
    Ok(r.consumed())
}

pub fn inflate_sized(data: &[u8], expected: usize) -> InflateResult<Vec<u8>> {
    inflate_counted(data, expected).map(|(out, _)| out)
}

pub fn inflate_counted(data: &[u8], expected: usize) -> InflateResult<(Vec<u8>, usize)> {
    let capacity = if expected > 0 { expected } else { data.len().saturating_mul(4) };
    let mut sink = Sink { out: Vec::with_capacity(capacity), emit: None, emitted: 0, failed: false, limit: 1 << 31 };
    let used = run(data, &mut sink)?;
    Ok((sink.out, used))
}

pub fn inflate_stream(data: &[u8], emit: &mut dyn FnMut(&[u8]) -> bool) -> InflateResult<(usize, u64)> {
    let mut sink = Sink { out: Vec::with_capacity(STREAM_CHUNK + WINDOW * 2), emit: Some(emit), emitted: 0, failed: false, limit: usize::MAX };
    let used = run(data, &mut sink)?;
    sink.flush(0);
    if sink.failed {
        return Err("inflate: output rejected");
    }
    Ok((used, sink.emitted))
}

pub fn zlib_decompress_sized(data: &[u8], expected: usize) -> InflateResult<Vec<u8>> {
    if data.len() < 6 {
        return Err("zlib: stream too short");
    }
    if data[0] & 0x0F != 8 || (data[0] as u16 * 256 + data[1] as u16) % 31 != 0 {
        return Err("zlib: bad header");
    }
    if data[1] & 0x20 != 0 {
        return Err("zlib: preset dictionaries are not supported");
    }
    inflate_sized(&data[2..], expected)
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}
