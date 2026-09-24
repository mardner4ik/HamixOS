pub mod hid;
pub mod uhci;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::drivers::pci::{self, PciDevice};

pub const CLASS_SERIAL_BUS: u8 = 0x0C;
pub const SUBCLASS_USB: u8 = 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControllerKind {
    Uhci,
    Ohci,
    Ehci,
    Xhci,
    Unknown,
}

impl ControllerKind {
    pub fn from_prog_if(prog_if: u8) -> Self {
        match prog_if {
            0x00 => Self::Uhci,
            0x10 => Self::Ohci,
            0x20 => Self::Ehci,
            0x30 => Self::Xhci,
            _ => Self::Unknown,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Uhci => "UHCI",
            Self::Ohci => "OHCI",
            Self::Ehci => "EHCI",
            Self::Xhci => "xHCI",
            Self::Unknown => "USB",
        }
    }
}

#[derive(Clone)]
pub struct Controller {
    pub kind: ControllerKind,
    pub device: PciDevice,
    pub base: u64,
    pub ports: u8,
    pub driven: bool,
    pub note: String,
}

pub static CONTROLLERS: Mutex<Vec<Controller>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceClass {
    HidMouse,
    HidKeyboard,
    HidOther,
    MassStorage,
    Hub,
    Other,
}

impl DeviceClass {
    pub fn name(&self) -> &'static str {
        match self {
            DeviceClass::HidMouse => "HID mouse",
            DeviceClass::HidKeyboard => "HID keyboard",
            DeviceClass::HidOther => "HID device",
            DeviceClass::MassStorage => "mass storage (not supported yet)",
            DeviceClass::Hub => "hub (not supported yet)",
            DeviceClass::Other => "device",
        }
    }
}

#[derive(Clone)]
pub struct UsbDevice {
    pub address: u8,
    pub port: u8,
    pub vendor: u16,
    pub product: u16,
    pub class: DeviceClass,
    pub interface: u8,
    pub endpoint: u8,
    pub max_packet: u16,
    pub low_speed: bool,
    pub controller: u64,
}

pub static DEVICES: Mutex<Vec<UsbDevice>> = Mutex::new(Vec::new());

fn mmio32(base: u64, offset: u64) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn set_mmio32(base: u64, offset: u64, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn spin_ms(ms: u32) {
    crate::arch::delay_ms(ms as u64);
}

fn ehci_take_over(device: &PciDevice, controller: &mut Controller) {
    let Some(base) = device.mmio_bar(0) else {
        controller.note = String::from("no MMIO BAR");
        return;
    };
    controller.base = base;
    if base == 0 || base >= 1 << 32 {
        controller.note = String::from("register window outside the identity map");
        return;
    }
    let hcc = mmio32(base, 0x08);
    let mut eecp = ((hcc >> 8) & 0xFF) as u8;
    let mut guard = 0;
    while eecp >= 0x40 && guard < 8 {
        let cap = pci::read_config_u32(device.address, eecp);
        if cap & 0xFF == 1 {
            if cap & (1 << 16) != 0 {
                pci::write_config_u32(device.address, eecp, cap | (1 << 24));
                for _ in 0..100 {
                    if pci::read_config_u32(device.address, eecp) & (1 << 16) == 0 {
                        break;
                    }
                    spin_ms(10);
                }
            }
            let ctl = pci::read_config_u32(device.address, eecp + 4);
            pci::write_config_u32(device.address, eecp + 4, (ctl & 0xE000_0000) | 0xE000_0000);
        }
        eecp = ((cap >> 8) & 0xFF) as u8;
        guard += 1;
    }
    let operational = base + (mmio32(base, 0) & 0xFF) as u64;
    let command = mmio32(operational, 0);
    set_mmio32(operational, 0, command & !1);
    for _ in 0..50 {
        if mmio32(operational, 4) & (1 << 12) != 0 {
            break;
        }
        spin_ms(1);
    }
    set_mmio32(operational, 8, 0);
    set_mmio32(operational, 0x40, 0);
    let ports = (mmio32(base, 0x04) & 0xF) as u8;
    for port in 0..ports as u64 {
        let register = 0x44 + port * 4;
        let value = mmio32(operational, register);
        set_mmio32(operational, register, (value & !(0x2A | 0x04)) | (1 << 13));
    }
    controller.ports = ports;
    controller.note = format!("taken over from firmware, {} port(s) routed to companion UHCI/OHCI controllers", ports);
}

pub fn init() {
    let devices: Vec<PciDevice> = pci::enumerate()
        .into_iter()
        .filter(|d| d.class == CLASS_SERIAL_BUS && d.subclass == SUBCLASS_USB)
        .collect();
    let mut controllers = Vec::new();
    let has_companions = devices.iter().any(|d| ControllerKind::from_prog_if(d.prog_if) == ControllerKind::Uhci);
    for device in devices.iter().filter(|d| ControllerKind::from_prog_if(d.prog_if) == ControllerKind::Ehci) {
        let mut controller = Controller { kind: ControllerKind::Ehci, device: *device, base: 0, ports: 0, driven: false, note: String::new() };
        let companions = devices.iter().any(|d| {
            ControllerKind::from_prog_if(d.prog_if) == ControllerKind::Uhci && d.address.bus == device.address.bus && d.address.device == device.address.device
        });
        if companions || (has_companions && device.vendor != 0x8086) {
            pci::enable_bus_master(device.address);
            ehci_take_over(device, &mut controller);
        } else {
            controller.base = device.mmio_bar(0).unwrap_or(0);
            controller.note = String::from("no UHCI companions (rate-matching hub), left to the firmware");
        }
        controllers.push(controller);
    }
    if !controllers.is_empty() {
        spin_ms(50);
    }
    for device in devices.iter() {
        let kind = ControllerKind::from_prog_if(device.prog_if);
        if kind == ControllerKind::Ehci {
            continue;
        }
        let mut controller = Controller { kind, device: *device, base: 0, ports: 0, driven: false, note: String::new() };
        match kind {
            ControllerKind::Uhci => {
                pci::enable_bus_master(device.address);
                uhci::attach(&mut controller);
            }
            ControllerKind::Xhci => {
                controller.base = device.mmio_bar(0).unwrap_or(0);
                controller.note = String::from("left to the firmware (USB legacy emulation keeps working)");
            }
            ControllerKind::Ohci => {
                controller.base = device.mmio_bar(0).unwrap_or(0);
                controller.note = String::from("left to the firmware");
            }
            ControllerKind::Unknown | ControllerKind::Ehci => controller.note = String::from("unrecognised interface"),
        }
        controllers.push(controller);
    }
    for controller in &controllers {
        crate::drivers::klog::log(&format!(
            "usb: {:04x}:{:04x} {} at {:#x} -- {}",
            controller.device.vendor,
            controller.device.device,
            controller.kind.name(),
            controller.base,
            controller.note
        ));
    }
    if controllers.is_empty() {
        crate::drivers::klog::log("usb: no host controllers found");
    }
    *CONTROLLERS.lock() = controllers;
}

extern "C" fn usb_daemon(_: u64) -> ! {
    loop {
        uhci::scan();
        crate::task::sleep_ticks(crate::task::TICK_HZ / 4);
    }
}

pub fn start_daemon() {
    if uhci::active() {
        crate::task::spawn_kernel_thread("usbd", 0, usb_daemon, 0);
    }
}

pub fn poll() {
    uhci::poll();
}

pub fn adopt_controller(bus: u8, device: u8, function: u8, kind: u32, ports: u32, owner: &str) {
    let mut list = CONTROLLERS.lock();
    if let Some(c) = list.iter_mut().find(|c| c.device.address.bus == bus && c.device.address.device == device && c.device.address.function == function) {
        c.driven = true;
        c.ports = ports as u8;
        c.note = format!("driven by module {}", owner);
        return;
    }
    let found = pci::devices().into_iter().find(|d| d.address.bus == bus && d.address.device == device && d.address.function == function);
    if let Some(dev) = found {
        let kind = match kind {
            0 => ControllerKind::Uhci,
            1 => ControllerKind::Ohci,
            2 => ControllerKind::Ehci,
            3 => ControllerKind::Xhci,
            _ => ControllerKind::Unknown,
        };
        list.push(Controller { kind, device: dev, base: dev.mmio_bar(0).unwrap_or(0), ports: ports as u8, driven: true, note: format!("driven by module {}", owner) });
    }
}

pub fn add_module_device(port: u8, vendor: u16, product: u16, interface: u32, speed: u32, owner: &str) -> usize {
    let class = match (interface >> 16, (interface >> 8) & 0xFF, interface & 0xFF) {
        (3, 1, 1) => DeviceClass::HidKeyboard,
        (3, 1, 2) => DeviceClass::HidMouse,
        (3, _, _) => DeviceClass::HidOther,
        (8, _, _) => DeviceClass::MassStorage,
        (9, _, _) => DeviceClass::Hub,
        _ => DeviceClass::Other,
    };
    let mut list = DEVICES.lock();
    list.push(UsbDevice { address: 0, port, vendor, product, class, interface: 0, endpoint: 0, max_packet: 0, low_speed: speed == 2, controller: 0 });
    drop(list);
    if class == DeviceClass::HidMouse {
        crate::drivers::input::mouse::mark_present();
    }
    crate::drivers::klog::log(&format!("usb: {:04x}:{:04x} {} on port {} ({})", vendor, product, class.name(), port, owner));
    DEVICES.lock().len() - 1
}

pub fn remove_module_device(index: usize) {
    let mut list = DEVICES.lock();
    if index < list.len() {
        list[index].class = DeviceClass::Other;
        list[index].vendor = 0;
        list[index].product = 0;
    }
}

pub fn controllers() -> Vec<Controller> {
    CONTROLLERS.lock().clone()
}

pub fn devices() -> Vec<UsbDevice> {
    DEVICES.lock().clone()
}

pub fn describe() -> String {
    let mut out = String::new();
    for c in controllers() {
        out.push_str(&format!(
            "controller\t{}\t{:04x}:{:04x}\t{:#x}\t{}\t{}\n",
            c.kind.name(),
            c.device.vendor,
            c.device.device,
            c.base,
            if c.driven { "driven" } else { "firmware" },
            c.note
        ));
    }
    for d in devices() {
        out.push_str(&format!(
            "device\t{:04x}:{:04x}\t{:#x}\t{}\t{}\t{}\n",
            d.vendor,
            d.product,
            d.controller,
            d.port,
            if d.low_speed { "low" } else { "full" },
            d.class.name()
        ));
    }
    out
}
