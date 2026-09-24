use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::image::Image;
use super::reloc;
use super::symbols;

const ET_REL: u16 = 1;
#[cfg(target_arch = "x86_64")]
const EM_HOST: u16 = 62;
#[cfg(target_arch = "aarch64")]
const EM_HOST: u16 = 183;
#[cfg(target_arch = "riscv64")]
const EM_HOST: u16 = 243;
const ELFCLASS64: u8 = 2;

const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;

const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;

const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xFFF1;
const SHN_COMMON: u16 = 0xFFF2;

const STT_FUNC: u8 = 2;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    ident: [u8; 16],
    kind: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SectionHeader {
    name: u32,
    kind: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Sym {
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rela {
    offset: u64,
    info: u64,
    addend: i64,
}

fn read<T: Copy>(data: &[u8], offset: usize) -> Result<T, &'static str> {
    let size = core::mem::size_of::<T>();
    if offset.checked_add(size).map(|end| end > data.len()).unwrap_or(true) {
        return Err("module: truncated ELF");
    }
    Ok(unsafe { core::ptr::read_unaligned(data.as_ptr().add(offset) as *const T) })
}

fn cstr(data: &[u8], offset: usize) -> &str {
    let rest = match data.get(offset..) {
        Some(r) => r,
        None => return "",
    };
    let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    core::str::from_utf8(&rest[..end]).unwrap_or("")
}

pub struct Loaded {
    pub image: Image,
    pub exported: BTreeMap<String, u64>,
    pub functions: BTreeMap<String, u64>,
}

pub fn load(blob: &[u8]) -> Result<Loaded, &'static str> {
    let header: Header = read(blob, 0)?;
    if header.ident[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err("module: not an ELF file");
    }
    if header.ident[4] != ELFCLASS64 || header.machine != EM_HOST {
        return Err("module: not an ELF64 object for this processor");
    }
    if header.kind != ET_REL {
        return Err("module: not a relocatable object (build with --emit=obj or ld -r)");
    }
    if header.shentsize as usize != core::mem::size_of::<SectionHeader>() {
        return Err("module: unexpected section header size");
    }

    let mut sections = Vec::with_capacity(header.shnum as usize);
    for i in 0..header.shnum as usize {
        sections.push(read::<SectionHeader>(blob, header.shoff as usize + i * header.shentsize as usize)?);
    }

    let mut placement = alloc::vec![u64::MAX; sections.len()];
    let mut text_bytes = 0usize;
    let mut data_bytes = 0usize;
    let mut cursor = 0usize;
    for (i, s) in sections.iter().enumerate() {
        if s.flags & SHF_ALLOC == 0 || (s.kind != SHT_PROGBITS && s.kind != SHT_NOBITS) {
            continue;
        }
        let align = (s.align.max(1) as usize).min(4096);
        cursor = cursor.div_ceil(align) * align;
        placement[i] = cursor as u64;
        cursor += s.size as usize;
        if s.flags & SHF_EXECINSTR != 0 {
            text_bytes += s.size as usize;
        } else {
            data_bytes += s.size as usize;
        }
    }

    let symtab_index = sections.iter().position(|s| s.kind == SHT_SYMTAB).ok_or("module: no symbol table")?;
    let symtab = &sections[symtab_index];
    let strtab = *sections.get(symtab.link as usize).ok_or("module: symbol table has no string table")?;
    if strtab.kind != SHT_STRTAB {
        return Err("module: symbol table link is not a string table");
    }
    let strings = blob.get(strtab.offset as usize..(strtab.offset + strtab.size) as usize).ok_or("module: truncated string table")?;

    let count = (symtab.size / core::mem::size_of::<Sym>() as u64) as usize;
    let mut syms = Vec::with_capacity(count);
    for i in 0..count {
        syms.push(read::<Sym>(blob, symtab.offset as usize + i * core::mem::size_of::<Sym>())?);
    }

    let mut commons = cursor;
    for sym in syms.iter() {
        if sym.shndx == SHN_COMMON {
            let align = (sym.value.max(1) as usize).min(4096);
            commons = commons.div_ceil(align) * align;
            commons += sym.size as usize;
        }
    }
    let (got_slots, stub_slots) = count_slots(blob, &sections, &syms);
    let got_at = commons.div_ceil(8) * 8;
    let stub_at = (got_at + got_slots * 8).div_ceil(16) * 16;
    let total = stub_at + stub_slots * reloc::STUB_SIZE;
    if total == 0 {
        return Err("module: object has nothing to load");
    }

    let mut image = Image::new(total).ok_or("module: out of memory")?;
    let base = image.base();

    for (i, s) in sections.iter().enumerate() {
        if placement[i] == u64::MAX {
            continue;
        }
        let at = placement[i] as usize;
        if s.kind == SHT_NOBITS {
            image.bytes_mut()[at..at + s.size as usize].fill(0);
            continue;
        }
        let src = blob.get(s.offset as usize..(s.offset + s.size) as usize).ok_or("module: truncated section")?;
        image.bytes_mut()[at..at + src.len()].copy_from_slice(src);
    }

    let mut common_at = cursor;
    let mut resolved: Vec<u64> = Vec::with_capacity(syms.len());
    let mut missing: Option<String> = None;
    for sym in syms.iter() {
        let name = cstr(strings, sym.name as usize);
        let value = match sym.shndx {
            SHN_UNDEF => {
                if name.is_empty() {
                    0
                } else {
                    match symbols::lookup(name) {
                        Some(addr) => addr,
                        None => {
                            if sym.info >> 4 == STB_WEAK {
                                0
                            } else {
                                if missing.is_none() {
                                    missing = Some(name.to_string());
                                }
                                0
                            }
                        }
                    }
                }
            }
            SHN_ABS => sym.value,
            SHN_COMMON => {
                let align = (sym.value.max(1) as usize).min(4096);
                common_at = common_at.div_ceil(align) * align;
                let at = common_at;
                common_at += sym.size as usize;
                image.bytes_mut()[at..at + sym.size as usize].fill(0);
                base + at as u64
            }
            other => {
                let index = other as usize;
                if index >= placement.len() || placement[index] == u64::MAX {
                    0
                } else {
                    base + placement[index] + sym.value
                }
            }
        };
        resolved.push(value);
    }
    if let Some(name) = missing {
        return Err(leak_missing(name));
    }

    let mut tables = reloc::Tables { got: BTreeMap::new(), next_got: got_at, stubs: BTreeMap::new(), next_stub: stub_at, hi: BTreeMap::new() };

    for s in sections.iter() {
        if s.kind != SHT_RELA {
            continue;
        }
        let target = s.info as usize;
        if target >= placement.len() || placement[target] == u64::MAX {
            continue;
        }
        let target_at = placement[target] as usize;
        let target_size = sections[target].size as usize;
        let entries = (s.size / core::mem::size_of::<Rela>() as u64) as usize;
        let mut relas = Vec::with_capacity(entries);
        for i in 0..entries {
            relas.push(read::<Rela>(blob, s.offset as usize + i * core::mem::size_of::<Rela>())?);
        }
        for pass in 0..reloc::PASSES {
            for rela in relas.iter() {
                let kind = (rela.info & 0xFFFF_FFFF) as u32;
                let sym_index = (rela.info >> 32) as usize;
                if !reloc::in_pass(kind, pass) {
                    continue;
                }
                let symbol = *resolved.get(sym_index).ok_or("module: relocation names an unknown symbol")?;
                if rela.offset as usize >= target_size {
                    return Err("module: relocation points outside its section");
                }
                let at = target_at + rela.offset as usize;
                reloc::apply(&mut image, base, &mut tables, kind, at, symbol, rela.addend)?;
            }
        }
    }
    crate::arch::pte::sync_code(base, image.len() as u64);

    let mut exported = BTreeMap::new();
    let mut functions = BTreeMap::new();
    for (i, sym) in syms.iter().enumerate() {
        let bind = sym.info >> 4;
        if sym.shndx == SHN_UNDEF || (bind != STB_GLOBAL && bind != STB_WEAK) {
            continue;
        }
        let name = cstr(strings, sym.name as usize);
        if name.is_empty() {
            continue;
        }
        exported.insert(name.to_string(), resolved[i]);
        if sym.info & 0xF == STT_FUNC {
            functions.insert(name.to_string(), resolved[i]);
        }
    }

    image.set_split(text_bytes, data_bytes);
    Ok(Loaded { image, exported, functions })
}

fn count_slots(blob: &[u8], sections: &[SectionHeader], syms: &[Sym]) -> (usize, usize) {
    let mut got: Vec<usize> = Vec::new();
    let mut stubs: Vec<usize> = Vec::new();
    for s in sections.iter().filter(|s| s.kind == SHT_RELA) {
        let entries = (s.size / core::mem::size_of::<Rela>() as u64) as usize;
        for i in 0..entries {
            let Ok(rela) = read::<Rela>(blob, s.offset as usize + i * core::mem::size_of::<Rela>()) else {
                continue;
            };
            let kind = (rela.info & 0xFFFF_FFFF) as u32;
            let index = (rela.info >> 32) as usize;
            if index >= syms.len() {
                continue;
            }
            if reloc::needs_got(kind) && !got.contains(&index) {
                got.push(index);
            }
            if reloc::needs_stub(kind) && !stubs.contains(&index) {
                stubs.push(index);
            }
        }
    }
    (got.len(), stubs.len())
}

fn leak_missing(name: String) -> &'static str {
    super::record_missing(name);
    "module: the kernel does not export a symbol this module needs"
}
