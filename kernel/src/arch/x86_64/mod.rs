pub mod gdt;
pub mod idt;
pub mod paging;

#[inline]
pub fn hlt() {
    unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
}

#[inline]
pub fn disable_interrupts() {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)) };
}

#[inline]
pub fn enable_interrupts() {
    unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
}

#[inline]
pub fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") val,
            options(nomem, nostack)
        );
    }
}

#[inline]
pub fn inb(port: u16) -> u8 {
    let val: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") val,
            in("dx") port,
            options(nomem, nostack)
        );
    }
    val
}

#[inline]
pub fn outw(port: u16, val: u16) {
    unsafe {
        core::arch::asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") val,
            options(nomem, nostack)
        );
    }
}

#[inline]
pub fn inw(port: u16) -> u16 {
    let val: u16;
    unsafe {
        core::arch::asm!(
            "in ax, dx",
            out("ax") val,
            in("dx") port,
            options(nomem, nostack)
        );
    }
    val
}

#[inline]
pub fn io_wait() {
    outb(0x80, 0);
}

pub fn read_msr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

pub fn write_msr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") lo,
            in("edx") hi,
            options(nomem, nostack)
        );
    }
}
