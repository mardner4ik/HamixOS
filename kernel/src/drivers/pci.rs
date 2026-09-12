//! Minimal PCI configuration-space access via the legacy I/O ports
//! (0xCF8 address / 0xCFC data), supported by every x86 chipset since the
//! mid-90s -- no MMCONFIG/ACPI parsing required. Just enough to walk
//! bus/device/function space and find the display controller, so the
//! graphics driver can report what GPU is actually in the machine instead
//! of always assuming one specific chipset.

use alloc::vec::Vec;

use crate::arch::x86_64::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// PCI class code 0x03 covers all display controllers (VGA-compatible,
/// XGA, 3D, other); subclass/prog-if vary, but the class byte alone is
/// enough for "is this a GPU".
const CLASS_DISPLAY_CONTROLLER: u8 = 0x03;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

fn config_address(addr: PciAddress, offset: u8) -> u32 {
    (1u32 << 31)
        | ((addr.bus as u32) << 16)
        | ((addr.device as u32) << 11)
        | ((addr.function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

pub fn read_config_u32(addr: PciAddress, offset: u8) -> u32 {
    outl(CONFIG_ADDRESS, config_address(addr, offset));
    inl(CONFIG_DATA)
}

pub fn write_config_u32(addr: PciAddress, offset: u8, value: u32) {
    outl(CONFIG_ADDRESS, config_address(addr, offset));
    outl(CONFIG_DATA, value);
}

pub fn read_config_u16(addr: PciAddress, offset: u8) -> u16 {
    let word = read_config_u32(addr, offset & 0xFC);
    ((word >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

pub fn write_config_u16(addr: PciAddress, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let shift = (offset & 2) * 8;
    let mut word = read_config_u32(addr, aligned);
    word &= !(0xFFFFu32 << shift);
    word |= (value as u32) << shift;
    write_config_u32(addr, aligned, word);
}

pub fn enable_bus_master(addr: PciAddress) {
    let command = read_config_u16(addr, 0x04);
    write_config_u16(addr, 0x04, command | 0x0007);
}

#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub interrupt_line: u8,
}

impl PciDevice {
    pub fn bar(&self, index: u8) -> u32 {
        read_config_u32(self.address, 0x10 + index * 4)
    }

    pub fn io_bar(&self, index: u8) -> Option<u16> {
        let value = self.bar(index);
        if value & 1 == 1 { Some((value & 0xFFFC) as u16) } else { None }
    }

    pub fn mmio_bar(&self, index: u8) -> Option<u64> {
        let value = self.bar(index);
        if value & 1 != 0 {
            return None;
        }
        let base = (value & 0xFFFF_FFF0) as u64;
        if (value >> 1) & 3 == 2 {
            let high = read_config_u32(self.address, 0x10 + (index + 1) * 4) as u64;
            Some(base | (high << 32))
        } else {
            Some(base)
        }
    }
}

pub fn enumerate() -> Vec<PciDevice> {
    let mut found = Vec::new();
    for bus in 0..=255u16 {
        for device in 0..32u8 {
            let probe = PciAddress { bus: bus as u8, device, function: 0 };
            if (read_config_u32(probe, 0x00) & 0xFFFF) as u16 == 0xFFFF {
                continue;
            }
            let header_type = ((read_config_u32(probe, 0x0C) >> 16) & 0xFF) as u8;
            let max_function = if header_type & 0x80 != 0 { 8 } else { 1 };

            for function in 0..max_function {
                let address = PciAddress { bus: bus as u8, device, function };
                let id_word = read_config_u32(address, 0x00);
                let vendor = (id_word & 0xFFFF) as u16;
                if vendor == 0xFFFF {
                    continue;
                }
                let class_word = read_config_u32(address, 0x08);
                found.push(PciDevice {
                    address,
                    vendor,
                    device: ((id_word >> 16) & 0xFFFF) as u16,
                    class: ((class_word >> 24) & 0xFF) as u8,
                    subclass: ((class_word >> 16) & 0xFF) as u8,
                    prog_if: ((class_word >> 8) & 0xFF) as u8,
                    interrupt_line: (read_config_u32(address, 0x3C) & 0xFF) as u8,
                });
            }
        }
    }
    found
}

/// Walks every bus/device/function looking for a display controller
/// (class 0x03) and returns its vendor/device IDs. Only the first match is
/// returned -- multi-GPU machines are out of scope for a single fixed
/// framebuffer anyway. Function 0 of a nonexistent device reads back as
/// 0xFFFFFFFF (vendor 0xFFFF), which is how we skip empty slots.
pub fn find_display_controller() -> Option<(u16, u16)> {
    for bus in 0..=255u16 {
        for device in 0..32u8 {
            let addr = PciAddress { bus: bus as u8, device, function: 0 };
            let id_word = read_config_u32(addr, 0x00);
            let vendor = (id_word & 0xFFFF) as u16;
            if vendor == 0xFFFF {
                continue;
            }

            let header_type = ((read_config_u32(addr, 0x0C) >> 16) & 0xFF) as u8;
            let multi_function = header_type & 0x80 != 0;
            let max_function = if multi_function { 8 } else { 1 };

            for function in 0..max_function {
                let addr = PciAddress { bus: bus as u8, device, function };
                let id_word = read_config_u32(addr, 0x00);
                let vendor = (id_word & 0xFFFF) as u16;
                if vendor == 0xFFFF {
                    continue;
                }
                let device_id = ((id_word >> 16) & 0xFFFF) as u16;

                let class_word = read_config_u32(addr, 0x08);
                let class = ((class_word >> 24) & 0xFF) as u8;
                if class == CLASS_DISPLAY_CONTROLLER {
                    return Some((vendor, device_id));
                }
            }
        }
    }
    None
}
