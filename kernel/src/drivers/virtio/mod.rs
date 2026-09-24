pub mod blk;
pub mod gpu;
pub mod input;
pub mod net;
pub mod queue;
pub mod transport;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub use queue::Queue;
pub use transport::{MmioTransport, PciTransport, Transport};

pub const ID_NET: u32 = 1;
pub const ID_BLOCK: u32 = 2;
pub const ID_CONSOLE: u32 = 3;
pub const ID_RNG: u32 = 4;
pub const ID_GPU: u32 = 16;
pub const ID_INPUT: u32 = 18;

pub const STATUS_ACKNOWLEDGE: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;
pub const STATUS_FAILED: u8 = 128;

pub const F_VERSION_1: u64 = 1 << 32;

pub struct Found {
    pub transport: Box<dyn Transport>,
    pub location: String,
    pub irq: Option<u32>,
}

static PENDING: Mutex<Vec<Found>> = Mutex::new(Vec::new());
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn name_of(id: u32) -> &'static str {
    match id {
        ID_NET => "net",
        ID_BLOCK => "block",
        ID_CONSOLE => "console",
        ID_RNG => "rng",
        ID_GPU => "gpu",
        ID_INPUT => "input",
        _ => "device",
    }
}

pub fn negotiate(transport: &mut dyn Transport, wanted: u64) -> Option<u64> {
    transport.set_status(0);
    transport.set_status(STATUS_ACKNOWLEDGE);
    transport.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
    let offered = transport.device_features();
    let mut accepted = offered & wanted;
    if transport.modern() {
        accepted |= offered & F_VERSION_1;
    }
    transport.set_driver_features(accepted);
    if transport.modern() {
        transport.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
        if transport.status() & STATUS_FEATURES_OK == 0 {
            transport.set_status(STATUS_FAILED);
            return None;
        }
    }
    Some(accepted)
}

pub fn finish(transport: &mut dyn Transport) {
    let status = if transport.modern() { STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK } else { STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_DRIVER_OK };
    transport.set_status(status);
}

fn remember(found: Found) {
    let mut seen = SEEN.lock();
    if seen.contains(&found.location) {
        return;
    }
    seen.push(found.location.clone());
    drop(seen);
    crate::drivers::klog::log(&format!("virtio: {} at {}{}", name_of(found.transport.device_id()), found.location, found.irq.map(|i| format!(", irq {}", i)).unwrap_or_default()));
    PENDING.lock().push(found);
}

pub fn take(id: u32) -> Vec<Found> {
    let mut pending = PENDING.lock();
    let mut taken = Vec::new();
    let mut index = 0;
    while index < pending.len() {
        if pending[index].transport.device_id() == id {
            taken.push(pending.remove(index));
        } else {
            index += 1;
        }
    }
    taken
}

#[cfg(not(target_arch = "x86_64"))]
pub fn probe_mmio(fdt: &crate::fdt::Fdt) {
    for node in fdt.nodes().filter(|n| n.compatible_with("virtio,mmio") && n.enabled()) {
        let Some((base, size)) = node.reg().next() else {
            continue;
        };
        let Some(transport) = MmioTransport::probe(base, size) else {
            continue;
        };
        let irq = crate::arch::irq::interrupt_of(&node);
        remember(Found { transport: Box::new(transport), location: format!("mmio {:#x}", base), irq });
    }
}

pub fn probe_pci(skip_gpu: bool) {
    for dev in crate::drivers::pci::devices() {
        if dev.vendor != 0x1AF4 || !(0x1000..=0x107F).contains(&dev.device) {
            continue;
        }
        let id = if dev.device >= 0x1040 { (dev.device - 0x1040) as u32 } else { transport::legacy_pci_id(dev.device) };
        if id == ID_GPU && skip_gpu {
            continue;
        }
        if id != ID_BLOCK && id != ID_NET && id != ID_INPUT && id != ID_GPU {
            continue;
        }
        let Some(transport) = PciTransport::probe(dev.address) else {
            continue;
        };
        let irq = if dev.interrupt_line != 0 && dev.interrupt_line != 0xFF { Some(dev.interrupt_line as u32) } else { None };
        remember(Found {
            transport: Box::new(transport),
            location: format!("pci {:02x}:{:02x}.{}", dev.address.bus, dev.address.device, dev.address.function),
            irq,
        });
    }
}

pub fn probe_all() {
    probe_pci(true);
}
