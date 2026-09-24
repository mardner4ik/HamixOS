use crate::memory::frame::{self, PAGE_SIZE};

pub struct DmaRegion {
    pub phys: u64,
    pub len: usize,
}

impl DmaRegion {
    pub fn new(len: usize) -> Option<DmaRegion> {
        let pages = len.div_ceil(PAGE_SIZE).max(1);
        let base = frame::alloc_contiguous(pages, 1usize << 32)?;
        unsafe { core::ptr::write_bytes(base as *mut u8, 0, pages * PAGE_SIZE) };
        Some(DmaRegion { phys: base as u64, len: pages * PAGE_SIZE })
    }

    pub fn ptr<T>(&self, offset: usize) -> *mut T {
        (self.phys as usize + offset) as *mut T
    }

    pub fn slice(&self, offset: usize, len: usize) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr::<u8>(offset), len.min(self.len - offset)) }
    }
}

impl Drop for DmaRegion {
    fn drop(&mut self) {
        let mut page = 0;
        while page < self.len {
            frame::free_frame(self.phys as usize + page);
            page += PAGE_SIZE;
        }
    }
}

pub struct Mmio {
    pub base: u64,
}

impl Mmio {
    #[inline]
    pub fn read32(&self, reg: u32) -> u32 {
        unsafe { core::ptr::read_volatile((self.base + reg as u64) as *const u32) }
    }

    #[inline]
    pub fn write32(&self, reg: u32, value: u32) {
        unsafe { core::ptr::write_volatile((self.base + reg as u64) as *mut u32, value) }
    }

    #[inline]
    pub fn read16(&self, reg: u32) -> u16 {
        unsafe { core::ptr::read_volatile((self.base + reg as u64) as *const u16) }
    }

    #[inline]
    pub fn write16(&self, reg: u32, value: u16) {
        unsafe { core::ptr::write_volatile((self.base + reg as u64) as *mut u16, value) }
    }

    #[inline]
    pub fn read8(&self, reg: u32) -> u8 {
        unsafe { core::ptr::read_volatile((self.base + reg as u64) as *const u8) }
    }

    #[inline]
    pub fn write8(&self, reg: u32, value: u8) {
        unsafe { core::ptr::write_volatile((self.base + reg as u64) as *mut u8, value) }
    }
}

pub fn mac_text(mac: &[u8; 6]) -> alloc::string::String {
    alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5])
}
