use crate::bios;

pub const GUID: [u8; 16] = [0x20, 0x20, 0x77, 0x77, 0x77, 0x2e, 0x76, 0x65, 0x6e, 0x74, 0x6f, 0x79, 0x2e, 0x6e, 0x65, 0x74];
pub const PARAM_SIZE: u32 = 512;
pub const PARAM_ADDRESS: u32 = 0x80000;
pub const INJECT_MAX: u32 = 0x2000;
const LOCATION_ADDR: u32 = 16 + 1 + 16 + 8 + 2 + 2 + 384 + 8;
const LOCATION_LEN: u32 = LOCATION_ADDR + 8;

#[derive(Clone, Copy)]
pub struct Found {
    pub param: u32,
    pub location: u32,
    pub location_len: u32,
}

pub fn find() -> Option<Found> {
    let mut at = PARAM_ADDRESS;
    while at + PARAM_SIZE <= 0xa0000 {
        if bios::bytes(at, 16) == GUID && bios::bytes(at, PARAM_SIZE as usize).iter().fold(0u8, |a, &b| a.wrapping_add(b)) == 0 {
            let address = bios::read_u64(at + LOCATION_ADDR);
            let len = bios::read_u32(at + LOCATION_LEN);
            let (location, location_len) = if address != 0 && address < 1 << 32 && len > 0 && len <= 0x1800 { (address as u32, len) } else { (0, 0) };
            return Some(Found { param: at, location, location_len });
        }
        at += 1;
    }
    None
}
