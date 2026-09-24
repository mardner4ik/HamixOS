pub mod io;
#[cfg(target_arch = "x86_64")]
pub mod x86_64;
#[cfg(target_arch = "x86_64")]
#[allow(unused_imports)]
pub use x86_64::{context, platform, disable_interrupts, enable_interrupts, hlt, idle_wait, interrupts_enabled, paging, smp, without_interrupts};
#[cfg(target_arch = "x86_64")]
pub use x86_64::{delay_ms, pit_delay_us as delay_us, start_tick};
#[cfg(target_arch = "x86_64")]
pub const HEAP_LIMIT: usize = 1 << 32;
#[cfg(target_arch = "x86_64")]
pub use x86_64::paging::pte;
#[cfg(target_arch = "x86_64")]
pub const MACHINE: &str = "x86_64";

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use aarch64::{context, platform, disable_interrupts, enable_interrupts, hlt, idle_wait, interrupts_enabled};
#[cfg(target_arch = "aarch64")]
pub const MACHINE: &str = "aarch64";

#[cfg(target_arch = "riscv64")]
pub mod riscv64;
#[cfg(target_arch = "riscv64")]
pub use riscv64::{context, platform, disable_interrupts, enable_interrupts, hlt, idle_wait, interrupts_enabled};
#[cfg(target_arch = "riscv64")]
pub const MACHINE: &str = "riscv64";

#[cfg(not(target_arch = "x86_64"))]
pub const HEAP_LIMIT: usize = usize::MAX;

#[cfg(not(target_arch = "x86_64"))]
pub mod irqtab;
#[cfg(not(target_arch = "x86_64"))]
pub mod up;
#[cfg(not(target_arch = "x86_64"))]
pub use up as smp;
#[cfg(not(target_arch = "x86_64"))]
pub fn delay_us(micros: u64) {
    let end = timer::now_ns().saturating_add(micros.saturating_mul(1000));
    while timer::now_ns() < end {
        core::hint::spin_loop();
    }
}
#[cfg(not(target_arch = "x86_64"))]
pub fn delay_ms(ms: u64) {
    if interrupts_enabled() && crate::task::current_pid() != 0 && crate::task::ticks() > 0 {
        crate::task::sleep_ticks(crate::task::ms_to_ticks(ms));
    } else {
        delay_us(ms * 1000);
    }
}
#[cfg(not(target_arch = "x86_64"))]
pub fn start_tick(hz: u64) {
    timer::set_hz(hz as u32);
}

#[cfg(target_arch = "aarch64")]
pub use aarch64::{gic as irq, paging, paging::pte, timer, traps};
#[cfg(target_arch = "riscv64")]
pub use riscv64::{paging, paging::pte, plic as irq, timer, traps};

#[cfg(not(target_arch = "x86_64"))]
pub mod kmap {
    pub use super::paging::flush_tlb as flush_local;

    pub const VMALLOC_BASE: usize = super::paging::VMALLOC_BASE as usize;
    pub const VMALLOC_SIZE: usize = super::paging::VMALLOC_SIZE as usize;
    pub const MAX_CPUS: usize = super::smp::MAX_CPUS;

    pub fn ready() -> bool {
        super::paging::vmalloc_ready()
    }

    pub fn map(virt: usize, phys: usize) -> bool {
        super::paging::kernel_map_page(virt as u64, phys as u64)
    }

    pub fn unmap(virt: usize) -> Option<usize> {
        super::paging::kernel_unmap_page(virt as u64).map(|p| p as usize)
    }

    pub fn translate(virt: usize) -> Option<usize> {
        super::paging::kernel_translate(virt as u64).map(|p| p as usize)
    }

    pub fn cpu_id() -> usize {
        super::smp::cpu_id()
    }

    pub fn cpu_online(cpu: usize) -> bool {
        super::smp::online(cpu)
    }
}

#[cfg(not(target_arch = "x86_64"))]
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

#[cfg(target_arch = "x86_64")]
pub mod kmap {
    use super::x86_64::{paging, smp};

    pub const VMALLOC_BASE: usize = paging::VMALLOC_BASE as usize;
    pub const VMALLOC_SIZE: usize = paging::VMALLOC_SIZE as usize;
    pub const MAX_CPUS: usize = smp::MAX_CPUS;

    pub fn ready() -> bool {
        paging::vmalloc_ready()
    }

    pub fn map(virt: usize, phys: usize) -> bool {
        paging::kernel_map_page(virt as u64, phys as u64)
    }

    pub fn unmap(virt: usize) -> Option<usize> {
        paging::kernel_unmap_page(virt as u64).map(|p| p as usize)
    }

    pub fn translate(virt: usize) -> Option<usize> {
        paging::kernel_translate(virt as u64).map(|p| p as usize)
    }

    pub fn flush_local() {
        paging::flush_tlb();
    }

    pub fn cpu_id() -> usize {
        smp::cpu_id()
    }

    pub fn cpu_online(cpu: usize) -> bool {
        smp::online(cpu)
    }
}
