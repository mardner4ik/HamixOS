#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "Rust" fn __hamix_main() -> i32 {
    hxutils::exec_shell()
}
