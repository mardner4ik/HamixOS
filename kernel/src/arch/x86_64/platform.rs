use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{inb, outb, outw};

pub fn reset() {
    for _ in 0..10000 {
        if inb(0x64) & 2 == 0 {
            break;
        }
    }
    super::acpi::reset();
    outb(0x64, 0xFE);
    outb(0xCF9, 0x06);
}

pub fn poweroff() {
    super::acpi::poweroff();
    outw(0x604, 0x2000);
    outw(0xB004, 0x2000);
    outw(0x4004, 0x3400);
}

pub fn cpu_brand() -> String {
    let cpu = super::cpuid::identify();
    if cpu.brand.is_empty() { cpu.vendor } else { String::from(cpu.brand.trim()) }
}

pub fn cpuinfo_text() -> String {
    let info = super::cpuid::identify();
    let leaf1 = core::arch::x86_64::__cpuid_count(1, 0);
    let leaf7 = if core::arch::x86_64::__cpuid_count(0, 0).eax >= 7 { core::arch::x86_64::__cpuid_count(7, 0) } else { core::arch::x86_64::__cpuid_count(0, 0) };
    let ext = core::arch::x86_64::__cpuid_count(0x8000_0001, 0);
    let mut feats: Vec<&str> = Vec::new();
    let edx_names: [(u32, &str); 24] = [
        (0, "fpu"), (1, "vme"), (2, "de"), (3, "pse"), (4, "tsc"), (5, "msr"), (6, "pae"), (7, "mce"), (8, "cx8"), (9, "apic"), (11, "sep"), (12, "mtrr"),
        (13, "pge"), (14, "mca"), (15, "cmov"), (16, "pat"), (17, "pse36"), (19, "clflush"), (23, "mmx"), (24, "fxsr"), (25, "sse"), (26, "sse2"), (28, "ht"), (29, "tm"),
    ];
    for (bit, name) in edx_names {
        if leaf1.edx & (1 << bit) != 0 {
            feats.push(name);
        }
    }
    if ext.edx & (1 << 11) != 0 {
        feats.push("syscall");
    }
    if ext.edx & (1 << 20) != 0 {
        feats.push("nx");
    }
    if ext.edx & (1 << 27) != 0 {
        feats.push("rdtscp");
    }
    if ext.edx & (1 << 29) != 0 {
        feats.push("lm");
    }
    let ecx_names: [(u32, &str); 14] = [
        (0, "pni"), (1, "pclmulqdq"), (9, "ssse3"), (12, "fma"), (13, "cx16"), (19, "sse4_1"), (20, "sse4_2"), (22, "movbe"), (23, "popcnt"), (25, "aes"), (26, "xsave"), (28, "avx"), (29, "f16c"), (30, "rdrand"),
    ];
    for (bit, name) in ecx_names {
        if leaf1.ecx & (1 << bit) != 0 {
            feats.push(name);
        }
    }
    if ext.ecx & (1 << 0) != 0 {
        feats.push("lahf_lm");
    }
    if ext.ecx & (1 << 5) != 0 {
        feats.push("abm");
    }
    let ebx7: [(u32, &str); 8] = [(0, "fsgsbase"), (3, "bmi1"), (5, "avx2"), (8, "bmi2"), (9, "erms"), (16, "avx512f"), (18, "rdseed"), (19, "adx")];
    for (bit, name) in ebx7 {
        if leaf7.ebx & (1 << bit) != 0 {
            feats.push(name);
        }
    }
    let flags = feats.join(" ");
    let name = if info.brand.is_empty() { info.vendor.clone() } else { String::from(info.brand.trim()) };
    let count = super::smp::count().max(1);
    let mhz = brand_mhz(&name).unwrap_or(2000.0);
    let mut out = String::new();
    for cpu in 0..count {
        out.push_str(&format!(
            "processor\t: {}\nvendor_id\t: {}\ncpu family\t: {}\nmodel\t\t: {}\nmodel name\t: {}\nstepping\t: {}\ncpu MHz\t\t: {:.3}\ncache size\t: 4096 KB\nphysical id\t: 0\nsiblings\t: {}\ncore id\t\t: {}\ncpu cores\t: {}\napicid\t\t: {}\ninitial apicid\t: {}\nfpu\t\t: yes\nfpu_exception\t: yes\ncpuid level\t: {}\nwp\t\t: yes\nflags\t\t: {}\nbogomips\t: {:.2}\nclflush size\t: 64\ncache_alignment\t: 64\naddress sizes\t: 39 bits physical, 48 bits virtual\npower management:\n\n",
            cpu, info.vendor, info.family, info.model, name, info.stepping, mhz, count, cpu, count, cpu, cpu, core::arch::x86_64::__cpuid_count(0, 0).eax, flags, mhz * 2.0
        ));
    }
    out
}

fn brand_mhz(brand: &str) -> Option<f64> {
    let at = brand.find("GHz")?;
    let head = &brand[..at];
    let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map(|i| i + 1).unwrap_or(0);
    head[start..].parse::<f64>().ok().map(|g| g * 1000.0)
}

pub fn cycles() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

fn has_rdrand() -> bool {
    use core::sync::atomic::{AtomicU8, Ordering};
    static RDRAND: AtomicU8 = AtomicU8::new(0);
    match RDRAND.load(Ordering::Relaxed) {
        0 => {
            let present = core::arch::x86_64::__cpuid_count(1, 0).ecx & (1 << 30) != 0;
            RDRAND.store(if present { 1 } else { 2 }, Ordering::Relaxed);
            present
        }
        v => v == 1,
    }
}

pub fn hw_random() -> Option<u64> {
    if !has_rdrand() {
        return None;
    }
    for _ in 0..10 {
        let value: u64;
        let ok: u8;
        unsafe {
            core::arch::asm!("rdrand {v}", "setc {c}", v = out(reg) value, c = out(reg_byte) ok, options(nomem, nostack));
        }
        if ok != 0 {
            return Some(value);
        }
    }
    None
}
