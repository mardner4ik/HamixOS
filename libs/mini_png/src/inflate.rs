//! A from-scratch DEFLATE (RFC 1951) decoder. The bit-order rules in
//! DEFLATE are a classic source of bugs: everything *except* Huffman codes
//! is read least-significant-bit-first, while Huffman codes themselves are
//! conceptually packed most-significant-bit-first. The `decode_symbol`
//! function below follows the structure of `puff.c` (Mark Adler's minimal
//! reference inflate implementation) specifically to get that right,
//! rather than risk re-deriving it from scratch with no way to test here.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

pub type InflateResult<T> = Result<T, &'static str>;

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u32, // 0..8, next bit to read within data[byte_pos]
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte_pos: 0, bit_pos: 0 }
    }

    /// Reads a single bit, least-significant-bit-first within each byte.
    fn bit(&mut self) -> InflateResult<u32> {
        if self.byte_pos >= self.data.len() {
            return Err("inflate: ran out of input");
        }
        let byte = self.data[self.byte_pos];
        let b = (byte >> self.bit_pos) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(b as u32)
    }

    /// Reads `n` bits (n <= 16) as a plain little-endian-ish integer: the
    /// first bit read becomes the *lowest* order bit. Used for everything
    /// that isn't a Huffman code (extra bits, header fields, etc).
    fn bits(&mut self, n: u32) -> InflateResult<u32> {
        let mut value = 0u32;
        for i in 0..n {
            value |= self.bit()? << i;
        }
        Ok(value)
    }

    /// Discards any partial byte, moving to the next whole byte boundary.
    fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    fn read_u16_le(&mut self) -> InflateResult<u16> {
        if self.byte_pos + 2 > self.data.len() {
            return Err("inflate: ran out of input reading u16");
        }
        let v = u16::from_le_bytes([self.data[self.byte_pos], self.data[self.byte_pos + 1]]);
        self.byte_pos += 2;
        Ok(v)
    }

    fn read_bytes(&mut self, n: usize) -> InflateResult<&'a [u8]> {
        if self.byte_pos + n > self.data.len() {
            return Err("inflate: ran out of input reading raw bytes");
        }
        let slice = &self.data[self.byte_pos..self.byte_pos + n];
        self.byte_pos += n;
        Ok(slice)
    }
}

/// A canonical Huffman table built from a list of code lengths (one per
/// symbol, 0 meaning "symbol unused"), keyed by (code length, code value)
/// for lookup. Small and simple rather than fast -- decoding a wallpaper
/// once at boot doesn't need a speed-optimized table.
struct HuffTable {
    map: BTreeMap<(u8, u32), u16>,
    max_len: u8,
}

impl HuffTable {
    fn build(lengths: &[u8]) -> InflateResult<Self> {
        let max_len = *lengths.iter().max().unwrap_or(&0);
        if max_len == 0 {
            return Ok(Self { map: BTreeMap::new(), max_len: 0 });
        }

        let mut bl_count = vec![0u32; max_len as usize + 1];
        for &len in lengths {
            if len != 0 {
                bl_count[len as usize] += 1;
            }
        }

        let mut next_code = vec![0u32; max_len as usize + 2];
        let mut code = 0u32;
        for len in 1..=max_len as usize {
            code = (code + bl_count[len - 1]) << 1;
            next_code[len] = code;
        }

        let mut map = BTreeMap::new();
        for (symbol, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let c = next_code[len as usize];
            next_code[len as usize] += 1;
            map.insert((len, c), symbol as u16);
        }

        Ok(Self { map, max_len })
    }

    /// Mirrors puff.c's `decode()`: read one bit at a time, building the
    /// code value MSB-first (`code = (code << 1) | new_bit`), checking
    /// after each bit whether (length-so-far, code-so-far) names a known
    /// symbol.
    fn decode(&self, r: &mut BitReader) -> InflateResult<u16> {
        if self.max_len == 0 {
            return Err("inflate: decode from empty huffman table");
        }
        let mut code: u32 = 0;
        for len in 1..=self.max_len {
            code = (code << 1) | r.bit()?;
            if let Some(&symbol) = self.map.get(&(len, code)) {
                return Ok(symbol);
            }
        }
        Err("inflate: invalid huffman code")
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
    1537, 2049, 3073, 4097, 6145, 8193, 12289,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_LENGTH_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn fixed_literal_lengths() -> Vec<u8> {
    let mut lengths = vec![0u8; 288];
    for i in 0..144 {
        lengths[i] = 8;
    }
    for i in 144..256 {
        lengths[i] = 9;
    }
    for i in 256..280 {
        lengths[i] = 7;
    }
    for i in 280..288 {
        lengths[i] = 8;
    }
    lengths
}

fn fixed_distance_lengths() -> Vec<u8> {
    vec![5u8; 30]
}

fn inflate_block(r: &mut BitReader, out: &mut Vec<u8>, lit: &HuffTable, dist: &HuffTable) -> InflateResult<()> {
    loop {
        let symbol = lit.decode(r)?;
        if symbol == 256 {
            return Ok(());
        }
        if symbol < 256 {
            out.push(symbol as u8);
            continue;
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

        if distance == 0 || distance > out.len() {
            return Err("inflate: back-reference distance out of range");
        }
        let start = out.len() - distance;
        for i in 0..length {
            let byte = out[start + i];
            out.push(byte);
        }
    }
}

fn read_dynamic_tables(r: &mut BitReader) -> InflateResult<(HuffTable, HuffTable)> {
    let hlit = r.bits(5)? as usize + 257;
    let hdist = r.bits(5)? as usize + 1;
    let hclen = r.bits(4)? as usize + 4;

    let mut cl_lengths = vec![0u8; 19];
    for i in 0..hclen {
        cl_lengths[CODE_LENGTH_ORDER[i]] = r.bits(3)? as u8;
    }
    let cl_table = HuffTable::build(&cl_lengths)?;

    let mut lengths = Vec::with_capacity(hlit + hdist);
    while lengths.len() < hlit + hdist {
        let sym = cl_table.decode(r)?;
        match sym {
            0..=15 => lengths.push(sym as u8),
            16 => {
                let prev = *lengths.last().ok_or("inflate: repeat with no previous code length")?;
                let rep = 3 + r.bits(2)?;
                for _ in 0..rep {
                    lengths.push(prev);
                }
            }
            17 => {
                let rep = 3 + r.bits(3)?;
                for _ in 0..rep {
                    lengths.push(0);
                }
            }
            18 => {
                let rep = 11 + r.bits(7)?;
                for _ in 0..rep {
                    lengths.push(0);
                }
            }
            _ => return Err("inflate: invalid code-length symbol"),
        }
    }
    if lengths.len() != hlit + hdist {
        return Err("inflate: code length run overshot HLIT+HDIST");
    }

    let lit_table = HuffTable::build(&lengths[..hlit])?;
    let dist_table = HuffTable::build(&lengths[hlit..])?;
    Ok((lit_table, dist_table))
}

/// Decompresses a raw DEFLATE stream (no zlib/gzip wrapper).
pub fn inflate(data: &[u8]) -> InflateResult<Vec<u8>> {
    let mut r = BitReader::new(data);
    let mut out = Vec::new();

    loop {
        let bfinal = r.bit()?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                r.align_to_byte();
                let len = r.read_u16_le()?;
                let _nlen = r.read_u16_le()?;
                let bytes = r.read_bytes(len as usize)?;
                out.extend_from_slice(bytes);
            }
            1 => {
                let lit = HuffTable::build(&fixed_literal_lengths())?;
                let dist = HuffTable::build(&fixed_distance_lengths())?;
                inflate_block(&mut r, &mut out, &lit, &dist)?;
            }
            2 => {
                let (lit, dist) = read_dynamic_tables(&mut r)?;
                inflate_block(&mut r, &mut out, &lit, &dist)?;
            }
            _ => return Err("inflate: reserved block type"),
        }
        if bfinal == 1 {
            break;
        }
    }

    Ok(out)
}

/// Strips the 2-byte zlib header (RFC 1950) and inflates the payload.
/// Does not verify the trailing Adler-32 checksum.
pub fn zlib_decompress(data: &[u8]) -> InflateResult<Vec<u8>> {
    if data.len() < 6 {
        return Err("zlib: stream too short");
    }
    let cmf = data[0];
    if cmf & 0x0F != 8 {
        return Err("zlib: unsupported compression method (expected DEFLATE)");
    }
    let flg = data[1];
    if flg & 0x20 != 0 {
        return Err("zlib: preset dictionaries are not supported");
    }
    inflate(&data[2..data.len() - 4])
}
