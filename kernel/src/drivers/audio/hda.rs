use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::AudioDevice;
use crate::arch::delay_us as pit_delay_us;
use crate::drivers::pci::{self, PciDevice};
use crate::net::dma::{DmaRegion, Mmio};

const GCAP: u32 = 0x00;
const GCTL: u32 = 0x08;
const STATESTS: u32 = 0x0E;
const INTCTL: u32 = 0x20;
const CORBLBASE: u32 = 0x40;
const CORBUBASE: u32 = 0x44;
const CORBWP: u32 = 0x48;
const CORBRP: u32 = 0x4A;
const CORBCTL: u32 = 0x4C;
const CORBSIZE: u32 = 0x4E;
const RIRBLBASE: u32 = 0x50;
const RIRBUBASE: u32 = 0x54;
const RIRBWP: u32 = 0x58;
const RINTCNT: u32 = 0x5A;
const RIRBCTL: u32 = 0x5C;
const RIRBSTS: u32 = 0x5D;
const RIRBSIZE: u32 = 0x5E;
const ICOI: u32 = 0x60;
const IRII: u32 = 0x64;
const ICIS: u32 = 0x68;
const DPLBASE: u32 = 0x70;
const DPUBASE: u32 = 0x74;

const SD_CTL: u32 = 0x00;
const SD_STS: u32 = 0x03;
const SD_LPIB: u32 = 0x04;
const SD_CBL: u32 = 0x08;
const SD_LVI: u32 = 0x0C;
const SD_FMT: u32 = 0x12;
const SD_BDPL: u32 = 0x18;
const SD_BDPU: u32 = 0x1C;

const PARAM_VENDOR: u32 = 0x00;
const PARAM_NODE_COUNT: u32 = 0x04;
const PARAM_FUNCTION_TYPE: u32 = 0x05;
const PARAM_WIDGET_CAPS: u32 = 0x09;
const PARAM_PCM: u32 = 0x0A;
const PARAM_PIN_CAPS: u32 = 0x0C;
const PARAM_IN_AMP: u32 = 0x0D;
const PARAM_CONN_LEN: u32 = 0x0E;
const PARAM_OUT_AMP: u32 = 0x12;

const WIDGET_OUTPUT: u8 = 0x0;
const WIDGET_MIXER: u8 = 0x2;
const WIDGET_PIN: u8 = 0x4;

const DEVICE_LINE_OUT: u8 = 0x0;
const DEVICE_SPEAKER: u8 = 0x1;
const DEVICE_HP_OUT: u8 = 0x2;

const RING_FRAMES: usize = 16384;
const BDL_ENTRIES: usize = 8;
const STREAM_TAG: u32 = 1;

struct Widget {
    nid: u16,
    caps: u32,
    pin_caps: u32,
    config: u32,
    conns: Vec<u16>,
    out_amp: u32,
    in_amp: u32,
}

impl Widget {
    fn kind(&self) -> u8 {
        ((self.caps >> 20) & 0xF) as u8
    }

    fn digital(&self) -> bool {
        self.caps & (1 << 9) != 0
    }

    fn device(&self) -> u8 {
        ((self.config >> 20) & 0xF) as u8
    }

    fn connectivity(&self) -> u32 {
        self.config >> 30
    }
}

struct Output {
    pin: u16,
    device: u8,
    path: Vec<(u16, usize)>,
    presence: bool,
}

pub struct Hda {
    mmio: Mmio,
    corb: Option<DmaRegion>,
    rirb: Option<DmaRegion>,
    corb_entries: u16,
    rirb_entries: u16,
    rirb_read: u16,
    stream_base: u32,
    bdl: DmaRegion,
    buffer: DmaRegion,
    position: DmaRegion,
    codec: u32,
    codec_name: String,
    rate: u32,
    format: u16,
    outputs: Vec<Output>,
    dacs: Vec<u16>,
    running: bool,
    headphones_in: bool,
    controller: String,
}

unsafe impl Send for Hda {}

fn vendor_name(id: u32) -> String {
    let vendor = (id >> 16) as u16;
    let device = id as u16;
    let brand = match vendor {
        0x10EC => "Realtek",
        0x14F1 => "Conexant",
        0x111D | 0x8384 => "IDT/SigmaTel",
        0x11D4 => "Analog Devices",
        0x1106 => "VIA",
        0x10DE => "NVIDIA HDMI",
        0x1002 => "AMD HDMI",
        0x8086 => "Intel HDMI",
        0x1AF4 => "QEMU",
        0x13F6 => "C-Media",
        0x434D => "C-Media",
        0x1057 => "Motorola",
        0x11C1 => "LSI",
        0x17E8 => "Chrontel",
        0x15AD => "VMware",
        _ => "",
    };
    match (brand, vendor) {
        ("", _) => format!("codec {:04x}:{:04x}", vendor, device),
        (b, 0x10EC) => format!("{} ALC{:x}", b, device),
        (b, 0x14F1) => format!("{} CX{:x}", b, device),
        (b, _) => format!("{} {:04x}", b, device),
    }
}

impl Hda {
    fn wait<F: Fn(&Mmio) -> bool>(&self, micros: u64, done: F) -> bool {
        let mut waited = 0;
        while waited < micros {
            if done(&self.mmio) {
                return true;
            }
            pit_delay_us(100);
            waited += 100;
        }
        done(&self.mmio)
    }

    fn reset(&mut self) -> bool {
        self.mmio.write8(CORBCTL, 0);
        self.mmio.write8(RIRBCTL, 0);
        let gctl = self.mmio.read32(GCTL);
        if gctl & 1 != 0 {
            self.mmio.write32(GCTL, gctl & !1);
            self.wait(100_000, |m| m.read32(GCTL) & 1 == 0);
        }
        pit_delay_us(1000);
        self.mmio.write32(GCTL, self.mmio.read32(GCTL) | 1);
        if !self.wait(100_000, |m| m.read32(GCTL) & 1 == 1) {
            return false;
        }
        pit_delay_us(50_000);
        self.mmio.write16(STATESTS, 0xFFFF & self.mmio.read16(STATESTS));
        true
    }

    fn setup_rings(&mut self) {
        let corb_size = self.mmio.read8(CORBSIZE);
        let (corb_code, corb_entries) = if corb_size & 0x40 != 0 { (2, 256) } else if corb_size & 0x20 != 0 { (1, 16) } else { (0, 2) };
        let rirb_size = self.mmio.read8(RIRBSIZE);
        let (rirb_code, rirb_entries) = if rirb_size & 0x40 != 0 { (2, 256) } else if rirb_size & 0x20 != 0 { (1, 16) } else { (0, 2) };
        let (Some(corb), Some(rirb)) = (DmaRegion::new(4096), DmaRegion::new(4096)) else {
            return;
        };
        self.mmio.write8(CORBSIZE, (corb_size & !3) | corb_code);
        self.mmio.write32(CORBLBASE, corb.phys as u32);
        self.mmio.write32(CORBUBASE, (corb.phys >> 32) as u32);
        self.mmio.write16(CORBRP, 0x8000);
        self.wait(10_000, |m| m.read16(CORBRP) & 0x8000 != 0);
        self.mmio.write16(CORBRP, 0);
        self.wait(10_000, |m| m.read16(CORBRP) & 0x8000 == 0);
        self.mmio.write16(CORBWP, 0);
        self.mmio.write8(RIRBSIZE, (rirb_size & !3) | rirb_code);
        self.mmio.write32(RIRBLBASE, rirb.phys as u32);
        self.mmio.write32(RIRBUBASE, (rirb.phys >> 32) as u32);
        self.mmio.write16(RIRBWP, 0x8000);
        self.mmio.write16(RINTCNT, 1);
        self.mmio.write8(RIRBSTS, 0x05);
        self.mmio.write8(CORBCTL, 0x02);
        self.mmio.write8(RIRBCTL, 0x02);
        self.wait(10_000, |m| m.read8(CORBCTL) & 2 != 0 && m.read8(RIRBCTL) & 2 != 0);
        self.corb = Some(corb);
        self.rirb = Some(rirb);
        self.corb_entries = corb_entries;
        self.rirb_entries = rirb_entries;
        self.rirb_read = 0;
    }

    fn command_ring(&mut self, verb: u32) -> Option<u32> {
        let (corb, rirb) = (self.corb.as_ref()?, self.rirb.as_ref()?);
        let wp = (self.mmio.read16(CORBWP) & 0xFF) as u16;
        let next = (wp + 1) % self.corb_entries;
        unsafe { core::ptr::write_volatile(corb.ptr::<u32>(next as usize * 4), verb) };
        self.mmio.write16(CORBWP, next);
        let rirb_mask = self.rirb_entries - 1;
        for _ in 0..2000 {
            let write_ptr = self.mmio.read16(RIRBWP) & 0xFF;
            while self.rirb_read != write_ptr {
                self.rirb_read = (self.rirb_read + 1) & rirb_mask;
                let at = self.rirb_read as usize * 8;
                let response = unsafe { core::ptr::read_volatile(rirb.ptr::<u32>(at)) };
                let extended = unsafe { core::ptr::read_volatile(rirb.ptr::<u32>(at + 4)) };
                if extended & 0x10 == 0 {
                    self.mmio.write8(RIRBSTS, 0x05);
                    return Some(response);
                }
            }
            pit_delay_us(50);
        }
        None
    }

    fn command_immediate(&mut self, verb: u32) -> Option<u32> {
        if !self.wait(20_000, |m| m.read16(ICIS) & 1 == 0) {
            return None;
        }
        self.mmio.write16(ICIS, 0x2);
        self.mmio.write32(ICOI, verb);
        self.mmio.write16(ICIS, 0x1);
        if !self.wait(20_000, |m| m.read16(ICIS) & 0x3 == 0x2) {
            return None;
        }
        Some(self.mmio.read32(IRII))
    }

    fn command(&mut self, nid: u16, verb: u32, payload: u32) -> u32 {
        let raw = if verb <= 0xF { (self.codec << 28) | ((nid as u32) << 20) | (verb << 16) | (payload & 0xFFFF) } else { (self.codec << 28) | ((nid as u32) << 20) | (verb << 8) | (payload & 0xFF) };
        if self.corb.is_some() {
            if let Some(r) = self.command_ring(raw) {
                return r;
            }
            self.mmio.write8(CORBCTL, 0);
            self.mmio.write8(RIRBCTL, 0);
            self.corb = None;
            self.rirb = None;
        }
        self.command_immediate(raw).unwrap_or(0)
    }

    fn param(&mut self, nid: u16, id: u32) -> u32 {
        self.command(nid, 0xF00, id)
    }

    fn connections(&mut self, nid: u16) -> Vec<u16> {
        let info = self.param(nid, PARAM_CONN_LEN);
        let count = (info & 0x7F) as usize;
        let long = info & 0x80 != 0;
        let mut raw = Vec::new();
        let mut index = 0;
        while raw.len() < count && index < 64 {
            let r = self.command(nid, 0xF02, index as u32);
            let per = if long { 2 } else { 4 };
            for i in 0..per {
                if raw.len() >= count {
                    break;
                }
                let entry = if long { (r >> (i * 16)) & 0xFFFF } else { (r >> (i * 8)) & 0xFF };
                raw.push(entry);
            }
            index += per;
        }
        let range_bit = if long { 0x8000 } else { 0x80 };
        let mut out: Vec<u16> = Vec::new();
        for entry in raw {
            let nid = (entry & !range_bit) as u16;
            if entry & range_bit != 0 {
                if let Some(&last) = out.last() {
                    for n in last + 1..=nid {
                        out.push(n);
                    }
                    continue;
                }
            }
            out.push(nid);
        }
        out
    }

    fn scan_codec(&mut self, codec: u32) -> Option<(Vec<Widget>, u16, u32, u32, u32)> {
        self.codec = codec;
        let vendor = self.param(0, PARAM_VENDOR);
        if vendor == 0 || vendor == 0xFFFF_FFFF {
            return None;
        }
        let nodes = self.param(0, PARAM_NODE_COUNT);
        let (start, count) = (((nodes >> 16) & 0xFF) as u16, (nodes & 0xFF) as u16);
        for fg in start..start + count {
            if self.param(fg, PARAM_FUNCTION_TYPE) & 0xFF != 1 {
                continue;
            }
            self.command(fg, 0x705, 0);
            pit_delay_us(10_000);
            let afg_out_amp = self.param(fg, PARAM_OUT_AMP);
            let afg_in_amp = self.param(fg, PARAM_IN_AMP);
            let afg_pcm = self.param(fg, PARAM_PCM);
            let sub = self.param(fg, PARAM_NODE_COUNT);
            let (wstart, wcount) = (((sub >> 16) & 0xFF) as u16, (sub & 0xFF) as u16);
            let mut widgets = Vec::new();
            for nid in wstart..wstart + wcount {
                let caps = self.param(nid, PARAM_WIDGET_CAPS);
                let kind = ((caps >> 20) & 0xF) as u8;
                let pin_caps = if kind == WIDGET_PIN { self.param(nid, PARAM_PIN_CAPS) } else { 0 };
                let config = if kind == WIDGET_PIN { self.command(nid, 0xF1C, 0) } else { 0 };
                let conns = if caps & (1 << 8) != 0 { self.connections(nid) } else { Vec::new() };
                let override_amp = caps & (1 << 3) != 0;
                let out_amp = if caps & (1 << 2) != 0 { if override_amp { self.param(nid, PARAM_OUT_AMP) } else { afg_out_amp } } else { 0 };
                let in_amp = if caps & (1 << 1) != 0 { if override_amp { self.param(nid, PARAM_IN_AMP) } else { afg_in_amp } } else { 0 };
                widgets.push(Widget { nid, caps, pin_caps, config, conns, out_amp, in_amp });
            }
            return Some((widgets, fg, vendor, afg_pcm, 0));
        }
        None
    }

    fn find_path(widgets: &[Widget], nid: u16, depth: usize, path: &mut Vec<(u16, usize)>) -> bool {
        if depth > 6 {
            return false;
        }
        let Some(w) = widgets.iter().find(|w| w.nid == nid) else {
            return false;
        };
        if w.kind() == WIDGET_OUTPUT {
            if w.digital() {
                return false;
            }
            path.push((nid, 0));
            return true;
        }
        if w.kind() == WIDGET_PIN && depth > 0 {
            return false;
        }
        if !matches!(w.kind(), WIDGET_PIN | WIDGET_MIXER | 0x3) {
            return false;
        }
        for (index, next) in w.conns.iter().enumerate() {
            if path.iter().any(|(n, _)| n == next) {
                continue;
            }
            path.push((nid, index));
            if Self::find_path(widgets, *next, depth + 1, path) {
                return true;
            }
            path.pop();
        }
        false
    }

    fn pick_outputs(widgets: &[Widget]) -> Vec<Output> {
        let usable = |w: &&Widget| w.kind() == WIDGET_PIN && w.pin_caps & (1 << 4) != 0 && !w.digital() && w.pin_caps & ((1 << 7) | (1 << 24)) == 0;
        let mut candidates: Vec<&Widget> = widgets.iter().filter(usable).filter(|w| w.connectivity() != 1 && matches!(w.device(), DEVICE_LINE_OUT | DEVICE_SPEAKER | DEVICE_HP_OUT)).collect();
        if candidates.is_empty() {
            candidates = widgets.iter().filter(usable).filter(|w| w.connectivity() != 1).collect();
        }
        if candidates.is_empty() {
            candidates = widgets.iter().filter(usable).collect();
        }
        let mut outputs = Vec::new();
        for pin in candidates {
            let mut path = Vec::new();
            if Self::find_path(widgets, pin.nid, 0, &mut path) {
                let device = if pin.connectivity() == 1 && !matches!(pin.device(), DEVICE_LINE_OUT | DEVICE_SPEAKER | DEVICE_HP_OUT) { DEVICE_LINE_OUT } else { pin.device() };
                outputs.push(Output { pin: pin.nid, device, path, presence: pin.pin_caps & (1 << 2) != 0 && pin.config & (1 << 8) == 0 });
            }
        }
        outputs
    }

    fn unmute_path(&mut self, widgets: &[Widget], output: &Output) {
        for (i, (nid, index)) in output.path.iter().enumerate() {
            let Some(w) = widgets.iter().find(|w| w.nid == *nid) else {
                continue;
            };
            if w.caps & (1 << 10) != 0 {
                self.command(*nid, 0x705, 0);
            }
            if w.out_amp != 0 {
                let offset = w.out_amp & 0x7F;
                self.command(*nid, 0x3, 0xB000 | offset);
            }
            let is_last = i + 1 == output.path.len();
            if is_last {
                continue;
            }
            if w.kind() == WIDGET_MIXER {
                if w.in_amp != 0 {
                    for (j, _) in w.conns.iter().enumerate() {
                        let mute = if j == *index { 0 } else { 0x80 };
                        let offset = w.in_amp & 0x7F;
                        self.command(*nid, 0x3, 0x7000 | ((j as u32) << 8) | mute | if mute == 0 { offset } else { 0 });
                    }
                }
            } else {
                if w.conns.len() > 1 {
                    self.command(*nid, 0x701, *index as u32);
                }
                if w.in_amp != 0 {
                    let offset = w.in_amp & 0x7F;
                    self.command(*nid, 0x3, 0x7000 | ((*index as u32) << 8) | offset);
                }
            }
        }
    }

    fn set_pin(&mut self, output: &Output, enabled: bool) {
        let control = if !enabled { 0 } else if output.device == DEVICE_HP_OUT { 0xC0 } else { 0x40 };
        self.command(output.pin, 0x707, control);
    }

    fn configure_codec(&mut self) -> bool {
        let codecs = self.mmio.read16(STATESTS);
        let mut best: Option<(u32, Vec<Widget>, u16, u32, u32, Vec<Output>)> = None;
        for codec in 0..15u32 {
            if codecs & (1 << codec) == 0 && !(codecs == 0 && codec == 0) {
                continue;
            }
            let Some((widgets, fg, vendor, pcm, _)) = self.scan_codec(codec) else {
                continue;
            };
            let outputs = Self::pick_outputs(&widgets);
            if outputs.is_empty() {
                continue;
            }
            let analog = outputs.iter().any(|o| matches!(o.device, DEVICE_SPEAKER | DEVICE_HP_OUT));
            let replace = match &best {
                None => true,
                Some((_, _, _, _, _, current)) => analog && !current.iter().any(|o| matches!(o.device, DEVICE_SPEAKER | DEVICE_HP_OUT)),
            };
            if replace {
                best = Some((codec, widgets, fg, vendor, pcm, outputs));
            }
        }
        let Some((codec, widgets, fg, vendor, afg_pcm, outputs)) = best else {
            return false;
        };
        self.codec = codec;
        self.codec_name = vendor_name(vendor);
        self.command(fg, 0x705, 0);
        let mut dacs: Vec<u16> = Vec::new();
        for output in outputs.iter() {
            if let Some((dac, _)) = output.path.last() {
                if !dacs.contains(dac) {
                    dacs.push(*dac);
                }
            }
        }
        let mut rates = afg_pcm;
        for dac in dacs.iter() {
            let caps = widgets.iter().find(|w| w.nid == *dac).map(|w| w.caps).unwrap_or(0);
            if caps & (1 << 11) != 0 {
                let own = self.param(*dac, PARAM_PCM);
                if own != 0 {
                    rates = own;
                }
            }
        }
        let (rate, format) = if rates & (1 << 6) != 0 || rates == 0 { (48000, 0x0011u16) } else { (44100, 0x4011u16) };
        self.rate = rate;
        self.format = format;
        for dac in dacs.iter() {
            self.command(*dac, 0x705, 0);
            self.command(*dac, 0x2, format as u32);
            self.command(*dac, 0x706, STREAM_TAG << 4);
        }
        for output in outputs.iter() {
            self.unmute_path(&widgets, output);
            let pin = widgets.iter().find(|w| w.nid == output.pin);
            if pin.map(|p| p.pin_caps & (1 << 16) != 0).unwrap_or(false) {
                self.command(output.pin, 0x70C, 0x02);
            }
            self.set_pin(output, true);
        }
        self.dacs = dacs;
        self.outputs = outputs;
        self.update_jacks(true);
        true
    }

    fn update_jacks(&mut self, force: bool) {
        let hp: Vec<u16> = self.outputs.iter().filter(|o| o.device == DEVICE_HP_OUT && o.presence).map(|o| o.pin).collect();
        if hp.is_empty() {
            return;
        }
        let mut plugged = false;
        for pin in hp {
            let sense = self.command(pin, 0xF09, 0);
            if sense & 0x8000_0000 != 0 {
                plugged = true;
            }
        }
        if plugged == self.headphones_in && !force {
            return;
        }
        self.headphones_in = plugged;
        let targets: Vec<usize> = (0..self.outputs.len()).filter(|i| self.outputs[*i].device == DEVICE_SPEAKER).collect();
        if targets.len() == self.outputs.len() {
            return;
        }
        for i in targets {
            let output = Output { pin: self.outputs[i].pin, device: self.outputs[i].device, path: Vec::new(), presence: false };
            self.set_pin(&output, !plugged);
        }
    }

    fn setup_stream(&mut self) {
        let base = self.stream_base;
        let ctl = self.mmio.read8(base + SD_CTL);
        self.mmio.write8(base + SD_CTL, ctl & !0x02);
        self.wait(10_000, |m| m.read8(base + SD_CTL) & 0x02 == 0);
        self.mmio.write8(base + SD_CTL, 0x01);
        self.wait(10_000, |m| m.read8(base + SD_CTL) & 0x01 != 0);
        self.mmio.write8(base + SD_CTL, 0x00);
        self.wait(10_000, |m| m.read8(base + SD_CTL) & 0x01 == 0);
        self.mmio.write8(base + SD_STS, 0x1C);
        let entry_bytes = RING_FRAMES * 4 / BDL_ENTRIES;
        for i in 0..BDL_ENTRIES {
            unsafe {
                let e = self.bdl.ptr::<u8>(i * 16);
                core::ptr::write_volatile(e as *mut u64, self.buffer.phys + (i * entry_bytes) as u64);
                core::ptr::write_volatile(e.add(8) as *mut u32, entry_bytes as u32);
                core::ptr::write_volatile(e.add(12) as *mut u32, 0);
            }
        }
        self.mmio.write32(base + SD_BDPL, self.bdl.phys as u32);
        self.mmio.write32(base + SD_BDPU, (self.bdl.phys >> 32) as u32);
        self.mmio.write32(base + SD_CBL, (RING_FRAMES * 4) as u32);
        self.mmio.write16(base + SD_LVI, (BDL_ENTRIES - 1) as u16);
        self.mmio.write16(base + SD_FMT, self.format);
        let tag = self.mmio.read8(base + SD_CTL + 2);
        self.mmio.write8(base + SD_CTL + 2, (tag & 0x0F) | ((STREAM_TAG as u8) << 4));
        for dac in self.dacs.clone() {
            self.command(dac, 0x2, self.format as u32);
            self.command(dac, 0x706, STREAM_TAG << 4);
        }
    }
}

impl AudioDevice for Hda {
    fn name(&self) -> String {
        format!("{} on {}", self.codec_name, self.controller)
    }

    fn driver(&self) -> &'static str {
        "Intel High Definition Audio"
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn ring_frames(&self) -> usize {
        RING_FRAMES
    }

    fn ring(&mut self) -> &mut [i16] {
        unsafe { core::slice::from_raw_parts_mut(self.buffer.ptr::<i16>(0), RING_FRAMES * 2) }
    }

    fn position(&mut self) -> usize {
        let dpl = unsafe { core::ptr::read_volatile(self.position.ptr::<u32>(self.stream_index() * 8)) } as usize;
        let lpib = self.mmio.read32(self.stream_base + SD_LPIB) as usize;
        let bytes = if lpib == 0 && dpl != 0 { dpl } else { lpib };
        (bytes / 4) % RING_FRAMES
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        self.setup_stream();
        let base = self.stream_base;
        self.mmio.write8(base + SD_CTL, self.mmio.read8(base + SD_CTL) | 0x02);
        self.running = true;
    }

    fn stop(&mut self) {
        let base = self.stream_base;
        self.mmio.write8(base + SD_CTL, self.mmio.read8(base + SD_CTL) & !0x02);
        self.running = false;
    }

    fn keep_alive(&mut self) {
        let base = self.stream_base;
        let status = self.mmio.read8(base + SD_STS);
        if status & 0x1C != 0 {
            self.mmio.write8(base + SD_STS, status & 0x1C);
        }
        if self.running && self.mmio.read8(base + SD_CTL) & 0x02 == 0 {
            self.mmio.write8(base + SD_CTL, self.mmio.read8(base + SD_CTL) | 0x02);
        }
    }

    fn poll(&mut self) {
        self.update_jacks(false);
    }

    fn outputs(&self) -> String {
        let mut names: Vec<&str> = Vec::new();
        for o in self.outputs.iter() {
            let name = match o.device {
                DEVICE_SPEAKER => "speaker",
                DEVICE_HP_OUT => "headphones",
                _ => "line out",
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        let mut text = names.join(" + ");
        if self.headphones_in {
            text.push_str(" (headphones plugged in)");
        }
        text
    }
}

impl Hda {
    fn stream_index(&self) -> usize {
        ((self.stream_base - 0x80) / 0x20) as usize
    }
}

fn controller_name(dev: &PciDevice) -> String {
    let vendor = match dev.vendor {
        0x8086 => "Intel",
        0x1002 | 0x1022 => "AMD",
        0x10DE => "NVIDIA",
        0x1106 => "VIA",
        0x1039 => "SiS",
        0x10B9 => "ULi",
        0x6549 => "Teradici",
        0x15AD => "VMware",
        _ => "",
    };
    if vendor.is_empty() { format!("HDA controller {:04x}:{:04x}", dev.vendor, dev.device) } else { format!("{} HDA {:04x}", vendor, dev.device) }
}

pub fn probe(devices: &[PciDevice]) -> Option<Box<dyn AudioDevice>> {
    for dev in devices.iter().filter(|d| d.class == 0x04 && d.subclass == 0x03) {
        let Some(base) = dev.mmio_bar(0) else {
            continue;
        };
        if base == 0 {
            continue;
        }
        pci::enable_bus_master(dev.address);
        if dev.vendor == 0x8086 {
            let tcsel = pci::read_config_u32(dev.address, 0x44);
            pci::write_config_u32(dev.address, 0x44, tcsel & !0xFF);
        }
        let (Some(bdl), Some(buffer), Some(position)) = (DmaRegion::new(4096), DmaRegion::new(RING_FRAMES * 4), DmaRegion::new(4096)) else {
            continue;
        };
        let mut hda = Hda {
            mmio: Mmio { base },
            corb: None,
            rirb: None,
            corb_entries: 0,
            rirb_entries: 0,
            rirb_read: 0,
            stream_base: 0,
            bdl,
            buffer,
            position,
            codec: 0,
            codec_name: String::new(),
            rate: 48000,
            format: 0x0011,
            outputs: Vec::new(),
            dacs: Vec::new(),
            running: false,
            headphones_in: false,
            controller: controller_name(dev),
        };
        hda.mmio.write32(INTCTL, 0);
        if !hda.reset() {
            crate::drivers::klog::log(&format!("hda: {} did not leave reset", hda.controller));
            continue;
        }
        let gcap = hda.mmio.read16(GCAP);
        let iss = ((gcap >> 8) & 0xF) as u32;
        let oss = ((gcap >> 12) & 0xF) as u32;
        if oss == 0 {
            continue;
        }
        hda.stream_base = 0x80 + iss * 0x20;
        hda.mmio.write32(DPUBASE, (hda.position.phys >> 32) as u32);
        hda.mmio.write32(DPLBASE, hda.position.phys as u32 | 1);
        hda.setup_rings();
        if !hda.configure_codec() {
            crate::drivers::klog::log(&format!("hda: {} has no usable output codec", hda.controller));
            continue;
        }
        crate::drivers::klog::log(&format!("hda: {} codec {} at {} Hz, outputs {}", hda.controller, hda.codec_name, hda.rate, hda.outputs()));
        return Some(Box::new(hda));
    }
    None
}
