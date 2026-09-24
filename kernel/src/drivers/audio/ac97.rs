use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

use super::AudioDevice;
use crate::arch::io::{inb, inl, inw, outb, outl, outw};
use crate::arch::delay_us as pit_delay_us;
use crate::drivers::pci::{self, PciDevice};
use crate::net::dma::DmaRegion;

const NAM_RESET: u16 = 0x00;
const NAM_MASTER: u16 = 0x02;
const NAM_HEADPHONE: u16 = 0x04;
const NAM_PCM_OUT: u16 = 0x18;
const NAM_EXT_ID: u16 = 0x28;
const NAM_EXT_CTRL: u16 = 0x2A;
const NAM_FRONT_RATE: u16 = 0x2C;
const NAM_VENDOR1: u16 = 0x7C;
const NAM_VENDOR2: u16 = 0x7E;

const PO_BDBAR: u16 = 0x10;
const PO_CIV: u16 = 0x14;
const PO_LVI: u16 = 0x15;
const PO_SR: u16 = 0x16;
const PO_PICB: u16 = 0x18;
const PO_CR: u16 = 0x1B;
const GLOB_CNT: u16 = 0x2C;
const GLOB_STA: u16 = 0x30;

const ENTRIES: usize = 32;
const ENTRY_FRAMES: usize = 512;
const RING_FRAMES: usize = ENTRIES * ENTRY_FRAMES;

pub struct Ac97 {
    nabm: u16,
    bdl: DmaRegion,
    buffer: DmaRegion,
    rate: u32,
    running: bool,
    sis: bool,
    codec: String,
    controller: String,
}

unsafe impl Send for Ac97 {}

impl Ac97 {
    fn sr_port(&self) -> u16 {
        self.nabm + if self.sis { PO_PICB } else { PO_SR }
    }

    fn picb_port(&self) -> u16 {
        self.nabm + if self.sis { PO_SR } else { PO_PICB }
    }

    fn reset_stream(&mut self) {
        outb(self.nabm + PO_CR, 0);
        outb(self.nabm + PO_CR, 0x02);
        for _ in 0..100 {
            if inb(self.nabm + PO_CR) & 0x02 == 0 {
                break;
            }
            pit_delay_us(1000);
        }
        outb(self.nabm + PO_CR, 0);
        for i in 0..ENTRIES {
            let samples = if self.sis { ENTRY_FRAMES * 4 } else { ENTRY_FRAMES * 2 };
            unsafe {
                let e = self.bdl.ptr::<u8>(i * 8);
                core::ptr::write_volatile(e as *mut u32, (self.buffer.phys + (i * ENTRY_FRAMES * 4) as u64) as u32);
                core::ptr::write_volatile(e.add(4) as *mut u16, samples as u16);
                core::ptr::write_volatile(e.add(6) as *mut u16, 0);
            }
        }
        outl(self.nabm + PO_BDBAR, self.bdl.phys as u32);
        outb(self.nabm + PO_LVI, (ENTRIES - 1) as u8);
        outw(self.sr_port(), 0x1C);
    }
}

impl AudioDevice for Ac97 {
    fn name(&self) -> String {
        format!("{} on {}", self.codec, self.controller)
    }

    fn driver(&self) -> &'static str {
        "AC'97"
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
        let civ = (inb(self.nabm + PO_CIV) as usize) % ENTRIES;
        let picb = inw(self.picb_port()) as usize;
        let remaining = if self.sis { picb / 4 } else { picb / 2 };
        let done = ENTRY_FRAMES.saturating_sub(remaining.min(ENTRY_FRAMES));
        (civ * ENTRY_FRAMES + done) % RING_FRAMES
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        self.reset_stream();
        outb(self.nabm + PO_CR, 0x01);
        self.running = true;
    }

    fn stop(&mut self) {
        outb(self.nabm + PO_CR, 0);
        self.running = false;
    }

    fn keep_alive(&mut self) {
        if !self.running {
            return;
        }
        let civ = inb(self.nabm + PO_CIV) as usize;
        outb(self.nabm + PO_LVI, ((civ + ENTRIES - 1) % ENTRIES) as u8);
        let status = inw(self.sr_port());
        if status & 0x1C != 0 {
            outw(self.sr_port(), status & 0x1C);
        }
        if status & 0x01 != 0 {
            outb(self.nabm + PO_CR, 0x01);
        }
    }

    fn outputs(&self) -> String {
        String::from("line out")
    }
}

fn codec_name(v1: u16, v2: u16) -> String {
    let id = ((v1 as u32) << 16) | v2 as u32;
    let prefix = [(id >> 24) as u8, (id >> 16) as u8, (id >> 8) as u8];
    let brand = match &prefix {
        b"ADS" => "Analog Devices",
        b"ALG" => "Realtek",
        b"CRY" => "Cirrus Logic",
        b"SIG" => "SigmaTel",
        b"TRA" => "TriTech",
        b"VIA" => "VIA",
        b"YMH" => "Yamaha",
        b"CXT" => "Conexant",
        b"EMC" => "eMicro",
        b"ICE" => "ICEnsemble",
        b"KBD" => "KBD",
        b"83\x84" | b"\x83\x84\x76" => "SigmaTel",
        _ => "",
    };
    if brand.is_empty() { format!("AC'97 codec {:08x}", id) } else { format!("{} AC'97 codec {:02x}", brand, id & 0xFF) }
}

pub fn probe(devices: &[PciDevice]) -> Option<Box<dyn AudioDevice>> {
    for dev in devices.iter().filter(|d| d.class == 0x04 && d.subclass == 0x01) {
        let (Some(nam), Some(nabm)) = (dev.io_bar(0), dev.io_bar(1)) else {
            continue;
        };
        pci::enable_bus_master(dev.address);
        let sis = dev.vendor == 0x1039;
        let control = inl(nabm + GLOB_CNT);
        outl(nabm + GLOB_CNT, (control & !0x04) | 0x02);
        let mut ready = false;
        for _ in 0..600 {
            if inl(nabm + GLOB_STA) & (1 << 8) != 0 {
                ready = true;
                break;
            }
            pit_delay_us(1000);
        }
        if !ready {
            crate::drivers::klog::log("ac97: codec not ready");
        }
        outw(nam + NAM_RESET, 0);
        pit_delay_us(20_000);
        outw(nam + NAM_MASTER, 0x0000);
        outw(nam + NAM_HEADPHONE, 0x0000);
        outw(nam + NAM_PCM_OUT, 0x0808);
        let mut rate = 48000;
        if inw(nam + NAM_EXT_ID) & 1 != 0 {
            outw(nam + NAM_EXT_CTRL, inw(nam + NAM_EXT_CTRL) | 1);
            pit_delay_us(10_000);
            outw(nam + NAM_FRONT_RATE, 48000);
            pit_delay_us(10_000);
            let actual = inw(nam + NAM_FRONT_RATE) as u32;
            if actual >= 8000 {
                rate = actual;
            }
        }
        let (Some(bdl), Some(buffer)) = (DmaRegion::new(ENTRIES * 8), DmaRegion::new(RING_FRAMES * 4)) else {
            continue;
        };
        let codec = codec_name(inw(nam + NAM_VENDOR1), inw(nam + NAM_VENDOR2));
        let controller = match dev.vendor {
            0x8086 => format!("Intel ICH AC'97 {:04x}", dev.device),
            0x10DE => format!("NVIDIA nForce AC'97 {:04x}", dev.device),
            0x1039 => format!("SiS 7012 AC'97"),
            0x1022 => format!("AMD AC'97 {:04x}", dev.device),
            _ => format!("AC'97 controller {:04x}:{:04x}", dev.vendor, dev.device),
        };
        let mut device = Ac97 { nabm, bdl, buffer, rate, running: false, sis, codec, controller };
        device.reset_stream();
        crate::drivers::klog::log(&format!("ac97: {} at {} Hz", device.name(), rate));
        return Some(Box::new(device));
    }
    None
}
