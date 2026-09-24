pub struct DmaRegion {
    virt: *mut u8,
    pub phys: u64,
    pub len: usize,
}

unsafe impl Send for DmaRegion {}

impl DmaRegion {
    pub fn new(len: usize) -> Option<DmaRegion> {
        let len = len.div_ceil(4096).max(1) * 4096;
        let mut phys = 0u64;
        let virt = unsafe { crate::hamix_dma_alloc(len, &mut phys) };
        if virt.is_null() || phys == 0 || phys + len as u64 > 1u64 << 32 {
            if !virt.is_null() {
                unsafe { crate::hamix_dma_free(virt, len) };
            }
            return None;
        }
        Some(DmaRegion { virt, phys, len })
    }

    #[inline(always)]
    pub fn ptr<T>(&self, offset: usize) -> *mut T {
        unsafe { self.virt.add(offset.min(self.len)) as *mut T }
    }

    #[inline(always)]
    pub fn slice(&self, offset: usize, len: usize) -> &mut [u8] {
        let offset = offset.min(self.len);
        unsafe { core::slice::from_raw_parts_mut(self.virt.add(offset), len.min(self.len - offset)) }
    }
}

impl Drop for DmaRegion {
    fn drop(&mut self) {
        unsafe { crate::hamix_dma_free(self.virt, self.len) };
    }
}

pub struct Mmio {
    base: *mut u8,
    len: u64,
}

unsafe impl Send for Mmio {}

impl Mmio {
    pub fn map(phys: u64, len: u64) -> Option<Mmio> {
        if phys == 0 || len == 0 {
            return None;
        }
        let base = unsafe { crate::hamix_ioremap(phys, len as usize) };
        if base.is_null() { None } else { Some(Mmio { base, len }) }
    }

    #[inline(always)]
    pub fn read32(&self, reg: u32) -> u32 {
        if reg as u64 + 4 > self.len {
            return 0xFFFF_FFFF;
        }
        unsafe { crate::hamix_readl(self.base.add(reg as usize) as *const u32) }
    }

    #[inline(always)]
    pub fn write32(&self, reg: u32, value: u32) {
        if reg as u64 + 4 > self.len {
            return;
        }
        unsafe { crate::hamix_writel(value, self.base.add(reg as usize) as *mut u32) }
    }

    #[inline(always)]
    pub fn read16(&self, reg: u32) -> u16 {
        if reg as u64 + 2 > self.len {
            return 0xFFFF;
        }
        unsafe { crate::hamix_readw(self.base.add(reg as usize) as *const u16) }
    }

    #[inline(always)]
    pub fn write16(&self, reg: u32, value: u16) {
        if reg as u64 + 2 > self.len {
            return;
        }
        unsafe { crate::hamix_writew(value, self.base.add(reg as usize) as *mut u16) }
    }

    #[inline(always)]
    pub fn read8(&self, reg: u32) -> u8 {
        if reg as u64 + 1 > self.len {
            return 0xFF;
        }
        unsafe { crate::hamix_readb(self.base.add(reg as usize)) }
    }

    #[inline(always)]
    pub fn write8(&self, reg: u32, value: u8) {
        if reg as u64 + 1 > self.len {
            return;
        }
        unsafe { crate::hamix_writeb(value, self.base.add(reg as usize)) }
    }
}

#[inline(always)]
pub fn delay_us(us: u64) {
    unsafe { crate::hamix_udelay(us) }
}

#[inline(always)]
pub fn uptime_ms() -> u64 {
    unsafe { crate::hamix_uptime_ms() }
}
