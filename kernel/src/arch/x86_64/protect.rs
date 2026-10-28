use core::arch::x86_64::__cpuid_count;
use core::sync::atomic::{AtomicBool, Ordering};

const CR4_UMIP: u64 = 1 << 11;
const CR4_SMEP: u64 = 1 << 20;

static SMEP: AtomicBool = AtomicBool::new(false);
static UMIP: AtomicBool = AtomicBool::new(false);

fn supported() -> (bool, bool) {
    if __cpuid_count(0, 0).eax < 7 {
        return (false, false);
    }
    let leaf = __cpuid_count(7, 0);
    (leaf.ebx & (1 << 7) != 0, leaf.ecx & (1 << 2) != 0)
}

pub fn enable() {
    let (smep, umip) = supported();
    let off = |key: &str| crate::memory::cmdline().split_whitespace().any(|w| w == key);
    let smep = smep && !off("nosmep");
    let umip = umip && !off("noumip");
    let mut bits = 0;
    if smep {
        bits |= CR4_SMEP;
    }
    if umip {
        bits |= CR4_UMIP;
    }
    if bits != 0 {
        unsafe {
            let mut cr4: u64;
            core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
            cr4 |= bits;
            core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
        }
    }
    SMEP.store(smep, Ordering::Relaxed);
    UMIP.store(umip, Ordering::Relaxed);
}

pub fn protect_kernel() -> bool {
    if crate::memory::cmdline().split_whitespace().any(|w| w == "nokwx") {
        return false;
    }
    super::paging::protect_kernel_image(super::smp::trampoline())
}

pub fn summary() -> alloc::string::String {
    let on = |flag: bool| if flag { "on" } else { "off" };
    alloc::format!("SMEP {}, UMIP {}, kernel W^X {}", on(SMEP.load(Ordering::Relaxed)), on(UMIP.load(Ordering::Relaxed)), on(super::paging::kernel_wx()))
}
