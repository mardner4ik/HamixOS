#![no_std]
#![no_main]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(alloc_error_handler)]
#![cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
#![crate_type = "bin"]

extern crate alloc;

mod arch;

mod drivers;
mod fs;
mod hxinit;
mod login;
mod net;
mod memory;
mod module;
#[cfg(not(target_arch = "x86_64"))]
mod early;
#[cfg(not(target_arch = "x86_64"))]
mod fdt;
#[cfg(not(target_arch = "x86_64"))]
mod platform;
mod syscall;
mod random;
mod task;
mod users;
mod vt;

#[cfg(target_arch = "x86_64")]
mod x86_64_main {
    use core::fmt;
    use core::panic::PanicInfo;

    use crate::{arch, drivers, hxinit, memory, syscall, task};

    // Requests a linear graphics-mode framebuffer from GRUB at boot (see
    // memory::init's tag-8 handling). The console
    // keeps its character cells in kernel RAM and renders them with an 8x8
    // bitmap font scaled to the framebuffer (drivers::video::text_mode); it
    // only falls back to the legacy 0xB8000 text buffer when no framebuffer
    // was handed to us, since that window stops decoding once the hardware
    // leaves text mode.
    #[repr(C, packed)]
    struct Mb2FramebufferTag {
        typ: u16,
        flags: u16,
        size: u32,
        width: u32,
        height: u32,
        depth: u32,
        _pad: u32,
    }

    #[repr(C, packed)]
    struct Mb2EndTag {
        typ: u32,
        size: u32,
    }

    #[repr(C, packed)]
    struct MultibootHeader {
        magic: u32,
        arch: u32,
        length: u32,
        checksum: u32,
        fb_tag: Mb2FramebufferTag,
        end_tag: Mb2EndTag,
    }

    #[used]
    #[unsafe(link_section = ".multiboot_header")]
    static MULTIBOOT_HEADER: MultibootHeader = {
        let magic: u32 = 0xe85250d6;
        let arch: u32 = 0;
        let length: u32 = core::mem::size_of::<MultibootHeader>() as u32;
        let checksum: u32 = (0u32).wrapping_sub(magic.wrapping_add(arch).wrapping_add(length));
        MultibootHeader {
            magic,
            arch,
            length,
            checksum,
            fb_tag: Mb2FramebufferTag {
                typ: 5,
                flags: 0,
                size: 20,
                width: 1024,
                height: 768,
                depth: 32,
                _pad: 0,
            },
            end_tag: Mb2EndTag { typ: 0, size: 8 },
        }
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
        arch::x86_64::smp::early_init();
        task::bkl::acquire();

        drivers::serial::init();
        crate::serial_println!("boot: start");
        // Must run before anything logs via drivers::klog, which now
        // allocates (owned Strings) so it can record real hardware-detected
        // messages -- see memory::early_heap_init's doc comment.
        memory::early_heap_init();
        arch::x86_64::gdt::init();
        drivers::klog::log("boot: gdt");
        arch::x86_64::idt::init();
        drivers::klog::log("boot: idt");
        {
            let cpu = arch::x86_64::cpuid::identify();
            let line = if cpu.brand.is_empty() {
                alloc::format!("cpu: {} (family {}, model {}, stepping {})", cpu.vendor, cpu.family, cpu.model, cpu.stepping)
            } else {
                alloc::format!("cpu: {}", cpu.brand)
            };
            drivers::klog::log(&line);
        }
        memory::init(mbi_copy.as_ptr() as usize);
        arch::x86_64::paging::init();
        drivers::klog::log("boot: memory");
        drivers::video::text_mode::cache_framebuffer();
        if let Some(fb) = *memory::FRAMEBUFFER.lock() {
            arch::x86_64::paging::enable_kernel_write_combining(fb.addr, fb.byte_len());
        }
        drivers::video::text_mode::fb_clear_full();
        {
            let (free, total) = memory::frame::memory_info();
            hxinit::record("memory", "Physical memory", hxinit::ok(alloc::format!("{} MiB, {} MiB free, kernel heap grows on demand", total >> 20, free >> 20)));
        }
        hxinit::record("cpu", "Processor", hxinit::ok({
            let cpu = arch::x86_64::cpuid::identify();
            if cpu.brand.is_empty() { cpu.vendor } else { alloc::string::String::from(cpu.brand.trim()) }
        }));
        hxinit::record("interrupts", "Interrupts and exceptions", hxinit::ok("GDT, IDT, PIC, per-CPU TSS"));
        hxinit::record("framebuffer", "Framebuffer console", match *memory::FRAMEBUFFER.lock() {
            Some(fb) => hxinit::ok(alloc::format!("{}x{}x{}{}", fb.width, fb.height, fb.bpp, if arch::x86_64::paging::write_combining() { ", write-combining" } else { "" })),
            None => hxinit::warn("VGA text mode"),
        });
        syscall::init();
        task::init();
        hxinit::start();
        task::idle_loop();
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
