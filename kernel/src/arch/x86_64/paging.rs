use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::memory::frame;

pub const PAGE_SIZE: u64 = 4096;
pub const USER_BASE: u64 = 0x0000_0080_0000_0000;
pub const USER_END: u64 = 0x0000_0100_0000_0000;
pub const USER_MMAP_BASE: u64 = 0x0000_00A0_0000_0000;
pub const USER_SHM_BASE: u64 = 0x0000_00C0_0000_0000;
pub const USER_FB_BASE: u64 = 0x0000_00E0_0000_0000;
pub const USER_STACK_TOP: u64 = 0x0000_00FF_FFFF_F000;

const PRESENT: u64 = 1;
const WRITABLE: u64 = 2;
const USER: u64 = 4;
const HUGE: u64 = 0x80;
const PAT_4K: u64 = 0x80;
const PAT_2M: u64 = 0x1000;
const NO_EXECUTE: u64 = 1 << 63;
const OWNED: u64 = 0x200;
const LAZY: u64 = 0x400;
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const USER_P4_INDEX: usize = 1;
const VMALLOC_P4_INDEX: usize = 2;
pub const VMALLOC_BASE: u64 = (VMALLOC_P4_INDEX as u64) << 39;
pub const VMALLOC_SIZE: u64 = 1 << 39;

static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);
static PAT_WC: AtomicBool = AtomicBool::new(false);
static NX: AtomicBool = AtomicBool::new(false);
static VMALLOC_READY: AtomicBool = AtomicBool::new(false);

pub fn read_cr3() -> u64 {
    let cr3: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    cr3
}

pub fn load_cr3(value: u64) {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) value, options(nostack, preserves_flags)) };
}

pub fn flush_tlb() {
    load_cr3(read_cr3());
}

pub fn kernel_cr3() -> u64 {
    KERNEL_CR3.load(Ordering::Relaxed)
}

pub fn kernel_root() -> u64 {
    kernel_cr3()
}

pub fn read_root() -> u64 {
    read_cr3() & ADDR_MASK
}

pub fn load_root(root: u64) {
    load_cr3(root)
}

fn program_pat() -> bool {
    let leaf1 = core::arch::x86_64::__cpuid_count(1, 0);
    if leaf1.edx & (1 << 16) != 0 {
        const IA32_PAT: u32 = 0x277;
        let pat = super::read_msr(IA32_PAT);
        let updated = (pat & !(0xFFu64 << 32)) | (0x01u64 << 32);
        super::write_msr(IA32_PAT, updated);
        return true;
    }
    false
}

fn program_nx() -> bool {
    let leaf = core::arch::x86_64::__cpuid(0x8000_0001);
    if leaf.edx & (1 << 20) == 0 {
        return false;
    }
    const IA32_EFER: u32 = 0xC000_0080;
    let efer = super::read_msr(IA32_EFER);
    super::write_msr(IA32_EFER, efer | (1 << 11));
    true
}

pub fn no_execute() -> bool {
    NX.load(Ordering::Relaxed)
}

pub fn init() {
    KERNEL_CR3.store(read_cr3() & ADDR_MASK, Ordering::Relaxed);
    if program_pat() {
        PAT_WC.store(true, Ordering::Relaxed);
    }
    if program_nx() {
        NX.store(true, Ordering::Relaxed);
    }
    unsafe {
        let slot = (kernel_cr3() as *mut u64).add(VMALLOC_P4_INDEX);
        if *slot & PRESENT == 0 {
            if let Some(table) = frame::alloc_zeroed_frame() {
                *slot = table as u64 | PRESENT | WRITABLE;
            }
        }
        if *slot & PRESENT != 0 {
            VMALLOC_READY.store(true, Ordering::Release);
        }
    }
}

pub fn vmalloc_ready() -> bool {
    VMALLOC_READY.load(Ordering::Acquire)
}

pub fn invlpg(virt: u64) {
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags)) };
}

unsafe fn kernel_pte(virt: u64, create: bool) -> Option<*mut u64> {
    let mut table = kernel_cr3() as *mut u64;
    for shift in [39u64, 30, 21] {
        unsafe {
            let slot = table.add(((virt >> shift) & 0x1FF) as usize);
            if *slot & PRESENT == 0 {
                if !create {
                    return None;
                }
                *slot = frame::alloc_zeroed_frame()? as u64 | PRESENT | WRITABLE;
            }
            if *slot & HUGE != 0 {
                return None;
            }
            table = (*slot & ADDR_MASK) as *mut u64;
        }
    }
    Some(unsafe { table.add(((virt >> 12) & 0x1FF) as usize) })
}

pub fn kernel_map_page(virt: u64, phys: u64) -> bool {
    let Some(pte) = (unsafe { kernel_pte(virt, true) }) else {
        return false;
    };
    let nx = if no_execute() { NO_EXECUTE } else { 0 };
    unsafe { *pte = (phys & ADDR_MASK) | PRESENT | WRITABLE | nx };
    true
}

pub fn kernel_unmap_page(virt: u64) -> Option<u64> {
    let pte = unsafe { kernel_pte(virt, false) }?;
    let old = unsafe { *pte };
    if old & PRESENT == 0 {
        return None;
    }
    unsafe { *pte = 0 };
    invlpg(virt);
    Some(old & ADDR_MASK)
}

pub fn kernel_translate(virt: u64) -> Option<u64> {
    let pte = unsafe { kernel_pte(virt, false) }?;
    let entry = unsafe { *pte };
    if entry & PRESENT == 0 {
        return None;
    }
    Some((entry & ADDR_MASK) | (virt & 0xFFF))
}

pub fn init_ap() {
    if PAT_WC.load(Ordering::Relaxed) {
        program_pat();
    }
    if NX.load(Ordering::Relaxed) {
        program_nx();
    }
    flush_tlb();
}

const CACHE_DISABLE: u64 = 1 << 4;
const WRITE_THROUGH: u64 = 1 << 3;
pub const KERNEL_IDENTITY_LIMIT: u64 = 512u64 << 30;

pub fn map_kernel_mmio(phys: u64, len: u64) -> bool {
    const PAGE_2M: u64 = 0x20_0000;
    if len == 0 || phys.saturating_add(len) > KERNEL_IDENTITY_LIMIT {
        return false;
    }
    let start = phys & !(PAGE_2M - 1);
    let end = (phys + len + PAGE_2M - 1) & !(PAGE_2M - 1);
    let mut page = start;
    unsafe {
        let p4 = kernel_cr3() as *mut u64;
        while page < end {
            let p4e = *p4.add(((page >> 39) & 0x1FF) as usize);
            if p4e & PRESENT == 0 {
                return false;
            }
            let p3 = (p4e & ADDR_MASK) as *mut u64;
            let p3_slot = p3.add(((page >> 30) & 0x1FF) as usize);
            if *p3_slot & PRESENT == 0 {
                let Some(fresh) = frame::alloc_zeroed_frame() else {
                    return false;
                };
                *p3_slot = (fresh as u64) | PRESENT | WRITABLE;
            } else if *p3_slot & HUGE != 0 {
                page += PAGE_2M;
                continue;
            }
            let p2 = (*p3_slot & ADDR_MASK) as *mut u64;
            let p2_slot = p2.add(((page >> 21) & 0x1FF) as usize);
            if *p2_slot & PRESENT == 0 {
                *p2_slot = page | PRESENT | WRITABLE | HUGE | CACHE_DISABLE | WRITE_THROUGH;
            }
            page += PAGE_2M;
        }
    }
    flush_tlb();
    true
}

unsafe fn split_kernel_2m(addr: u64) -> Option<*mut u64> {
    unsafe {
        let p4 = kernel_cr3() as *mut u64;
        let p4e = *p4.add(((addr >> 39) & 0x1FF) as usize);
        if p4e & PRESENT == 0 {
            return None;
        }
        let p3 = (p4e & ADDR_MASK) as *mut u64;
        let p3e = *p3.add(((addr >> 30) & 0x1FF) as usize);
        if p3e & PRESENT == 0 || p3e & HUGE != 0 {
            return None;
        }
        let p2 = (p3e & ADDR_MASK) as *mut u64;
        let slot = p2.add(((addr >> 21) & 0x1FF) as usize);
        let entry = *slot;
        if entry & PRESENT == 0 {
            return None;
        }
        if entry & HUGE == 0 {
            return Some((entry & ADDR_MASK) as *mut u64);
        }
        let table = frame::alloc_zeroed_frame()? as *mut u64;
        let base = entry & ADDR_MASK & !0x1F_FFFF;
        let flags = entry & (PRESENT | WRITABLE | USER);
        let pat = if entry & PAT_2M != 0 { 0x80 } else { 0 };
        for index in 0..512u64 {
            *table.add(index as usize) = (base + index * PAGE_SIZE) | flags | pat;
        }
        *slot = (table as u64) | PRESENT | WRITABLE | USER;
        Some(table)
    }
}

pub fn protect_kernel_range(addr: u64, len: u64, writable: bool, executable: bool) -> bool {
    if !no_execute() && !executable {
        return false;
    }
    let start = addr & !(PAGE_SIZE - 1);
    let end = (addr + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let mut page = start;
    while page < end {
        let Some(table) = (unsafe { split_kernel_2m(page) }) else {
            return false;
        };
        let mut index = ((page >> 12) & 0x1FF) as usize;
        while index < 512 && page < end {
            unsafe {
                let slot = table.add(index);
                let mut entry = *slot;
                if writable {
                    entry |= WRITABLE;
                } else {
                    entry &= !WRITABLE;
                }
                if executable || !no_execute() {
                    entry &= !NO_EXECUTE;
                } else {
                    entry |= NO_EXECUTE;
                }
                *slot = entry;
            }
            index += 1;
            page += PAGE_SIZE;
        }
    }
    flush_tlb();
    true
}

pub fn write_combining() -> bool {
    PAT_WC.load(Ordering::Relaxed)
}

pub fn enable_kernel_write_combining(addr: u64, len: u64) {
    if !write_combining() {
        return;
    }
    const PAGE_2M: u64 = 0x20_0000;
    let start = (addr + PAGE_2M - 1) & !(PAGE_2M - 1);
    let end = (addr + len) & !(PAGE_2M - 1);
    let mut cur = start;
    unsafe {
        let p4 = kernel_cr3() as *mut u64;
        while cur < end && cur < (1u64 << 32) {
            let p4e = *p4.add(((cur >> 39) & 0x1FF) as usize);
            if p4e & PRESENT == 0 {
                break;
            }
            let p3 = (p4e & ADDR_MASK) as *mut u64;
            let p3e = *p3.add(((cur >> 30) & 0x1FF) as usize);
            if p3e & PRESENT == 0 || p3e & HUGE != 0 {
                break;
            }
            let p2 = (p3e & ADDR_MASK) as *mut u64;
            let slot = p2.add(((cur >> 21) & 0x1FF) as usize);
            if *slot & (PRESENT | HUGE) == PRESENT | HUGE {
                *slot |= PAT_2M;
            }
            cur += PAGE_2M;
        }
    }
    flush_tlb();
}

pub use crate::memory::aspace::{is_user_range, resident_pages, AddressSpace, LazyRegion, ANON_NODE};

pub mod pte {
    use super::*;

    pub use super::{USER_BASE, USER_END};

    pub const LEVELS: usize = 4;
    pub const USER_TOP: core::ops::Range<usize> = USER_P4_INDEX..USER_P4_INDEX + 1;

    #[inline]
    pub fn index(virt: u64, level: usize) -> usize {
        ((virt >> (39 - 9 * level as u64)) & 0x1FF) as usize
    }

    #[inline]
    pub fn valid(e: u64) -> bool {
        e & PRESENT != 0
    }

    #[inline]
    pub fn is_table(e: u64, _level: usize) -> bool {
        e & HUGE == 0
    }

    #[inline]
    pub fn table(phys: u64) -> u64 {
        phys | PRESENT | WRITABLE | USER
    }

    #[inline]
    pub fn addr(e: u64) -> u64 {
        e & ADDR_MASK
    }

    #[inline]
    pub fn with_addr(e: u64, phys: u64) -> u64 {
        phys | (e & !ADDR_MASK)
    }

    #[inline]
    pub fn user_page(phys: u64, owned: bool, wc: bool) -> u64 {
        let mut e = (phys & ADDR_MASK) | PRESENT | WRITABLE | USER;
        if owned {
            e |= OWNED;
        }
        if wc && write_combining() {
            e |= PAT_4K;
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
        e & PRESENT == 0 && e & LAZY != 0
    }

    #[inline]
    pub fn lazy_index(e: u64) -> usize {
        ((e & ADDR_MASK) >> 12) as usize
    }

    pub fn kernel_root() -> u64 {
        kernel_cr3()
    }

    pub fn read_root() -> u64 {
        read_cr3() & ADDR_MASK
    }

    pub fn load_root(root: u64) {
        load_cr3(root)
    }

    pub fn flush_page(virt: u64) {
        invlpg(virt)
    }

    pub fn flush_all() {
        flush_tlb()
    }

    #[inline]
    pub fn barrier() {}

    #[inline]
    pub fn sync_code(_phys: u64, _len: u64) {}
}
