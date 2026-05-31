use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

const HEAP_SIZE: usize = 4 * 1024 * 1024;
static mut HEAP_STORAGE: [u8; HEAP_SIZE] = [0u8; HEAP_SIZE];

struct Node {
    size: usize,
    next: Option<*mut Node>,
}

pub struct LinkedListAllocator {
    head: Option<*mut Node>,
}

unsafe impl Send for LinkedListAllocator {}

impl LinkedListAllocator {
    const fn new() -> Self {
        Self { head: None }
    }

    unsafe fn init(&mut self, start: *mut u8, size: usize) {
        let node = start as *mut Node;
        (*node).size = size - core::mem::size_of::<Node>();
        (*node).next = None;
        self.head = Some(node);
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(core::mem::align_of::<Node>());
        let size = layout.size().max(core::mem::size_of::<Node>());

        let mut prev: Option<*mut Node> = None;
        let mut current = self.head;

        while let Some(node) = current {
            let node_end = node as usize + core::mem::size_of::<Node>();
            let alloc_start = (node_end + align - 1) & !(align - 1);
            let alloc_end = alloc_start + size;
            let node_region_end = node as usize + core::mem::size_of::<Node>() + (*node).size;

            if alloc_end <= node_region_end {
                let next = (*node).next;
                if let Some(prev_node) = prev {
                    (*prev_node).next = next;
                } else {
                    self.head = next;
                }

                let leftover = node_region_end - alloc_end;
                if leftover >= core::mem::size_of::<Node>() + 8 {
                    let new_node = alloc_end as *mut Node;
                    (*new_node).size = leftover - core::mem::size_of::<Node>();
                    (*new_node).next = next;
                    if let Some(prev_node) = prev {
                        (*prev_node).next = Some(new_node);
                    } else {
                        self.head = Some(new_node);
                    }
                }

                return alloc_start as *mut u8;
            }

            prev = current;
            current = (*node).next;
        }

        core::ptr::null_mut()
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(core::mem::size_of::<Node>());
        let node = ptr as *mut Node;
        (*node).size = size;
        (*node).next = self.head;
        self.head = Some(node);
    }
}

pub struct LockedAllocator(Mutex<LinkedListAllocator>);

impl LockedAllocator {
    const fn new() -> Self {
        Self(Mutex::new(LinkedListAllocator::new()))
    }
}

unsafe impl GlobalAlloc for LockedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().dealloc(ptr, layout);
    }
}

#[global_allocator]
pub static ALLOCATOR: LockedAllocator = LockedAllocator::new();

pub fn init() {
    unsafe {
        ALLOCATOR
            .0
            .lock()
            .init(HEAP_STORAGE.as_mut_ptr(), HEAP_SIZE);
    }
}
