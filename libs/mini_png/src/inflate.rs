use alloc::vec;
use alloc::vec::Vec;

pub type InflateResult<T> = Result<T, &'static str>;

const MAX_BITS: usize = 15;
const FAST_BITS: u32 = 11;
const FAST_MASK: u64 = (1 << FAST_BITS) - 1;

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u64,
    count: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, buffer: 0, count: 0 }
    }

    #[inline(always)]
    fn refill(&mut self) {
        if self.pos + 8 <= self.data.len() {
            let chunk = u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
            self.buffer |= chunk << self.count;
            let taken = (63 - self.count) / 8;
            self.pos += taken as usize;
            self.count += taken * 8;
            return;
        }
        while self.count <= 56 {
            let byte = if self.pos < self.data.len() { self.data[self.pos] } else if self.pos < self.data.len() + 8 { 0 } else { break };
            self.pos += 1;
            self.buffer |= (byte as u64) << self.count;
            self.count += 8;
        }
    }

    #[inline(always)]
    fn bits(&mut self, n: u32) -> InflateResult<u32> {
        if n == 0 {
            return Ok(0);
        }
        if self.count < n {
            self.refill();
            if self.count < n {
                return Err("inflate: ran out of input");
            }
        }
        let value = (self.buffer & ((1u64 << n) - 1)) as u32;
        self.buffer >>= n;
        self.count -= n;
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        let drop = self.count % 8;
        self.buffer >>= drop;
        self.count -= drop;
    }

    fn byte(&mut self) -> InflateResult<u8> {
        self.bits(8).map(|v| v as u8)
    }

    #[inline(always)]
    fn exhausted(&self) -> bool {
        (self.pos as u64 * 8).saturating_sub(self.count as u64) > self.data.len() as u64 * 8 + 16
    }
}

struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
    fast: Vec<u32>,
}

impl Huffman {
    fn build(lengths: &[u8]) -> InflateResult<Self> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &len in lengths {
            counts[len as usize] += 1;
        }
        counts[0] = 0;
        let mut left: i32 = 1;
        for len in 1..=MAX_BITS {
            left <<= 1;
            left -= counts[len] as i32;
            if left < 0 {
                return Err("inflate: over-subscribed huffman code");
            }
        }
        let mut offsets = [0u16; MAX_BITS + 2];
        for len in 1..=MAX_BITS {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &len) in lengths.iter().enumerate() {
            if len != 0 {
                symbols[offsets[len as usize] as usize] = symbol as u16;
                offsets[len as usize] += 1;
            }
        }

        let mut fast = vec![0u32; 1 << FAST_BITS];
        let mut first: u32 = 0;
        let mut index: usize = 0;
        for len in 1..=FAST_BITS as usize {
            let count = counts[len] as u32;
            for i in 0..count {
                let canonical = first + i;
                let reversed = canonical.reverse_bits() >> (32 - len);
                let entry = ((len as u32) << 16) | symbols[index + i as usize] as u32;
                let mut slot = reversed;
                while slot < (1 << FAST_BITS) {
                    fast[slot as usize] = entry;
                    slot += 1 << len;
                }
            }
            index += count as usize;
            first = (first + count) << 1;
        }

        Ok(Self { counts, symbols, fast })
    }

    #[inline(always)]
    fn decode(&self, r: &mut BitReader) -> InflateResult<u16> {
        if r.count < MAX_BITS as u32 {
            r.refill();
        }
        let entry = self.fast[(r.buffer & FAST_MASK) as usize];
        if entry != 0 && entry >> 16 <= r.count {
            let len = entry >> 16;
            r.buffer >>= len;
            r.count -= len;
            return Ok(entry as u16);
        }
        self.decode_slow(r)
    }

    #[inline(never)]
    fn decode_slow(&self, r: &mut BitReader) -> InflateResult<u16> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAX_BITS {
            code |= r.bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("inflate: invalid huffman code")
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CODE_LENGTH_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

const WINDOW: usize = 32 * 1024;
const STREAM_CHUNK: usize = 512 * 1024;

struct Output<'s> {
    buf: Vec<u8>,
    len: usize,
    sink: Option<&'s mut dyn FnMut(&[u8]) -> bool>,
    emitted: u64,
    failed: bool,
}

impl<'s> Output<'s> {
    fn with_capacity(capacity: usize) -> Self {
        Output { buf: vec![0u8; capacity.max(1024)], len: 0, sink: None, emitted: 0, failed: false }
    }

    fn streaming(sink: &'s mut dyn FnMut(&[u8]) -> bool) -> Self {
        Output { buf: vec![0u8; STREAM_CHUNK + WINDOW + 1024], len: 0, sink: Some(sink), emitted: 0, failed: false }
    }

    fn flush(&mut self, keep: usize) {
        let keep = keep.min(self.len);
        let out = self.len - keep;
        if out == 0 {
            return;
        }
        if let Some(sink) = self.sink.as_mut() {
            if !sink(&self.buf[..out]) {
                self.failed = true;
            }
        }
        self.emitted += out as u64;
        self.buf.copy_within(out..self.len, 0);
        self.len = keep;
    }

    #[inline(always)]
    fn reserve(&mut self, extra: usize) {
        if self.len + extra > self.buf.len() {
            if self.sink.is_some() {
                self.flush(WINDOW);
                if self.len + extra <= self.buf.len() {
                    return;
                }
            }
            let wanted = (self.buf.len() * 2).max(self.len + extra);
            self.buf.resize(wanted, 0);
        }
    }

    #[inline(always)]
    fn push(&mut self, byte: u8) {
        self.reserve(1);
        self.buf[self.len] = byte;
        self.len += 1;
    }

    #[inline(always)]
    fn copy_back(&mut self, distance: usize, length: usize) {
        self.reserve(length);
        let start = self.len - distance;
        if distance >= length {
            self.buf.copy_within(start..start + length, self.len);
        } else if distance == 1 {
            let value = self.buf[start];
            self.buf[self.len..self.len + length].fill(value);
        } else {
            let buf = &mut self.buf;
            for i in 0..length {
                buf[self.len + i] = buf[start + i];
            }
        }
        self.len += length;
    }

    fn finish(mut self) -> Vec<u8> {
        self.buf.truncate(self.len);
        self.buf
    }
}

fn inflate_block(r: &mut BitReader, out: &mut Output<'_>, lit: &Huffman, dist: &Huffman) -> InflateResult<()> {
    loop {
        if r.exhausted() {
            return Err("inflate: ran out of input");
        }
        let symbol = lit.decode(r)?;
        if symbol < 256 {
            out.push(symbol as u8);
            continue;
        }
        if symbol == 256 {
            return Ok(());
        }
        let idx = (symbol - 257) as usize;
        if idx >= LENGTH_BASE.len() {
            return Err("inflate: invalid length symbol");
        }
        let length = LENGTH_BASE[idx] as usize + r.bits(LENGTH_EXTRA[idx] as u32)? as usize;
        let dist_symbol = dist.decode(r)? as usize;
        if dist_symbol >= DIST_BASE.len() {
            return Err("inflate: invalid distance symbol");
        }
        let distance = DIST_BASE[dist_symbol] as usize + r.bits(DIST_EXTRA[dist_symbol] as u32)? as usize;
        if distance == 0 || distance > out.len {
            return Err("inflate: back-reference distance out of range");
        }
        out.copy_back(distance, length);
        if out.failed {
            return Err("inflate: output rejected");
        }
    }
}

fn read_dynamic_tables(r: &mut BitReader) -> InflateResult<(Huffman, Huffman)> {
    let hlit = r.bits(5)? as usize + 257;
    let hdist = r.bits(5)? as usize + 1;
    let hclen = r.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err("inflate: bad counts");
    }
    let mut cl_lengths = [0u8; 19];
    for &slot in CODE_LENGTH_ORDER.iter().take(hclen) {
        cl_lengths[slot] = r.bits(3)? as u8;
    }
    let cl_table = Huffman::build(&cl_lengths)?;

    let mut lengths = vec![0u8; hlit + hdist];
    let mut index = 0usize;
    while index < hlit + hdist {
        let sym = cl_table.decode(r)?;
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                if index == 0 {
                    return Err("inflate: repeat with no previous code length");
                }
                (lengths[index - 1], 3 + r.bits(2)? as usize)
            }
            17 => (0, 3 + r.bits(3)? as usize),
            18 => (0, 11 + r.bits(7)? as usize),
            _ => return Err("inflate: invalid code-length symbol"),
        };
        if index + repeat > hlit + hdist {
            return Err("inflate: code length run overshot HLIT+HDIST");
        }
        for slot in &mut lengths[index..index + repeat] {
            *slot = value;
        }
        index += repeat;
    }
    if lengths[256] == 0 {
        return Err("inflate: missing end-of-block code");
    }
    Ok((Huffman::build(&lengths[..hlit])?, Huffman::build(&lengths[hlit..])?))
}

pub fn inflate_sized(data: &[u8], expected: usize) -> InflateResult<Vec<u8>> {
    inflate_counted(data, expected).map(|(out, _)| out)
}

pub fn inflate_counted(data: &[u8], expected: usize) -> InflateResult<(Vec<u8>, usize)> {
    let mut out = Output::with_capacity(if expected > 0 { expected } else { data.len() * 4 });
    let consumed = inflate_into(data, &mut out)?;
    Ok((out.finish(), consumed))
}

pub fn inflate_stream(data: &[u8], sink: &mut dyn FnMut(&[u8]) -> bool) -> InflateResult<(usize, u64)> {
    let mut out = Output::streaming(sink);
    let consumed = inflate_into(data, &mut out)?;
    out.flush(0);
    if out.failed {
        return Err("inflate: output rejected");
    }
    Ok((consumed, out.emitted))
}

fn inflate_into(data: &[u8], out: &mut Output<'_>) -> InflateResult<usize> {
    let mut r = BitReader::new(data);
    let mut fixed: Option<(Huffman, Huffman)> = None;

    loop {
        let bfinal = r.bits(1)?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                r.align_to_byte();
                let len = r.byte()? as u16 | (r.byte()? as u16) << 8;
                let nlen = r.byte()? as u16 | (r.byte()? as u16) << 8;
                if len != !nlen {
                    return Err("inflate: stored block length mismatch");
                }
                for _ in 0..len {
                    let b = r.byte()?;
                    out.push(b);
                }
            }
            1 => {
                if fixed.is_none() {
                    let mut lengths = [0u8; 288];
                    lengths[..144].fill(8);
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    lengths[280..].fill(8);
                    fixed = Some((Huffman::build(&lengths)?, Huffman::build(&[5u8; 30])?));
                }
                let (lit, dist) = fixed.as_ref().unwrap();
                inflate_block(&mut r, out, lit, dist)?;
            }
            2 => {
                let (lit, dist) = read_dynamic_tables(&mut r)?;
                inflate_block(&mut r, out, &lit, &dist)?;
            }
            _ => return Err("inflate: reserved block type"),
        }
        if bfinal == 1 {
            break;
        }
    }
    let consumed = (r.pos.min(data.len() + 8) - (r.count / 8) as usize).min(data.len());
    Ok(consumed)
}

pub fn zlib_decompress_sized(data: &[u8], expected: usize) -> InflateResult<Vec<u8>> {
    if data.len() < 6 {
        return Err("zlib: stream too short");
    }
    if data[0] & 0x0F != 8 {
        return Err("zlib: unsupported compression method");
    }
    if data[1] & 0x20 != 0 {
        return Err("zlib: preset dictionaries are not supported");
    }
    inflate_sized(&data[2..], expected)
}
