use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::arch::{paging, smp, without_interrupts};
use crate::memory::frame;

static DEFERRED: Mutex<Vec<(u64, usize)>> = Mutex::new(Vec::new());
static SHOOTDOWNS: AtomicU64 = AtomicU64::new(0);

pub fn defer_free(root: u64, phys: usize) {
    without_interrupts(|| DEFERRED.lock().push((root, phys)));
}

pub fn shootdown(root: u64) {
    if smp::shootdown(root) {
        SHOOTDOWNS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn shootdowns() -> u64 {
    SHOOTDOWNS.load(Ordering::Relaxed)
}

pub fn timeouts() -> u64 {
    smp::shootdown_timeouts()
}

pub fn sync() {
    paging::flush_tlb();
    let pending = without_interrupts(|| core::mem::take(&mut *DEFERRED.lock()));
    let mut roots: Vec<u64> = pending.iter().map(|&(root, _)| root).collect();
    roots.push(paging::read_root());
    roots.sort_unstable();
    roots.dedup();
    for root in roots {
        shootdown(root);
    }
    for (_, phys) in pending {
        frame::free_frame(phys);
    }
}
