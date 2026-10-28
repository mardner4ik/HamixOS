use crate::acpi::Rsdp;
use crate::bios;
use crate::layout::{MB_INFO, MB_INFO_SIZE};
use crate::memmap::Map;
use crate::vbe::Framebuffer;
use crate::ventoy;

pub const TAG_VENTOY: u32 = 0x4858_5601;

#[derive(Clone, Copy)]
pub struct Module {
    pub start: u32,
    pub end: u32,
    pub name: &'static str,
}

impl Module {
    pub const EMPTY: Module = Module { start: 0, end: 0, name: "" };
}

struct Writer {
    at: u32,
}

impl Writer {
    fn room(&self, len: u32) -> bool {
        self.at + len + 16 <= MB_INFO + MB_INFO_SIZE
    }

    fn tag(&mut self, kind: u32, body: &[&[u8]]) {
        let len = 8 + body.iter().map(|b| b.len() as u32).sum::<u32>();
        if !self.room(len) {
            return;
        }
        bios::write_u32(self.at, kind);
        bios::write_u32(self.at + 4, len);
        let mut p = self.at + 8;
        for part in body {
            bios::bytes_mut(p, part.len()).copy_from_slice(part);
            p += part.len() as u32;
        }
        self.at = (self.at + len + 7) & !7;
    }
}

pub fn build(cmdline: &str, modules: &[Module], map: &Map, framebuffer: Option<Framebuffer>, rsdp: Option<Rsdp>, ventoy: Option<ventoy::Found>) -> u32 {
    bios::bytes_mut(MB_INFO, MB_INFO_SIZE as usize).fill(0);
    let mut w = Writer { at: MB_INFO + 8 };
    w.tag(1, &[cmdline.as_bytes(), &[0]]);
    w.tag(2, &[b"hxboot ", env!("CARGO_PKG_VERSION").as_bytes(), &[0]]);
    for m in modules {
        w.tag(3, &[&m.start.to_le_bytes(), &m.end.to_le_bytes(), m.name.as_bytes(), &[0]]);
    }
    let mut entries = [0u8; 24 * crate::memmap::MAX];
    for (i, r) in map.regions[..map.count].iter().enumerate() {
        let e = &mut entries[i * 24..i * 24 + 24];
        e[0..8].copy_from_slice(&r.base.to_le_bytes());
        e[8..16].copy_from_slice(&r.len.to_le_bytes());
        e[16..20].copy_from_slice(&r.kind.to_le_bytes());
    }
    w.tag(6, &[&24u32.to_le_bytes(), &0u32.to_le_bytes(), &entries[..map.count * 24]]);
    if let Some(fb) = framebuffer {
        w.tag(
            8,
            &[
                &(fb.addr as u64).to_le_bytes(),
                &fb.pitch.to_le_bytes(),
                &fb.width.to_le_bytes(),
                &fb.height.to_le_bytes(),
                &[fb.bpp, 1, 0, 0],
                &[fb.red.0, fb.red.1, fb.green.0, fb.green.1, fb.blue.0, fb.blue.1],
            ],
        );
    }
    if let Some(r) = rsdp {
        if r.revision >= 2 {
            w.tag(15, &[bios::bytes(r.address, 36)]);
        } else {
            w.tag(14, &[bios::bytes(r.address, 20)]);
        }
    }
    if let Some(v) = ventoy {
        let location = if v.location_len > 0 { bios::bytes(v.location, v.location_len as usize) } else { &[] };
        w.tag(TAG_VENTOY, &[bios::bytes(v.param, ventoy::PARAM_SIZE as usize), location]);
    }
    w.tag(0, &[]);
    bios::write_u32(MB_INFO, w.at - MB_INFO);
    MB_INFO
}
