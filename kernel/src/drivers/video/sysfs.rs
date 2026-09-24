use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::pci::{self, PciDevice};

const PCI_DEVICES: &str = "/sys/bus/pci/devices";
const DRM_CLASS: &str = "/sys/class/drm";

pub fn slot(device: &PciDevice) -> String {
    format!("0000:{:02x}:{:02x}.{}", device.address.bus, device.address.device, device.address.function)
}

fn modalias(device: &PciDevice, subsystem: (u16, u16), revision: u8) -> String {
    let _ = revision;
    format!(
        "pci:v{:08X}d{:08X}sv{:08X}sd{:08X}bc{:02X}sc{:02X}i{:02X}",
        device.vendor as u32, device.device as u32, subsystem.0 as u32, subsystem.1 as u32, device.class, device.subclass, device.prog_if
    )
}

fn attributes(device: &PciDevice) -> Vec<(&'static str, String)> {
    let subsystem_raw = pci::read_config_u32(device.address, 0x2C);
    let subsystem = ((subsystem_raw & 0xFFFF) as u16, (subsystem_raw >> 16) as u16);
    let revision = (pci::read_config_u32(device.address, 0x08) & 0xFF) as u8;
    let class = ((device.class as u32) << 16) | ((device.subclass as u32) << 8) | device.prog_if as u32;
    let alias = modalias(device, subsystem, revision);
    alloc::vec![
        ("vendor", format!("0x{:04x}\n", device.vendor)),
        ("device", format!("0x{:04x}\n", device.device)),
        ("subsystem_vendor", format!("0x{:04x}\n", subsystem.0)),
        ("subsystem_device", format!("0x{:04x}\n", subsystem.1)),
        ("class", format!("0x{:06x}\n", class)),
        ("revision", format!("0x{:02x}\n", revision)),
        ("irq", format!("{}\n", device.interrupt_line)),
        ("enable", String::from("1\n")),
        ("modalias", format!("{}\n", alias)),
        (
            "uevent",
            format!(
                "PCI_CLASS={:06X}\nPCI_ID={:04X}:{:04X}\nPCI_SUBSYS_ID={:04X}:{:04X}\nPCI_SLOT_NAME={}\nMODALIAS={}\n",
                class,
                device.vendor,
                device.device,
                subsystem.0,
                subsystem.1,
                slot(device),
                alias
            ),
        ),
    ]
}

fn put(vfs: &mut crate::fs::Vfs, dir: &str, files: &[(&'static str, String)]) {
    if vfs.mkdir_all(dir, 0).is_err() {
        return;
    }
    for (name, value) in files {
        let _ = vfs.write(0, &format!("{}/{}", dir, name), value.as_bytes(), false, 0);
    }
}

pub fn populate_cpus(count: usize) {
    let mut guard = crate::fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let tracking = vfs.tracking;
    vfs.tracking = false;
    let base = "/sys/devices/system/cpu";
    let _ = vfs.mkdir_all(base, 0);
    let range = if count <= 1 { String::from("0\n") } else { format!("0-{}\n", count - 1) };
    for name in ["possible", "present", "online"] {
        let _ = vfs.write(0, &format!("{}/{}", base, name), range.as_bytes(), false, 0);
    }
    let _ = vfs.write(0, &format!("{}/kernel_max", base), b"63\n", false, 0);
    let _ = vfs.write(0, &format!("{}/offline", base), b"\n", false, 0);
    for cpu in 0..count {
        let dir = format!("{}/cpu{}", base, cpu);
        let _ = vfs.mkdir_all(&format!("{}/topology", dir), 0);
        let _ = vfs.write(0, &format!("{}/online", dir), b"1\n", false, 0);
        let _ = vfs.write(0, &format!("{}/topology/core_id", dir), format!("{}\n", cpu).as_bytes(), false, 0);
        let _ = vfs.write(0, &format!("{}/topology/physical_package_id", dir), b"0\n", false, 0);
    }
    vfs.tracking = tracking;
}

fn secondary_display(device: &PciDevice, devices: &[PciDevice]) -> bool {
    device.class == 0x03
        && device.address.function != 0
        && devices.iter().any(|d| d.class == 0x03 && d.address.function == 0 && d.address.bus == device.address.bus && d.address.device == device.address.device)
}

pub fn populate() {
    let devices = pci::devices();
    let mut guard = crate::fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let tracking = vfs.tracking;
    vfs.tracking = false;
    let _ = vfs.mkdir_all(PCI_DEVICES, 0);
    let _ = vfs.mkdir_all(DRM_CLASS, 0);
    let mut card = 0usize;
    for device in devices.iter() {
        if secondary_display(device, &devices) {
            continue;
        }
        let files = attributes(device);
        let base = format!("{}/{}", PCI_DEVICES, slot(device));
        put(vfs, &base, &files);
        if device.class != 0x03 {
            continue;
        }
        let drm = format!("{}/card{}", DRM_CLASS, card);
        put(vfs, &format!("{}/device", drm), &files);
        let _ = vfs.mkdir_all(&format!("{}/device/drm/card{}", drm, card), 0);
        let _ = vfs.mkdir_all(&format!("{}/drm/card{}", base, card), 0);
        let _ = vfs.write(0, &format!("{}/device/power/runtime_status", drm), b"active\n", false, 0);
        let _ = vfs.write(0, &format!("{}/dev", drm), format!("226:{}\n", card).as_bytes(), false, 0);
        let _ = vfs.write(0, &format!("{}/uevent", drm), format!("MAJOR=226\nMINOR={}\nDEVNAME=dri/card{}\nDEVTYPE=drm_minor\n", card, card).as_bytes(), false, 0);
        card += 1;
    }
    vfs.tracking = tracking;
}

pub fn display_name() -> String {
    if let Some((model, _, _)) = super::gpu::identity() {
        return model;
    }
    let Some(device) = pci::devices().into_iter().find(|d| d.class == 0x03) else {
        return String::from("no display controller");
    };
    format!("{} {:04x}:{:04x}", vendor_name(device.vendor), device.vendor, device.device)
}

pub fn vendor_name(vendor: u16) -> &'static str {
    match vendor {
        0x8086 => "Intel",
        0x1002 | 0x1022 => "AMD",
        0x10DE => "NVIDIA",
        0x1af4 | 0x1b36 => "Red Hat",
        0x15AD => "VMware",
        0x1234 => "QEMU",
        0x1013 => "Cirrus Logic",
        0x80EE => "VirtualBox",
        _ => "Unknown vendor",
    }
}
