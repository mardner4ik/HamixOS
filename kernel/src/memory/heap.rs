use core::alloc::{GlobalAlloc, Layout};
use core::mem::{align_of, size_of};
use core::ptr::NonNull;
use spin::Mutex;

use crate::arch::x86_64::without_interrupts;

const HEAP_SIZE: usize = 4 * 1024 * 1024;
const HEADER_ALIGN: usize = align_of::<usize>();

#[repr(align(8))]
struct AlignedHeap([u8; HEAP_SIZE]);

static mut HEAP_STORAGE: AlignedHeap = AlignedHeap([0u8; HEAP_SIZE]);

struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

struct AllocHeader {
    start: usize,
    size: usize,
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

pub struct BlockAllocator {
    head: Option<NonNull<FreeBlock>>,
}

unsafe impl Send for BlockAllocator {}

impl BlockAllocator {
    const fn new() -> Self {
        Self { head: None }
    }

    unsafe fn init(&mut self, start: *mut u8, size: usize) {
        unsafe {
            let aligned_start = align_up(start as usize, HEADER_ALIGN);
            let trim = aligned_start - start as usize;
            if size > trim {
                self.insert_free(aligned_start, size - trim);
            }
        }
    }

    unsafe fn insert_free(&mut self, addr: usize, size: usize) {
        if size < size_of::<FreeBlock>() {
            return;
        }

        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut cur = self.head;

        while let Some(node) = cur {
            if node.as_ptr() as usize >= addr {
                break;
            }
            prev = cur;
            cur = unsafe { (*node.as_ptr()).next };
        }

        let mut final_addr = addr;
        let mut final_size = size;
        let mut final_next = cur;

        if let Some(next) = cur {
            let next_addr = next.as_ptr() as usize;
            if final_addr + final_size == next_addr {
                final_size += unsafe { (*next.as_ptr()).size };
                final_next = unsafe { (*next.as_ptr()).next };
            }
        }

        if let Some(p) = prev {
            let p_addr = p.as_ptr() as usize;
            let p_size = unsafe { (*p.as_ptr()).size };
            if p_addr + p_size == final_addr {
                unsafe {
                    (*p.as_ptr()).size = p_size + final_size;
                    (*p.as_ptr()).next = final_next;
                }
                return;
            }
        }

        let block = final_addr as *mut FreeBlock;
        unsafe {
            (*block).size = final_size;
            (*block).next = final_next;
        }
        let new_node = NonNull::new(block);
        if let Some(p) = prev {
            unsafe { (*p.as_ptr()).next = new_node; }
        } else {
            self.head = new_node;
        }
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let header_size = size_of::<AllocHeader>();
        let align = layout.align().max(HEADER_ALIGN);
        let payload = layout.size();

        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut cur = self.head;

        while let Some(node) = cur {
            let block_addr = node.as_ptr() as usize;
            let block_size = unsafe { (*node.as_ptr()).size };
            let block_end = block_addr + block_size;

            let user_ptr = align_up(block_addr + header_size, align);
            let consumed_end = user_ptr + payload;

            if consumed_end <= block_end {
                let next = unsafe { (*node.as_ptr()).next };
                if let Some(p) = prev {
                    unsafe { (*p.as_ptr()).next = next; }
                } else {
                    self.head = next;
                }

                let leftover_start = consumed_end;
                let leftover_size = block_end - consumed_end;
                let header_recorded_size = if leftover_size >= size_of::<FreeBlock>() {
                    unsafe { self.insert_free(leftover_start, leftover_size); }
                    leftover_start - block_addr
                } else {
                    block_size
                };

                let header = (user_ptr - header_size) as *mut AllocHeader;
                unsafe {
                    (*header).start = block_addr;
                    (*header).size = header_recorded_size;
                }

                return user_ptr as *mut u8;
            }

            prev = cur;
            cur = unsafe { (*node.as_ptr()).next };
        }

        core::ptr::null_mut()
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let header_size = size_of::<AllocHeader>();
        let header = (ptr as usize - header_size) as *mut AllocHeader;
        let (start, size) = unsafe { ((*header).start, (*header).size) };
        unsafe {
            self.insert_free(start, size);
        }
    }
}

pub struct LockedAllocator(Mutex<BlockAllocator>);

impl LockedAllocator {
    const fn new() -> Self {
        Self(Mutex::new(BlockAllocator::new()))
    }
}

unsafe impl GlobalAlloc for LockedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        without_interrupts(|| unsafe { self.0.lock().alloc(layout) })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        without_interrupts(|| unsafe { self.0.lock().dealloc(ptr) });
    }
}

#[global_allocator]
pub static ALLOCATOR: LockedAllocator = LockedAllocator::new();

pub fn init() {
    unsafe {
        ALLOCATOR
            .0
            .lock()
            .init((&raw mut HEAP_STORAGE) as *mut u8, HEAP_SIZE);
    }
}
