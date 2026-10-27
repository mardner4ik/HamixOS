use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

const MIN_ALIGN: usize = 16;
const SPLIT_THRESHOLD: usize = 64;
const GROW_SLACK: usize = 256 * 1024;
const SLAB_SIZE: usize = 64 * 1024;
const LARGE_THRESHOLD: usize = 128 * 1024;
const PAGE: usize = 4096;
const CLASSES: [usize; 16] = [16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 3072];

#[repr(C)]
struct Block {
    size: usize,
    next: *mut Block,
}

struct Slot {
    next: *mut Slot,
}

static mut FREE_LIST: *mut Block = ptr::null_mut();
static mut CLASS_FREE: [*mut Slot; CLASSES.len()] = [ptr::null_mut(); CLASSES.len()];
static mut SLAB_CURSOR: usize = 0;
static mut SLAB_END: usize = 0;
static mut MAPPED_BYTES: usize = 0;

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn class_of(layout: &Layout) -> Option<usize> {
    if layout.align() > MIN_ALIGN {
        return None;
    }
    let size = layout.size().max(1);
    CLASSES.iter().position(|c| *c >= size)
}

fn is_large(layout: &Layout) -> bool {
    layout.size() >= LARGE_THRESHOLD
}

unsafe fn insert_free(block: *mut Block) {
    unsafe {
        let mut previous: *mut Block = ptr::null_mut();
        let mut cursor = FREE_LIST;
        while !cursor.is_null() && cursor < block {
            previous = cursor;
            cursor = (*cursor).next;
        }
        (*block).next = cursor;
        if !cursor.is_null() && (block as usize) + (*block).size == cursor as usize {
            (*block).size += (*cursor).size;
            (*block).next = (*cursor).next;
        }
        if previous.is_null() {
            FREE_LIST = block;
        } else if (previous as usize) + (*previous).size == block as usize {
            (*previous).size += (*block).size;
            (*previous).next = (*block).next;
        } else {
            (*previous).next = block;
        }
    }
}

unsafe fn take_free(request: usize) -> *mut Block {
    unsafe {
        let mut cursor = &raw mut FREE_LIST;
        while !(*cursor).is_null() {
            let candidate = *cursor;
            let size = (*candidate).size;
            if size >= request {
                if size - request >= SPLIT_THRESHOLD {
                    let remainder = (candidate as usize + request) as *mut Block;
                    (*remainder).size = size - request;
                    (*remainder).next = (*candidate).next;
                    (*candidate).size = request;
                    *cursor = remainder;
                } else {
                    *cursor = (*candidate).next;
                }
                return candidate;
            }
            cursor = &raw mut (*candidate).next;
        }
        ptr::null_mut()
    }
}

unsafe fn grow_brk(request: usize) -> *mut u8 {
    let current = crate::sys::brk(0);
    if current < 0 {
        return ptr::null_mut();
    }
    let start = align_up(current as usize, MIN_ALIGN);
    let target = start + request;
    if crate::sys::brk(target) < target as i64 {
        return ptr::null_mut();
    }
    start as *mut u8
}

unsafe fn grow(request: usize) -> *mut Block {
    unsafe {
        let total = request + GROW_SLACK;
        let mut start = grow_brk(total);
        let mut spare = GROW_SLACK;
        if start.is_null() {
            start = grow_brk(request);
            spare = 0;
            if start.is_null() {
                return ptr::null_mut();
            }
        }
        let block = start as *mut Block;
        (*block).size = request;
        (*block).next = ptr::null_mut();
        if spare > 0 {
            let rest = (start as usize + request) as *mut Block;
            (*rest).size = spare;
            (*rest).next = ptr::null_mut();
            insert_free(rest);
        }
        block
    }
}

unsafe fn medium_alloc(layout: Layout) -> *mut u8 {
    let align = layout.align().max(MIN_ALIGN);
    let request = align_up(MIN_ALIGN + align + layout.size(), MIN_ALIGN);
    unsafe {
        let mut block = take_free(request);
        if block.is_null() {
            block = grow(request);
        }
        if block.is_null() {
            return ptr::null_mut();
        }
        let payload = align_up(block as usize + MIN_ALIGN, align);
        ptr::write((payload - 8) as *mut usize, payload - block as usize);
        payload as *mut u8
    }
}

unsafe fn medium_free(pointer: *mut u8) {
    unsafe {
        let offset = ptr::read((pointer as usize - 8) as *const usize);
        let block = (pointer as usize - offset) as *mut Block;
        insert_free(block);
    }
}

unsafe fn small_alloc(class: usize) -> *mut u8 {
    unsafe {
        let head = CLASS_FREE[class];
        if !head.is_null() {
            CLASS_FREE[class] = (*head).next;
            return head as *mut u8;
        }
        let size = CLASSES[class];
        if SLAB_CURSOR + size > SLAB_END {
            let slab = medium_alloc(Layout::from_size_align_unchecked(SLAB_SIZE, MIN_ALIGN));
            if slab.is_null() {
                return ptr::null_mut();
            }
            SLAB_CURSOR = slab as usize;
            SLAB_END = slab as usize + SLAB_SIZE;
        }
        let slot = SLAB_CURSOR;
        SLAB_CURSOR += size;
        slot as *mut u8
    }
}

unsafe fn small_free(class: usize, pointer: *mut u8) {
    unsafe {
        let slot = pointer as *mut Slot;
        (*slot).next = CLASS_FREE[class];
        CLASS_FREE[class] = slot;
    }
}

fn large_span(layout: &Layout) -> (usize, usize) {
    let align = layout.align().max(MIN_ALIGN);
    let header = align_up(16, align);
    (header, align_up(header + layout.size(), PAGE))
}

const CACHE_SLOTS: usize = 12;
const CACHE_LIMIT: usize = 16 * 1024 * 1024;

static mut CACHE: [(usize, usize); CACHE_SLOTS] = [(0, 0); CACHE_SLOTS];
static mut CACHED_BYTES: usize = 0;

unsafe fn cache_take(span: usize) -> usize {
    unsafe {
        let mut best = CACHE_SLOTS;
        for i in 0..CACHE_SLOTS {
            let (base, size) = CACHE[i];
            if base != 0 && size >= span && size <= span + span / 2 + PAGE * 16 && (best == CACHE_SLOTS || size < CACHE[best].1) {
                best = i;
            }
        }
        if best == CACHE_SLOTS {
            return 0;
        }
        let (base, size) = CACHE[best];
        CACHE[best] = (0, 0);
        CACHED_BYTES -= size;
        base
    }
}

unsafe fn cache_put(base: usize, span: usize) -> bool {
    unsafe {
        if span > CACHE_LIMIT / 2 {
            return false;
        }
        while CACHED_BYTES + span > CACHE_LIMIT {
            let largest = (0..CACHE_SLOTS).filter(|i| CACHE[*i].0 != 0).max_by_key(|i| CACHE[*i].1);
            let Some(i) = largest else {
                break;
            };
            let (b, s) = CACHE[i];
            CACHE[i] = (0, 0);
            CACHED_BYTES -= s;
            crate::sys::munmap(b as u64, s as u64);
        }
        let slot = match (0..CACHE_SLOTS).find(|i| CACHE[*i].0 == 0) {
            Some(i) => i,
            None => {
                let i = (0..CACHE_SLOTS).min_by_key(|i| CACHE[*i].1).unwrap_or(0);
                let (b, s) = CACHE[i];
                CACHED_BYTES -= s;
                crate::sys::munmap(b as u64, s as u64);
                i
            }
        };
        CACHE[slot] = (base, span);
        CACHED_BYTES += span;
        true
    }
}

pub fn release_cached() {
    unsafe {
        for i in 0..CACHE_SLOTS {
            let (b, s) = CACHE[i];
            if b != 0 {
                CACHE[i] = (0, 0);
                crate::sys::munmap(b as u64, s as u64);
            }
        }
        CACHED_BYTES = 0;
    }
}

unsafe fn large_alloc(layout: Layout, zeroed: bool) -> *mut u8 {
    let (header, span) = large_span(&layout);
    unsafe {
        let cached = cache_take(span);
        if cached != 0 {
            let real = ptr::read(cached as *const usize);
            MAPPED_BYTES += real;
            if zeroed {
                ptr::write_bytes((cached + header) as *mut u8, 0, layout.size());
            }
            return (cached + header) as *mut u8;
        }
    }
    let base = crate::sys::mmap_anon(span);
    if base <= 0 {
        unsafe {
            release_cached();
            let base = crate::sys::mmap_anon(span);
            if base > 0 {
                ptr::write(base as *mut usize, span);
                MAPPED_BYTES += span;
                return (base as usize + header) as *mut u8;
            }
            let pointer = medium_alloc(layout);
            if zeroed && !pointer.is_null() {
                ptr::write_bytes(pointer, 0, layout.size());
            }
            return pointer;
        }
    }
    unsafe {
        ptr::write(base as *mut usize, span);
        MAPPED_BYTES += span;
    }
    (base as usize + header) as *mut u8
}

unsafe fn large_free(layout: Layout, pointer: *mut u8) {
    let (header, _) = large_span(&layout);
    let base = pointer as usize - header;
    if base < 0x0000_00A0_0000_0000 {
        unsafe { medium_free(pointer) };
        return;
    }
    unsafe {
        let span = ptr::read(base as *const usize);
        MAPPED_BYTES -= span;
        if !cache_put(base, span) {
            crate::sys::munmap(base as u64, span as u64);
        }
    }
}

pub fn mapped_bytes() -> usize {
    unsafe { MAPPED_BYTES }
}

pub struct HeapAllocator;

unsafe impl GlobalAlloc for HeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if is_large(&layout) {
                return large_alloc(layout, false);
            }
            match class_of(&layout) {
                Some(class) => small_alloc(class),
                None => medium_alloc(layout),
            }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if is_large(&layout) {
                return large_alloc(layout, true);
            }
            let pointer = self.alloc(layout);
            if !pointer.is_null() {
                ptr::write_bytes(pointer, 0, layout.size());
            }
            pointer
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if pointer.is_null() {
            return;
        }
        unsafe {
            if is_large(&layout) {
                return large_free(layout, pointer);
            }
            match class_of(&layout) {
                Some(class) => small_free(class, pointer),
                None => medium_free(pointer),
            }
        }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            if is_large(&layout) && is_large(&new_layout) {
                let (header, span) = large_span(&layout);
                let base = pointer as usize - header;
                if base >= 0x0000_00A0_0000_0000 && header + new_size <= span {
                    return pointer;
                }
            } else if !is_large(&layout) && !is_large(&new_layout) {
                if let (Some(a), Some(b)) = (class_of(&layout), class_of(&new_layout)) {
                    if a == b {
                        return pointer;
                    }
                }
            }
            let fresh = self.alloc(new_layout);
            if !fresh.is_null() {
                ptr::copy_nonoverlapping(pointer, fresh, layout.size().min(new_size));
                self.dealloc(pointer, layout);
            }
            fresh
        }
    }
}

#[global_allocator]
static ALLOCATOR: HeapAllocator = HeapAllocator;

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    crate::eprintln!("hamix_std: allocation failed: {:?}", layout);
    crate::sys::exit(101)
}
