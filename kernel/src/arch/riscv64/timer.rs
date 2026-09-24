use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::fdt::Fdt;

static FREQUENCY: AtomicU64 = AtomicU64::new(10_000_000);
static INTERVAL: AtomicU64 = AtomicU64::new(0);
static HZ: AtomicU32 = AtomicU32::new(0);
static TICKS: AtomicU64 = AtomicU64::new(0);

const STIE: usize = 1 << 5;

pub fn frequency() -> u64 {
    FREQUENCY.load(Ordering::Relaxed)
}

pub fn counter() -> u64 {
    let time: u64;
    unsafe { core::arch::asm!("rdtime {}", out(reg) time, options(nomem, nostack)) };
    time
}

pub fn now_ns() -> u64 {
    (counter() as u128 * 1_000_000_000 / frequency().max(1) as u128) as u64
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn hz() -> u32 {
    HZ.load(Ordering::Relaxed)
}

pub fn on_interrupt() {
    super::sbi::set_timer(counter() + INTERVAL.load(Ordering::Relaxed));
    TICKS.fetch_add(1, Ordering::Relaxed);
    if SCHEDULER.load(Ordering::Relaxed) {
        super::traps::note_tick();
    } else {
        crate::memory::vmalloc::tick(0);
    }
}

static SCHEDULER: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn set_hz(hz: u32) {
    INTERVAL.store(frequency() / hz.max(1) as u64, Ordering::Relaxed);
    HZ.store(hz, Ordering::Relaxed);
    SCHEDULER.store(true, Ordering::Relaxed);
    super::sbi::set_timer(counter() + INTERVAL.load(Ordering::Relaxed));
}

pub fn init(fdt: &Fdt, hz: u32) -> u32 {
    if let Some(freq) = fdt.find("/cpus").and_then(|c| c.u32_property("timebase-frequency")) {
        FREQUENCY.store(freq as u64, Ordering::Relaxed);
    }
    INTERVAL.store(frequency() / hz as u64, Ordering::Relaxed);
    HZ.store(hz, Ordering::Relaxed);
    super::sbi::init();
    super::sbi::set_timer(counter() + INTERVAL.load(Ordering::Relaxed));
    unsafe { core::arch::asm!("csrs sie, {}", in(reg) STIE, options(nostack)) };
    IRQ_TIMER_ID
}

const IRQ_TIMER_ID: u32 = 5;
