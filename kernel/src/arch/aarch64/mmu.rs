use core::arch::asm;

#[repr(C, align(4096))]
struct Table([u64; 512]);

static mut LEVEL0: Table = Table([0; 512]);
static mut LEVEL1: Table = Table([0; 512]);

const BLOCK: u64 = 0b01;
const ATTR_DEVICE: u64 = 0 << 2;
const ATTR_NORMAL: u64 = 1 << 2;
const INNER_SHAREABLE: u64 = 0b11 << 8;
const ACCESSED: u64 = 1 << 10;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;

pub const MAIR: u64 = 0x00 | (0xff << 8) | (0x44 << 16);
const TABLE: u64 = 0b11;
const T0SZ: u64 = 16;
const IRGN0_WBWA: u64 = 1 << 8;
const ORGN0_WBWA: u64 = 1 << 10;
const SH0_INNER: u64 = 3 << 12;
const EPD1: u64 = 1 << 23;

const SCTLR_M: u64 = 1 << 0;
const SCTLR_A: u64 = 1 << 1;
const SCTLR_C: u64 = 1 << 2;
const SCTLR_I: u64 = 1 << 12;
const SCTLR_WXN: u64 = 1 << 19;
const SCTLR_EL0_ACCESS: u64 = (1 << 14) | (1 << 15) | (1 << 16) | (1 << 18) | (1 << 26);

const RAM_FIRST_GIB: usize = 1;
const RAM_END_GIB: usize = 256;

static ENABLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn tcr() -> u64 {
    let mmfr0: u64;
    unsafe { asm!("mrs {}, id_aa64mmfr0_el1", out(reg) mmfr0, options(nomem, nostack)) };
    let ips = (mmfr0 & 0x7).min(5);
    T0SZ | IRGN0_WBWA | ORGN0_WBWA | SH0_INNER | EPD1 | (ips << 32)
}

pub fn init() {
    unsafe {
        let root = &raw mut LEVEL0;
        let table = &raw mut LEVEL1;
        (*root).0[0] = table as u64 | TABLE;
        for gib in 0..512 {
            let base = (gib as u64) << 30;
            (*table).0[gib] = if (RAM_FIRST_GIB..RAM_END_GIB).contains(&gib) {
                base | BLOCK | ATTR_NORMAL | INNER_SHAREABLE | ACCESSED | UXN
            } else {
                base | BLOCK | ATTR_DEVICE | ACCESSED | PXN | UXN
            };
        }
        let tcr = tcr();
        asm!(
            "msr mair_el1, {mair}",
            "msr tcr_el1, {tcr}",
            "msr ttbr0_el1, {ttbr}",
            "dsb ish",
            "isb",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            mair = in(reg) MAIR,
            tcr = in(reg) tcr,
            ttbr = in(reg) root as u64,
            options(nostack),
        );
        let mut sctlr: u64;
        asm!("mrs {}, sctlr_el1", out(reg) sctlr, options(nomem, nostack));
        sctlr |= SCTLR_M | SCTLR_C | SCTLR_I | SCTLR_EL0_ACCESS;
        sctlr &= !(SCTLR_A | SCTLR_WXN);
        asm!("msr sctlr_el1, {}", "isb", in(reg) sctlr, options(nostack));
    }
    ENABLED.store(true, core::sync::atomic::Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(core::sync::atomic::Ordering::Relaxed)
}
