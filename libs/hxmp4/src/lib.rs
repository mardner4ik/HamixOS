#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::num::NonZeroU32;
use shiguredo_mp4::aux::SampleTableAccessor;
use shiguredo_mp4::boxes::{MoofBox, MoovBox, SampleEntry, StblBox, TrakBox, TrexBox};
use shiguredo_mp4::Decode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Io,
    NotMp4,
    NoMovie,
    Corrupt,
    TooLarge,
}

pub trait Source {
    fn size(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> usize;
}

impl Source for &[u8] {
    fn size(&self) -> u64 {
        self.len() as u64
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        if offset >= self.len() as u64 {
            return 0;
        }
        let start = offset as usize;
        let n = buf.len().min(self.len() - start);
        buf[..n].copy_from_slice(&self[start..start + n]);
        n
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Other,
}

#[derive(Clone, Debug)]
pub enum Codec {
    Avc { config: Vec<u8>, width: u32, height: u32, profile: u8, level: u8 },
    Aac { config: Vec<u8>, sample_rate: u32, channels: u32, object_type: u8 },
    Unsupported(String),
}

impl Codec {
    pub fn name(&self) -> String {
        match self {
            Codec::Avc { .. } => String::from("H.264"),
            Codec::Aac { .. } => String::from("AAC"),
            Codec::Unsupported(name) => name.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SampleInfo {
    pub offset: u64,
    pub size: u32,
    pub dts: u64,
    pub pts: i64,
    pub duration: u32,
    pub sync: bool,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: u32,
    pub kind: TrackKind,
    pub timescale: u32,
    pub duration: u64,
    pub codec: Codec,
    pub samples: Vec<SampleInfo>,
    pub edit_offset: i64,
    pub language: String,
    pub display_order: Vec<u32>,
}

impl Track {
    pub fn to_ms(&self, time: i64) -> i64 {
        (time - self.edit_offset) * 1000 / self.timescale.max(1) as i64
    }

    pub fn from_ms(&self, ms: i64) -> i64 {
        ms * self.timescale.max(1) as i64 / 1000 + self.edit_offset
    }

    pub fn sample_ms(&self, index: usize) -> i64 {
        self.samples.get(index).map(|s| self.to_ms(s.pts)).unwrap_or(0)
    }

    pub fn duration_ms(&self) -> u64 {
        let end = self.samples.iter().map(|s| s.pts + s.duration as i64).max().unwrap_or(0);
        let from_samples = self.to_ms(end).max(0) as u64;
        let declared = self.duration * 1000 / self.timescale.max(1) as u64;
        from_samples.max(declared.min(from_samples.saturating_add(1000)))
    }

    pub fn average_rate(&self) -> f32 {
        let ms = self.duration_ms();
        if ms == 0 { 0.0 } else { self.samples.len() as f32 * 1000.0 / ms as f32 }
    }

    pub fn index_at_ms(&self, ms: i64) -> usize {
        let target = self.from_ms(ms);
        let pos = self.samples.partition_point(|s| (s.dts as i64) <= target);
        pos.saturating_sub(1)
    }

    pub fn keyframe_at_or_before(&self, index: usize) -> usize {
        let mut i = index.min(self.samples.len().saturating_sub(1));
        while i > 0 && !self.samples[i].sync {
            i -= 1;
        }
        i
    }

    pub fn keyframe_for_ms(&self, ms: i64) -> usize {
        if self.samples.is_empty() {
            return 0;
        }
        let target = self.from_ms(ms);
        let mut best = 0usize;
        for (i, s) in self.samples.iter().enumerate() {
            if s.sync && s.pts <= target {
                best = i;
            }
            if s.dts as i64 > target {
                break;
            }
        }
        best
    }

    pub fn read_sample<S: Source>(&self, source: &mut S, index: usize, buf: &mut Vec<u8>) -> Result<(), Error> {
        let info = self.samples.get(index).ok_or(Error::Corrupt)?;
        if info.size > 64 * 1024 * 1024 {
            return Err(Error::TooLarge);
        }
        buf.clear();
        buf.resize(info.size as usize, 0);
        let n = source.read_at(info.offset, buf);
        if n != buf.len() {
            return Err(Error::Io);
        }
        Ok(())
    }
}

pub struct Movie {
    pub tracks: Vec<Track>,
    pub duration_ms: u64,
    pub brand: String,
}

impl Movie {
    pub fn video(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == TrackKind::Video && matches!(t.codec, Codec::Avc { .. })).or_else(|| self.tracks.iter().find(|t| t.kind == TrackKind::Video))
    }

    pub fn audio(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == TrackKind::Audio && matches!(t.codec, Codec::Aac { .. })).or_else(|| self.tracks.iter().find(|t| t.kind == TrackKind::Audio))
    }

    pub fn video_index(&self) -> Option<usize> {
        let v = self.video()?;
        self.tracks.iter().position(|t| t.id == v.id)
    }

    pub fn audio_index(&self) -> Option<usize> {
        let a = self.audio()?;
        self.tracks.iter().position(|t| t.id == a.id)
    }
}

fn read_exact<S: Source>(source: &mut S, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    if source.read_at(offset, buf) == buf.len() { Ok(()) } else { Err(Error::Io) }
}

pub fn probe(head: &[u8]) -> bool {
    head.len() >= 8 && matches!(&head[4..8], b"ftyp" | b"moov" | b"mdat" | b"free" | b"skip" | b"wide" | b"pnot")
}

pub fn open<S: Source>(source: &mut S) -> Result<Movie, Error> {
    let size = source.size();
    let mut offset = 0u64;
    let mut moov: Option<Vec<u8>> = None;
    let mut brand = String::new();
    let mut header = [0u8; 16];
    let mut first = true;
    let mut fragments: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut fragment_bytes = 0u64;
    while offset + 8 <= size {
        read_exact(source, offset, &mut header[..8])?;
        let mut box_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let kind = [header[4], header[5], header[6], header[7]];
        if first && !matches!(&kind, b"ftyp" | b"moov" | b"mdat" | b"free" | b"skip" | b"wide" | b"pnot" | b"uuid") {
            return Err(Error::NotMp4);
        }
        first = false;
        let mut header_len = 8u64;
        if box_size == 1 {
            read_exact(source, offset + 8, &mut header[8..16])?;
            box_size = u64::from_be_bytes([header[8], header[9], header[10], header[11], header[12], header[13], header[14], header[15]]);
            header_len = 16;
        } else if box_size == 0 {
            box_size = size - offset;
        }
        if box_size < header_len {
            return Err(Error::Corrupt);
        }
        match &kind {
            b"ftyp" if box_size >= 12 => {
                let mut b = [0u8; 4];
                read_exact(source, offset + header_len, &mut b)?;
                brand = String::from_utf8_lossy(&b).trim().into();
            }
            b"moov" => {
                if box_size > 512 * 1024 * 1024 {
                    return Err(Error::TooLarge);
                }
                let mut data = alloc::vec![0u8; box_size as usize];
                read_exact(source, offset, &mut data)?;
                moov = Some(data);
            }
            b"moof" => {
                fragment_bytes += box_size;
                if box_size > 64 * 1024 * 1024 || fragment_bytes > 256 * 1024 * 1024 {
                    return Err(Error::TooLarge);
                }
                let mut data = alloc::vec![0u8; box_size as usize];
                read_exact(source, offset, &mut data)?;
                fragments.push((offset, data));
            }
            _ => {}
        }
        if moov.is_some() && offset + box_size >= size {
            break;
        }
        offset += box_size;
    }
    let data = moov.ok_or(Error::NoMovie)?;
    let (moov, _) = MoovBox::decode(&data).map_err(|_| Error::Corrupt)?;
    drop(data);
    let mut tracks = Vec::new();
    for trak in moov.trak_boxes.iter() {
        if let Some(track) = build_track(trak) {
            tracks.push(track);
        }
    }
    if !fragments.is_empty() {
        let trex: &[TrexBox] = moov.mvex_box.as_ref().map(|m| m.trex_boxes.as_slice()).unwrap_or(&[]);
        for (moof_offset, data) in fragments.iter() {
            let (moof, _) = MoofBox::decode(data).map_err(|_| Error::Corrupt)?;
            add_fragment(&mut tracks, trex, *moof_offset, &moof);
        }
        drop(fragments);
    }
    for track in tracks.iter_mut() {
        finish_track(track, moov.trak_boxes.iter().find(|t| t.tkhd_box.track_id == track.id));
    }
    tracks.retain(|t| t.kind == TrackKind::Other || !t.samples.is_empty());
    if tracks.is_empty() {
        return Err(Error::NoMovie);
    }
    if tracks.is_empty() {
        return Err(Error::NoMovie);
    }
    let movie_duration = moov.mvhd_box.duration * 1000 / (moov.mvhd_box.timescale.get() as u64).max(1);
    let longest = tracks.iter().filter(|t| t.kind != TrackKind::Other).map(|t| t.duration_ms()).max().unwrap_or(0);
    Ok(Movie { tracks, duration_ms: if longest > 0 { longest } else { movie_duration }, brand })
}

fn avcc_bytes(avcc: &shiguredo_mp4::boxes::AvccBox) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(1);
    out.push(avcc.avc_profile_indication);
    out.push(avcc.profile_compatibility);
    out.push(avcc.avc_level_indication);
    out.push(0xFC | (avcc.length_size_minus_one.get() & 3));
    out.push(0xE0 | (avcc.sps_list.len() as u8 & 0x1F));
    for sps in avcc.sps_list.iter() {
        out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        out.extend_from_slice(sps);
    }
    out.push(avcc.pps_list.len() as u8);
    for pps in avcc.pps_list.iter() {
        out.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        out.extend_from_slice(pps);
    }
    out
}

fn aac_object_type(config: &[u8]) -> u8 {
    config.first().map(|b| b >> 3).unwrap_or(0)
}

fn codec_of(entry: &SampleEntry) -> Codec {
    match entry {
        SampleEntry::Avc1(avc1) => Codec::Avc {
            config: avcc_bytes(&avc1.avcc_box),
            width: avc1.visual.width as u32,
            height: avc1.visual.height as u32,
            profile: avc1.avcc_box.avc_profile_indication,
            level: avc1.avcc_box.avc_level_indication,
        },
        SampleEntry::Mp4a(mp4a) => {
            let config = mp4a.esds_box.es.dec_config_descr.dec_specific_info.as_ref().map(|d| d.payload.clone()).unwrap_or_default();
            let object_type = aac_object_type(&config);
            Codec::Aac { object_type, sample_rate: mp4a.audio.samplerate.integer as u32, channels: mp4a.audio.channelcount as u32, config }
        }
        SampleEntry::Hev1(_) | SampleEntry::Hvc1(_) => Codec::Unsupported(String::from("H.265")),
        SampleEntry::Vp08(_) => Codec::Unsupported(String::from("VP8")),
        SampleEntry::Vp09(_) => Codec::Unsupported(String::from("VP9")),
        SampleEntry::Av01(_) => Codec::Unsupported(String::from("AV1")),
        SampleEntry::Opus(_) => Codec::Unsupported(String::from("Opus")),
        SampleEntry::Flac(_) => Codec::Unsupported(String::from("FLAC")),
        _ => Codec::Unsupported(String::from("unknown")),
    }
}

fn build_track(trak: &TrakBox) -> Option<Track> {
    let mdia = &trak.mdia_box;
    let kind = match &mdia.hdlr_box.handler_type {
        b"vide" => TrackKind::Video,
        b"soun" => TrackKind::Audio,
        _ => TrackKind::Other,
    };
    let stbl: &StblBox = &mdia.minf_box.stbl_box;
    let codec = stbl.stsd_box.entries.first().map(codec_of).unwrap_or(Codec::Unsupported(String::from("none")));
    let accessor = SampleTableAccessor::new(stbl).ok();
    let count = accessor.as_ref().map(|a| a.sample_count()).unwrap_or(0);
    let mut samples = Vec::with_capacity(count as usize);
    for i in 1..=count {
        let Some(accessor) = accessor.as_ref() else {
            break;
        };
        let Some(index) = NonZeroU32::new(i) else {
            continue;
        };
        let Some(sample) = accessor.get_sample(index) else {
            continue;
        };
        let dts = sample.timestamp();
        let pts = dts as i64 + sample.composition_time_offset().unwrap_or(0);
        samples.push(SampleInfo { offset: sample.data_offset(), size: sample.data_size(), dts, pts, duration: sample.duration(), sync: sample.is_sync_sample() });
    }
    Some(Track { id: trak.tkhd_box.track_id, kind, timescale: mdia.mdhd_box.timescale.get(), duration: mdia.mdhd_box.duration, codec, samples, edit_offset: 0, language: String::from_utf8_lossy(&mdia.mdhd_box.language.as_bytes()).into_owned(), display_order: Vec::new() })
}

fn finish_track(track: &mut Track, trak: Option<&TrakBox>) {
    let samples = &track.samples;
    track.edit_offset = trak
        .and_then(|t| t.edts_box.as_ref())
        .and_then(|e| e.elst_box.as_ref())
        .and_then(|elst| elst.entries.iter().find(|e| e.media_time >= 0).map(|e| e.media_time))
        .unwrap_or_else(|| samples.iter().map(|s| s.pts).min().unwrap_or(0).min(0));
    let mut display_order: Vec<u32> = (0..samples.len() as u32).collect();
    display_order.sort_by_key(|i| samples[*i as usize].pts);
    track.display_order = display_order;
}

fn add_fragment(tracks: &mut [Track], trex: &[TrexBox], moof_offset: u64, moof: &MoofBox) {
    let mut previous_end = moof_offset;
    for traf in moof.traf_boxes.iter() {
        let tfhd = &traf.tfhd_box;
        let defaults = trex.iter().find(|t| t.track_id == tfhd.track_id);
        let base = match tfhd.base_data_offset {
            Some(b) => b,
            None if tfhd.default_base_is_moof => moof_offset,
            None => previous_end,
        };
        let Some(track) = tracks.iter_mut().find(|t| t.id == tfhd.track_id) else {
            continue;
        };
        let default_duration = tfhd.default_sample_duration.or(defaults.map(|d| d.default_sample_duration)).unwrap_or(0);
        let default_size = tfhd.default_sample_size.or(defaults.map(|d| d.default_sample_size)).unwrap_or(0);
        let default_flags = tfhd.default_sample_flags.or(defaults.map(|d| d.default_sample_flags));
        let mut dts = match &traf.tfdt_box {
            Some(tfdt) => tfdt.base_media_decode_time,
            None => track.samples.last().map(|s| s.dts + s.duration as u64).unwrap_or(0),
        };
        let mut data = base;
        for trun in traf.trun_boxes.iter() {
            if let Some(offset) = trun.data_offset {
                data = (base as i64 + offset as i64).max(0) as u64;
            }
            track.samples.reserve(trun.samples.len());
            for (i, sample) in trun.samples.iter().enumerate() {
                let duration = sample.duration.unwrap_or(default_duration);
                let size = sample.size.unwrap_or(default_size);
                let flags = sample.flags.or(if i == 0 { trun.first_sample_flags } else { None }).or(default_flags);
                let sync = match flags {
                    Some(f) => !f.sample_is_non_sync_sample(),
                    None => track.kind != TrackKind::Video,
                };
                let pts = dts as i64 + sample.composition_time_offset.unwrap_or(0);
                track.samples.push(SampleInfo { offset: data, size, dts, pts, duration, sync });
                dts += duration as u64;
                data += size as u64;
            }
        }
        previous_end = data;
    }
}
