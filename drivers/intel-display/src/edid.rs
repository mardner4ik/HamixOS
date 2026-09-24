use hamix_kpi as kpi;

use crate::regs::Mmio;

const GMBUS0: u32 = 0xC5100;
const GMBUS1: u32 = 0xC5104;
const GMBUS2: u32 = 0xC5108;
const GMBUS3: u32 = 0xC510C;

const GMBUS_RATE_100KHZ: u32 = 0;
const GMBUS_SW_RDY: u32 = 1 << 30;
const GMBUS_SW_CLR_INT: u32 = 1 << 31;
const GMBUS_CYCLE_WAIT: u32 = 1 << 25;
const GMBUS_CYCLE_INDEX: u32 = 1 << 26;
const GMBUS_CYCLE_STOP: u32 = 1 << 27;
const GMBUS_READ: u32 = 1;
const GMBUS_ACTIVE: u32 = 1 << 9;
const GMBUS_HW_RDY: u32 = 1 << 11;
const GMBUS_SATOER: u32 = 1 << 10;

const PINS: [u32; 6] = [2, 3, 4, 5, 6, 1];
const EDID_ADDR: u32 = 0x50;
const EDID_LEN: usize = 128;

#[derive(Clone, Copy, Default)]
pub struct Limits {
    pub min_vertical_hz: u32,
    pub max_vertical_hz: u32,
    pub preferred: (u32, u32),
    pub name: [u8; 14],
    pub name_len: usize,
}

fn wait(mmio: &Mmio, mask: u32, us: u64) -> bool {
    let mut waited = 0u64;
    while waited < us {
        let status = mmio.read(GMBUS2);
        if status & GMBUS_SATOER != 0 {
            return false;
        }
        if status & mask != 0 {
            return true;
        }
        unsafe { kpi::hamix_udelay(10) };
        waited += 10;
    }
    false
}

fn idle(mmio: &Mmio) {
    mmio.write(GMBUS1, GMBUS_SW_CLR_INT);
    mmio.write(GMBUS1, 0);
    mmio.write(GMBUS0, 0);
}

fn read_block(mmio: &Mmio, pin: u32, out: &mut [u8; EDID_LEN]) -> bool {
    mmio.write(GMBUS0, GMBUS_RATE_100KHZ | pin);
    mmio.write(GMBUS1, GMBUS_SW_CLR_INT);
    mmio.write(GMBUS1, 0);

    mmio.write(GMBUS3, 0);
    mmio.write(GMBUS1, GMBUS_SW_RDY | GMBUS_CYCLE_WAIT | GMBUS_CYCLE_INDEX | (1 << 16) | (EDID_ADDR << 1));
    if !wait(mmio, GMBUS_HW_RDY, 50_000) {
        idle(mmio);
        return false;
    }

    mmio.write(GMBUS1, GMBUS_SW_RDY | GMBUS_CYCLE_WAIT | GMBUS_CYCLE_STOP | ((EDID_LEN as u32) << 16) | (EDID_ADDR << 1) | GMBUS_READ);
    let mut at = 0usize;
    while at < EDID_LEN {
        if !wait(mmio, GMBUS_HW_RDY, 50_000) {
            idle(mmio);
            return false;
        }
        let word = mmio.read(GMBUS3);
        for shift in 0..4 {
            if at < EDID_LEN {
                out[at] = (word >> (shift * 8)) as u8;
                at += 1;
            }
        }
    }
    wait(mmio, GMBUS_ACTIVE, 10_000);
    idle(mmio);
    out[0] == 0x00 && out[1] == 0xFF && out[2] == 0xFF && out[7] == 0x00
}

fn checksum_ok(block: &[u8; EDID_LEN]) -> bool {
    block.iter().fold(0u8, |acc, b| acc.wrapping_add(*b)) == 0
}

pub fn parse(block: &[u8; EDID_LEN]) -> Limits {
    let mut limits = Limits::default();
    for at in (54..126).step_by(18) {
        let d = &block[at..at + 18];
        if d[0] == 0 && d[1] == 0 && d[2] == 0 {
            match d[3] {
                0xFD => {
                    limits.min_vertical_hz = d[5] as u32;
                    limits.max_vertical_hz = d[6] as u32;
                }
                0xFC => {
                    let mut len = 0usize;
                    for byte in &d[5..18] {
                        if *byte == 0x0A {
                            break;
                        }
                        limits.name[len] = *byte;
                        len += 1;
                    }
                    limits.name_len = len;
                }
                _ => {}
            }
            continue;
        }
        if limits.preferred == (0, 0) {
            let width = d[2] as u32 | (((d[4] as u32) & 0xF0) << 4);
            let height = d[5] as u32 | (((d[7] as u32) & 0xF0) << 4);
            if width >= 320 && height >= 200 {
                limits.preferred = (width, height);
            }
        }
    }
    limits
}

pub fn read_pin(mmio: &Mmio, pin: u32, out: &mut [u8; 256]) -> usize {
    let mut block = [0u8; EDID_LEN];
    if read_block(mmio, pin, &mut block) && checksum_ok(&block) {
        out[..EDID_LEN].copy_from_slice(&block);
        return EDID_LEN;
    }
    0
}

pub fn limits_of(raw: &[u8; 256]) -> Limits {
    let mut block = [0u8; EDID_LEN];
    block.copy_from_slice(&raw[..EDID_LEN]);
    parse(&block)
}

pub fn read(mmio: &Mmio) -> Option<Limits> {
    let mut block = [0u8; EDID_LEN];
    for pin in PINS {
        if read_block(mmio, pin, &mut block) && checksum_ok(&block) {
            return Some(parse(&block));
        }
    }
    None
}
