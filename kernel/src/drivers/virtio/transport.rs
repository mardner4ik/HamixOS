use crate::drivers::pci::{self, PciAddress};

pub trait Transport: Send {
    fn device_id(&self) -> u32;
    fn modern(&self) -> bool;
    fn status(&self) -> u8;
    fn set_status(&mut self, status: u8);
    fn device_features(&mut self) -> u64;
    fn set_driver_features(&mut self, features: u64);
    fn queue_max(&mut self, index: u16) -> u16;
    fn setup_queue(&mut self, index: u16, size: u16, desc: u64, avail: u64, used: u64) -> bool;
    fn notify(&self, index: u16);
    fn config8(&self, offset: usize) -> u8;
    fn config32(&self, offset: usize) -> u32;
    fn write_config32(&self, offset: usize, value: u32);
    fn write_config8(&self, offset: usize, value: u8);
    fn ack_interrupt(&self) -> u32;

    fn config16(&self, offset: usize) -> u16 {
        self.config8(offset) as u16 | (self.config8(offset + 1) as u16) << 8
    }

    fn config64(&self, offset: usize) -> u64 {
        self.config32(offset) as u64 | (self.config32(offset + 4) as u64) << 32
    }
}

const MAGIC: u32 = 0x7472_6976;

const MMIO_VERSION: usize = 0x004;
const MMIO_DEVICE_ID: usize = 0x008;
const MMIO_DEVICE_FEATURES: usize = 0x010;
const MMIO_DEVICE_FEATURES_SEL: usize = 0x014;
const MMIO_DRIVER_FEATURES: usize = 0x020;
const MMIO_DRIVER_FEATURES_SEL: usize = 0x024;
const MMIO_GUEST_PAGE_SIZE: usize = 0x028;
const MMIO_QUEUE_SEL: usize = 0x030;
const MMIO_QUEUE_NUM_MAX: usize = 0x034;
const MMIO_QUEUE_NUM: usize = 0x038;
const MMIO_QUEUE_ALIGN: usize = 0x03C;
const MMIO_QUEUE_PFN: usize = 0x040;
const MMIO_QUEUE_READY: usize = 0x044;
const MMIO_QUEUE_NOTIFY: usize = 0x050;
const MMIO_INTERRUPT_STATUS: usize = 0x060;
const MMIO_INTERRUPT_ACK: usize = 0x064;
const MMIO_STATUS: usize = 0x070;
const MMIO_QUEUE_DESC_LOW: usize = 0x080;
const MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const MMIO_QUEUE_DRIVER_LOW: usize = 0x090;
const MMIO_QUEUE_DRIVER_HIGH: usize = 0x094;
const MMIO_QUEUE_DEVICE_LOW: usize = 0x0A0;
const MMIO_QUEUE_DEVICE_HIGH: usize = 0x0A4;
const MMIO_CONFIG: usize = 0x100;

pub struct MmioTransport {
    base: usize,
    version: u32,
    id: u32,
}

impl MmioTransport {
    pub fn probe(base: u64, size: u64) -> Option<MmioTransport> {
        if size < 0x100 || !crate::arch::paging::map_kernel_mmio(base, size) {
            return None;
        }
        let transport = MmioTransport { base: base as usize, version: 0, id: 0 };
        if transport.read(0) != MAGIC {
            return None;
        }
        let version = transport.read(MMIO_VERSION);
        let id = transport.read(MMIO_DEVICE_ID);
        if id == 0 || !(1..=2).contains(&version) {
            return None;
        }
        Some(MmioTransport { base: base as usize, version, id })
    }

    fn read(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base + offset) as *const u32) }
    }

    fn write(&self, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.base + offset) as *mut u32, value) }
    }
}

impl Transport for MmioTransport {
    fn device_id(&self) -> u32 {
        self.id
    }

    fn modern(&self) -> bool {
        self.version == 2
    }

    fn status(&self) -> u8 {
        self.read(MMIO_STATUS) as u8
    }

    fn set_status(&mut self, status: u8) {
        self.write(MMIO_STATUS, status as u32);
    }

    fn device_features(&mut self) -> u64 {
        self.write(MMIO_DEVICE_FEATURES_SEL, 0);
        let low = self.read(MMIO_DEVICE_FEATURES) as u64;
        self.write(MMIO_DEVICE_FEATURES_SEL, 1);
        let high = self.read(MMIO_DEVICE_FEATURES) as u64;
        low | high << 32
    }

    fn set_driver_features(&mut self, features: u64) {
        self.write(MMIO_DRIVER_FEATURES_SEL, 0);
        self.write(MMIO_DRIVER_FEATURES, features as u32);
        self.write(MMIO_DRIVER_FEATURES_SEL, 1);
        self.write(MMIO_DRIVER_FEATURES, (features >> 32) as u32);
    }

    fn queue_max(&mut self, index: u16) -> u16 {
        self.write(MMIO_QUEUE_SEL, index as u32);
        self.read(MMIO_QUEUE_NUM_MAX) as u16
    }

    fn setup_queue(&mut self, index: u16, size: u16, desc: u64, avail: u64, used: u64) -> bool {
        self.write(MMIO_QUEUE_SEL, index as u32);
        self.write(MMIO_QUEUE_NUM, size as u32);
        if self.version == 1 {
            self.write(MMIO_GUEST_PAGE_SIZE, 4096);
            self.write(MMIO_QUEUE_ALIGN, 4096);
            self.write(MMIO_QUEUE_PFN, (desc >> 12) as u32);
            return self.read(MMIO_QUEUE_PFN) != 0;
        }
        self.write(MMIO_QUEUE_DESC_LOW, desc as u32);
        self.write(MMIO_QUEUE_DESC_HIGH, (desc >> 32) as u32);
        self.write(MMIO_QUEUE_DRIVER_LOW, avail as u32);
        self.write(MMIO_QUEUE_DRIVER_HIGH, (avail >> 32) as u32);
        self.write(MMIO_QUEUE_DEVICE_LOW, used as u32);
        self.write(MMIO_QUEUE_DEVICE_HIGH, (used >> 32) as u32);
        self.write(MMIO_QUEUE_READY, 1);
        self.read(MMIO_QUEUE_READY) == 1
    }

    fn notify(&self, index: u16) {
        self.write(MMIO_QUEUE_NOTIFY, index as u32);
    }

    fn config8(&self, offset: usize) -> u8 {
        unsafe { core::ptr::read_volatile((self.base + MMIO_CONFIG + offset) as *const u8) }
    }

    fn config32(&self, offset: usize) -> u32 {
        if offset % 4 == 0 {
            return self.read(MMIO_CONFIG + offset);
        }
        u32::from_le_bytes([self.config8(offset), self.config8(offset + 1), self.config8(offset + 2), self.config8(offset + 3)])
    }

    fn write_config32(&self, offset: usize, value: u32) {
        self.write(MMIO_CONFIG + offset, value);
    }

    fn write_config8(&self, offset: usize, value: u8) {
        unsafe { core::ptr::write_volatile((self.base + MMIO_CONFIG + offset) as *mut u8, value) }
    }

    fn ack_interrupt(&self) -> u32 {
        let status = self.read(MMIO_INTERRUPT_STATUS);
        if status != 0 {
            self.write(MMIO_INTERRUPT_ACK, status);
        }
        status
    }
}

pub fn legacy_pci_id(device: u16) -> u32 {
    match device {
        0x1000 => 1,
        0x1001 => 2,
        0x1003 => 3,
        0x1005 => 4,
        _ => 0,
    }
}

const CAP_VENDOR: u8 = 0x09;
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_ISR: u8 = 3;
const CFG_DEVICE: u8 = 4;

const COMMON_DEVICE_FEATURE_SELECT: usize = 0x00;
const COMMON_DEVICE_FEATURE: usize = 0x04;
const COMMON_DRIVER_FEATURE_SELECT: usize = 0x08;
const COMMON_DRIVER_FEATURE: usize = 0x0C;
const COMMON_DEVICE_STATUS: usize = 0x14;
const COMMON_QUEUE_SELECT: usize = 0x16;
const COMMON_QUEUE_SIZE: usize = 0x18;
const COMMON_QUEUE_ENABLE: usize = 0x1C;
const COMMON_QUEUE_NOTIFY_OFF: usize = 0x1E;
const COMMON_QUEUE_DESC: usize = 0x20;
const COMMON_QUEUE_DRIVER: usize = 0x28;
const COMMON_QUEUE_DEVICE: usize = 0x30;

pub struct PciTransport {
    id: u32,
    common: usize,
    notify: usize,
    notify_multiplier: u32,
    isr: usize,
    device: usize,
    notify_offsets: [u16; 8],
}

fn cfg8(address: PciAddress, offset: u8) -> u8 {
    (pci::read_config_u32(address, offset & !3) >> ((offset & 3) * 8)) as u8
}

impl PciTransport {
    pub fn probe(address: PciAddress) -> Option<PciTransport> {
        let id_word = pci::read_config_u32(address, 0);
        let device = (id_word >> 16) as u16;
        let id = if device >= 0x1040 { (device - 0x1040) as u32 } else { legacy_pci_id(device) };
        if pci::read_config_u16(address, 0x06) & 0x10 == 0 {
            return None;
        }
        let bars = pci::bars(address);
        let mut regions = [0usize; 5];
        let mut multiplier = 0u32;
        let mut at = cfg8(address, 0x34) & 0xFC;
        let mut guard = 0;
        while at != 0 && guard < 48 {
            guard += 1;
            let kind_id = cfg8(address, at);
            let next = cfg8(address, at + 1) & 0xFC;
            if kind_id == CAP_VENDOR {
                let kind = cfg8(address, at + 3);
                let bar = cfg8(address, at + 4);
                let offset = pci::read_config_u32(address, at + 8) as u64;
                let len = pci::read_config_u32(address, at + 12) as u64;
                if let Some((_, info)) = bars.iter().find(|(index, _)| *index == bar) {
                    if !info.io && info.base != 0 && (kind as usize) < regions.len() && regions[kind as usize] == 0 {
                        let phys = info.base + offset;
                        if crate::arch::paging::map_kernel_mmio(phys, len.max(4)) {
                            regions[kind as usize] = phys as usize;
                            if kind == CFG_NOTIFY {
                                multiplier = pci::read_config_u32(address, at + 16);
                            }
                        }
                    }
                }
            }
            at = next;
        }
        if regions[CFG_COMMON as usize] == 0 || regions[CFG_NOTIFY as usize] == 0 {
            return None;
        }
        pci::enable_bus_master(address);
        Some(PciTransport {
            id,
            common: regions[CFG_COMMON as usize],
            notify: regions[CFG_NOTIFY as usize],
            notify_multiplier: multiplier,
            isr: regions[CFG_ISR as usize],
            device: regions[CFG_DEVICE as usize],
            notify_offsets: [0; 8],
        })
    }

    fn read8(&self, offset: usize) -> u8 {
        unsafe { core::ptr::read_volatile((self.common + offset) as *const u8) }
    }

    fn write8(&self, offset: usize, value: u8) {
        unsafe { core::ptr::write_volatile((self.common + offset) as *mut u8, value) }
    }

    fn read16(&self, offset: usize) -> u16 {
        unsafe { core::ptr::read_volatile((self.common + offset) as *const u16) }
    }

    fn write16(&self, offset: usize, value: u16) {
        unsafe { core::ptr::write_volatile((self.common + offset) as *mut u16, value) }
    }

    fn read32(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.common + offset) as *const u32) }
    }

    fn write32(&self, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.common + offset) as *mut u32, value) }
    }

    fn write64(&self, offset: usize, value: u64) {
        self.write32(offset, value as u32);
        self.write32(offset + 4, (value >> 32) as u32);
    }
}

impl Transport for PciTransport {
    fn device_id(&self) -> u32 {
        self.id
    }

    fn modern(&self) -> bool {
        true
    }

    fn status(&self) -> u8 {
        self.read8(COMMON_DEVICE_STATUS)
    }

    fn set_status(&mut self, status: u8) {
        self.write8(COMMON_DEVICE_STATUS, status);
    }

    fn device_features(&mut self) -> u64 {
        self.write32(COMMON_DEVICE_FEATURE_SELECT, 0);
        let low = self.read32(COMMON_DEVICE_FEATURE) as u64;
        self.write32(COMMON_DEVICE_FEATURE_SELECT, 1);
        let high = self.read32(COMMON_DEVICE_FEATURE) as u64;
        low | high << 32
    }

    fn set_driver_features(&mut self, features: u64) {
        self.write32(COMMON_DRIVER_FEATURE_SELECT, 0);
        self.write32(COMMON_DRIVER_FEATURE, features as u32);
        self.write32(COMMON_DRIVER_FEATURE_SELECT, 1);
        self.write32(COMMON_DRIVER_FEATURE, (features >> 32) as u32);
    }

    fn queue_max(&mut self, index: u16) -> u16 {
        self.write16(COMMON_QUEUE_SELECT, index);
        self.read16(COMMON_QUEUE_SIZE)
    }

    fn setup_queue(&mut self, index: u16, size: u16, desc: u64, avail: u64, used: u64) -> bool {
        self.write16(COMMON_QUEUE_SELECT, index);
        self.write16(COMMON_QUEUE_SIZE, size);
        self.write64(COMMON_QUEUE_DESC, desc);
        self.write64(COMMON_QUEUE_DRIVER, avail);
        self.write64(COMMON_QUEUE_DEVICE, used);
        if (index as usize) < self.notify_offsets.len() {
            self.notify_offsets[index as usize] = self.read16(COMMON_QUEUE_NOTIFY_OFF);
        }
        self.write16(COMMON_QUEUE_ENABLE, 1);
        self.read16(COMMON_QUEUE_ENABLE) == 1
    }

    fn notify(&self, index: u16) {
        let offset = self.notify_offsets.get(index as usize).copied().unwrap_or(0) as usize * self.notify_multiplier as usize;
        unsafe { core::ptr::write_volatile((self.notify + offset) as *mut u16, index) };
    }

    fn config8(&self, offset: usize) -> u8 {
        if self.device == 0 {
            return 0;
        }
        unsafe { core::ptr::read_volatile((self.device + offset) as *const u8) }
    }

    fn config32(&self, offset: usize) -> u32 {
        if self.device == 0 {
            return 0;
        }
        if offset % 4 == 0 {
            return unsafe { core::ptr::read_volatile((self.device + offset) as *const u32) };
        }
        u32::from_le_bytes([self.config8(offset), self.config8(offset + 1), self.config8(offset + 2), self.config8(offset + 3)])
    }

    fn write_config32(&self, offset: usize, value: u32) {
        if self.device != 0 {
            unsafe { core::ptr::write_volatile((self.device + offset) as *mut u32, value) };
        }
    }

    fn write_config8(&self, offset: usize, value: u8) {
        if self.device != 0 {
            unsafe { core::ptr::write_volatile((self.device + offset) as *mut u8, value) };
        }
    }

    fn ack_interrupt(&self) -> u32 {
        if self.isr == 0 {
            return 0;
        }
        unsafe { core::ptr::read_volatile(self.isr as *const u8) as u32 }
    }
}
