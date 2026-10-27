use alloc::string::String;
use alloc::vec::Vec;

use crate::sys::{syscall3, syscall5};

pub const SYS_AUDIO_OPEN: u64 = 9170;
pub const SYS_AUDIO_WRITE: u64 = 9171;
pub const SYS_AUDIO_STATUS: u64 = 9172;
pub const SYS_AUDIO_CONTROL: u64 = 9173;
pub const SYS_AUDIO_CLOSE: u64 = 9174;
pub const SYS_AUDIO_VOLUME: u64 = 9175;
pub const SYS_AUDIO_INFO: u64 = 9176;

const CONTROL_PAUSE: u64 = 1;
const CONTROL_FLUSH: u64 = 2;
const CONTROL_VOLUME: u64 = 3;
const CONTROL_DRAIN: u64 = 4;
const WRITE_NONBLOCK: u64 = 1;

#[derive(Clone, Copy, Default, Debug)]
pub struct StreamStatus {
    pub queued: u64,
    pub delay: u64,
    pub written: u64,
    pub capacity: u64,
    pub underruns: u64,
    pub device_rate: u64,
    pub played: u64,
    pub xruns: u64,
}

pub struct Stream {
    id: u32,
    rate: u32,
    channels: u32,
}

impl Stream {
    pub fn open(rate: u32, channels: u32) -> Result<Stream, i64> {
        Self::open_with_buffer(rate, channels, 0)
    }

    pub fn open_with_buffer(rate: u32, channels: u32, buffer_ms: u32) -> Result<Stream, i64> {
        let id = unsafe { syscall3(SYS_AUDIO_OPEN, rate as u64, channels as u64, buffer_ms as u64) };
        if id < 0 { Err(id) } else { Ok(Stream { id: id as u32, rate, channels }) }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    fn write_raw(&self, samples: &[i16], flags: u64) -> Result<usize, i64> {
        let bytes = samples.len() * 2;
        let n = unsafe { syscall5(SYS_AUDIO_WRITE, self.id as u64, samples.as_ptr() as u64, bytes as u64, flags, 0) };
        if n < 0 { Err(n) } else { Ok(n as usize) }
    }

    pub fn write(&self, samples: &[i16]) -> Result<usize, i64> {
        self.write_raw(samples, WRITE_NONBLOCK)
    }

    pub fn write_all(&self, samples: &[i16]) -> Result<(), i64> {
        let per_frame = self.channels as usize;
        let mut offset = 0;
        while offset < samples.len() {
            let frames = self.write_raw(&samples[offset..], 0)?;
            offset += frames * per_frame;
        }
        Ok(())
    }

    pub fn status(&self) -> StreamStatus {
        let mut raw = [0u64; 8];
        let r = unsafe { syscall3(SYS_AUDIO_STATUS, self.id as u64, raw.as_mut_ptr() as u64, 64) };
        if r < 0 {
            return StreamStatus::default();
        }
        StreamStatus { queued: raw[0], delay: raw[1], written: raw[2], capacity: raw[3], underruns: raw[4], device_rate: raw[5], played: raw[6], xruns: raw[7] }
    }

    pub fn room(&self) -> usize {
        let s = self.status();
        s.capacity.saturating_sub(s.queued) as usize
    }

    pub fn delay_ms(&self) -> u64 {
        self.status().delay * 1000 / self.rate.max(1) as u64
    }

    pub fn played_ms(&self) -> u64 {
        self.status().played * 1000 / self.rate.max(1) as u64
    }

    pub fn set_paused(&self, paused: bool) {
        unsafe { syscall3(SYS_AUDIO_CONTROL, self.id as u64, CONTROL_PAUSE, paused as u64) };
    }

    pub fn flush(&self) {
        unsafe { syscall3(SYS_AUDIO_CONTROL, self.id as u64, CONTROL_FLUSH, 0) };
    }

    pub fn set_volume(&self, percent: u32) {
        unsafe { syscall3(SYS_AUDIO_CONTROL, self.id as u64, CONTROL_VOLUME, percent.min(100) as u64) };
    }

    pub fn drain(&self) {
        unsafe { syscall3(SYS_AUDIO_CONTROL, self.id as u64, CONTROL_DRAIN, 0) };
        while self.status().delay > 0 {
            crate::sys::sleep_ms(10);
        }
    }

    pub fn close_after_playing(self) {
        unsafe { syscall3(SYS_AUDIO_CLOSE, self.id as u64, 1, 0) };
        core::mem::forget(self);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { syscall3(SYS_AUDIO_CLOSE, self.id as u64, 0, 0) };
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Volume {
    pub level: u32,
    pub muted: bool,
    pub changes: u64,
}

fn decode_volume(word: i64) -> Option<Volume> {
    if word < 0 {
        return None;
    }
    Some(Volume { level: (word & 0xFF) as u32, muted: (word >> 8) & 1 != 0, changes: (word >> 16) as u64 })
}

fn volume_call(op: u64, value: u64) -> Option<Volume> {
    decode_volume(unsafe { syscall3(SYS_AUDIO_VOLUME, op, value, 0) })
}

pub fn volume() -> Option<Volume> {
    volume_call(0, 0)
}

pub fn set_volume(level: u32) -> Option<Volume> {
    volume_call(1, level.min(100) as u64)
}

pub fn set_muted(muted: bool) -> Option<Volume> {
    volume_call(2, muted as u64)
}

pub fn step_volume(delta: i32) -> Option<Volume> {
    volume_call(3, delta as i64 as u64)
}

pub fn toggle_mute() -> Option<Volume> {
    volume_call(4, 0)
}

#[derive(Clone, Default, Debug)]
pub struct Info {
    pub available: bool,
    pub device: String,
    pub driver: String,
    pub rate: u32,
    pub outputs: String,
    pub running: bool,
    pub streams: usize,
}

pub fn info() -> Info {
    let mut buf = alloc::vec![0u8; 4096];
    let n = unsafe { syscall3(SYS_AUDIO_INFO, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
    if n <= 0 {
        return Info::default();
    }
    let text = String::from_utf8_lossy(&buf[..(n as usize).min(buf.len())]).into_owned();
    let mut info = Info::default();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["device", v] => info.device = String::from(*v),
            ["driver", v] => info.driver = String::from(*v),
            ["rate", v] => info.rate = v.parse().unwrap_or(0),
            ["outputs", v] => info.outputs = String::from(*v),
            ["running", v] => info.running = *v == "yes",
            ["stream", ..] => info.streams += 1,
            _ => {}
        }
    }
    info.available = !info.device.is_empty();
    info
}

pub fn perceived_level(level: u32) -> &'static str {
    match level {
        0 => "muted",
        1..=33 => "low",
        34..=66 => "medium",
        _ => "high",
    }
}
