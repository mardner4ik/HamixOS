//! Real CPUID-based CPU identification, replacing what used to be a
//! hardcoded "Pentium G640 / Celeron T3100" string regardless of what
//! chip the kernel actually boots on.

use alloc::string::String;
use core::arch::x86_64::__cpuid_count;

pub struct CpuInfo {
    pub vendor: String,
    pub brand: String,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub features: Features,
}

pub struct Features {
    pub sse: bool,
    pub sse2: bool,
    pub sse3: bool,
    pub ssse3: bool,
    pub sse4_1: bool,
    pub sse4_2: bool,
    pub avx: bool,
    pub fpu: bool,
    pub mmx: bool,
}

fn cpuid(leaf: u32) -> core::arch::x86_64::CpuidResult {
    __cpuid_count(leaf, 0)
}

fn max_leaf() -> u32 {
    cpuid(0).eax
}

fn max_extended_leaf() -> u32 {
    cpuid(0x8000_0000).eax
}

fn vendor_string() -> String {
    let r = cpuid(0);
    let mut bytes = [0u8; 12];
    bytes[0..4].copy_from_slice(&r.ebx.to_le_bytes());
    bytes[4..8].copy_from_slice(&r.edx.to_le_bytes());
    bytes[8..12].copy_from_slice(&r.ecx.to_le_bytes());
    String::from_utf8_lossy(&bytes).trim().into()
}

/// The brand string (leaves 0x80000002-0x80000004) is what actually
/// contains the human-readable model name, e.g. "Intel(R) Core(TM)
/// i5-6300U CPU @ 2.40GHz" -- exactly the kind of line `cpuinfo` should
/// show instead of a hardcoded guess. Not every CPU implements it (older
/// or virtualized ones may not), so callers should be ready for an empty
/// string.
fn brand_string() -> String {
    if max_extended_leaf() < 0x8000_0004 {
        return String::new();
    }
    let mut bytes = [0u8; 48];
    for (i, leaf) in (0x8000_0002u32..=0x8000_0004u32).enumerate() {
        let r = __cpuid_count(leaf, 0);
        let off = i * 16;
        bytes[off..off + 4].copy_from_slice(&r.eax.to_le_bytes());
        bytes[off + 4..off + 8].copy_from_slice(&r.ebx.to_le_bytes());
        bytes[off + 8..off + 12].copy_from_slice(&r.ecx.to_le_bytes());
        bytes[off + 12..off + 16].copy_from_slice(&r.edx.to_le_bytes());
    }
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().into()
}

pub fn identify() -> CpuInfo {
    let vendor = vendor_string();
    let brand = brand_string();

    let (family, model, stepping) = if max_leaf() >= 1 {
        let r = cpuid(1);
        let base_family = (r.eax >> 8) & 0xF;
        let ext_family = (r.eax >> 20) & 0xFF;
        let family = if base_family == 0xF { base_family + ext_family } else { base_family };

        let base_model = (r.eax >> 4) & 0xF;
        let ext_model = (r.eax >> 16) & 0xF;
        let model = if base_family == 0x6 || base_family == 0xF {
            (ext_model << 4) | base_model
        } else {
            base_model
        };

        let stepping = r.eax & 0xF;
        (family, model, stepping)
    } else {
        (0, 0, 0)
    };

    let features = if max_leaf() >= 1 {
        let r = cpuid(1);
        Features {
            fpu: r.edx & (1 << 0) != 0,
            mmx: r.edx & (1 << 23) != 0,
            sse: r.edx & (1 << 25) != 0,
            sse2: r.edx & (1 << 26) != 0,
            sse3: r.ecx & (1 << 0) != 0,
            ssse3: r.ecx & (1 << 9) != 0,
            sse4_1: r.ecx & (1 << 19) != 0,
            sse4_2: r.ecx & (1 << 20) != 0,
            avx: r.ecx & (1 << 28) != 0,
        }
    } else {
        Features {
            fpu: false, mmx: false, sse: false, sse2: false,
            sse3: false, ssse3: false, sse4_1: false, sse4_2: false, avx: false,
        }
    };

    CpuInfo { vendor, brand, family, model, stepping, features }
}
