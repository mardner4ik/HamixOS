use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::x86_64::{inb, inw, outw};
use crate::drivers::usb::hid;
use crate::drivers::usb::{Controller, DeviceClass, UsbDevice, DEVICES};

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
const CMD_MAXP: u16 = 1 << 7;

const PORT_CONNECTED: u16 = 1 << 0;
const PORT_CONNECT_CHANGE: u16 = 1 << 1;
const PORT_ENABLED: u16 = 1 << 2;
const PORT_ENABLE_CHANGE: u16 = 1 << 3;
const PORT_LOW_SPEED: u16 = 1 << 8;
const PORT_RESET: u16 = 1 << 9;

const LINK_TERMINATE: u32 = 1;
const LINK_QUEUE_HEAD: u32 = 2;

const PID_SETUP: u32 = 0x2D;
const PID_IN: u32 = 0x69;
const PID_OUT: u32 = 0xE1;

const TD_ACTIVE: u32 = 1 << 23;
const TD_LOW_SPEED: u32 = 1 << 26;
const TD_ERROR_LIMIT: u32 = 3 << 27;
const TD_STALLED: u32 = 1 << 22;
const TD_NAK: u32 = 1 << 19;
const TD_CRC_TIMEOUT: u32 = 1 << 18;

const REQUEST_GET_DESCRIPTOR: u8 = 6;
const REQUEST_SET_ADDRESS: u8 = 5;
const REQUEST_SET_CONFIGURATION: u8 = 9;
const REQUEST_SET_PROTOCOL: u8 = 11;

const DESCRIPTOR_DEVICE: u8 = 1;
const DESCRIPTOR_CONFIGURATION: u8 = 2;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TransferDescriptor {
    link: u32,
    control: u32,
    token: u32,
    buffer: u32,
    reserved: [u32; 4],
}

impl TransferDescriptor {
    const fn new() -> Self {
        Self { link: LINK_TERMINATE, control: 0, token: 0, buffer: 0, reserved: [0; 4] }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct QueueHead {
    head: u32,
    element: u32,
    reserved: [u32; 2],
}

impl QueueHead {
    const fn new() -> Self {
        Self { head: LINK_TERMINATE, element: LINK_TERMINATE, reserved: [0; 2] }
    }
}

#[repr(C, align(4096))]
struct FrameList([u32; 1024]);

const TD_POOL_SIZE: usize = 32;
const BUFFER_SIZE: usize = 512;

static mut FRAME_LIST: FrameList = FrameList([LINK_TERMINATE; 1024]);
static mut TD_POOL: [TransferDescriptor; TD_POOL_SIZE] =
    [TransferDescriptor::new(); TD_POOL_SIZE];
static mut CONTROL_QUEUE: QueueHead = QueueHead::new();
static mut INTERRUPT_QUEUE: QueueHead = QueueHead::new();
static mut INTERRUPT_TD: TransferDescriptor = TransferDescriptor::new();

#[repr(C, align(16))]
struct DmaBuffer([u8; BUFFER_SIZE]);

static mut SETUP_BUFFER: DmaBuffer = DmaBuffer([0; BUFFER_SIZE]);
static mut DATA_BUFFER: DmaBuffer = DmaBuffer([0; BUFFER_SIZE]);
static mut REPORT_BUFFER: DmaBuffer = DmaBuffer([0; BUFFER_SIZE]);

struct InterruptEndpoint {
    io_base: u16,
    address: u8,
    endpoint: u8,
    length: usize,
    toggle: u32,
    low_speed: bool,
    armed: bool,
}

static ACTIVE_IO_BASE: Mutex<Option<u16>> = Mutex::new(None);
static INTERRUPT_ENDPOINT: Mutex<Option<InterruptEndpoint>> = Mutex::new(None);
static NEXT_ADDRESS: Mutex<u8> = Mutex::new(1);

fn physical(pointer: *const u8) -> u32 {
    pointer as usize as u32
}

fn delay_us(micros: u32) {
    for _ in 0..micros {
        inb(0x80);
    }
}

fn delay_ms(millis: u32) {
    delay_us(millis * 1000);
}

fn port_register(io_base: u16, port: u8) -> u16 {
    io_base + REG_PORTSC + (port as u16) * 2
}

pub fn attach(controller: &mut Controller) {
    let Some(io_base) = controller.device.io_bar(4) else {
        controller.note = String::from("no I/O BAR -- cannot drive this controller");
        return;
    };
    controller.base = io_base as u64;

    crate::drivers::pci::write_config_u16(controller.device.address, 0xC0, 0x8F00);

    outw(io_base + REG_USBCMD, CMD_GRESET);
    delay_ms(15);
    outw(io_base + REG_USBCMD, 0);
    delay_ms(15);

    outw(io_base + REG_USBCMD, CMD_HCRESET);
    for _ in 0..100 {
        if inw(io_base + REG_USBCMD) & CMD_HCRESET == 0 {
            break;
        }
        delay_ms(1);
    }

    outw(io_base + REG_USBINTR, 0);
    outw(io_base + REG_FRNUM, 0);
    outw(io_base + REG_SOFMOD, 0x40);
    outw(io_base + REG_USBSTS, 0xFFFF);

    let frame_list_address = unsafe { physical((&raw const FRAME_LIST).cast()) };
    let interrupt_queue_address = unsafe { physical((&raw const INTERRUPT_QUEUE).cast()) };
    let control_queue_address = unsafe { physical((&raw const CONTROL_QUEUE).cast()) };

    unsafe {
        core::ptr::write_volatile(&raw mut INTERRUPT_QUEUE.head, control_queue_address | LINK_QUEUE_HEAD);
        core::ptr::write_volatile(&raw mut INTERRUPT_QUEUE.element, LINK_TERMINATE);
        core::ptr::write_volatile(&raw mut CONTROL_QUEUE.head, LINK_TERMINATE);
        core::ptr::write_volatile(&raw mut CONTROL_QUEUE.element, LINK_TERMINATE);
        for slot in (&raw mut FRAME_LIST).as_mut().unwrap().0.iter_mut() {
            *slot = interrupt_queue_address | LINK_QUEUE_HEAD;
        }
    }

    crate::arch::x86_64::outl(io_base + REG_FRBASEADD, frame_list_address);
    outw(io_base + REG_USBCMD, CMD_RUN | CMD_MAXP);

    controller.ports = detect_port_count(io_base);
    controller.driven = true;
    *ACTIVE_IO_BASE.lock() = Some(io_base);

    let mut found = 0usize;
    for port in 0..controller.ports {
        if enumerate_port(io_base, port) {
            found += 1;
        }
    }

    controller.note = alloc::format!("{} port(s), {} device(s) enumerated", controller.ports, found);
}

fn detect_port_count(io_base: u16) -> u8 {
    let mut count = 0u8;
    while count < 8 {
        let value = inw(port_register(io_base, count));
        if value == 0xFFFF || value & 0x0080 == 0 {
            break;
        }
        count += 1;
    }
    if count == 0 { 2 } else { count }
}

fn reset_port(io_base: u16, port: u8) -> bool {
    let register = port_register(io_base, port);

    let status = inw(register);
    if status & PORT_CONNECTED == 0 {
        return false;
    }

    outw(register, PORT_RESET);
    delay_ms(50);
    outw(register, inw(register) & !PORT_RESET);
    delay_us(300);

    for _ in 0..10 {
        let status = inw(register);
        if status & PORT_CONNECTED == 0 {
            return false;
        }
        if status & (PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE) != 0 {
            outw(register, status & !(PORT_RESET) | PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE);
            continue;
        }
        if status & PORT_ENABLED != 0 {
            return true;
        }
        outw(register, status | PORT_ENABLED);
        delay_ms(10);
    }
    false
}

fn build_token(pid: u32, address: u8, endpoint: u8, toggle: u32, length: usize) -> u32 {
    let max_len = if length == 0 { 0x7FF } else { (length - 1) as u32 };
    pid | ((address as u32 & 0x7F) << 8)
        | ((endpoint as u32 & 0x0F) << 15)
        | (toggle << 19)
        | (max_len << 21)
}

fn td_control(index: usize) -> u32 {
    unsafe { core::ptr::read_volatile(&raw const TD_POOL[index].control) }
}

fn queue_element(queue: *const QueueHead) -> u32 {
    unsafe { core::ptr::read_volatile(&raw const (*queue).element) }
}

fn set_queue_element(queue: *mut QueueHead, value: u32) {
    unsafe { core::ptr::write_volatile(&raw mut (*queue).element, value) }
}

fn run_chain(io_base: u16, count: usize) -> bool {
    let first = unsafe { physical((&raw const TD_POOL).cast()) };
    set_queue_element(&raw mut CONTROL_QUEUE, first);

    let mut spins = 0u32;
    loop {
        if queue_element(&raw const CONTROL_QUEUE) & LINK_TERMINATE != 0 {
            break;
        }

        let mut stalled = false;
        for index in 0..count {
            if td_control(index) & (TD_STALLED | TD_CRC_TIMEOUT) != 0 {
                stalled = true;
            }
        }
        if stalled {
            set_queue_element(&raw mut CONTROL_QUEUE, LINK_TERMINATE);
            return false;
        }

        delay_us(20);
        spins += 1;
        if spins > 25_000 {
            set_queue_element(&raw mut CONTROL_QUEUE, LINK_TERMINATE);
            return false;
        }
    }

    let _ = io_base;
    true
}

fn control_transfer(
    io_base: u16,
    address: u8,
    low_speed: bool,
    max_packet: u16,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
) -> Option<usize> {
    let setup = [
        request_type,
        request,
        value as u8,
        (value >> 8) as u8,
        index as u8,
        (index >> 8) as u8,
        length as u8,
        (length >> 8) as u8,
    ];
    unsafe {
        let buffer = (&raw mut SETUP_BUFFER).cast::<u8>();
        core::ptr::copy_nonoverlapping(setup.as_ptr(), buffer, 8);
        core::ptr::write_bytes((&raw mut DATA_BUFFER).cast::<u8>(), 0, BUFFER_SIZE);
    }

    let device_to_host = request_type & 0x80 != 0;
    let packet = max_packet.max(8) as usize;
    let low_speed_flag = if low_speed { TD_LOW_SPEED } else { 0 };

    let mut count = 0usize;
    let setup_address = unsafe { physical((&raw const SETUP_BUFFER).cast()) };
    let data_address = unsafe { physical((&raw const DATA_BUFFER).cast()) };

    unsafe {
        core::ptr::write_volatile(&raw mut TD_POOL[count].control, TD_ACTIVE | TD_ERROR_LIMIT | low_speed_flag);
        core::ptr::write_volatile(&raw mut TD_POOL[count].token, build_token(PID_SETUP, address, 0, 0, 8));
        core::ptr::write_volatile(&raw mut TD_POOL[count].buffer, setup_address);
        count += 1;
    }

    let mut toggle = 1u32;
    let mut remaining = length as usize;
    let mut offset = 0usize;
    while remaining > 0 && count < TD_POOL_SIZE - 1 {
        let chunk = remaining.min(packet);
        unsafe {
            core::ptr::write_volatile(&raw mut TD_POOL[count].control, TD_ACTIVE | TD_ERROR_LIMIT | low_speed_flag);
            core::ptr::write_volatile(
                &raw mut TD_POOL[count].token,
                build_token(
                    if device_to_host { PID_IN } else { PID_OUT },
                    address,
                    0,
                    toggle,
                    chunk,
                ),
            );
            core::ptr::write_volatile(&raw mut TD_POOL[count].buffer, data_address + offset as u32);
        }
        count += 1;
        toggle ^= 1;
        offset += chunk;
        remaining -= chunk;
    }

    unsafe {
        core::ptr::write_volatile(&raw mut TD_POOL[count].control, TD_ACTIVE | TD_ERROR_LIMIT | low_speed_flag);
        core::ptr::write_volatile(
            &raw mut TD_POOL[count].token,
            build_token(if device_to_host { PID_OUT } else { PID_IN }, address, 0, 1, 0),
        );
        core::ptr::write_volatile(&raw mut TD_POOL[count].buffer, 0);
        count += 1;
    }

    unsafe {
        for index in 0..count {
            let link = if index + 1 == count {
                LINK_TERMINATE
            } else {
                physical((&raw const TD_POOL[index + 1]).cast()) | 4
            };
            core::ptr::write_volatile(&raw mut TD_POOL[index].link, link);
        }
    }

    if !run_chain(io_base, count) {
        return None;
    }

    let mut transferred = 0usize;
    for index in 1..count - 1 {
        let control = td_control(index);
        let actual = (control & 0x7FF) as usize;
        transferred += if actual == 0x7FF { 0 } else { actual + 1 };
    }
    Some(transferred)
}

fn data_buffer(length: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(length);
    unsafe {
        let buffer = (&raw const DATA_BUFFER).cast::<u8>();
        for index in 0..length.min(BUFFER_SIZE) {
            out.push(core::ptr::read_volatile(buffer.add(index)));
        }
    }
    out
}

fn enumerate_port(io_base: u16, port: u8) -> bool {
    if !reset_port(io_base, port) {
        return false;
    }

    let low_speed = inw(port_register(io_base, port)) & PORT_LOW_SPEED != 0;

    let descriptor = match control_transfer(
        io_base, 0, low_speed, 8, 0x80, REQUEST_GET_DESCRIPTOR,
        (DESCRIPTOR_DEVICE as u16) << 8, 0, 8,
    ) {
        Some(_) => data_buffer(8),
        None => return false,
    };
    if descriptor.len() < 8 {
        return false;
    }
    let max_packet = descriptor[7].max(8) as u16;

    let address = {
        let mut next = NEXT_ADDRESS.lock();
        let value = *next;
        *next += 1;
        value
    };

    if control_transfer(
        io_base, 0, low_speed, max_packet, 0x00, REQUEST_SET_ADDRESS,
        address as u16, 0, 0,
    )
    .is_none()
    {
        return false;
    }
    delay_ms(5);

    let device = match control_transfer(
        io_base, address, low_speed, max_packet, 0x80, REQUEST_GET_DESCRIPTOR,
        (DESCRIPTOR_DEVICE as u16) << 8, 0, 18,
    ) {
        Some(_) => data_buffer(18),
        None => return false,
    };
    if device.len() < 18 {
        return false;
    }
    let vendor = u16::from_le_bytes([device[8], device[9]]);
    let product = u16::from_le_bytes([device[10], device[11]]);

    let header = match control_transfer(
        io_base, address, low_speed, max_packet, 0x80, REQUEST_GET_DESCRIPTOR,
        (DESCRIPTOR_CONFIGURATION as u16) << 8, 0, 9,
    ) {
        Some(_) => data_buffer(9),
        None => return false,
    };
    if header.len() < 9 {
        return false;
    }
    let total = u16::from_le_bytes([header[2], header[3]]).min(BUFFER_SIZE as u16);
    let configuration_value = header[5];

    let configuration = match control_transfer(
        io_base, address, low_speed, max_packet, 0x80, REQUEST_GET_DESCRIPTOR,
        (DESCRIPTOR_CONFIGURATION as u16) << 8, 0, total,
    ) {
        Some(_) => data_buffer(total as usize),
        None => return false,
    };

    let Some(found) = parse_configuration(&configuration) else {
        crate::drivers::klog::log(&alloc::format!(
            "usb: port {} device {:04x}:{:04x} has no boot-protocol HID interface",
            port, vendor, product
        ));
        return true;
    };

    if control_transfer(
        io_base, address, low_speed, max_packet, 0x00, REQUEST_SET_CONFIGURATION,
        configuration_value as u16, 0, 0,
    )
    .is_none()
    {
        return false;
    }

    control_transfer(
        io_base, address, low_speed, max_packet, 0x21, REQUEST_SET_PROTOCOL,
        0, found.interface as u16, 0,
    );

    let class = match found.protocol {
        1 => DeviceClass::HidKeyboard,
        2 => DeviceClass::HidMouse,
        _ => DeviceClass::HidOther,
    };

    DEVICES.lock().push(UsbDevice {
        address,
        port,
        vendor,
        product,
        class,
        interface: found.interface,
        endpoint: found.endpoint,
        max_packet: found.packet_size,
        low_speed,
    });

    crate::drivers::klog::log(&alloc::format!(
        "usb: port {} {:04x}:{:04x} {} on endpoint {}",
        port,
        vendor,
        product,
        match class {
            DeviceClass::HidMouse => "HID boot mouse",
            DeviceClass::HidKeyboard => "HID boot keyboard",
            _ => "HID device",
        },
        found.endpoint
    ));

    if class == DeviceClass::HidMouse {
        arm_interrupt_endpoint(io_base, address, found.endpoint, found.packet_size as usize, low_speed);
    }

    true
}

struct HidInterface {
    interface: u8,
    protocol: u8,
    endpoint: u8,
    packet_size: u16,
}

fn parse_configuration(data: &[u8]) -> Option<HidInterface> {
    let mut offset = 0usize;
    let mut current: Option<(u8, u8)> = None;

    while offset + 2 <= data.len() {
        let length = data[offset] as usize;
        let kind = data[offset + 1];
        if length < 2 || offset + length > data.len() {
            break;
        }

        if kind == 0x04 && length >= 9 {
            let number = data[offset + 2];
            let class = data[offset + 5];
            let subclass = data[offset + 6];
            let protocol = data[offset + 7];
            current = if class == 0x03 && subclass == 0x01 {
                Some((number, protocol))
            } else {
                None
            };
        }

        if kind == 0x05 && length >= 7 {
            if let Some((number, protocol)) = current {
                let address = data[offset + 2];
                let attributes = data[offset + 3];
                if address & 0x80 != 0 && attributes & 0x03 == 0x03 {
                    return Some(HidInterface {
                        interface: number,
                        protocol,
                        endpoint: address & 0x0F,
                        packet_size: u16::from_le_bytes([data[offset + 4], data[offset + 5]]),
                    });
                }
            }
        }

        offset += length;
    }
    None
}

fn arm_interrupt_endpoint(
    io_base: u16,
    address: u8,
    endpoint: u8,
    length: usize,
    low_speed: bool,
) {
    *INTERRUPT_ENDPOINT.lock() = Some(InterruptEndpoint {
        io_base,
        address,
        endpoint,
        length: length.clamp(1, 8),
        toggle: 0,
        low_speed,
        armed: false,
    });
    rearm();
}

fn rearm() {
    let mut guard = INTERRUPT_ENDPOINT.lock();
    let Some(endpoint) = guard.as_mut() else {
        return;
    };

    let buffer = unsafe { physical((&raw const REPORT_BUFFER).cast()) };
    unsafe {
        core::ptr::write_bytes((&raw mut REPORT_BUFFER).cast::<u8>(), 0, 8);
        core::ptr::write_volatile(&raw mut INTERRUPT_TD.link, LINK_TERMINATE);
        core::ptr::write_volatile(
            &raw mut INTERRUPT_TD.control,
            TD_ACTIVE | TD_ERROR_LIMIT | if endpoint.low_speed { TD_LOW_SPEED } else { 0 },
        );
        core::ptr::write_volatile(
            &raw mut INTERRUPT_TD.token,
            build_token(
                PID_IN,
                endpoint.address,
                endpoint.endpoint,
                endpoint.toggle,
                endpoint.length,
            ),
        );
        core::ptr::write_volatile(&raw mut INTERRUPT_TD.buffer, buffer);
    }
    set_queue_element(&raw mut INTERRUPT_QUEUE, unsafe {
        physical((&raw const INTERRUPT_TD).cast())
    });
    endpoint.armed = true;
}

pub fn poll() {
    let ready = {
        let guard = INTERRUPT_ENDPOINT.lock();
        match guard.as_ref() {
            Some(endpoint) if endpoint.armed => endpoint.length,
            _ => return,
        }
    };

    let control = unsafe { core::ptr::read_volatile(&raw const INTERRUPT_TD.control) };
    if control & TD_ACTIVE != 0 {
        return;
    }

    if control & (TD_STALLED | TD_CRC_TIMEOUT) != 0 {
        rearm();
        return;
    }

    if control & TD_NAK == 0 {
        let actual = (control & 0x7FF) as usize;
        let received = if actual == 0x7FF { 0 } else { actual + 1 };
        if received > 0 {
            let mut report = [0u8; 8];
            unsafe {
                let buffer = (&raw const REPORT_BUFFER).cast::<u8>();
                for index in 0..received.min(ready).min(8) {
                    report[index] = core::ptr::read_volatile(buffer.add(index));
                }
            }
            hid::on_boot_mouse_report(&report[..received.min(ready).min(8)]);
        }
        if let Some(endpoint) = INTERRUPT_ENDPOINT.lock().as_mut() {
            endpoint.toggle ^= 1;
        }
    }

    rearm();
}

pub fn active() -> bool {
    ACTIVE_IO_BASE.lock().is_some()
}
