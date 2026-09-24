use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::fdt::Fdt;
use crate::memory::frame;

pub const PAGE_SIZE: u64 = 4096;
pub const IDENTITY_END: u64 = 1 << 39;
pub const USER_BASE: u64 = 1 << 39;
pub const USER_END: u64 = 2 << 39;
pub const VMALLOC_BASE: u64 = 2 << 39;
pub const USER_MMAP_BASE: u64 = 0x0000_00A0_0000_0000;
pub const USER_SHM_BASE: u64 = 0x0000_00C0_0000_0000;
pub const USER_FB_BASE: u64 = 0x0000_00E0_0000_0000;
pub const USER_STACK_TOP: u64 = 0x0000_00FF_FFFF_F000;
pub const VMALLOC_SIZE: u64 = 1 << 39;

const USER_INDEX: usize = 1;
const VMALLOC_INDEX: usize = 2;

const VALID: u64 = 1 << 0;
const TABLE: u64 = 1 << 1;
const PAGE: u64 = VALID | TABLE;
const BLOCK: u64 = VALID;
const ATTR_DEVICE: u64 = 0 << 2;
const ATTR_NORMAL: u64 = 1 << 2;
const AP_EL0: u64 = 1 << 6;
const AP_READ_ONLY: u64 = 1 << 7;
const SH_INNER: u64 = 3 << 8;
const ACCESSED: u64 = 1 << 10;
const NOT_GLOBAL: u64 = 1 << 11;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const OWNED: u64 = 1 << 55;
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;

pub const WRITE: u8 = 1;
pub const EXEC: u8 = 2;

static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);
static READY: AtomicBool = AtomicBool::new(false);

fn barrier_tables() {
    unsafe { core::arch::asm!("dsb ishst", "isb", options(nostack)) };
}

pub fn flush_tlb() {
    unsafe { core::arch::asm!("dsb ishst", "tlbi vmalle1is", "dsb ish", "isb", options(nostack)) };
}

fn flush_page(virt: u64) {
    unsafe { core::arch::asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) (virt >> 12) & 0xFFF_FFFF_FFFF, options(nostack)) };
}

pub fn load_root(root: u64) {
    unsafe { core::arch::asm!("dsb ishst", "msr ttbr0_el1, {}", "isb", in(reg) root, options(nostack)) };
    flush_tlb();
}

pub fn kernel_root() -> u64 {
    KERNEL_ROOT.load(Ordering::Relaxed)
}

pub fn vmalloc_ready() -> bool {
    READY.load(Ordering::Acquire)
}

fn index(virt: u64, level: usize) -> usize {
    ((virt >> (39 - 9 * level)) & 0x1FF) as usize
}

pub fn init(fdt: &Fdt) -> Option<usize> {
    let root = frame::alloc_zeroed_frame()? as u64;
    let identity = frame::alloc_zeroed_frame()? as u64;
    let vmalloc = frame::alloc_zeroed_frame()? as u64;
    let banks: alloc::vec::Vec<(u64, u64)> = fdt
        .nodes()
        .filter(|n| n.str_property("device_type") == Some("memory") && n.enabled())
        .flat_map(|n| n.reg())
        .collect();
    let mut normal = 0;
    unsafe {
        let entries = identity as *mut u64;
        for gib in 0..512u64 {
            let (start, end) = (gib << 30, (gib + 1) << 30);
            let ram = banks.iter().any(|&(base, size)| base < end && base + size > start);
            *entries.add(gib as usize) = if ram {
                normal += 1;
                start | BLOCK | ATTR_NORMAL | SH_INNER | ACCESSED | UXN
            } else {
                start | BLOCK | ATTR_DEVICE | ACCESSED | PXN | UXN
            };
        }
        let top = root as *mut u64;
        *top = identity | PAGE;
        *top.add(VMALLOC_INDEX) = vmalloc | PAGE;
    }
    KERNEL_ROOT.store(root, Ordering::Relaxed);
    load_root(root);
    READY.store(true, Ordering::Release);
    Some(normal)
}

unsafe fn walk(root: u64, virt: u64, create: bool) -> Option<*mut u64> {
    let mut table = root as *mut u64;
    for level in 0..3 {
        unsafe {
            let slot = table.add(index(virt, level));
            if *slot & VALID == 0 {
                if !create {
                    return None;
                }
                *slot = frame::alloc_zeroed_frame()? as u64 | PAGE;
            }
            if *slot & TABLE == 0 {
                return None;
            }
            table = (*slot & ADDR_MASK) as *mut u64;
        }
    }
    Some(unsafe { table.add(index(virt, 3)) })
}

pub fn kernel_map_page(virt: u64, phys: u64) -> bool {
    let Some(pte) = (unsafe { walk(kernel_root(), virt, true) }) else {
        return false;
    };
    unsafe { *pte = (phys & ADDR_MASK) | PAGE | ATTR_NORMAL | SH_INNER | ACCESSED | PXN | UXN };
    barrier_tables();
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
    Some(old & ADDR_MASK)
}

pub fn kernel_translate(virt: u64) -> Option<u64> {
    let pte = unsafe { walk(kernel_root(), virt, false) }?;
    let entry = unsafe { *pte };
    if entry & VALID == 0 {
        return None;
    }
    Some((entry & ADDR_MASK) | (virt & 0xFFF))
}


pub fn read_root() -> u64 {
    let current: u64;
    unsafe { core::arch::asm!("mrs {}, ttbr0_el1", out(reg) current, options(nomem, nostack)) };
    current & ADDR_MASK
}

pub use crate::memory::aspace::{is_user_range, resident_pages, AddressSpace, LazyRegion, ANON_NODE};

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

const ATTR_NONCACHED: u64 = 2 << 2;
const LAZY: u64 = 1 << 2;

pub mod pte {
    use super::*;

    pub use super::{USER_BASE, USER_END};

    pub const LEVELS: usize = 4;
    pub const USER_TOP: core::ops::Range<usize> = USER_INDEX..USER_INDEX + 1;

    #[inline]
    pub fn index(virt: u64, level: usize) -> usize {
        super::index(virt, level)
    }

    #[inline]
    pub fn valid(e: u64) -> bool {
        e & VALID != 0
    }

    #[inline]
    pub fn is_table(e: u64, _level: usize) -> bool {
        e & TABLE != 0
    }

    #[inline]
    pub fn table(phys: u64) -> u64 {
        phys | PAGE
    }

    #[inline]
    pub fn addr(e: u64) -> u64 {
        e & ADDR_MASK
    }

    #[inline]
    pub fn with_addr(e: u64, phys: u64) -> u64 {
        (phys & ADDR_MASK) | (e & !ADDR_MASK)
    }

    #[inline]
    pub fn user_page(phys: u64, owned: bool, wc: bool) -> u64 {
        let attr = if wc { ATTR_NONCACHED } else { ATTR_NORMAL };
        let mut e = (phys & ADDR_MASK) | PAGE | attr | SH_INNER | ACCESSED | NOT_GLOBAL | AP_EL0 | PXN;
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
        super::load_root(root)
    }

    pub fn flush_page(virt: u64) {
        super::flush_page(virt)
    }

    pub fn flush_all() {
        super::flush_tlb()
    }

    #[inline]
    pub fn barrier() {
        super::barrier_tables()
    }

    pub fn sync_code(phys: u64, len: u64) {
        let line = 64u64;
        let mut at = phys & !(line - 1);
        let end = phys.saturating_add(len);
        unsafe {
            while at < end {
                core::arch::asm!("dc cvau, {}", in(reg) at, options(nostack));
                at += line;
            }
            core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb", options(nostack));
        }
    }
}
