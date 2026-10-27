use hamix_kpi::{self as kpi, PciHandle};

pub const CFG_COMMON: u8 = 1;
pub const CFG_NOTIFY: u8 = 2;
pub const CFG_ISR: u8 = 3;
pub const CFG_DEVICE: u8 = 4;

pub const STATUS_ACKNOWLEDGE: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;

pub const COMMON_DEVICE_FEATURE_SELECT: usize = 0x00;
pub const COMMON_DEVICE_FEATURE: usize = 0x04;
pub const COMMON_DRIVER_FEATURE_SELECT: usize = 0x08;
pub const COMMON_DRIVER_FEATURE: usize = 0x0C;
pub const COMMON_NUM_QUEUES: usize = 0x12;
pub const COMMON_DEVICE_STATUS: usize = 0x14;
pub const COMMON_QUEUE_SELECT: usize = 0x16;
pub const COMMON_QUEUE_SIZE: usize = 0x18;
pub const COMMON_QUEUE_ENABLE: usize = 0x1C;
pub const COMMON_QUEUE_NOTIFY_OFF: usize = 0x1E;
pub const COMMON_QUEUE_DESC: usize = 0x20;
pub const COMMON_QUEUE_DRIVER: usize = 0x28;
pub const COMMON_QUEUE_DEVICE: usize = 0x30;

#[derive(Clone, Copy, Default)]
pub struct Region {
    pub base: *mut u8,
    pub len: u32,
}

impl Region {
    pub fn is_valid(&self) -> bool {
        !self.base.is_null() && self.len > 0
    }
}

#[derive(Default)]
pub struct Transport {
    pub common: Region,
    pub notify: Region,
    pub isr: Region,
    pub device: Region,
    pub notify_multiplier: u32,
}

fn cfg8(handle: &PciHandle, offset: u8) -> u8 {
    let word = unsafe { kpi::hamix_pci_read32(handle, offset & !3) };
    (word >> ((offset & 3) * 8)) as u8
}

fn cfg32(handle: &PciHandle, offset: u8) -> u32 {
    unsafe { kpi::hamix_pci_read32(handle, offset) }
}

pub fn discover(handle: &PciHandle) -> Option<Transport> {
    let status = unsafe { kpi::hamix_pci_read16(handle, 0x06) };
    if status & 0x10 == 0 {
        return None;
    }
    let mut transport = Transport::default();
    let mut at = cfg8(handle, 0x34) & 0xFC;
    let mut guard = 0;
    while at != 0 && guard < 48 {
        guard += 1;
        let id = cfg8(handle, at);
        let next = cfg8(handle, at + 1) & 0xFC;
        if id == 0x09 {
            let kind = cfg8(handle, at + 3);
            let bar = cfg8(handle, at + 4);
            let offset = cfg32(handle, at + 8);
            let len = cfg32(handle, at + 12);
            let mut bar_len = 0u64;
            let base = unsafe { kpi::hamix_pci_bar(handle, bar, &mut bar_len) };
            if base != 0 && len > 0 {
                let region = Region { base: unsafe { kpi::hamix_ioremap(base + offset as u64, len as usize) }, len };
                match kind {
                    CFG_COMMON => transport.common = region,
                    CFG_NOTIFY => {
                        transport.notify = region;
                        transport.notify_multiplier = cfg32(handle, at + 16);
                    }
                    CFG_DEVICE => transport.device = region,
                    CFG_ISR => transport.isr = region,
                    _ => {}
                }
            }
        }
        at = next;
    }
    if transport.common.is_valid() && transport.notify.is_valid() { Some(transport) } else { None }
}

impl Transport {
    pub fn read8(&self, offset: usize) -> u8 {
        unsafe { kpi::hamix_readb(self.common.base.add(offset)) }
    }

    pub fn write8(&self, offset: usize, value: u8) {
        unsafe { kpi::hamix_writeb(value, self.common.base.add(offset)) }
    }

    pub fn read16(&self, offset: usize) -> u16 {
        unsafe { kpi::hamix_readw(self.common.base.add(offset) as *const u16) }
    }

    pub fn write16(&self, offset: usize, value: u16) {
        unsafe { kpi::hamix_writew(value, self.common.base.add(offset) as *mut u16) }
    }

    pub fn read32(&self, offset: usize) -> u32 {
        unsafe { kpi::hamix_readl(self.common.base.add(offset) as *const u32) }
    }

    pub fn write32(&self, offset: usize, value: u32) {
        unsafe { kpi::hamix_writel(value, self.common.base.add(offset) as *mut u32) }
    }

    pub fn write64(&self, offset: usize, value: u64) {
        self.write32(offset, value as u32);
        self.write32(offset + 4, (value >> 32) as u32);
    }

    pub fn status(&self) -> u8 {
        self.read8(COMMON_DEVICE_STATUS)
    }

    pub fn set_status(&self, value: u8) {
        self.write8(COMMON_DEVICE_STATUS, value);
    }

    pub fn add_status(&self, bits: u8) {
        let current = self.status();
        self.set_status(current | bits);
    }

    pub fn device_features(&self, select: u32) -> u32 {
        self.write32(COMMON_DEVICE_FEATURE_SELECT, select);
        self.read32(COMMON_DEVICE_FEATURE)
    }

    pub fn set_driver_features(&self, select: u32, value: u32) {
        self.write32(COMMON_DRIVER_FEATURE_SELECT, select);
        self.write32(COMMON_DRIVER_FEATURE, value);
    }

    pub fn num_queues(&self) -> u16 {
        self.read16(COMMON_NUM_QUEUES)
    }

    pub fn device32(&self, offset: usize) -> u32 {
        if !self.device.is_valid() {
            return 0;
        }
        unsafe { kpi::hamix_readl(self.device.base.add(offset) as *const u32) }
    }

    pub fn write_device32(&self, offset: usize, value: u32) {
        if self.device.is_valid() {
            unsafe { kpi::hamix_writel(value, self.device.base.add(offset) as *mut u32) }
        }
    }

    pub fn notify_address(&self, queue_notify_off: u16) -> *mut u16 {
        let offset = queue_notify_off as usize * self.notify_multiplier as usize;
        unsafe { self.notify.base.add(offset) as *mut u16 }
    }
}
