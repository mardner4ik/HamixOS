use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::audio;
use hamix_std::sys;
use hx264::Picture;
use hxmp4::{Codec, Movie, Source, Track};

const AUDIO_AHEAD_MS: u64 = 350;
const SKIP_ENTER_MS: i64 = 150;
const SKIP_LEAVE_MS: i64 = 20;
const DECODE_BUDGET_MS: u64 = 12;
const CLOCK_SNAP_MS: i64 = 250;
const LOOKAHEAD_BUDGET: usize = 24 * 1024 * 1024;

pub struct FileSource {
    fd: u64,
    size: u64,
    position: u64,
}

impl FileSource {
    pub fn open(path: &str) -> Result<FileSource, String> {
        let fd = sys::open_with(path, sys::O_RDONLY);
        if fd < 0 {
            return Err(format!("Cannot open the file ({})", sys::error_name(fd)));
        }
        let size = sys::fstat_size(fd as u64);
        if size < 0 {
            sys::close(fd as u64);
            return Err(String::from("Cannot read the file size"));
        }
        Ok(FileSource { fd: fd as u64, size: size as u64, position: 0 })
    }
}

impl Drop for FileSource {
    fn drop(&mut self) {
        sys::close(self.fd);
    }
}

impl Source for FileSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        if offset != self.position {
            if sys::lseek(self.fd, offset as i64, 0) < 0 {
                return 0;
            }
            self.position = offset;
        }
        let mut done = 0;
        while done < buf.len() {
            let n = sys::read(self.fd, &mut buf[done..]);
            if n <= 0 {
                break;
            }
            done += n as usize;
        }
        self.position += done as u64;
        done
    }
}

struct VideoState {
    track: usize,
    decoder: hx264::Decoder,
    next: usize,
    queue: Vec<Picture>,
    reorder: usize,
    lookahead: usize,
    frame_ms: i64,
    ended: bool,
    skipping: bool,
    decoded: u64,
    dropped: u64,
    skipped: u64,
}

struct AudioState {
    track: usize,
    decoder: hxaac::Decoder,
    stream: Option<audio::Stream>,
    next: usize,
    rate: u32,
    channels: u32,
    base_ms: i64,
    pending: Vec<i16>,
    ended: bool,
    written_frames: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Playing,
    Paused,
    Ended,
}

pub struct Player {
    pub source: FileSource,
    pub movie: Movie,
    video: Option<VideoState>,
    audio: Option<AudioState>,
    pub state: State,
    pub duration_ms: i64,
    clock_base_ms: i64,
    clock_wall: u64,
    paused_at_ms: i64,
    seek_target: Option<i64>,
    sample: Vec<u8>,
    pub notes: Vec<String>,
    pub current: Option<Picture>,
    pub new_frame: bool,
    audio_released: bool,
    clock_offset: Option<i64>,
}

fn reorder_depth(track: &Track) -> usize {
    let n = track.samples.len().min(600);
    let mut rank: Vec<usize> = (0..n).collect();
    rank.sort_by_key(|i| track.samples[*i].pts);
    let mut depth = 0usize;
    for (display, decode) in rank.iter().enumerate() {
        depth = depth.max(decode.saturating_sub(display));
        depth = depth.max(display.saturating_sub(*decode));
    }
    depth.clamp(1, 16)
}

impl Player {
    pub fn open(path: &str) -> Result<Player, String> {
        let mut source = FileSource::open(path)?;
        let movie = hxmp4::open(&mut source).map_err(|e| match e {
            hxmp4::Error::NotMp4 => String::from("This is not an MP4 file"),
            hxmp4::Error::NoMovie => String::from("The file has no playable tracks"),
            hxmp4::Error::TooLarge => String::from("The file index is too large"),
            _ => String::from("The MP4 file is damaged or cannot be read"),
        })?;
        let mut notes = Vec::new();
        let mut video = None;
        if let Some(index) = movie.video_index() {
            let track = &movie.tracks[index];
            match &track.codec {
                Codec::Avc { config, .. } => match hx264::Decoder::with_avcc(config) {
                    Ok(decoder) => {
                        let frame_ms = track.samples.get(1).map(|s| (track.to_ms(s.dts as i64) - track.to_ms(track.samples[0].dts as i64)).max(1)).unwrap_or(40);
                        let lookahead = match &track.codec {
                            Codec::Avc { width, height, .. } => (LOOKAHEAD_BUDGET / ((*width as usize * *height as usize * 3 / 2).max(1))).clamp(2, 12),
                            _ => 4,
                        };
                        video = Some(VideoState { track: index, decoder, next: 0, queue: Vec::new(), reorder: reorder_depth(track), lookahead, frame_ms, ended: false, skipping: false, decoded: 0, dropped: 0, skipped: 0 });
                    }
                    Err(_) => notes.push(String::from("The H.264 configuration is invalid")),
                },
                other => notes.push(format!("{} video is not supported", other.name())),
            }
        }
        let mut audio_state = None;
        if let Some(index) = movie.audio_index() {
            let track = &movie.tracks[index];
            match &track.codec {
                Codec::Aac { config, .. } => match hxaac::Decoder::new(config) {
                    Ok(decoder) => {
                        let (rate, channels) = (decoder.sample_rate(), decoder.output_channels());
                        if decoder.info().he_aac {
                            notes.push(String::from("HE-AAC audio is played without its high band"));
                        }
                        audio_state = Some(AudioState { track: index, decoder, stream: None, next: 0, rate, channels, base_ms: 0, pending: Vec::new(), ended: false, written_frames: 0 });
                    }
                    Err(_) => notes.push(String::from("This AAC audio profile is not supported")),
                },
                other => notes.push(format!("{} audio is not supported", other.name())),
            }
        }
        if video.is_none() && audio_state.is_none() {
            return Err(if notes.is_empty() { String::from("Nothing in this file can be played") } else { notes.join(", ") });
        }
        let duration_ms = movie.duration_ms as i64;
        let mut player = Player { source, movie, video, audio: audio_state, state: State::Paused, duration_ms, clock_base_ms: 0, clock_wall: sys::uptime_ms(), paused_at_ms: 0, seek_target: None, sample: Vec::new(), notes, current: None, new_frame: false, audio_released: false, clock_offset: None };
        if let Some(a) = &mut player.audio {
            match audio::Stream::open_with_buffer(a.rate, a.channels, 1500) {
                Ok(stream) => a.stream = Some(stream),
                Err(_) => player.notes.push(String::from("No sound device, playing without sound")),
            }
        }
        player.seek(0);
        Ok(player)
    }

    pub fn has_audio(&self) -> bool {
        self.audio.as_ref().map(|a| a.stream.is_some()).unwrap_or(false)
    }

    pub fn video_size(&self) -> Option<(u32, u32)> {
        let v = self.video.as_ref()?;
        match &self.movie.tracks[v.track].codec {
            Codec::Avc { width, height, .. } => Some((*width, *height)),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = &self.video {
            let track = &self.movie.tracks[v.track];
            if let Codec::Avc { width, height, profile, .. } = &track.codec {
                parts.push(format!("H.264 {} {}×{} {:.0} fps", hx264::profile_name(*profile), width, height, track.average_rate()));
            }
        }
        if let Some(a) = &self.audio {
            parts.push(format!("AAC {} kHz {}", a.rate / 1000, if a.channels == 1 { "mono" } else { "stereo" }));
        }
        parts.join(" · ")
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        self.video.as_ref().map(|v| (v.decoded, v.dropped, v.skipped)).unwrap_or((0, 0, 0))
    }

    pub fn position_ms(&self) -> i64 {
        if let Some(target) = self.seek_target {
            return target;
        }
        match self.state {
            State::Playing => self.clock_ms(),
            _ => self.paused_at_ms,
        }
        .clamp(0, self.duration_ms.max(0))
    }

    fn audio_clock(&self) -> Option<i64> {
        let a = self.audio.as_ref()?;
        let stream = a.stream.as_ref()?;
        let status = stream.status();
        if a.ended && a.pending.is_empty() && status.delay == 0 {
            return None;
        }
        let played = a.written_frames.saturating_sub(status.delay.min(a.written_frames));
        Some(a.base_ms + (played * 1000 / a.rate as u64) as i64)
    }

    fn clock_ms(&self) -> i64 {
        if !self.audio_released {
            if let Some(offset) = self.clock_offset {
                return sys::uptime_ms() as i64 + offset;
            }
            if let Some(ms) = self.audio_clock() {
                return ms;
            }
        }
        self.clock_base_ms + sys::uptime_ms().saturating_sub(self.clock_wall) as i64
    }

    fn advance_clock(&mut self) {
        if self.audio_released {
            self.clock_offset = None;
            return;
        }
        let Some(measured) = self.audio_clock() else {
            self.clock_offset = None;
            return;
        };
        let target = measured - sys::uptime_ms() as i64;
        self.clock_offset = Some(match self.clock_offset {
            Some(current) if (target - current).abs() < CLOCK_SNAP_MS => current + (target - current) / 8,
            _ => target,
        });
    }

    fn release_audio_clock(&mut self) {
        if self.audio_released || self.state != State::Playing {
            return;
        }
        let Some(a) = &self.audio else {
            return;
        };
        if a.stream.is_none() || self.audio_clock().is_some() {
            return;
        }
        self.clock_base_ms = a.base_ms + (a.written_frames * 1000 / a.rate.max(1) as u64) as i64;
        self.clock_wall = sys::uptime_ms();
        self.audio_released = true;
    }

    pub fn play(&mut self) {
        if self.state == State::Ended {
            self.seek(0);
        }
        if self.state == State::Playing {
            return;
        }
        self.clock_base_ms = self.paused_at_ms;
        self.clock_wall = sys::uptime_ms();
        self.clock_offset = None;
        if let Some(stream) = self.audio.as_ref().and_then(|a| a.stream.as_ref()) {
            stream.set_paused(false);
        }
        self.state = State::Playing;
    }

    pub fn pause(&mut self) {
        if self.state != State::Playing {
            return;
        }
        self.paused_at_ms = self.clock_ms();
        if let Some(stream) = self.audio.as_ref().and_then(|a| a.stream.as_ref()) {
            stream.set_paused(true);
        }
        self.state = State::Paused;
    }

    pub fn toggle(&mut self) {
        match self.state {
            State::Playing => self.pause(),
            _ => self.play(),
        }
    }

    pub fn seek(&mut self, target: i64) {
        let target = target.clamp(0, (self.duration_ms - 50).max(0));
        let was_playing = self.state == State::Playing;
        self.seek_target = Some(target);
        if let Some(v) = &mut self.video {
            let track = &self.movie.tracks[v.track];
            v.next = track.keyframe_for_ms(target);
            v.queue.clear();
            v.ended = false;
            v.skipping = false;
            v.decoder.reset();
        }
        if let Some(a) = &mut self.audio {
            let track = &self.movie.tracks[a.track];
            let index = track.index_at_ms(target);
            a.next = index.saturating_sub(1);
            a.base_ms = track.sample_ms(index);
            a.decoder.reset();
            a.pending.clear();
            a.ended = false;
            a.written_frames = 0;
            if let Some(old) = a.stream.take() {
                old.flush();
                drop(old);
                a.stream = audio::Stream::open_with_buffer(a.rate, a.channels, 1500).ok();
                if !was_playing {
                    if let Some(stream) = &a.stream {
                        stream.set_paused(true);
                    }
                }
            }
            let skip_frames = if a.next < index { 1 } else { 0 };
            if skip_frames > 0 {
                let mut discard = Vec::new();
                let _ = track.read_sample(&mut self.source, a.next, &mut self.sample);
                let _ = a.decoder.decode(&self.sample, &mut discard);
                a.next += 1;
            }
        }
        self.audio_released = false;
        self.clock_offset = None;
        self.paused_at_ms = target;
        self.clock_base_ms = target;
        self.clock_wall = sys::uptime_ms();
        if self.state == State::Ended {
            self.state = State::Paused;
        }
        if self.video.is_none() {
            self.seek_target = None;
        }
    }

    fn pump_audio(&mut self) {
        let Some(a) = &mut self.audio else {
            return;
        };
        let Some(stream) = &a.stream else {
            return;
        };
        let track = &self.movie.tracks[a.track];
        let per_frame = a.channels as usize;
        loop {
            if !a.pending.is_empty() {
                let written = match stream.write(&a.pending) {
                    Ok(frames) => frames,
                    Err(_) => return,
                };
                a.written_frames += written as u64;
                a.pending.drain(..written * per_frame);
                if !a.pending.is_empty() {
                    return;
                }
            }
            let status = stream.status();
            if status.queued * 1000 / a.rate as u64 >= AUDIO_AHEAD_MS {
                return;
            }
            if a.next >= track.samples.len() {
                if !a.ended {
                    a.ended = true;
                    stream.set_paused(false);
                }
                return;
            }
            if track.read_sample(&mut self.source, a.next, &mut self.sample).is_err() {
                a.next += 1;
                continue;
            }
            a.next += 1;
            if a.decoder.decode(&self.sample, &mut a.pending).is_err() {
                continue;
            }
        }
    }

    fn pump_video(&mut self, clock: i64) {
        let playing = self.state == State::Playing;
        let seeking = self.seek_target;
        let Some(v) = &mut self.video else {
            return;
        };
        let track = &self.movie.tracks[v.track];
        let target_depth = v.reorder + if seeking.is_some() { 1 } else { v.lookahead };
        let started = sys::uptime_ms();
        while !v.ended && v.queue.len() <= target_depth {
            if v.next >= track.samples.len() {
                v.ended = true;
                break;
            }
            let index = v.next;
            v.next += 1;
            if track.read_sample(&mut self.source, index, &mut self.sample).is_err() {
                continue;
            }
            let pts_ms = track.to_ms(track.samples[index].pts);
            let late = match seeking {
                Some(target) => pts_ms < target - v.frame_ms,
                None => {
                    if playing {
                        let behind = clock - pts_ms;
                        if v.skipping {
                            if behind < SKIP_LEAVE_MS {
                                v.skipping = false;
                            }
                        } else if behind > SKIP_ENTER_MS {
                            v.skipping = true;
                        }
                        v.skipping && behind > 0
                    } else {
                        false
                    }
                }
            };
            if late && hx264::is_disposable(&self.sample, v.decoder.length_size()) {
                v.skipped += 1;
                continue;
            }
            match v.decoder.decode_sample(&self.sample, pts_ms) {
                Ok(Some(picture)) => {
                    v.decoded += 1;
                    let at = v.queue.partition_point(|p| p.pts <= picture.pts);
                    v.queue.insert(at, picture);
                }
                Ok(None) => {}
                Err(_) => {}
            }
            if sys::uptime_ms().saturating_sub(started) >= DECODE_BUDGET_MS && v.queue.len() > v.reorder {
                break;
            }
        }
    }

    fn take_due_frame(&mut self, clock: i64) -> Option<Picture> {
        let v = self.video.as_mut()?;
        let flush = v.ended;
        let mut chosen = None;
        while let Some(first) = v.queue.first() {
            let ready = v.queue.len() > v.reorder || flush;
            if !ready || first.pts > clock + 4 {
                break;
            }
            if chosen.is_some() {
                v.dropped += 1;
            }
            chosen = Some(v.queue.remove(0));
        }
        chosen
    }

    pub fn tick(&mut self) -> u64 {
        self.new_frame = false;
        if let Some(target) = self.seek_target {
            self.pump_video(target);
            let v = self.video.as_mut().unwrap();
            let mut found = None;
            while !v.queue.is_empty() {
                let ready = v.queue.len() > v.reorder || v.ended;
                if !ready {
                    break;
                }
                let p = v.queue.remove(0);
                if p.pts + v.frame_ms > target || v.queue.is_empty() && v.ended {
                    found = Some(p);
                    break;
                }
            }
            if let Some(p) = found {
                self.paused_at_ms = p.pts.max(target);
                self.clock_base_ms = self.paused_at_ms;
                self.clock_wall = sys::uptime_ms();
                self.current = Some(p);
                self.new_frame = true;
                self.seek_target = None;
            } else if v.ended && v.queue.is_empty() {
                self.seek_target = None;
            }
            if self.seek_target.is_some() {
                return 0;
            }
        }
        if self.state != State::Playing {
            return 50;
        }
        self.pump_audio();
        self.release_audio_clock();
        self.advance_clock();
        let clock = self.clock_ms();
        self.pump_video(clock);
        if let Some(p) = self.take_due_frame(clock) {
            self.current = Some(p);
            self.new_frame = true;
        }
        let video_done = self.video.as_ref().map(|v| v.ended && v.queue.is_empty()).unwrap_or(true);
        let audio_done = self.audio.as_ref().map(|a| a.stream.is_none() || (a.ended && a.pending.is_empty() && a.stream.as_ref().map(|s| s.status().delay == 0).unwrap_or(true))).unwrap_or(true);
        if video_done && audio_done {
            self.paused_at_ms = self.duration_ms;
            self.state = State::Ended;
            return 50;
        }
        let next_due = self.video.as_ref().and_then(|v| v.queue.first()).map(|p| (p.pts - clock).clamp(1, 20) as u64).unwrap_or(10);
        next_due.min(if self.has_audio() { 15 } else { 20 })
    }
}
