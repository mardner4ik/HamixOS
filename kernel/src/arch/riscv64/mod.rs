pub mod context;
pub mod paging;
pub mod platform;
pub mod plic;
pub mod sbi;
pub mod timer;
pub mod traps;

core::arch::global_asm!(
    r#"
    .section .text.boot
    .global _start
_start:
    csrw sie, zero
    la t0, hamix_park
    csrw stvec, t0
    la t0, hamix_boot_lottery
    li t1, 1
    amoadd.w t1, t1, (t0)
    bnez t1, hamix_park
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    la sp, __stack_top
    la t0, __bss_start
    la t1, __bss_end
hamix_clear_bss:
    bgeu t0, t1, hamix_bss_cleared
    sd zero, (t0)
    addi t0, t0, 8
    j hamix_clear_bss
hamix_bss_cleared:
    call kernel_main
    .balign 4
hamix_park:
    wfi
    j hamix_park

    .section .data
    .balign 4
hamix_boot_lottery:
    .word 0
    "#
);

#[unsafe(no_mangle)]
extern "C" fn kernel_main(hart: usize, dtb: usize) -> ! {
    unsafe { core::arch::asm!("csrs sstatus, {}", in(reg) 3usize << 13, options(nostack)) };
    crate::early::start(dtb, hart)
}

#[inline(always)]
pub fn idle_wait() {
    unsafe { core::arch::asm!("wfi", "csrsi sstatus, 2", options(nomem, nostack)) };
}

#[inline(always)]
pub fn hlt() {
    unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
}

#[inline(always)]
pub fn disable_interrupts() {
    unsafe { core::arch::asm!("csrci sstatus, 2", options(nomem, nostack)) };
}

#[inline(always)]
pub fn enable_interrupts() {
    unsafe { core::arch::asm!("csrsi sstatus, 2", options(nomem, nostack)) };
}

#[inline(always)]
pub fn interrupts_enabled() -> bool {
    let sstatus: usize;
    unsafe { core::arch::asm!("csrr {}, sstatus", out(reg) sstatus, options(nomem, nostack)) };
    sstatus & 2 != 0
}
