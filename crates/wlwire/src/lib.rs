#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub const HEADER_SIZE: usize = 8;
pub const MAX_MESSAGE_SIZE: usize = 0xffff;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub object: u32,
    pub opcode: u16,
    pub size: usize,
}

pub fn decode_header(data: &[u8]) -> Option<Header> {
    if data.len() < HEADER_SIZE {
        return None;
    }
    let object = u32::from_le_bytes(data[0..4].try_into().ok()?);
    let word = u32::from_le_bytes(data[4..8].try_into().ok()?);
    let size = (word >> 16) as usize;
    if size < HEADER_SIZE || size > MAX_MESSAGE_SIZE {
        return None;
    }
    Some(Header { object, opcode: word as u16, size })
}

pub struct Message {
    buf: Vec<u8>,
}

impl Message {
    pub fn new(object: u32, opcode: u16) -> Self {
        let mut buf = Vec::with_capacity(32);
        buf.extend_from_slice(&object.to_le_bytes());
        buf.extend_from_slice(&(opcode as u32).to_le_bytes());
        Self { buf }
    }

    pub fn u32(mut self, value: u32) -> Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn uint(self, value: u32) -> Self {
        self.u32(value)
    }

    pub fn i32(self, value: i32) -> Self {
        self.u32(value as u32)
    }

    pub fn int(self, value: i32) -> Self {
        self.i32(value)
    }

    pub fn fixed(self, value: i32) -> Self {
        self.i32(value.saturating_mul(256))
    }

    pub fn string(mut self, text: &str) -> Self {
        let len = text.len() + 1;
        self.buf.extend_from_slice(&(len as u32).to_le_bytes());
        self.buf.extend_from_slice(text.as_bytes());
        self.buf.push(0);
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        self
    }

    pub fn array(mut self, data: &[u8]) -> Self {
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
        self.buf[4..8].copy_from_slice(&((size << 16) | (opcode & 0xffff)).to_le_bytes());
        self.buf
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn u32(&mut self) -> u32 {
        let Some(end) = self.pos.checked_add(4) else {
            self.pos = self.data.len();
            return 0;
        };
        let Some(bytes) = self.data.get(self.pos..end) else {
            self.pos = self.data.len();
            return 0;
        };
        self.pos = end;
        u32::from_le_bytes(bytes.try_into().unwrap())
    }

    pub fn uint(&mut self) -> u32 {
        self.u32()
    }

    pub fn i32(&mut self) -> i32 {
        self.u32() as i32
    }

    pub fn int(&mut self) -> i32 {
        self.i32()
    }

    pub fn fixed(&mut self) -> i32 {
        self.i32() / 256
    }

    pub fn string(&mut self) -> String {
        let len = self.u32() as usize;
        if len == 0 {
            return String::new();
        }
        let start = self.pos;
        let end = start.saturating_add(len).min(self.data.len());
        let raw = &self.data[start..end];
        let text = String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(raw)).into_owned();
        self.pos = start.saturating_add(len.saturating_add(3) & !3).min(self.data.len());
        text
    }

    pub fn array(&mut self) -> Vec<u8> {
        let len = self.u32() as usize;
        let start = self.pos;
        let end = start.saturating_add(len).min(self.data.len());
        let bytes = self.data[start..end].to_vec();
        self.pos = start.saturating_add(len.saturating_add(3) & !3).min(self.data.len());
        bytes
    }
}
