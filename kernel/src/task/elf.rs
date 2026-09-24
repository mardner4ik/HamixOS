use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{linux_loader, Abi, Pid, UserImage};
use crate::arch::paging::{self, AddressSpace, PAGE_SIZE};
use crate::fs;

const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_PHDR: u32 = 6;
pub(super) const ET_EXEC: u16 = 2;
pub(super) const ET_DYN: u16 = 3;
#[cfg(target_arch = "x86_64")]
const EM_HOST: u16 = 0x3E;
#[cfg(target_arch = "aarch64")]
const EM_HOST: u16 = 183;
#[cfg(target_arch = "riscv64")]
const EM_HOST: u16 = 243;

pub const SIGRETURN_TRAMPOLINE: u64 = paging::USER_STACK_TOP;

#[cfg(target_arch = "aarch64")]
const TRAMPOLINE_CODE: [u32; 2] = [0xd280_1168, 0xd400_0001];
#[cfg(target_arch = "riscv64")]
const TRAMPOLINE_CODE: [u32; 2] = [0x08b0_0893, 0x0000_0073];

#[cfg(not(target_arch = "x86_64"))]
fn map_trampoline(aspace: &mut AddressSpace) -> bool {
    if !aspace.alloc_range(SIGRETURN_TRAMPOLINE, PAGE_SIZE) {
        return false;
    }
    let code: Vec<u8> = TRAMPOLINE_CODE.iter().flat_map(|w| w.to_le_bytes()).collect();
    aspace.write_bytes(SIGRETURN_TRAMPOLINE, &code)
}

#[cfg(target_arch = "x86_64")]
fn map_trampoline(_aspace: &mut AddressSpace) -> bool {
    true
}

#[cfg(target_arch = "x86_64")]
fn hwcap() -> (u64, u64) {
    (0, 0)
}

#[cfg(target_arch = "aarch64")]
fn hwcap() -> (u64, u64) {
    let isar0: u64;
    unsafe { core::arch::asm!("mrs {}, id_aa64isar0_el1", out(reg) isar0, options(nomem, nostack)) };
    let field = |shift: u32| (isar0 >> shift) & 0xF;
    let mut caps = (1 << 0) | (1 << 1) | (1 << 11);
    if field(4) >= 1 {
        caps |= 1 << 3;
    }
    if field(4) >= 2 {
        caps |= 1 << 4;
    }
    if field(8) >= 1 {
        caps |= 1 << 5;
    }
    if field(12) >= 1 {
        caps |= 1 << 6;
    }
    if field(16) >= 1 {
        caps |= 1 << 7;
    }
    if field(20) >= 2 {
        caps |= 1 << 8;
    }
    (caps, 0)
}

#[cfg(target_arch = "riscv64")]
fn hwcap() -> (u64, u64) {
    let letters = b"imafdc";
    (letters.iter().fold(0u64, |acc, l| acc | 1 << (l - b'a')), 0)
}
const ELFCLASS64: u8 = 2;
const EI_OSABI: usize = 7;
const ELFOSABI_SYSV: u8 = 0;
const ELFOSABI_GNU: u8 = 3;
const USER_STACK_SIZE: u64 = 8 * 1024 * 1024;
const USER_STACK_EAGER: u64 = 256 * 1024;
const MAX_STRINGS: usize = 64 * 1024;

const HAMIX_NOTE_NAME: &[u8] = b"Hamix\0";
const HAMIX_NOTE_TYPE: u32 = 0x4858_4F53;

const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_PLATFORM: u64 = 15;
const AT_HWCAP: u64 = 16;
const AT_CLKTCK: u64 = 17;
const AT_SECURE: u64 = 23;
const AT_RANDOM: u64 = 25;
const AT_HWCAP2: u64 = 26;
const AT_EXECFN: u64 = 31;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Elf64Header {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Elf64ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

fn read_struct<T: Copy>(data: &[u8], offset: usize) -> Result<T, &'static str> {
    if offset.checked_add(core::mem::size_of::<T>()).map(|end| end > data.len()).unwrap_or(true) {
        return Err("elf: field out of bounds");
    }
    Ok(unsafe { core::ptr::read_unaligned(data.as_ptr().add(offset) as *const T) })
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn validate(header: &Elf64Header) -> Result<(), &'static str> {
    if header.e_ident[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err("not an ELF executable");
    }
    if header.e_ident[4] != ELFCLASS64 || header.e_machine != EM_HOST {
        return Err("not an ELF64 binary for this processor");
    }
    if header.e_ident[EI_OSABI] != ELFOSABI_SYSV && header.e_ident[EI_OSABI] != ELFOSABI_GNU {
        return Err("unsupported ELF OS ABI");
    }
    if header.e_type != ET_EXEC && header.e_type != ET_DYN {
        return Err("only ET_EXEC and ET_DYN binaries are supported");
    }
    if header.e_phentsize as usize != core::mem::size_of::<Elf64ProgramHeader>() {
        return Err("elf: unexpected program header size");
    }
    Ok(())
}

pub(super) struct Elf<'a> {
    pub data: &'a [u8],
    pub header: Elf64Header,
    pub phdrs: Vec<Elf64ProgramHeader>,
    pub lazy: Option<usize>,
}

impl<'a> Elf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Elf<'a>, &'static str> {
        let header: Elf64Header = read_struct(data, 0)?;
        validate(&header)?;
        let mut phdrs = Vec::with_capacity(header.e_phnum as usize);
        for i in 0..header.e_phnum as usize {
            phdrs.push(read_struct(data, header.e_phoff as usize + i * header.e_phentsize as usize)?);
        }
        Ok(Elf { data, header, phdrs, lazy: None })
    }

    fn segment_bytes(&self, ph: &Elf64ProgramHeader) -> Result<&'a [u8], &'static str> {
        let start = ph.p_offset as usize;
        let end = start.checked_add(ph.p_filesz as usize).ok_or("elf: bad segment")?;
        self.data.get(start..end).ok_or("elf: segment reaches past end of file")
    }

    pub fn interp(&self) -> Result<Option<String>, &'static str> {
        let Some(ph) = self.phdrs.iter().find(|p| p.p_type == PT_INTERP) else {
            return Ok(None);
        };
        let bytes = self.segment_bytes(ph)?;
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        if end == 0 || end > 4096 {
            return Err("elf: bad PT_INTERP");
        }
        Ok(Some(String::from_utf8_lossy(&bytes[..end]).into_owned()))
    }

    pub fn has_hamix_note(&self) -> bool {
        self.phdrs.iter().filter(|p| p.p_type == PT_NOTE).any(|ph| {
            let Ok(bytes) = self.segment_bytes(ph) else {
                return false;
            };
            let mut off = 0usize;
            while off + 12 <= bytes.len() {
                let (Some(namesz), Some(descsz), Some(kind)) = (read_u32(bytes, off), read_u32(bytes, off + 4), read_u32(bytes, off + 8)) else {
                    return false;
                };
                let name_start = off + 12;
                let name_end = name_start + namesz as usize;
                if bytes.get(name_start..name_end) == Some(HAMIX_NOTE_NAME) && kind == HAMIX_NOTE_TYPE {
                    return true;
                }
                off = name_start + (namesz as usize).next_multiple_of(4) + (descsz as usize).next_multiple_of(4);
            }
            false
        })
    }

    pub fn lazy_capable(&self) -> bool {
        let within = |p: &Elf64ProgramHeader| p.p_offset.saturating_add(p.p_filesz) <= self.data.len() as u64;
        if !self.phdrs.iter().filter(|p| p.p_type == PT_INTERP || p.p_type == PT_NOTE).all(within) {
            return false;
        }
        let mut pages: Vec<(u64, u64)> = self
            .phdrs
            .iter()
            .filter(|p| p.p_type == PT_LOAD && p.p_memsz != 0)
            .map(|p| (p.p_vaddr & !(PAGE_SIZE - 1), (p.p_vaddr + p.p_memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)))
            .collect();
        pages.sort();
        let aligned = self.phdrs.iter().filter(|p| p.p_type == PT_LOAD).all(|p| p.p_offset % PAGE_SIZE == p.p_vaddr % PAGE_SIZE);
        aligned && pages.windows(2).all(|w| w[0].1 <= w[1].0)
    }

    pub fn span(&self) -> Option<(u64, u64)> {
        let loads = self.phdrs.iter().filter(|p| p.p_type == PT_LOAD && p.p_memsz != 0);
        let lo = loads.clone().map(|p| p.p_vaddr & !(PAGE_SIZE - 1)).min()?;
        let hi = loads.map(|p| p.p_vaddr.saturating_add(p.p_memsz)).max()?;
        Some((lo, hi))
    }

    pub fn classify(&self) -> Result<Abi, &'static str> {
        if self.has_hamix_note() {
            if self.header.e_type != ET_EXEC {
                return Err("elf: HamixOS binaries must be ET_EXEC");
            }
            return Ok(Abi::Native);
        }
        if self.header.e_type == ET_DYN || self.interp()?.is_some() {
            return Ok(Abi::Linux);
        }
        match self.span() {
            Some((lo, _)) if lo >= paging::USER_BASE => Ok(Abi::Native),
            Some(_) => Ok(Abi::Linux),
            None => Err("elf: no PT_LOAD segments"),
        }
    }
}

pub(super) struct Loaded {
    pub bias: u64,
    pub entry: u64,
    pub phdr: Option<u64>,
    pub phnum: u64,
    pub end: u64,
}

pub(super) fn load(aspace: &mut AddressSpace, elf: &Elf, bias: u64, limit: u64) -> Result<Loaded, &'static str> {
    let mut end = 0u64;
    let mut phdr = None;
    let mut loaded = false;
    let phdr_size = elf.header.e_phnum as u64 * elf.header.e_phentsize as u64;
    for ph in elf.phdrs.iter() {
        if ph.p_type == PT_PHDR {
            phdr = Some(bias.wrapping_add(ph.p_vaddr));
        }
        if ph.p_type != PT_LOAD || ph.p_memsz == 0 {
            continue;
        }
        if ph.p_filesz > ph.p_memsz {
            return Err("elf: p_filesz larger than p_memsz");
        }
        let start = bias.checked_add(ph.p_vaddr).ok_or("elf: bad segment address")?;
        let seg_end = start.checked_add(ph.p_memsz).ok_or("elf: bad segment address")?;
        if !paging::is_user_range(start, ph.p_memsz) || seg_end > limit {
            return Err("elf: segment outside the user image area");
        }
        if let Some(node) = elf.lazy {
            let page = start & !(PAGE_SIZE - 1);
            let region = paging::LazyRegion { node, start: page, offset: ph.p_offset - (start - page), file_len: ph.p_offset + ph.p_filesz };
            if !aspace.map_lazy(page, seg_end - page, region) {
                return Err("out of memory");
            }
            if phdr.is_none() && elf.header.e_phoff >= ph.p_offset && elf.header.e_phoff + phdr_size <= ph.p_offset + ph.p_filesz {
                phdr = Some(start + (elf.header.e_phoff - ph.p_offset));
            }
            end = end.max(seg_end);
            loaded = true;
            continue;
        }
        let bytes = elf.segment_bytes(ph)?;
        if !aspace.alloc_range(start, ph.p_memsz) {
            return Err("out of memory");
        }
        let page_off = start & (PAGE_SIZE - 1);
        let prefix = if ph.p_offset >= page_off { (start - page_off).max(end.min(start)) } else { start };
        let prefix_len = (start - prefix) as usize;
        if prefix_len > 0 {
            let from = ph.p_offset as usize - prefix_len;
            if !aspace.write_bytes(prefix, &elf.data[from..ph.p_offset as usize]) {
                return Err("elf: failed to copy segment");
            }
        }
        if !aspace.write_bytes(start, bytes) {
            return Err("elf: failed to copy segment");
        }
        if phdr.is_none() && elf.header.e_phoff >= ph.p_offset && elf.header.e_phoff + phdr_size <= ph.p_offset + ph.p_filesz {
            phdr = Some(start + (elf.header.e_phoff - ph.p_offset));
        }
        end = end.max(seg_end);
        loaded = true;
    }
    if !loaded {
        return Err("elf: no PT_LOAD segments");
    }
    let entry = bias.wrapping_add(elf.header.e_entry);
    let inside = elf.phdrs.iter().any(|p| p.p_type == PT_LOAD && entry >= bias + p.p_vaddr && entry < bias + p.p_vaddr + p.p_memsz);
    if !inside {
        return Err("elf: entry point outside the image");
    }
    Ok(Loaded { bias, entry, phdr, phnum: elf.header.e_phnum as u64, end })
}

pub(super) struct StackSpec<'a> {
    pub args: &'a [String],
    pub envp: &'a [String],
    pub execfn: &'a str,
    pub uid: u32,
    pub ruid: u32,
    pub secure: bool,
}

fn build_stack(aspace: &mut AddressSpace, spec: &StackSpec, main: &Loaded, elf: &Elf, interp_base: u64) -> Result<u64, &'static str> {
    let stack_base = paging::USER_STACK_TOP - USER_STACK_SIZE;
    let eager_base = paging::USER_STACK_TOP - USER_STACK_EAGER;
    if !aspace.map_zero(stack_base, USER_STACK_SIZE - USER_STACK_EAGER) || !aspace.alloc_range(eager_base, USER_STACK_EAGER) {
        return Err("out of memory");
    }

    let strings_size: usize = spec.args.iter().chain(spec.envp.iter()).map(|s| s.len() + 1).sum::<usize>() + spec.execfn.len() + 1;
    if strings_size > MAX_STRINGS {
        return Err("argument list too long");
    }

    let mut blob: Vec<u8> = Vec::with_capacity(strings_size + 128);
    let mut offsets = Vec::with_capacity(spec.args.len() + spec.envp.len());
    for s in spec.args.iter().chain(spec.envp.iter()) {
        offsets.push(blob.len());
        blob.extend_from_slice(s.as_bytes());
        blob.push(0);
    }
    let execfn_off = blob.len();
    blob.extend_from_slice(spec.execfn.as_bytes());
    blob.push(0);
    let platform_off = blob.len();
    blob.extend_from_slice(crate::arch::MACHINE.as_bytes());
    blob.push(0);
    let random_off = blob.len();
    let mut random = [0u8; 16];
    crate::random::fill(&mut random);
    blob.extend_from_slice(&random);
    let phdr_off = if main.phdr.is_none() {
        let start = elf.header.e_phoff as usize;
        let len = elf.header.e_phnum as usize * elf.header.e_phentsize as usize;
        let off = blob.len().next_multiple_of(8);
        blob.resize(off, 0);
        blob.extend_from_slice(&elf.data[start..start + len]);
        Some(off)
    } else {
        None
    };

    let blob_start = (paging::USER_STACK_TOP - 16 - blob.len() as u64) & !0xF;
    if !aspace.write_bytes(blob_start, &blob) {
        return Err("elf: failed to write stack");
    }
    let at = |off: usize| blob_start + off as u64;
    let phdr = main.phdr.unwrap_or_else(|| at(phdr_off.unwrap_or(0)));

    let auxv: [(u64, u64); 18] = [
        (AT_PHDR, phdr),
        (AT_PHENT, core::mem::size_of::<Elf64ProgramHeader>() as u64),
        (AT_PHNUM, main.phnum),
        (AT_PAGESZ, PAGE_SIZE),
        (AT_BASE, interp_base),
        (AT_FLAGS, 0),
        (AT_ENTRY, main.entry),
        (AT_UID, spec.ruid as u64),
        (AT_EUID, spec.uid as u64),
        (AT_GID, spec.ruid as u64),
        (AT_EGID, spec.uid as u64),
        (AT_SECURE, spec.secure as u64),
        (AT_RANDOM, at(random_off)),
        (AT_PLATFORM, at(platform_off)),
        (AT_HWCAP, hwcap().0),
        (AT_HWCAP2, hwcap().1),
        (AT_CLKTCK, 100),
        (AT_EXECFN, at(execfn_off)),
    ];

    let mut words: Vec<u64> = Vec::with_capacity(4 + offsets.len() + 2 * (auxv.len() + 1));
    words.push(spec.args.len() as u64);
    words.extend(offsets[..spec.args.len()].iter().map(|o| at(*o)));
    words.push(0);
    words.extend(offsets[spec.args.len()..].iter().map(|o| at(*o)));
    words.push(0);
    for (key, value) in auxv {
        words.push(key);
        words.push(value);
    }
    words.push(AT_NULL);
    words.push(0);

    let sp = (blob_start - words.len() as u64 * 8) & !0xF;
    if sp < paging::USER_STACK_TOP - USER_STACK_EAGER + PAGE_SIZE || sp < stack_base + PAGE_SIZE {
        return Err("argument list too long");
    }
    let raw: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    if !aspace.write_bytes(sp, &raw) {
        return Err("elf: failed to write stack");
    }
    Ok(sp)
}

pub(super) fn assemble(main: &Elf, main_bias: u64, interp: Option<(&Elf, u64)>, abi: Abi, spec: &StackSpec) -> Result<UserImage, &'static str> {
    let mut aspace = AddressSpace::new().ok_or("out of memory")?;
    let result = (|| {
        let main_limit = match interp {
            Some((_, base)) => base,
            None => paging::USER_MMAP_BASE,
        };
        let loaded = load(&mut aspace, main, main_bias, main_limit)?;
        let (entry, interp_base) = match interp {
            Some((ielf, base)) => {
                let li = load(&mut aspace, ielf, base, paging::USER_MMAP_BASE)?;
                (li.entry, li.bias)
            }
            None => (loaded.entry, 0),
        };
        let sp = build_stack(&mut aspace, spec, &loaded, main, interp_base)?;
        if !map_trampoline(&mut aspace) {
            return Err("out of memory");
        }
        Ok((entry, sp, loaded.end))
    })();
    match result {
        Ok((entry, stack, end)) => Ok(UserImage {
            aspace,
            entry,
            stack,
            brk: (end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1),
            abi,
            exe: String::from(spec.execfn),
            env: spec.envp.to_vec(),
            args: spec.args.to_vec(),
        }),
        Err(e) => {
            aspace.destroy();
            Err(e)
        }
    }
}

fn build_image(data: &[u8], lazy: Option<usize>, spec: &StackSpec) -> Result<UserImage, &'static str> {
    let mut elf = Elf::parse(data)?;
    elf.lazy = lazy;
    match elf.classify()? {
        Abi::Linux => linux_loader::build(&elf, spec),
        Abi::Native => assemble(&elf, 0, None, Abi::Native, spec),
    }
}

const MIN_EXEC_HEADROOM: usize = 64 << 20;
const LAZY_EXEC_MIN: usize = 8 << 20;
const LAZY_EXEC_HEAD: usize = 256 << 10;

fn read_whole(vfs: &mut fs::Vfs, real: &str, size: usize) -> Result<Vec<u8>, &'static str> {
    let limit = exec_limit();
    if size > limit {
        crate::drivers::klog::log(&alloc::format!("exec: {} is {} MiB, and only {} MiB can be spared to load it whole", real, size >> 20, limit >> 20));
        return Err("not enough memory to load this executable");
    }
    vfs.read(vfs.root_id(), real).map_err(|_| "no such file")
}

fn exec_limit() -> usize {
    let (free, _) = crate::memory::frame::memory_info();
    let (heap_free, _) = crate::memory::heap_stats();
    (free / 4 + heap_free).max(MIN_EXEC_HEADROOM)
}

pub const DEFAULT_PATH: &str = "/usr/bin:/bin:/sbin:/usr/sbin:/opt/linux/bin:/opt/linux/usr/bin";

pub fn default_env(uid: u32) -> Vec<String> {
    let user = crate::users::name_of(uid).and_then(|n| crate::users::find_by_name(&n));
    let (name, home, shell) = match user {
        Some(u) => (u.name, u.home, u.shell),
        None if uid == 0 => (String::from("root"), String::from("/root"), String::from("/usr/bin/hsh")),
        None => (format!("{}", uid), String::from("/"), String::from("/usr/bin/hsh")),
    };
    alloc::vec![
        format!("PATH={}", DEFAULT_PATH),
        format!("HOME={}", home),
        format!("USER={}", name),
        format!("LOGNAME={}", name),
        format!("SHELL={}", shell),
        String::from("TERM=xterm"),
        String::from("LANG=C.UTF-8"),
        format!("XDG_RUNTIME_DIR=/tmp/runtime-{}", uid),
        format!("DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/runtime-{}/bus", uid),
        String::from("WAYLAND_DISPLAY=wayland-0"),
        String::from("DISPLAY=:0"),
        String::from("QT_QPA_PLATFORM=wayland;xcb"),
        String::from("NO_AT_BRIDGE=1"),
        String::from("FLTK_SCHEME=gtk+"),
        String::from("XDG_SESSION_TYPE=wayland"),
        String::from("XDG_CURRENT_DESKTOP=Nook"),
        String::from("XDG_SESSION_DESKTOP=nook"),
        String::from("XDG_MENU_PREFIX=nook-"),
        String::from("DESKTOP_SESSION=nook"),
    ]
}

pub fn load_program(path: &str, argv: &[String], envp: &[String], uid: u32, ruid: u32, depth: usize) -> Result<(UserImage, String), &'static str> {
    if depth > 4 {
        return Err("too many levels of interpreters");
    }
    let mut lazy: Option<usize> = None;
    let data: Vec<u8> = {
        let mut guard = fs::VFS.lock();
        let vfs = guard.as_mut().ok_or("no filesystem mounted")?;
        let real = linux_loader::resolve_links(vfs, path, true)?;
        let id = vfs.resolve(vfs.root_id(), &real).ok_or("no such file")?;
        if vfs.is_dir(id) {
            return Err("is a directory");
        }
        let stat = vfs.stat(id);
        if uid != 0 && stat.mode & 0o111 == 0 {
            return Err("permission denied");
        }
        let size = vfs.node_size(id);
        if size > LAZY_EXEC_MIN {
            let mut head = alloc::vec![0u8; LAZY_EXEC_HEAD.min(size)];
            let n = vfs.read_node_at(id, 0, &mut head).map_err(|_| "no such file")?;
            head.truncate(n);
            let lazy_ok = !head.starts_with(b"#!") && Elf::parse(&head).map(|e| e.lazy_capable() && e.classify().map(|a| a == Abi::Linux).unwrap_or(false)).unwrap_or(false);
            if lazy_ok {
                lazy = Some(id);
                head
            } else {
                read_whole(vfs, &real, size)?
            }
        } else {
            read_whole(vfs, &real, size)?
        }
    };
    if data.len() >= 2 && &data[0..2] == b"#!" {
        let line_end = data.iter().position(|b| *b == b'\n').unwrap_or(data.len()).min(256);
        let line = String::from_utf8_lossy(&data[2..line_end]).into_owned();
        let line = line.trim();
        let (interpreter, optional) = match line.split_once(|c: char| c == ' ' || c == '\t') {
            Some((i, rest)) => (String::from(i), Some(String::from(rest.trim()))),
            None => (String::from(line), None),
        };
        if interpreter.is_empty() {
            return Err("bad interpreter line");
        }
        if interpreter == path {
            return Err("recursive interpreter");
        }
        let resolved = if interpreter.starts_with('/') { linux_loader::resolve(&interpreter) } else { interpreter.clone() };
        let mut new_argv = alloc::vec![interpreter.clone()];
        if let Some(opt) = optional.filter(|o| !o.is_empty()) {
            new_argv.push(opt);
        }
        new_argv.push(String::from(path));
        new_argv.extend(argv.iter().skip(1).cloned());
        return load_program(&resolved, &new_argv, envp, uid, ruid, depth + 1);
    }
    let spec = StackSpec { args: argv, envp, execfn: path, uid, ruid, secure: false };
    let image = build_image(&data, lazy, &spec)?;
    let name = String::from(path.rsplit('/').next().unwrap_or(path));
    Ok((image, name))
}

#[allow(clippy::too_many_arguments)]
pub fn spawn(path: &str, args: &[String], envp: &[String], parent: Pid, vt: usize, uid: u32, ruid: u32, cwd: &str) -> Result<Pid, &'static str> {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(String::from(path));
    argv.extend_from_slice(args);
    let (image, _) = load_program(path, &argv, envp, uid, ruid, 0)?;
    let name = path.rsplit('/').next().unwrap_or(path);
    super::spawn_user(name, image, parent, vt, uid, ruid, cwd).ok_or("out of memory")
}
