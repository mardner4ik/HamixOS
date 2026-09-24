use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::fdt::{Fdt, Node};

const PRIORITY: usize = 0x0000;
const ENABLE: usize = 0x2000;
const ENABLE_STRIDE: usize = 0x80;
const CONTEXT: usize = 0x20_0000;
const CONTEXT_STRIDE: usize = 0x1000;
const SEIE: usize = 1 << 9;
const SUPERVISOR_EXTERNAL: u32 = 9;

static BASE: AtomicUsize = AtomicUsize::new(0);
static CONTEXT_ID: AtomicUsize = AtomicUsize::new(usize::MAX);

fn words(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    bytes.chunks_exact(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn hart_controller(fdt: &Fdt, hart: usize) -> Option<u32> {
    let cpus = fdt.find("/cpus")?;
    let cpu = cpus.children().find(|n| n.str_property("device_type") == Some("cpu") && n.reg().next().map(|r| r.0 as usize) == Some(hart))?;
    cpu.children().find(|n| n.compatible_with("riscv,cpu-intc"))?.u32_property("phandle")
}

pub fn init(fdt: &Fdt, hart: usize) -> Option<usize> {
    let node = fdt.find_compatible("sifive,plic-1.0.0").or_else(|| fdt.find_compatible("riscv,plic0"))?;
    let base = node.reg().next()?.0 as usize;
    let controller = hart_controller(fdt, hart)?;
    let pairs: alloc::vec::Vec<u32> = words(node.property("interrupts-extended")?).collect();
    let context = pairs.chunks_exact(2).position(|p| p[0] == controller && p[1] == SUPERVISOR_EXTERNAL)?;
    let sources = node.u32_property("riscv,ndev").unwrap_or(127) as usize;
    unsafe {
        for word in 0..=sources / 32 {
            write_volatile((base + ENABLE + context * ENABLE_STRIDE + word * 4) as *mut u32, 0);
        }
        write_volatile((base + CONTEXT + context * CONTEXT_STRIDE) as *mut u32, 0);
        core::arch::asm!("csrs sie, {}", in(reg) SEIE, options(nostack));
    }
    BASE.store(base, Ordering::Relaxed);
    CONTEXT_ID.store(context, Ordering::Release);
    Some(context)
}

pub fn disable(irq: u32) {
    let base = BASE.load(Ordering::Relaxed);
    let context = CONTEXT_ID.load(Ordering::Acquire);
    if base == 0 || context == usize::MAX || irq == 0 {
        return;
    }
    let n = irq as usize;
    unsafe {
        let enable = (base + ENABLE + context * ENABLE_STRIDE + (n / 32) * 4) as *mut u32;
        write_volatile(enable, read_volatile(enable) & !(1 << (n % 32)));
    }
}

pub fn enable(irq: u32) {
    let base = BASE.load(Ordering::Relaxed);
    let context = CONTEXT_ID.load(Ordering::Acquire);
    if base == 0 || context == usize::MAX || irq == 0 {
        return;
    }
    let n = irq as usize;
    unsafe {
        write_volatile((base + PRIORITY + n * 4) as *mut u32, 1);
        let enable = (base + ENABLE + context * ENABLE_STRIDE + (n / 32) * 4) as *mut u32;
        write_volatile(enable, read_volatile(enable) | 1 << (n % 32));
    }
}

pub fn interrupt_of(node: &Node) -> Option<u32> {
    node.u32_property("interrupts")
}

pub fn handle() {
    let base = BASE.load(Ordering::Relaxed);
    let context = CONTEXT_ID.load(Ordering::Acquire);
    if base == 0 || context == usize::MAX {
        return;
    }
    let claim = (base + CONTEXT + context * CONTEXT_STRIDE + 4) as *mut u32;
    loop {
        let irq = unsafe { read_volatile(claim) };
        if irq == 0 {
            break;
        }
        crate::arch::irqtab::dispatch(irq);
        unsafe { write_volatile(claim, irq) };
    }
}
