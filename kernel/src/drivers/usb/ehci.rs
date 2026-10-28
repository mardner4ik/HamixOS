use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use crate::drivers::pci::{self, PciDevice};
use crate::drivers::usb::uhci::{parse_configuration, InterfaceInfo};
use crate::drivers::usb::{hid, Controller, DeviceClass, UsbDevice, DEVICES};
use crate::memory::frame;

const USBCMD: usize = 0x00;
const USBSTS: usize = 0x04;
const USBINTR: usize = 0x08;
const CTRLDSSEGMENT: usize = 0x10;
const PERIODICLISTBASE: usize = 0x14;
const ASYNCLISTADDR: usize = 0x18;
const CONFIGFLAG: usize = 0x40;
const PORTSC: usize = 0x44;

const CMD_RUN: u32 = 1 << 0;
const CMD_RESET: u32 = 1 << 1;
const CMD_PERIODIC: u32 = 1 << 4;
const CMD_ASYNC: u32 = 1 << 5;
const STS_HALTED: u32 = 1 << 12;
const STS_PERIODIC: u32 = 1 << 14;
const STS_ASYNC: u32 = 1 << 15;

const PORT_CONNECTED: u32 = 1 << 0;
const PORT_CONNECT_CHANGE: u32 = 1 << 1;
const PORT_ENABLED: u32 = 1 << 2;
const PORT_ENABLE_CHANGE: u32 = 1 << 3;
const PORT_OVERCURRENT_CHANGE: u32 = 1 << 5;
const PORT_RESET: u32 = 1 << 8;
const PORT_LINE_STATUS: u32 = 3 << 10;
const PORT_LINE_K: u32 = 1 << 10;
const PORT_POWER: u32 = 1 << 12;
const PORT_OWNER: u32 = 1 << 13;
const PORT_CLEAR_BITS: u32 = PORT_CONNECT_CHANGE | PORT_ENABLE_CHANGE | PORT_OVERCURRENT_CHANGE;

const LINK_TERMINATE: u32 = 1;
const LINK_QH: u32 = 2;

const TOKEN_ACTIVE: u32 = 1 << 7;
const TOKEN_HALTED: u32 = 1 << 6;
const TOKEN_ERRORS: u32 = TOKEN_HALTED | (1 << 5) | (1 << 4) | (1 << 3);
const TOKEN_CERR: u32 = 3 << 10;
const TOKEN_TOGGLE: u32 = 1 << 31;

const PID_OUT: u32 = 0;
const PID_IN: u32 = 1;
const PID_SETUP: u32 = 2;

const QH_OFFSET: usize = 0;
const DUMMY_OFFSET: usize = 128;
const TD_OFFSET: usize = 256;
const TD_STRIDE: usize = 32;
const MAX_TDS: usize = 96;
const SETUP_OFFSET: usize = 4096;
const CONTROL_DATA_OFFSET: usize = 4096 + 64;
const CONTROL_DATA: usize = 2048;
const BULK_OFFSET: usize = 8192;
const BULK_BYTES: usize = 64 * 1024;
const PERIODIC_OFFSET: usize = BULK_OFFSET + BULK_BYTES;
const INTERRUPT_OFFSET: usize = PERIODIC_OFFSET + 4096;
const INTERRUPT_STRIDE: usize = 256;
const MAX_INTERRUPTS: usize = 16;
const REGION_PAGES: usize = 4 + BULK_BYTES / 4096;
const TD_BYTES: usize = 16 * 1024;

const REQUEST_SET_IDLE: u8 = 10;
const REQUEST_SET_PROTOCOL: u8 = 11;

const CBW_SIGNATURE: u32 = 0x4342_5355;
const CSW_SIGNATURE: u32 = 0x5342_5355;

fn rd32(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

fn wr32(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

fn delay_ms(ms: u64) {
    crate::arch::spin_delay_us(ms * 1000);
}

#[derive(Clone, Copy, PartialEq)]
struct Route {
    speed: u32,
    hub: u8,
    port: u8,
}

const HIGH_SPEED: Route = Route { speed: 2, hub: 0, port: 0 };

const HUB_PORT_CONNECTION: u16 = 1 << 0;
const HUB_PORT_ENABLE: u16 = 1 << 1;
const HUB_PORT_LOW_SPEED: u16 = 1 << 9;
const HUB_PORT_HIGH_SPEED: u16 = 1 << 10;
const HUB_C_CONNECTION: u16 = 1 << 0;
const HUB_C_RESET: u16 = 1 << 4;
const FEATURE_PORT_RESET: u8 = 4;
const FEATURE_PORT_POWER: u8 = 8;
const FEATURE_C_PORT_CONNECTION: u8 = 16;
const FEATURE_C_PORT_ENABLE: u8 = 17;
const FEATURE_C_PORT_RESET: u8 = 20;

struct Hub {
    address: u8,
    root_port: u8,
    ports: u8,
    connected: [bool; 16],
    children: [u8; 16],
}

struct Interrupt {
    slot: usize,
    address: u8,
    endpoint: u8,
    errors: u32,
    last_error: u32,
    reports: u32,
    reported: bool,
    port: u8,
    hub: u8,
    hub_port: u8,
    length: usize,
    keyboard: bool,
    previous: [u8; 8],
}

struct Stage {
    pid: u32,
    offset: usize,
    len: usize,
    toggle: u32,
}

struct Ehci {
    base: usize,
    op: usize,
    ports: u8,
    region: usize,
    connected: [bool; 16],
    next_address: u8,
    found: Vec<Storage>,
    routes: [Route; 128],
    hubs: Vec<Hub>,
    interrupts: Vec<Interrupt>,
}

struct Storage {
    base: usize,
    address: u8,
    port: u8,
    hub: u8,
    hub_port: u8,
    interface: u8,
    in_ep: u8,
    out_ep: u8,
    in_packet: u32,
    out_packet: u32,
    in_toggle: u32,
    out_toggle: u32,
    tag: u32,
    block_size: u64,
    blocks: u64,
    model: String,
    alive: bool,
    disk: Option<usize>,
}

static CONTROLLERS: Mutex<Vec<Ehci>> = Mutex::new(Vec::new());
static STORAGE: Mutex<Vec<Storage>> = Mutex::new(Vec::new());

impl Ehci {
    fn op_read(&self, reg: usize) -> u32 {
        rd32(self.op + reg)
    }

    fn op_write(&self, reg: usize, value: u32) {
        wr32(self.op + reg, value)
    }

    fn port_read(&self, port: u8) -> u32 {
        self.op_read(PORTSC + port as usize * 4)
    }

    fn port_write(&self, port: u8, value: u32) {
        self.op_write(PORTSC + port as usize * 4, value)
    }

    fn qh(&self) -> usize {
        self.region + QH_OFFSET
    }

    fn td(&self, index: usize) -> usize {
        self.region + TD_OFFSET + index * TD_STRIDE
    }

    fn set_async(&self, on: bool) -> bool {
        let command = self.op_read(USBCMD);
        self.op_write(USBCMD, if on { command | CMD_ASYNC } else { command & !CMD_ASYNC });
        for _ in 0..2000 {
            if (self.op_read(USBSTS) & STS_ASYNC != 0) == on {
                return true;
            }
            core::hint::spin_loop();
            crate::arch::spin_delay_us(5);
        }
        false
    }

    fn transfer(&mut self, address: u8, endpoint: u8, max_packet: u32, stages: &[Stage], qh_toggle: Option<u32>) -> Result<(Vec<usize>, u32), bool> {
        if stages.is_empty() || stages.len() > MAX_TDS {
            return Err(false);
        }
        self.set_async(false);
        let qh = self.qh();
        let dummy = self.region + DUMMY_OFFSET;
        for i in 0..8 {
            wr32(dummy + i * 4, 0);
        }
        wr32(dummy, LINK_TERMINATE);
        wr32(dummy + 4, LINK_TERMINATE);
        for (i, stage) in stages.iter().enumerate() {
            let td = self.td(i);
            let next = if i + 1 < stages.len() { self.td(i + 1) as u32 } else { LINK_TERMINATE };
            let buffer = self.region + stage.offset;
            let alternate = stages[i + 1..].iter().position(|later| later.pid != stage.pid).map(|k| self.td(i + 1 + k) as u32).unwrap_or(dummy as u32);
            wr32(td, next);
            wr32(td + 4, alternate);
            wr32(td + 8, TOKEN_ACTIVE | TOKEN_CERR | (stage.pid << 8) | ((stage.len as u32) << 16) | (stage.toggle << 31));
            wr32(td + 12, buffer as u32);
            for page in 1..5 {
                wr32(td + 12 + page * 4, ((buffer & !0xFFF) + page * 4096) as u32);
            }
        }
        wr32(qh, qh as u32 | LINK_QH);
        let toggle_from_td = if qh_toggle.is_none() { 1 << 14 } else { 0 };
        let route = self.routes[(address & 0x7f) as usize];
        let control_flag = if route.speed != 2 && endpoint == 0 { 1 << 27 } else { 0 };
        wr32(qh + 4, address as u32 | ((endpoint as u32) << 8) | (route.speed << 12) | toggle_from_td | (1 << 15) | (max_packet << 16) | control_flag);
        wr32(qh + 8, (1 << 30) | ((route.hub as u32) << 16) | ((route.port as u32) << 23));
        wr32(qh + 12, 0);
        wr32(qh + 16, self.td(0) as u32);
        wr32(qh + 20, LINK_TERMINATE);
        for i in 6..17 {
            wr32(qh + i * 4, 0);
        }
        wr32(qh + 24, qh_toggle.unwrap_or(0) << 31);
        self.op_write(ASYNCLISTADDR, qh as u32);
        self.set_async(true);
        let started = crate::task::uptime_ms();
        let mut halted = false;
        loop {
            let mut active = false;
            for i in 0..stages.len() {
                let token = rd32(self.td(i) + 8);
                if token & TOKEN_HALTED != 0 {
                    halted = true;
                }
                if token & TOKEN_ACTIVE != 0 {
                    active = true;
                }
            }
            if halted || !active {
                break;
            }
            if rd32(qh + 12) == dummy as u32 && rd32(qh + 24) & TOKEN_ACTIVE == 0 {
                break;
            }
            if self.op_read(USBSTS) & STS_HALTED != 0 || crate::task::uptime_ms().saturating_sub(started) > 2000 {
                self.set_async(false);
                crate::drivers::klog::log(&format!("ehci: transfer to {}.{} timed out (status {:#x}, token {:#x})", address, endpoint, self.op_read(USBSTS), rd32(qh + 24)));
                return Err(false);
            }
            core::hint::spin_loop();
        }
        self.set_async(false);
        let mut actual = Vec::with_capacity(stages.len());
        let mut toggle = stages[0].toggle;
        let mut stalled = false;
        for (i, stage) in stages.iter().enumerate() {
            let token = rd32(self.td(i) + 8);
            if token & TOKEN_ACTIVE != 0 {
                break;
            }
            if token & TOKEN_ERRORS != 0 {
                stalled = token & TOKEN_HALTED != 0 && token & ((1 << 5) | (1 << 4) | (1 << 3)) == 0;
                break;
            }
            let remaining = ((token >> 16) & 0x7FFF) as usize;
            actual.push(stage.len.saturating_sub(remaining));
            toggle = (token & TOKEN_TOGGLE) >> 31;
        }
        if halted {
            if !stalled {
                crate::drivers::klog::log(&format!("ehci: transfer to {}.{} failed (token {:#x})", address, endpoint, rd32(qh + 24)));
            }
            return Err(stalled);
        }
        if qh_toggle.is_some() {
            toggle = (rd32(qh + 24) & TOKEN_TOGGLE) >> 31;
        }
        Ok((actual, toggle))
    }

    fn control(&mut self, address: u8, max_packet: u32, setup: [u8; 8]) -> Option<Vec<u8>> {
        let length = u16::from_le_bytes([setup[6], setup[7]]) as usize;
        if length > CONTROL_DATA {
            return None;
        }
        let device_to_host = setup[0] & 0x80 != 0;
        unsafe {
            core::ptr::copy_nonoverlapping(setup.as_ptr(), (self.region + SETUP_OFFSET) as *mut u8, 8);
            core::ptr::write_bytes((self.region + CONTROL_DATA_OFFSET) as *mut u8, 0, CONTROL_DATA);
        }
        let mut stages = alloc::vec![Stage { pid: PID_SETUP, offset: SETUP_OFFSET, len: 8, toggle: 0 }];
        if length > 0 {
            stages.push(Stage { pid: if device_to_host { PID_IN } else { PID_OUT }, offset: CONTROL_DATA_OFFSET, len: length, toggle: 1 });
        }
        stages.push(Stage { pid: if device_to_host && length > 0 { PID_OUT } else { PID_IN }, offset: CONTROL_DATA_OFFSET, len: 0, toggle: 1 });
        let (actual, _) = self.transfer(address, 0, max_packet, &stages, None).ok()?;
        if actual.len() < stages.len() {
            return None;
        }
        let received = if length > 0 { actual.get(1).copied().unwrap_or(0) } else { 0 };
        let mut out = alloc::vec![0u8; received.min(length)];
        unsafe { core::ptr::copy_nonoverlapping((self.region + CONTROL_DATA_OFFSET) as *const u8, out.as_mut_ptr(), out.len()) };
        Some(out)
    }

    fn get_descriptor(&mut self, address: u8, max_packet: u32, kind: u8, length: u16) -> Option<Vec<u8>> {
        self.control(address, max_packet, [0x80, 6, 0, kind, 0, 0, length as u8, (length >> 8) as u8])
    }

    fn bulk(&mut self, dev: &mut Storage, input: bool, data: &mut [u8]) -> Result<usize, bool> {
        let packet = if input { dev.in_packet } else { dev.out_packet };
        let endpoint = if input { dev.in_ep } else { dev.out_ep };
        let mut done = 0usize;
        while done < data.len() || (data.is_empty() && done == 0) {
            let chunk = (data.len() - done).min(BULK_BYTES);
            if !input && chunk > 0 {
                unsafe { core::ptr::copy_nonoverlapping(data[done..].as_ptr(), (self.region + BULK_OFFSET) as *mut u8, chunk) };
            }
            let toggle = if input { dev.in_toggle } else { dev.out_toggle };
            let mut stages = Vec::new();
            let mut at = 0usize;
            loop {
                let len = (chunk - at).min(TD_BYTES);
                stages.push(Stage { pid: if input { PID_IN } else { PID_OUT }, offset: BULK_OFFSET + at, len, toggle });
                at += len;
                if at >= chunk {
                    break;
                }
            }
            let result = self.transfer(dev.address, endpoint, packet, &stages, Some(toggle));
            let (actual, next_toggle) = match result {
                Ok(v) => v,
                Err(stalled) => return Err(stalled),
            };
            if input { dev.in_toggle = next_toggle } else { dev.out_toggle = next_toggle }
            let moved: usize = actual.iter().sum();
            if input {
                unsafe { core::ptr::copy_nonoverlapping((self.region + BULK_OFFSET) as *const u8, data[done..].as_mut_ptr(), moved.min(chunk)) };
            }
            done += moved;
            if chunk == 0 || moved < chunk {
                break;
            }
        }
        Ok(done)
    }

    fn clear_halt(&mut self, dev: &mut Storage, input: bool) {
        let endpoint = if input { 0x80 | dev.in_ep } else { dev.out_ep };
        self.control(dev.address, 64, [0x02, 1, 0, 0, endpoint, 0, 0, 0]);
        if input { dev.in_toggle = 0 } else { dev.out_toggle = 0 }
    }

    fn reset_recovery(&mut self, dev: &mut Storage) {
        self.control(dev.address, 64, [0x21, 0xFF, 0, 0, dev.interface, 0, 0, 0]);
        delay_ms(5);
        self.clear_halt(dev, true);
        self.clear_halt(dev, false);
    }

    fn scsi(&mut self, dev: &mut Storage, cdb: &[u8], data: &mut [u8], input: bool) -> Result<(), ()> {
        let mut cbw = [0u8; 31];
        cbw[0..4].copy_from_slice(&CBW_SIGNATURE.to_le_bytes());
        dev.tag = dev.tag.wrapping_add(1);
        cbw[4..8].copy_from_slice(&dev.tag.to_le_bytes());
        cbw[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
        cbw[12] = if input { 0x80 } else { 0 };
        cbw[14] = cdb.len() as u8;
        cbw[15..15 + cdb.len()].copy_from_slice(cdb);
        if self.bulk(dev, false, &mut cbw).is_err() {
            self.reset_recovery(dev);
            return Err(());
        }
        if !data.is_empty() {
            match self.bulk(dev, input, data) {
                Ok(_) => {}
                Err(true) => self.clear_halt(dev, input),
                Err(false) => {
                    self.reset_recovery(dev);
                    return Err(());
                }
            }
        }
        let mut csw = [0u8; 13];
        let mut got = self.bulk(dev, true, &mut csw);
        if got == Err(true) {
            self.clear_halt(dev, true);
            got = self.bulk(dev, true, &mut csw);
        }
        if got != Ok(13) || u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]) != CSW_SIGNATURE {
            self.reset_recovery(dev);
            return Err(());
        }
        match csw[12] {
            0 => Ok(()),
            2 => {
                self.reset_recovery(dev);
                Err(())
            }
            _ => Err(()),
        }
    }

    fn storage_init(&mut self, dev: &mut Storage) -> bool {
        let mut inquiry = [0u8; 36];
        if self.scsi(dev, &[0x12, 0, 0, 0, 36, 0], &mut inquiry, true).is_ok() {
            let vendor = String::from_utf8_lossy(&inquiry[8..16]).trim().to_string();
            let product = String::from_utf8_lossy(&inquiry[16..32]).trim().to_string();
            dev.model = format!("{} {}", vendor, product).trim().to_string();
        }
        let mut ready = false;
        for _ in 0..30 {
            if self.scsi(dev, &[0; 6], &mut [], false).is_ok() {
                ready = true;
                break;
            }
            let mut sense = [0u8; 18];
            let _ = self.scsi(dev, &[0x03, 0, 0, 0, 18, 0], &mut sense, true);
            delay_ms(100);
        }
        if !ready {
            crate::drivers::klog::log(&format!("usb-storage: {} has no medium or is not ready", dev.model));
            return false;
        }
        let mut capacity = [0u8; 8];
        if self.scsi(dev, &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0], &mut capacity, true).is_err() {
            return false;
        }
        let last = u32::from_be_bytes([capacity[0], capacity[1], capacity[2], capacity[3]]) as u64;
        let size = u32::from_be_bytes([capacity[4], capacity[5], capacity[6], capacity[7]]) as u64;
        if !matches!(size, 512 | 1024 | 2048 | 4096) || last == 0 {
            return false;
        }
        dev.block_size = size;
        dev.blocks = last + 1;
        let probe_blocks = (BULK_BYTES as u64 / size).min(dev.blocks) as usize;
        let mut probe = alloc::vec![0u8; probe_blocks * size as usize];
        if self.storage_io(dev, 0, probe_blocks, &mut probe, false).is_err() {
            crate::drivers::klog::log(&format!("usb-storage: {} fails reads at high speed", dev.model));
            return false;
        }
        let speed = match self.routes[(dev.address & 0x7f) as usize].speed {
            2 => "high speed",
            1 => "low speed",
            _ => "full speed",
        };
        let place = if dev.hub != 0 { format!("port {} hub {} port {}", dev.port, dev.hub, dev.hub_port) } else { format!("port {}", dev.port) };
        crate::drivers::klog::log(&format!("usb-storage: {} on EHCI {:#x} {} ({}): {} blocks of {} bytes", dev.model, dev.base, place, speed, dev.blocks, size));
        true
    }

    fn storage_io(&mut self, dev: &mut Storage, block: u64, count: usize, data: &mut [u8], write: bool) -> Result<(), ()> {
        let per = (BULK_BYTES / dev.block_size as usize).max(1);
        let mut done = 0usize;
        while done < count {
            let n = (count - done).min(per);
            let lba = (block + done as u64) as u32;
            let opcode = if write { 0x2A } else { 0x28 };
            let cdb = [opcode, 0, (lba >> 24) as u8, (lba >> 16) as u8, (lba >> 8) as u8, lba as u8, 0, (n >> 8) as u8, n as u8, 0];
            let bytes = n * dev.block_size as usize;
            let slice = &mut data[done * dev.block_size as usize..done * dev.block_size as usize + bytes];
            if !(0..3).any(|_| self.scsi(dev, &cdb, slice, !write).is_ok()) {
                return Err(());
            }
            done += n;
        }
        Ok(())
    }

    fn hand_over(&self, port: u8) {
        let status = self.port_read(port);
        self.port_write(port, (status & !PORT_CLEAR_BITS) | PORT_OWNER);
    }

    fn reset_port(&self, port: u8) -> bool {
        let status = self.port_read(port) & !PORT_CLEAR_BITS;
        self.port_write(port, (status & !PORT_ENABLED) | PORT_RESET);
        delay_ms(50);
        self.port_write(port, self.port_read(port) & !PORT_CLEAR_BITS & !PORT_RESET);
        for _ in 0..100 {
            if self.port_read(port) & PORT_RESET == 0 {
                break;
            }
            delay_ms(1);
        }
        delay_ms(10);
        self.port_read(port) & PORT_ENABLED != 0
    }

    fn hub_request(&mut self, hub: u8, request: u8, feature: u8, port: u8) -> bool {
        self.control(hub, 64, [0x23, request, feature, 0, port, 0, 0, 0]).is_some()
    }

    fn hub_status(&mut self, hub: u8, port: u8) -> Option<(u16, u16)> {
        let raw = self.control(hub, 64, [0xa3, 0, 0, 0, port, 0, 4, 0])?;
        if raw.len() < 4 {
            return None;
        }
        Some((u16::from_le_bytes([raw[0], raw[1]]), u16::from_le_bytes([raw[2], raw[3]])))
    }

    fn hub_reset(&mut self, hub: u8, port: u8) -> Option<u32> {
        if !self.hub_request(hub, 3, FEATURE_PORT_RESET, port) {
            return None;
        }
        let mut status = None;
        for _ in 0..50 {
            delay_ms(10);
            if let Some((s, change)) = self.hub_status(hub, port) {
                if change & HUB_C_RESET != 0 {
                    status = Some(s);
                    break;
                }
            }
        }
        self.hub_request(hub, 1, FEATURE_C_PORT_RESET, port);
        self.hub_request(hub, 1, FEATURE_C_PORT_ENABLE, port);
        delay_ms(10);
        let status = status?;
        if status & HUB_PORT_ENABLE == 0 {
            return None;
        }
        Some(if status & HUB_PORT_HIGH_SPEED != 0 { 2 } else if status & HUB_PORT_LOW_SPEED != 0 { 1 } else { 0 })
    }

    fn setup_hub(&mut self, address: u8, max_packet: u32, root_port: u8) -> bool {
        let Some(descriptor) = self.control(address, max_packet, [0xa0, 6, 0, 0x29, 0, 0, 16, 0]) else {
            return false;
        };
        if descriptor.len() < 7 {
            return false;
        }
        let ports = descriptor[2].min(15);
        let power_ms = (descriptor[5] as u64 * 2).max(20);
        for p in 1..=ports {
            self.hub_request(address, 3, FEATURE_PORT_POWER, p);
        }
        delay_ms(power_ms + 20);
        crate::drivers::klog::log(&format!("usb: hub at address {} on EHCI {:#x} port {}: {} port(s)", address, self.base, root_port, ports));
        self.hubs.push(Hub { address, root_port, ports, connected: [false; 16], children: [0; 16] });
        true
    }

    fn hub_changes(&mut self) -> Vec<(u8, u8, u8, bool)> {
        let mut changes = Vec::new();
        let hubs: Vec<(u8, u8, u8)> = self.hubs.iter().map(|h| (h.address, h.root_port, h.ports)).collect();
        for (address, root_port, ports) in hubs {
            for p in 1..=ports {
                let Some((status, change)) = self.hub_status(address, p) else {
                    continue;
                };
                if change & HUB_C_CONNECTION != 0 {
                    self.hub_request(address, 1, FEATURE_C_PORT_CONNECTION, p);
                }
                let connected = status & HUB_PORT_CONNECTION != 0;
                let known = self.hubs.iter().find(|h| h.address == address).map(|h| h.connected[p as usize]).unwrap_or(connected);
                if connected != known || (connected && change & HUB_C_CONNECTION != 0) {
                    changes.push((address, root_port, p, connected));
                }
            }
        }
        changes
    }

    fn enumerate(&mut self, port: u8) -> Option<UsbDevice> {
        self.enumerate_at(port, HIGH_SPEED, 0, 0)
    }

    fn enumerate_at(&mut self, port: u8, route: Route, hub: u8, hub_port: u8) -> Option<UsbDevice> {
        self.routes[0] = route;
        let head = (0..3).find_map(|_| {
            let d = self.get_descriptor(0, 64, 1, 8);
            if d.as_ref().map(|d| d.len() >= 8).unwrap_or(false) { d } else { delay_ms(20); None }
        })?;
        let max_packet = head[7].max(8) as u32;
        let address = self.next_address;
        self.next_address = if self.next_address >= 127 { 1 } else { self.next_address + 1 };
        self.control(0, max_packet, [0x00, 5, address, 0, 0, 0, 0, 0])?;
        self.routes[address as usize] = route;
        delay_ms(10);
        let device = self.get_descriptor(address, max_packet, 1, 18)?;
        if device.len() < 18 {
            return None;
        }
        let header = self.get_descriptor(address, max_packet, 2, 9)?;
        if header.len() < 9 {
            return None;
        }
        let total = u16::from_le_bytes([header[2], header[3]]).min(CONTROL_DATA as u16);
        let configuration_value = header[5];
        let configuration = self.get_descriptor(address, max_packet, 2, total)?;
        let interfaces: Vec<InterfaceInfo> = parse_configuration(&configuration);
        let mut usb = UsbDevice {
            address,
            port,
            vendor: u16::from_le_bytes([device[8], device[9]]),
            product: u16::from_le_bytes([device[10], device[11]]),
            class: match device[4] {
                9 => DeviceClass::Hub,
                8 => DeviceClass::MassStorage,
                _ => DeviceClass::Other,
            },
            interface: 0,
            endpoint: 0,
            max_packet: max_packet as u16,
            low_speed: route.speed == 1,
            controller: self.base as u64,
        };
        if device[4] == 9 {
            if route.speed == 2 && self.control(address, max_packet, [0x00, 9, configuration_value, 0, 0, 0, 0, 0]).is_some() {
                self.setup_hub(address, max_packet, port);
            } else {
                crate::drivers::klog::log(&format!("usb: hub at address {} is not high speed; its ports are not driven", address));
            }
            return Some(usb);
        }
        if let Some(storage) = interfaces.iter().find(|i| i.class == 8 && i.protocol == 0x50 && i.bulk_in != 0 && i.bulk_out != 0) {
            usb.class = DeviceClass::MassStorage;
            self.control(address, max_packet, [0x00, 9, configuration_value, 0, 0, 0, 0, 0])?;
            delay_ms(5);
            let mut dev = Storage {
                base: self.base,
                address,
                port,
                hub,
                hub_port,
                interface: storage.number,
                in_ep: storage.bulk_in,
                out_ep: storage.bulk_out,
                in_packet: (storage.bulk_in_packet as u32).clamp(64, 1024),
                out_packet: (storage.bulk_out_packet as u32).clamp(64, 1024),
                in_toggle: 0,
                out_toggle: 0,
                tag: 1,
                block_size: 512,
                blocks: 0,
                model: String::new(),
                alive: true,
                disk: None,
            };
            if self.storage_init(&mut dev) {
                self.found.push(dev);
            }
            return Some(usb);
        }
        let hid: Vec<&InterfaceInfo> = interfaces.iter().filter(|i| i.class == 3 && i.endpoint != 0).collect();
        if hid.is_empty() {
            if interfaces.iter().any(|i| i.class == 3) {
                usb.class = DeviceClass::HidOther;
            }
            return Some(usb);
        }
        self.control(address, max_packet, [0x00, 9, configuration_value, 0, 0, 0, 0, 0])?;
        delay_ms(5);
        for interface in hid {
            let protocol = if interface.subclass == 1 { interface.protocol } else { 0 };
            let number = interface.number;
            if interface.subclass == 1 {
                self.control(address, max_packet, [0x21, REQUEST_SET_PROTOCOL, 0, 0, number, 0, 0, 0]);
            }
            self.control(address, max_packet, [0x21, REQUEST_SET_IDLE, 0, 0, number, 0, 0, 0]);
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
                self.add_interrupt(address, interface.endpoint, interface.packet_size as usize, class == DeviceClass::HidKeyboard, port, hub, hub_port);
                if class == DeviceClass::HidMouse {
                    crate::drivers::input::mouse::mark_present();
                }
            }
        }
        Some(usb)
    }

    fn interrupt_qh(&self, slot: usize) -> usize {
        self.region + INTERRUPT_OFFSET + slot * INTERRUPT_STRIDE
    }

    fn interrupt_td(&self, slot: usize) -> usize {
        self.interrupt_qh(slot) + 64
    }

    fn interrupt_buffer(&self, slot: usize) -> usize {
        self.interrupt_qh(slot) + 128
    }

    fn relink_interrupts(&self) {
        let mut slots: Vec<usize> = self.interrupts.iter().map(|i| i.slot).collect();
        slots.sort_unstable();
        for &slot in &slots {
            wr32(self.interrupt_qh(slot), LINK_TERMINATE);
        }
        for entry in 0..1024 {
            let link = if slots.is_empty() { LINK_TERMINATE } else { self.interrupt_qh(slots[entry % slots.len()]) as u32 | LINK_QH };
            wr32(self.region + PERIODIC_OFFSET + entry * 4, link);
        }
    }

    fn add_interrupt(&mut self, address: u8, endpoint: u8, packet: usize, keyboard: bool, port: u8, hub: u8, hub_port: u8) {
        let used: Vec<usize> = self.interrupts.iter().map(|i| i.slot).collect();
        let Some(slot) = (0..MAX_INTERRUPTS).find(|s| !used.contains(s)) else {
            return;
        };
        let packet = packet & 0x7FF;
        let length = if keyboard { packet.clamp(8, 64) } else { packet.clamp(3, 64) };
        let route = self.routes[(address & 0x7f) as usize];
        let qh = self.interrupt_qh(slot);
        for i in 0..16 {
            wr32(qh + i * 4, 0);
        }
        wr32(qh, LINK_TERMINATE);
        wr32(qh + 4, address as u32 | ((endpoint as u32) << 8) | (route.speed << 12) | ((packet.clamp(1, 1024) as u32) << 16));
        let masks = if route.speed == 2 { 0x01 } else { 0x01 | (0x1C << 8) };
        wr32(qh + 8, (1 << 30) | ((route.hub as u32) << 16) | ((route.port as u32) << 23) | masks);
        wr32(qh + 16, LINK_TERMINATE);
        wr32(qh + 20, LINK_TERMINATE);
        self.interrupts.push(Interrupt { slot, address, endpoint, errors: 0, last_error: 0, reports: 0, reported: false, port, hub, hub_port, length, keyboard, previous: [0; 8] });
        self.arm_interrupt(self.interrupts.len() - 1, false);
        self.relink_interrupts();
        let speed = match route.speed {
            2 => "high speed",
            1 => "low speed",
            _ => "full speed",
        };
        crate::drivers::klog::log(&format!(
            "usb: EHCI {:#x} polls {} at address {} endpoint {} ({}, {} bytes, hub {} port {})",
            self.base,
            if keyboard { "keyboard" } else { "mouse" },
            address,
            endpoint,
            speed,
            length,
            route.hub,
            route.port
        ));
    }

    fn report_interrupt_errors(&mut self) {
        let base = self.base;
        for interrupt in self.interrupts.iter_mut() {
            if !interrupt.reported && interrupt.errors >= 50 {
                interrupt.reported = true;
                crate::drivers::klog::log(&format!(
                    "usb: EHCI {:#x} device {} endpoint {}: {} failed polls, {} reports, last token {:#x}",
                    base, interrupt.address, interrupt.endpoint, interrupt.errors, interrupt.reports, interrupt.last_error
                ));
            }
        }
    }

    fn arm_interrupt(&self, index: usize, reset_toggle: bool) {
        let interrupt = &self.interrupts[index];
        let qh = self.interrupt_qh(interrupt.slot);
        let td = self.interrupt_td(interrupt.slot);
        let buffer = self.interrupt_buffer(interrupt.slot);
        unsafe { core::ptr::write_bytes(buffer as *mut u8, 0, 64) };
        wr32(td, LINK_TERMINATE);
        wr32(td + 4, LINK_TERMINATE);
        wr32(td + 8, TOKEN_ACTIVE | TOKEN_CERR | (PID_IN << 8) | ((interrupt.length as u32) << 16));
        wr32(td + 12, buffer as u32);
        for page in 1..5 {
            wr32(td + 12 + page * 4, 0);
        }
        let toggle = if reset_toggle { 0 } else { rd32(qh + 24) & TOKEN_TOGGLE };
        wr32(qh + 24, toggle);
        wr32(qh + 16, td as u32);
    }

    fn drop_interrupts(&mut self, matches: impl Fn(&Interrupt) -> bool) {
        let before = self.interrupts.len();
        self.interrupts.retain(|i| !matches(i));
        if self.interrupts.len() != before {
            self.relink_interrupts();
            delay_ms(2);
        }
    }

    fn service(&mut self) {
        for index in 0..self.interrupts.len() {
            let slot = self.interrupts[index].slot;
            let token = rd32(self.interrupt_td(slot) + 8);
            if token & TOKEN_ACTIVE != 0 {
                continue;
            }
            let mut reset_toggle = false;
            if token & TOKEN_ERRORS == 0 {
                let remaining = ((token >> 16) & 0x7FFF) as usize;
                let n = self.interrupts[index].length.saturating_sub(remaining).min(8);
                self.interrupts[index].reports = self.interrupts[index].reports.saturating_add(1);
                let mut report = [0u8; 8];
                unsafe { core::ptr::copy_nonoverlapping(self.interrupt_buffer(slot) as *const u8, report.as_mut_ptr(), n) };
                if n > 0 {
                    if self.interrupts[index].keyboard {
                        let mut previous = self.interrupts[index].previous;
                        hid::on_boot_keyboard_report(&report[..n], &mut previous);
                        self.interrupts[index].previous = previous;
                    } else {
                        hid::on_boot_mouse_report(&report[..n]);
                    }
                }
            } else {
                self.interrupts[index].errors = self.interrupts[index].errors.saturating_add(1);
                self.interrupts[index].last_error = token;
                reset_toggle = token & TOKEN_HALTED != 0 && token & ((1 << 5) | (1 << 4) | (1 << 3)) == 0;
            }
            self.arm_interrupt(index, reset_toggle);
        }
    }

    fn check_ports(&mut self) -> Vec<(u8, bool)> {
        let mut changes = Vec::new();
        for port in 0..self.ports {
            let status = self.port_read(port);
            let connected = status & PORT_CONNECTED != 0;
            if status & PORT_OWNER != 0 {
                continue;
            }
            if status & PORT_CONNECT_CHANGE != 0 {
                self.port_write(port, (status & !PORT_CLEAR_BITS) | PORT_CONNECT_CHANGE);
            }
            if connected != self.connected[port as usize] {
                changes.push((port, connected));
            }
        }
        changes
    }
}

fn take_from_bios(device: &PciDevice, eecp: u8, base: usize) {
    let legacy = pci::read_config_u32(device.address, eecp);
    if legacy & (1 << 16) != 0 {
        let control = pci::read_config_u32(device.address, eecp + 4);
        pci::write_config_u32(device.address, eecp + 4, control | (1 << 13));
        pci::write_config_u16(device.address, eecp + 2, 0x0101);
        let mut waited = 0;
        while waited < 1000 && pci::read_config_u32(device.address, eecp) & (1 << 16) != 0 {
            delay_ms(1);
            waited += 1;
        }
        if pci::read_config_u32(device.address, eecp) & (1 << 16) != 0 {
            crate::drivers::klog::log(&format!("ehci: the BIOS did not release the controller at {:#x}; taking it anyway", base));
            pci::write_config_u16(device.address, eecp + 2, 0x0100);
        } else {
            crate::drivers::klog::log(&format!("ehci: the BIOS released the controller at {:#x} after {} ms", base, waited));
        }
    } else {
        pci::write_config_u16(device.address, eecp + 2, 0x0100);
    }
    let control = pci::read_config_u32(device.address, eecp + 4);
    pci::write_config_u32(device.address, eecp + 4, control & 0xffff_0000);
}

pub fn attach(device: &PciDevice, controller: &mut Controller) -> bool {
    let Some(base) = device.mmio_bar(0) else {
        return false;
    };
    if base == 0 || base >= 1 << 32 {
        return false;
    }
    let base = base as usize;
    let Some(region) = frame::alloc_contiguous(REGION_PAGES, 1 << 32) else {
        controller.note = String::from("out of memory");
        return false;
    };
    unsafe { core::ptr::write_bytes(region as *mut u8, 0, REGION_PAGES * 4096) };
    let op = base + (rd32(base) & 0xFF) as usize;
    let params = rd32(base + 0x04);
    let ports = ((params & 0xF) as u8).min(16);
    let hcc = rd32(base + 0x08);
    let mut eecp = ((hcc >> 8) & 0xff) as u8;
    for _ in 0..8 {
        if eecp < 0x40 {
            break;
        }
        let capability = pci::read_config_u32(device.address, eecp);
        if capability & 0xff == 1 {
            take_from_bios(device, eecp, base);
            break;
        }
        eecp = ((capability >> 8) & 0xff) as u8;
    }
    let ehci = Ehci { base, op, ports, region, connected: [false; 16], next_address: 1, found: Vec::new(), routes: [HIGH_SPEED; 128], hubs: Vec::new(), interrupts: Vec::new() };
    ehci.op_write(USBCMD, ehci.op_read(USBCMD) & !CMD_RUN);
    for _ in 0..50 {
        if ehci.op_read(USBSTS) & STS_HALTED != 0 {
            break;
        }
        delay_ms(1);
    }
    ehci.op_write(USBCMD, CMD_RESET);
    for _ in 0..100 {
        if ehci.op_read(USBCMD) & CMD_RESET == 0 {
            break;
        }
        delay_ms(1);
    }
    if rd32(base + 0x08) & 1 != 0 {
        ehci.op_write(CTRLDSSEGMENT, 0);
    }
    ehci.op_write(USBINTR, 0);
    ehci.op_write(USBSTS, 0x3F);
    for entry in 0..1024 {
        wr32(region + PERIODIC_OFFSET + entry * 4, LINK_TERMINATE);
    }
    ehci.op_write(PERIODICLISTBASE, (region + PERIODIC_OFFSET) as u32);
    ehci.op_write(USBCMD, (8 << 16) | CMD_RUN | CMD_PERIODIC);
    for _ in 0..50 {
        if ehci.op_read(USBSTS) & STS_HALTED == 0 && ehci.op_read(USBSTS) & STS_PERIODIC != 0 {
            break;
        }
        delay_ms(1);
    }
    ehci.op_write(CONFIGFLAG, 1);
    delay_ms(5);
    if params & (1 << 4) != 0 {
        for port in 0..ports {
            let status = ehci.port_read(port);
            ehci.port_write(port, (status & !PORT_CLEAR_BITS) | PORT_POWER);
        }
    }
    delay_ms(30);
    pci::enable_bus_master(device.address);
    controller.base = base as u64;
    controller.ports = ports;
    controller.driven = true;
    controller.note = format!("{} port(s), high-speed devices driven, others handed to the companion controllers", ports);
    CONTROLLERS.lock().push(ehci);
    true
}

fn log_device(device: &UsbDevice) {
    crate::drivers::klog::log(&format!(
        "usb: {:04x}:{:04x} on EHCI {:#x} port {} -- {} (high speed)",
        device.vendor, device.product, device.controller, device.port, device.class.name()
    ));
}

fn register_storage(found: Vec<Storage>) {
    for dev in found {
        let sectors = dev.blocks * dev.block_size / 512;
        let model = if dev.model.is_empty() { String::from("USB storage") } else { dev.model.clone() };
        let slot = {
            let mut list = STORAGE.lock();
            list.push(dev);
            list.len() - 1
        };
        let disk = crate::drivers::block::register_ehci_disk(&model, sectors, slot);
        if let Some(d) = STORAGE.lock().get_mut(slot) {
            d.disk = Some(disk);
        }
    }
}

fn forget(base: usize, matches: impl Fn(&Storage) -> bool) {
    let gone: Vec<usize> = STORAGE
        .lock()
        .iter_mut()
        .filter(|d| d.alive && d.base == base && matches(d))
        .filter_map(|d| {
            d.alive = false;
            d.disk
        })
        .collect();
    for disk in gone {
        let paths = crate::fs::media::forget_disk(disk);
        crate::drivers::block::detach(disk);
        crate::drivers::klog::log(&format!("usb-storage: disk {} removed{}", disk, if paths.is_empty() { String::new() } else { format!(", unmounted {}", paths.join(", ")) }));
    }
}

fn scan_root_ports(index: usize) -> bool {
    let changes = match CONTROLLERS.lock().get_mut(index).map(|c| c.check_ports()) {
        Some(c) => c,
        None => return false,
    };
    let mut changed = false;
    for (port, connected) in changes {
        changed = true;
        let base = {
            let mut guard = CONTROLLERS.lock();
            guard[index].connected[port as usize] = connected;
            guard[index].hubs.retain(|h| h.root_port != port);
            guard[index].drop_interrupts(|i| i.port == port);
            guard[index].base
        };
        DEVICES.lock().retain(|d| !(d.controller == base as u64 && d.port == port));
        forget(base, |d| d.port == port);
        if !connected {
            continue;
        }
        delay_ms(100);
        let mut guard = CONTROLLERS.lock();
        let controller = &mut guard[index];
        if controller.port_read(port) & PORT_LINE_STATUS == PORT_LINE_K || !controller.reset_port(port) {
            controller.hand_over(port);
            controller.connected[port as usize] = false;
            continue;
        }
        let result = controller.enumerate(port);
        let found: Vec<Storage> = controller.found.drain(..).collect();
        let keep = result.as_ref().map(|d| matches!(d.class, DeviceClass::Hub | DeviceClass::HidMouse | DeviceClass::HidKeyboard)).unwrap_or(false);
        if found.is_empty() && !keep {
            controller.hand_over(port);
            controller.connected[port as usize] = false;
            drop(guard);
            if let Some(device) = result {
                crate::drivers::klog::log(&format!("usb: {:04x}:{:04x} on EHCI {:#x} port {} is not a storage device, handed to the companion controller", device.vendor, device.product, base, port));
            }
            continue;
        }
        drop(guard);
        register_storage(found);
        match result {
            Some(device) => {
                log_device(&device);
                DEVICES.lock().push(device);
            }
            None => crate::drivers::klog::log(&format!("usb: EHCI {:#x} port {}: device did not enumerate", base, port)),
        }
    }
    changed
}

fn scan_hub_ports(index: usize) -> bool {
    let changes = match CONTROLLERS.lock().get_mut(index).map(|c| c.hub_changes()) {
        Some(c) => c,
        None => return false,
    };
    let mut changed = false;
    for (hub, root_port, hub_port, connected) in changes {
        changed = true;
        let (base, child) = {
            let mut guard = CONTROLLERS.lock();
            let controller = &mut guard[index];
            let mut child = 0;
            if let Some(h) = controller.hubs.iter_mut().find(|h| h.address == hub) {
                h.connected[hub_port as usize] = connected;
                child = core::mem::take(&mut h.children[hub_port as usize]);
            }
            controller.drop_interrupts(|i| (i.hub == hub && i.hub_port == hub_port) || (child != 0 && i.hub == child));
            (controller.base, child)
        };
        if child != 0 {
            DEVICES.lock().retain(|d| !(d.controller == base as u64 && d.address == child));
            let mut guard = CONTROLLERS.lock();
            let removed: Vec<u8> = guard[index].hubs.iter().filter(|h| h.address == child).map(|h| h.address).collect();
            guard[index].hubs.retain(|h| !removed.contains(&h.address));
        }
        forget(base, |d| d.hub == hub && d.hub_port == hub_port);
        if !connected {
            continue;
        }
        delay_ms(100);
        let mut guard = CONTROLLERS.lock();
        let controller = &mut guard[index];
        let Some(speed) = controller.hub_reset(hub, hub_port) else {
            drop(guard);
            crate::drivers::klog::log(&format!("usb: EHCI {:#x} hub {} port {}: the port did not come up after a reset", base, hub, hub_port));
            continue;
        };
        let route = if speed == 2 { HIGH_SPEED } else { Route { speed, hub, port: hub_port } };
        let result = controller.enumerate_at(root_port, route, hub, hub_port);
        if let Some(device) = &result {
            if let Some(h) = controller.hubs.iter_mut().find(|h| h.address == hub) {
                h.children[hub_port as usize] = device.address;
            }
        }
        let found: Vec<Storage> = controller.found.drain(..).collect();
        drop(guard);
        register_storage(found);
        match result {
            Some(device) => {
                crate::drivers::klog::log(&format!(
                    "usb: {:04x}:{:04x} on EHCI {:#x} port {} hub {} port {} -- {} ({})",
                    device.vendor,
                    device.product,
                    base,
                    root_port,
                    hub,
                    hub_port,
                    device.class.name(),
                    match speed {
                        2 => "high speed",
                        1 => "low speed",
                        _ => "full speed",
                    }
                ));
                DEVICES.lock().push(device);
            }
            None => crate::drivers::klog::log(&format!("usb: EHCI {:#x} hub {} port {}: device did not enumerate", base, hub, hub_port)),
        }
    }
    changed
}

pub fn scan() -> bool {
    let count = CONTROLLERS.lock().len();
    let mut changed = false;
    for index in 0..count {
        changed |= scan_root_ports(index);
        changed |= scan_hub_ports(index);
        if let Some(controller) = CONTROLLERS.lock().get_mut(index) {
            controller.report_interrupt_errors();
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

pub fn storage_io(slot: usize, lba: u64, buf: &mut [u8], write: bool) -> Result<(), ()> {
    let mut list = STORAGE.lock();
    let dev = list.get_mut(slot).filter(|d| d.alive).ok_or(())?;
    let mut guard = CONTROLLERS.lock();
    let controller = guard.iter_mut().find(|c| c.base == dev.base).ok_or(())?;
    let size = dev.block_size;
    let start = lba * 512;
    let end = start + buf.len() as u64;
    let first = start / size;
    let last = end.div_ceil(size);
    let count = (last - first) as usize;
    if last > dev.blocks {
        return Err(());
    }
    let skip = (start - first * size) as usize;
    if skip == 0 && buf.len() as u64 % size == 0 {
        return controller.storage_io(dev, first, count, buf, write);
    }
    let mut staging = alloc::vec![0u8; count * size as usize];
    controller.storage_io(dev, first, count, &mut staging, false)?;
    if write {
        staging[skip..skip + buf.len()].copy_from_slice(buf);
        controller.storage_io(dev, first, count, &mut staging, true)
    } else {
        buf.copy_from_slice(&staging[skip..skip + buf.len()]);
        Ok(())
    }
}
