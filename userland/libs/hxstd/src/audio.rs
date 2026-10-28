use alloc::string::String;
use alloc::vec::Vec;

use crate::sys::{self, O_CLOEXEC, O_NONBLOCK, O_RDWR, O_WRONLY};

const AFMT_S16_LE: i32 = 0x10;
const PCM_ENABLE_OUTPUT: i32 = 2;
const MIXER_VOLUME: u64 = 0;
const MIXER_SPEAKER: u64 = 5;
const EAGAIN: i64 = -11;

fn ioc(dir: u64, kind: u8, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | ((kind as u64) << 8) | nr
}

fn dsp_rw(nr: u64) -> u64 {
    ioc(3, b'P', nr, 4)
}

fn dsp_read(nr: u64, size: u64) -> u64 {
    ioc(2, b'P', nr, size)
}

fn int_call(fd: u64, request: u64, value: i32) -> Result<i32, i64> {
    let mut v = value.to_le_bytes();
    let r = sys::ioctl(fd, request, v.as_mut_ptr());
    if r < 0 { Err(r) } else { Ok(i32::from_le_bytes(v)) }
}

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
    fd: u64,
    rate: u32,
    channels: u32,
    written: core::cell::Cell<u64>,
}

impl Stream {
    pub fn open(rate: u32, channels: u32) -> Result<Stream, i64> {
        Self::open_with_buffer(rate, channels, 0)
    }

    pub fn open_with_buffer(rate: u32, channels: u32, buffer_ms: u32) -> Result<Stream, i64> {
        let fd = sys::open_with("/dev/dsp", O_WRONLY | O_NONBLOCK | O_CLOEXEC);
        if fd < 0 {
            return Err(fd);
        }
        let fd = fd as u64;
        let setup = || -> Result<(), i64> {
            int_call(fd, dsp_rw(5), AFMT_S16_LE)?;
            int_call(fd, dsp_rw(6), channels as i32)?;
            int_call(fd, dsp_rw(2), rate as i32)?;
            let ms = if buffer_ms == 0 { 1000 } else { buffer_ms.clamp(100, 10_000) };
            let bytes = rate as u64 * channels as u64 * 2 * ms as u64 / 1000;
            let fragments = (bytes / 4096).clamp(2, 0x7FFF) as i32;
            int_call(fd, dsp_rw(10), (fragments << 16) | 12)?;
            Ok(())
        };
        if let Err(e) = setup() {
            sys::close(fd);
            return Err(e);
        }
        Ok(Stream { fd, rate, channels, written: core::cell::Cell::new(0) })
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    fn frame_bytes(&self) -> usize {
        self.channels as usize * 2
    }

    pub fn write(&self, samples: &[i16]) -> Result<usize, i64> {
        let bytes = unsafe { core::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 2) };
        let n = sys::write(self.fd, bytes);
        if n == EAGAIN {
            return Ok(0);
        }
        if n < 0 {
            return Err(n);
        }
        let frames = n as usize / self.frame_bytes();
        self.written.set(self.written.get() + frames as u64);
        Ok(frames)
    }

    pub fn write_all(&self, samples: &[i16]) -> Result<(), i64> {
        let per_frame = self.channels as usize;
        let mut offset = 0;
        while offset < samples.len() {
            let frames = self.write(&samples[offset..])?;
            if frames == 0 {
                sys::sleep_ms(5);
                continue;
            }
            offset += frames * per_frame;
        }
        Ok(())
    }

    pub fn status(&self) -> StreamStatus {
        let frame = self.frame_bytes() as u64;
        let mut space = [0u8; 16];
        let has_space = sys::ioctl(self.fd, dsp_read(12, 16), space.as_mut_ptr()) == 0;
        let fragstotal = i32::from_le_bytes(space[4..8].try_into().unwrap()) as u64;
        let fragsize = i32::from_le_bytes(space[8..12].try_into().unwrap()) as u64;
        let room = i32::from_le_bytes(space[12..16].try_into().unwrap()).max(0) as u64;
        let delay = int_call(self.fd, dsp_read(23, 4), 0).unwrap_or(0).max(0) as u64 / frame;
        let mut optr = [0u8; 12];
        let played = if sys::ioctl(self.fd, dsp_read(18, 12), optr.as_mut_ptr()) == 0 { i32::from_le_bytes(optr[0..4].try_into().unwrap()).max(0) as u64 / frame } else { 0 };
        let capacity = if has_space { (fragstotal * fragsize / frame).max(room / frame) } else { 0 };
        StreamStatus {
            queued: capacity.saturating_sub(room / frame),
            delay,
            written: self.written.get(),
            capacity,
            underruns: 0,
            device_rate: self.rate as u64,
            played,
            xruns: 0,
        }
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
        let _ = int_call(self.fd, ioc(1, b'P', 16, 4), if paused { 0 } else { PCM_ENABLE_OUTPUT });
    }

    pub fn flush(&self) {
        sys::ioctl_value(self.fd, ioc(0, b'P', 0, 0), 0);
    }

    pub fn set_volume(&self, percent: u32) {
        let level = percent.min(100) as i32;
        let _ = int_call(self.fd, dsp_rw(24), level | level << 8);
    }

    pub fn drain(&self) {
        sys::ioctl_value(self.fd, ioc(0, b'P', 1, 0), 0);
    }

    pub fn close_after_playing(self) {
        sys::close(self.fd);
        core::mem::forget(self);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        sys::ioctl_value(self.fd, ioc(0, b'P', 0, 0), 0);
        sys::close(self.fd);
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Volume {
    pub level: u32,
    pub muted: bool,
    pub changes: u64,
}

fn mixer<R>(f: impl FnOnce(u64) -> Result<R, i64>) -> Option<R> {
    let fd = sys::open_with("/dev/mixer", O_RDWR | O_CLOEXEC);
    if fd < 0 {
        return None;
    }
    let r = f(fd as u64).ok();
    sys::close(fd as u64);
    r
}

fn read_volume(fd: u64) -> Result<Volume, i64> {
    let level = int_call(fd, ioc(2, b'M', MIXER_VOLUME, 4), 0)?;
    let speaker = int_call(fd, ioc(2, b'M', MIXER_SPEAKER, 4), 0)?;
    let mut info = [0u8; 92];
    let changes = if sys::ioctl(fd, ioc(2, b'M', 101, 92), info.as_mut_ptr()) == 0 { i32::from_le_bytes(info[48..52].try_into().unwrap()) as u32 as u64 } else { 0 };
    Ok(Volume { level: (level & 0xFF) as u32, muted: speaker & 0xFF == 0, changes })
}

fn write_level(fd: u64, level: u32) -> Result<(), i64> {
    let level = level.min(100) as i32;
    int_call(fd, ioc(3, b'M', MIXER_VOLUME, 4), level | level << 8).map(|_| ())
}

fn write_muted(fd: u64, muted: bool) -> Result<(), i64> {
    int_call(fd, ioc(3, b'M', MIXER_SPEAKER, 4), if muted { 0 } else { 100 | 100 << 8 }).map(|_| ())
}

pub fn volume() -> Option<Volume> {
    mixer(read_volume)
}

pub fn set_volume(level: u32) -> Option<Volume> {
    mixer(|fd| {
        write_level(fd, level)?;
        if level > 0 {
            write_muted(fd, false)?;
        }
        read_volume(fd)
    })
}

pub fn set_muted(muted: bool) -> Option<Volume> {
    mixer(|fd| {
        write_muted(fd, muted)?;
        read_volume(fd)
    })
}

pub fn step_volume(delta: i32) -> Option<Volume> {
    mixer(|fd| {
        let current = read_volume(fd)?;
        let level = (current.level as i32 + delta).clamp(0, 100) as u32;
        write_level(fd, level)?;
        if delta > 0 && current.muted {
            write_muted(fd, false)?;
        }
        read_volume(fd)
    })
}

pub fn toggle_mute() -> Option<Volume> {
    mixer(|fd| {
        let current = read_volume(fd)?;
        write_muted(fd, !current.muted)?;
        read_volume(fd)
    })
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
    let Some(text) = crate::fs::read_to_string("/proc/audio") else {
        return Info::default();
    };
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
