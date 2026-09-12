use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

const MIN_ALIGN: usize = 16;
const SPLIT_THRESHOLD: usize = 64;
const GROW_SLACK: usize = 64 * 1024;

#[repr(C)]
struct Block {
    size: usize,
    next: *mut Block,
}

static mut FREE_LIST: *mut Block = ptr::null_mut();

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

unsafe fn block_size(block: *mut Block) -> usize {
    unsafe { (*block).size }
}

unsafe fn insert_free(block: *mut Block) {
    unsafe {
        let mut cursor = &raw mut FREE_LIST;
        while !(*cursor).is_null() && (*cursor) < block {
            cursor = &raw mut (*(*cursor)).next;
        }
        (*block).next = *cursor;
        *cursor = block;

        let next = (*block).next;
        if !next.is_null() && (block as usize) + (*block).size == next as usize {
            (*block).size += (*next).size;
            (*block).next = (*next).next;
        }

        let mut previous = &raw mut FREE_LIST;
        while !(*previous).is_null() && (*previous) != block {
            let candidate = *previous;
            if (candidate as usize) + (*candidate).size == block as usize {
                (*candidate).size += (*block).size;
                (*candidate).next = (*block).next;
                return;
            }
            previous = &raw mut (*candidate).next;
        }
    }
}

unsafe fn take_free(request: usize) -> *mut Block {
    unsafe {
        let mut cursor = &raw mut FREE_LIST;
        while !(*cursor).is_null() {
            let candidate = *cursor;
            let size = block_size(candidate);
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

unsafe fn grow(request: usize) -> *mut Block {
    let current = crate::sys::brk(0);
    if current < 0 {
        return ptr::null_mut();
    }
    let start = align_up(current as usize, MIN_ALIGN);
    let total = request + GROW_SLACK;
    let target = start + total;
    if crate::sys::brk(target) < target as i64 {
        let target = start + request;
        if crate::sys::brk(target) < target as i64 {
            return ptr::null_mut();
        }
        let block = start as *mut Block;
        unsafe {
            (*block).size = request;
            (*block).next = ptr::null_mut();
        }
        return block;
    }

    let block = start as *mut Block;
    unsafe {
        (*block).size = request;
        (*block).next = ptr::null_mut();
        let spare = (start + request) as *mut Block;
        (*spare).size = GROW_SLACK;
        (*spare).next = ptr::null_mut();
        insert_free(spare);
    }
    block
}

pub struct HeapAllocator;

unsafe impl GlobalAlloc for HeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
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

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        if pointer.is_null() {
            return;
        }
        unsafe {
            let offset = ptr::read((pointer as usize - 8) as *const usize);
            let block = (pointer as usize - offset) as *mut Block;
            insert_free(block);
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
