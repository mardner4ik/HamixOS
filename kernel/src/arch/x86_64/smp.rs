use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

use super::{lapic, read_msr, write_msr};

pub const MAX_CPUS: usize = 16;
const TRAMPOLINE_BASE: u64 = 0x7000;
const AP_STACK_SIZE: usize = 64 * 1024;
const AP_DF_STACK_SIZE: usize = 16 * 1024;
const IA32_GS_BASE: u32 = 0xC000_0101;
const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

#[repr(C)]
pub struct PerCpu {
    pub kernel_rsp: u64,
    pub scratch: u64,
    pub cpu: u64,
}

pub static mut PER_CPU: [PerCpu; MAX_CPUS] = [const { PerCpu { kernel_rsp: 0, scratch: 0, cpu: 0 } }; MAX_CPUS];

static CPU_COUNT: AtomicUsize = AtomicUsize::new(1);
static ONLINE: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
static APIC_IDS: [AtomicU8; MAX_CPUS] = [const { AtomicU8::new(0) }; MAX_CPUS];
static AP_BOOTED: AtomicBool = AtomicBool::new(false);
static AP_CPU: AtomicUsize = AtomicUsize::new(0);
pub static BUSY_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
pub static TOTAL_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

unsafe extern "C" {
    static ap_trampoline: u8;
    static ap_trampoline_end: u8;
    static ap_slot_cr3: u8;
    static ap_slot_stack: u8;
    static ap_slot_entry: u8;
}

#[inline(always)]
pub fn cpu_id() -> usize {
    let id: u64;
    unsafe { core::arch::asm!("mov {}, gs:[16]", out(reg) id, options(nostack, preserves_flags, readonly)) };
    id as usize
}

fn install_percpu(cpu: usize) {
    unsafe {
        let per = ((&raw mut PER_CPU) as *mut PerCpu).add(cpu);
        (*per).cpu = cpu as u64;
        write_msr(IA32_GS_BASE, per as u64);
        write_msr(IA32_KERNEL_GS_BASE, per as u64);
    }
}

pub fn early_init() {
    install_percpu(0);
    ONLINE[0].store(true, Ordering::Relaxed);
}

pub fn percpu_ptr(cpu: usize) -> *mut PerCpu {
    unsafe { ((&raw mut PER_CPU) as *mut PerCpu).add(cpu) }
}

pub fn set_syscall_stack(top: u64) {
    unsafe { (*percpu_ptr(cpu_id())).kernel_rsp = top };
}

pub fn count() -> usize {
    CPU_COUNT.load(Ordering::Relaxed)
}

pub fn online(cpu: usize) -> bool {
    cpu < MAX_CPUS && ONLINE[cpu].load(Ordering::Relaxed)
}

pub fn apic_id_of(cpu: usize) -> u8 {
    APIC_IDS[cpu.min(MAX_CPUS - 1)].load(Ordering::Relaxed)
}

pub fn kick_idle(except: usize) {
    if count() < 2 {
        return;
    }
    for cpu in 0..count() {
        if cpu != except && online(cpu) && crate::task::cpu_is_idle(cpu) {
            lapic::send_ipi(apic_id_of(cpu), lapic::WAKE_VECTOR);
        }
    }
}

fn prepare_trampoline(stack_top: u64) {
    unsafe {
        let start = &raw const ap_trampoline as usize;
        let end = &raw const ap_trampoline_end as usize;
        core::ptr::copy_nonoverlapping(start as *const u8, TRAMPOLINE_BASE as *mut u8, end - start);
        let slot = |sym: *const u8| TRAMPOLINE_BASE + (sym as u64 - start as u64);
        core::ptr::write_unaligned(slot(&raw const ap_slot_cr3) as *mut u32, super::paging::kernel_cr3() as u32);
        core::ptr::write_unaligned(slot(&raw const ap_slot_stack) as *mut u64, stack_top);
        core::ptr::write_unaligned(slot(&raw const ap_slot_entry) as *mut u64, ap_entry as *const () as u64);
    }
}

fn alloc_stack(size: usize) -> Option<u64> {
    let ptr = unsafe { alloc::alloc::alloc(core::alloc::Layout::from_size_align_unchecked(size, 16)) };
    if ptr.is_null() { None } else { Some((ptr as u64 + size as u64) & !0xF) }
}

pub fn start_aps(hz: u64) -> usize {
    if !lapic::present() {
        return 1;
    }
    let madt = super::acpi::madt();
    let hint = madt.as_ref().map(|m| m.lapic_address).unwrap_or(0);
    lapic::enable(true, hint);
    lapic::calibrate(hz);
    let bsp_apic = lapic::id();
    APIC_IDS[0].store(bsp_apic, Ordering::Relaxed);
    let Some(madt) = madt else {
        return 1;
    };
    if crate::memory::cmdline_value("nosmp").is_some() || crate::memory::cmdline().split_whitespace().any(|w| w == "nosmp") {
        return 1;
    }
    let limit = crate::memory::cmdline_value("maxcpus").and_then(|v| v.parse::<usize>().ok()).unwrap_or(MAX_CPUS).clamp(1, MAX_CPUS);
    let mut next = 1usize;
    for processor in madt.processors.iter() {
        if !processor.enabled || processor.apic_id == bsp_apic || next >= limit {
            continue;
        }
        let Some(stack_top) = alloc_stack(AP_STACK_SIZE) else {
            break;
        };
        AP_CPU.store(next, Ordering::SeqCst);
        AP_BOOTED.store(false, Ordering::SeqCst);
        APIC_IDS[next].store(processor.apic_id, Ordering::Relaxed);
        prepare_trampoline(stack_top);
        lapic::send_init(processor.apic_id);
        super::pit_delay_us(10_000);
        let vector = (TRAMPOLINE_BASE >> 12) as u8;
        let mut booted = false;
        for _ in 0..2 {
            lapic::send_startup(processor.apic_id, vector);
            for _ in 0..200 {
                if AP_BOOTED.load(Ordering::SeqCst) {
                    booted = true;
                    break;
                }
                super::pit_delay_us(1000);
            }
            if booted {
                break;
            }
        }
        if booted {
            next += 1;
            CPU_COUNT.store(next, Ordering::SeqCst);
        } else {
            crate::drivers::klog::log(&alloc::format!("smp: processor with APIC id {} did not start", processor.apic_id));
        }
    }
    next
}

extern "C" fn ap_entry() -> ! {
    let cpu = AP_CPU.load(Ordering::SeqCst);
    install_percpu(cpu);
    let df_top = alloc_stack(AP_DF_STACK_SIZE).unwrap_or(0);
    super::gdt::init_ap(cpu, df_top, 0);
    super::idt::load();
    super::paging::init_ap();
    crate::syscall::init_cpu();
    lapic::enable(false, 0);
    ONLINE[cpu].store(true, Ordering::SeqCst);
    AP_BOOTED.store(true, Ordering::SeqCst);
    lapic::start_timer();
    crate::task::ap_idle_loop(cpu)
}

pub fn record_tick(cpu: usize, busy: bool) {
    if cpu < MAX_CPUS {
        TOTAL_TICKS[cpu].fetch_add(1, Ordering::Relaxed);
        if busy {
            BUSY_TICKS[cpu].fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn read_gs_base() -> u64 {
    read_msr(IA32_GS_BASE)
}
