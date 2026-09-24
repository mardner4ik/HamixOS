use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::arch::smp::{cpu_id, MAX_CPUS};
use crate::arch::{disable_interrupts, enable_interrupts, interrupts_enabled};

static LOCKED: AtomicBool = AtomicBool::new(false);
static HELD: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
static CONTENDED: AtomicU64 = AtomicU64::new(0);

pub fn held() -> bool {
    HELD[cpu_id()].load(Ordering::Relaxed)
}

pub fn try_acquire() -> bool {
    let cpu = cpu_id();
    if HELD[cpu].load(Ordering::Relaxed) {
        return true;
    }
    if LOCKED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        HELD[cpu].store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

pub fn acquire() {
    if try_acquire() {
        return;
    }
    CONTENDED.fetch_add(1, Ordering::Relaxed);
    let was_enabled = interrupts_enabled();
    loop {
        disable_interrupts();
        if try_acquire() {
            break;
        }
        enable_interrupts();
        for _ in 0..64 {
            core::hint::spin_loop();
            if !LOCKED.load(Ordering::Relaxed) {
                break;
            }
        }
    }
    if was_enabled {
        enable_interrupts();
    }
}

pub fn acquire_from_trap() {
    acquire();
}

pub fn release() {
    let cpu = cpu_id();
    if HELD[cpu].swap(false, Ordering::Relaxed) {
        LOCKED.store(false, Ordering::Release);
    }
}

pub fn contention() -> u64 {
    CONTENDED.load(Ordering::Relaxed)
}
