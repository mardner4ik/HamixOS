#![no_std]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use hamix_kpi::io::{delay_us, DmaRegion, Mmio};
use hamix_kpi::{self as kpi, NetOps, PciHandle};

const IDR0: u32 = 0x00;
const MAR0: u32 = 0x08;
const TX_DESC_LOW: u32 = 0x20;
const TX_DESC_HIGH: u32 = 0x24;
const CHIP_CMD: u32 = 0x37;
const TX_POLL: u32 = 0x38;
const INTR_MASK: u32 = 0x3C;
const INTR_STATUS: u32 = 0x3E;
const TX_CONFIG: u32 = 0x40;
const RX_CONFIG: u32 = 0x44;
const CFG9346: u32 = 0x50;
const PHYAR: u32 = 0x60;
const PHY_STATUS: u32 = 0x6C;
const PMCH: u32 = 0x6F;
const RX_MAX_SIZE: u32 = 0xDA;
const CPLUS_CMD: u32 = 0xE0;
const INTR_MITIGATE: u32 = 0xE2;
const RX_DESC_LOW: u32 = 0xE4;
const RX_DESC_HIGH: u32 = 0xE8;
const MAX_TX_PACKET: u32 = 0xEC;
const MISC: u32 = 0xF0;

const CMD_RESET: u8 = 0x10;
const CMD_RX: u8 = 0x08;
const CMD_TX: u8 = 0x04;
const TX_POLL_NORMAL: u8 = 0x40;
const CFG_UNLOCK: u8 = 0xC0;
const CFG_LOCK: u8 = 0x00;
const LINK_UP: u8 = 0x02;

const ACCEPT_BROADCAST: u32 = 0x08;
const ACCEPT_MULTICAST: u32 = 0x04;
const ACCEPT_MINE: u32 = 0x02;
const RX128_INT_EN: u32 = 1 << 15;
const RX_MULTI_EN: u32 = 1 << 14;
const RX_EARLY_OFF: u32 = 1 << 11;
const RX_DMA_BURST: u32 = 7 << 8;
const TX_DMA_BURST: u32 = 7 << 8;
const TX_IFG: u32 = 3 << 24;
const TX_AUTO_FIFO: u32 = 1 << 7;
const RXDV_GATED: u32 = 1 << 19;
const CPLUS_RX_VLAN: u16 = 1 << 6;
const CPLUS_RX_CHECKSUM: u16 = 1 << 5;

const OWN: u32 = 1 << 31;
const RING_END: u32 = 1 << 30;
const FIRST: u32 = 1 << 29;
const LAST: u32 = 1 << 28;
const RX_ERROR: u32 = 1 << 21;

const MII_BMCR: u32 = 0x00;
const MII_ADVERTISE: u32 = 0x04;
const MII_CTRL1000: u32 = 0x09;
const MII_PAGE: u32 = 0x1F;
const BMCR_ANENABLE: u16 = 1 << 12;
const BMCR_ANRESTART: u16 = 1 << 9;
const BMCR_PDOWN: u16 = 1 << 11;

const RX_COUNT: usize = 128;
const TX_COUNT: usize = 64;
const BUFFER: usize = 2048;
const RX_BUDGET: usize = 96;
const MIN_FRAME: usize = 60;

const IDS: &[u16] = &[0x8168, 0x8161];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    B,
    C,
    D,
    E,
    EVl,
    F,
    G,
}

fn family(xid: u32) -> Family {
    match xid & 0x7C8 {
        0x300 | 0x380 | 0x180 | 0x100 => Family::B,
        0x3C0 | 0x3C8 => Family::C,
        0x280 | 0x288 => Family::D,
        0x2C0 => Family::E,
        0x2C8 => Family::EVl,
        0x480 | 0x488 => Family::F,
        _ => Family::G,
    }
}

fn family_name(family: Family) -> &'static str {
    match family {
        Family::B => "RTL8111B/8168B",
        Family::C => "RTL8111C/8168C",
        Family::D => "RTL8111D/8168D",
        Family::E => "RTL8111E/8168E",
        Family::EVl => "RTL8111E-VL/8168E-VL",
        Family::F => "RTL8111F/8168F",
        Family::G => "RTL8111G or newer",
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Desc {
    opts1: u32,
    opts2: u32,
    addr: u64,
}

struct Nic {
    mmio: Mmio,
    mac: [u8; 6],
    family: Family,
    rx_ring: DmaRegion,
    rx_buffers: DmaRegion,
    tx_ring: DmaRegion,
    tx_buffers: DmaRegion,
    rx_next: usize,
    tx_next: usize,
    id: i32,
    carrier: bool,
    enabled: bool,
}

static mut NICS: Vec<Nic> = Vec::new();

fn nics() -> &'static mut Vec<Nic> {
    unsafe { &mut *(&raw mut NICS) }
}

fn nic(context: u64) -> Option<&'static mut Nic> {
    nics().get_mut(context as usize)
}

fn mmio_bar(handle: &PciHandle) -> Option<(u64, u64)> {
    let mut index = 0u8;
    while index < 6 {
        let raw = unsafe { kpi::hamix_pci_read32(handle, 0x10 + index * 4) };
        if raw & 1 == 0 && raw != 0 {
            let (base, len) = kpi::bar(handle, index);
            if base != 0 && len >= 256 {
                return Some((base, len.min(4096)));
            }
        }
        index += if raw & 1 == 0 && (raw >> 1) & 3 == 2 { 2 } else { 1 };
    }
    None
}

impl Nic {
    fn rx(&self, i: usize) -> *mut Desc {
        self.rx_ring.ptr::<Desc>(i * 16)
    }

    fn tx(&self, i: usize) -> *mut Desc {
        self.tx_ring.ptr::<Desc>(i * 16)
    }

    fn phy_write(&self, reg: u32, value: u16) {
        self.mmio.write32(PHYAR, 0x8000_0000 | (reg & 0x1F) << 16 | value as u32);
        for _ in 0..100 {
            delay_us(25);
            if self.mmio.read32(PHYAR) & 0x8000_0000 == 0 {
                break;
            }
        }
        delay_us(20);
    }

    fn phy_read(&self, reg: u32) -> Option<u16> {
        self.mmio.write32(PHYAR, (reg & 0x1F) << 16);
        for _ in 0..100 {
            delay_us(25);
            let value = self.mmio.read32(PHYAR);
            if value & 0x8000_0000 != 0 {
                delay_us(20);
                return Some(value as u16);
            }
        }
        None
    }

    fn reset(&self) -> bool {
        let m = &self.mmio;
        m.write32(RX_CONFIG, m.read32(RX_CONFIG) & !(ACCEPT_BROADCAST | ACCEPT_MULTICAST | ACCEPT_MINE | 1));
        if self.family >= Family::G {
            m.write32(MISC, m.read32(MISC) | RXDV_GATED);
            delay_us(2000);
        }
        m.write8(CHIP_CMD, CMD_RESET);
        for _ in 0..100 {
            delay_us(1000);
            if m.read8(CHIP_CMD) & CMD_RESET == 0 {
                return true;
            }
        }
        false
    }

    fn start_phy(&self) {
        if self.family >= Family::G {
            return;
        }
        if matches!(self.family, Family::D | Family::E) {
            self.mmio.write8(PMCH, self.mmio.read8(PMCH) | 0x80);
        }
        self.phy_write(MII_PAGE, 0);
        let bmcr = self.phy_read(MII_BMCR).unwrap_or(0);
        if bmcr & BMCR_ANENABLE != 0 && bmcr & BMCR_PDOWN == 0 {
            return;
        }
        let advertise = self.phy_read(MII_ADVERTISE).unwrap_or(0);
        self.phy_write(MII_ADVERTISE, (advertise & !0x0DE0) | 0x01E1 | 0x0C00);
        let gigabit = self.phy_read(MII_CTRL1000).unwrap_or(0);
        self.phy_write(MII_CTRL1000, gigabit | 0x0300);
        self.phy_write(MII_BMCR, (bmcr & !BMCR_PDOWN) | BMCR_ANENABLE | BMCR_ANRESTART);
    }

    fn start(handle: &PciHandle) -> Option<Nic> {
        let (base, len) = mmio_bar(handle)?;
        let mmio = Mmio::map(base, len)?;
        unsafe { kpi::hamix_pci_enable(handle) };
        let xid = (mmio.read32(TX_CONFIG) >> 20) & 0x7CF;
        let nic = Nic {
            mmio,
            mac: [0; 6],
            family: family(xid),
            rx_ring: DmaRegion::new(RX_COUNT * 16)?,
            rx_buffers: DmaRegion::new(RX_COUNT * BUFFER)?,
            tx_ring: DmaRegion::new(TX_COUNT * 16)?,
            tx_buffers: DmaRegion::new(TX_COUNT * BUFFER)?,
            rx_next: 0,
            tx_next: 0,
            id: 0,
            carrier: false,
            enabled: true,
        };
        kpi::dev_info(&format!("{} (XID {:03x})", family_name(nic.family), xid));
        let m = &nic.mmio;
        m.write16(INTR_MASK, 0);
        m.write16(INTR_STATUS, 0xFFFF);
        if !nic.reset() {
            return None;
        }
        let mut nic = nic;
        for i in 0..6 {
            nic.mac[i] = nic.mmio.read8(IDR0 + i as u32);
        }
        if nic.mac == [0; 6] || nic.mac == [0xFF; 6] || nic.mac[0] & 1 != 0 {
            kpi::dev_err("no valid MAC address in the IDR registers");
            return None;
        }
        nic.start_phy();
        for i in 0..RX_COUNT {
            let end = if i == RX_COUNT - 1 { RING_END } else { 0 };
            let desc = Desc { opts1: OWN | end | BUFFER as u32, opts2: 0, addr: nic.rx_buffers.phys + (i * BUFFER) as u64 };
            unsafe { write_volatile(nic.rx(i), desc) };
        }
        for i in 0..TX_COUNT {
            let end = if i == TX_COUNT - 1 { RING_END } else { 0 };
            let desc = Desc { opts1: end, opts2: 0, addr: nic.tx_buffers.phys + (i * BUFFER) as u64 };
            unsafe { write_volatile(nic.tx(i), desc) };
        }
        fence(Ordering::SeqCst);
        let m = &nic.mmio;
        m.write8(CFG9346, CFG_UNLOCK);
        let cplus = m.read16(CPLUS_CMD) & !(CPLUS_RX_VLAN | CPLUS_RX_CHECKSUM);
        m.write16(CPLUS_CMD, cplus);
        m.write16(INTR_MITIGATE, 0);
        m.write16(RX_MAX_SIZE, BUFFER as u16);
        m.write8(MAX_TX_PACKET, if nic.family >= Family::EVl { 0x27 } else { 0x3F });
        m.write32(TX_DESC_HIGH, (nic.tx_ring.phys >> 32) as u32);
        m.write32(TX_DESC_LOW, nic.tx_ring.phys as u32);
        m.write32(RX_DESC_HIGH, (nic.rx_ring.phys >> 32) as u32);
        m.write32(RX_DESC_LOW, nic.rx_ring.phys as u32);
        if nic.family >= Family::G {
            m.write32(MISC, m.read32(MISC) & !RXDV_GATED);
        }
        m.write8(CHIP_CMD, CMD_TX | CMD_RX);
        let auto_fifo = if nic.family >= Family::EVl { TX_AUTO_FIFO } else { 0 };
        m.write32(TX_CONFIG, TX_DMA_BURST | TX_IFG | auto_fifo);
        let rx_mode = match nic.family {
            Family::B => RX128_INT_EN | RX_DMA_BURST,
            Family::G => RX128_INT_EN | RX_MULTI_EN | RX_DMA_BURST | RX_EARLY_OFF,
            _ => RX128_INT_EN | RX_MULTI_EN | RX_DMA_BURST,
        };
        m.write32(MAR0, 0xFFFF_FFFF);
        m.write32(MAR0 + 4, 0xFFFF_FFFF);
        m.write32(RX_CONFIG, rx_mode | ACCEPT_BROADCAST | ACCEPT_MULTICAST | ACCEPT_MINE);
        m.write8(CFG9346, CFG_LOCK);
        let _ = m.read8(CHIP_CMD);
        Some(nic)
    }

    fn drain(&mut self) {
        for _ in 0..RX_BUDGET {
            let slot = self.rx(self.rx_next);
            let status = unsafe { read_volatile(&raw const (*slot).opts1) };
            if status & OWN != 0 {
                return;
            }
            fence(Ordering::Acquire);
            let length = (status & 0x3FFF) as usize;
            let whole = status & (FIRST | LAST) == FIRST | LAST;
            if whole && status & RX_ERROR == 0 && length > 4 && length <= BUFFER && self.enabled {
                kpi::net_receive(self.id, self.rx_buffers.slice(self.rx_next * BUFFER, length - 4));
            }
            let end = if self.rx_next == RX_COUNT - 1 { RING_END } else { 0 };
            unsafe { write_volatile(&raw mut (*slot).opts2, 0) };
            fence(Ordering::Release);
            unsafe { write_volatile(&raw mut (*slot).opts1, OWN | end | BUFFER as u32) };
            self.rx_next = (self.rx_next + 1) % RX_COUNT;
        }
    }

    fn stop(&self) {
        self.mmio.write16(INTR_MASK, 0);
        self.mmio.write8(CHIP_CMD, 0);
        self.reset();
    }
}

extern "C" fn transmit(context: u64, frame: *const u8, len: usize) -> i32 {
    let Some(nic) = nic(context) else {
        return -19;
    };
    if frame.is_null() || len > BUFFER || !nic.enabled {
        return -22;
    }
    let slot = nic.tx(nic.tx_next);
    if unsafe { read_volatile(&raw const (*slot).opts1) } & OWN != 0 {
        nic.mmio.write8(TX_POLL, TX_POLL_NORMAL);
        return -11;
    }
    let data = unsafe { core::slice::from_raw_parts(frame, len) };
    let size = len.max(MIN_FRAME);
    let buffer = nic.tx_buffers.slice(nic.tx_next * BUFFER, size);
    buffer[..len].copy_from_slice(data);
    buffer[len..].fill(0);
    let end = if nic.tx_next == TX_COUNT - 1 { RING_END } else { 0 };
    unsafe { write_volatile(&raw mut (*slot).opts2, 0) };
    fence(Ordering::Release);
    unsafe { write_volatile(&raw mut (*slot).opts1, OWN | FIRST | LAST | end | size as u32) };
    fence(Ordering::SeqCst);
    nic.tx_next = (nic.tx_next + 1) % TX_COUNT;
    nic.mmio.write8(TX_POLL, TX_POLL_NORMAL);
    0
}

extern "C" fn set_enabled(context: u64, enabled: i32) {
    if let Some(nic) = nic(context) {
        nic.enabled = enabled != 0;
    }
}

extern "C" fn poll(context: *mut c_void) {
    let Some(nic) = nic(context as u64) else {
        return;
    };
    let status = nic.mmio.read16(INTR_STATUS);
    if status != 0 && status != 0xFFFF {
        nic.mmio.write16(INTR_STATUS, status);
    }
    let carrier = nic.mmio.read8(PHY_STATUS) & LINK_UP != 0;
    if carrier != nic.carrier {
        nic.carrier = carrier;
        kpi::net_carrier(nic.id, carrier);
    }
    nic.drain();
}

fn mac_text(mac: &[u8; 6]) -> alloc::string::String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5])
}

fn init() -> i32 {
    for device in IDS {
        let mut handle = PciHandle::default();
        if unsafe { kpi::hamix_pci_find(0x10EC, *device, &mut handle) } != 0 {
            continue;
        }
        let Some(mut nic) = Nic::start(&handle) else {
            kpi::dev_err(&format!("Realtek {:04x} did not come out of reset", device));
            continue;
        };
        let index = nics().len();
        let model = family_name(nic.family);
        let mut mac = [0u8; 8];
        mac[..6].copy_from_slice(&nic.mac);
        let ops = NetOps { abi: kpi::NET_ABI, flags: 0, context: index as u64, mac, transmit: Some(transmit), set_enabled: Some(set_enabled) };
        let id = kpi::register_netdev(model, &ops);
        if id <= 0 {
            nic.stop();
            continue;
        }
        nic.id = id;
        kpi::dev_info(&format!("{} {}", model, mac_text(&nic.mac)));
        nics().push(nic);
        if !kpi::claim(kpi::CLASS_NETWORK, "r8168", &handle, Some(poll), index as *mut c_void) {
            return -1;
        }
    }
    if nics().is_empty() { -19 } else { 0 }
}

fn exit() {
    for nic in nics().iter() {
        nic.stop();
        unsafe { kpi::hamix_unregister_netdev(nic.id) };
    }
    nics().clear();
}

hamix_kpi::kernel_heap!();

hamix_kpi::module!(init = init, exit = exit, version = "1.0");
