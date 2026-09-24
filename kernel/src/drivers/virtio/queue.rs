use alloc::vec::Vec;
use core::sync::atomic::{fence, Ordering};

use super::Transport;
use crate::net::dma::DmaRegion;

const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;
const ALIGN: usize = 4096;

pub struct Queue {
    pub index: u16,
    pub size: u16,
    region: DmaRegion,
    avail_off: usize,
    used_off: usize,
    free: Vec<u16>,
    avail_idx: u16,
    last_used: u16,
}

unsafe impl Send for Queue {}

impl Queue {
    pub fn new(transport: &mut dyn Transport, index: u16, wanted: u16) -> Option<Queue> {
        let max = transport.queue_max(index);
        if max == 0 {
            return None;
        }
        let size = wanted.min(max).max(1);
        let n = size as usize;
        let avail_off = 16 * n;
        let used_off = (avail_off + 6 + 2 * n).div_ceil(ALIGN) * ALIGN;
        let total = used_off + 6 + 8 * n;
        let region = DmaRegion::new(total)?;
        let phys = region.phys;
        if !transport.setup_queue(index, size, phys, phys + avail_off as u64, phys + used_off as u64) {
            return None;
        }
        Some(Queue { index, size, region, avail_off, used_off, free: (0..size).rev().collect(), avail_idx: 0, last_used: 0 })
    }

    fn desc(&self, slot: u16) -> *mut u8 {
        self.region.ptr::<u8>(slot as usize * 16)
    }

    pub fn address_of(&self, slot: u16) -> u64 {
        unsafe { core::ptr::read_volatile(self.desc(slot) as *const u64) }
    }

    pub fn available(&self) -> usize {
        self.free.len()
    }

    pub fn push(&mut self, parts: &[(u64, u32, bool)]) -> Option<u16> {
        if parts.is_empty() || parts.len() > self.free.len() {
            return None;
        }
        let slots: Vec<u16> = (0..parts.len()).map(|_| self.free.pop().unwrap()).collect();
        for (i, (addr, len, writable)) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            let mut flags = 0u16;
            if !last {
                flags |= DESC_NEXT;
            }
            if *writable {
                flags |= DESC_WRITE;
            }
            let at = self.desc(slots[i]);
            unsafe {
                core::ptr::write_volatile(at as *mut u64, *addr);
                core::ptr::write_volatile(at.add(8) as *mut u32, *len);
                core::ptr::write_volatile(at.add(12) as *mut u16, flags);
                core::ptr::write_volatile(at.add(14) as *mut u16, if last { 0 } else { slots[i + 1] });
            }
        }
        let head = slots[0];
        let ring_slot = self.avail_idx % self.size;
        unsafe { core::ptr::write_volatile(self.region.ptr::<u16>(self.avail_off + 4 + ring_slot as usize * 2), head) };
        fence(Ordering::SeqCst);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        unsafe { core::ptr::write_volatile(self.region.ptr::<u16>(self.avail_off + 2), self.avail_idx) };
        fence(Ordering::SeqCst);
        Some(head)
    }

    pub fn kick(&self, transport: &dyn Transport) {
        transport.notify(self.index);
    }

    fn used_idx(&self) -> u16 {
        unsafe { core::ptr::read_volatile(self.region.ptr::<u16>(self.used_off + 2)) }
    }

    pub fn has_used(&self) -> bool {
        self.used_idx() != self.last_used
    }

    pub fn pop(&mut self) -> Option<(u16, u32)> {
        if !self.has_used() {
            return None;
        }
        fence(Ordering::SeqCst);
        let slot = (self.last_used % self.size) as usize;
        let id = unsafe { core::ptr::read_volatile(self.region.ptr::<u32>(self.used_off + 4 + slot * 8)) } as u16;
        let len = unsafe { core::ptr::read_volatile(self.region.ptr::<u32>(self.used_off + 4 + slot * 8 + 4)) };
        self.last_used = self.last_used.wrapping_add(1);
        let mut at = id;
        loop {
            self.free.push(at);
            let desc = self.desc(at);
            let flags = unsafe { core::ptr::read_volatile(desc.add(12) as *const u16) };
            if flags & DESC_NEXT == 0 {
                break;
            }
            at = unsafe { core::ptr::read_volatile(desc.add(14) as *const u16) };
            if self.free.len() > self.size as usize {
                break;
            }
        }
        Some((id, len))
    }

    pub fn wait(&mut self, transport: &dyn Transport, timeout_ms: u64) -> Option<(u16, u32)> {
        let start = crate::task::uptime_ms();
        let mut spins = 0u64;
        loop {
            if let Some(done) = self.pop() {
                return Some(done);
            }
            spins += 1;
            if crate::task::uptime_ms().saturating_sub(start) > timeout_ms || spins > 400_000_000 {
                return None;
            }
            if spins % 4096 == 0 {
                transport.notify(self.index);
            }
            core::hint::spin_loop();
        }
    }
}
