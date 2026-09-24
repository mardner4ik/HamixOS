use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_CPUS: usize = 8;

pub static BUSY_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
pub static TOTAL_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

#[inline(always)]
pub fn cpu_id() -> usize {
    0
}

pub fn count() -> usize {
    1
}

pub fn online(cpu: usize) -> bool {
    cpu == 0
}

pub fn apic_id_of(cpu: usize) -> u8 {
    cpu as u8
}

pub fn kick_idle(_except: usize) {}

pub fn start_aps(_hz: u64) -> usize {
    1
}

pub fn set_syscall_stack(_top: u64) {}

pub fn record_tick(cpu: usize, busy: bool) {
    if cpu < MAX_CPUS {
        TOTAL_TICKS[cpu].fetch_add(1, Ordering::Relaxed);
        if busy {
            BUSY_TICKS[cpu].fetch_add(1, Ordering::Relaxed);
        }
    }
}
