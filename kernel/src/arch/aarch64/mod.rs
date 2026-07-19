pub mod uart;

// Entry point + minimal EL3/EL2 -> EL1 drop, BSS clear, stack setup, then a
// jump into Rust at kernel_main. QEMU's `virt` machine already starts a
// plain -kernel ELF at EL1 in the common case, but this handles booting
// under -machine virtualization=on/secure=on too instead of assuming EL1.
core::arch::global_asm!(
    r#"
    .section .text.boot
    .global _start
_start:
    mrs x0, CurrentEL
    lsr x0, x0, #2
    cmp x0, #3
    b.eq el3_entry
    cmp x0, #2
    b.eq el2_entry
    b el1_entry

el3_entry:
    mov x0, #0x431
    msr scr_el3, x0
    mov x0, #0x3c5
    msr spsr_el3, x0
    adr x0, el1_entry
    msr elr_el3, x0
    eret

el2_entry:
    mov x0, #0x80000000
    msr hcr_el2, x0
    mov x0, #0x3c5
    msr spsr_el2, x0
    adr x0, el1_entry
    msr elr_el2, x0
    eret

el1_entry:
    adrp x1, __stack_top
    add  x1, x1, :lo12:__stack_top
    mov  sp, x1

    adrp x1, __bss_start
    add  x1, x1, :lo12:__bss_start
    adrp x2, __bss_end
    add  x2, x2, :lo12:__bss_end
clear_bss:
    cmp x1, x2
    b.ge bss_cleared
    str xzr, [x1], #8
    b clear_bss
bss_cleared:
    bl kernel_main
hang:
    wfe
    b hang
    "#
);

#[inline(always)]
pub fn hlt() {
    unsafe { core::arch::asm!("wfe") };
}

#[inline(always)]
#[allow(dead_code)]
pub fn disable_interrupts() {
    unsafe { core::arch::asm!("msr daifset, #2") };
}

#[inline(always)]
#[allow(dead_code)]
pub fn enable_interrupts() {
    unsafe { core::arch::asm!("msr daifclr, #2") };
}
