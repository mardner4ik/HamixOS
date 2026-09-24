use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::{read_msr, write_msr};

const IA32_APIC_BASE: u32 = 0x1B;
const REG_ID: u64 = 0x20;
const REG_TPR: u64 = 0x80;
const REG_EOI: u64 = 0xB0;
const REG_SVR: u64 = 0xF0;
const REG_ESR: u64 = 0x280;
const REG_ICR_LOW: u64 = 0x300;
const REG_ICR_HIGH: u64 = 0x310;
const REG_LVT_TIMER: u64 = 0x320;
const REG_LVT_LINT0: u64 = 0x350;
const REG_LVT_LINT1: u64 = 0x360;
const REG_LVT_ERROR: u64 = 0x370;
const REG_TIMER_INIT: u64 = 0x380;
const REG_TIMER_CURRENT: u64 = 0x390;
const REG_TIMER_DIVIDE: u64 = 0x3E0;

pub const TIMER_VECTOR: u8 = 0xF0;
pub const WAKE_VECTOR: u8 = 0xF1;
pub const ERROR_VECTOR: u8 = 0xFE;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

static BASE: AtomicU64 = AtomicU64::new(0);
static TIMER_COUNT: AtomicU32 = AtomicU32::new(0);

fn read(reg: u64) -> u32 {
    unsafe { core::ptr::read_volatile((BASE.load(Ordering::Relaxed) + reg) as *const u32) }
}

fn write(reg: u64, value: u32) {
    unsafe { core::ptr::write_volatile((BASE.load(Ordering::Relaxed) + reg) as *mut u32, value) }
}

pub fn present() -> bool {
    core::arch::x86_64::__cpuid_count(1, 0).edx & (1 << 9) != 0
}

pub fn ready() -> bool {
    BASE.load(Ordering::Relaxed) != 0
}

pub fn id() -> u8 {
    if !ready() {
        return 0;
    }
    (read(REG_ID) >> 24) as u8
}

pub fn enable(bsp: bool, hint: u64) {
    let msr = read_msr(IA32_APIC_BASE);
    let mut base = msr & 0x000F_FFFF_FFFF_F000;
    if base == 0 {
        base = if hint != 0 { hint } else { 0xFEE0_0000 };
    }
    write_msr(IA32_APIC_BASE, (msr & !0xFFF_FFFF_F000) | base | (1 << 11));
    BASE.store(base, Ordering::Relaxed);
    write(REG_TPR, 0);
    write(REG_LVT_LINT0, if bsp { 0x700 } else { 0x1_0000 });
    write(REG_LVT_LINT1, if bsp { 0x400 } else { 0x1_0000 });
    write(REG_LVT_ERROR, ERROR_VECTOR as u32);
    write(REG_ESR, 0);
    write(REG_ESR, 0);
    write(REG_SVR, 0x100 | SPURIOUS_VECTOR as u32);
    write(REG_EOI, 0);
}

pub fn eoi() {
    write(REG_EOI, 0);
}

pub fn calibrate(hz: u64) -> u32 {
    write(REG_TIMER_DIVIDE, 0x3);
    write(REG_LVT_TIMER, 0x1_0000 | TIMER_VECTOR as u32);
    write(REG_TIMER_INIT, u32::MAX);
    super::pit_delay_us(20_000);
    let elapsed = u32::MAX - read(REG_TIMER_CURRENT);
    write(REG_TIMER_INIT, 0);
    let per_tick = ((elapsed as u64 * 50) / hz).clamp(16, u32::MAX as u64) as u32;
    TIMER_COUNT.store(per_tick, Ordering::Relaxed);
    per_tick
}

pub fn start_timer() {
    let count = TIMER_COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return;
    }
    write(REG_TIMER_DIVIDE, 0x3);
    write(REG_LVT_TIMER, 0x2_0000 | TIMER_VECTOR as u32);
    write(REG_TIMER_INIT, count);
}

fn wait_delivery() {
    for _ in 0..100_000 {
        if read(REG_ICR_LOW) & (1 << 12) == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

pub fn send_init(apic_id: u8) {
    write(REG_ESR, 0);
    write(REG_ICR_HIGH, (apic_id as u32) << 24);
    write(REG_ICR_LOW, 0x0000_C500);
    wait_delivery();
    super::pit_delay_us(10_000);
    write(REG_ICR_HIGH, (apic_id as u32) << 24);
    write(REG_ICR_LOW, 0x0000_8500);
    wait_delivery();
}

pub fn send_startup(apic_id: u8, vector: u8) {
    write(REG_ESR, 0);
    write(REG_ICR_HIGH, (apic_id as u32) << 24);
    write(REG_ICR_LOW, 0x0000_0600 | vector as u32);
    wait_delivery();
}

pub fn send_ipi(apic_id: u8, vector: u8) {
    super::without_interrupts(|| {
        write(REG_ICR_HIGH, (apic_id as u32) << 24);
        write(REG_ICR_LOW, vector as u32);
    });
}
