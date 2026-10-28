#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod audio;
pub mod console;
pub mod display;
pub mod drm;
pub mod env;
pub mod fs;
pub mod heap;
pub mod input;
pub mod io;
pub mod net;
pub mod permissions;
pub mod sys;
pub mod uevent;
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

#[cfg(all(target_arch = "x86_64", not(target_os = "hamix")))]
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

#[cfg(all(target_arch = "aarch64", not(target_os = "hamix")))]
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

#[cfg(all(target_arch = "riscv64", not(target_os = "hamix")))]
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

#[cfg(target_os = "hamix")]
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: i32, argv: *const *const u8, envp: *const *const u8) -> i32 {
    unsafe {
        hxlibc_sys::libc::signal(hxlibc_sys::libc::SIGPIPE, hxlibc_sys::libc::SIG_IGN);
        env::init_c(argc as usize, argv, envp);
    }
    let code = unsafe { __hamix_main() };
    sys::exit(code)
}

#[cfg(not(target_os = "hamix"))]
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
