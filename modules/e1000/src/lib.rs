#![no_std]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use core::ffi::c_void;

use hamix_kpi::io::{delay_us, DmaRegion, Mmio};
use hamix_kpi::{self as kpi, NetOps, PciHandle};

const CTRL: u32 = 0x0000;
const STATUS: u32 = 0x0008;
const EERD: u32 = 0x0014;
const ICR: u32 = 0x00C0;
const IMC: u32 = 0x00D8;
const RCTL: u32 = 0x0100;
const TCTL: u32 = 0x0400;
const TIPG: u32 = 0x0410;
const RDBAL: u32 = 0x2800;
const RDBAH: u32 = 0x2804;
const RDLEN: u32 = 0x2808;
const RDH: u32 = 0x2810;
const RDT: u32 = 0x2818;
const TDBAL: u32 = 0x3800;
const TDBAH: u32 = 0x3804;
const TDLEN: u32 = 0x3808;
const TDH: u32 = 0x3810;
const TDT: u32 = 0x3818;
const MTA: u32 = 0x5200;
const RAL0: u32 = 0x5400;
const RAH0: u32 = 0x5404;

const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;
const CTRL_RST: u32 = 1 << 26;
const CTRL_LRST: u32 = 1 << 3;
const CTRL_PHY_RST: u32 = 1 << 31;
const CTRL_ILOS: u32 = 1 << 7;
const CTRL_VME: u32 = 1 << 30;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;

const RX_COUNT: usize = 64;
const TX_COUNT: usize = 32;
const BUFFER: usize = 2048;
const RX_BUDGET: usize = 48;

const IDS: &[(u16, &str)] = &[
    (0x100E, "Intel 82540EM"),
    (0x100F, "Intel 82545EM"),
    (0x1004, "Intel 82543GC"),
    (0x100C, "Intel 82544GC"),
    (0x1015, "Intel 82540EM (LOM)"),
    (0x1026, "Intel 82545GM"),
    (0x1076, "Intel 82541GI"),
    (0x107C, "Intel 82541PI"),
];

#[repr(C)]
#[derive(Clone, Copy)]
struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

struct Nic {
    mmio: Mmio,
    mac: [u8; 6],
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

impl Nic {
    fn read_eeprom(&self, word: u32) -> Option<u16> {
        self.mmio.write32(EERD, 1 | (word << 2));
        for _ in 0..1000 {
            let v = self.mmio.read32(EERD);
            if v & (1 << 1) != 0 {
                return Some((v >> 16) as u16);
            }
            delay_us(5);
        }
        None
    }

    fn rx(&self, i: usize) -> *mut RxDesc {
        self.rx_ring.ptr::<RxDesc>(i * 16)
    }

    fn tx(&self, i: usize) -> *mut TxDesc {
        self.tx_ring.ptr::<TxDesc>(i * 16)
    }

    fn start(handle: &PciHandle) -> Option<Nic> {
        let (base, len) = kpi::bar(handle, 0);
        let mmio = Mmio::map(base, len)?;
        unsafe { kpi::hamix_pci_enable(handle) };
        let mut nic = Nic {
            mmio,
            mac: [0; 6],
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
        let m = &nic.mmio;
        m.write32(IMC, 0xFFFF_FFFF);
        m.write32(CTRL, m.read32(CTRL) | CTRL_RST);
        delay_us(20_000);
        for _ in 0..100 {
            if m.read32(CTRL) & CTRL_RST == 0 {
                break;
            }
            delay_us(1000);
        }
        m.write32(IMC, 0xFFFF_FFFF);
        let _ = m.read32(ICR);
        let ctrl = (m.read32(CTRL) | CTRL_SLU | CTRL_ASDE) & !(CTRL_LRST | CTRL_PHY_RST | CTRL_ILOS | CTRL_VME);
        m.write32(CTRL, ctrl);

        let ral = m.read32(RAL0);
        let rah = m.read32(RAH0);
        if rah & (1 << 31) != 0 {
            nic.mac = [ral as u8, (ral >> 8) as u8, (ral >> 16) as u8, (ral >> 24) as u8, rah as u8, (rah >> 8) as u8];
        } else if let (Some(a), Some(b), Some(c)) = (nic.read_eeprom(0), nic.read_eeprom(1), nic.read_eeprom(2)) {
            nic.mac = [a as u8, (a >> 8) as u8, b as u8, (b >> 8) as u8, c as u8, (c >> 8) as u8];
            nic.mmio.write32(RAL0, u32::from_le_bytes([nic.mac[0], nic.mac[1], nic.mac[2], nic.mac[3]]));
            nic.mmio.write32(RAH0, nic.mac[4] as u32 | (nic.mac[5] as u32) << 8 | 1 << 31);
        } else {
            return None;
        }
        for i in 0..128 {
            nic.mmio.write32(MTA + i * 4, 0);
        }
        for i in 0..RX_COUNT {
            unsafe { *nic.rx(i) = RxDesc { addr: nic.rx_buffers.phys + (i * BUFFER) as u64, length: 0, checksum: 0, status: 0, errors: 0, special: 0 } };
        }
        let m = &nic.mmio;
        m.write32(RDBAL, nic.rx_ring.phys as u32);
        m.write32(RDBAH, (nic.rx_ring.phys >> 32) as u32);
        m.write32(RDLEN, (RX_COUNT * 16) as u32);
        m.write32(RDH, 0);
        m.write32(RDT, (RX_COUNT - 1) as u32);
        m.write32(RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
        for i in 0..TX_COUNT {
            unsafe { *nic.tx(i) = TxDesc { addr: nic.tx_buffers.phys + (i * BUFFER) as u64, length: 0, cso: 0, cmd: 0, status: 1, css: 0, special: 0 } };
        }
        let m = &nic.mmio;
        m.write32(TDBAL, nic.tx_ring.phys as u32);
        m.write32(TDBAH, (nic.tx_ring.phys >> 32) as u32);
        m.write32(TDLEN, (TX_COUNT * 16) as u32);
        m.write32(TDH, 0);
        m.write32(TDT, 0);
        m.write32(TCTL, TCTL_EN | TCTL_PSP | (0x0F << 4) | (0x40 << 12));
        m.write32(TIPG, 0x0060_200A);
        Some(nic)
    }

    fn drain(&mut self) {
        for _ in 0..RX_BUDGET {
            let desc = unsafe { &mut *self.rx(self.rx_next) };
            if desc.status & 1 == 0 {
                return;
            }
            let length = desc.length as usize;
            if desc.status & 2 != 0 && desc.errors == 0 && length <= BUFFER && self.enabled {
                kpi::net_receive(self.id, self.rx_buffers.slice(self.rx_next * BUFFER, length));
            }
            desc.status = 0;
            let index = self.rx_next;
            self.rx_next = (self.rx_next + 1) % RX_COUNT;
            self.mmio.write32(RDT, index as u32);
        }
    }

    fn stop(&self) {
        self.mmio.write32(IMC, 0xFFFF_FFFF);
        self.mmio.write32(RCTL, 0);
        self.mmio.write32(TCTL, 0);
    }
}

extern "C" fn transmit(context: u64, frame: *const u8, len: usize) -> i32 {
    let Some(nic) = nic(context) else {
        return -19;
    };
    if frame.is_null() || len > BUFFER || !nic.enabled {
        return -22;
    }
    let desc = unsafe { &mut *nic.tx(nic.tx_next) };
    if desc.status & 1 == 0 {
        return -11;
    }
    let data = unsafe { core::slice::from_raw_parts(frame, len) };
    nic.tx_buffers.slice(nic.tx_next * BUFFER, len).copy_from_slice(data);
    desc.length = len as u16;
    desc.cmd = 0x01 | 0x02 | 0x08;
    desc.status = 0;
    nic.tx_next = (nic.tx_next + 1) % TX_COUNT;
    nic.mmio.write32(TDT, nic.tx_next as u32);
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
    let carrier = nic.mmio.read32(STATUS) & 2 != 0;
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
    for (device, model) in IDS {
        let mut handle = PciHandle::default();
        if unsafe { kpi::hamix_pci_find(0x8086, *device, &mut handle) } != 0 {
            continue;
        }
        let Some(mut nic) = Nic::start(&handle) else {
            kpi::dev_err(&format!("{} did not come out of reset", model));
            continue;
        };
        let index = nics().len();
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
        if !kpi::claim(kpi::CLASS_NETWORK, "e1000", &handle, Some(poll), index as *mut c_void) {
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
