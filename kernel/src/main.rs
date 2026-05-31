#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![crate_type = "bin"]

extern crate alloc;

use core::panic::PanicInfo;

mod arch;
mod drivers;
mod memory;
mod syscall;
mod task;

#[used]
#[unsafe(link_section = ".multiboot_header")]
static MULTIBOOT_HEADER: [u32; 4] = {
    let magic: u32 = 0xe85250d6;
    let arch: u32 = 0;
    let length: u32 = 16;
    let checksum: u32 = (0u32).wrapping_sub(magic.wrapping_add(arch).wrapping_add(length));
    [magic, arch, length, checksum]
};

#[unsafe(no_mangle)]
pub extern "C" fn _start(multiboot_info_ptr: u32) -> ! {
    unsafe { 
        arch::x86_64::disable_interrupts();
        zero_bss();
    }

    arch::x86_64::gdt::init();
    arch::x86_64::idt::init();
    memory::init(multiboot_info_ptr as usize);
    drivers::video::vesa::init();
    drivers::serial::init();
    drivers::input::keyboard::init();
    syscall::init();
    task::init();

    drivers::console::run_login();
}

unsafe fn zero_bss() {
    unsafe {
        unsafe extern "C" {
            static mut __bss_start: u8;
            static mut __bss_end: u8;
        }
        let start = core::ptr::addr_of_mut!(__bss_start);
        let end = core::ptr::addr_of_mut!(__bss_end);
        let len = end as usize - start as usize;
        core::ptr::write_bytes(start, 0, len);
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;
    use drivers::serial::SERIAL;
    use drivers::console::CONSOLE;

    if let Some(mut s) = SERIAL.try_lock() {
        let _ = writeln!(s, "\n[KERNEL PANIC] {}", info);
    }
    if let Some(mut c) = CONSOLE.try_lock() {
        let _ = writeln!(c, "\n[KERNEL PANIC] {}", info);
    }
    loop {
        arch::x86_64::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("allocation failed: {:?}", layout);
}
