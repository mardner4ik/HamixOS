pub mod acpi;
pub mod context;
pub mod cpuid;
pub mod gdt;
pub mod idt;
pub mod lapic;
pub mod paging;
pub mod platform;
pub mod smp;

#[inline]
pub fn hlt() {
    unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
}

pub fn start_tick(hz: u64) {
    let divisor = (1_193_182 / hz) as u16;
    outb(0x43, 0x36);
    outb(0x40, (divisor & 0xFF) as u8);
    outb(0x40, (divisor >> 8) as u8);
}

#[inline]
pub fn idle_wait() {
    unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
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
pub fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) flags);
    }
    flags & (1 << 9) != 0
}

pub fn without_interrupts<F: FnOnce() -> R, R>(f: F) -> R {
    let was_enabled = interrupts_enabled();
    if was_enabled {
        disable_interrupts();
    }
    let result = f();
    if was_enabled {
        enable_interrupts();
    }
    result
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
pub fn outl(port: u16, val: u32) {
    unsafe {
        core::arch::asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") val,
            options(nomem, nostack)
        );
    }
}

#[inline]
pub fn inl(port: u16) -> u32 {
    let val: u32;
    unsafe {
        core::arch::asm!(
            "in eax, dx",
            out("eax") val,
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

pub fn pit_delay_us(micros: u64) {
    let mut remaining = micros;
    while remaining > 0 {
        let chunk = remaining.min(50_000);
        let count = ((1_193_182 * chunk) / 1_000_000).clamp(1, 0xFFFF) as u16;
        let gate = inb(0x61);
        outb(0x61, (gate & 0xFC) | 0x00);
        outb(0x43, 0xB0);
        outb(0x42, count as u8);
        outb(0x42, (count >> 8) as u8);
        outb(0x61, (gate & 0xFC) | 0x01);
        let mut guard = 0u32;
        while inb(0x61) & 0x20 == 0 {
            guard += 1;
            if guard > 5_000_000 {
                break;
            }
            core::hint::spin_loop();
        }
        outb(0x61, gate & 0xFC);
        remaining -= chunk;
    }
}

pub fn delay_ms(ms: u64) {
    if interrupts_enabled() && crate::task::current_pid() != 0 && crate::task::ticks() > 0 {
        crate::task::sleep_ticks(crate::task::ms_to_ticks(ms));
    } else {
        pit_delay_us(ms * 1000);
    }
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
