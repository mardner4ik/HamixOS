#![no_std]

pub mod nr;

#[cfg(target_os = "hamix")]
pub mod libc;

pub use nr::*;

#[cfg(target_os = "hamix")]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    unsafe { libc::__do_syscall6(num as i64, a1 as i64, a2 as i64, a3 as i64, a4 as i64, a5 as i64, a6 as i64) }
}

#[cfg(all(target_arch = "x86_64", not(target_os = "hamix")))]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[cfg(all(target_arch = "aarch64", not(target_os = "hamix")))]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") num,
            inlateout("x0") a1 as i64 => ret,
            in("x1") a2,
            in("x2") a3,
            in("x3") a4,
            in("x4") a5,
            in("x5") a6,
            options(nostack),
        );
    }
    ret
}

#[cfg(all(target_arch = "riscv64", not(target_os = "hamix")))]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") num,
            inlateout("a0") a1 as i64 => ret,
            in("a1") a2,
            in("a2") a3,
            in("a3") a4,
            in("a4") a5,
            in("a5") a6,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
pub unsafe fn syscall5(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    unsafe { syscall6(num, a1, a2, a3, a4, a5, 0) }
}

#[inline(always)]
pub unsafe fn syscall4(num: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    unsafe { syscall6(num, a1, a2, a3, a4, 0, 0) }
}

#[inline(always)]
pub unsafe fn syscall3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    unsafe { syscall6(num, a1, a2, a3, 0, 0, 0) }
}

#[inline(always)]
pub unsafe fn syscall2(num: u64, a1: u64, a2: u64) -> i64 {
    unsafe { syscall6(num, a1, a2, 0, 0, 0, 0) }
}

#[inline(always)]
pub unsafe fn syscall1(num: u64, a1: u64) -> i64 {
    unsafe { syscall6(num, a1, 0, 0, 0, 0, 0) }
}

#[inline(always)]
pub unsafe fn syscall0(num: u64) -> i64 {
    unsafe { syscall6(num, 0, 0, 0, 0, 0, 0) }
}

pub fn is_error(ret: i64) -> bool {
    (-4095..0).contains(&ret)
}
