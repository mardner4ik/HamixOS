use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::memory::frame;

pub const PAGE_SIZE: u64 = 4096;
pub const IDENTITY_END: u64 = 128 << 30;
pub const USER_BASE: u64 = 128 << 30;
pub const USER_END: u64 = 192 << 30;
pub const VMALLOC_BASE: u64 = 192 << 30;
pub const VMALLOC_SIZE: u64 = 32 << 30;
pub const USER_MMAP_BASE: u64 = 144 << 30;
pub const USER_SHM_BASE: u64 = 160 << 30;
pub const USER_FB_BASE: u64 = 176 << 30;
pub const USER_STACK_TOP: u64 = (192 << 30) - 0x1000;

const IDENTITY_SLOTS: usize = 128;
const USER_SLOTS: core::ops::Range<usize> = 128..192;
const VMALLOC_SLOTS: core::ops::Range<usize> = 192..224;

const VALID: u64 = 1 << 0;
const READ: u64 = 1 << 1;
const WRITABLE: u64 = 1 << 2;
const EXECUTE: u64 = 1 << 3;
const USER: u64 = 1 << 4;
const GLOBAL: u64 = 1 << 5;
const ACCESSED: u64 = 1 << 6;
const DIRTY: u64 = 1 << 7;
const OWNED: u64 = 1 << 8;
const LEAF: u64 = READ | WRITABLE | EXECUTE;
const PPN_MASK: u64 = (1 << 44) - 1;

const SATP_SV39: u64 = 8 << 60;
const SSTATUS_SUM: usize = 1 << 18;

pub const WRITE: u8 = 1;
pub const EXEC: u8 = 2;

static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);
static READY: AtomicBool = AtomicBool::new(false);

fn entry(phys: u64, flags: u64) -> u64 {
    ((phys >> 12) << 10) | flags
}

fn address(pte: u64) -> u64 {
    ((pte >> 10) & PPN_MASK) << 12
}

pub fn flush_tlb() {
    unsafe { core::arch::asm!("sfence.vma", options(nostack)) };
}

pub fn flush_page(virt: u64) {
    unsafe { core::arch::asm!("sfence.vma {}, zero", in(reg) virt, options(nostack)) };
}

fn read_satp() -> u64 {
    let satp: u64;
    unsafe { core::arch::asm!("csrr {}, satp", out(reg) satp, options(nomem, nostack)) };
    satp
}

fn load_kernel_root(root: u64) -> bool {
    unsafe {
        core::arch::asm!("sfence.vma", "csrw satp, {}", "sfence.vma", in(reg) SATP_SV39 | (root >> 12), options(nostack));
    }
    read_satp() >> 60 == 8
}

pub fn kernel_root() -> u64 {
    KERNEL_ROOT.load(Ordering::Relaxed)
}

pub fn vmalloc_ready() -> bool {
    READY.load(Ordering::Acquire)
}

pub fn init() -> Option<&'static str> {
    let root = frame::alloc_zeroed_frame()? as u64;
    unsafe {
        let top = root as *mut u64;
        for gib in 0..IDENTITY_SLOTS {
            *top.add(gib) = entry((gib as u64) << 30, VALID | LEAF | GLOBAL | ACCESSED | DIRTY);
        }
        for slot in VMALLOC_SLOTS {
            let Some(table) = frame::alloc_zeroed_frame() else {
                return None;
            };
            *top.add(slot) = entry(table as u64, VALID);
        }
        core::arch::asm!("csrs sstatus, {}", in(reg) SSTATUS_SUM, options(nostack));
    }
    if !load_kernel_root(root) {
        unsafe { core::arch::asm!("csrw satp, zero", "sfence.vma", options(nostack)) };
        return None;
    }
    KERNEL_ROOT.store(root, Ordering::Relaxed);
    READY.store(true, Ordering::Release);
    Some("sv39")
}

unsafe fn walk(root: u64, virt: u64, create: bool) -> Option<*mut u64> {
    let mut table = root as *mut u64;
    for shift in [30u64, 21] {
        unsafe {
            let slot = table.add(((virt >> shift) & 0x1FF) as usize);
            if *slot & VALID == 0 {
                if !create {
                    return None;
                }
                *slot = entry(frame::alloc_zeroed_frame()? as u64, VALID);
            }
            if *slot & LEAF != 0 {
                return None;
            }
            table = address(*slot) as *mut u64;
        }
    }
    Some(unsafe { table.add(((virt >> 12) & 0x1FF) as usize) })
}

pub fn kernel_map_page(virt: u64, phys: u64) -> bool {
    let Some(pte) = (unsafe { walk(kernel_root(), virt, true) }) else {
        return false;
    };
    unsafe { *pte = entry(phys, VALID | READ | WRITABLE | GLOBAL | ACCESSED | DIRTY) };
    true
}

pub fn kernel_unmap_page(virt: u64) -> Option<u64> {
    let pte = unsafe { walk(kernel_root(), virt, false) }?;
    let old = unsafe { *pte };
    if old & VALID == 0 {
        return None;
    }
    unsafe { *pte = 0 };
    flush_page(virt);
    Some(address(old))
}

pub fn kernel_translate(virt: u64) -> Option<u64> {
    let pte = unsafe { walk(kernel_root(), virt, false) }?;
    let value = unsafe { *pte };
    if value & VALID == 0 {
        return None;
    }
    Some(address(value) | (virt & 0xFFF))
}


pub fn read_root() -> u64 {
    (read_satp() & PPN_MASK) << 12
}

pub fn load_user_root(root: u64) {
    unsafe {
        core::arch::asm!("csrw satp, {}", "sfence.vma", in(reg) SATP_SV39 | (root >> 12), options(nostack));
    }
}

pub use crate::memory::aspace::{is_user_range, resident_pages, AddressSpace, LazyRegion, ANON_NODE};

pub fn load_root(root: u64) {
    load_user_root(root)
}

pub fn no_execute() -> bool {
    false
}

pub fn protect_kernel_range(_addr: u64, _len: u64, _writable: bool, _executable: bool) -> bool {
    false
}

pub fn write_combining() -> bool {
    false
}

pub fn enable_kernel_write_combining(_addr: u64, _len: u64) {}

pub fn map_kernel_mmio(phys: u64, len: u64) -> bool {
    phys.saturating_add(len) <= IDENTITY_END
}

const LAZY: u64 = 1 << 1;

pub mod pte {
    use super::*;

    pub use super::{USER_BASE, USER_END};

    pub const LEVELS: usize = 3;
    pub const USER_TOP: core::ops::Range<usize> = USER_SLOTS;

    #[inline]
    pub fn index(virt: u64, level: usize) -> usize {
        ((virt >> (30 - 9 * level as u64)) & 0x1FF) as usize
    }

    #[inline]
    pub fn valid(e: u64) -> bool {
        e & VALID != 0
    }

    #[inline]
    pub fn is_table(e: u64, _level: usize) -> bool {
        e & LEAF == 0
    }

    #[inline]
    pub fn table(phys: u64) -> u64 {
        entry(phys, VALID)
    }

    #[inline]
    pub fn addr(e: u64) -> u64 {
        address(e)
    }

    #[inline]
    pub fn with_addr(e: u64, phys: u64) -> u64 {
        (e & 0x3FF) | ((phys >> 12) << 10)
    }

    #[inline]
    pub fn user_page(phys: u64, owned: bool, _wc: bool) -> u64 {
        let mut e = entry(phys, VALID | LEAF | USER | ACCESSED | DIRTY);
        if owned {
            e |= OWNED;
        }
        e
    }

    #[inline]
    pub fn owned(e: u64) -> bool {
        e & OWNED != 0
    }

    #[inline]
    pub fn lazy(index: usize) -> u64 {
        LAZY | ((index as u64) << 12)
    }

    #[inline]
    pub fn is_lazy(e: u64) -> bool {
        e & VALID == 0 && e & LAZY != 0
    }

    #[inline]
    pub fn lazy_index(e: u64) -> usize {
        (e >> 12) as usize
    }

    pub fn kernel_root() -> u64 {
        super::kernel_root()
    }

    pub fn read_root() -> u64 {
        super::read_root()
    }

    pub fn load_root(root: u64) {
        super::load_user_root(root)
    }

    pub fn flush_page(virt: u64) {
        super::flush_page(virt)
    }

    pub fn flush_all() {
        super::flush_tlb()
    }

    #[inline]
    pub fn barrier() {
        super::flush_tlb()
    }

    #[inline]
    pub fn sync_code(_phys: u64, _len: u64) {
        unsafe { core::arch::asm!("fence.i", options(nostack)) };
    }
}
