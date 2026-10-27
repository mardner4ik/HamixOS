#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use hamix_std::audio::{self, Stream};
use hamix_std::{entry, env, eprintln, fs, println, sys};

fn usage() -> i32 {
    println!("usage: hxsound info");
    println!("       hxsound volume [0-100 | +N | -N | mute | unmute | toggle]");
    println!("       hxsound tone [frequency] [seconds]");
    println!("       hxsound play file.wav");
    1
}

fn sine(phase: f32) -> f32 {
    let x = phase - (phase as i32) as f32;
    let t = if x < 0.5 { x * 2.0 } else { (x - 0.5) * 2.0 };
    let y = 4.0 * t * (1.0 - t);
    let refined = 0.225 * (y * y - y) + y;
    if x < 0.5 { refined } else { -refined }
}

fn show_volume() {
    match audio::volume() {
        Some(v) => println!("volume {}%{}", v.level, if v.muted { " (muted)" } else { "" }),
        None => println!("no sound device"),
    }
}

fn info() -> i32 {
    let info = audio::info();
    if !info.available {
        println!("no sound device");
        return 1;
    }
    println!("device   {}", info.device);
    println!("driver   {}", info.driver);
    println!("rate     {} Hz", info.rate);
    println!("outputs  {}", info.outputs);
    println!("state    {}, {} stream(s)", if info.running { "playing" } else { "idle" }, info.streams);
    show_volume();
    0
}

fn volume(arg: Option<&str>) -> i32 {
    let result = match arg {
        None => audio::volume(),
        Some("mute") => audio::set_muted(true),
        Some("unmute") => audio::set_muted(false),
        Some("toggle") => audio::toggle_mute(),
        Some(v) if v.starts_with('+') || v.starts_with('-') => match v.parse::<i32>() {
            Ok(delta) => audio::step_volume(delta),
            Err(_) => return usage(),
        },
        Some(v) => match v.parse::<u32>() {
            Ok(level) => audio::set_volume(level),
            Err(_) => return usage(),
        },
    };
    if result.is_none() {
        eprintln!("hxsound: no sound device");
        return 1;
    }
    show_volume();
    0
}

fn tone(frequency: f32, seconds: f32) -> i32 {
    let rate = 48000u32;
    let stream = match Stream::open(rate, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("hxsound: cannot open the sound device ({})", sys::error_name(e));
            return 1;
        }
    };
    let total = (seconds * rate as f32) as usize;
    let mut phase = 0.0f32;
    let step = frequency / rate as f32;
    let mut done = 0;
    let mut chunk = Vec::with_capacity(4096);
    while done < total {
        chunk.clear();
        let frames = (total - done).min(2048);
        for i in 0..frames {
            let fade = ((done + i).min(total - done - i) as f32 / 480.0).min(1.0);
            let v = (sine(phase) * 12000.0 * fade) as i16;
            chunk.push(v);
            chunk.push(v);
            phase += step;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
        if stream.write_all(&chunk).is_err() {
            return 1;
        }
        done += frames;
    }
    stream.drain();
    0
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn play(path: &str) -> i32 {
    let Some(data) = fs::read(path) else {
        eprintln!("hxsound: cannot read {}", path);
        return 1;
    };
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        eprintln!("hxsound: {} is not a WAV file", path);
        return 1;
    }
    let mut at = 12;
    let (mut channels, mut rate, mut bits, mut format) = (0u16, 0u32, 0u16, 0u16);
    let mut pcm: Option<&[u8]> = None;
    while at + 8 <= data.len() {
        let id = &data[at..at + 4];
        let size = le32(&data, at + 4) as usize;
        let body = at + 8;
        let end = (body + size).min(data.len());
        if id == b"fmt " && end - body >= 16 {
            format = le16(&data, body);
            channels = le16(&data, body + 2);
            rate = le32(&data, body + 4);
            bits = le16(&data, body + 14);
        } else if id == b"data" {
            pcm = Some(&data[body..end]);
        }
        at = body + size + (size & 1);
    }
    let Some(pcm) = pcm else {
        eprintln!("hxsound: {} has no audio data", path);
        return 1;
    };
    if !(format == 1 || format == 0xFFFE) || !(bits == 16 || bits == 8) || !(1..=2).contains(&channels) {
        eprintln!("hxsound: only 8/16-bit PCM mono or stereo WAV files are supported");
        return 1;
    }
    let stream = match Stream::open(rate, channels as u32) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("hxsound: cannot open the sound device ({})", sys::error_name(e));
            return 1;
        }
    };
    println!("playing {} ({} Hz, {} bit, {} channel{})", path, rate, bits, channels, if channels == 1 { "" } else { "s" });
    let samples: Vec<i16> = if bits == 16 { pcm.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect() } else { pcm.iter().map(|b| ((*b as i16) - 128) << 8).collect() };
    for chunk in samples.chunks(channels as usize * 4096) {
        if stream.write_all(chunk).is_err() {
            return 1;
        }
    }
    stream.drain();
    0
}

fn main() -> i32 {
    let args = env::args();
    match args.get(1).map(|s| s.as_str()) {
        Some("info") | None => info(),
        Some("volume") => volume(args.get(2).map(|s| s.as_str())),
        Some("tone") => {
            let frequency = args.get(2).and_then(|v| v.parse::<u32>().ok()).unwrap_or(440) as f32;
            let seconds = args.get(3).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1) as f32;
            tone(frequency, seconds)
        }
        Some("play") => match args.get(2) {
            Some(path) => play(path),
            None => usage(),
        },
        _ => usage(),
    }
}

entry!(main);
