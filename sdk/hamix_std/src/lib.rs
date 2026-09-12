#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod heap;
pub mod io;
pub mod sys;

#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _start() -> ! {
            let f: fn() -> i32 = $main;
            let code = f();
            // `_start` is reached via a direct ring3 entry (iretq), not a
            // normal `call` -- and both this function and sys::exit() are
            // `-> !`. Without a barrier here, LLVM was found (empirically,
            // by testing) to sometimes miscompile the handoff of `code`
            // into the exit() call in that specific shape, resulting in
            // exit() being invoked with a garbage value despite `code`
            // being correct right up to this point. black_box forces the
            // value through an opaque boundary so the optimizer can't
            // fuse/reorder its way into that state.
            let code = core::hint::black_box(code);
            $crate::sys::exit(code);
        }
    };
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::eprintln!("hamix_std: panic: {}", info);
    sys::exit(101)
}
