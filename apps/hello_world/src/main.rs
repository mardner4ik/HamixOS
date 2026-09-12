#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use hamix_std::{entry, println, sys};

fn main() -> i32 {
    println!("HamixOS userspace: hello from Rust + hamix_std!");

    let numbers = vec![1u32, 2, 3, 4, 5];
    let sum: u32 = numbers.iter().sum();
    println!("computed on the heap (brk syscall): sum = {}", sum);

    println!("uid={} euid={} pid={}", sys::getuid(), sys::geteuid(), sys::getpid());

    0
}

entry!(main);
