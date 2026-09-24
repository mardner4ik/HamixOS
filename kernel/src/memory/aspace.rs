use alloc::vec::Vec;

use crate::arch::pte;
use crate::memory::frame;

pub const PAGE_SIZE: u64 = 4096;
const ENTRIES: usize = 512;

pub use crate::arch::pte::{USER_BASE, USER_END};

pub const ANON_NODE: usize = usize::MAX;

#[derive(Clone, Copy)]
pub struct LazyRegion {
    pub node: usize,
    pub start: u64,
    pub offset: u64,
    pub file_len: u64,
}

pub fn is_user_range(addr: u64, len: u64) -> bool {
    addr >= USER_BASE && len <= USER_END - USER_BASE && addr.checked_add(len).map(|end| end <= USER_END).unwrap_or(false)
}

pub struct AddressSpace {
    pub root: u64,
    pub lazy: Vec<LazyRegion>,
}

fn user_top(index: usize) -> bool {
    pte::USER_TOP.contains(&index)
}

impl AddressSpace {
    pub fn new() -> Option<Self> {
        let root = frame::alloc_zeroed_frame()? as u64;
        unsafe {
            let kernel = pte::kernel_root() as *const u64;
            let target = root as *mut u64;
            for i in 0..ENTRIES {
                if !user_top(i) {
                    *target.add(i) = *kernel.add(i);
                }
            }
        }
        pte::barrier();
        Some(Self { root, lazy: Vec::new() })
    }

    unsafe fn entry(&self, virt: u64, create: bool) -> Option<*mut u64> {
        let mut table = self.root as *mut u64;
        for level in 0..pte::LEVELS - 1 {
            unsafe {
                let slot = table.add(pte::index(virt, level));
                if !pte::valid(*slot) {
                    if !create {
                        return None;
                    }
                    let fresh = frame::alloc_zeroed_frame()? as u64;
                    *slot = pte::table(fresh);
                    pte::barrier();
                }
                if !pte::is_table(*slot, level) {
                    return None;
                }
                table = pte::addr(*slot) as *mut u64;
            }
        }
        Some(unsafe { table.add(pte::index(virt, pte::LEVELS - 1)) })
    }

    pub fn activate(&self) {
        pte::load_root(self.root);
    }

    pub fn translate(&self, virt: u64) -> Option<u64> {
        if !is_user_range(virt, 1) {
            return None;
        }
        let e = unsafe { *self.entry(virt, false)? };
        if !pte::valid(e) {
            return None;
        }
        Some(pte::addr(e) + (virt & 0xFFF))
    }

    pub fn is_mapped(&self, addr: u64, len: u64) -> bool {
        if !is_user_range(addr, len.max(1)) {
            return false;
        }
        let end = addr + len.max(1);
        let mut page = addr & !(PAGE_SIZE - 1);
        while page < end {
            let Some(first) = (unsafe { self.entry(page, false) }) else {
                return false;
            };
            let mut slot = pte::index(page, pte::LEVELS - 1);
            let mut at = first;
            while slot < ENTRIES && page < end {
                if !pte::valid(unsafe { *at }) {
                    return false;
                }
                at = unsafe { at.add(1) };
                slot += 1;
                page += PAGE_SIZE;
            }
        }
        true
    }

    pub fn map_lazy(&mut self, start: u64, len: u64, region: LazyRegion) -> bool {
        let index = match self.lazy.iter().position(|r| r.node == region.node && r.start == region.start && r.offset == region.offset && r.file_len == region.file_len) {
            Some(i) => i,
            None => {
                self.lazy.push(region);
                self.lazy.len() - 1
            }
        };
        let mut page = start & !(PAGE_SIZE - 1);
        let end = (start + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        if !is_user_range(page, end - page) {
            return false;
        }
        while page < end {
            unsafe {
                let Some(slot) = self.entry(page, true) else {
                    return false;
                };
                if pte::valid(*slot) && pte::owned(*slot) {
                    frame::free_frame(pte::addr(*slot) as usize);
                }
                *slot = pte::lazy(index);
            }
            page += PAGE_SIZE;
        }
        true
    }

    pub fn map_zero(&mut self, start: u64, len: u64) -> bool {
        self.map_lazy(start, len, LazyRegion { node: ANON_NODE, start: 0, offset: 0, file_len: 0 })
    }

    pub fn occupied(&self, virt: u64) -> bool {
        if !is_user_range(virt, 1) {
            return false;
        }
        unsafe { self.entry(virt, false).map(|e| pte::valid(*e) || pte::is_lazy(*e)).unwrap_or(false) }
    }

    pub fn lazy_at(&self, virt: u64) -> Option<(usize, LazyRegion)> {
        if !is_user_range(virt, 1) {
            return None;
        }
        let e = unsafe { *self.entry(virt, false)? };
        if pte::valid(e) || !pte::is_lazy(e) {
            return None;
        }
        let index = pte::lazy_index(e);
        self.lazy.get(index).map(|r| (index, *r))
    }

    pub fn fill_lazy(&mut self, virt: u64, index: usize, data: &[u8]) -> bool {
        unsafe {
            let Some(slot) = self.entry(virt, false) else {
                return false;
            };
            if pte::valid(*slot) || !pte::is_lazy(*slot) || pte::lazy_index(*slot) != index {
                return false;
            }
            let Some(phys) = frame::alloc_zeroed_frame() else {
                return false;
            };
            let n = data.len().min(PAGE_SIZE as usize);
            core::ptr::copy_nonoverlapping(data.as_ptr(), phys as *mut u8, n);
            pte::sync_code(phys as u64, PAGE_SIZE);
            *slot = pte::user_page(phys as u64, true, false);
        }
        pte::barrier();
        true
    }

    pub fn alloc_range(&mut self, start: u64, len: u64) -> bool {
        let mut page = start & !(PAGE_SIZE - 1);
        let end = (start + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        if !is_user_range(page, end - page) {
            return false;
        }
        while page < end {
            unsafe {
                let Some(slot) = self.entry(page, true) else {
                    return false;
                };
                if !pte::valid(*slot) {
                    let Some(phys) = frame::alloc_zeroed_frame() else {
                        return false;
                    };
                    *slot = pte::user_page(phys as u64, true, false);
                }
            }
            page += PAGE_SIZE;
        }
        pte::barrier();
        true
    }

    pub fn map_shared(&mut self, virt: u64, phys: u64, write_combining: bool) -> bool {
        if !is_user_range(virt, PAGE_SIZE) {
            return false;
        }
        unsafe {
            let Some(slot) = self.entry(virt, true) else {
                return false;
            };
            let replaced = pte::valid(*slot);
            if replaced && pte::owned(*slot) {
                frame::free_frame(pte::addr(*slot) as usize);
            }
            *slot = pte::user_page(phys, false, write_combining);
            if replaced {
                pte::flush_page(virt);
            }
        }
        pte::barrier();
        true
    }

    pub fn unmap_range(&mut self, start: u64, len: u64) {
        let mut page = start & !(PAGE_SIZE - 1);
        let end = (start + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        while page < end {
            unsafe {
                if let Some(slot) = self.entry(page, false) {
                    if pte::valid(*slot) {
                        if pte::owned(*slot) {
                            frame::free_frame(pte::addr(*slot) as usize);
                        }
                        *slot = 0;
                    } else if pte::is_lazy(*slot) {
                        *slot = 0;
                    }
                }
            }
            page += PAGE_SIZE;
        }
        self.release_tables(start, len);
    }

    fn release_tables(&mut self, start: u64, len: u64) {
        let span = PAGE_SIZE * ENTRIES as u64;
        let first = start & !(span - 1);
        let end = (start + len + span - 1) & !(span - 1);
        let mut region = first;
        while region < end {
            unsafe {
                let mut table = self.root as *mut u64;
                let mut slot: *mut u64 = core::ptr::null_mut();
                let mut ok = true;
                for level in 0..pte::LEVELS - 1 {
                    slot = table.add(pte::index(region, level));
                    if !pte::valid(*slot) || !pte::is_table(*slot, level) {
                        ok = false;
                        break;
                    }
                    if level < pte::LEVELS - 2 {
                        table = pte::addr(*slot) as *mut u64;
                    }
                }
                if ok && !slot.is_null() {
                    let pt = pte::addr(*slot) as *const u64;
                    if (0..ENTRIES).all(|i| *pt.add(i) == 0) {
                        *slot = 0;
                        pte::flush_all();
                        frame::free_frame(pt as usize);
                    }
                }
            }
            region += span;
        }
    }

    pub fn write_bytes(&self, virt: u64, data: &[u8]) -> bool {
        let mut done = 0usize;
        while done < data.len() {
            let addr = virt + done as u64;
            let Some(phys) = self.translate(addr) else {
                return false;
            };
            let room = (PAGE_SIZE - (addr & 0xFFF)) as usize;
            let n = room.min(data.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(data.as_ptr().add(done), phys as *mut u8, n) };
            pte::sync_code(phys, n as u64);
            done += n;
        }
        true
    }

    fn copy_level(source: u64, level: usize) -> Option<u64> {
        let fresh = frame::alloc_zeroed_frame()? as u64;
        let leaf = level == pte::LEVELS - 1;
        unsafe {
            let src = source as *const u64;
            let dst = fresh as *mut u64;
            for i in 0..ENTRIES {
                let e = *src.add(i);
                if !pte::valid(e) {
                    if leaf && pte::is_lazy(e) {
                        *dst.add(i) = e;
                    }
                    continue;
                }
                if leaf {
                    if pte::owned(e) {
                        let Some(page) = frame::alloc_frame() else {
                            *dst.add(i) = 0;
                            Self::free_level(fresh, level);
                            return None;
                        };
                        core::ptr::copy_nonoverlapping(pte::addr(e) as *const u8, page as *mut u8, PAGE_SIZE as usize);
                        pte::sync_code(page as u64, PAGE_SIZE);
                        *dst.add(i) = pte::with_addr(e, page as u64);
                    } else {
                        *dst.add(i) = e;
                    }
                } else if !pte::is_table(e, level) {
                    *dst.add(i) = e;
                } else {
                    let Some(child) = Self::copy_level(pte::addr(e), level + 1) else {
                        Self::free_level(fresh, level);
                        return None;
                    };
                    *dst.add(i) = pte::with_addr(e, child);
                }
            }
        }
        Some(fresh)
    }

    pub fn duplicate(&self) -> Option<AddressSpace> {
        let mut copy = AddressSpace::new()?;
        copy.lazy = self.lazy.clone();
        for index in pte::USER_TOP {
            unsafe {
                let e = *(self.root as *const u64).add(index);
                if !pte::valid(e) || !pte::is_table(e, 0) {
                    continue;
                }
                let Some(table) = Self::copy_level(pte::addr(e), 1) else {
                    copy.destroy();
                    return None;
                };
                *(copy.root as *mut u64).add(index) = pte::with_addr(e, table);
            }
        }
        pte::barrier();
        Some(copy)
    }

    fn free_level(table: u64, level: usize) {
        let leaf = level == pte::LEVELS - 1;
        unsafe {
            let entries = table as *const u64;
            for i in 0..ENTRIES {
                let e = *entries.add(i);
                if !pte::valid(e) {
                    continue;
                }
                if leaf {
                    if pte::owned(e) {
                        frame::free_frame(pte::addr(e) as usize);
                    }
                } else if pte::is_table(e, level) {
                    Self::free_level(pte::addr(e), level + 1);
                }
            }
        }
        frame::free_frame(table as usize);
    }

    pub fn destroy(&mut self) {
        if self.root == 0 {
            return;
        }
        if pte::read_root() == self.root {
            pte::load_root(pte::kernel_root());
        }
        for index in pte::USER_TOP {
            unsafe {
                let e = *(self.root as *const u64).add(index);
                if pte::valid(e) && pte::is_table(e, 0) {
                    Self::free_level(pte::addr(e), 1);
                }
            }
        }
        frame::free_frame(self.root as usize);
        self.root = 0;
    }
}

pub fn resident_pages(root: u64) -> u64 {
    fn walk(table: u64, level: usize) -> u64 {
        let mut count = 0;
        unsafe {
            let entries = table as *const u64;
            for i in 0..ENTRIES {
                let e = *entries.add(i);
                if !pte::valid(e) {
                    continue;
                }
                if level == pte::LEVELS - 1 {
                    if pte::owned(e) {
                        count += 1;
                    }
                } else if pte::is_table(e, level) {
                    count += walk(pte::addr(e), level + 1);
                }
            }
        }
        count
    }
    if root == 0 {
        return 0;
    }
    let mut total = 0;
    for index in pte::USER_TOP {
        let e = unsafe { *(root as *const u64).add(index) };
        if pte::valid(e) && pte::is_table(e, 0) {
            total += walk(pte::addr(e), 1);
        }
    }
    total
}
