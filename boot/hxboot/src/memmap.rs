use crate::bios::{self, Regs};
use crate::layout::LOW;

pub const MAX: usize = 128;
const ENTRY: u32 = LOW + 0x100;

#[derive(Clone, Copy)]
pub struct Region {
    pub base: u64,
    pub len: u64,
    pub kind: u32,
}

pub struct Map {
    pub regions: [Region; MAX],
    pub count: usize,
}

static mut MAP: Map = Map { regions: [Region { base: 0, len: 0, kind: 0 }; MAX], count: 0 };

pub fn read() -> &'static Map {
    let map = unsafe { &mut *(&raw mut MAP) };
    map.count = 0;
    let mut next = 0u32;
    loop {
        bios::bytes_mut(ENTRY, 24).fill(0);
        bios::write_u32(ENTRY + 20, 1);
        let mut r = Regs { eax: 0xe820, ebx: next, ecx: 24, edx: 0x534d4150, edi: bios::offset(ENTRY), es: bios::segment(ENTRY), ..Regs::default() };
        bios::int(0x15, &mut r);
        if r.carry() || r.eax != 0x534d4150 {
            break;
        }
        let len = bios::read_u64(ENTRY + 8);
        let attributes = if r.ecx >= 24 { bios::read_u32(ENTRY + 20) } else { 1 };
        if len != 0 && attributes & 1 != 0 && map.count < MAX {
            map.regions[map.count] = Region { base: bios::read_u64(ENTRY), len, kind: bios::read_u32(ENTRY + 16) };
            map.count += 1;
        }
        next = r.ebx;
        if next == 0 {
            break;
        }
    }
    if map.count == 0 {
        let mut r = Regs { eax: 0xe801, ..Regs::default() };
        bios::int(0x15, &mut r);
        let (low, high) = if r.eax & 0xffff != 0 { (r.eax & 0xffff, r.ebx & 0xffff) } else { (r.ecx & 0xffff, r.edx & 0xffff) };
        map.regions[0] = Region { base: 0, len: 0x9fc00, kind: 1 };
        map.regions[1] = Region { base: 0x100000, len: low as u64 * 1024, kind: 1 };
        map.regions[2] = Region { base: 0x1000000, len: high as u64 * 65536, kind: 1 };
        map.count = 3;
    }
    map
}

pub fn fits(start: u32, size: u32) -> bool {
    let map = read();
    let end = start as u64 + size as u64;
    map.regions[..map.count].iter().any(|r| r.kind == 1 && r.base <= start as u64 && r.base + r.len >= end) && end < (1u64 << 32)
}
