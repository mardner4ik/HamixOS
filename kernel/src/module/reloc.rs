use alloc::collections::BTreeMap;

use super::image::Image;

pub struct Tables {
    pub got: BTreeMap<u64, usize>,
    pub next_got: usize,
    pub stubs: BTreeMap<u64, usize>,
    pub next_stub: usize,
    pub hi: BTreeMap<u64, i64>,
}

impl Tables {
    fn got_slot(&mut self, image: &mut Image, symbol: u64) -> usize {
        let next = &mut self.next_got;
        let slot = *self.got.entry(symbol).or_insert_with(|| {
            let at = *next;
            *next += 8;
            at
        });
        image.bytes_mut()[slot..slot + 8].copy_from_slice(&symbol.to_le_bytes());
        slot
    }
}

fn put32(image: &mut Image, at: usize, value: u32) {
    image.bytes_mut()[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn get32(image: &mut Image, at: usize) -> u32 {
    let b = image.bytes_mut();
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn put64(image: &mut Image, at: usize, value: u64) {
    image.bytes_mut()[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn fits_signed(value: i64, bits: u32) -> bool {
    let limit = 1i64 << (bits - 1);
    value >= -limit && value < limit
}

#[cfg(target_arch = "x86_64")]
mod arch {
    use super::*;

    pub const STUB_SIZE: usize = 16;
    pub const PASSES: usize = 1;

    const R_X86_64_NONE: u32 = 0;
    const R_X86_64_64: u32 = 1;
    const R_X86_64_PC32: u32 = 2;
    const R_X86_64_GOT32: u32 = 3;
    const R_X86_64_PLT32: u32 = 4;
    const R_X86_64_GOTPCREL: u32 = 9;
    const R_X86_64_32: u32 = 10;
    const R_X86_64_32S: u32 = 11;
    const R_X86_64_16: u32 = 12;
    const R_X86_64_PC16: u32 = 13;
    const R_X86_64_8: u32 = 14;
    const R_X86_64_PC8: u32 = 15;
    const R_X86_64_PC64: u32 = 24;
    const R_X86_64_GOTPCRELX: u32 = 41;
    const R_X86_64_REX_GOTPCRELX: u32 = 42;

    pub fn needs_got(kind: u32) -> bool {
        matches!(kind, R_X86_64_GOT32 | R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX)
    }

    pub fn needs_stub(_kind: u32) -> bool {
        false
    }

    pub fn in_pass(_kind: u32, _pass: usize) -> bool {
        true
    }

    pub fn apply(image: &mut Image, base: u64, tables: &mut Tables, kind: u32, at: usize, symbol: u64, addend: i64) -> Result<(), &'static str> {
        if kind == R_X86_64_NONE {
            return Ok(());
        }
        let place = base + at as u64;
        let value: i64 = match kind {
            R_X86_64_64 => symbol as i64 + addend,
            R_X86_64_PC32 | R_X86_64_PLT32 => symbol as i64 + addend - place as i64,
            R_X86_64_32 | R_X86_64_32S => symbol as i64 + addend,
            R_X86_64_16 | R_X86_64_8 => symbol as i64 + addend,
            R_X86_64_PC16 | R_X86_64_PC8 => symbol as i64 + addend - place as i64,
            R_X86_64_PC64 => symbol as i64 + addend - place as i64,
            R_X86_64_GOT32 | R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX => {
                let slot_addr = base + tables.got_slot(image, symbol) as u64;
                if kind == R_X86_64_GOT32 {
                    slot_addr as i64 - base as i64 + addend
                } else {
                    slot_addr as i64 + addend - place as i64
                }
            }
            _ => return Err("module: unsupported relocation type"),
        };
        let bytes = image.bytes_mut();
        match kind {
            R_X86_64_64 | R_X86_64_PC64 => bytes[at..at + 8].copy_from_slice(&value.to_le_bytes()),
            R_X86_64_PC32 | R_X86_64_PLT32 | R_X86_64_GOT32 | R_X86_64_GOTPCREL | R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX => {
                if !fits_signed(value, 32) {
                    return Err("module: relocation does not fit in 32 bits");
                }
                bytes[at..at + 4].copy_from_slice(&(value as i32).to_le_bytes());
            }
            R_X86_64_32 => {
                if value < 0 || value > u32::MAX as i64 {
                    return Err("module: 32-bit relocation overflowed");
                }
                bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
            }
            R_X86_64_32S => {
                if !fits_signed(value, 32) {
                    return Err("module: signed 32-bit relocation overflowed");
                }
                bytes[at..at + 4].copy_from_slice(&(value as i32).to_le_bytes());
            }
            R_X86_64_16 | R_X86_64_PC16 => bytes[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes()),
            R_X86_64_8 | R_X86_64_PC8 => bytes[at] = value as u8,
            _ => {}
        }
        Ok(())
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::*;

    pub const STUB_SIZE: usize = 16;
    pub const PASSES: usize = 1;

    const NONE: u32 = 0;
    const NONE_ALT: u32 = 256;
    const ABS64: u32 = 257;
    const ABS32: u32 = 258;
    const ABS16: u32 = 259;
    const PREL64: u32 = 260;
    const PREL32: u32 = 261;
    const PREL16: u32 = 262;
    const ADR_PREL_LO21: u32 = 274;
    const ADR_PREL_PG_HI21: u32 = 275;
    const ADR_PREL_PG_HI21_NC: u32 = 276;
    const ADD_ABS_LO12_NC: u32 = 277;
    const LDST8_ABS_LO12_NC: u32 = 278;
    const TSTBR14: u32 = 279;
    const CONDBR19: u32 = 280;
    const JUMP26: u32 = 282;
    const CALL26: u32 = 283;
    const LDST16_ABS_LO12_NC: u32 = 284;
    const LDST32_ABS_LO12_NC: u32 = 285;
    const LDST64_ABS_LO12_NC: u32 = 286;
    const LDST128_ABS_LO12_NC: u32 = 299;
    const ADR_GOT_PAGE: u32 = 311;
    const LD64_GOT_LO12_NC: u32 = 312;

    pub fn needs_got(kind: u32) -> bool {
        matches!(kind, ADR_GOT_PAGE | LD64_GOT_LO12_NC)
    }

    pub fn needs_stub(kind: u32) -> bool {
        matches!(kind, JUMP26 | CALL26)
    }

    pub fn in_pass(_kind: u32, _pass: usize) -> bool {
        true
    }

    fn page(value: u64) -> i64 {
        (value & !0xFFF) as i64
    }

    fn set_adr(image: &mut Image, at: usize, imm: i64) {
        let insn = get32(image, at) & !((0x3 << 29) | (0x7FFFF << 5));
        let imm = imm as u32;
        put32(image, at, insn | ((imm & 0x3) << 29) | (((imm >> 2) & 0x7FFFF) << 5));
    }

    fn set_imm12(image: &mut Image, at: usize, value: u64) {
        let insn = get32(image, at) & !(0xFFF << 10);
        put32(image, at, insn | (((value & 0xFFF) as u32) << 10));
    }

    fn stub(image: &mut Image, base: u64, tables: &mut Tables, symbol: u64) -> u64 {
        let next = &mut tables.next_stub;
        let slot = *tables.stubs.entry(symbol).or_insert_with(|| {
            let at = *next;
            *next += STUB_SIZE;
            at
        });
        put32(image, slot, 0x5800_0050);
        put32(image, slot + 4, 0xD61F_0200);
        put64(image, slot + 8, symbol);
        base + slot as u64
    }

    pub fn apply(image: &mut Image, base: u64, tables: &mut Tables, kind: u32, at: usize, symbol: u64, addend: i64) -> Result<(), &'static str> {
        let place = base + at as u64;
        let s = symbol as i64 + addend;
        match kind {
            NONE | NONE_ALT => {}
            ABS64 => put64(image, at, s as u64),
            ABS32 => put32(image, at, s as u32),
            ABS16 => image.bytes_mut()[at..at + 2].copy_from_slice(&(s as u16).to_le_bytes()),
            PREL64 => put64(image, at, (s - place as i64) as u64),
            PREL32 => {
                let v = s - place as i64;
                if !fits_signed(v, 32) {
                    return Err("module: PREL32 relocation overflowed");
                }
                put32(image, at, v as u32);
            }
            PREL16 => image.bytes_mut()[at..at + 2].copy_from_slice(&((s - place as i64) as u16).to_le_bytes()),
            ADR_PREL_LO21 => {
                let v = s - place as i64;
                if !fits_signed(v, 21) {
                    return Err("module: ADR relocation overflowed");
                }
                set_adr(image, at, v);
            }
            ADR_PREL_PG_HI21 | ADR_PREL_PG_HI21_NC => {
                let v = (page(s as u64) - page(place)) >> 12;
                if kind == ADR_PREL_PG_HI21 && !fits_signed(v, 21) {
                    return Err("module: ADRP relocation overflowed");
                }
                set_adr(image, at, v);
            }
            ADD_ABS_LO12_NC | LDST8_ABS_LO12_NC => set_imm12(image, at, s as u64),
            LDST16_ABS_LO12_NC => set_imm12(image, at, (s as u64 & 0xFFF) >> 1),
            LDST32_ABS_LO12_NC => set_imm12(image, at, (s as u64 & 0xFFF) >> 2),
            LDST64_ABS_LO12_NC => set_imm12(image, at, (s as u64 & 0xFFF) >> 3),
            LDST128_ABS_LO12_NC => set_imm12(image, at, (s as u64 & 0xFFF) >> 4),
            JUMP26 | CALL26 => {
                let mut v = s - place as i64;
                if !fits_signed(v, 28) {
                    v = stub(image, base, tables, s as u64) as i64 - place as i64;
                }
                if !fits_signed(v, 28) {
                    return Err("module: branch target out of range");
                }
                let insn = get32(image, at) & !0x03FF_FFFF;
                put32(image, at, insn | ((v >> 2) as u32 & 0x03FF_FFFF));
            }
            CONDBR19 => {
                let v = s - place as i64;
                if !fits_signed(v, 21) {
                    return Err("module: conditional branch out of range");
                }
                let insn = get32(image, at) & !(0x7FFFF << 5);
                put32(image, at, insn | ((((v >> 2) as u32) & 0x7FFFF) << 5));
            }
            TSTBR14 => {
                let v = s - place as i64;
                if !fits_signed(v, 16) {
                    return Err("module: test branch out of range");
                }
                let insn = get32(image, at) & !(0x3FFF << 5);
                put32(image, at, insn | ((((v >> 2) as u32) & 0x3FFF) << 5));
            }
            ADR_GOT_PAGE => {
                let slot = base + tables.got_slot(image, symbol) as u64;
                let v = (page(slot) - page(place)) >> 12;
                set_adr(image, at, v);
            }
            LD64_GOT_LO12_NC => {
                let slot = base + tables.got_slot(image, symbol) as u64;
                set_imm12(image, at, (slot & 0xFFF) >> 3);
            }
            _ => return Err("module: unsupported aarch64 relocation type"),
        }
        Ok(())
    }
}

#[cfg(target_arch = "riscv64")]
mod arch {
    use super::*;

    pub const STUB_SIZE: usize = 16;
    pub const PASSES: usize = 2;

    const NONE: u32 = 0;
    const R32: u32 = 1;
    const R64: u32 = 2;
    const BRANCH: u32 = 16;
    const JAL: u32 = 17;
    const CALL: u32 = 18;
    const CALL_PLT: u32 = 19;
    const GOT_HI20: u32 = 20;
    const PCREL_HI20: u32 = 23;
    const PCREL_LO12_I: u32 = 24;
    const PCREL_LO12_S: u32 = 25;
    const HI20: u32 = 26;
    const LO12_I: u32 = 27;
    const LO12_S: u32 = 28;
    const ADD8: u32 = 33;
    const ADD16: u32 = 34;
    const ADD32: u32 = 35;
    const ADD64: u32 = 36;
    const SUB8: u32 = 37;
    const SUB16: u32 = 38;
    const SUB32: u32 = 39;
    const SUB64: u32 = 40;
    const ALIGN: u32 = 43;
    const RVC_BRANCH: u32 = 44;
    const RVC_JUMP: u32 = 45;
    const RELAX: u32 = 51;
    const SUB6: u32 = 52;
    const SET6: u32 = 53;
    const SET8: u32 = 54;
    const SET16: u32 = 55;
    const SET32: u32 = 56;
    const PCREL32: u32 = 57;

    pub fn needs_got(kind: u32) -> bool {
        kind == GOT_HI20
    }

    pub fn needs_stub(_kind: u32) -> bool {
        false
    }

    pub fn in_pass(kind: u32, pass: usize) -> bool {
        let late = matches!(kind, PCREL_LO12_I | PCREL_LO12_S);
        if pass == 0 { !late } else { late }
    }

    fn hi20(value: i64) -> u32 {
        (((value + 0x800) >> 12) as u32) & 0xFFFFF
    }

    fn set_u(image: &mut Image, at: usize, value: i64) {
        let insn = get32(image, at) & 0xFFF;
        put32(image, at, insn | (hi20(value) << 12));
    }

    fn set_i(image: &mut Image, at: usize, value: i64) {
        let insn = get32(image, at) & 0x000F_FFFF;
        put32(image, at, insn | (((value as u32) & 0xFFF) << 20));
    }

    fn set_s(image: &mut Image, at: usize, value: i64) {
        let v = value as u32 & 0xFFF;
        let insn = get32(image, at) & !((0x7F << 25) | (0x1F << 7));
        put32(image, at, insn | ((v >> 5) << 25) | ((v & 0x1F) << 7));
    }

    fn get16(image: &mut Image, at: usize) -> u16 {
        let b = image.bytes_mut();
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn put16(image: &mut Image, at: usize, value: u16) {
        image.bytes_mut()[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn apply(image: &mut Image, base: u64, tables: &mut Tables, kind: u32, at: usize, symbol: u64, addend: i64) -> Result<(), &'static str> {
        let place = base + at as u64;
        let s = symbol as i64 + addend;
        match kind {
            NONE | ALIGN | RELAX => {}
            R32 => put32(image, at, s as u32),
            R64 => put64(image, at, s as u64),
            PCREL32 => put32(image, at, (s - place as i64) as u32),
            BRANCH => {
                let v = s - place as i64;
                if !fits_signed(v, 13) {
                    return Err("module: branch out of range");
                }
                let v = v as u32;
                let insn = get32(image, at) & 0x01FF_F07F;
                let encoded = ((v >> 12) & 1) << 31 | ((v >> 5) & 0x3F) << 25 | ((v >> 1) & 0xF) << 8 | ((v >> 11) & 1) << 7;
                put32(image, at, insn | encoded);
            }
            JAL => {
                let v = s - place as i64;
                if !fits_signed(v, 21) {
                    return Err("module: jal out of range");
                }
                let v = v as u32;
                let insn = get32(image, at) & 0xFFF;
                let encoded = ((v >> 20) & 1) << 31 | ((v >> 1) & 0x3FF) << 21 | ((v >> 11) & 1) << 20 | ((v >> 12) & 0xFF) << 12;
                put32(image, at, insn | encoded);
            }
            CALL | CALL_PLT => {
                let v = s - place as i64;
                if !fits_signed(v, 32) {
                    return Err("module: call out of range");
                }
                set_u(image, at, v);
                set_i(image, at + 4, v);
            }
            PCREL_HI20 => {
                let v = s - place as i64;
                tables.hi.insert(place, v);
                set_u(image, at, v);
            }
            GOT_HI20 => {
                let slot = base + tables.got_slot(image, symbol) as u64;
                let v = slot as i64 + addend - place as i64;
                tables.hi.insert(place, v);
                set_u(image, at, v);
            }
            PCREL_LO12_I | PCREL_LO12_S => {
                let v = *tables.hi.get(&symbol).ok_or("module: PCREL_LO12 without a matching HI20")?;
                let lo = v - (((v + 0x800) >> 12) << 12);
                if kind == PCREL_LO12_I {
                    set_i(image, at, lo);
                } else {
                    set_s(image, at, lo);
                }
            }
            HI20 => {
                if !fits_signed(s, 32) {
                    return Err("module: absolute HI20 needs an address below 2 GiB, build with -C code-model=medium");
                }
                set_u(image, at, s);
            }
            LO12_I => set_i(image, at, s),
            LO12_S => set_s(image, at, s),
            ADD8 | SUB8 | SET8 => {
                let old = image.bytes_mut()[at] as i64;
                let v = match kind {
                    ADD8 => old + s,
                    SUB8 => old - s,
                    _ => s,
                };
                image.bytes_mut()[at] = v as u8;
            }
            ADD16 | SUB16 | SET16 => {
                let old = get16(image, at) as i64;
                let v = match kind {
                    ADD16 => old + s,
                    SUB16 => old - s,
                    _ => s,
                };
                put16(image, at, v as u16);
            }
            ADD32 | SUB32 | SET32 => {
                let old = get32(image, at) as i64;
                let v = match kind {
                    ADD32 => old + s,
                    SUB32 => old - s,
                    _ => s,
                };
                put32(image, at, v as u32);
            }
            ADD64 | SUB64 => {
                let b = image.bytes_mut();
                let old = i64::from_le_bytes(b[at..at + 8].try_into().unwrap());
                let v = if kind == ADD64 { old + s } else { old - s };
                put64(image, at, v as u64);
            }
            SUB6 | SET6 => {
                let old = image.bytes_mut()[at] as i64;
                let v = if kind == SUB6 { (old & 0x3F) - s } else { s };
                image.bytes_mut()[at] = (old as u8 & 0xC0) | (v as u8 & 0x3F);
            }
            RVC_BRANCH => {
                let v = s - place as i64;
                if !fits_signed(v, 9) {
                    return Err("module: compressed branch out of range");
                }
                let v = v as u16;
                let insn = get16(image, at) & 0xE383;
                let encoded = ((v >> 8) & 1) << 12 | ((v >> 3) & 3) << 10 | ((v >> 6) & 3) << 5 | ((v >> 1) & 3) << 3 | ((v >> 5) & 1) << 2;
                put16(image, at, insn | encoded);
            }
            RVC_JUMP => {
                let v = s - place as i64;
                if !fits_signed(v, 12) {
                    return Err("module: compressed jump out of range");
                }
                let v = v as u16;
                let insn = get16(image, at) & 0xE003;
                let encoded = ((v >> 11) & 1) << 12 | ((v >> 4) & 1) << 11 | ((v >> 8) & 3) << 9 | ((v >> 10) & 1) << 8 | ((v >> 6) & 1) << 7 | ((v >> 7) & 1) << 6 | ((v >> 1) & 7) << 3 | ((v >> 5) & 1) << 2;
                put16(image, at, insn | encoded);
            }
            _ => return Err("module: unsupported riscv64 relocation type"),
        }
        Ok(())
    }
}

pub use arch::{apply, in_pass, needs_got, needs_stub, PASSES, STUB_SIZE};
