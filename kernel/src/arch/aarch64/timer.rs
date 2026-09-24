use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::fdt::Fdt;

static INTERVAL: AtomicU64 = AtomicU64::new(0);
static HZ: AtomicU32 = AtomicU32::new(0);
static TICKS: AtomicU64 = AtomicU64::new(0);

const VIRTUAL_PPI: u32 = 11;

pub fn frequency() -> u64 {
    let freq: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack)) };
    freq
}

pub fn counter() -> u64 {
    let count: u64;
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) count, options(nomem, nostack)) };
    count
}

pub fn now_ns() -> u64 {
    let freq = frequency().max(1);
    (counter() as u128 * 1_000_000_000 / freq as u128) as u64
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn hz() -> u32 {
    HZ.load(Ordering::Relaxed)
}

fn arm(interval: u64) {
    unsafe {
        core::arch::asm!("msr cntv_tval_el0, {}", "msr cntv_ctl_el0, {}", "isb", in(reg) interval, in(reg) 1u64, options(nostack));
    }
}

fn on_tick(_: u32) {
    arm(INTERVAL.load(Ordering::Relaxed));
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
    arm(INTERVAL.load(Ordering::Relaxed));
}

pub fn init(fdt: &Fdt, hz: u32) -> u32 {
    let ppi = fdt
        .find_compatible("arm,armv8-timer")
        .and_then(|n| n.property("interrupts"))
        .and_then(|cells| cells.get(28..32))
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(VIRTUAL_PPI);
    let intid = 16 + ppi;
    INTERVAL.store(frequency() / hz as u64, Ordering::Relaxed);
    HZ.store(hz, Ordering::Relaxed);
    crate::arch::irqtab::register(intid, on_tick);
    super::gic::enable(intid);
    arm(INTERVAL.load(Ordering::Relaxed));
    intid
}
