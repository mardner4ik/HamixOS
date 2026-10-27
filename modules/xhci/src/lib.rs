#![no_std]

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering, fence};

use hamix_kpi::{self as kpi, PciHandle};

const USBCMD: usize = 0x00;
const USBSTS: usize = 0x04;
const DNCTRL: usize = 0x14;
const CRCR: usize = 0x18;
const DCBAAP: usize = 0x30;
const CONFIG: usize = 0x38;
const PORTSC: usize = 0x400;

const CMD_RUN: u32 = 1 << 0;
const CMD_RESET: u32 = 1 << 1;
const CMD_INTE: u32 = 1 << 2;
const STS_HALTED: u32 = 1 << 0;
const STS_EINT: u32 = 1 << 3;
const STS_NOT_READY: u32 = 1 << 11;

const PORT_CCS: u32 = 1 << 0;
const PORT_PED: u32 = 1 << 1;
const PORT_PR: u32 = 1 << 4;
const PORT_PP: u32 = 1 << 9;
const PORT_LWS: u32 = 1 << 16;
const PORT_CHANGES: u32 = 0x00FE_0000;
const PORT_PRC: u32 = 1 << 21;
const PORT_WPR: u32 = 1 << 31;

const IMAN: usize = 0x20;
const IMOD: usize = 0x24;
const ERSTSZ: usize = 0x28;
const ERSTBA: usize = 0x30;
const ERDP: usize = 0x38;

const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_DISABLE_SLOT: u32 = 10;
const TRB_ADDRESS_DEVICE: u32 = 11;
const TRB_CONFIGURE_EP: u32 = 12;
const TRB_EVALUATE: u32 = 13;
const TRB_RESET_EP: u32 = 14;
const TRB_SET_DEQUEUE: u32 = 16;
const EVENT_TRANSFER: u32 = 32;
const EVENT_COMMAND: u32 = 33;
const EVENT_PORT: u32 = 34;

const TRB_IOC: u32 = 1 << 5;
const TRB_ISP: u32 = 1 << 2;
const TRB_IDT: u32 = 1 << 6;
const TRB_TC: u32 = 1 << 1;
const TRB_DIR_IN: u32 = 1 << 16;

const CC_SUCCESS: u32 = 1;
const CC_SHORT: u32 = 13;

const RING_TRBS: u32 = 256;
const MAX_HCS: usize = 4;
const MAX_DEVICES: usize = 16;
const MAX_ENDPOINTS: usize = 4;
const PAGE: usize = 4096;

#[derive(Clone, Copy)]
struct Ring {
    virt: usize,
    phys: u64,
    index: u32,
    cycle: u32,
}

impl Ring {
    fn new() -> Option<Ring> {
        let (phys, virt) = dma_page()?;
        Some(Ring { virt, phys, index: 0, cycle: 1 })
    }

    fn push(&mut self, param: u64, status: u32, control: u32) -> u64 {
        let at = self.virt + self.index as usize * 16;
        let phys = self.phys + self.index as u64 * 16;
        unsafe {
            core::ptr::write_volatile(at as *mut u64, param);
            core::ptr::write_volatile((at + 8) as *mut u32, status);
            fence(Ordering::SeqCst);
            core::ptr::write_volatile((at + 12) as *mut u32, (control & !1) | self.cycle);
        }
        self.index += 1;
        if self.index == RING_TRBS - 1 {
            let link = self.virt + self.index as usize * 16;
            unsafe {
                core::ptr::write_volatile(link as *mut u64, self.phys);
                core::ptr::write_volatile((link + 8) as *mut u32, 0);
                fence(Ordering::SeqCst);
                core::ptr::write_volatile((link + 12) as *mut u32, (TRB_LINK << 10) | TRB_TC | self.cycle);
            }
            self.index = 0;
            self.cycle ^= 1;
        }
        phys
    }

    fn dequeue(&self) -> u64 {
        self.phys + self.index as u64 * 16 | self.cycle as u64
    }

    fn free(&self) {
        unsafe { kpi::hamix_dma_free(self.virt as *mut u8, PAGE) };
    }
}

#[derive(Clone, Copy)]
struct Endpoint {
    dci: u8,
    kind: u32,
    ring: Ring,
    buffer: usize,
    buffer_phys: u64,
    len: u32,
    usb: u32,
    halted: bool,
}

#[derive(Clone, Copy)]
struct Device {
    slot: u8,
    port: u8,
    context: usize,
    ep0: Ring,
    page: usize,
    page_phys: u64,
    endpoints: [Option<Endpoint>; MAX_ENDPOINTS],
    published: Option<u32>,
}

struct Hc {
    op: usize,
    rt: usize,
    db: usize,
    context_size: usize,
    ports: u32,
    dcbaa: usize,
    command: Ring,
    events: usize,
    events_phys: u64,
    event_index: u32,
    event_cycle: u32,
    input: usize,
    input_phys: u64,
    devices: [Option<Device>; MAX_DEVICES],
    command_done: Option<(u32, u8)>,
    control_done: Option<(u8, u32, u32)>,
    pending: u64,
    recover: bool,
}

static mut HCS: [Option<Hc>; MAX_HCS] = [const { None }; MAX_HCS];
static DRAINING: AtomicBool = AtomicBool::new(false);
static SERVICING: AtomicBool = AtomicBool::new(false);
static QUEUED: AtomicBool = AtomicBool::new(false);

fn hcs() -> &'static mut [Option<Hc>; MAX_HCS] {
    unsafe { &mut *(&raw mut HCS) }
}

fn read(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn write(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn write64(base: usize, offset: usize, value: u64) {
    write(base, offset, value as u32);
    write(base, offset + 4, (value >> 32) as u32);
}

fn put32(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn get32(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn sleep(ms: u64) {
    unsafe { kpi::hamix_mdelay(ms) }
}

fn now() -> u64 {
    unsafe { kpi::hamix_uptime_ms() }
}

fn wait_until(timeout_ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let start = now();
    loop {
        if done() {
            return true;
        }
        if now().saturating_sub(start) >= timeout_ms {
            return done();
        }
        unsafe { kpi::hamix_udelay(200) };
    }
}

fn dma_page() -> Option<(u64, usize)> {
    let mut phys = 0u64;
    let page = unsafe { kpi::hamix_dma_alloc(PAGE, &mut phys) };
    if page.is_null() {
        return None;
    }
    unsafe { core::ptr::write_bytes(page, 0, PAGE) };
    Some((phys, page as usize))
}

fn dma_free(virt: usize) {
    if virt != 0 {
        unsafe { kpi::hamix_dma_free(virt as *mut u8, PAGE) };
    }
}

struct Line {
    buf: [u8; 128],
    len: usize,
}

impl Line {
    fn new(text: &str) -> Line {
        let mut line = Line { buf: [0; 128], len: 0 };
        line.text(text);
        line
    }

    fn text(&mut self, text: &str) -> &mut Line {
        for b in text.bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        self
    }

    fn dec(&mut self, mut value: u64) -> &mut Line {
        let mut digits = [0u8; 20];
        let mut count = 0;
        loop {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
            if value == 0 || count == digits.len() {
                break;
            }
        }
        while count > 0 {
            count -= 1;
            if self.len < self.buf.len() {
                self.buf[self.len] = digits[count];
                self.len += 1;
            }
        }
        self
    }

    fn hex(&mut self, value: u64, width: usize) -> &mut Line {
        for i in (0..width).rev() {
            let nibble = ((value >> (i * 4)) & 0xF) as u8;
            if self.len < self.buf.len() {
                self.buf[self.len] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
                self.len += 1;
            }
        }
        self
    }

    fn print(&self) {
        unsafe { kpi::hamix_printk(self.buf.as_ptr(), self.len) };
    }
}

fn port_offset(port: u32) -> usize {
    PORTSC + (port as usize - 1) * 0x10
}

fn port_neutral(value: u32) -> u32 {
    value & !(PORT_PED | PORT_PR | PORT_CHANGES | PORT_LWS | PORT_WPR)
}

fn ring_doorbell(hc: &Hc, slot: u8, target: u32) {
    fence(Ordering::SeqCst);
    write(hc.db, slot as usize * 4, target);
}

fn handoff(hc_mmio: usize, hccparams: u32) {
    let mut offset = ((hccparams >> 16) & 0xFFFF) as usize * 4;
    let mut guard = 0;
    while offset != 0 && guard < 64 {
        guard += 1;
        let cap = read(hc_mmio, offset);
        if cap & 0xFF == 1 {
            if cap & (1 << 16) != 0 {
                write(hc_mmio, offset, cap | (1 << 24));
                if !wait_until(1000, || read(hc_mmio, offset) & (1 << 16) == 0) {
                    kpi::dev_warn("xhci: firmware did not release the controller, taking it anyway");
                    write(hc_mmio, offset, (read(hc_mmio, offset) & !(1 << 16)) | (1 << 24));
                }
            } else {
                write(hc_mmio, offset, cap | (1 << 24));
            }
            let control = read(hc_mmio, offset + 4);
            write(hc_mmio, offset + 4, (control & 0xFFFF_1FEE) | 0xE000_0000);
        }
        let next = ((cap >> 8) & 0xFF) as usize;
        if next == 0 {
            break;
        }
        offset += next * 4;
    }
}

fn intel_route_ports(handle: &PciHandle) {
    if handle.vendor != 0x8086 || !matches!(handle.device_id, 0x1E31 | 0x8C31 | 0x9C31 | 0x8CB1 | 0x9CB1) {
        return;
    }
    unsafe {
        let usb3 = kpi::hamix_pci_read32(handle, 0xDC);
        kpi::hamix_pci_write32(handle, 0xD8, usb3);
        let usb2 = kpi::hamix_pci_read32(handle, 0xD4);
        kpi::hamix_pci_write32(handle, 0xD0, usb2);
    }
}

fn drain(index: usize) {
    if DRAINING.swap(true, Ordering::Acquire) {
        return;
    }
    if let Some(hc) = hcs()[index].as_mut() {
        drain_events(hc);
    }
    DRAINING.store(false, Ordering::Release);
}

fn drain_events(hc: &mut Hc) {
    let mut handled = 0;
    loop {
        let at = hc.events + hc.event_index as usize * 16;
        let control = get32(at, 12);
        if control & 1 != hc.event_cycle {
            break;
        }
        fence(Ordering::SeqCst);
        let param = unsafe { core::ptr::read_volatile(at as *const u64) };
        let status = get32(at, 8);
        handle_event(hc, (control >> 10) & 0x3F, param, status, control);
        hc.event_index += 1;
        if hc.event_index == RING_TRBS {
            hc.event_index = 0;
            hc.event_cycle ^= 1;
        }
        handled += 1;
    }
    if handled > 0 {
        write64(hc.rt, ERDP, (hc.events_phys + hc.event_index as u64 * 16) | (1 << 3));
    }
}

fn handle_event(hc: &mut Hc, kind: u32, param: u64, status: u32, control: u32) {
    let code = status >> 24;
    let slot = (control >> 24) as u8;
    match kind {
        EVENT_COMMAND => hc.command_done = Some((code, slot)),
        EVENT_PORT => {
            let port = ((param >> 24) & 0xFF) as u32;
            if port >= 1 && port <= hc.ports {
                let value = read(hc.op, port_offset(port));
                write(hc.op, port_offset(port), port_neutral(value) | (value & PORT_CHANGES & !PORT_PRC));
                hc.pending |= 1u64 << ((port - 1) & 63);
            }
        }
        EVENT_TRANSFER => {
            let dci = ((control >> 16) & 0x1F) as u8;
            if dci == 1 {
                hc.control_done = Some((slot, code, status & 0xFF_FFFF));
                return;
            }
            let db = hc.db;
            for device in hc.devices.iter_mut().flatten() {
                if device.slot != slot {
                    continue;
                }
                for endpoint in device.endpoints.iter_mut().flatten() {
                    if endpoint.dci != dci {
                        continue;
                    }
                    if code == CC_SUCCESS || code == CC_SHORT {
                        let got = endpoint.len.saturating_sub(status & 0xFF_FFFF);
                        if got > 0 {
                            unsafe { kpi::hamix_usb_hid_report(endpoint.usb, endpoint.kind, endpoint.buffer as *const u8, got as usize) };
                        }
                        queue_interrupt(endpoint);
                        fence(Ordering::SeqCst);
                        write(db, slot as usize * 4, dci as u32);
                    } else {
                        endpoint.halted = true;
                        hc.recover = true;
                    }
                    return;
                }
            }
        }
        _ => {}
    }
}

fn queue_interrupt(endpoint: &mut Endpoint) {
    endpoint.ring.push(endpoint.buffer_phys, endpoint.len, (TRB_NORMAL << 10) | TRB_IOC | TRB_ISP);
}

fn command(index: usize, param: u64, control: u32) -> Option<(u32, u8)> {
    {
        let hc = hcs()[index].as_mut()?;
        hc.command_done = None;
        hc.command.push(param, 0, control);
        ring_doorbell(hc, 0, 0);
    }
    let start = now();
    loop {
        drain(index);
        let hc = hcs()[index].as_mut()?;
        if let Some(done) = hc.command_done.take() {
            return Some(done);
        }
        if now().saturating_sub(start) > 1000 {
            return None;
        }
        unsafe { kpi::hamix_udelay(20) };
    }
}

fn control(index: usize, slot: usize, request_type: u8, request: u8, value: u16, windex: u16, length: u16) -> Option<u32> {
    let slot_id = {
        let hc = hcs()[index].as_mut()?;
        let device = hc.devices[slot].as_mut()?;
        let setup = request_type as u64 | (request as u64) << 8 | (value as u64) << 16 | (windex as u64) << 32 | (length as u64) << 48;
        let input = request_type & 0x80 != 0;
        let transfer = if length == 0 { 0 } else if input { 3 } else { 2 };
        device.ep0.push(setup, 8, (TRB_SETUP << 10) | TRB_IDT | (transfer << 16));
        if length > 0 {
            device.ep0.push(device.page_phys, length as u32, (TRB_DATA << 10) | if input { TRB_DIR_IN } else { 0 });
        }
        let status_in = length == 0 || !input;
        device.ep0.push(0, 0, (TRB_STATUS << 10) | TRB_IOC | if status_in { TRB_DIR_IN } else { 0 });
        hc.control_done = None;
        device.slot
    };
    {
        let hc = hcs()[index].as_mut()?;
        ring_doorbell(hc, slot_id, 1);
    }
    let start = now();
    loop {
        drain(index);
        let hc = hcs()[index].as_mut()?;
        if let Some((done_slot, code, residual)) = hc.control_done.take() {
            if done_slot == slot_id {
                return if code == CC_SUCCESS || code == CC_SHORT { Some(length as u32 - residual.min(length as u32)) } else { None };
            }
        }
        if now().saturating_sub(start) > 500 {
            return None;
        }
        unsafe { kpi::hamix_udelay(50) };
    }
}

fn input_slot(hc: &Hc) -> usize {
    hc.input + hc.context_size
}

fn input_endpoint(hc: &Hc, dci: usize) -> usize {
    hc.input + hc.context_size * (dci + 1)
}

fn clear_input(hc: &Hc) {
    unsafe { core::ptr::write_bytes(hc.input as *mut u8, 0, PAGE) };
}

fn default_packet(speed: u8) -> u32 {
    match speed {
        2 => 8,
        3 => 64,
        4 | 5 | 6 | 7 => 512,
        _ => 64,
    }
}

fn free_device(device: &Device) {
    for endpoint in device.endpoints.iter().flatten() {
        endpoint.ring.free();
    }
    device.ep0.free();
    dma_free(device.context);
    dma_free(device.page);
}

fn reset_port(hc: &Hc, port: u32) -> bool {
    let offset = port_offset(port);
    let value = read(hc.op, offset);
    if value & PORT_CCS == 0 {
        return false;
    }
    if value & PORT_PED != 0 {
        return true;
    }
    write(hc.op, offset, port_neutral(value) | PORT_PR);
    let done = wait_until(200, || read(hc.op, offset) & PORT_PRC != 0);
    let after = read(hc.op, offset);
    write(hc.op, offset, port_neutral(after) | PORT_PRC);
    if !done {
        return false;
    }
    sleep(10);
    read(hc.op, offset) & PORT_PED != 0
}

struct Found {
    interface: u8,
    protocol: u8,
    endpoint: u8,
    packet: u16,
    interval: u8,
}

fn attach(index: usize, port: u32) {
    let (speed, ready) = {
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        if hc.devices.iter().flatten().any(|d| d.port as u32 == port) {
            return;
        }
        let ready = reset_port(hc, port);
        (((read(hc.op, port_offset(port)) >> 10) & 0xF) as u8, ready)
    };
    if !ready {
        return;
    }
    let Some((code, slot)) = command(index, 0, TRB_ENABLE_SLOT << 10) else {
        kpi::dev_warn("xhci: Enable Slot timed out");
        return;
    };
    if code != CC_SUCCESS || slot == 0 {
        Line::new("xhci: Enable Slot failed, code ").dec(code as u64).print();
        return;
    }
    let Some(position) = hcs()[index].as_ref().and_then(|hc| hc.devices.iter().position(|d| d.is_none())) else {
        let _ = command(index, 0, (TRB_DISABLE_SLOT << 10) | (slot as u32) << 24);
        return;
    };
    let (Some((context_phys, context)), Some(ep0), Some((page_phys, page))) = (dma_page(), Ring::new(), dma_page()) else {
        kpi::dev_err("xhci: out of DMA memory for a device");
        return;
    };
    let device = Device { slot, port: port as u8, context, ep0, page, page_phys, endpoints: [None; MAX_ENDPOINTS], published: None };
    {
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        unsafe { core::ptr::write_volatile((hc.dcbaa + slot as usize * 8) as *mut u64, context_phys) };
        hc.devices[position] = Some(device);
        clear_input(hc);
        put32(hc.input, 4, 0b11);
        let slot_ctx = input_slot(hc);
        put32(slot_ctx, 0, (speed as u32) << 20 | 1 << 27);
        put32(slot_ctx, 4, port << 16);
        let ep = input_endpoint(hc, 1);
        put32(ep, 4, 3 << 1 | 4 << 3 | default_packet(speed) << 16);
        let dequeue = ep0.dequeue();
        put32(ep, 8, dequeue as u32);
        put32(ep, 12, (dequeue >> 32) as u32);
        put32(ep, 16, 8);
    }
    let input_phys = hcs()[index].as_ref().map(|hc| hc.input_phys).unwrap_or(0);
    let addressed = command(index, input_phys, (TRB_ADDRESS_DEVICE << 10) | (slot as u32) << 24);
    if !matches!(addressed, Some((CC_SUCCESS, _))) {
        Line::new("xhci: Address Device failed on port ").dec(port as u64).print();
        detach_slot(index, position);
        return;
    }
    sleep(2);
    if control(index, position, 0x80, 6, 0x0100, 0, 8).is_none() {
        Line::new("xhci: no device descriptor from port ").dec(port as u64).print();
        detach_slot(index, position);
        return;
    }
    let packet = unsafe { core::ptr::read_volatile((page + 7) as *const u8) } as u32;
    let packet = if speed >= 4 { 1u32 << packet.min(10) } else { packet };
    if packet != 0 && packet != default_packet(speed) {
        {
            let Some(hc) = hcs()[index].as_mut() else {
                return;
            };
            clear_input(hc);
            put32(hc.input, 4, 0b10);
            let ep = input_endpoint(hc, 1);
            put32(ep, 4, 3 << 1 | 4 << 3 | packet << 16);
        }
        let _ = command(index, input_phys, (TRB_EVALUATE << 10) | (slot as u32) << 24);
    }
    if control(index, position, 0x80, 6, 0x0100, 0, 18).is_none() {
        detach_slot(index, position);
        return;
    }
    let byte = |offset: usize| unsafe { core::ptr::read_volatile((page + offset) as *const u8) };
    let vendor = byte(8) as u16 | (byte(9) as u16) << 8;
    let product = byte(10) as u16 | (byte(11) as u16) << 8;
    let device_class = (byte(4) as u32) << 16 | (byte(5) as u32) << 8 | byte(6) as u32;
    if control(index, position, 0x80, 6, 0x0200, 0, 9).is_none() {
        detach_slot(index, position);
        return;
    }
    let total = (byte(2) as u16 | (byte(3) as u16) << 8).clamp(9, 256);
    let Some(got) = control(index, position, 0x80, 6, 0x0200, 0, total) else {
        detach_slot(index, position);
        return;
    };
    let configuration = byte(5);
    let mut found: [Option<Found>; MAX_ENDPOINTS] = [const { None }; MAX_ENDPOINTS];
    let mut first_interface = if device_class >> 16 != 0 { Some(device_class) } else { None };
    let mut current: Option<(u8, u8, u8, u8)> = None;
    let mut offset = 0usize;
    let mut count = 0usize;
    while offset + 2 <= got as usize {
        let len = byte(offset) as usize;
        if len < 2 {
            break;
        }
        match byte(offset + 1) {
            4 if offset + 8 <= got as usize => {
                let class = (byte(offset + 5), byte(offset + 6), byte(offset + 7));
                if first_interface.is_none() {
                    first_interface = Some((class.0 as u32) << 16 | (class.1 as u32) << 8 | class.2 as u32);
                }
                current = Some((byte(offset + 2), class.0, class.1, class.2));
            }
            5 if offset + 7 <= got as usize => {
                if let Some((interface, class, subclass, protocol)) = current {
                    let address = byte(offset + 2);
                    let attributes = byte(offset + 3);
                    if class == 3 && subclass == 1 && (protocol == 1 || protocol == 2) && address & 0x80 != 0 && attributes & 3 == 3 && count < MAX_ENDPOINTS {
                        found[count] = Some(Found {
                            interface,
                            protocol,
                            endpoint: address & 0xF,
                            packet: (byte(offset + 4) as u16 | (byte(offset + 5) as u16) << 8) & 0x7FF,
                            interval: byte(offset + 6),
                        });
                        count += 1;
                        current = None;
                    }
                }
            }
            _ => {}
        }
        offset += len;
    }
    let mut max_dci = 1u32;
    let mut add = 1u32;
    {
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        clear_input(hc);
        let (input, context_size) = (hc.input, hc.context_size);
        let Some(dev) = hc.devices[position].as_mut() else {
            return;
        };
        let dev_context = dev.context;
        let dev_page = dev.page;
        let dev_page_phys = dev.page_phys;
        let mut rings: [Option<Endpoint>; MAX_ENDPOINTS] = [None; MAX_ENDPOINTS];
        for (i, entry) in found.iter().enumerate() {
            let Some(f) = entry else {
                continue;
            };
            let Some(ring) = Ring::new() else {
                continue;
            };
            let dci = f.endpoint as u32 * 2 + 1;
            let len = (f.packet as u32).clamp(1, 64);
            let interval = if speed >= 3 {
                (f.interval.clamp(1, 16) - 1) as u32
            } else {
                let frames = (f.interval.max(1) as u32) * 8;
                (31 - frames.leading_zeros()).clamp(3, 10)
            };
            let ep = input + context_size * (dci as usize + 1);
            put32(ep, 0, interval << 16);
            put32(ep, 4, 3 << 1 | 7 << 3 | (f.packet as u32) << 16);
            let dequeue = ring.dequeue();
            put32(ep, 8, dequeue as u32);
            put32(ep, 12, (dequeue >> 32) as u32);
            put32(ep, 16, f.packet as u32 | (f.packet as u32) << 16);
            add |= 1 << dci;
            max_dci = max_dci.max(dci);
            rings[i] = Some(Endpoint {
                dci: dci as u8,
                kind: f.protocol as u32,
                ring,
                buffer: dev_page + 256 + i * 64,
                buffer_phys: dev_page_phys + 256 + i as u64 * 64,
                len,
                usb: 0,
                halted: false,
            });
        }
        dev.endpoints = rings;
        put32(hc.input, 4, add);
        let slot_ctx = input_slot(hc);
        put32(slot_ctx, 0, (get32(dev_context, 0) & !(0x1F << 27)) | max_dci << 27);
        put32(slot_ctx, 4, get32(dev_context, 4));
        put32(slot_ctx, 8, get32(dev_context, 8));
    }
    if add != 1 {
        let configured = command(index, input_phys, (TRB_CONFIGURE_EP << 10) | (slot as u32) << 24);
        if !matches!(configured, Some((CC_SUCCESS, _))) {
            Line::new("xhci: Configure Endpoint failed for ").hex(vendor as u64, 4).text(":").hex(product as u64, 4).print();
            if let Some(dev) = hcs()[index].as_mut().and_then(|hc| hc.devices[position].as_mut()) {
                for endpoint in dev.endpoints.iter_mut() {
                    if let Some(e) = endpoint.take() {
                        e.ring.free();
                    }
                }
            }
        }
    }
    let _ = control(index, position, 0x00, 9, configuration as u16, 0, 0);
    for f in found.iter().flatten() {
        let _ = control(index, position, 0x21, 0x0B, 0, f.interface as u16, 0);
        let _ = control(index, position, 0x21, 0x0A, 0, f.interface as u16, 0);
    }
    let mut line = Line::new("xhci: port ");
    line.dec(port as u64).text(" device ").hex(vendor as u64, 4).text(":").hex(product as u64, 4);
    let Some(hc) = hcs()[index].as_mut() else {
        return;
    };
    let db = hc.db;
    let Some(dev) = hc.devices[position].as_mut() else {
        return;
    };
    let mut published = false;
    for endpoint in dev.endpoints.iter_mut() {
        let Some(endpoint) = endpoint else {
            continue;
        };
        let interface = 0x03_01_00 | endpoint.kind;
        let usb = unsafe { kpi::hamix_usb_add_device(port, vendor, product, interface, speed as u32) };
        if usb < 0 {
            continue;
        }
        published = true;
        endpoint.usb = usb as u32;
        line.text(if endpoint.kind == 1 { " keyboard" } else { " mouse" });
        queue_interrupt(endpoint);
        fence(Ordering::SeqCst);
        write(db, dev.slot as usize * 4, endpoint.dci as u32);
    }
    if !published {
        let first_interface = first_interface.unwrap_or(0);
        let usb = unsafe { kpi::hamix_usb_add_device(port, vendor, product, first_interface, speed as u32) };
        if usb >= 0 {
            dev.published = Some(usb as u32);
        }
        line.text(" class ").hex(first_interface as u64, 6);
    }
    line.print();
}

fn detach_slot(index: usize, position: usize) {
    let removed = {
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        hc.devices[position].take()
    };
    let Some(device) = removed else {
        return;
    };
    for endpoint in device.endpoints.iter().flatten() {
        unsafe { kpi::hamix_usb_remove_device(endpoint.usb) };
    }
    if let Some(usb) = device.published {
        unsafe { kpi::hamix_usb_remove_device(usb) };
    }
    let _ = command(index, 0, (TRB_DISABLE_SLOT << 10) | (device.slot as u32) << 24);
    if let Some(hc) = hcs()[index].as_mut() {
        unsafe { core::ptr::write_volatile((hc.dcbaa + device.slot as usize * 8) as *mut u64, 0) };
    }
    free_device(&device);
}

fn recover(index: usize) {
    let mut work: [(u8, u8, u64); MAX_DEVICES] = [(0, 0, 0); MAX_DEVICES];
    let mut count = 0;
    {
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        if !hc.recover {
            return;
        }
        hc.recover = false;
        for device in hc.devices.iter_mut().flatten() {
            for endpoint in device.endpoints.iter_mut().flatten() {
                if endpoint.halted && count < work.len() {
                    endpoint.halted = false;
                    work[count] = (device.slot, endpoint.dci, endpoint.ring.dequeue());
                    count += 1;
                }
            }
        }
    }
    for &(slot, dci, dequeue) in work[..count].iter() {
        let target = (slot as u32) << 24 | (dci as u32) << 16;
        let _ = command(index, 0, (TRB_RESET_EP << 10) | target);
        let _ = command(index, dequeue, (TRB_SET_DEQUEUE << 10) | target);
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        let db = hc.db;
        for device in hc.devices.iter_mut().flatten().filter(|d| d.slot == slot) {
            for endpoint in device.endpoints.iter_mut().flatten().filter(|e| e.dci == dci) {
                queue_interrupt(endpoint);
                fence(Ordering::SeqCst);
                write(db, slot as usize * 4, dci as u32);
            }
        }
    }
}

fn service(index: usize) {
    if SERVICING.swap(true, Ordering::Acquire) {
        return;
    }
    service_locked(index);
    SERVICING.store(false, Ordering::Release);
}

fn needs_service(index: usize) -> bool {
    hcs()[index].as_ref().map(|hc| hc.pending != 0 || hc.recover).unwrap_or(false)
}

fn service_locked(index: usize) {
    drain(index);
    recover(index);
    let (pending, ports) = {
        let Some(hc) = hcs()[index].as_mut() else {
            return;
        };
        let pending = hc.pending;
        (pending, hc.ports)
    };
    if pending == 0 {
        return;
    }
    let port = pending.trailing_zeros() + 1;
    if let Some(hc) = hcs()[index].as_mut() {
        hc.pending &= !(1u64 << (port - 1));
    }
    if port > ports {
        return;
    }
    let (connected, position) = {
        let Some(hc) = hcs()[index].as_ref() else {
            return;
        };
        let connected = read(hc.op, port_offset(port)) & PORT_CCS != 0;
        (connected, hc.devices.iter().position(|d| d.map(|d| d.port as u32 == port).unwrap_or(false)))
    };
    match (connected, position) {
        (true, None) => {
            sleep(100);
            attach(index, port);
        }
        (false, Some(position)) => {
            detach_slot(index, position);
            Line::new("xhci: port ").dec(port as u64).text(" disconnected").print();
        }
        _ => {}
    }
}

extern "C" fn serve(context: *mut c_void) {
    QUEUED.store(false, Ordering::Release);
    let index = context as usize;
    let mut rounds = 0;
    while needs_service(index) && rounds < 64 {
        service(index);
        rounds += 1;
    }
}

extern "C" fn poll(context: *mut c_void) {
    let index = context as usize;
    drain(index);
    if needs_service(index) && !QUEUED.swap(true, Ordering::AcqRel) && !kpi::schedule_work(serve, context) {
        QUEUED.store(false, Ordering::Release);
    }
}

extern "C" fn interrupt(context: *mut c_void) -> i32 {
    let index = context as usize;
    let Some(hc) = hcs()[index].as_ref() else {
        return 0;
    };
    let status = read(hc.op, USBSTS);
    let iman = read(hc.rt, IMAN);
    if status & STS_EINT == 0 && iman & 1 == 0 {
        return 0;
    }
    write(hc.op, USBSTS, STS_EINT);
    write(hc.rt, IMAN, iman | 1);
    drain(index);
    1
}

fn start(handle: &PciHandle, slot: usize) -> Result<u32, &'static str> {
    let (base, len) = kpi::bar(handle, 0);
    if base == 0 || len == 0 {
        return Err("xhci: BAR0 is not a memory BAR");
    }
    let mmio = unsafe { kpi::hamix_ioremap(base, (len as usize).max(0x1000)) } as usize;
    if mmio == 0 {
        return Err("xhci: the kernel refused to map BAR0");
    }
    let command_reg = unsafe { kpi::hamix_pci_read16(handle, 0x04) };
    unsafe { kpi::hamix_pci_write16(handle, 0x04, (command_reg | 0x0006) & !0x0400) };
    let caplength = read(mmio, 0) & 0xFF;
    let hcsparams1 = read(mmio, 0x04);
    let hcsparams2 = read(mmio, 0x08);
    let hccparams1 = read(mmio, 0x10);
    let op = mmio + caplength as usize;
    let rt = mmio + (read(mmio, 0x18) & !0x1F) as usize;
    let db = mmio + (read(mmio, 0x14) & !0x3) as usize;
    handoff(mmio, hccparams1);
    intel_route_ports(handle);
    write(op, USBCMD, read(op, USBCMD) & !(CMD_RUN | CMD_INTE));
    if !wait_until(100, || read(op, USBSTS) & STS_HALTED != 0) {
        return Err("xhci: controller did not halt");
    }
    write(op, USBCMD, CMD_RESET);
    if !wait_until(1000, || read(op, USBCMD) & CMD_RESET == 0 && read(op, USBSTS) & STS_NOT_READY == 0) {
        return Err("xhci: controller reset timed out");
    }
    let slots = (hcsparams1 & 0xFF).min(64);
    let ports = ((hcsparams1 >> 24) & 0xFF).min(64);
    let context_size = if hccparams1 & (1 << 2) != 0 { 64 } else { 32 };
    let (dcbaa_phys, dcbaa) = dma_page().ok_or("xhci: out of DMA memory")?;
    let scratch_count = (((hcsparams2 >> 21) & 0x1F) << 5 | (hcsparams2 >> 27) & 0x1F) as usize;
    if scratch_count > 0 {
        let (array_phys, array) = dma_page().ok_or("xhci: out of DMA memory")?;
        for i in 0..scratch_count.min(PAGE / 8) {
            let (phys, _) = dma_page().ok_or("xhci: out of DMA memory for scratchpads")?;
            unsafe { core::ptr::write_volatile((array + i * 8) as *mut u64, phys) };
        }
        unsafe { core::ptr::write_volatile(dcbaa as *mut u64, array_phys) };
    }
    let command = Ring::new().ok_or("xhci: out of DMA memory")?;
    let (events_phys, events) = dma_page().ok_or("xhci: out of DMA memory")?;
    let (erst_phys, erst) = dma_page().ok_or("xhci: out of DMA memory")?;
    let (input_phys, input) = dma_page().ok_or("xhci: out of DMA memory")?;
    unsafe {
        core::ptr::write_volatile(erst as *mut u64, events_phys);
        core::ptr::write_volatile((erst + 8) as *mut u32, RING_TRBS);
    }
    write(op, CONFIG, (read(op, CONFIG) & !0xFF) | slots);
    write(op, DNCTRL, 0x2);
    write64(op, DCBAAP, dcbaa_phys);
    write64(op, CRCR, command.phys | 1);
    write(rt, ERSTSZ, 1);
    write64(rt, ERDP, events_phys);
    write64(rt, ERSTBA, erst_phys);
    write(rt, IMOD, 160);
    hcs()[slot] = Some(Hc {
        op,
        rt,
        db,
        context_size,
        ports,
        dcbaa,
        command,
        events,
        events_phys,
        event_index: 0,
        event_cycle: 1,
        input,
        input_phys,
        devices: [None; MAX_DEVICES],
        command_done: None,
        control_done: None,
        pending: 0,
        recover: false,
    });
    let irq = kpi::request_irq(handle, interrupt, slot as *mut c_void);
    if irq.is_some() {
        write(rt, IMAN, read(rt, IMAN) | 0b11);
        write(op, USBCMD, CMD_RUN | CMD_INTE);
    } else {
        write(rt, IMAN, (read(rt, IMAN) | 1) & !0b10);
        write(op, USBCMD, CMD_RUN);
    }
    if !wait_until(100, || read(op, USBSTS) & STS_HALTED == 0) {
        return Err("xhci: controller did not start");
    }
    if hccparams1 & (1 << 3) != 0 {
        for port in 1..=ports {
            let value = read(op, port_offset(port));
            if value & PORT_PP == 0 {
                write(op, port_offset(port), port_neutral(value) | PORT_PP);
            }
        }
        sleep(20);
    }
    unsafe { kpi::hamix_usb_register_hcd(handle, 3, ports) };
    let mut line = Line::new("xhci: ");
    line.hex(handle.vendor as u64, 4).text(":").hex(handle.device_id as u64, 4).text(" running, ");
    line.dec(ports as u64).text(" ports, ").dec(slots as u64).text(" slots, ").dec(scratch_count as u64).text(" scratchpads");
    line.text(if irq.is_some() { ", interrupts" } else { ", polled" }).print();
    Ok(ports)
}

fn init() -> i32 {
    let mut index = 0u32;
    let mut running = 0usize;
    let mut seen = 0usize;
    while let Some(handle) = kpi::find_class(0x0C, 0x03, 0x30, index) {
        index += 1;
        seen += 1;
        if running >= MAX_HCS {
            break;
        }
        match start(&handle, running) {
            Ok(ports) => {
                kpi::claim(kpi::CLASS_USB_HCD, "xhci", &handle, Some(poll), running as *mut c_void);
                sleep(50);
                if let Some(hc) = hcs()[running].as_mut() {
                    for port in 1..=ports {
                        if read(hc.op, port_offset(port)) & PORT_CCS != 0 {
                            hc.pending |= 1u64 << ((port - 1) & 63);
                        }
                    }
                }
                for _ in 0..ports {
                    service(running);
                }
                running += 1;
            }
            Err(reason) => {
                kpi::dev_warn(reason);
                hcs()[running] = None;
            }
        }
    }
    if seen == 0 {
        kpi::printk("xhci: no xHCI controller");
        return -1;
    }
    if running == 0 {
        kpi::printk("xhci: no controller could be started");
        return -1;
    }
    0
}

fn exit() {
    for index in 0..MAX_HCS {
        for position in 0..MAX_DEVICES {
            if hcs()[index].as_ref().map(|hc| hc.devices[position].is_some()).unwrap_or(false) {
                detach_slot(index, position);
            }
        }
        if let Some(hc) = hcs()[index].take() {
            write(hc.op, USBCMD, read(hc.op, USBCMD) & !(CMD_RUN | CMD_INTE));
            hc.command.free();
            dma_free(hc.events);
            dma_free(hc.input);
        }
    }
    unsafe { kpi::hamix_free_irq() };
}

hamix_kpi::module!(init = init, exit = exit, version = "1.0");
