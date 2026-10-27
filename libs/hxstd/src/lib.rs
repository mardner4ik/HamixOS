#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod audio;
pub mod console;
pub mod display;
pub mod env;
pub mod fs;
pub mod heap;
pub mod io;
pub mod linux_bridge;
pub mod net;
pub mod sys;
pub mod unix;
pub mod users;

#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        pub extern "Rust" fn __hamix_main() -> i32 {
            let f: fn() -> i32 = $main;
            f()
        }
    };
}

unsafe extern "Rust" {
    fn __hamix_main() -> i32;
}

#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov rdi, rsp",
        "and rsp, -16",
        "xor ebp, ebp",
        "call {start}",
        "ud2",
        start = sym runtime_start,
    );
}

#[cfg(target_arch = "aarch64")]
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov x0, sp",
        "and x1, x0, #-16",
        "mov sp, x1",
        "mov x29, #0",
        "mov x30, #0",
        "bl {start}",
        "brk #1",
        start = sym runtime_start,
    );
}

#[cfg(target_arch = "riscv64")]
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mv a0, sp",
        "andi sp, sp, -16",
        "li fp, 0",
        "li ra, 0",
        "call {start}",
        "unimp",
        start = sym runtime_start,
    );
}

extern "C" fn runtime_start(stack: *const u64) -> ! {
    unsafe { env::init(stack) };
    let code = unsafe { __hamix_main() };
    sys::exit(code)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::eprintln!("panic: {}", info);
    sys::exit(101)
}
