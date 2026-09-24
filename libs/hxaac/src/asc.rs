// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Adapted for HamixOS: reduced to what the AAC-LC decoder needs.

use crate::support::bit::{BitReaderLtr, ReadBitsLtr};
use crate::support::errors::{Result, decode_error, unsupported_error};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum AudioObjectType {
    #[default]
    Null,
    Main,
    Lc,
    Ssr,
    Ltp,
    Sbr,
    Scalable,
    Ps,
    Other(u32),
}

impl AudioObjectType {
    fn from_index(index: u32) -> AudioObjectType {
        match index {
            0 => AudioObjectType::Null,
            1 => AudioObjectType::Main,
            2 => AudioObjectType::Lc,
            3 => AudioObjectType::Ssr,
            4 => AudioObjectType::Ltp,
            5 => AudioObjectType::Sbr,
            6 => AudioObjectType::Scalable,
            29 => AudioObjectType::Ps,
            other => AudioObjectType::Other(other),
        }
    }
}

const SAMPLE_RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

#[derive(Clone, Debug, Default)]
pub struct AudioSpecificConfig {
    pub object_type: AudioObjectType,
    pub sample_rate: u32,
    pub channels: Option<u32>,
    pub samples: usize,
    pub extension_rate: Option<u32>,
    pub sbr_present: bool,
    pub ps_present: bool,
}

impl AudioSpecificConfig {
    pub fn read(buf: &[u8]) -> Result<AudioSpecificConfig> {
        let mut bs = BitReaderLtr::new(buf);

        let mut asc = AudioSpecificConfig {
            object_type: Self::read_audio_object_type(&mut bs)?,
            sample_rate: Self::read_sampling_frequency(&mut bs)?,
            ..Default::default()
        };

        if asc.sample_rate == 0 {
            return decode_error("aac: a sample rate of 0 is invalid");
        }

        asc.channels = Self::read_channel_config(&mut bs)?;

        if asc.object_type == AudioObjectType::Sbr || asc.object_type == AudioObjectType::Ps {
            asc.sbr_present = true;
            asc.ps_present = asc.object_type == AudioObjectType::Ps;
            asc.extension_rate = Some(Self::read_sampling_frequency(&mut bs)?);
            asc.object_type = Self::read_audio_object_type(&mut bs)?;
        }

        match asc.object_type {
            AudioObjectType::Main | AudioObjectType::Lc | AudioObjectType::Ssr | AudioObjectType::Ltp | AudioObjectType::Scalable => {
                let short_frame = bs.read_bool()?;
                asc.samples = if short_frame { 960 } else { 1024 };
                if bs.read_bool()? {
                    let _delay = bs.read_bits_leq32(14)?;
                }
                let extension_flag = bs.read_bool()?;
                if asc.channels.is_none() {
                    return unsupported_error("aac: program config element");
                }
                if asc.object_type == AudioObjectType::Scalable {
                    let _layer = bs.read_bits_leq32(3)?;
                }
                if extension_flag && bs.read_bool()? {
                    return unsupported_error("aac: version3 extensions");
                }
            }
            _ => return unsupported_error("aac: unsupported audio object type"),
        }

        Ok(asc)
    }

    pub fn from_adts(profile: u32, rate_index: u32, channels: u32) -> Result<AudioSpecificConfig> {
        let sample_rate = *SAMPLE_RATES.get(rate_index as usize).ok_or(crate::support::errors::Error::Decode("aac: bad ADTS rate"))?;
        Ok(AudioSpecificConfig { object_type: AudioObjectType::from_index(profile + 1), sample_rate, channels: if channels == 0 { None } else { Some(channels) }, samples: 1024, ..Default::default() })
    }

    fn read_audio_object_type<B: ReadBitsLtr>(bs: &mut B) -> Result<AudioObjectType> {
        let mut index = bs.read_bits_leq32(5)?;
        if index == 31 {
            index = 32 + bs.read_bits_leq32(6)?;
        }
        Ok(AudioObjectType::from_index(index))
    }

    fn read_sampling_frequency<B: ReadBitsLtr>(bs: &mut B) -> Result<u32> {
        let index = bs.read_bits_leq32(4)?;
        if index == 15 {
            return Ok(bs.read_bits_leq32(24)?);
        }
        Ok(SAMPLE_RATES.get(index as usize).copied().unwrap_or(0))
    }

    fn read_channel_config<B: ReadBitsLtr>(bs: &mut B) -> Result<Option<u32>> {
        let index = bs.read_bits_leq32(4)?;
        Ok(match index {
            1..=6 => Some(index),
            7 => Some(8),
            _ => None,
        })
    }
}
