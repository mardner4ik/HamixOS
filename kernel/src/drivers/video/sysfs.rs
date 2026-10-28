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

fn link(vfs: &mut crate::fs::Vfs, path: &str, target: &str) {
    let mut body = Vec::from(crate::fs::LINK_MAGIC);
    body.extend_from_slice(target.as_bytes());
    let _ = vfs.write(0, path, &body, false, 0);
    let _ = vfs.chmod(0, path, 0, 0o777);
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
    let _ = vfs.mkdir_all("/sys/dev/char", 0);
    let _ = vfs.mkdir_all("/sys/dev/block", 0);
    let _ = vfs.mkdir_all("/sys/class/input", 0);
    let mut card = 0usize;
    for device in devices.iter() {
        if secondary_display(device, &devices) {
            continue;
        }
        let files = attributes(device);
        let base = format!("{}/{}", PCI_DEVICES, slot(device));
        put(vfs, &base, &files);
        link(vfs, &format!("{}/subsystem", base), "/sys/bus/pci");
        if device.class != 0x03 {
            continue;
        }
        let dir = format!("{}/drm/card{}", base, card);
        let _ = vfs.mkdir_all(&format!("{}/power", base), 0);
        let _ = vfs.write(0, &format!("{}/power/runtime_status", base), b"active\n", false, 0);
        let _ = vfs.write(0, &format!("{}/boot_vga", base), if card == 0 { b"1\n" } else { b"0\n" }, false, 0);
        put(
            vfs,
            &dir,
            &[
                ("dev", format!("226:{}\n", card)),
                ("uevent", format!("MAJOR=226\nMINOR={}\nDEVNAME=dri/card{}\nDEVTYPE=drm_minor\n", card, card)),
            ],
        );
        link(vfs, &format!("{}/subsystem", dir), "/sys/class/drm");
        link(vfs, &format!("{}/device", dir), &base);
        link(vfs, &format!("{}/card{}", DRM_CLASS, card), &dir);
        link(vfs, &format!("/sys/dev/char/226:{}", card), &dir);
        card += 1;
    }
    populate_input(vfs);
    vfs.tracking = tracking;
}

fn populate_input(vfs: &mut crate::fs::Vfs) {
    use crate::drivers::input::evdev;
    let dmi = "dmi:bvnHamixOS:bvr1:svnHamixOS:pnPC:pvr1:rvnHamixOS:rnPC:rvr1:cvnHamixOS:ct3:cvr1:";
    put(
        vfs,
        "/sys/devices/virtual/dmi/id",
        &[
            ("uevent", format!("MODALIAS={}\n", dmi)),
            ("modalias", format!("{}\n", dmi)),
            ("sys_vendor", String::from("HamixOS\n")),
            ("product_name", String::from("PC\n")),
            ("chassis_type", String::from("3\n")),
        ],
    );
    let _ = vfs.mkdir_all("/sys/class/dmi", 0);
    link(vfs, "/sys/class/dmi/id", "/sys/devices/virtual/dmi/id");
    for (n, info) in evdev::DEVICES.iter().enumerate() {
        let base = format!("/sys/devices/virtual/input/input{}", n);
        let keys = evdev::key_bits(n);
        let ev = evdev::hex_words(&[evdev::ev_bits(n)]);
        let key = evdev::hex_words(&keys);
        let rel = evdev::hex_words(&[evdev::rel_bits(n)]);
        let abs = evdev::hex_words(&[evdev::abs_bits(n)]);
        let product = format!("{:x}/{:x}/{:x}/{:x}", info.bus, info.vendor, info.product, info.version);
        put(
            vfs,
            &base,
            &[
                ("name", format!("{}\n", info.name)),
                ("phys", format!("{}\n", info.phys)),
                ("uniq", String::from("\n")),
                ("properties", String::from("0\n")),
                (
                    "uevent",
                    format!("PRODUCT={}\nNAME=\"{}\"\nPHYS=\"{}\"\nPROP=0\nEV={}\nKEY={}\nREL={}\nABS={}\n", product, info.name, info.phys, ev, key, rel, abs),
                ),
            ],
        );
        put(vfs, &format!("{}/capabilities", base), &[("ev", format!("{}\n", ev)), ("key", format!("{}\n", key)), ("rel", format!("{}\n", rel)), ("abs", format!("{}\n", abs)), ("msc", String::from("0\n")), ("led", String::from("0\n")), ("sw", String::from("0\n")), ("ff", String::from("0\n")), ("snd", String::from("0\n"))]);
        put(
            vfs,
            &format!("{}/id", base),
            &[
                ("bustype", format!("{:04x}\n", info.bus)),
                ("vendor", format!("{:04x}\n", info.vendor)),
                ("product", format!("{:04x}\n", info.product)),
                ("version", format!("{:04x}\n", info.version)),
            ],
        );
        link(vfs, &format!("{}/subsystem", base), "/sys/class/input");
        let minor = evdev::FIRST_MINOR + n as u32;
        let event = format!("{}/event{}", base, n);
        put(
            vfs,
            &event,
            &[("dev", format!("{}:{}\n", evdev::MAJOR, minor)), ("uevent", format!("MAJOR={}\nMINOR={}\nDEVNAME=input/event{}\n", evdev::MAJOR, minor, n))],
        );
        link(vfs, &format!("{}/subsystem", event), "/sys/class/input");
        link(vfs, &format!("{}/device", event), &base);
        link(vfs, &format!("/sys/class/input/input{}", n), &base);
        link(vfs, &format!("/sys/class/input/event{}", n), &event);
        link(vfs, &format!("/sys/dev/char/{}:{}", evdev::MAJOR, minor), &event);
    }
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
