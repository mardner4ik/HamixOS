//! Minimal PCI configuration-space access via the legacy I/O ports
//! (0xCF8 address / 0xCFC data), supported by every x86 chipset since the
//! mid-90s -- no MMCONFIG/ACPI parsing required. Just enough to walk
//! bus/device/function space and find the display controller, so the
//! graphics driver can report what GPU is actually in the machine instead
//! of always assuming one specific chipset.

use alloc::vec::Vec;

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use crate::arch::io::{inl, outl};

static ECAM_BASE: AtomicU64 = AtomicU64::new(0);
static ECAM_FIRST_BUS: AtomicU8 = AtomicU8::new(0);
static ECAM_LAST_BUS: AtomicU8 = AtomicU8::new(0);

pub fn set_ecam(base: u64, first_bus: u8, last_bus: u8) {
    ECAM_FIRST_BUS.store(first_bus, Ordering::Relaxed);
    ECAM_LAST_BUS.store(last_bus, Ordering::Relaxed);
    ECAM_BASE.store(base, Ordering::Release);
    *CACHE.lock() = None;
}

pub fn ecam() -> Option<(u64, u8, u8)> {
    let base = ECAM_BASE.load(Ordering::Acquire);
    if base == 0 {
        return None;
    }
    Some((base, ECAM_FIRST_BUS.load(Ordering::Relaxed), ECAM_LAST_BUS.load(Ordering::Relaxed)))
}

fn ecam_slot(addr: PciAddress, offset: u8) -> Option<*mut u32> {
    let (base, first, last) = ecam()?;
    if addr.bus < first || addr.bus > last {
        return None;
    }
    let bus = (addr.bus - first) as u64;
    Some((base + (bus << 20) + ((addr.device as u64) << 15) + ((addr.function as u64) << 12) + (offset as u64 & 0xFFC)) as *mut u32)
}

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
    if ECAM_BASE.load(Ordering::Relaxed) != 0 {
        return match ecam_slot(addr, offset) {
            Some(slot) => unsafe { core::ptr::read_volatile(slot) },
            None => 0xFFFF_FFFF,
        };
    }
    if !crate::arch::io::PORTS {
        return 0xFFFF_FFFF;
    }
    crate::arch::without_interrupts(|| {
        let _guard = CONFIG_LOCK.lock();
        outl(CONFIG_ADDRESS, config_address(addr, offset));
        inl(CONFIG_DATA)
    })
}

pub fn write_config_u32(addr: PciAddress, offset: u8, value: u32) {
    if ECAM_BASE.load(Ordering::Relaxed) != 0 {
        if let Some(slot) = ecam_slot(addr, offset) {
            unsafe { core::ptr::write_volatile(slot, value) };
        }
        return;
    }
    if !crate::arch::io::PORTS {
        return;
    }
    crate::arch::without_interrupts(|| {
        let _guard = CONFIG_LOCK.lock();
        outl(CONFIG_ADDRESS, config_address(addr, offset));
        outl(CONFIG_DATA, value);
    })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarInfo {
    pub base: u64,
    pub len: u64,
    pub io: bool,
    pub wide: bool,
}

pub fn bar_info(addr: PciAddress, index: u8) -> Option<BarInfo> {
    if index > 5 {
        return None;
    }
    let offset = 0x10 + index * 4;
    crate::arch::without_interrupts(|| {
        let original = read_config_u32(addr, offset);
        let io = original & 1 == 1;
        let wide = !io && (original >> 1) & 3 == 2 && index < 5;
        let high = if wide { read_config_u32(addr, offset + 4) } else { 0 };
        let command = read_config_u16(addr, 0x04);
        write_config_u16(addr, 0x04, command & !0x0003);
        write_config_u32(addr, offset, 0xFFFF_FFFF);
        let probe_low = read_config_u32(addr, offset);
        write_config_u32(addr, offset, original);
        let probe_high = if wide {
            write_config_u32(addr, offset + 4, 0xFFFF_FFFF);
            let value = read_config_u32(addr, offset + 4);
            write_config_u32(addr, offset + 4, high);
            value
        } else {
            0
        };
        write_config_u16(addr, 0x04, command);
        if io {
            let mask = probe_low & 0xFFFF_FFFC & 0xFFFF;
            if mask == 0 {
                return None;
            }
            let len = ((!mask).wrapping_add(1) & 0xFFFF) as u64;
            return Some(BarInfo { base: (original & 0xFFFC) as u64, len, io, wide: false });
        }
        let mask = if wide { ((probe_high as u64) << 32) | (probe_low & 0xFFFF_FFF0) as u64 } else { (probe_low & 0xFFFF_FFF0) as u64 | 0xFFFF_FFFF_0000_0000 };
        if mask & 0xFFFF_FFF0 == 0 && (!wide || probe_high == 0) {
            return None;
        }
        let len = (!mask).wrapping_add(1);
        let base = ((original & 0xFFFF_FFF0) as u64) | ((high as u64) << 32);
        Some(BarInfo { base, len, io, wide })
    })
}

pub fn bars(addr: PciAddress) -> Vec<(u8, BarInfo)> {
    let mut out = Vec::new();
    let mut index = 0u8;
    while index < 6 {
        match bar_info(addr, index) {
            Some(info) => {
                out.push((index, info));
                index += if info.wide { 2 } else { 1 };
            }
            None => index += 1,
        }
    }
    out
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

static CACHE: spin::Mutex<Option<Vec<PciDevice>>> = spin::Mutex::new(None);
static CONFIG_LOCK: spin::Mutex<()> = spin::Mutex::new(());

pub fn devices() -> Vec<PciDevice> {
    let cached = CACHE.lock().clone();
    match cached {
        Some(list) => list,
        None => {
            let list = scan();
            *CACHE.lock() = Some(list.clone());
            list
        }
    }
}

pub fn enumerate() -> Vec<PciDevice> {
    devices()
}

fn scan() -> Vec<PciDevice> {
    let mut found = Vec::new();
    let (first, last) = match ecam() {
        Some((_, first, last)) => (first as u16, last as u16),
        None if crate::arch::io::PORTS => (0, 255),
        None => return found,
    };
    for bus in first..=last {
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
    devices().iter().find(|d| d.class == CLASS_DISPLAY_CONTROLLER).map(|d| (d.vendor, d.device))
}

#[allow(dead_code)]
fn find_display_controller_slow() -> Option<(u16, u16)> {
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

#[cfg(not(target_arch = "x86_64"))]
fn be_cells(raw: &[u8]) -> Vec<u32> {
    raw.chunks_exact(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect()
}

#[cfg(not(target_arch = "x86_64"))]
fn parent_address_cells(fdt: &crate::fdt::Fdt, phandle: u32) -> Option<(usize, usize)> {
    let node = fdt.nodes().find(|n| n.u32_property("phandle") == Some(phandle) || n.u32_property("linux,phandle") == Some(phandle))?;
    Some((node.u32_property("#address-cells").unwrap_or(0) as usize, node.u32_property("#interrupt-cells").unwrap_or(1) as usize))
}

#[cfg(not(target_arch = "x86_64"))]
fn route_intx(fdt: &crate::fdt::Fdt, node: &crate::fdt::Node, address: PciAddress, pin: u8) -> Option<u8> {
    let map = be_cells(node.property("interrupt-map")?);
    let mask = node.property("interrupt-map-mask").map(be_cells).unwrap_or_else(|| alloc::vec![0xFFFF_FFFF; 4]);
    let hi = ((address.bus as u32) << 16) | ((address.device as u32) << 11) | ((address.function as u32) << 8);
    let key = [hi & mask[0], 0, 0, pin as u32 & mask.get(3).copied().unwrap_or(7)];
    let mut at = 0;
    while at + 5 < map.len() {
        let child = [map[at], map[at + 1], map[at + 2], map[at + 3]];
        let (parent_addr, parent_int) = parent_address_cells(fdt, map[at + 4])?;
        let spec = at + 5 + parent_addr;
        let end = spec + parent_int;
        if end > map.len() {
            return None;
        }
        if [child[0] & mask[0], 0, 0, child[3] & mask.get(3).copied().unwrap_or(7)] == key {
            let line = if parent_int >= 3 { 32 + map[spec + 1] } else { map[spec] };
            return u8::try_from(line).ok();
        }
        at = end;
    }
    None
}

#[cfg(not(target_arch = "x86_64"))]
pub fn configure_host(fdt: &crate::fdt::Fdt, node: &crate::fdt::Node) {
    let ranges = node.property("ranges").map(be_cells).unwrap_or_default();
    let mut window: Option<(u64, u64, u64)> = None;
    for entry in ranges.chunks_exact(7) {
        let space = (entry[0] >> 24) & 3;
        let pci = ((entry[1] as u64) << 32) | entry[2] as u64;
        let cpu = ((entry[3] as u64) << 32) | entry[4] as u64;
        let size = ((entry[5] as u64) << 32) | entry[6] as u64;
        if space == 2 && window.map(|w| size > w.2).unwrap_or(true) {
            window = Some((cpu, pci, size));
        }
    }
    let Some((cpu_base, pci_base, size)) = window else {
        return;
    };
    let offset = cpu_base.wrapping_sub(pci_base);
    let mut next = pci_base;
    let end = pci_base + size;
    let mut assigned = 0usize;
    for dev in scan() {
        if (read_config_u32(dev.address, 0x0C) >> 16) & 0x7F != 0 {
            continue;
        }
        let command = read_config_u16(dev.address, 0x04);
        write_config_u16(dev.address, 0x04, command & !0x0007);
        let mut index = 0u8;
        while index < 6 {
            let Some(info) = bar_info(dev.address, index) else {
                index += 1;
                continue;
            };
            let step = if info.wide { 2 } else { 1 };
            if info.io || info.len == 0 {
                index += step;
                continue;
            }
            let base = next.div_ceil(info.len) * info.len;
            if base + info.len > end {
                index += step;
                continue;
            }
            next = base + info.len;
            let offset_reg = 0x10 + index * 4;
            let flags = read_config_u32(dev.address, offset_reg) & 0xF;
            write_config_u32(dev.address, offset_reg, (base as u32 & 0xFFFF_FFF0) | flags);
            if info.wide {
                write_config_u32(dev.address, offset_reg + 4, (base >> 32) as u32);
            }
            assigned += 1;
            index += step;
        }
        let pin = (read_config_u32(dev.address, 0x3C) >> 8) as u8;
        if pin != 0 {
            let line = route_intx(fdt, node, dev.address, pin).unwrap_or(0xFF);
            let word = read_config_u32(dev.address, 0x3C);
            write_config_u32(dev.address, 0x3C, (word & !0xFF) | line as u32);
        }
        write_config_u16(dev.address, 0x04, command | 0x0006);
    }
    if offset != 0 {
        crate::drivers::klog::log(&alloc::format!("pci: memory window cpu {:#x} != pci {:#x}, BARs use pci addresses", cpu_base, pci_base));
    }
    *CACHE.lock() = None;
    crate::drivers::klog::log(&alloc::format!("pci: assigned {} BAR(s) from the {} MiB window at {:#x}", assigned, size >> 20, cpu_base));
}
