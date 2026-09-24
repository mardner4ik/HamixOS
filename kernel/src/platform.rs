use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fdt::{Fdt, Node};

pub fn cells(node: &Node, name: &str) -> Vec<u32> {
    node.property(name).map(|raw| raw.chunks_exact(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect()).unwrap_or_default()
}

fn probe_pci(fdt: &Fdt) -> Option<String> {
    let node = fdt.nodes().find(|n| n.compatible_with("pci-host-ecam-generic") && n.enabled())?;
    let (base, size) = node.reg().next()?;
    let range = cells(&node, "bus-range");
    let (first, last) = match range.as_slice() {
        [first, last] => (*first as u8, *last as u8),
        _ => (0, ((size >> 20).saturating_sub(1)).min(255) as u8),
    };
    crate::drivers::pci::set_ecam(base, first, last);
    crate::drivers::pci::configure_host(fdt, &node);
    Some(format!("PCIe ECAM at {:#x}, buses {}-{}", base, first, last))
}

fn probe_rtc(fdt: &Fdt) -> Option<String> {
    for (compatible, kind) in [("arm,pl031", crate::drivers::rtc::PL031), ("google,goldfish-rtc", crate::drivers::rtc::GOLDFISH)] {
        if let Some(node) = fdt.find_compatible(compatible) {
            let (base, _) = node.reg().next()?;
            crate::drivers::rtc::attach(base, kind);
            return Some(format!("{} at {:#x}", compatible, base));
        }
    }
    None
}

pub fn probe(fdt: &Fdt) {
    if let Some(line) = probe_pci(fdt) {
        crate::drivers::klog::log(&format!("platform: {}", line));
    }
    if let Some(line) = probe_rtc(fdt) {
        crate::drivers::klog::log(&format!("platform: rtc {}", line));
    }
    crate::drivers::virtio::probe_mmio(fdt);
}
