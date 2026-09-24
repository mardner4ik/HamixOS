use alloc::string::String;
use alloc::vec::Vec;

pub struct Sha1 {
    state: [u32; 5],
    buffer: [u8; 64],
    filled: usize,
    length: u64,
}

impl Sha1 {
    pub fn new() -> Sha1 {
        Sha1 { state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0], buffer: [0; 64], filled: 0, length: 0 }
    }

    fn block(state: &mut [u32; 5], chunk: &[u8]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = *state;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e]) {
            *s = s.wrapping_add(v);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.buffer[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == 64 {
                let buffer = self.buffer;
                Self::block(&mut self.state, &buffer);
                self.filled = 0;
            }
        }
        while data.len() >= 64 {
            Self::block(&mut self.state, &data[..64]);
            data = &data[64..];
        }
        self.buffer[..data.len()].copy_from_slice(data);
        self.filled += data.len();
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = [0u8; 20];
        for (i, v) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
        }
        out
    }
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(data);
    h.finish()
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bits = (data.len() as u64).wrapping_mul(8);
    let mut tail: Vec<u8> = Vec::with_capacity(128);
    let full = data.len() / 64 * 64;
    tail.extend_from_slice(&data[full..]);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bits.to_be_bytes());
    let blocks = data[..full].chunks_exact(64).chain(tail.chunks_exact(64));
    for chunk in blocks {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[derive(Clone)]
struct Big {
    limbs: Vec<u32>,
}

impl Big {
    fn from_be(bytes: &[u8]) -> Big {
        let mut limbs = Vec::with_capacity(bytes.len() / 4 + 1);
        let mut end = bytes.len();
        while end > 0 {
            let start = end.saturating_sub(4);
            let mut v = 0u32;
            for b in &bytes[start..end] {
                v = (v << 8) | *b as u32;
            }
            limbs.push(v);
            end = start;
        }
        let mut big = Big { limbs };
        big.trim();
        big
    }

    fn to_be(&self, len: usize) -> Vec<u8> {
        let mut out = alloc::vec![0u8; len];
        for (i, limb) in self.limbs.iter().enumerate() {
            for j in 0..4 {
                let pos = i * 4 + j;
                if pos < len {
                    out[len - 1 - pos] = (limb >> (8 * j)) as u8;
                }
            }
        }
        out
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    fn bits(&self) -> usize {
        match self.limbs.last() {
            Some(top) => self.limbs.len() * 32 - top.leading_zeros() as usize,
            None => 0,
        }
    }

    fn bit(&self, i: usize) -> bool {
        self.limbs.get(i / 32).map(|l| (l >> (i % 32)) & 1 == 1).unwrap_or(false)
    }

    fn cmp(&self, other: &Big) -> core::cmp::Ordering {
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            if a != b {
                return a.cmp(b);
            }
        }
        core::cmp::Ordering::Equal
    }

    fn sub_assign(&mut self, other: &Big) {
        let mut borrow = 0i64;
        for i in 0..self.limbs.len() {
            let rhs = *other.limbs.get(i).unwrap_or(&0) as i64 + borrow;
            let mut v = self.limbs[i] as i64 - rhs;
            if v < 0 {
                v += 1 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            self.limbs[i] = v as u32;
        }
        self.trim();
    }

    fn shl1_with(&mut self, bit: bool) {
        let mut carry = bit as u32;
        for limb in self.limbs.iter_mut() {
            let next = *limb >> 31;
            *limb = (*limb << 1) | carry;
            carry = next;
        }
        if carry != 0 {
            self.limbs.push(carry);
        }
    }

    fn mul(&self, other: &Big) -> Big {
        let mut out = alloc::vec![0u32; self.limbs.len() + other.limbs.len() + 1];
        for (i, a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, b) in other.limbs.iter().enumerate() {
                let t = *a as u64 * *b as u64 + out[i + j] as u64 + carry;
                out[i + j] = t as u32;
                carry = t >> 32;
            }
            let mut k = i + other.limbs.len();
            while carry != 0 {
                let t = out[k] as u64 + carry;
                out[k] = t as u32;
                carry = t >> 32;
                k += 1;
            }
        }
        let mut big = Big { limbs: out };
        big.trim();
        big
    }

    fn rem(&self, m: &Big) -> Big {
        let mut r = Big { limbs: Vec::with_capacity(m.limbs.len() + 1) };
        for i in (0..self.bits()).rev() {
            r.shl1_with(self.bit(i));
            if r.cmp(m) != core::cmp::Ordering::Less {
                r.sub_assign(m);
            }
        }
        r
    }

    fn pow_mod(&self, exp: &Big, m: &Big) -> Big {
        let mut result = Big { limbs: alloc::vec![1] };
        let base = self.rem(m);
        for i in (0..exp.bits()).rev() {
            result = result.mul(&result).rem(m);
            if exp.bit(i) {
                result = result.mul(&base).rem(m);
            }
        }
        result
    }
}

pub struct RsaKey {
    n: Big,
    e: Big,
    size: usize,
}

struct Der<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    fn read(&mut self) -> Option<(u8, &'a [u8])> {
        let tag = *self.data.get(self.pos)?;
        let first = *self.data.get(self.pos + 1)? as usize;
        self.pos += 2;
        let len = if first & 0x80 == 0 {
            first
        } else {
            let count = first & 0x7F;
            if count == 0 || count > 4 {
                return None;
            }
            let mut len = 0usize;
            for _ in 0..count {
                len = (len << 8) | *self.data.get(self.pos)? as usize;
                self.pos += 1;
            }
            len
        };
        let body = self.data.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some((tag, body))
    }
}

impl RsaKey {
    pub fn from_pem(text: &str) -> Option<RsaKey> {
        let body: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = base64_decode(&body)?;
        let mut outer = Der { data: &der, pos: 0 };
        let (tag, spki) = outer.read()?;
        if tag != 0x30 {
            return None;
        }
        let mut spki = Der { data: spki, pos: 0 };
        let (tag, _algorithm) = spki.read()?;
        if tag != 0x30 {
            return None;
        }
        let (tag, bits) = spki.read()?;
        if tag != 0x03 || bits.first() != Some(&0) {
            return None;
        }
        let mut rsa = Der { data: &bits[1..], pos: 0 };
        let (tag, seq) = rsa.read()?;
        if tag != 0x30 {
            return None;
        }
        let mut fields = Der { data: seq, pos: 0 };
        let (t1, n) = fields.read()?;
        let (t2, e) = fields.read()?;
        if t1 != 0x02 || t2 != 0x02 {
            return None;
        }
        let n = Big::from_be(n);
        let size = n.bits().div_ceil(8);
        Some(RsaKey { n, e: Big::from_be(e), size })
    }

    pub fn verify_sha1(&self, digest: &[u8; 20], signature: &[u8]) -> bool {
        const PREFIX: [u8; 15] = [0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14];
        self.verify(&PREFIX, digest, signature)
    }

    pub fn verify_sha256(&self, digest: &[u8; 32], signature: &[u8]) -> bool {
        const PREFIX: [u8; 19] = [0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20];
        self.verify(&PREFIX, digest, signature)
    }

    fn verify(&self, prefix: &[u8], message_digest: &[u8], signature: &[u8]) -> bool {
        if signature.len() != self.size || self.size < 64 {
            return false;
        }
        let s = Big::from_be(signature);
        if s.cmp(&self.n) != core::cmp::Ordering::Less {
            return false;
        }
        let em = s.pow_mod(&self.e, &self.n).to_be(self.size);
        let tail = prefix.len() + message_digest.len();
        let pad_end = self.size - tail - 1;
        if em[0] != 0 || em[1] != 1 || em[pad_end] != 0 || pad_end < 10 {
            return false;
        }
        if em[2..pad_end].iter().any(|b| *b != 0xFF) {
            return false;
        }
        &em[pad_end + 1..pad_end + 1 + prefix.len()] == prefix && &em[self.size - message_digest.len()..] == message_digest
    }
}
