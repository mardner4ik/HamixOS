#![no_std]
#![no_main]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![cfg_attr(target_arch = "x86_64", feature(alloc_error_handler))]
#![crate_type = "bin"]

#[cfg(target_arch = "x86_64")]
extern crate alloc;

mod arch;

#[cfg(target_arch = "x86_64")]
mod drivers;
#[cfg(target_arch = "x86_64")]
mod fs;
#[cfg(target_arch = "x86_64")]
mod hsh;
#[cfg(target_arch = "x86_64")]
mod memory;
#[cfg(target_arch = "x86_64")]
mod syscall;
#[cfg(target_arch = "x86_64")]
mod task;
#[cfg(target_arch = "x86_64")]
mod users;

#[cfg(target_arch = "x86_64")]
mod x86_64_main {
    use core::fmt;
    use core::panic::PanicInfo;

    use crate::{arch, drivers, fs, hsh, memory, syscall, task, users};

    #[used]
    #[unsafe(link_section = ".multiboot_header")]
    static MULTIBOOT_HEADER: [u32; 6] = {
        let magic: u32 = 0xe85250d6;
        let arch: u32 = 0;
        let length: u32 = 24;
        let checksum: u32 = (0u32).wrapping_sub(magic.wrapping_add(arch).wrapping_add(length));
        [magic, arch, length, checksum, 0, 8]
    };

    const MBI_BUF_SIZE: usize = 8192;

    #[unsafe(no_mangle)]
    pub extern "C" fn rust_main(multiboot_magic: u32, multiboot_info_ptr: usize) -> ! {
        if multiboot_magic != 0x36d76289 {
            loop {
                arch::x86_64::hlt();
            }
        }

        let mut mbi_copy = [0u8; MBI_BUF_SIZE];
        unsafe {
            snapshot_multiboot_info(multiboot_info_ptr, &mut mbi_copy);
        }

        unsafe {
            arch::x86_64::disable_interrupts();
            zero_bss();
        }

        drivers::serial::init();
        serial_println!("boot: start");
        arch::x86_64::gdt::init();
        drivers::klog::log("boot: gdt");
        arch::x86_64::idt::init();
        drivers::klog::log("boot: idt");
        memory::init(mbi_copy.as_ptr() as usize);
        drivers::klog::log("boot: memory");
        serial_println!("{}", drivers::video::intel_penryn::info_string());
        drivers::klog::log("boot: video");
        drivers::klog::log("boot: serial");
        fs::init();
        drivers::klog::log("boot: fs");
        if let Some(module) = memory::modules()[0] {
            let addr = module.start as usize;
            let size = (module.end - module.start) as usize;
            serial_println!("boot: initramfs at {:#x}, {} bytes", addr, size);
            fs::load_initramfs(addr, size);
        } else {
            serial_println!("boot: no initramfs module found");
        }
        drivers::klog::log("boot: initramfs");
        if let Some(disk) = memory::modules()[1] {
            let addr = disk.start as usize;
            let size = (disk.end - disk.start) as usize;
            serial_println!("boot: disk image module at {:#x}, {} bytes", addr, size);
            *fs::DISK_IMAGE.lock() = Some((addr, size));
        }
        users::init();
        drivers::klog::log("boot: users");
        drivers::input::keyboard::init();
        drivers::klog::log("boot: keyboard");
        syscall::init();
        drivers::klog::log("boot: syscall");
        task::init();
        drivers::klog::log("boot: task");
        arch::x86_64::enable_interrupts();

        hsh::run_login();
    }

    unsafe fn snapshot_multiboot_info(ptr: usize, dst: &mut [u8; MBI_BUF_SIZE]) {
        unsafe {
            let total_size = core::ptr::read_unaligned(ptr as *const u32) as usize;
            let len = total_size.min(MBI_BUF_SIZE);
            core::ptr::copy_nonoverlapping(ptr as *const u8, dst.as_mut_ptr(), len);
            if total_size > MBI_BUF_SIZE {
                let clamped = MBI_BUF_SIZE as u32;
                core::ptr::write_unaligned(dst.as_mut_ptr() as *mut u32, clamped);
            }
        }
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

    struct FixedWriter<'a> {
        buf: &'a mut [u8],
        len: usize,
    }

    impl<'a> fmt::Write for FixedWriter<'a> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let bytes = s.as_bytes();
            let space = self.buf.len() - self.len;
            let take = bytes.len().min(space);
            self.buf[self.len..self.len + take].copy_from_slice(&bytes[..take]);
            self.len += take;
            Ok(())
        }
    }

    static PANICKING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        use core::fmt::Write;
        use drivers::serial::SERIAL;

        arch::x86_64::disable_interrupts();

        if PANICKING.swap(true, core::sync::atomic::Ordering::SeqCst) {
            // We're panicking while already panicking -- something in this very
            // print/draw path just faulted. CLI doesn't mask CPU exceptions like
            // #GP, so retrying the same broken call would recurse forever and
            // eat the kernel stack. Stop now so the *first* panic message (the
            // real one) stays on screen instead of being overwritten.
            loop {
                arch::x86_64::hlt();
            }
        }

        unsafe {
            SERIAL.force_unlock();
        }
        {
            let mut s = SERIAL.lock();
            let _ = writeln!(s, "\n[KERNEL PANIC] {}", info);
        }

        let mut reason_buf = [0u8; 512];
        let reason_len = {
            let mut writer = FixedWriter { buf: &mut reason_buf, len: 0 };
            let _ = write!(writer, "{}", info);
            writer.len
        };
        let reason = core::str::from_utf8(&reason_buf[..reason_len]).unwrap_or("unknown panic");

        drivers::video::text_mode::draw_panic_screen("KERNEL PANIC", reason);

        loop {
            arch::x86_64::hlt();
        }
    }

    #[alloc_error_handler]
    fn alloc_error(layout: core::alloc::Layout) -> ! {
        panic!("allocation failed: {:?}", layout);
    }
}

/// aarch64 is a boot-chain skeleton only (see kernel/src/arch/aarch64): the
/// global_asm boot stub there does the EL-drop, BSS clear, and stack setup,
/// then jumps straight here. None of drivers, fs, hsh, memory, syscall,
/// task, or users are ported yet -- they're all x86_64-only above. This
/// just proves the boot chain works and gives a panic handler so failures
/// are visible instead of silent.
#[cfg(target_arch = "aarch64")]
mod aarch64_main {
    use core::panic::PanicInfo;

    use crate::arch;

    #[unsafe(no_mangle)]
    pub extern "C" fn kernel_main() -> ! {
        arch::aarch64::uart::puts("HamixOS aarch64 skeleton -- alive on QEMU virt (PL011 UART)\n");
        arch::aarch64::uart::puts("Boot chain only: EL-drop -> BSS clear -> UART. Nothing else is ported yet.\n");
        loop {
            arch::aarch64::hlt();
        }
    }

    #[panic_handler]
    fn panic(_info: &PanicInfo) -> ! {
        arch::aarch64::uart::puts("[KERNEL PANIC] aarch64 skeleton\n");
        loop {
            arch::aarch64::hlt();
        }
    }
}
