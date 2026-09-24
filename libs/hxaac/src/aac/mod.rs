// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// Previous Author: Kostya Shishkov <kostya.shiskov@gmail.com>
//
// This source file includes code originally written for the NihAV
// project. With the author's permission, it has been relicensed for,
// and ported to the Symphonia project.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Adapted for HamixOS: no_std, decoupled from symphonia-core, more than two
// channels are decoded (the caller downmixes) and SBR payloads are skipped.

use alloc::vec::Vec;

use crate::asc::{AudioObjectType, AudioSpecificConfig};
use crate::support::bit::{BitReaderLtr, FiniteBitStream, ReadBitsLtr};
use crate::support::errors::{Result, unsupported_error};
use crate::PlanarBuffer;

mod codebooks;
mod common;
mod cpe;
mod dsp;
mod ics;
mod window;

use common::*;

pub struct AacCore {
    pub asc: AudioSpecificConfig,
    pairs: Vec<cpe::ChannelPair>,
    dsp: dsp::Dsp,
    sbinfo: GASubbandInfo,
    pub buf: PlanarBuffer,
    pub channels: usize,
}

impl AacCore {
    pub fn new(asc: AudioSpecificConfig) -> Result<Self> {
        let channels = match asc.channels {
            Some(count) if (1..=8).contains(&count) => count as usize,
            Some(_) => return unsupported_error("aac: too many channels"),
            None => return unsupported_error("aac: program config element"),
        };
        if asc.object_type != AudioObjectType::Lc || asc.samples != 1024 {
            return unsupported_error("aac: only AAC-LC is supported");
        }
        let sbinfo = GASubbandInfo::find(asc.sample_rate);
        Ok(AacCore { asc, pairs: Vec::new(), dsp: dsp::Dsp::new(), sbinfo, buf: PlanarBuffer::new(channels, 1024), channels })
    }

    pub fn reset(&mut self) {
        for pair in self.pairs.iter_mut() {
            pair.reset();
        }
    }

    fn set_pair(&mut self, pair_no: usize, channel: usize, pair: bool) -> Result<()> {
        if self.pairs.len() <= pair_no {
            self.pairs.push(cpe::ChannelPair::new(pair, channel, self.sbinfo));
        }
        else {
            validate!(self.pairs[pair_no].channel == channel);
            validate!(self.pairs[pair_no].is_pair == pair);
        }

        validate!(if pair { channel + 1 } else { channel } < self.channels);

        Ok(())
    }

    fn decode_ga<B: ReadBitsLtr + FiniteBitStream>(&mut self, bs: &mut B) -> Result<()> {
        let mut cur_pair = 0;
        let mut cur_ch = 0;
        while bs.bits_left() > 3 {
            let id = bs.read_bits_leq32(3)?;

            match id {
                0 => {
                    let _tag = bs.read_bits_leq32(4)?;
                    self.set_pair(cur_pair, cur_ch, false)?;
                    self.pairs[cur_pair].decode_ga_sce(bs, self.asc.object_type)?;
                    cur_pair += 1;
                    cur_ch += 1;
                }
                1 => {
                    let _tag = bs.read_bits_leq32(4)?;
                    self.set_pair(cur_pair, cur_ch, true)?;
                    self.pairs[cur_pair].decode_ga_cpe(bs, self.asc.object_type)?;
                    cur_pair += 1;
                    cur_ch += 2;
                }
                2 => {
                    return unsupported_error("aac: coupling channel element");
                }
                3 => {
                    let _tag = bs.read_bits_leq32(4)?;
                    self.set_pair(cur_pair, cur_ch, false)?;
                    self.pairs[cur_pair].decode_ga_sce(bs, self.asc.object_type)?;
                    cur_pair += 1;
                    cur_ch += 1;
                }
                4 => {
                    let _id = bs.read_bits_leq32(4)?;
                    let align = bs.read_bool()?;
                    let mut count = bs.read_bits_leq32(8)?;
                    if count == 255 {
                        count += bs.read_bits_leq32(8)?;
                    }
                    if align {
                        bs.realign();
                    }
                    bs.ignore_bits(count * 8)?;
                }
                5 => {
                    return unsupported_error("aac: program config");
                }
                6 => {
                    let mut count = bs.read_bits_leq32(4)? as usize;
                    if count == 15 {
                        count += bs.read_bits_leq32(8)? as usize;
                        count -= 1;
                    }
                    if count > 0 {
                        let ext_type = bs.read_bits_leq32(4)?;
                        if matches!(ext_type, 0xd | 0xe) {
                            self.asc.sbr_present = true;
                        }
                        bs.ignore_bits(4)?;
                        for _ in 0..count - 1 {
                            bs.ignore_bits(8)?;
                        }
                    }
                }
                7 => {
                    break;
                }
                _ => unreachable!(),
            };
        }
        let rate_idx = GASubbandInfo::find_idx(self.asc.sample_rate);
        for pair in 0..cur_pair {
            self.pairs[pair].synth_audio(&mut self.dsp, &mut self.buf, rate_idx);
        }
        Ok(())
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<()> {
        self.buf.clear();
        let mut bs = BitReaderLtr::new(data);
        self.decode_ga(&mut bs)
    }
}
