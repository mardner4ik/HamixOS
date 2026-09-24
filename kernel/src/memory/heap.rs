use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use super::{frame, vmalloc};
use crate::arch::without_interrupts;

const STATIC_HEAP_SIZE: usize = 2 * 1024 * 1024;
const GROW_CHUNK: usize = 2 * 1024 * 1024;
const MAX_CHUNKS: usize = 1024;
const KEEP_FREE: usize = 2 * 1024 * 1024;

const ALIGN: usize = 16;
const HEADER: usize = 16;
const MIN_BLOCK: usize = 32;
const FENCE: usize = 16;

const FLAG_INUSE: usize = 1;
const FLAG_PREV_INUSE: usize = 2;
const FLAG_MASK: usize = FLAG_INUSE | FLAG_PREV_INUSE;

const SMALL_BINS: usize = 64;
const LARGE_BINS: usize = 32;
const BINS: usize = SMALL_BINS + LARGE_BINS;

#[repr(align(16))]
struct AlignedHeap([u8; STATIC_HEAP_SIZE]);

static mut HEAP_STORAGE: AlignedHeap = AlignedHeap([0u8; STATIC_HEAP_SIZE]);

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

#[inline(always)]
unsafe fn head(block: usize) -> usize {
    unsafe { ptr::read(block as *const usize) }
}

#[inline(always)]
unsafe fn set_head(block: usize, value: usize) {
    unsafe { ptr::write(block as *mut usize, value) }
}

#[inline(always)]
unsafe fn block_size(block: usize) -> usize {
    unsafe { head(block) & !FLAG_MASK }
}

#[inline(always)]
unsafe fn is_inuse(block: usize) -> bool {
    unsafe { head(block) & FLAG_INUSE != 0 }
}

#[inline(always)]
unsafe fn set_next(block: usize, value: usize) {
    unsafe { ptr::write((block + 8) as *mut usize, value) }
}

#[inline(always)]
unsafe fn next_link(block: usize) -> usize {
    unsafe { ptr::read((block + 8) as *const usize) }
}

#[inline(always)]
unsafe fn set_prev(block: usize, value: usize) {
    unsafe { ptr::write((block + 16) as *mut usize, value) }
}

#[inline(always)]
unsafe fn prev_link(block: usize) -> usize {
    unsafe { ptr::read((block + 16) as *const usize) }
}

#[inline(always)]
unsafe fn write_footer(block: usize, size: usize) {
    unsafe { ptr::write((block + size - 8) as *mut usize, size) }
}

#[inline(always)]
unsafe fn previous_size(block: usize) -> usize {
    unsafe { ptr::read((block - 8) as *const usize) }
}

fn bin_for(size: usize) -> usize {
    let slot = size / ALIGN;
    if slot < SMALL_BINS {
        return slot;
    }
    let order = usize::BITS as usize - 1 - size.leading_zeros() as usize;
    (SMALL_BINS + order - 10).min(BINS - 1)
}

pub struct BlockAllocator {
    bins: [usize; BINS],
    bitmap: [u64; 2],
    chunks: [(usize, usize); MAX_CHUNKS],
    chunk_count: usize,
    free_bytes: usize,
}

unsafe impl Send for BlockAllocator {}

impl BlockAllocator {
    const fn new() -> Self {
        Self { bins: [0; BINS], bitmap: [0; 2], chunks: [(0, 0); MAX_CHUNKS], chunk_count: 0, free_bytes: 0 }
    }

    fn mark_bin(&mut self, bin: usize, occupied: bool) {
        let (word, bit) = (bin / 64, 1u64 << (bin % 64));
        if occupied {
            self.bitmap[word] |= bit;
        } else {
            self.bitmap[word] &= !bit;
        }
    }

    unsafe fn link_free(&mut self, block: usize, size: usize) {
        let bin = bin_for(size);
        let head_block = self.bins[bin];
        unsafe {
            set_next(block, head_block);
            set_prev(block, 0);
            if head_block != 0 {
                set_prev(head_block, block);
            }
        }
        self.bins[bin] = block;
        self.mark_bin(bin, true);
    }

    unsafe fn unlink_free(&mut self, block: usize, size: usize) {
        let bin = bin_for(size);
        unsafe {
            let (next, prev) = (next_link(block), prev_link(block));
            if prev != 0 {
                set_next(prev, next);
            } else {
                self.bins[bin] = next;
            }
            if next != 0 {
                set_prev(next, prev);
            }
        }
        if self.bins[bin] == 0 {
            self.mark_bin(bin, false);
        }
    }

    unsafe fn make_free(&mut self, block: usize, size: usize, prev_flag: usize) {
        unsafe {
            set_head(block, size | prev_flag);
            write_footer(block, size);
            let after = block + size;
            set_head(after, head(after) & !FLAG_PREV_INUSE);
            self.link_free(block, size);
        }
        self.free_bytes += size;
    }

    unsafe fn take_free(&mut self, block: usize, size: usize) {
        unsafe { self.unlink_free(block, size) };
        self.free_bytes -= size;
    }

    unsafe fn insert_region(&mut self, base: usize, size: usize) {
        let start = align_up(base, ALIGN);
        let end = (base + size) & !(ALIGN - 1);
        if end < start + FENCE * 2 + MIN_BLOCK {
            return;
        }
        let body = start + FENCE;
        let body_size = end - FENCE - body;
        unsafe {
            set_head(start, FENCE | FLAG_INUSE | FLAG_PREV_INUSE);
            set_head(end - FENCE, FENCE | FLAG_INUSE);
            self.make_free(body, body_size, FLAG_PREV_INUSE);
        }
    }

    unsafe fn find_fit(&self, need: usize, align: usize) -> Option<(usize, usize, usize)> {
        let first = bin_for((need + HEADER).max(MIN_BLOCK));
        for bin in first..BINS {
            if self.bitmap[bin / 64] & (1u64 << (bin % 64)) == 0 {
                continue;
            }
            let mut block = self.bins[bin];
            let mut scanned = 0usize;
            while block != 0 && scanned < 64 {
                scanned += 1;
                let size = unsafe { block_size(block) };
                let payload = align_up(block + HEADER, align);
                if payload + need <= block + size {
                    return Some((block, size, payload));
                }
                block = unsafe { next_link(block) };
            }
        }
        None
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(ALIGN);
        let need = align_up(layout.size().max(1), ALIGN);
        let Some((block, size, payload)) = (unsafe { self.find_fit(need, align) }) else {
            return ptr::null_mut();
        };
        unsafe { self.take_free(block, size) };
        let used_end = align_up(payload + need, ALIGN);
        let leftover = block + size - used_end;
        let taken = if leftover >= MIN_BLOCK { used_end - block } else { size };
        unsafe {
            let prev_flag = head(block) & FLAG_PREV_INUSE;
            set_head(block, taken | FLAG_INUSE | prev_flag);
            let after = block + taken;
            if taken != size {
                self.make_free(after, size - taken, FLAG_PREV_INUSE);
            } else {
                set_head(after, head(after) | FLAG_PREV_INUSE);
            }
            ptr::write((payload - 8) as *mut usize, block);
        }
        payload as *mut u8
    }

    unsafe fn dealloc(&mut self, pointer: *mut u8) {
        let mut block = unsafe { ptr::read((pointer as usize - 8) as *const usize) };
        let mut size = unsafe { block_size(block) };
        let mut prev_flag = unsafe { head(block) & FLAG_PREV_INUSE };
        unsafe {
            let after = block + size;
            if !is_inuse(after) {
                let after_size = block_size(after);
                self.take_free(after, after_size);
                size += after_size;
            }
            if prev_flag == 0 {
                let before_size = previous_size(block);
                let before = block - before_size;
                self.take_free(before, before_size);
                block = before;
                size += before_size;
                prev_flag = head(block) & FLAG_PREV_INUSE;
            }
            self.make_free(block, size, prev_flag);
        }
    }

    fn add_chunk(&mut self, base: usize, size: usize) -> bool {
        if self.chunk_count >= MAX_CHUNKS {
            return false;
        }
        self.chunks[self.chunk_count] = (base, size);
        self.chunk_count += 1;
        unsafe { self.insert_region(base, size) };
        true
    }

    unsafe fn whole_chunk_free(&self, base: usize, size: usize) -> Option<(usize, usize)> {
        let start = align_up(base, ALIGN);
        let end = (base + size) & !(ALIGN - 1);
        if end < start + FENCE * 2 + MIN_BLOCK {
            return None;
        }
        let body = start + FENCE;
        let body_size = end - FENCE - body;
        if unsafe { is_inuse(body) } || unsafe { block_size(body) } != body_size {
            return None;
        }
        Some((body, body_size))
    }
}

pub struct LockedAllocator(Mutex<BlockAllocator>);

impl LockedAllocator {
    const fn new() -> Self {
        Self(Mutex::new(BlockAllocator::new()))
    }
}

static HEAP_TOTAL: AtomicUsize = AtomicUsize::new(0);
static HEAP_PEAK: AtomicUsize = AtomicUsize::new(0);

fn release_chunk(base: usize, size: usize) {
    if vmalloc::contains(base) {
        vmalloc::free(base as *mut u8);
        return;
    }
    for page in (0..size).step_by(frame::PAGE_SIZE) {
        frame::free_frame(base + page);
    }
}

fn grow(needed: usize) -> bool {
    let size = align_up(needed + FENCE * 2 + MIN_BLOCK, frame::PAGE_SIZE).max(GROW_CHUNK);
    let base = match frame::alloc_contiguous(size / frame::PAGE_SIZE, crate::arch::HEAP_LIMIT) {
        Some(base) => base,
        None => match vmalloc::alloc(size) {
            Some((pointer, _)) => pointer as usize,
            None => return false,
        },
    };
    if !ALLOCATOR.0.lock().add_chunk(base, size) {
        release_chunk(base, size);
        return false;
    }
    let total = HEAP_TOTAL.fetch_add(size, Ordering::Relaxed) + size;
    HEAP_PEAK.fetch_max(total, Ordering::Relaxed);
    true
}

const LARGE: usize = 256 * 1024;
const VM_MIN: usize = 64 * 1024;

fn vm_block(pointer: *mut u8, size: usize) -> Option<usize> {
    if size < VM_MIN || !vmalloc::contains(pointer as usize) {
        return None;
    }
    vmalloc::usable(pointer)
}

fn account(delta: isize) {
    if delta >= 0 {
        let total = HEAP_TOTAL.fetch_add(delta as usize, Ordering::Relaxed) + delta as usize;
        HEAP_PEAK.fetch_max(total, Ordering::Relaxed);
    } else {
        HEAP_TOTAL.fetch_sub(delta.unsigned_abs(), Ordering::Relaxed);
    }
}
const LARGE_MAGIC: [u64; 2] = [0x4C41_5247_4548_4D58, 0x9E37_79B9_7F4A_7C15];

fn large_alloc(size: usize) -> *mut u8 {
    let pages = (size + frame::PAGE_SIZE).div_ceil(frame::PAGE_SIZE);
    let Some(base) = frame::alloc_contiguous(pages, crate::arch::HEAP_LIMIT) else {
        return ptr::null_mut();
    };
    unsafe {
        let header = base as *mut u64;
        *header = LARGE_MAGIC[0];
        *header.add(1) = LARGE_MAGIC[1];
        *header.add(2) = pages as u64;
    }
    HEAP_TOTAL.fetch_add(pages * frame::PAGE_SIZE, Ordering::Relaxed);
    (base + frame::PAGE_SIZE) as *mut u8
}

fn large_pages(pointer: *mut u8, size: usize) -> Option<usize> {
    let addr = pointer as usize;
    if size < LARGE || addr % frame::PAGE_SIZE != 0 || addr < frame::PAGE_SIZE {
        return None;
    }
    unsafe {
        let header = (addr - frame::PAGE_SIZE) as *const u64;
        if *header == LARGE_MAGIC[0] && *header.add(1) == LARGE_MAGIC[1] {
            return Some(*header.add(2) as usize);
        }
    }
    None
}

fn large_free(pointer: *mut u8, pages: usize) {
    let base = pointer as usize - frame::PAGE_SIZE;
    unsafe {
        let header = base as *mut u64;
        *header = 0;
        *header.add(1) = 0;
    }
    for page in 0..pages {
        frame::free_frame(base + page * frame::PAGE_SIZE);
    }
    HEAP_TOTAL.fetch_sub(pages * frame::PAGE_SIZE, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for LockedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= LARGE && layout.align() <= frame::PAGE_SIZE {
            if let Some((pointer, mapped)) = vmalloc::alloc(layout.size()) {
                account(mapped as isize);
                return pointer;
            }
            let pointer = large_alloc(layout.size());
            if !pointer.is_null() {
                return pointer;
            }
        }
        without_interrupts(|| {
            let pointer = unsafe { self.0.lock().alloc(layout) };
            if !pointer.is_null() {
                return pointer;
            }
            if !grow(layout.size() + layout.align() + HEADER * 2) {
                return ptr::null_mut();
            }
            unsafe { self.0.lock().alloc(layout) }
        })
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if vm_block(pointer, layout.size()).is_some() {
            account(-(vmalloc::free(pointer) as isize));
            return;
        }
        if let Some(pages) = large_pages(pointer, layout.size()) {
            large_free(pointer, pages);
            return;
        }
        without_interrupts(|| unsafe { self.0.lock().dealloc(pointer) });
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if let Some(usable) = vm_block(pointer, layout.size()) {
            if new_size >= VM_MIN && new_size.saturating_mul(2) >= usable {
                if let Some((moved, delta)) = vmalloc::grow(pointer, new_size) {
                    account(delta);
                    return moved;
                }
            }
            let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
            let fresh = unsafe { self.alloc(new_layout) };
            if !fresh.is_null() {
                unsafe {
                    ptr::copy_nonoverlapping(pointer, fresh, layout.size().min(new_size));
                    self.dealloc(pointer, layout);
                }
            }
            return fresh;
        }
        let large = large_pages(pointer, layout.size());
        if let Some(pages) = large {
            if new_size + frame::PAGE_SIZE <= pages * frame::PAGE_SIZE {
                return pointer;
            }
        }
        let grown = large.is_none() && new_size < LARGE && unsafe {
            without_interrupts(|| {
                let mut heap = self.0.lock();
                let block = ptr::read((pointer as usize - 8) as *const usize);
                let size = block_size(block);
                let slack = block + size - pointer as usize;
                if new_size <= slack {
                    return true;
                }
                let after = block + size;
                if is_inuse(after) {
                    return false;
                }
                let after_size = block_size(after);
                if slack + after_size < align_up(new_size, ALIGN) {
                    return false;
                }
                heap.take_free(after, after_size);
                let merged = size + after_size;
                let wanted = align_up(pointer as usize + new_size, ALIGN) - block;
                let prev_flag = head(block) & FLAG_PREV_INUSE;
                let taken = if merged - wanted >= MIN_BLOCK { wanted } else { merged };
                set_head(block, taken | FLAG_INUSE | prev_flag);
                if taken != merged {
                    heap.make_free(block + taken, merged - taken, FLAG_PREV_INUSE);
                } else {
                    let end = block + merged;
                    set_head(end, head(end) | FLAG_PREV_INUSE);
                }
                true
            })
        };
        if grown {
            return pointer;
        }
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        let fresh = unsafe { self.alloc(new_layout) };
        if !fresh.is_null() {
            unsafe {
                ptr::copy_nonoverlapping(pointer, fresh, layout.size().min(new_size));
                self.dealloc(pointer, layout);
            }
        }
        fresh
    }
}

#[global_allocator]
pub static ALLOCATOR: LockedAllocator = LockedAllocator::new();

pub fn trim() -> usize {
    let mut released: [(usize, usize); 64] = [(0, 0); 64];
    let mut count = 0;
    without_interrupts(|| {
        let mut heap = ALLOCATOR.0.lock();
        let mut spare = heap.free_bytes;
        let mut i = heap.chunk_count;
        while i > 0 && count < released.len() {
            i -= 1;
            let (base, size) = heap.chunks[i];
            if base == (&raw const HEAP_STORAGE) as usize || spare < size + KEEP_FREE {
                continue;
            }
            let Some((body, body_size)) = (unsafe { heap.whole_chunk_free(base, size) }) else {
                continue;
            };
            unsafe { heap.take_free(body, body_size) };
            spare -= body_size;
            released[count] = (base, size);
            count += 1;
            let last = heap.chunk_count - 1;
            heap.chunks[i] = heap.chunks[last];
            heap.chunk_count = last;
        }
    });
    let mut bytes = 0;
    for &(base, size) in &released[..count] {
        release_chunk(base, size);
        HEAP_TOTAL.fetch_sub(size, Ordering::Relaxed);
        bytes += size;
    }
    bytes
}

pub fn stats() -> (usize, usize) {
    let free = without_interrupts(|| ALLOCATOR.0.lock().free_bytes);
    (free, HEAP_TOTAL.load(Ordering::Relaxed))
}

pub fn init() {
    HEAP_TOTAL.store(STATIC_HEAP_SIZE, Ordering::Relaxed);
    without_interrupts(|| {
        ALLOCATOR.0.lock().add_chunk((&raw mut HEAP_STORAGE) as usize, STATIC_HEAP_SIZE);
    });
}
