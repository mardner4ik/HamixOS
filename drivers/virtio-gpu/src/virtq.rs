use core::sync::atomic::{fence, Ordering};

use hamix_kpi as kpi;

pub const DESC_NEXT: u16 = 1;
pub const DESC_WRITE: u16 = 2;

pub struct Ring {
    pub size: u16,
    desc: *mut u8,
    avail: *mut u8,
    used: *mut u8,
    pub desc_phys: u64,
    pub avail_phys: u64,
    pub used_phys: u64,
    notify: *mut u16,
    index: u16,
    avail_idx: u16,
    last_used: u16,
    backing: *mut u8,
    backing_len: usize,
}

fn align_up(value: usize, to: usize) -> usize {
    value.div_ceil(to) * to
}

impl Ring {
    pub fn new(index: u16, size: u16) -> Option<Ring> {
        let n = size as usize;
        let desc_len = 16 * n;
        let avail_len = 6 + 2 * n;
        let used_off = align_up(desc_len + avail_len, 4);
        let used_len = 6 + 8 * n;
        let total = used_off + used_len;
        let mut phys = 0u64;
        let backing = unsafe { kpi::hamix_dma_alloc(total, &mut phys) };
        if backing.is_null() {
            return None;
        }
        Some(Ring {
            size,
            desc: backing,
            avail: unsafe { backing.add(desc_len) },
            used: unsafe { backing.add(used_off) },
            desc_phys: phys,
            avail_phys: phys + desc_len as u64,
            used_phys: phys + used_off as u64,
            notify: core::ptr::null_mut(),
            index,
            avail_idx: 0,
            last_used: 0,
            backing,
            backing_len: total,
        })
    }

    pub fn set_notify(&mut self, addr: *mut u16) {
        self.notify = addr;
    }

    fn write_desc(&mut self, slot: u16, addr: u64, len: u32, flags: u16, next: u16) {
        let at = unsafe { self.desc.add(slot as usize * 16) };
        unsafe {
            core::ptr::write_volatile(at as *mut u64, addr);
            core::ptr::write_volatile(at.add(8) as *mut u32, len);
            core::ptr::write_volatile(at.add(12) as *mut u16, flags);
            core::ptr::write_volatile(at.add(14) as *mut u16, next);
        }
    }

    fn used_idx(&self) -> u16 {
        unsafe { core::ptr::read_volatile(self.used.add(2) as *const u16) }
    }

    fn used_len(&self, slot: u16) -> u32 {
        let at = unsafe { self.used.add(4 + slot as usize * 8 + 4) };
        unsafe { core::ptr::read_volatile(at as *const u32) }
    }

    pub fn submit(&mut self, parts: &[(u64, u32, bool)]) -> Option<u32> {
        if parts.is_empty() || parts.len() > self.size as usize {
            return None;
        }
        for (i, (addr, len, writable)) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            let mut flags = 0u16;
            if !last {
                flags |= DESC_NEXT;
            }
            if *writable {
                flags |= DESC_WRITE;
            }
            self.write_desc(i as u16, *addr, *len, flags, if last { 0 } else { i as u16 + 1 });
        }
        let slot = self.avail_idx % self.size;
        unsafe { core::ptr::write_volatile(self.avail.add(4 + slot as usize * 2) as *mut u16, 0) };
        fence(Ordering::SeqCst);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        unsafe { core::ptr::write_volatile(self.avail.add(2) as *mut u16, self.avail_idx) };
        fence(Ordering::SeqCst);
        if !self.notify.is_null() {
            unsafe { core::ptr::write_volatile(self.notify, self.index) };
        }
        Some(0)
    }

    pub fn wait(&mut self, timeout_us: u64) -> Option<u32> {
        let mut waited = 0u64;
        while self.used_idx() == self.last_used {
            if waited >= timeout_us {
                return None;
            }
            unsafe { kpi::hamix_udelay(10) };
            waited += 10;
        }
        fence(Ordering::SeqCst);
        let slot = self.last_used % self.size;
        let len = self.used_len(slot);
        self.last_used = self.last_used.wrapping_add(1);
        Some(len)
    }

    pub fn drain(&mut self) {
        while self.used_idx() != self.last_used {
            self.last_used = self.last_used.wrapping_add(1);
        }
    }

    pub fn release(&mut self) {
        if !self.backing.is_null() {
            unsafe { kpi::hamix_dma_free(self.backing, self.backing_len) };
            self.backing = core::ptr::null_mut();
        }
    }
}
