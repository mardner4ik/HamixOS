use alloc::vec::Vec;

use crate::arch::x86_64::paging;
use crate::fs;
use crate::task::usermode;

const PT_LOAD: u32 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3E;
const ELFCLASS64: u8 = 2;

const USER_STACK_SIZE: usize = 64 * 1024;
static mut USER_STACK: [u8; USER_STACK_SIZE] = [0u8; USER_STACK_SIZE];

unsafe extern "C" {
    static __kernel_end: u8;
}

/// Everything from address 0 up to the kernel's own image end (text, data,
/// bss, and the static kernel heap in memory::heap) is off-limits to
/// user binaries in this single-address-space bridge stage. A binary
/// linked to load anywhere in that range would silently overwrite live
/// kernel state instead of failing loudly, so we check for it explicitly.
fn kernel_reserved_end() -> u64 {
    (&raw const __kernel_end) as u64
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

fn read_struct<T: Copy>(data: &[u8], offset: usize) -> Result<T, &'static str> {
    if offset + core::mem::size_of::<T>() > data.len() {
        return Err("elf: field out of bounds");
    }
    Ok(unsafe { core::ptr::read_unaligned(data.as_ptr().add(offset) as *const T) })
}

fn validate(header: &Elf64Header) -> Result<(), &'static str> {
    if header.e_ident[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err("elf: bad magic");
    }
    if header.e_ident[4] != ELFCLASS64 {
        return Err("elf: not a 64-bit ELF (build with -m64)");
    }
    if header.e_machine != EM_X86_64 {
        return Err("elf: not x86_64");
    }
    if header.e_type != ET_EXEC {
        return Err("elf: only static, non-PIE ET_EXEC binaries are supported (musl-gcc -static -no-pie)");
    }
    Ok(())
}

/// Loads a static ET_EXEC x86_64 binary from the VFS and jumps to it in
/// ring 3. Does not return on success -- see usermode::enter_user_mode.
///
/// HamixOS does not yet have per-process page tables (see
/// docs/USERSPACE_ROADMAP.md), so every PT_LOAD segment is copied straight
/// into the kernel's single identity-mapped address space at its literal
/// p_vaddr, and paging::allow_user_access() punches a coarse, 2MB-page-
/// granular hole in the supervisor-only mapping to make it reachable from
/// ring 3. A binary whose p_vaddr overlaps kernel code/data will corrupt
/// the kernel; this loader trusts the binary the same way early Linux
/// trusted init before real isolation existed. It is a bridge to real
/// per-process address spaces, not a replacement for them.
pub fn load_and_exec(path: &str) -> Result<(), &'static str> {
    let data: Vec<u8> = {
        let vfs_guard = fs::VFS.lock();
        let vfs = vfs_guard.as_ref().ok_or("elf: no filesystem mounted")?;
        vfs.read(vfs.root_id(), path).map_err(|_| "elf: file not found")?
    };

    let header: Elf64Header = read_struct(&data, 0)?;
    validate(&header)?;

    let phoff = header.e_phoff as usize;
    let phentsize = header.e_phentsize as usize;
    let phnum = header.e_phnum as usize;

    if phentsize < core::mem::size_of::<Elf64ProgramHeader>() {
        return Err("elf: program header entry too small");
    }

    let mut loaded_any = false;
    let reserved_end = kernel_reserved_end();

    for i in 0..phnum {
        let ph: Elf64ProgramHeader = read_struct(&data, phoff + i * phentsize)?;
        if ph.p_type != PT_LOAD {
            continue;
        }

        let seg_end = ph
            .p_vaddr
            .checked_add(ph.p_memsz)
            .ok_or("elf: segment address overflow")?;
        if ph.p_vaddr < reserved_end {
            return Err(
                "elf: segment overlaps the kernel image/heap -- relink the binary above \
                 the kernel (e.g. musl-gcc/gcc ... -Wl,-Ttext-segment=0x2000000)",
            );
        }
        let _ = seg_end;

        let file_start = ph.p_offset as usize;
        let file_len = ph.p_filesz as usize;
        let file_end = file_start.checked_add(file_len).ok_or("elf: segment size overflow")?;
        if file_end > data.len() {
            return Err("elf: segment reaches past end of file");
        }
        if ph.p_memsz < ph.p_filesz {
            return Err("elf: p_memsz smaller than p_filesz");
        }

        let dest = ph.p_vaddr as usize as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr().add(file_start), dest, file_len);
            let bss_len = (ph.p_memsz - ph.p_filesz) as usize;
            if bss_len > 0 {
                core::ptr::write_bytes(dest.add(file_len), 0, bss_len);
            }
        }

        paging::allow_user_access(ph.p_vaddr, ph.p_memsz.max(1));
        loaded_any = true;
    }

    if !loaded_any {
        return Err("elf: no PT_LOAD segments found");
    }

    let stack_top = unsafe { (&raw const USER_STACK) as u64 + USER_STACK_SIZE as u64 };
    paging::allow_user_access(stack_top - USER_STACK_SIZE as u64, USER_STACK_SIZE as u64);
    let aligned_stack_top = stack_top & !0xF;

    crate::serial_println!(
        "elf: loaded {}, entry {:#x}, stack {:#x}",
        path,
        header.e_entry,
        aligned_stack_top
    );

    unsafe { usermode::enter_user_mode(header.e_entry, aligned_stack_top) }
}
