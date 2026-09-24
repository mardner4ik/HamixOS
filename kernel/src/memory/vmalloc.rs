use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use super::frame::{self, PAGE_SIZE};
use crate::arch::kmap;
use crate::arch::without_interrupts;

const MAGIC: [u64; 2] = [0x564D_414C_4C4F_4321, 0xC2B2_AE3D_27D4_EB4F];
const QUARANTINE: usize = 256;
const MIN_SLACK_PAGES: usize = 16;

#[derive(Clone, Copy)]
struct Hole {
    start: usize,
    pages: usize,
    epoch: u64,
}

struct Space {
    next: usize,
    holes: [Hole; QUARANTINE],
    count: usize,
}

static SPACE: Mutex<Space> = Mutex::new(Space { next: 0, holes: [Hole { start: 0, pages: 0, epoch: 0 }; QUARANTINE], count: 0 });
static EPOCH: AtomicU64 = AtomicU64::new(0);
static SEEN: [AtomicU64; kmap::MAX_CPUS] = [const { AtomicU64::new(0) }; kmap::MAX_CPUS];
static MAPPED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);

#[repr(C)]
struct Header {
    magic: [u64; 2],
    mapped: usize,
    reserved: usize,
}

pub fn contains(addr: usize) -> bool {
    addr.wrapping_sub(kmap::VMALLOC_BASE) < kmap::VMALLOC_SIZE
}

pub fn translate(addr: usize) -> Option<usize> {
    if contains(addr) { kmap::translate(addr) } else { Some(addr) }
}

pub fn contiguous_run(addr: usize, len: usize) -> (usize, usize) {
    if !contains(addr) {
        return (addr, len);
    }
    let Some(phys) = kmap::translate(addr) else {
        return (0, 0);
    };
    let mut run = (PAGE_SIZE - addr % PAGE_SIZE).min(len);
    while run < len {
        match kmap::translate(addr + run) {
            Some(next) if next == phys + run => run = (run + PAGE_SIZE).min(len),
            _ => break,
        }
    }
    (phys, run)
}

pub fn stats() -> (usize, usize) {
    (MAPPED.load(Ordering::Relaxed), LIVE.load(Ordering::Relaxed))
}

pub fn tick(cpu: usize) {
    if cpu >= kmap::MAX_CPUS {
        return;
    }
    let epoch = EPOCH.load(Ordering::Acquire);
    if SEEN[cpu].load(Ordering::Relaxed) != epoch {
        kmap::flush_local();
        SEEN[cpu].store(epoch, Ordering::Release);
    }
}

fn settled_epoch() -> u64 {
    let current = kmap::cpu_id();
    let mut settled = EPOCH.load(Ordering::Acquire);
    for cpu in 0..kmap::MAX_CPUS {
        if cpu != current && kmap::cpu_online(cpu) {
            settled = settled.min(SEEN[cpu].load(Ordering::Acquire));
        }
    }
    settled
}

impl Space {
    fn take(&mut self, pages: usize) -> Option<usize> {
        let settled = settled_epoch();
        for i in 0..self.count {
            let hole = self.holes[i];
            if hole.pages < pages || hole.epoch > settled {
                continue;
            }
            kmap::flush_local();
            if hole.pages == pages {
                self.count -= 1;
                self.holes[i] = self.holes[self.count];
            } else {
                self.holes[i].start += pages * PAGE_SIZE;
                self.holes[i].pages -= pages;
            }
            return Some(hole.start);
        }
        let bytes = pages.checked_mul(PAGE_SIZE)?;
        if self.next + bytes > kmap::VMALLOC_SIZE {
            return None;
        }
        let start = kmap::VMALLOC_BASE + self.next;
        self.next += bytes;
        Some(start)
    }

    fn give_back(&mut self, start: usize, pages: usize, epoch: u64) {
        if start + pages * PAGE_SIZE == kmap::VMALLOC_BASE + self.next && epoch <= settled_epoch() {
            self.next -= pages * PAGE_SIZE;
            return;
        }
        if self.count == QUARANTINE {
            let oldest = (0..self.count).min_by_key(|&i| self.holes[i].pages).unwrap_or(0);
            self.holes[oldest] = self.holes[self.count - 1];
            self.count -= 1;
        }
        self.holes[self.count] = Hole { start, pages, epoch };
        self.count += 1;
    }
}

fn retire(space: &mut Space, start: usize, pages: usize) {
    let epoch = EPOCH.fetch_add(1, Ordering::AcqRel) + 1;
    space.give_back(start, pages, epoch);
}

fn map_fresh(start: usize, from: usize, to: usize) -> bool {
    for page in from..to {
        let Some(phys) = frame::alloc_frame() else {
            unmap_range(start, from, page);
            return false;
        };
        if !kmap::map(start + page * PAGE_SIZE, phys) {
            frame::free_frame(phys);
            unmap_range(start, from, page);
            return false;
        }
    }
    MAPPED.fetch_add((to - from) * PAGE_SIZE, Ordering::Relaxed);
    true
}

fn unmap_range(start: usize, from: usize, to: usize) {
    let mut freed = 0;
    for page in from..to {
        if let Some(phys) = kmap::unmap(start + page * PAGE_SIZE) {
            frame::free_frame(phys);
            freed += PAGE_SIZE;
        }
    }
    MAPPED.fetch_sub(freed.min(MAPPED.load(Ordering::Relaxed)), Ordering::Relaxed);
}

fn pages_for(bytes: usize) -> usize {
    bytes.div_ceil(PAGE_SIZE) + 1
}

fn reserve_for(pages: usize) -> usize {
    pages.saturating_mul(2).max(pages + MIN_SLACK_PAGES)
}

unsafe fn header(pointer: *mut u8) -> Option<&'static mut Header> {
    let addr = pointer as usize;
    if !contains(addr) || addr % PAGE_SIZE != 0 {
        return None;
    }
    let header = unsafe { &mut *((addr - PAGE_SIZE) as *mut Header) };
    if header.magic != MAGIC {
        return None;
    }
    Some(header)
}

pub fn alloc(bytes: usize) -> Option<(*mut u8, usize)> {
    if !kmap::ready() {
        return None;
    }
    let pages = pages_for(bytes);
    let reserved = reserve_for(pages);
    without_interrupts(|| {
        let mut space = SPACE.lock();
        let start = space.take(reserved)?;
        if !map_fresh(start, 0, pages) {
            space.give_back(start, reserved, 0);
            return None;
        }
        unsafe {
            (start as *mut Header).write(Header { magic: MAGIC, mapped: pages, reserved });
        }
        LIVE.fetch_add(1, Ordering::Relaxed);
        Some(((start + PAGE_SIZE) as *mut u8, pages * PAGE_SIZE))
    })
}

pub fn free(pointer: *mut u8) -> usize {
    without_interrupts(|| {
        let Some(head) = (unsafe { header(pointer) }) else {
            return 0;
        };
        let (mapped, reserved) = (head.mapped, head.reserved);
        head.magic = [0, 0];
        let start = pointer as usize - PAGE_SIZE;
        let mut space = SPACE.lock();
        unmap_range(start, 0, mapped);
        retire(&mut space, start, reserved);
        LIVE.fetch_sub(1, Ordering::Relaxed);
        mapped * PAGE_SIZE
    })
}

pub fn usable(pointer: *mut u8) -> Option<usize> {
    let head = unsafe { header(pointer) }?;
    Some((head.mapped - 1) * PAGE_SIZE)
}

pub fn grow(pointer: *mut u8, bytes: usize) -> Option<(*mut u8, isize)> {
    without_interrupts(|| {
        let head = unsafe { header(pointer) }?;
        let (mapped, reserved) = (head.mapped, head.reserved);
        let pages = pages_for(bytes);
        let start = pointer as usize - PAGE_SIZE;
        if pages <= mapped {
            return Some((pointer, 0));
        }
        let mut space = SPACE.lock();
        if pages <= reserved {
            if !map_fresh(start, mapped, pages) {
                return None;
            }
            head.mapped = pages;
            return Some((pointer, ((pages - mapped) * PAGE_SIZE) as isize));
        }
        let wider = reserve_for(pages);
        let target = space.take(wider)?;
        for page in 0..mapped {
            let from = start + page * PAGE_SIZE;
            let to = target + page * PAGE_SIZE;
            let phys = kmap::translate(from)?;
            if !kmap::map(to, phys) {
                for undo in 0..page {
                    kmap::unmap(target + undo * PAGE_SIZE);
                }
                space.give_back(target, wider, 0);
                return None;
            }
        }
        if !map_fresh(target, mapped, pages) {
            for page in 0..mapped {
                kmap::unmap(target + page * PAGE_SIZE);
            }
            space.give_back(target, wider, 0);
            return None;
        }
        for page in 0..mapped {
            kmap::unmap(start + page * PAGE_SIZE);
        }
        retire(&mut space, start, reserved);
        let moved = unsafe { &mut *(target as *mut Header) };
        moved.mapped = pages;
        moved.reserved = wider;
        Some(((target + PAGE_SIZE) as *mut u8, ((pages - mapped) * PAGE_SIZE) as isize))
    })
}
