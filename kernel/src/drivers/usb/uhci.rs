use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::io::{inb, inw, outl, outw};
use crate::drivers::usb::hid;
use crate::drivers::usb::{Controller, DeviceClass, UsbDevice, DEVICES};
use crate::memory::frame;

const REG_USBCMD: u16 = 0x00;
const REG_USBSTS: u16 = 0x02;
const REG_USBINTR: u16 = 0x04;
const REG_FRNUM: u16 = 0x06;
const REG_FRBASEADD: u16 = 0x08;
const REG_SOFMOD: u16 = 0x0C;
const REG_PORTSC: u16 = 0x10;

const CMD_RUN: u16 = 1 << 0;
const CMD_HCRESET: u16 = 1 << 1;
const CMD_GRESET: u16 = 1 << 2;
const CMD_CF: u16 = 1 << 6;
const CMD_MAXP: u16 = 1 << 7;

const STS_HALTED: u16 = 1 << 5;

const PORT_CONNECTED: u16 = 1 << 0;
const PORT_CONNECT_CHANGE: u16 = 1 << 1;
const PORT_ENABLED: u16 = 1 << 2;
const PORT_ENABLE_CHANGE: u16 = 1 << 3;
const PORT_LOW_SPEED: u16 = 1 << 8;
const PORT_RESET: u16 = 1 << 9;
const PORT_WRITE_MASK: u16 = PORT_ENABLED | PORT_RESET | (1 << 12);

const LINK_TERMINATE: u32 = 1;
const LINK_QUEUE_HEAD: u32 = 2;
const LINK_DEPTH_FIRST: u32 = 4;

const PID_SETUP: u32 = 0x2D;
const PID_IN: u32 = 0x69;
const PID_OUT: u32 = 0xE1;

const TD_ACTIVE: u32 = 1 << 23;
const TD_STALLED: u32 = 1 << 22;
const TD_BUFFER_ERROR: u32 = 1 << 21;
const TD_BABBLE: u32 = 1 << 20;
const TD_NAK: u32 = 1 << 19;
const TD_CRC_TIMEOUT: u32 = 1 << 18;
const TD_BITSTUFF: u32 = 1 << 17;
const TD_LOW_SPEED: u32 = 1 << 26;
const TD_ERROR_LIMIT: u32 = 3 << 27;
const TD_ERRORS: u32 = TD_STALLED | TD_BUFFER_ERROR | TD_BABBLE | TD_CRC_TIMEOUT | TD_BITSTUFF;

const REQUEST_GET_DESCRIPTOR: u8 = 6;
const REQUEST_SET_ADDRESS: u8 = 5;
const REQUEST_SET_CONFIGURATION: u8 = 9;
const REQUEST_SET_IDLE: u8 = 10;
const REQUEST_SET_PROTOCOL: u8 = 11;

const CONTROL_TDS: usize = 40;
const TD_SIZE: usize = 32;
const QH_SIZE: usize = 16;
const ENDPOINT_SLOT: usize = 64;
const MAX_ENDPOINTS: usize = 32;
const DATA_SIZE: usize = 1024;

struct Endpoint {
    address: u8,
    endpoint: u8,
    length: usize,
    toggle: u32,
    low_speed: bool,
    keyboard: bool,
    port: u8,
    slot: usize,
    previous: [u8; 8],
    armed: bool,
}

struct Uhci {
    io: u16,
    frame_list: usize,
    pool: usize,
    data: usize,
    endpoints_page: usize,
    ports: u8,
    connected: [bool; 8],
    next_address: u8,
    endpoints: Vec<Endpoint>,
}

static CONTROLLERS: Mutex<Vec<Uhci>> = Mutex::new(Vec::new());

fn delay_ms(ms: u64) {
    crate::arch::delay_ms(ms);
}

fn wr32(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

fn rd32(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

fn port_register(io: u16, port: u8) -> u16 {
    io + REG_PORTSC + port as u16 * 2
}

fn build_token(pid: u32, address: u8, endpoint: u8, toggle: u32, length: usize) -> u32 {
    let max_len = if length == 0 { 0x7FF } else { (length - 1) as u32 };
    pid | ((address as u32 & 0x7F) << 8) | ((endpoint as u32 & 0x0F) << 15) | (toggle << 19) | (max_len << 21)
}

impl Uhci {
    fn interrupt_qh(&self) -> usize {
        self.pool
    }

    fn control_qh(&self) -> usize {
        self.pool + QH_SIZE
    }

    fn td(&self, index: usize) -> usize {
        self.pool + 2 * QH_SIZE + index * TD_SIZE
    }

    fn setup_buffer(&self) -> usize {
        self.pool + 2 * QH_SIZE + CONTROL_TDS * TD_SIZE
    }

    fn endpoint_qh(&self, slot: usize) -> usize {
        self.endpoints_page + slot * ENDPOINT_SLOT
    }

    fn endpoint_td(&self, slot: usize) -> usize {
        self.endpoint_qh(slot) + QH_SIZE
    }

    fn endpoint_buffer(&self, slot: usize) -> usize {
        self.endpoint_qh(slot) + QH_SIZE + TD_SIZE
    }

    fn relink_schedule(&self) {
        let mut next = self.control_qh() as u32 | LINK_QUEUE_HEAD;
        for ep in self.endpoints.iter().rev() {
            let qh = self.endpoint_qh(ep.slot);
            wr32(qh, next);
            next = qh as u32 | LINK_QUEUE_HEAD;
        }
        wr32(self.interrupt_qh(), next);
    }

    fn reset_port(&self, port: u8) -> bool {
        let register = port_register(self.io, port);
        if inw(register) & PORT_CONNECTED == 0 {
            return false;
        }
        outw(register, PORT_RESET);
        delay_ms(60);
        outw(register, inw(register) & !PORT_RESET & PORT_WRITE_MASK);
        delay_ms(10);
        for _ in 0..16 {
            let status = inw(register);
            if status & PORT_CONNECTED == 0 {
                return false;
            }
            if status & (PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE) != 0 {
                outw(register, (status & PORT_WRITE_MASK) | PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE);
                continue;
            }
            if status & PORT_ENABLED != 0 {
                delay_ms(10);
                return true;
            }
            outw(register, (status & PORT_WRITE_MASK) | PORT_ENABLED);
            delay_ms(10);
        }
        false
    }

    fn control(&mut self, address: u8, low_speed: bool, max_packet: u16, setup: [u8; 8]) -> Option<Vec<u8>> {
        let length = u16::from_le_bytes([setup[6], setup[7]]) as usize;
        if length > DATA_SIZE {
            return None;
        }
        let device_to_host = setup[0] & 0x80 != 0;
        let packet = max_packet.max(8) as usize;
        let speed = if low_speed { TD_LOW_SPEED } else { 0 };
        unsafe {
            core::ptr::copy_nonoverlapping(setup.as_ptr(), self.setup_buffer() as *mut u8, 8);
            core::ptr::write_bytes(self.data as *mut u8, 0, DATA_SIZE);
        }

        let mut count = 0usize;
        let fill = |this: &Self, index: usize, pid: u32, toggle: u32, len: usize, buffer: usize| {
            let td = this.td(index);
            wr32(td + 4, TD_ACTIVE | TD_ERROR_LIMIT | speed);
            wr32(td + 8, build_token(pid, address, 0, toggle, len));
            wr32(td + 12, buffer as u32);
        };
        fill(self, count, PID_SETUP, 0, 8, self.setup_buffer());
        count += 1;
        let mut toggle = 1;
        let mut offset = 0;
        while offset < length && count < CONTROL_TDS - 1 {
            let chunk = (length - offset).min(packet);
            fill(self, count, if device_to_host { PID_IN } else { PID_OUT }, toggle, chunk, self.data + offset);
            toggle ^= 1;
            offset += chunk;
            count += 1;
        }
        fill(self, count, if device_to_host && length > 0 { PID_OUT } else { PID_IN }, 1, 0, 0);
        count += 1;
        for index in 0..count {
            let link = if index + 1 == count { LINK_TERMINATE } else { self.td(index + 1) as u32 | LINK_DEPTH_FIRST };
            wr32(self.td(index), link);
        }

        wr32(self.control_qh() + 4, self.td(0) as u32);
        let mut waited = 0u32;
        let mut ok = false;
        loop {
            let element = rd32(self.control_qh() + 4);
            if element & LINK_TERMINATE != 0 {
                ok = true;
                break;
            }
            if (0..count).any(|i| rd32(self.td(i) + 4) & TD_ERRORS != 0 && rd32(self.td(i) + 4) & TD_ACTIVE == 0) {
                break;
            }
            if inw(self.io + REG_USBSTS) & STS_HALTED != 0 {
                break;
            }
            waited += 1;
            if waited > 1000 {
                break;
            }
            delay_ms(1);
        }
        if !ok {
            wr32(self.control_qh() + 4, LINK_TERMINATE);
            delay_ms(2);
            return None;
        }

        let mut received = 0usize;
        for index in 1..count - 1 {
            let actual = (rd32(self.td(index) + 4) & 0x7FF) as usize;
            let got = if actual == 0x7FF { 0 } else { actual + 1 };
            received += got;
            if device_to_host && got < (length - (index - 1) * packet).min(packet) {
                break;
            }
        }
        let mut out = alloc::vec![0u8; received.min(length)];
        unsafe { core::ptr::copy_nonoverlapping(self.data as *const u8, out.as_mut_ptr(), out.len()) };
        Some(out)
    }

    fn get_descriptor(&mut self, address: u8, low_speed: bool, max_packet: u16, kind: u8, length: u16) -> Option<Vec<u8>> {
        let setup = [0x80, REQUEST_GET_DESCRIPTOR, 0, kind, 0, 0, length as u8, (length >> 8) as u8];
        self.control(address, low_speed, max_packet, setup)
    }

    fn enumerate_port(&mut self, port: u8) -> Option<UsbDevice> {
        if !self.reset_port(port) {
            return None;
        }
        let low_speed = inw(port_register(self.io, port)) & PORT_LOW_SPEED != 0;
        let head = (0..3).find_map(|_| {
            let d = self.get_descriptor(0, low_speed, 8, 1, 8);
            if d.as_ref().map(|d| d.len() >= 8).unwrap_or(false) { d } else { delay_ms(20); None }
        })?;
        let max_packet = head[7].max(8) as u16;
        if !self.reset_port(port) {
            return None;
        }

        let address = self.next_address;
        self.next_address = if self.next_address >= 127 { 1 } else { self.next_address + 1 };
        self.control(0, low_speed, max_packet, [0x00, REQUEST_SET_ADDRESS, address, 0, 0, 0, 0, 0])?;
        delay_ms(10);

        let device = self.get_descriptor(address, low_speed, max_packet, 1, 18)?;
        if device.len() < 18 {
            return None;
        }
        let vendor = u16::from_le_bytes([device[8], device[9]]);
        let product = u16::from_le_bytes([device[10], device[11]]);
        let device_class = device[4];

        let header = self.get_descriptor(address, low_speed, max_packet, 2, 9)?;
        if header.len() < 9 {
            return None;
        }
        let total = u16::from_le_bytes([header[2], header[3]]).min(DATA_SIZE as u16);
        let configuration_value = header[5];
        let configuration = self.get_descriptor(address, low_speed, max_packet, 2, total)?;
        let interfaces = parse_configuration(&configuration);

        let mut usb = UsbDevice {
            address,
            port,
            vendor,
            product,
            class: match device_class {
                9 => DeviceClass::Hub,
                8 => DeviceClass::MassStorage,
                _ => DeviceClass::Other,
            },
            interface: 0,
            endpoint: 0,
            max_packet,
            low_speed,
            controller: self.io as u64,
        };
        if interfaces.iter().any(|i| i.class == 8) {
            usb.class = DeviceClass::MassStorage;
        }

        let hid: Vec<&InterfaceInfo> = interfaces.iter().filter(|i| i.class == 3 && i.endpoint != 0).collect();
        if hid.is_empty() {
            if usb.class == DeviceClass::Other && interfaces.iter().any(|i| i.class == 3) {
                usb.class = DeviceClass::HidOther;
            }
            return Some(usb);
        }
        self.control(address, low_speed, max_packet, [0x00, REQUEST_SET_CONFIGURATION, configuration_value, 0, 0, 0, 0, 0])?;
        delay_ms(5);

        for interface in hid {
            let protocol = if interface.subclass == 1 { interface.protocol } else { 0 };
            let number = interface.number;
            if interface.subclass == 1 {
                self.control(address, low_speed, max_packet, [0x21, REQUEST_SET_PROTOCOL, 0, 0, number, 0, 0, 0]);
            }
            self.control(address, low_speed, max_packet, [0x21, REQUEST_SET_IDLE, 0, 0, number, 0, 0, 0]);
            let class = match protocol {
                1 => DeviceClass::HidKeyboard,
                2 => DeviceClass::HidMouse,
                _ => DeviceClass::HidOther,
            };
            if usb.class != DeviceClass::HidMouse && usb.class != DeviceClass::HidKeyboard {
                usb.class = class;
                usb.interface = number;
                usb.endpoint = interface.endpoint;
            }
            if matches!(class, DeviceClass::HidMouse | DeviceClass::HidKeyboard) {
                self.add_endpoint(address, interface.endpoint, interface.packet_size as usize, low_speed, class == DeviceClass::HidKeyboard, port);
            }
        }
        Some(usb)
    }

    fn add_endpoint(&mut self, address: u8, endpoint: u8, length: usize, low_speed: bool, keyboard: bool, port: u8) {
        let used: Vec<usize> = self.endpoints.iter().map(|e| e.slot).collect();
        let Some(slot) = (0..MAX_ENDPOINTS).find(|s| !used.contains(s)) else {
            return;
        };
        let length = if keyboard { length.clamp(8, 8) } else { length.clamp(3, 8) };
        let ep = Endpoint { address, endpoint, length, toggle: 0, low_speed, keyboard, port, slot, previous: [0; 8], armed: false };
        wr32(self.endpoint_qh(slot) + 4, LINK_TERMINATE);
        self.endpoints.push(ep);
        let index = self.endpoints.len() - 1;
        self.arm(index);
        self.relink_schedule();
    }

    fn arm(&mut self, index: usize) {
        let ep = &self.endpoints[index];
        let td = self.endpoint_td(ep.slot);
        let buffer = self.endpoint_buffer(ep.slot);
        unsafe { core::ptr::write_bytes(buffer as *mut u8, 0, 8) };
        wr32(td, LINK_TERMINATE);
        wr32(td + 4, TD_ACTIVE | TD_ERROR_LIMIT | if ep.low_speed { TD_LOW_SPEED } else { 0 });
        wr32(td + 8, build_token(PID_IN, ep.address, ep.endpoint, ep.toggle, ep.length));
        wr32(td + 12, buffer as u32);
        wr32(self.endpoint_qh(ep.slot) + 4, td as u32);
        self.endpoints[index].armed = true;
    }

    fn service(&mut self) {
        for index in 0..self.endpoints.len() {
            if !self.endpoints[index].armed {
                continue;
            }
            let slot = self.endpoints[index].slot;
            let status = rd32(self.endpoint_td(slot) + 4);
            if status & TD_ACTIVE != 0 {
                continue;
            }
            if status & TD_ERRORS == 0 {
                let actual = (status & 0x7FF) as usize;
                let received = if actual == 0x7FF { 0 } else { actual + 1 };
                let mut report = [0u8; 8];
                let n = received.min(self.endpoints[index].length).min(8);
                unsafe { core::ptr::copy_nonoverlapping(self.endpoint_buffer(slot) as *const u8, report.as_mut_ptr(), n) };
                self.endpoints[index].toggle ^= 1;
                if n > 0 {
                    if self.endpoints[index].keyboard {
                        let mut previous = self.endpoints[index].previous;
                        hid::on_boot_keyboard_report(&report[..n], &mut previous);
                        self.endpoints[index].previous = previous;
                    } else {
                        hid::on_boot_mouse_report(&report[..n]);
                    }
                }
            } else if status & TD_STALLED != 0 {
                self.endpoints[index].toggle = 0;
            }
            self.arm(index);
        }
    }

    fn check_ports(&mut self) -> Vec<(u8, bool)> {
        let mut changes = Vec::new();
        for port in 0..self.ports {
            let register = port_register(self.io, port);
            let status = inw(register);
            let connected = status & PORT_CONNECTED != 0;
            if status & PORT_CONNECT_CHANGE != 0 || connected != self.connected[port as usize] {
                outw(register, (status & PORT_WRITE_MASK) | PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE);
                changes.push((port, connected));
            }
        }
        changes
    }
}

struct InterfaceInfo {
    number: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    endpoint: u8,
    packet_size: u16,
}

fn parse_configuration(data: &[u8]) -> Vec<InterfaceInfo> {
    let mut out: Vec<InterfaceInfo> = Vec::new();
    let mut offset = 0usize;
    while offset + 2 <= data.len() {
        let length = data[offset] as usize;
        let kind = data[offset + 1];
        if length < 2 || offset + length > data.len() {
            break;
        }
        if kind == 0x04 && length >= 9 {
            out.push(InterfaceInfo {
                number: data[offset + 2],
                class: data[offset + 5],
                subclass: data[offset + 6],
                protocol: data[offset + 7],
                endpoint: 0,
                packet_size: 0,
            });
        }
        if kind == 0x05 && length >= 7 {
            if let Some(current) = out.last_mut() {
                let address = data[offset + 2];
                let attributes = data[offset + 3];
                if current.endpoint == 0 && address & 0x80 != 0 && attributes & 0x03 == 0x03 {
                    current.endpoint = address & 0x0F;
                    current.packet_size = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
                }
            }
        }
        offset += length;
    }
    out
}

fn detect_port_count(io: u16) -> u8 {
    let mut count = 0u8;
    while count < 8 {
        let value = inw(port_register(io, count));
        if value == 0xFFFF || value & 0x0080 == 0 {
            break;
        }
        count += 1;
    }
    if count == 0 { 2 } else { count }
}

pub fn attach(controller: &mut Controller) {
    let Some(io) = controller.device.io_bar(4) else {
        controller.note = String::from("no I/O BAR -- cannot drive this controller");
        return;
    };
    controller.base = io as u64;
    let (Some(frame_list), Some(pool), Some(data), Some(endpoints_page)) =
        (frame::alloc_zeroed_frame(), frame::alloc_zeroed_frame(), frame::alloc_zeroed_frame(), frame::alloc_zeroed_frame())
    else {
        controller.note = String::from("out of memory");
        return;
    };

    crate::drivers::pci::write_config_u16(controller.device.address, 0xC0, 0x8F00);
    outw(io + REG_USBINTR, 0);
    outw(io + REG_USBCMD, 0);
    for _ in 0..20 {
        if inw(io + REG_USBSTS) & STS_HALTED != 0 {
            break;
        }
        delay_ms(1);
    }
    outw(io + REG_USBCMD, CMD_GRESET);
    delay_ms(20);
    outw(io + REG_USBCMD, 0);
    delay_ms(5);
    outw(io + REG_USBCMD, CMD_HCRESET);
    for _ in 0..50 {
        if inw(io + REG_USBCMD) & CMD_HCRESET == 0 {
            break;
        }
        delay_ms(1);
    }
    crate::drivers::pci::write_config_u16(controller.device.address, 0xC0, 0x2000);

    let uhci = Uhci {
        io,
        frame_list,
        pool,
        data,
        endpoints_page,
        ports: detect_port_count(io),
        connected: [false; 8],
        next_address: 1,
        endpoints: Vec::new(),
    };
    wr32(uhci.control_qh(), LINK_TERMINATE);
    wr32(uhci.control_qh() + 4, LINK_TERMINATE);
    wr32(uhci.interrupt_qh() + 4, LINK_TERMINATE);
    uhci.relink_schedule();
    for i in 0..1024 {
        wr32(frame_list + i * 4, uhci.interrupt_qh() as u32 | LINK_QUEUE_HEAD);
    }

    outw(io + REG_USBINTR, 0);
    outw(io + REG_FRNUM, 0);
    crate::arch::io::outb(io + REG_SOFMOD, 0x40);
    outl(io + REG_FRBASEADD, frame_list as u32);
    outw(io + REG_USBSTS, 0xFFFF);
    outw(io + REG_USBCMD, CMD_RUN | CMD_CF | CMD_MAXP);
    delay_ms(10);

    for port in 0..uhci.ports {
        let register = port_register(io, port);
        let status = inw(register);
        outw(register, (status & PORT_WRITE_MASK) | PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE);
    }
    let _ = REG_SOFMOD;

    controller.ports = uhci.ports;
    controller.driven = true;
    controller.note = format!("{} port(s)", uhci.ports);
    crate::arch::without_interrupts(|| CONTROLLERS.lock().push(uhci));
}

fn log_device(device: &UsbDevice) {
    crate::drivers::klog::log(&format!(
        "usb: {:04x}:{:04x} on UHCI {:#x} port {} -- {}{}",
        device.vendor,
        device.product,
        device.controller,
        device.port,
        device.class.name(),
        if device.low_speed { " (low speed)" } else { "" }
    ));
}

pub fn scan() -> bool {
    let count = crate::arch::without_interrupts(|| CONTROLLERS.lock().len());
    let mut changed = false;
    for index in 0..count {
        let changes = match crate::arch::without_interrupts(|| CONTROLLERS.lock().get_mut(index).map(|c| c.check_ports())) {
            Some(c) => c,
            None => continue,
        };
        for (port, connected) in changes {
            changed = true;
            let io = crate::arch::without_interrupts(|| {
                let mut guard = CONTROLLERS.lock();
                let c = &mut guard[index];
                c.connected[port as usize] = connected;
                c.endpoints.retain(|e| e.port != port);
                c.relink_schedule();
                c.io
            });
            DEVICES.lock().retain(|d| !(d.controller == io as u64 && d.port == port));
            if !connected {
                crate::drivers::klog::log(&format!("usb: device removed from UHCI {:#x} port {}", io, port));
                continue;
            }
            delay_ms(100);
            let mut guard = loop {
                if let Some(g) = CONTROLLERS.try_lock() {
                    break g;
                }
                delay_ms(1);
            };
            let result = guard[index].enumerate_port(port);
            drop(guard);
            match result {
                Some(device) => {
                    log_device(&device);
                    DEVICES.lock().push(device);
                }
                None => crate::drivers::klog::log(&format!("usb: UHCI {:#x} port {}: device did not enumerate", io, port)),
            }
        }
    }
    changed
}

pub fn poll() {
    if let Some(mut controllers) = CONTROLLERS.try_lock() {
        for c in controllers.iter_mut() {
            c.service();
        }
    }
}

pub fn active() -> bool {
    !CONTROLLERS.lock().is_empty()
}
