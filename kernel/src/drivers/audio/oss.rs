use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::task::{self, Pid};

pub const DSP_MAJOR: u32 = 14;
pub const MIXER_MINOR: u32 = 0;
pub const DSP_MINOR: u32 = 3;

const AFMT_U8: i32 = 0x08;
const AFMT_S16_LE: i32 = 0x10;
const PCM_ENABLE_OUTPUT: i32 = 2;
const DSP_CAP_TRIGGER: i32 = 0x1000;
const DSP_CAP_BATCH: i32 = 0x200;
const DSP_CAP_REALTIME: i32 = 0x200 << 1;
const MIXER_VOLUME: u32 = 0;
const MIXER_PCM: u32 = 4;
const MIXER_SPEAKER: u32 = 5;
const DEVMASK: i32 = (1 << MIXER_VOLUME) | (1 << MIXER_PCM) | (1 << MIXER_SPEAKER);

use crate::errno::EINVAL;
use crate::errno::ENOTTY;
use crate::errno::EAGAIN;
use crate::errno::EFAULT;
use crate::errno::ENODEV;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Dsp,
    Mixer,
}

struct Dsp {
    owner: Pid,
    rate: u32,
    channels: u32,
    format: i32,
    stream: Option<u32>,
    fragment_shift: u32,
    fragments: u32,
    volume: u32,
    trigger: i32,
    written_bytes: u64,
}

static FILES: Mutex<BTreeMap<u32, Node>> = Mutex::new(BTreeMap::new());
static DSPS: Mutex<BTreeMap<u32, Dsp>> = Mutex::new(BTreeMap::new());

fn with<R>(f: impl FnOnce(&mut BTreeMap<u32, Dsp>) -> R) -> R {
    without_interrupts(|| f(&mut DSPS.lock()))
}

pub fn opened(id: u32, node: Node) -> i64 {
    if !super::present() {
        return ENODEV;
    }
    without_interrupts(|| FILES.lock().insert(id, node));
    if node == Node::Dsp {
        let owner = task::current_pid();
        with(|d| d.insert(id, Dsp { owner, rate: 8000, channels: 1, format: AFMT_U8, stream: None, fragment_shift: 12, fragments: 16, volume: 100, trigger: PCM_ENABLE_OUTPUT, written_bytes: 0 }));
    }
    0
}

pub fn node(id: u32) -> Option<Node> {
    without_interrupts(|| FILES.lock().get(&id).copied())
}

pub fn closed(id: u32) {
    without_interrupts(|| FILES.lock().remove(&id));
    if let Some(dsp) = with(|d| d.remove(&id)) {
        if let Some(stream) = dsp.stream {
            super::close(dsp.owner, stream, true);
        }
    }
}

fn stream(id: u32) -> Result<(Pid, u32, u32, i32), i64> {
    let existing = with(|d| d.get(&id).map(|s| (s.owner, s.stream, s.channels, s.format, s.rate, s.volume, s.fragments << s.fragment_shift)));
    let Some((owner, stream, channels, format, rate, volume, buffer)) = existing else {
        return Err(EINVAL);
    };
    if let Some(s) = stream {
        return Ok((owner, s, channels, format));
    }
    let bytes_per_second = rate as u64 * channels as u64 * if format == AFMT_S16_LE { 2 } else { 1 };
    let ms = (buffer as u64 * 1000 / bytes_per_second.max(1)).clamp(100, 2000) as u32;
    let s = super::open(owner, rate, channels, ms);
    if s < 0 {
        return Err(s);
    }
    let s = s as u32;
    super::control(owner, s, super::CONTROL_VOLUME, volume as u64);
    with(|d| {
        if let Some(dsp) = d.get_mut(&id) {
            dsp.stream = Some(s);
        }
    });
    Ok((owner, s, channels, format))
}

pub fn write(fd: u64, id: u32, data: &[u8]) -> i64 {
    if node(id) != Some(Node::Dsp) {
        return EINVAL;
    }
    let (owner, s, _, format) = match stream(id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let samples: Vec<u8> = if format == AFMT_U8 {
        data.iter().flat_map(|b| (((*b as i16) - 128) << 8).to_le_bytes()).collect()
    } else {
        data[..data.len() & !1].to_vec()
    };
    let per = if format == AFMT_U8 { 2 } else { 1 };
    let channels = with(|d| d.get(&id).map(|x| x.channels)).unwrap_or(1) as usize;
    let block = !crate::syscall::fd_nonblock(fd);
    let mut done = 0usize;
    while done < samples.len() {
        let frames = super::write(owner, s, &samples[done..], block);
        if frames < 0 {
            return if done > 0 { (done / per) as i64 } else { frames };
        }
        if frames == 0 {
            if done == 0 {
                return EAGAIN;
            }
            break;
        }
        done += frames as usize * channels * 2;
    }
    let written = done / per;
    with(|d| {
        if let Some(dsp) = d.get_mut(&id) {
            dsp.written_bytes += written as u64;
        }
    });
    written as i64
}

fn read_int(arg: u64) -> Result<i32, i64> {
    let raw = crate::syscall::user_slice(arg, 4)?;
    Ok(i32::from_le_bytes((&*raw).try_into().unwrap()))
}

fn put_int(arg: u64, value: i32) -> i64 {
    if crate::syscall::copy_out(arg, 4, &value.to_le_bytes()) < 0 { EFAULT } else { 0 }
}

fn reset(id: u32) {
    let old = with(|d| d.get_mut(&id).and_then(|dsp| dsp.stream.take().map(|s| (dsp.owner, s))));
    if let Some((owner, s)) = old {
        super::close(owner, s, false);
    }
}

fn frame_bytes(dsp: &Dsp) -> u64 {
    dsp.channels as u64 * if dsp.format == AFMT_S16_LE { 2 } else { 1 }
}

pub fn ioctl(id: u32, request: u64, arg: u64) -> i64 {
    match node(id) {
        Some(Node::Dsp) => dsp_ioctl(id, request, arg),
        Some(Node::Mixer) => mixer_ioctl(request, arg),
        None => EINVAL,
    }
}

fn dsp_ioctl(id: u32, request: u64, arg: u64) -> i64 {
    let kind = (request >> 8) & 0xFF;
    let nr = request & 0xFF;
    if kind == b'M' as u64 {
        return mixer_ioctl(request, arg);
    }
    if kind != b'P' as u64 {
        return ENOTTY;
    }
    match nr {
        0 => {
            reset(id);
            0
        }
        1 => {
            if let Some((owner, s)) = with(|d| d.get(&id).and_then(|x| x.stream.map(|s| (x.owner, s)))) {
                super::control(owner, s, super::CONTROL_DRAIN, 0);
                while super::status(owner, s).map(|v| v[1] > 0).unwrap_or(false) {
                    if crate::syscall::signal::pending() {
                        return crate::syscall::signal::EINTR;
                    }
                    task::sleep_ticks(task::ms_to_ticks(10));
                }
            }
            0
        }
        2 => {
            let rate = match read_int(arg) {
                Ok(v) => v.clamp(4000, 192_000) as u32,
                Err(e) => return e,
            };
            reset(id);
            with(|d| d.get_mut(&id).map(|x| x.rate = rate));
            put_int(arg, rate as i32)
        }
        3 | 6 => {
            let value = match read_int(arg) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let channels = if nr == 3 { if value != 0 { 2 } else { 1 } } else { value.clamp(1, 2) as u32 };
            reset(id);
            with(|d| d.get_mut(&id).map(|x| x.channels = channels));
            put_int(arg, if nr == 3 { (channels == 2) as i32 } else { channels as i32 })
        }
        4 => {
            let size = with(|d| d.get(&id).map(|x| 1i32 << x.fragment_shift)).unwrap_or(4096);
            put_int(arg, size)
        }
        5 => {
            let wanted = match read_int(arg) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let format = match wanted {
                0 => with(|d| d.get(&id).map(|x| x.format)).unwrap_or(AFMT_U8),
                AFMT_U8 | AFMT_S16_LE => {
                    reset(id);
                    with(|d| d.get_mut(&id).map(|x| x.format = wanted));
                    wanted
                }
                _ => {
                    reset(id);
                    with(|d| d.get_mut(&id).map(|x| x.format = AFMT_S16_LE));
                    AFMT_S16_LE
                }
            };
            put_int(arg, format)
        }
        8 | 20 => 0,
        10 => {
            let value = match read_int(arg) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let shift = ((value & 0xFFFF) as u32).clamp(8, 16);
            let count = (((value >> 16) & 0x7FFF) as u32).clamp(2, 64);
            with(|d| {
                if let Some(x) = d.get_mut(&id) {
                    x.fragment_shift = shift;
                    x.fragments = count;
                }
            });
            0
        }
        11 => put_int(arg, AFMT_U8 | AFMT_S16_LE),
        12 => {
            let Ok((owner, s, _, _)) = stream(id) else {
                return EINVAL;
            };
            let status = super::status(owner, s).unwrap_or([0; 8]);
            let (shift, fragments, frame) = with(|d| d.get(&id).map(|x| (x.fragment_shift, x.fragments, frame_bytes(x)))).unwrap_or((12, 16, 2));
            let room = status[3].saturating_sub(status[0]) * frame;
            let fragsize = 1u64 << shift;
            let mut info = [0u8; 16];
            info[0..4].copy_from_slice(&((room / fragsize).min(fragments as u64) as i32).to_le_bytes());
            info[4..8].copy_from_slice(&(fragments as i32).to_le_bytes());
            info[8..12].copy_from_slice(&(fragsize as i32).to_le_bytes());
            info[12..16].copy_from_slice(&(room.min(i32::MAX as u64) as i32).to_le_bytes());
            if crate::syscall::copy_out(arg, 16, &info) < 0 { EFAULT } else { 0 }
        }
        13 => EINVAL,
        14 => 0,
        15 => put_int(arg, DSP_CAP_TRIGGER | DSP_CAP_BATCH | DSP_CAP_REALTIME | 1),
        16 => {
            if request >> 30 & 1 != 0 {
                let value = match read_int(arg) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                with(|d| d.get_mut(&id).map(|x| x.trigger = value));
                if let Some((owner, s)) = with(|d| d.get(&id).and_then(|x| x.stream.map(|s| (x.owner, s)))) {
                    super::control(owner, s, super::CONTROL_PAUSE, (value & PCM_ENABLE_OUTPUT == 0) as u64);
                }
                0
            } else {
                put_int(arg, with(|d| d.get(&id).map(|x| x.trigger)).unwrap_or(PCM_ENABLE_OUTPUT))
            }
        }
        18 => {
            let frame = with(|d| d.get(&id).map(frame_bytes)).unwrap_or(2);
            let played = match with(|d| d.get(&id).and_then(|x| x.stream.map(|s| (x.owner, s)))) {
                Some((owner, s)) => super::status(owner, s).map(|v| v[6]).unwrap_or(0),
                None => 0,
            };
            let bytes = (played * frame) as i32;
            let mut info = [0u8; 12];
            info[0..4].copy_from_slice(&bytes.to_le_bytes());
            if crate::syscall::copy_out(arg, 12, &info) < 0 { EFAULT } else { 0 }
        }
        23 => {
            let frame = with(|d| d.get(&id).map(frame_bytes)).unwrap_or(2);
            let delay = match with(|d| d.get(&id).and_then(|x| x.stream.map(|s| (x.owner, s)))) {
                Some((owner, s)) => super::status(owner, s).map(|v| v[1]).unwrap_or(0),
                None => 0,
            };
            put_int(arg, (delay * frame).min(i32::MAX as u64) as i32)
        }
        24 => {
            if request >> 30 & 1 != 0 {
                let value = match read_int(arg) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                let level = ((value & 0xFF) as u32).min(100);
                with(|d| d.get_mut(&id).map(|x| x.volume = level));
                if let Some((owner, s)) = with(|d| d.get(&id).and_then(|x| x.stream.map(|s| (x.owner, s)))) {
                    super::control(owner, s, super::CONTROL_VOLUME, level as u64);
                }
                put_int(arg, (level | level << 8) as i32)
            } else {
                let level = with(|d| d.get(&id).map(|x| x.volume)).unwrap_or(100);
                put_int(arg, (level | level << 8) as i32)
            }
        }
        _ => EINVAL,
    }
}

fn mixer_ioctl(request: u64, arg: u64) -> i64 {
    let kind = (request >> 8) & 0xFF;
    let nr = (request & 0xFF) as u32;
    let write = request >> 30 & 1 != 0;
    if kind != b'M' as u64 {
        return ENOTTY;
    }
    match nr {
        MIXER_VOLUME | MIXER_PCM if write => {
            let value = match read_int(arg) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let level = (((value & 0xFF) + ((value >> 8) & 0xFF)) / 2).clamp(0, 100) as u32;
            super::set_volume(level);
            put_int(arg, (level | level << 8) as i32)
        }
        MIXER_SPEAKER if write => {
            let value = match read_int(arg) {
                Ok(v) => v,
                Err(e) => return e,
            };
            super::set_muted(value & 0xFF == 0);
            put_int(arg, if super::muted() { 0 } else { 100 | 100 << 8 })
        }
        MIXER_VOLUME | MIXER_PCM => {
            let level = super::volume();
            put_int(arg, (level | level << 8) as i32)
        }
        MIXER_SPEAKER => put_int(arg, if super::muted() { 0 } else { 100 | 100 << 8 }),
        0xFE | 0xFB => put_int(arg, DEVMASK),
        0xFD | 0xFF => put_int(arg, 0),
        0xFC => put_int(arg, 0),
        101 => {
            let mut info = [0u8; 92];
            info[0..3].copy_from_slice(b"hda");
            let name = b"HamixOS mixer";
            info[16..16 + name.len()].copy_from_slice(name);
            info[48..52].copy_from_slice(&(super::changes() as i32).to_le_bytes());
            if crate::syscall::copy_out(arg, 92, &info) < 0 { EFAULT } else { 0 }
        }
        118 => put_int(arg, 0x040000),
        _ => EINVAL,
    }
}
