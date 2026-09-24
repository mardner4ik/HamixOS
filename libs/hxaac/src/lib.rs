#![no_std]
#![allow(clippy::needless_range_loop, clippy::excessive_precision, clippy::identity_op, clippy::manual_range_contains)]

extern crate alloc;

mod aac;
mod asc;
mod support;

use alloc::vec;
use alloc::vec::Vec;

pub use asc::{AudioObjectType, AudioSpecificConfig};
pub use support::errors::Error;

pub struct PlanarBuffer {
    planes: Vec<Vec<f32>>,
}

impl PlanarBuffer {
    fn new(channels: usize, frames: usize) -> PlanarBuffer {
        PlanarBuffer { planes: (0..channels).map(|_| vec![0.0; frames]).collect() }
    }

    fn clear(&mut self) {
        for plane in self.planes.iter_mut() {
            plane.fill(0.0);
        }
    }

    pub fn plane_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        self.planes.get_mut(channel).map(|p| p.as_mut_slice())
    }

    pub fn plane(&self, channel: usize) -> &[f32] {
        &self.planes[channel]
    }
}

pub struct Decoder {
    core: aac::AacCore,
}

pub struct Info {
    pub sample_rate: u32,
    pub channels: u32,
    pub he_aac: bool,
}

fn to_i16(v: f32) -> i16 {
    let scaled = v * 32767.0;
    if scaled >= 32767.0 {
        32767
    } else if scaled <= -32768.0 {
        -32768
    } else {
        scaled as i16
    }
}

impl Decoder {
    pub fn new(audio_specific_config: &[u8]) -> Result<Decoder, Error> {
        let asc = AudioSpecificConfig::read(audio_specific_config)?;
        Ok(Decoder { core: aac::AacCore::new(asc)? })
    }

    pub fn from_adts_header(header: &[u8]) -> Result<Decoder, Error> {
        let (profile, rate, channels, _) = parse_adts(header).ok_or(Error::Decode("aac: not an ADTS header"))?;
        let asc = AudioSpecificConfig::from_adts(profile, rate, channels)?;
        Ok(Decoder { core: aac::AacCore::new(asc)? })
    }

    pub fn info(&self) -> Info {
        Info { sample_rate: self.core.asc.sample_rate, channels: self.core.channels as u32, he_aac: self.core.asc.sbr_present }
    }

    pub fn sample_rate(&self) -> u32 {
        self.core.asc.sample_rate
    }

    pub fn output_channels(&self) -> u32 {
        if self.core.channels == 1 { 1 } else { 2 }
    }

    pub fn reset(&mut self) {
        self.core.reset();
    }

    pub fn decode_planar(&mut self, packet: &[u8]) -> Result<&PlanarBuffer, Error> {
        self.core.decode(packet)?;
        Ok(&self.core.buf)
    }

    pub fn decode(&mut self, packet: &[u8], out: &mut Vec<i16>) -> Result<usize, Error> {
        let data = match parse_adts(packet) {
            Some((_, _, _, header)) if packet.len() > header => &packet[header..],
            _ => packet,
        };
        self.core.decode(data)?;
        let buf = &self.core.buf;
        let frames = 1024;
        match self.core.channels {
            1 => {
                out.extend(buf.plane(0).iter().map(|s| to_i16(*s)));
            }
            2 => {
                let (l, r) = (buf.plane(0), buf.plane(1));
                out.reserve(frames * 2);
                for i in 0..frames {
                    out.push(to_i16(l[i]));
                    out.push(to_i16(r[i]));
                }
            }
            n => {
                let center = 0.7071;
                let (c, fl, fr, sl, sr, lfe) = match n {
                    3 => (Some(0), 1, 2, None, None, None),
                    4 => (Some(0), 1, 2, Some(3), Some(3), None),
                    5 => (Some(0), 1, 2, Some(3), Some(4), None),
                    6 => (Some(0), 1, 2, Some(3), Some(4), Some(5)),
                    _ => (Some(0), 3, 4, Some(5), Some(6), Some(7)),
                };
                let norm = 1.0 / (1.0 + center + center);
                out.reserve(frames * 2);
                for i in 0..frames {
                    let mid = c.map(|ch| buf.plane(ch)[i] * center).unwrap_or(0.0) + lfe.map(|ch| buf.plane(ch)[i] * 0.5).unwrap_or(0.0);
                    let left = buf.plane(fl)[i] + mid + sl.map(|ch| buf.plane(ch)[i] * center).unwrap_or(0.0);
                    let right = buf.plane(fr)[i] + mid + sr.map(|ch| buf.plane(ch)[i] * center).unwrap_or(0.0);
                    out.push(to_i16(left * norm));
                    out.push(to_i16(right * norm));
                }
            }
        }
        Ok(frames)
    }
}

pub fn parse_adts(data: &[u8]) -> Option<(u32, u32, u32, usize)> {
    if data.len() < 7 || data[0] != 0xFF || data[1] & 0xF6 != 0xF0 {
        return None;
    }
    let protection_absent = data[1] & 1 == 1;
    let profile = (data[2] >> 6) as u32;
    let rate = ((data[2] >> 2) & 0xF) as u32;
    let channels = (((data[2] & 1) << 2) | (data[3] >> 6)) as u32;
    Some((profile, rate, channels, if protection_absent { 7 } else { 9 }))
}
