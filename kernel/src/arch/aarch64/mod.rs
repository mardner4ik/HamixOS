pub mod context;
pub mod efi;
pub mod gic;
pub mod mmu;
pub mod paging;
pub mod platform;
pub mod timer;
pub mod traps;

const RAM_BASE: usize = 0x4000_0000;

core::arch::global_asm!(
    r#"
    .section .text.boot
    .global _start
_start:
    add x13, x18, #0x16
    b hamix_entry
    .quad 0x200000
    .quad __kernel_image_size
    .quad 0xA
    .quad 0
    .quad 0
    .quad 0
    .ascii "ARM\x64"
    .long hamix_pe_header - _start
hamix_pe_header:
    .ascii "PE\0\0"
    .short 0xAA64
    .short 1
    .long 0
    .long 0
    .long 0
    .short hamix_section_table - hamix_optional_header
    .short 0x206
hamix_optional_header:
    .short 0x20B
    .byte 2, 20
    .long __efi_raw_size
    .long 0
    .long 0
    .long __efi_entry_rva
    .long 0x1000
    .quad 0
    .long 0x1000
    .long 0x1000
    .short 0, 0, 0, 0, 0, 0
    .long 0
    .long __kernel_image_size
    .long 0x1000
    .long 0
    .short 10
    .short 0
    .quad 0, 0, 0, 0
    .long 0
    .long 6
    .quad 0, 0, 0, 0, 0, 0
hamix_section_table:
    .ascii ".text\0\0\0"
    .long __efi_virtual_size
    .long 0x1000
    .long __efi_raw_size
    .long 0x1000
    .long 0
    .long 0
    .short 0
    .short 0
    .long 0xE0000020
hamix_entry:
    mov x20, x1
    mov x21, x2
    mov x22, x3
    mov x19, x0
    mrs x1, mpidr_el1
    and x1, x1, #0xffff
    cbnz x1, hamix_park
    mrs x0, CurrentEL
    lsr x0, x0, #2
    cmp x0, #3
    b.eq hamix_el3_entry
    cmp x0, #2
    b.eq hamix_el2_entry
    b hamix_el1_entry

hamix_el3_entry:
    mov x0, #0x431
    msr scr_el3, x0
    mov x0, #0x3c5
    msr spsr_el3, x0
    adr x0, hamix_el1_entry
    msr elr_el3, x0
    eret

hamix_el2_entry:
    mov x0, #3
    msr cnthctl_el2, x0
    msr cntvoff_el2, xzr
    mrs x0, id_aa64pfr0_el1
    ubfx x0, x0, #24, #4
    cbz x0, hamix_el2_no_gicv3
    mov x0, #0xf
    msr S3_4_C12_C9_5, x0
    isb
hamix_el2_no_gicv3:
    mov x0, #0x80000000
    msr hcr_el2, x0
    mov x0, #0x3c5
    msr spsr_el2, x0
    adr x0, hamix_el1_entry
    msr elr_el2, x0
    eret

hamix_el1_entry:
    mov x0, #0x0800
    movk x0, #0x30d0, lsl #16
    msr sctlr_el1, x0
    mov x0, #(3 << 20)
    msr cpacr_el1, x0
    isb
    adrp x1, __stack_top
    add  x1, x1, :lo12:__stack_top
    mov  sp, x1
    adrp x1, __bss_start
    add  x1, x1, :lo12:__bss_start
    adrp x2, __bss_end
    add  x2, x2, :lo12:__bss_end
hamix_clear_bss:
    cmp x1, x2
    b.ge hamix_bss_cleared
    str xzr, [x1], #8
    b hamix_clear_bss
hamix_bss_cleared:
    mov x0, x19
    mov x1, x20
    mov x2, x21
    mov x3, x22
    bl kernel_main
hamix_park:
    wfe
    b hamix_park
    "#
);

pub const EFI_BOOT_MAGIC: u64 = 0x4858_4D49_4E49_5444;

#[unsafe(no_mangle)]
extern "C" fn kernel_main(dtb: usize, initrd_start: u64, initrd_end: u64, magic: u64) -> ! {
    mmu::init();
    let dtb = if dtb != 0 { dtb } else { RAM_BASE };
    if magic == EFI_BOOT_MAGIC && initrd_end > initrd_start {
        crate::memory::set_boot_initrd(initrd_start, initrd_end);
    }
    crate::early::start(dtb, 0)
}

#[inline(always)]
pub fn hlt() {
    unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
}

#[inline(always)]
pub fn idle_wait() {
    unsafe { core::arch::asm!("wfi", "msr daifclr, #2", "isb", options(nomem, nostack)) };
}

#[inline(always)]
pub fn disable_interrupts() {
    unsafe { core::arch::asm!("msr daifset, #2", options(nomem, nostack)) };
}

#[inline(always)]
pub fn enable_interrupts() {
    unsafe { core::arch::asm!("msr daifclr, #2", options(nomem, nostack)) };
}

#[inline(always)]
pub fn interrupts_enabled() -> bool {
    let daif: u64;
    unsafe { core::arch::asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack)) };
    daif & (1 << 7) == 0
}

pub fn current_el() -> u64 {
    let el: u64;
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack)) };
    (el >> 2) & 3
}
