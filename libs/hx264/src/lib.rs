#![no_std]

extern crate alloc;

mod convert;

use alloc::vec::Vec;
pub use convert::{ColorMatrix, Scaler};
use rust_h264::decoder::{Decoder as AvcDecoder, SharedFrame};
use rust_h264::nal::{parse_annex_b, parse_avcc, parse_avcc_config, NalUnit, NalUnitType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    BadConfig,
    NotConfigured,
    Decode,
    Unsupported,
}

pub struct Picture {
    frame: SharedFrame,
    pub pts: i64,
}

impl Picture {
    pub fn width(&self) -> u32 {
        self.frame.width
    }

    pub fn height(&self) -> u32 {
        self.frame.height
    }

    pub fn planes(&self) -> Planes<'_> {
        Planes { y: self.frame.y(), u: self.frame.u(), v: self.frame.v(), stride_y: self.frame.stride_y(), stride_c: self.frame.stride_c(), width: self.frame.width as usize, height: self.frame.height as usize }
    }

    pub fn order(&self) -> i32 {
        self.frame.pic_order_cnt
    }
}

#[derive(Clone, Copy)]
pub struct Planes<'a> {
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub stride_y: usize,
    pub stride_c: usize,
    pub width: usize,
    pub height: usize,
}

pub struct Decoder {
    inner: AvcDecoder,
    length_size: usize,
    config: Vec<u8>,
    errors: u32,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

fn feed(inner: &mut AvcDecoder, nal: &NalUnit, out: &mut Vec<SharedFrame>) -> Result<(), Error> {
    match inner.decode_nal_shared(nal) {
        Ok(Some(frame)) => {
            out.push(frame);
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(_) => Err(Error::Decode),
    }
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder { inner: AvcDecoder::new(), length_size: 4, config: Vec::new(), errors: 0 }
    }

    pub fn with_avcc(avcc: &[u8]) -> Result<Decoder, Error> {
        let mut decoder = Decoder::new();
        decoder.configure(avcc)?;
        Ok(decoder)
    }

    pub fn configure(&mut self, avcc: &[u8]) -> Result<(), Error> {
        let config = parse_avcc_config(avcc).map_err(|_| Error::BadConfig)?;
        let mut sink = Vec::new();
        for nal in config.sps_nals.iter().chain(config.pps_nals.iter()) {
            feed(&mut self.inner, nal, &mut sink)?;
        }
        self.length_size = config.length_size;
        self.config = avcc.to_vec();
        Ok(())
    }

    pub fn reset(&mut self) {
        self.inner = AvcDecoder::new();
        self.errors = 0;
        if !self.config.is_empty() {
            let config = core::mem::take(&mut self.config);
            let _ = self.configure(&config);
        }
    }

    pub fn length_size(&self) -> usize {
        self.length_size
    }

    pub fn errors(&self) -> u32 {
        self.errors
    }

    pub fn decode_sample(&mut self, sample: &[u8], pts: i64) -> Result<Option<Picture>, Error> {
        let nals = parse_avcc(sample, self.length_size);
        let mut frames = Vec::new();
        let mut failed = false;
        for nal in nals.iter() {
            if feed(&mut self.inner, nal, &mut frames).is_err() {
                failed = true;
            }
        }
        if let Some(frame) = self.inner.flush_shared() {
            frames.push(frame);
        }
        if failed {
            self.errors += 1;
        }
        match frames.pop() {
            Some(frame) => Ok(Some(Picture { frame, pts })),
            None if failed => Err(Error::Decode),
            None => Ok(None),
        }
    }

    pub fn decode_annexb(&mut self, data: &[u8]) -> Vec<Picture> {
        let mut frames = Vec::new();
        for nal in parse_annex_b(data).iter() {
            if feed(&mut self.inner, nal, &mut frames).is_err() {
                self.errors += 1;
            }
        }
        frames.into_iter().map(|frame| Picture { pts: frame.pic_order_cnt as i64, frame }).collect()
    }

    pub fn finish(&mut self) -> Option<Picture> {
        self.inner.flush_shared().map(|frame| Picture { pts: frame.pic_order_cnt as i64, frame })
    }
}

pub fn is_disposable(sample: &[u8], length_size: usize) -> bool {
    let mut i = 0;
    let mut any_slice = false;
    while i + length_size < sample.len() {
        let mut len = 0usize;
        for b in &sample[i..i + length_size] {
            len = (len << 8) | *b as usize;
        }
        i += length_size;
        if len == 0 || i + len > sample.len() {
            return false;
        }
        let header = sample[i];
        let kind = header & 0x1F;
        if kind == 1 || kind == 5 {
            any_slice = true;
            if header & 0x60 != 0 {
                return false;
            }
        }
        i += len;
    }
    any_slice
}

pub fn is_keyframe(sample: &[u8], length_size: usize) -> bool {
    parse_avcc(sample, length_size).iter().any(|n| n.nal_unit_type == NalUnitType::SliceIdr)
}

pub fn describe_avcc(avcc: &[u8]) -> Option<(u8, u8)> {
    if avcc.len() < 4 || avcc[0] != 1 {
        return None;
    }
    Some((avcc[1], avcc[3]))
}

pub fn profile_name(profile: u8) -> &'static str {
    match profile {
        66 => "Baseline",
        77 => "Main",
        88 => "Extended",
        100 => "High",
        110 => "High 10",
        122 => "High 4:2:2",
        244 => "High 4:4:4",
        _ => "H.264",
    }
}
