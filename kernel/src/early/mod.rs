pub mod console;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::panic::PanicInfo;

use crate::fdt::Fdt;
use crate::{arch, memory};
use console::kprintln;

#[cfg(target_arch = "aarch64")]
const ARCH: &str = "aarch64";
#[cfg(target_arch = "riscv64")]
const ARCH: &str = "riscv64";

const MIB: usize = 1024 * 1024;
const TIMER_HZ: u32 = 100;

pub fn start(dtb: usize, cpu: usize) -> ! {
    console::init();
    memory::early_heap_init();
    kprintln!();
    kprintln!("HamixOS kernel ({}), boot cpu {}", ARCH, cpu);

    let Some(fdt) = (unsafe { Fdt::from_addr(dtb) }) else {
        panic!("no flattened device tree at {:#x}", dtb);
    };
    if let Some(uart) = console::adopt(&fdt) {
        kprintln!("console: {}", uart);
    }
    let model = fdt.root().and_then(|r| r.str_property("model")).unwrap_or("unknown machine");
    kprintln!("machine: {} (dtb at {:#x}, {} bytes)", model, fdt.address(), fdt.total_size());
    describe_cpus(&fdt);

    let layout = memory::init_devicetree(&fdt);
    let (free, total) = memory::frame::memory_info();
    kprintln!(
        "memory: {} MiB in {} bank(s), {} MiB free, {} reserved range(s), kernel {:#x}-{:#x}",
        total / MIB,
        layout.banks,
        free / MIB,
        layout.reserved,
        memory::kernel_start(),
        memory::kernel_end()
    );
    let cmdline = memory::cmdline();
    if !cmdline.is_empty() {
        kprintln!("cmdline: {}", cmdline);
    }
    if let Some(initrd) = memory::find_module("initrd") {
        kprintln!("initrd: {:#x}-{:#x} ({} KiB)", initrd.start, initrd.end, initrd.len() / 1024);
    }

    arch::traps::init();
    test_traps();

    let mode = start_paging(&fdt);
    kprintln!("paging: {}, kernel root {:#x}", mode, arch::paging::kernel_root());
    test_paging();
    self_test();

    start_interrupts(&fdt, cpu);
    test_timer();

    kprintln!("early boot complete, starting the kernel");
    boot_kernel(&fdt, cpu)
}

fn boot_kernel(fdt: &Fdt, cpu: usize) -> ! {
    use crate::{drivers, hxinit, syscall, task};

    crate::fdt::remember(fdt.address());
    crate::platform::probe(fdt);
    {
        let (free, total) = memory::frame::memory_info();
        hxinit::record("memory", "Physical memory", hxinit::ok(format!("{} MiB, {} MiB free, kernel heap grows on demand", total >> 20, free >> 20)));
    }
    hxinit::record("cpu", "Processor", hxinit::ok(format!("{} (boot cpu {})", arch::platform::cpu_brand(), cpu)));
    hxinit::record("interrupts", "Interrupts and exceptions", hxinit::ok(format!("{} lines, timer at {} Hz", ARCH, arch::timer::hz())));
    hxinit::record("framebuffer", "Framebuffer console", match *memory::FRAMEBUFFER.lock() {
        Some(fb) => hxinit::ok(format!("{}x{}x{}", fb.width, fb.height, fb.bpp)),
        None => hxinit::warn("serial console only"),
    });
    drivers::video::text_mode::cache_framebuffer();
    drivers::video::text_mode::fb_clear_full();
    syscall::init();
    task::init();
    console::forward_to_keyboard();
    hxinit::start();
    task::idle_loop();
}

#[cfg(target_arch = "aarch64")]
fn start_paging(fdt: &Fdt) -> String {
    match arch::paging::init(fdt) {
        Some(normal) => format!("4-level 4 KiB tables, 48-bit VA, {} GiB normal memory, rest device", normal),
        None => panic!("cannot build the kernel page tables"),
    }
}

#[cfg(target_arch = "riscv64")]
fn start_paging(_: &Fdt) -> String {
    match arch::paging::init() {
        Some(mode) => format!("{}, identity 0-128 GiB, user 128-192 GiB, vmalloc 192-224 GiB", mode),
        None => panic!("the cpu does not support Sv39"),
    }
}

#[cfg(target_arch = "aarch64")]
fn start_controller(fdt: &Fdt, _: usize) -> String {
    arch::irq::init(fdt).map(String::from).unwrap_or_else(|| String::from("no GIC"))
}

#[cfg(target_arch = "riscv64")]
fn start_controller(fdt: &Fdt, hart: usize) -> String {
    let (major, minor) = arch::riscv64::sbi::spec_version();
    match arch::irq::init(fdt, hart) {
        Some(context) => format!("PLIC context {}, SBI {}.{} (impl {})", context, major, minor, arch::riscv64::sbi::implementation()),
        None => String::from("no PLIC"),
    }
}

fn start_interrupts(fdt: &Fdt, cpu: usize) {
    let controller = start_controller(fdt, cpu);
    let timer_irq = arch::timer::init(fdt, TIMER_HZ);
    let uart_irq = console::enable_receive_interrupt();
    arch::enable_interrupts();
    kprintln!(
        "interrupts: {}, timer irq {} at {} Hz ({} MHz counter), uart irq {}",
        controller,
        timer_irq,
        arch::timer::hz(),
        arch::timer::frequency() / 1_000_000,
        uart_irq.map(|i| format!("{}", i)).unwrap_or_else(|| String::from("none"))
    );
}

fn test_traps() {
    arch::traps::breakpoint();
    arch::traps::breakpoint();
    assert_eq!(arch::traps::breakpoints(), 2);
    kprintln!("traps: vector table installed, 2 breakpoints trapped and resumed");
}

fn test_paging() {
    use arch::paging::{AddressSpace, PAGE_SIZE, USER_BASE};

    let (free_before, _) = memory::frame::memory_info();
    let base = USER_BASE + 0x40_0000;
    let fault = arch::traps::probe_read(base as usize).expect_err("an unmapped user page did not fault");
    let mut space = AddressSpace::new().expect("no memory for an address space");
    assert!(space.alloc_range(base, 3 * PAGE_SIZE));
    let phys = space.translate(base + PAGE_SIZE + 8).expect("mapped page has no translation");
    space.activate();
    unsafe { core::ptr::write_volatile((base + PAGE_SIZE + 8) as *mut u64, 0xC0FF_EE00_1234_5678) };
    assert_eq!(unsafe { core::ptr::read_volatile(phys as *const u64) }, 0xC0FF_EE00_1234_5678);
    assert_eq!(arch::traps::probe_read(base as usize), Ok(0));
    space.unmap_range(base + PAGE_SIZE, PAGE_SIZE);
    arch::paging::flush_tlb();
    assert!(arch::traps::probe_read((base + PAGE_SIZE) as usize).is_err());
    let other = AddressSpace::new().expect("no memory for an address space");
    assert!(other.translate(base).is_none());
    other.activate();
    assert!(arch::traps::probe_read(base as usize).is_err());
    let mut other = other;
    other.destroy();
    space.destroy();
    arch::paging::load_root(arch::paging::kernel_root());
    let (free_after, _) = memory::frame::memory_info();
    kprintln!(
        "address spaces: map/translate/unmap ok, isolation ok, fault at {:#x} recovered, {} frames leaked",
        fault,
        free_before.saturating_sub(free_after) / memory::frame::PAGE_SIZE
    );
}

fn test_timer() {
    let first = arch::timer::ticks();
    let started = arch::timer::now_ns();
    let wanted = first + TIMER_HZ as u64 / 4;
    let mut spins = 0u64;
    while arch::timer::ticks() < wanted && spins < 100_000_000 {
        arch::hlt();
        spins += 1;
    }
    let ticks = arch::timer::ticks() - first;
    let elapsed = arch::timer::now_ns() - started;
    assert!(ticks > 0, "the timer interrupt never fired");
    kprintln!(
        "timer: {} ticks in {} ms (measured {} Hz), spurious irqs {}",
        ticks,
        elapsed / 1_000_000,
        ticks * 1_000_000_000 / elapsed.max(1),
        arch::irqtab::spurious()
    );
}

fn describe_cpus(fdt: &Fdt) {
    let Some(cpus) = fdt.find("/cpus") else {
        return;
    };
    let count = cpus.children().filter(|n| n.str_property("device_type") == Some("cpu")).count();
    let first = cpus.children().find(|n| n.str_property("device_type") == Some("cpu"));
    let name = first.and_then(|n| n.strings("compatible").next()).unwrap_or("?");
    let isa = first.and_then(|n| n.str_property("riscv,isa"));
    let timebase = cpus.u32_property("timebase-frequency");
    match (isa, timebase) {
        (Some(isa), Some(hz)) => kprintln!("cpus: {} x {} ({}), timebase {} Hz", count, name, isa, hz),
        _ => kprintln!("cpus: {} x {}", count, name),
    }
    #[cfg(target_arch = "aarch64")]
    kprintln!("cpu: running at EL{}, mmu {}", arch::aarch64::current_el(), if arch::aarch64::mmu::enabled() { "on" } else { "off" });
}

fn self_test() {
    let (free_before, _) = memory::frame::memory_info();

    let mut numbers: Vec<u64> = (0..(1u64 << 20)).collect();
    for n in numbers.iter_mut() {
        *n = n.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17);
    }
    let checksum = numbers.iter().fold(0u64, |acc, &n| acc ^ n);

    let mut names = BTreeMap::new();
    for i in 0..4096u32 {
        names.insert(i, format!("node-{}", i));
    }
    let blocks: Vec<Box<[u8; 512]>> = (0..8192).map(|i| Box::new([i as u8; 512])).collect();
    let mut text = String::new();
    for block in blocks.iter().step_by(1024) {
        text.push(char::from(b'a' + block[0] % 26));
    }

    assert_eq!(numbers.len(), 1 << 20);
    assert_eq!(names.len(), 4096);
    assert_eq!(names[&4095], "node-4095");
    assert!(blocks.iter().enumerate().all(|(i, b)| b[511] == i as u8));
    assert_eq!(text.len(), 8);

    let (free_peak, _) = memory::frame::memory_info();
    let (vmalloc_bytes, vmalloc_blocks) = memory::vmalloc::stats();
    let numbers_addr = numbers.as_ptr() as usize;
    let numbers_phys = memory::vmalloc::translate(numbers_addr);
    let mut grown: Vec<u8> = Vec::with_capacity(300 * 1024);
    let before = grown.as_ptr() as usize;
    for round in 0..64u32 {
        grown.extend_from_slice(&[round as u8; 64 * 1024]);
    }
    assert!(grown.chunks(64 * 1024).enumerate().all(|(i, c)| c.iter().all(|&b| b == i as u8)));
    let moved = before != grown.as_ptr() as usize;
    drop(grown);
    drop(numbers);
    drop(names);
    drop(blocks);
    let trimmed = memory::heap_trim();
    let (free_after, _) = memory::frame::memory_info();
    let (heap_free, heap_total) = memory::heap_stats();
    kprintln!(
        "vmalloc: {} KiB in {} block(s) at peak, 8 MiB vec at {:#x} -> phys {:#x}, 4 MiB vec grown without copying (remapped to a new range: {})",
        vmalloc_bytes / 1024,
        vmalloc_blocks,
        numbers_addr,
        numbers_phys.unwrap_or(0),
        moved
    );
    kprintln!(
        "heap self-test ok: checksum {:#018x}, peak {} KiB, trimmed {} KiB, heap {} KiB ({} KiB free), frames held {}",
        checksum,
        (free_before - free_peak) / 1024,
        trimmed / 1024,
        heap_total / 1024,
        heap_free / 1024,
        free_before.saturating_sub(free_after) / memory::frame::PAGE_SIZE
    );
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::disable_interrupts();
    kprintln!("[KERNEL PANIC] {}", info);
    loop {
        arch::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("allocation failed: {:?}", layout);
}
