use core::alloc::{GlobalAlloc, Layout};

pub struct KernelHeap;

const NATURAL_ALIGN: usize = 16;

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= NATURAL_ALIGN {
            return unsafe { crate::hamix_kmalloc(layout.size().max(1)) };
        }
        let total = layout.size() + layout.align() + core::mem::size_of::<usize>();
        let raw = unsafe { crate::hamix_kmalloc(total) };
        if raw.is_null() {
            return raw;
        }
        let start = raw as usize + core::mem::size_of::<usize>();
        let aligned = (start + layout.align() - 1) & !(layout.align() - 1);
        unsafe { ((aligned - core::mem::size_of::<usize>()) as *mut usize).write(raw as usize) };
        aligned as *mut u8
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= NATURAL_ALIGN {
            return unsafe { crate::hamix_kzalloc(layout.size().max(1)) };
        }
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { core::ptr::write_bytes(ptr, 0, layout.size()) };
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        if layout.align() <= NATURAL_ALIGN {
            unsafe { crate::hamix_kfree(ptr) };
            return;
        }
        let raw = unsafe { ((ptr as usize - core::mem::size_of::<usize>()) as *const usize).read() };
        unsafe { crate::hamix_kfree(raw as *mut u8) };
    }
}

#[macro_export]
macro_rules! kernel_heap {
    () => {
        #[global_allocator]
        static __MODULE_HEAP: $crate::heap::KernelHeap = $crate::heap::KernelHeap;
    };
}
