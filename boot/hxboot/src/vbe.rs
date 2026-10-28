use crate::bios::{self, Regs};
use crate::console;
use crate::layout::{LOW, MODES};

const INFO: u32 = LOW + 0x200;
const MODE: u32 = LOW + 0x400;

#[derive(Clone, Copy)]
pub struct Framebuffer {
    pub addr: u32,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub red: (u8, u8),
    pub green: (u8, u8),
    pub blue: (u8, u8),
}

fn mode_info(mode: u16) -> Option<Framebuffer> {
    bios::bytes_mut(MODE, 256).fill(0);
    let mut r = Regs { eax: 0x4f01, ecx: mode as u32, edi: bios::offset(MODE), es: bios::segment(MODE), ..Regs::default() };
    bios::int(0x10, &mut r);
    if r.eax & 0xffff != 0x004f {
        return None;
    }
    let attributes = bios::read_u16(MODE);
    if attributes & 0x99 != 0x99 || bios::read_u8(MODE + 27) != 6 {
        return None;
    }
    let addr = bios::read_u32(MODE + 40);
    if addr == 0 {
        return None;
    }
    let linear_pitch = bios::read_u16(MODE + 50) as u32;
    let pitch = if linear_pitch != 0 { linear_pitch } else { bios::read_u16(MODE + 16) as u32 };
    Some(Framebuffer {
        addr,
        pitch,
        width: bios::read_u16(MODE + 18) as u32,
        height: bios::read_u16(MODE + 20) as u32,
        bpp: bios::read_u8(MODE + 25),
        red: (bios::read_u8(MODE + 32), bios::read_u8(MODE + 31)),
        green: (bios::read_u8(MODE + 34), bios::read_u8(MODE + 33)),
        blue: (bios::read_u8(MODE + 36), bios::read_u8(MODE + 35)),
    })
}

fn score(fb: &Framebuffer) -> u32 {
    if fb.bpp != 32 && fb.bpp != 24 {
        return 0;
    }
    if fb.width > 1920 || fb.height > 1200 {
        return 0;
    }
    let preferred = match (fb.width, fb.height) {
        (1024, 768) => 4_000_000,
        (1366, 768) => 3_000_000,
        (1280, 800) | (1280, 1024) => 2_500_000,
        (800, 600) => 2_000_000,
        _ => fb.width * fb.height,
    };
    preferred + if fb.bpp == 32 { 1 } else { 0 }
}

pub fn set_best_mode() -> Option<Framebuffer> {
    bios::bytes_mut(INFO, 512).fill(0);
    bios::bytes_mut(INFO, 4).copy_from_slice(b"VBE2");
    let mut r = Regs { eax: 0x4f00, edi: bios::offset(INFO), es: bios::segment(INFO), ..Regs::default() };
    bios::int(0x10, &mut r);
    if r.eax & 0xffff != 0x004f || bios::bytes(INFO, 4) != b"VESA" {
        return None;
    }
    let list = bios::far_to_linear(bios::read_u32(INFO + 14));
    let mut count = 0usize;
    while count < 1024 {
        let mode = bios::read_u16(list + count as u32 * 2);
        if mode == 0xffff {
            break;
        }
        bios::write_u16(MODES + count as u32 * 2, mode);
        count += 1;
    }
    let mut best: Option<(u16, Framebuffer, u32)> = None;
    for i in 0..count {
        let mode = bios::read_u16(MODES + i as u32 * 2);
        if let Some(fb) = mode_info(mode) {
            let s = score(&fb);
            if s > 0 && best.map(|(_, _, b)| s > b).unwrap_or(true) {
                best = Some((mode, fb, s));
            }
        }
    }
    let (mode, fb, _) = best?;
    let mut r = Regs { eax: 0x4f02, ebx: mode as u32 | 0x4000, ..Regs::default() };
    bios::int(0x10, &mut r);
    if r.eax & 0xffff != 0x004f {
        return None;
    }
    console::graphics();
    Some(fb)
}
