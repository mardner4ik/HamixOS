use crate::bios;

pub struct Rsdp {
    pub address: u32,
    pub revision: u8,
}

fn valid(address: u32) -> Option<Rsdp> {
    if bios::bytes(address, 8) != b"RSD PTR " {
        return None;
    }
    if bios::bytes(address, 20).iter().fold(0u8, |a, &b| a.wrapping_add(b)) != 0 {
        return None;
    }
    let revision = bios::read_u8(address + 15);
    if revision >= 2 && bios::bytes(address, 36).iter().fold(0u8, |a, &b| a.wrapping_add(b)) != 0 {
        return Some(Rsdp { address, revision: 0 });
    }
    Some(Rsdp { address, revision })
}

pub fn find_rsdp() -> Option<Rsdp> {
    let ebda = (bios::read_u16(0x40e) as u32) << 4;
    if (0x80000..0xa0000).contains(&ebda) {
        let mut at = ebda;
        while at < ebda + 1024 {
            if let Some(r) = valid(at) {
                return Some(r);
            }
            at += 16;
        }
    }
    let mut at = 0xe0000;
    while at < 0x100000 {
        if let Some(r) = valid(at) {
            return Some(r);
        }
        at += 16;
    }
    None
}
