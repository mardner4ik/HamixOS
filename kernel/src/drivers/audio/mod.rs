pub mod ac97;
pub mod hda;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::hxinit::{self, ok, skip, warn};
use crate::task::{self, Pid};

pub trait AudioDevice: Send {
    fn name(&self) -> String;
    fn driver(&self) -> &'static str;
    fn rate(&self) -> u32;
    fn ring_frames(&self) -> usize;
    fn ring(&mut self) -> &mut [i16];
    fn position(&mut self) -> usize;
    fn start(&mut self);
    fn stop(&mut self);
    fn keep_alive(&mut self) {}
    fn poll(&mut self) {}
    fn outputs(&self) -> String {
        String::new()
    }
}

pub const ENODEV: i64 = -19;
pub const EINVAL: i64 = -22;
pub const EBADF: i64 = -9;
pub const EBUSY: i64 = -16;

const ONE: u64 = 1 << 32;
const MAX_STREAMS: usize = 16;
const LATENCY_MS: usize = 60;
const MAX_LATENCY_MS: usize = 250;
const IDLE_STOP_MS: u64 = 3000;
const MIX_CHUNK: usize = 512;

pub const CONTROL_PAUSE: u64 = 1;
pub const CONTROL_FLUSH: u64 = 2;
pub const CONTROL_VOLUME: u64 = 3;
pub const CONTROL_DRAIN: u64 = 4;

static DEVICE: Mutex<Option<Box<dyn AudioDevice>>> = Mutex::new(None);
static MIXER: Mutex<Mixer> = Mutex::new(Mixer::new());
static VOLUME: AtomicU32 = AtomicU32::new(75);
static MUTED: AtomicBool = AtomicBool::new(false);
static CHANGES: AtomicU64 = AtomicU64::new(1);
static PRESENT: AtomicBool = AtomicBool::new(false);
static NEXT_STREAM: AtomicU32 = AtomicU32::new(1);

struct Stream {
    id: u32,
    owner: Pid,
    rate: u32,
    channels: u16,
    queue: VecDeque<i16>,
    capacity: usize,
    volume: u32,
    paused: bool,
    draining: bool,
    step: u64,
    phase: u64,
    prev: [i32; 2],
    cur: [i32; 2],
    written: u64,
    consumed: u64,
    underruns: u64,
    primed: bool,
    tail: u64,
}

impl Stream {
    fn frames(&self) -> usize {
        self.queue.len() / 2
    }

    fn mix_into(&mut self, out: &mut [i32], frames: usize, base: u64) -> bool {
        if self.paused {
            return false;
        }
        let gain = (self.volume.min(100) as i64 * 65536 / 100) as i32;
        let mut produced = false;
        for i in 0..frames {
            self.phase += self.step;
            let mut starved = false;
            while self.phase >= ONE {
                if self.queue.len() < 2 {
                    starved = true;
                    break;
                }
                self.prev = self.cur;
                let l = self.queue.pop_front().unwrap_or(0) as i32;
                let r = self.queue.pop_front().unwrap_or(0) as i32;
                self.cur = [l, r];
                self.consumed += 1;
                self.phase -= ONE;
            }
            if starved {
                self.phase = ONE;
                if self.primed && !self.draining {
                    self.underruns += 1;
                }
                self.primed = false;
                break;
            }
            self.primed = true;
            let frac = (self.phase >> 16) as i64;
            for c in 0..2 {
                let a = self.prev[c] as i64;
                let b = self.cur[c] as i64;
                let v = a + ((b - a) * frac >> 16);
                out[i * 2 + c] += ((v * gain as i64) >> 16) as i32;
            }
            produced = true;
            self.tail = base + i as u64 + 1;
        }
        produced
    }
}

struct Mixer {
    streams: Vec<Stream>,
    running: bool,
    rate: u32,
    ring_frames: usize,
    last_position: usize,
    played: u64,
    written: u64,
    xruns: u64,
    idle_since: u64,
    last_poll: u64,
    latency_ms: usize,
}

impl Mixer {
    const fn new() -> Self {
        Mixer { streams: Vec::new(), running: false, rate: 48000, ring_frames: 0, last_position: 0, played: 0, written: 0, xruns: 0, idle_since: 0, last_poll: 0, latency_ms: LATENCY_MS }
    }

    fn stream(&mut self, id: u32, owner: Pid) -> Option<&mut Stream> {
        self.streams.iter_mut().find(|s| s.id == id && s.owner == owner)
    }
}

fn with_mixer<R>(f: impl FnOnce(&mut Mixer) -> R) -> R {
    without_interrupts(|| f(&mut MIXER.lock()))
}

fn with_device<R>(f: impl FnOnce(&mut dyn AudioDevice) -> R) -> Option<R> {
    without_interrupts(|| DEVICE.lock().as_mut().map(|d| f(d.as_mut())))
}

pub fn present() -> bool {
    PRESENT.load(Ordering::Relaxed)
}

pub fn volume() -> u32 {
    VOLUME.load(Ordering::Relaxed)
}

pub fn muted() -> bool {
    MUTED.load(Ordering::Relaxed)
}

pub fn changes() -> u64 {
    CHANGES.load(Ordering::Relaxed)
}

pub fn set_volume(level: u32) {
    VOLUME.store(level.min(100), Ordering::Relaxed);
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

pub fn set_muted(muted: bool) {
    MUTED.store(muted, Ordering::Relaxed);
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

pub fn step_volume(delta: i32) {
    let level = (VOLUME.load(Ordering::Relaxed) as i32 + delta).clamp(0, 100) as u32;
    VOLUME.store(level, Ordering::Relaxed);
    if delta > 0 {
        MUTED.store(false, Ordering::Relaxed);
    }
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

pub fn toggle_mute() {
    MUTED.fetch_xor(true, Ordering::Relaxed);
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

pub fn volume_word() -> i64 {
    ((changes() as i64 & 0xFFFF_FFFF) << 16) | ((muted() as i64) << 8) | volume() as i64
}

fn master_gain() -> i64 {
    if muted() {
        return 0;
    }
    let v = volume() as i64;
    v * v * v * 65536 / 1_000_000
}

pub fn open(owner: Pid, rate: u32, channels: u32, capacity_ms: u32) -> i64 {
    if !present() {
        return ENODEV;
    }
    if !(4000..=192_000).contains(&rate) || !(1..=2).contains(&channels) {
        return EINVAL;
    }
    let capacity_ms = if capacity_ms == 0 { 1000 } else { capacity_ms.clamp(100, 10_000) };
    with_mixer(|m| {
        if m.streams.len() >= MAX_STREAMS {
            return EBUSY;
        }
        let id = NEXT_STREAM.fetch_add(1, Ordering::Relaxed);
        let capacity = rate as usize * capacity_ms as usize / 1000;
        let dev_rate = m.rate.max(1) as u64;
        m.streams.push(Stream {
            id,
            owner,
            rate,
            channels: channels as u16,
            queue: VecDeque::with_capacity(capacity.min(96_000) * 2),
            capacity,
            volume: 100,
            paused: false,
            draining: false,
            step: ((rate as u64) << 32) / dev_rate,
            phase: 0,
            prev: [0; 2],
            cur: [0; 2],
            written: 0,
            consumed: 0,
            underruns: 0,
            primed: false,
            tail: 0,
        });
        id as i64
    })
}

pub fn write(owner: Pid, id: u32, samples: &[u8], block: bool) -> i64 {
    loop {
        let result = with_mixer(|m| {
            let Some(stream) = m.stream(id, owner) else {
                return Err(EBADF);
            };
            let bytes_per_frame = stream.channels as usize * 2;
            let frames = samples.len() / bytes_per_frame;
            let room = stream.capacity.saturating_sub(stream.frames());
            let take = frames.min(room);
            if take == 0 && frames > 0 {
                return Ok(None);
            }
            for f in 0..take {
                let at = f * bytes_per_frame;
                let l = i16::from_le_bytes([samples[at], samples[at + 1]]);
                let r = if stream.channels == 2 { i16::from_le_bytes([samples[at + 2], samples[at + 3]]) } else { l };
                stream.queue.push_back(l);
                stream.queue.push_back(r);
            }
            stream.written += take as u64;
            stream.draining = false;
            Ok(Some(take as i64))
        });
        match result {
            Err(e) => return e,
            Ok(Some(n)) => {
                kick();
                return n;
            }
            Ok(None) if !block => return 0,
            Ok(None) => {
                kick();
                task::sleep_ticks(task::ms_to_ticks(5));
                task::check_killed();
            }
        }
    }
}

pub fn status(owner: Pid, id: u32) -> Option<[u64; 8]> {
    with_mixer(|m| {
        let played = m.played;
        let dev_rate = m.rate.max(1) as u64;
        let running = m.running;
        let xruns = m.xruns;
        let stream = m.stream(id, owner)?;
        let queued = stream.frames() as u64;
        let hw = if running { stream.tail.saturating_sub(played) * stream.rate as u64 / dev_rate } else { 0 };
        let delay = queued + hw.min(stream.consumed);
        Some([queued, delay, stream.written, stream.capacity as u64, stream.underruns, dev_rate, stream.written.saturating_sub(delay), xruns])
    })
}

pub fn control(owner: Pid, id: u32, op: u64, value: u64) -> i64 {
    with_mixer(|m| {
        let Some(stream) = m.stream(id, owner) else {
            return EBADF;
        };
        match op {
            CONTROL_PAUSE => stream.paused = value != 0,
            CONTROL_FLUSH => {
                stream.queue.clear();
                stream.phase = 0;
                stream.prev = [0; 2];
                stream.cur = [0; 2];
                stream.primed = false;
            }
            CONTROL_VOLUME => stream.volume = value.min(100) as u32,
            CONTROL_DRAIN => stream.draining = true,
            _ => return EINVAL,
        }
        0
    })
}

pub fn close(owner: Pid, id: u32, drain: bool) -> i64 {
    with_mixer(|m| {
        let Some(index) = m.streams.iter().position(|s| s.id == id && s.owner == owner) else {
            return EBADF;
        };
        if drain && m.streams[index].frames() > 0 {
            m.streams[index].draining = true;
            m.streams[index].owner = 0;
        } else {
            m.streams.remove(index);
        }
        0
    })
}

pub fn cleanup(pid: Pid) {
    if pid == 0 {
        return;
    }
    with_mixer(|m| m.streams.retain(|s| s.owner != pid));
}

fn kick() {
    let pid = AUDIOD.load(Ordering::Relaxed);
    if pid != 0 && !with_mixer(|m| m.running) {
        task::wake(pid, task::WAIT_TIMER);
    }
}

static AUDIOD: AtomicU32 = AtomicU32::new(0);

fn fill(dev: &mut dyn AudioDevice, m: &mut Mixer, scratch: &mut Vec<i32>) {
    let ring = m.ring_frames;
    let position = dev.position() % ring.max(1);
    let delta = (position + ring - m.last_position) % ring;
    if delta < ring * 3 / 4 {
        m.last_position = position;
        m.played += delta as u64;
    }
    if m.written < m.played {
        m.xruns += 1;
        m.latency_ms = (m.latency_ms + 40).min(MAX_LATENCY_MS.min(m.ring_frames * 1000 / m.rate.max(1) as usize / 2));
        m.written = m.played + (m.rate as u64 * 10 / 1000);
    }
    let target = m.played + (m.rate as usize * m.latency_ms / 1000) as u64;
    let gain = master_gain();
    while m.written < target {
        let frames = ((target - m.written) as usize).min(MIX_CHUNK);
        scratch.clear();
        scratch.resize(frames * 2, 0);
        for stream in m.streams.iter_mut() {
            stream.mix_into(scratch, frames, m.written);
        }
        let start = (m.written % ring as u64) as usize;
        let out = dev.ring();
        for i in 0..frames {
            let slot = (start + i) % ring;
            for c in 0..2 {
                let v = (scratch[i * 2 + c] as i64 * gain) >> 16;
                out[slot * 2 + c] = v.clamp(-32768, 32767) as i16;
            }
        }
        m.written += frames as u64;
    }
    m.streams.retain(|s| !(s.owner == 0 && s.frames() == 0));
    dev.keep_alive();
}

extern "C" fn daemon(_: u64) -> ! {
    let mut scratch: Vec<i32> = Vec::with_capacity(MIX_CHUNK * 2);
    loop {
        let now = task::uptime_ms();
        let busy = {
            let mut device = DEVICE.lock();
            let Some(dev) = device.as_mut() else {
                drop(device);
                task::sleep_ticks(task::TICK_HZ);
                continue;
            };
            let mut m = MIXER.lock();
            if now.saturating_sub(m.last_poll) >= 500 {
                m.last_poll = now;
                dev.poll();
            }
            let wanted = !m.streams.is_empty();
            if wanted {
                m.idle_since = now;
            }
            if wanted && !m.running {
                m.ring_frames = dev.ring_frames();
                m.rate = dev.rate();
                dev.ring().fill(0);
                dev.start();
                m.running = true;
                m.last_position = dev.position() % m.ring_frames.max(1);
                m.played = 0;
                m.written = (m.rate as u64 * 20) / 1000;
            }
            if m.running {
                fill(dev.as_mut(), &mut m, &mut scratch);
                if !wanted && now.saturating_sub(m.idle_since) > IDLE_STOP_MS {
                    dev.stop();
                    m.running = false;
                }
            }
            m.running
        };
        task::sleep_ticks(task::ms_to_ticks(if busy { 5 } else { 50 }));
    }
}

static MODULE_DEVICE: Mutex<Option<Box<dyn AudioDevice>>> = Mutex::new(None);

pub fn register_module_device(dev: Box<dyn AudioDevice>) {
    if PRESENT.load(Ordering::Relaxed) {
        crate::drivers::klog::log("audio: a sound device is already active, the module device waits for the next boot");
        return;
    }
    *MODULE_DEVICE.lock() = Some(dev);
}

pub fn init_unit() {
    hxinit::run("audio", "Sound", || {
        let pci = crate::drivers::pci::devices();
        let mut found: Option<Box<dyn AudioDevice>> = MODULE_DEVICE.lock().take();
        if found.is_none() {
            found = hda::probe(&pci);
        }
        if found.is_none() {
            found = ac97::probe(&pci);
        }
        let unsupported: Vec<String> = pci.iter().filter(|d| d.class == 0x04).map(|d| format!("{:04x}:{:04x}", d.vendor, d.device)).collect();
        let Some(dev) = found else {
            return if unsupported.is_empty() { skip("no sound controller") } else { warn(format!("no driver for {}", unsupported.join(", "))) };
        };
        let line = format!("{} ({}), {} Hz{}", dev.name(), dev.driver(), dev.rate(), {
            let outputs = dev.outputs();
            if outputs.is_empty() { String::new() } else { format!(", {}", outputs) }
        });
        with_mixer(|m| {
            m.rate = dev.rate();
            m.ring_frames = dev.ring_frames();
        });
        without_interrupts(|| *DEVICE.lock() = Some(dev));
        load_saved_volume();
        PRESENT.store(true, Ordering::Relaxed);
        if let Some(pid) = task::spawn_kernel_thread("audiod", 0, daemon, 0) {
            AUDIOD.store(pid, Ordering::Relaxed);
        }
        ok(line)
    });
}

fn load_saved_volume() {
    let Some(text) = crate::fs::VFS.lock().as_mut().and_then(|v| v.read(0, "/etc/hamix/audio.conf").ok()) else {
        return;
    };
    let text = String::from_utf8_lossy(&text).into_owned();
    for line in text.lines() {
        if let Some(v) = line.trim().strip_prefix("volume=") {
            if let Ok(level) = v.trim().parse::<u32>() {
                VOLUME.store(level.min(100), Ordering::Relaxed);
            }
        }
        if let Some(v) = line.trim().strip_prefix("muted=") {
            MUTED.store(v.trim() == "yes", Ordering::Relaxed);
        }
    }
}

pub fn info_text() -> String {
    let (name, driver, rate, outputs) = with_device(|d| (d.name(), d.driver(), d.rate(), d.outputs())).unwrap_or_default();
    let mut out = format!(
        "device\t{}\ndriver\t{}\nrate\t{}\noutputs\t{}\nvolume\t{}\nmuted\t{}\n",
        name,
        driver,
        rate,
        outputs,
        volume(),
        if muted() { "yes" } else { "no" }
    );
    with_mixer(|m| {
        out.push_str(&format!("running\t{}\nxruns\t{}\nlatency\t{}\n", if m.running { "yes" } else { "no" }, m.xruns, m.latency_ms));
        for s in m.streams.iter() {
            out.push_str(&format!("stream\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", s.id, s.owner, s.rate, s.channels, s.frames(), if s.paused { "paused" } else { "playing" }, s.underruns));
        }
    });
    out
}

pub fn hotkey(scancode: u8) -> bool {
    if !present() {
        return false;
    }
    match scancode {
        0x20 => toggle_mute(),
        0x2E => step_volume(-5),
        0x30 => step_volume(5),
        _ => return false,
    }
    true
}
