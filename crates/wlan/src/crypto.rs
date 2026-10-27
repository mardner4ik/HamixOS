use alloc::vec::Vec;

pub struct Sha1 {
    state: [u32; 5],
    buffer: [u8; 64],
    buffered: usize,
    length: u64,
}

impl Sha1 {
    pub fn new() -> Sha1 {
        Sha1 { state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0], buffer: [0; 64], buffered: 0, length: 0 }
    }

    fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
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
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                Self::compress(&mut self.state, &block);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            let block: [u8; 64] = data[..64].try_into().unwrap();
            Self::compress(&mut self.state, &block);
            data = &data[64..];
        }
        self.buffer[..data.len()].copy_from_slice(data);
        self.buffered += data.len();
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.length * 8;
        self.update(&[0x80]);
        while self.buffered != 56 {
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

pub fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..20].copy_from_slice(&sha1(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha1::new();
    let ipad: Vec<u8> = block.iter().map(|b| b ^ 0x36).collect();
    inner.update(&ipad);
    for part in parts {
        inner.update(part);
    }
    let inner = inner.finish();
    let mut outer = Sha1::new();
    let opad: Vec<u8> = block.iter().map(|b| b ^ 0x5c).collect();
    outer.update(&opad);
    outer.update(&inner);
    outer.finish()
}

pub fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    for (block_index, chunk) in out.chunks_mut(20).enumerate() {
        let counter = (block_index as u32 + 1).to_be_bytes();
        let mut u = hmac_sha1(password, &[salt, &counter]);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha1(password, &[&u]);
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

pub fn psk_from_passphrase(passphrase: &str, ssid: &[u8]) -> [u8; 32] {
    let mut pmk = [0u8; 32];
    pbkdf2_sha1(passphrase.as_bytes(), ssid, 4096, &mut pmk);
    pmk
}

pub fn prf_80211(key: &[u8], label: &[u8], data: &[u8], out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(20).enumerate() {
        let digest = hmac_sha1(key, &[label, &[0u8], data, &[i as u8]]);
        chunk.copy_from_slice(&digest[..chunk.len()]);
    }
}

const SBOX: [u8; 256] = {
    let mut sbox = [0u8; 256];
    let mut p: u8 = 1;
    let mut q: u8 = 1;
    loop {
        p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1B } else { 0 };
        q ^= q << 1;
        q ^= q << 2;
        q ^= q << 4;
        if q & 0x80 != 0 {
            q ^= 0x09;
        }
        let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
        sbox[p as usize] = x ^ 0x63;
        if p == 1 {
            break;
        }
    }
    sbox[0] = 0x63;
    sbox
};

const INV_SBOX: [u8; 256] = {
    let mut inv = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        inv[SBOX[i] as usize] = i as u8;
        i += 1;
    }
    inv
};

fn xtime(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1B } else { 0 }
}

fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0;
    while b != 0 {
        if b & 1 != 0 {
            p ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    p
}

pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
}

impl Aes128 {
    pub fn new(key: &[u8; 16]) -> Aes128 {
        let mut words = [[0u8; 4]; 44];
        for i in 0..4 {
            words[i].copy_from_slice(&key[i * 4..i * 4 + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut temp = words[i - 1];
            if i % 4 == 0 {
                temp = [SBOX[temp[1] as usize] ^ rcon, SBOX[temp[2] as usize], SBOX[temp[3] as usize], SBOX[temp[0] as usize]];
                rcon = xtime(rcon);
            }
            for j in 0..4 {
                words[i][j] = words[i - 4][j] ^ temp[j];
            }
        }
        let mut round_keys = [[0u8; 16]; 11];
        for r in 0..11 {
            for i in 0..4 {
                round_keys[r][i * 4..i * 4 + 4].copy_from_slice(&words[r * 4 + i]);
            }
        }
        Aes128 { round_keys }
    }

    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        for i in 0..16 {
            block[i] ^= self.round_keys[0][i];
        }
        for round in 1..11 {
            for b in block.iter_mut() {
                *b = SBOX[*b as usize];
            }
            let s = *block;
            for c in 0..4 {
                for r in 0..4 {
                    block[c * 4 + r] = s[((c + r) % 4) * 4 + r];
                }
            }
            if round != 10 {
                for c in 0..4 {
                    let col = [block[c * 4], block[c * 4 + 1], block[c * 4 + 2], block[c * 4 + 3]];
                    block[c * 4] = xtime(col[0]) ^ (xtime(col[1]) ^ col[1]) ^ col[2] ^ col[3];
                    block[c * 4 + 1] = col[0] ^ xtime(col[1]) ^ (xtime(col[2]) ^ col[2]) ^ col[3];
                    block[c * 4 + 2] = col[0] ^ col[1] ^ xtime(col[2]) ^ (xtime(col[3]) ^ col[3]);
                    block[c * 4 + 3] = (xtime(col[0]) ^ col[0]) ^ col[1] ^ col[2] ^ xtime(col[3]);
                }
            }
            for i in 0..16 {
                block[i] ^= self.round_keys[round][i];
            }
        }
    }

    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        for i in 0..16 {
            block[i] ^= self.round_keys[10][i];
        }
        for round in (0..10).rev() {
            let s = *block;
            for c in 0..4 {
                for r in 0..4 {
                    block[((c + r) % 4) * 4 + r] = s[c * 4 + r];
                }
            }
            for b in block.iter_mut() {
                *b = INV_SBOX[*b as usize];
            }
            for i in 0..16 {
                block[i] ^= self.round_keys[round][i];
            }
            if round != 0 {
                for c in 0..4 {
                    let col = [block[c * 4], block[c * 4 + 1], block[c * 4 + 2], block[c * 4 + 3]];
                    block[c * 4] = gmul(col[0], 14) ^ gmul(col[1], 11) ^ gmul(col[2], 13) ^ gmul(col[3], 9);
                    block[c * 4 + 1] = gmul(col[0], 9) ^ gmul(col[1], 14) ^ gmul(col[2], 11) ^ gmul(col[3], 13);
                    block[c * 4 + 2] = gmul(col[0], 13) ^ gmul(col[1], 9) ^ gmul(col[2], 14) ^ gmul(col[3], 11);
                    block[c * 4 + 3] = gmul(col[0], 11) ^ gmul(col[1], 13) ^ gmul(col[2], 9) ^ gmul(col[3], 14);
                }
            }
        }
    }
}

pub fn aes_unwrap(kek: &[u8; 16], wrapped: &[u8]) -> Option<Vec<u8>> {
    if wrapped.len() < 24 || wrapped.len() % 8 != 0 {
        return None;
    }
    let n = wrapped.len() / 8 - 1;
    let aes = Aes128::new(kek);
    let mut a: [u8; 8] = wrapped[..8].try_into().unwrap();
    let mut r: Vec<[u8; 8]> = wrapped[8..].chunks(8).map(|c| c.try_into().unwrap()).collect();
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut block = [0u8; 16];
            let at = u64::from_be_bytes(a) ^ t;
            block[..8].copy_from_slice(&at.to_be_bytes());
            block[8..].copy_from_slice(&r[i]);
            aes.decrypt_block(&mut block);
            a.copy_from_slice(&block[..8]);
            r[i].copy_from_slice(&block[8..]);
        }
    }
    if a != [0xA6; 8] {
        return None;
    }
    Some(r.concat())
}

pub fn aes_wrap(kek: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    let n = plain.len() / 8;
    let aes = Aes128::new(kek);
    let mut a = [0xA6u8; 8];
    let mut r: Vec<[u8; 8]> = plain.chunks(8).map(|c| c.try_into().unwrap()).collect();
    for j in 0..6 {
        for i in 0..n {
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&r[i]);
            aes.encrypt_block(&mut block);
            let t = (n * j + i + 1) as u64;
            a = (u64::from_be_bytes(block[..8].try_into().unwrap()) ^ t).to_be_bytes();
            r[i].copy_from_slice(&block[8..]);
        }
    }
    let mut out = a.to_vec();
    for block in r {
        out.extend_from_slice(&block);
    }
    out
}

pub fn ccm_mic(aes: &Aes128, nonce: &[u8; 13], aad: &[u8], payload: &[u8]) -> [u8; 8] {
    let mut x = [0u8; 16];
    x[0] = 0x40 | (3 << 3) | 1;
    x[1..14].copy_from_slice(nonce);
    x[14..16].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    aes.encrypt_block(&mut x);
    let mut header = Vec::with_capacity(aad.len() + 2);
    header.extend_from_slice(&(aad.len() as u16).to_be_bytes());
    header.extend_from_slice(aad);
    for data in [&header[..], payload] {
        for chunk in data.chunks(16) {
            for (i, b) in chunk.iter().enumerate() {
                x[i] ^= b;
            }
            aes.encrypt_block(&mut x);
        }
    }
    let mut mic = [0u8; 8];
    mic.copy_from_slice(&x[..8]);
    mic
}

fn ccm_ctr(aes: &Aes128, nonce: &[u8; 13], counter: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[0] = 1;
    a[1..14].copy_from_slice(nonce);
    a[14..16].copy_from_slice(&counter.to_be_bytes());
    aes.encrypt_block(&mut a);
    a
}

pub fn ccm_encrypt(key: &[u8; 16], nonce: &[u8; 13], aad: &[u8], payload: &[u8]) -> Vec<u8> {
    let aes = Aes128::new(key);
    let mic = ccm_mic(&aes, nonce, aad, payload);
    let mut out = Vec::with_capacity(payload.len() + 8);
    for (i, chunk) in payload.chunks(16).enumerate() {
        let s = ccm_ctr(&aes, nonce, i as u16 + 1);
        out.extend(chunk.iter().zip(s.iter()).map(|(a, b)| a ^ b));
    }
    let s0 = ccm_ctr(&aes, nonce, 0);
    out.extend(mic.iter().zip(s0.iter()).map(|(a, b)| a ^ b));
    out
}

pub fn ccm_decrypt(key: &[u8; 16], nonce: &[u8; 13], aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 8 {
        return None;
    }
    let aes = Aes128::new(key);
    let (cipher, tag) = data.split_at(data.len() - 8);
    let mut plain = Vec::with_capacity(cipher.len());
    for (i, chunk) in cipher.chunks(16).enumerate() {
        let s = ccm_ctr(&aes, nonce, i as u16 + 1);
        plain.extend(chunk.iter().zip(s.iter()).map(|(a, b)| a ^ b));
    }
    let s0 = ccm_ctr(&aes, nonce, 0);
    let expected = ccm_mic(&aes, nonce, aad, &plain);
    let mut diff = 0u8;
    for i in 0..8 {
        diff |= (tag[i] ^ s0[i]) ^ expected[i];
    }
    if diff == 0 { Some(plain) } else { None }
}
