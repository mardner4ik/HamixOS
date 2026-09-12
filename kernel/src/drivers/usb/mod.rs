pub mod hid;
pub mod uhci;
pub mod xhci;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::drivers::pci::{self, PciDevice};

pub const CLASS_SERIAL_BUS: u8 = 0x0C;
pub const SUBCLASS_USB: u8 = 0x03;

pub const PROGIF_UHCI: u8 = 0x00;
pub const PROGIF_OHCI: u8 = 0x10;
pub const PROGIF_EHCI: u8 = 0x20;
pub const PROGIF_XHCI: u8 = 0x30;

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
            PROGIF_UHCI => Self::Uhci,
            PROGIF_OHCI => Self::Ohci,
            PROGIF_EHCI => Self::Ehci,
            PROGIF_XHCI => Self::Xhci,
            _ => Self::Unknown,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Uhci => "UHCI (USB 1.1)",
            Self::Ohci => "OHCI (USB 1.1)",
            Self::Ehci => "EHCI (USB 2.0)",
            Self::Xhci => "xHCI (USB 3.x)",
            Self::Unknown => "unknown USB host controller",
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
}

pub static DEVICES: Mutex<Vec<UsbDevice>> = Mutex::new(Vec::new());

pub fn init() {
    let mut controllers = Vec::new();

    for device in pci::enumerate() {
        if device.class != CLASS_SERIAL_BUS || device.subclass != SUBCLASS_USB {
            continue;
        }
        let kind = ControllerKind::from_prog_if(device.prog_if);
        pci::enable_bus_master(device.address);

        let mut controller = Controller {
            kind,
            device,
            base: 0,
            ports: 0,
            driven: false,
            note: String::new(),
        };

        match kind {
            ControllerKind::Uhci => uhci::attach(&mut controller),
            ControllerKind::Xhci => xhci::attach(&mut controller),
            ControllerKind::Ehci => {
                controller.base = device.mmio_bar(0).unwrap_or(0);
                controller.note = String::from("detected, EHCI transfers not implemented yet");
            }
            ControllerKind::Ohci => {
                controller.base = device.mmio_bar(0).unwrap_or(0);
                controller.note = String::from("detected, OHCI transfers not implemented yet");
            }
            ControllerKind::Unknown => {
                controller.note = String::from("unrecognised programming interface");
            }
        }

        crate::drivers::klog::log(&alloc::format!(
            "usb: {:04x}:{:04x} {} at {:#x} -- {}",
            device.vendor,
            device.device,
            kind.name(),
            controller.base,
            if controller.note.is_empty() { "ready" } else { &controller.note }
        ));

        controllers.push(controller);
    }

    if controllers.is_empty() {
        crate::drivers::klog::log("usb: no host controllers found on the PCI bus");
    }

    *CONTROLLERS.lock() = controllers;
}

pub fn poll() {
    uhci::poll();
}

pub fn controllers() -> Vec<Controller> {
    CONTROLLERS.lock().clone()
}

pub fn devices() -> Vec<UsbDevice> {
    DEVICES.lock().clone()
}
