use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use crate::fdt::{Fdt, Node};

const GICD_CTLR: usize = 0x000;
const GICD_TYPER: usize = 0x004;
const GICD_IGROUPR: usize = 0x080;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICD_IPRIORITYR: usize = 0x400;
const GICD_ITARGETSR: usize = 0x800;
const GICD_IROUTER: usize = 0x6000;

const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_BPR: usize = 0x008;
const GICC_IAR: usize = 0x00C;
const GICC_EOIR: usize = 0x010;

const GICR_WAKER: usize = 0x014;
const GICR_SGI: usize = 0x1_0000;
const GICR_STRIDE: usize = 0x2_0000;
const GICD_CTLR_RWP: u32 = 1 << 31;

const SPURIOUS: u32 = 1020;
const PRIORITY: u8 = 0xA0;

static VERSION: AtomicU8 = AtomicU8::new(0);
static DIST: AtomicUsize = AtomicUsize::new(0);
static CPU: AtomicUsize = AtomicUsize::new(0);
static LINES: AtomicUsize = AtomicUsize::new(0);

fn read(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

fn write(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value) }
}

fn write8(addr: usize, value: u8) {
    unsafe { write_volatile(addr as *mut u8, value) }
}

fn wait_rwp(dist: usize) {
    let mut spins = 0u32;
    while read(dist + GICD_CTLR) & GICD_CTLR_RWP != 0 && spins < 1_000_000 {
        spins += 1;
        core::hint::spin_loop();
    }
}

fn icc_write_sre(value: u64) {
    unsafe { core::arch::asm!("msr S3_0_C12_C12_5, {}", "isb", in(reg) value, options(nostack)) };
}

fn icc_write_pmr(value: u64) {
    unsafe { core::arch::asm!("msr S3_0_C4_C6_0, {}", in(reg) value, options(nostack)) };
}

fn icc_write_bpr1(value: u64) {
    unsafe { core::arch::asm!("msr S3_0_C12_C12_3, {}", in(reg) value, options(nostack)) };
}

fn icc_write_igrpen1(value: u64) {
    unsafe { core::arch::asm!("msr S3_0_C12_C12_7, {}", "isb", in(reg) value, options(nostack)) };
}

fn icc_read_iar1() -> u32 {
    let value: u64;
    unsafe { core::arch::asm!("mrs {}, S3_0_C12_C12_0", out(reg) value, options(nostack)) };
    value as u32
}

fn icc_write_eoir1(value: u32) {
    unsafe { core::arch::asm!("msr S3_0_C12_C12_1, {}", in(reg) value as u64, options(nostack)) };
}

fn find_redistributor(base: usize) -> usize {
    let mut frame = base;
    for _ in 0..64 {
        let typer = unsafe { read_volatile((frame + 0x008) as *const u64) };
        if typer >> 32 == 0 {
            return frame;
        }
        if typer & (1 << 4) != 0 {
            break;
        }
        frame += GICR_STRIDE;
    }
    base
}

pub fn init(fdt: &Fdt) -> Option<&'static str> {
    let (node, version) = if let Some(node) = fdt.find_compatible("arm,gic-v3") {
        (node, 3)
    } else {
        let node = ["arm,cortex-a15-gic", "arm,gic-400", "arm,cortex-a9-gic"].iter().find_map(|c| fdt.find_compatible(c))?;
        (node, 2)
    };
    let mut regs = node.reg();
    let dist = regs.next()?.0 as usize;
    let cpu = regs.next()?.0 as usize;
    let lines = (((read(dist + GICD_TYPER) & 0x1F) as usize + 1) * 32).min(1020);

    write(dist + GICD_CTLR, 0);
    for n in (32..lines).step_by(32) {
        write(dist + GICD_ICENABLER + n / 8, u32::MAX);
    }
    for n in 32..lines {
        write8(dist + GICD_IPRIORITYR + n, PRIORITY);
    }

    if version == 3 {
        for n in (32..lines).step_by(32) {
            write(dist + GICD_IGROUPR + n / 8, u32::MAX);
        }
        for n in 32..lines {
            unsafe { write_volatile((dist + GICD_IROUTER + n * 8) as *mut u64, 0) };
        }
        write(dist + GICD_CTLR, (1 << 4) | (1 << 1));
        wait_rwp(dist);
        let rd = find_redistributor(cpu);
        write(rd + GICR_WAKER, read(rd + GICR_WAKER) & !(1 << 1));
        let mut spins = 0u32;
        while read(rd + GICR_WAKER) & (1 << 2) != 0 && spins < 1_000_000 {
            spins += 1;
            core::hint::spin_loop();
        }
        let sgi = rd + GICR_SGI;
        write(sgi + GICD_IGROUPR, u32::MAX);
        for n in 0..32 {
            write8(sgi + GICD_IPRIORITYR + n, PRIORITY);
        }
        CPU.store(rd, Ordering::Relaxed);
        icc_write_sre(0x7);
        icc_write_pmr(0xFF);
        icc_write_bpr1(0);
        icc_write_igrpen1(1);
    } else {
        for n in 32..lines {
            write8(dist + GICD_ITARGETSR + n, 1);
        }
        write(dist + GICD_CTLR, 1);
        write(cpu + GICC_PMR, 0xFF);
        write(cpu + GICC_BPR, 0);
        write(cpu + GICC_CTLR, 1);
        CPU.store(cpu, Ordering::Relaxed);
    }
    DIST.store(dist, Ordering::Relaxed);
    LINES.store(lines, Ordering::Relaxed);
    VERSION.store(version, Ordering::Release);
    Some(if version == 3 { "GICv3" } else { "GICv2" })
}

pub fn disable(intid: u32) {
    let n = intid as usize;
    if VERSION.load(Ordering::Acquire) == 3 && n < 32 {
        let sgi = CPU.load(Ordering::Relaxed) + GICR_SGI;
        write(sgi + GICD_ICENABLER, 1 << n);
        return;
    }
    let dist = DIST.load(Ordering::Relaxed);
    write(dist + GICD_ICENABLER + (n / 32) * 4, 1 << (n % 32));
}

pub fn enable(intid: u32) {
    let n = intid as usize;
    if VERSION.load(Ordering::Acquire) == 3 && n < 32 {
        let sgi = CPU.load(Ordering::Relaxed) + GICR_SGI;
        write8(sgi + GICD_IPRIORITYR + n, PRIORITY);
        write(sgi + GICD_ISENABLER, 1 << n);
        return;
    }
    if n >= LINES.load(Ordering::Relaxed) && n >= 32 {
        return;
    }
    let dist = DIST.load(Ordering::Relaxed);
    write8(dist + GICD_IPRIORITYR + n, PRIORITY);
    write(dist + GICD_ISENABLER + (n / 32) * 4, 1 << (n % 32));
}

pub fn interrupt_of(node: &Node) -> Option<u32> {
    let cells = node.property("interrupts")?;
    let word = |i: usize| cells.get(i * 4..i * 4 + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let (kind, number) = (word(0)?, word(1)?);
    Some(if kind == 1 { 16 + number } else { 32 + number })
}

pub fn handle() {
    let v3 = VERSION.load(Ordering::Acquire) == 3;
    let cpu = CPU.load(Ordering::Relaxed);
    loop {
        let iar = if v3 { icc_read_iar1() } else { read(cpu + GICC_IAR) };
        let intid = iar & 0xFF_FFFF;
        if intid >= SPURIOUS {
            break;
        }
        crate::arch::irqtab::dispatch(intid);
        if v3 {
            icc_write_eoir1(iar);
        } else {
            write(cpu + GICC_EOIR, iar);
        }
    }
}
