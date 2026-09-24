use alloc::format;
use alloc::string::String;

const PSCI_SYSTEM_OFF: u64 = 0x8400_0008;
const PSCI_SYSTEM_RESET: u64 = 0x8400_0009;

fn psci(function: u64) {
    let method = crate::fdt::current().and_then(|f| f.find_compatible("arm,psci-1.0").or_else(|| f.find_compatible("arm,psci-0.2")).or_else(|| f.find("/psci"))).and_then(|n| n.str_property("method")).unwrap_or("hvc");
    unsafe {
        if method == "smc" {
            core::arch::asm!("smc #0", inout("x0") function => _, options(nostack));
        } else {
            core::arch::asm!("hvc #0", inout("x0") function => _, options(nostack));
        }
    }
}

pub fn reset() {
    psci(PSCI_SYSTEM_RESET);
}

pub fn poweroff() {
    psci(PSCI_SYSTEM_OFF);
}

fn midr() -> u64 {
    let value: u64;
    unsafe { core::arch::asm!("mrs {}, midr_el1", out(reg) value, options(nomem, nostack)) };
    value
}

pub fn cpu_brand() -> String {
    let cpu = crate::fdt::current().and_then(|f| f.find("/cpus").and_then(|c| c.children().find(|n| n.str_property("device_type") == Some("cpu"))).and_then(|n| n.strings("compatible").next().map(String::from)));
    match cpu {
        Some(name) => name,
        None => {
            let id = midr();
            format!("ARMv8 implementer {:#x} part {:#x}", (id >> 24) & 0xFF, (id >> 4) & 0xFFF)
        }
    }
}

pub fn cpuinfo_text() -> String {
    let id = midr();
    let (features, _) = (String::from("fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid"), 0);
    format!(
        "processor\t: 0\nBogoMIPS\t: {}\nFeatures\t: {}\nCPU implementer\t: {:#x}\nCPU architecture: 8\nCPU variant\t: {:#x}\nCPU part\t: {:#05x}\nCPU revision\t: {}\nmodel name\t: {}\n",
        super::timer::frequency() / 500_000,
        features,
        (id >> 24) & 0xFF,
        (id >> 20) & 0xF,
        (id >> 4) & 0xFFF,
        id & 0xF,
        cpu_brand()
    )
}

pub fn cycles() -> u64 {
    super::timer::counter()
}

pub fn hw_random() -> Option<u64> {
    let isar0: u64;
    unsafe { core::arch::asm!("mrs {}, id_aa64isar0_el1", out(reg) isar0, options(nomem, nostack)) };
    if (isar0 >> 60) & 0xF == 0 {
        return None;
    }
    let value: u64;
    let flags: u64;
    unsafe { core::arch::asm!(".arch armv8.5-a+rng", "mrs {v}, s3_3_c2_c4_0", "mrs {f}, nzcv", v = out(reg) value, f = out(reg) flags, options(nomem, nostack)) };
    if flags & (1 << 30) != 0 { None } else { Some(value) }
}
