use alloc::string::String;
use alloc::vec::Vec;

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    pub fn u32(&mut self) -> u32 {
        if self.pos + 4 > self.data.len() {
            self.pos = self.data.len();
            return 0;
        }
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }

    pub fn i32(&mut self) -> i32 {
        self.u32() as i32
    }

    pub fn string(&mut self) -> String {
        let len = self.u32() as usize;
        if len == 0 {
            return String::new();
        }
        let end = (self.pos + len).min(self.data.len());
        let raw = &self.data[self.pos..end];
        let text = String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(raw)).into_owned();
        self.pos = (self.pos + ((len + 3) & !3)).min(self.data.len());
        text
    }
}

pub struct Message {
    buf: Vec<u8>,
}

impl Message {
    pub fn new(object: u32, opcode: u16) -> Message {
        let mut buf = Vec::with_capacity(32);
        buf.extend_from_slice(&object.to_le_bytes());
        buf.extend_from_slice(&(opcode as u32).to_le_bytes());
        Message { buf }
    }

    pub fn u32(mut self, v: u32) -> Message {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn i32(self, v: i32) -> Message {
        self.u32(v as u32)
    }

    pub fn fixed(self, v: i32) -> Message {
        self.i32(v.saturating_mul(256))
    }

    pub fn string(mut self, s: &str) -> Message {
        let len = s.len() + 1;
        self.buf.extend_from_slice(&(len as u32).to_le_bytes());
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        self
    }

    pub fn array(mut self, data: &[u8]) -> Message {
        self.buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(data);
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        self
    }

    pub fn finish(mut self) -> Vec<u8> {
        let size = self.buf.len() as u32;
        let opcode = u32::from_le_bytes(self.buf[4..8].try_into().unwrap());
        self.buf[4..8].copy_from_slice(&((size << 16) | opcode).to_le_bytes());
        self.buf
    }
}
