use hamix_kpi::io::{delay_us, Mmio};

const GMBUS0: u32 = 0x5100;
const GMBUS1: u32 = 0x5104;
const GMBUS2: u32 = 0x5108;
const GMBUS3: u32 = 0x510C;

const SW_RDY: u32 = 1 << 30;
const SW_CLR_INT: u32 = 1 << 31;
const CYCLE_WAIT: u32 = 1 << 25;
const CYCLE_INDEX: u32 = 1 << 26;
const CYCLE_STOP: u32 = 1 << 27;
const READ: u32 = 1;
const ACTIVE: u32 = 1 << 9;
const HW_RDY: u32 = 1 << 11;
const SATOER: u32 = 1 << 10;

pub const PIN_VGA: u32 = 2;
pub const PIN_PANEL: u32 = 3;

const EDID_ADDR: u32 = 0x50;
pub const EDID_LEN: usize = 128;

fn wait(mmio: &Mmio, mask: u32, us: u64) -> bool {
    let mut waited = 0u64;
    while waited < us {
        let status = mmio.read32(GMBUS2);
        if status & SATOER != 0 {
            return false;
        }
        if status & mask != 0 {
            return true;
        }
        delay_us(10);
        waited += 10;
    }
    false
}

fn idle(mmio: &Mmio) {
    mmio.write32(GMBUS1, SW_CLR_INT);
    mmio.write32(GMBUS1, 0);
    mmio.write32(GMBUS0, 0);
}

pub fn read_edid(mmio: &Mmio, pin: u32, out: &mut [u8; EDID_LEN]) -> bool {
    mmio.write32(GMBUS0, pin);
    mmio.write32(GMBUS1, SW_CLR_INT);
    mmio.write32(GMBUS1, 0);
    mmio.write32(GMBUS3, 0);
    mmio.write32(GMBUS1, SW_RDY | CYCLE_WAIT | CYCLE_INDEX | (1 << 16) | (EDID_ADDR << 1));
    if !wait(mmio, HW_RDY, 50_000) {
        idle(mmio);
        return false;
    }
    mmio.write32(GMBUS1, SW_RDY | CYCLE_WAIT | CYCLE_STOP | ((EDID_LEN as u32) << 16) | (EDID_ADDR << 1) | READ);
    let mut at = 0usize;
    while at < EDID_LEN {
        if !wait(mmio, HW_RDY, 50_000) {
            idle(mmio);
            return false;
        }
        let word = mmio.read32(GMBUS3);
        for shift in 0..4 {
            if at < EDID_LEN {
                out[at] = (word >> (shift * 8)) as u8;
                at += 1;
            }
        }
    }
    wait(mmio, ACTIVE, 10_000);
    idle(mmio);
    let header = out[0] == 0x00 && out[1] == 0xFF && out[2] == 0xFF && out[7] == 0x00;
    header && out.iter().fold(0u8, |acc, b| acc.wrapping_add(*b)) == 0
}
