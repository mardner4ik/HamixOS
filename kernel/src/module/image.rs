use crate::memory::frame::{self, PAGE_SIZE};

#[cfg(target_arch = "x86_64")]
const MODULE_LIMIT: usize = 1usize << 31;
#[cfg(not(target_arch = "x86_64"))]
const MODULE_LIMIT: usize = 1usize << 32;

pub struct Image {
    base: usize,
    len: usize,
    pub text_bytes: usize,
    pub data_bytes: usize,
    protected: bool,
}

unsafe impl Send for Image {}
unsafe impl Sync for Image {}

impl Image {
    pub fn new(len: usize) -> Option<Image> {
        let pages = len.div_ceil(PAGE_SIZE).max(1);
        let base = frame::alloc_contiguous(pages, MODULE_LIMIT)?;
        let len = pages * PAGE_SIZE;
        unsafe { core::ptr::write_bytes(base as *mut u8, 0, len) };
        Some(Image { base, len, text_bytes: 0, data_bytes: 0, protected: false })
    }

    pub fn base(&self) -> u64 {
        self.base as u64
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.base as *mut u8, self.len) }
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base() && addr < self.base() + self.len as u64
    }

    pub fn set_split(&mut self, text: usize, data: usize) {
        self.text_bytes = text;
        self.data_bytes = data;
    }

    pub fn protect(&mut self) -> bool {
        if !crate::arch::paging::no_execute() || self.text_bytes == 0 {
            return false;
        }
        let text = (self.text_bytes as u64 + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
        let text = text.min(self.len as u64);
        if !crate::arch::paging::protect_kernel_range(self.base as u64, text, false, true) {
            return false;
        }
        if text < self.len as u64 {
            crate::arch::paging::protect_kernel_range(self.base as u64 + text, self.len as u64 - text, true, false);
        }
        self.protected = true;
        true
    }

    pub fn unprotect(&mut self) {
        if self.protected {
            crate::arch::paging::protect_kernel_range(self.base as u64, self.len as u64, true, true);
            self.protected = false;
        }
    }

    pub fn protected(&self) -> bool {
        self.protected
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        self.unprotect();
        let mut page = 0;
        while page < self.len {
            frame::free_frame(self.base + page);
            page += PAGE_SIZE;
        }
    }
}
