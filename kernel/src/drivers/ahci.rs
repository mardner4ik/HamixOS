use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::drivers::pci::{self, PciDevice};
use crate::memory::frame;

const HBA_CAP: usize = 0x00;
const HBA_GHC: usize = 0x04;
const HBA_IS: usize = 0x08;
const HBA_PI: usize = 0x0C;
const HBA_VS: usize = 0x10;
const HBA_CAP2: usize = 0x24;
const HBA_BOHC: usize = 0x28;

const GHC_AE: u32 = 1 << 31;
const GHC_HR: u32 = 1 << 0;
const GHC_IE: u32 = 1 << 1;

const CAP_S64A: u32 = 1 << 31;
const CAP_SSS: u32 = 1 << 27;

const PX_CLB: usize = 0x00;
const PX_CLBU: usize = 0x04;
const PX_FB: usize = 0x08;
const PX_FBU: usize = 0x0C;
const PX_IS: usize = 0x10;
const PX_IE: usize = 0x14;
const PX_CMD: usize = 0x18;
const PX_TFD: usize = 0x20;
const PX_SIG: usize = 0x24;
const PX_SSTS: usize = 0x28;
const PX_SCTL: usize = 0x2C;
const PX_SERR: usize = 0x30;
const PX_CI: usize = 0x38;

const CMD_ST: u32 = 1 << 0;
const CMD_SUD: u32 = 1 << 1;
const CMD_POD: u32 = 1 << 2;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const CMD_ICC_ACTIVE: u32 = 1 << 28;

const TFD_ERR: u32 = 1 << 0;
const TFD_DRQ: u32 = 1 << 3;
const TFD_BSY: u32 = 1 << 7;

const IS_TFES: u32 = 1 << 30;
const IS_FATAL: u32 = (1 << 30) | (1 << 29) | (1 << 28) | (1 << 27) | (1 << 24);

const SIG_ATA: u32 = 0x0000_0101;
const SIG_ATAPI: u32 = 0xEB14_0101;

const ATA_IDENTIFY: u8 = 0xEC;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const ATA_READ_DMA: u8 = 0xC8;
const ATA_WRITE_DMA: u8 = 0xCA;
const ATA_FLUSH_EXT: u8 = 0xEA;
const ATA_FLUSH: u8 = 0xE7;

const PRD_MAX: usize = 248;
const PRD_CHUNK: usize = 0x10000;
pub const MAX_SECTORS_PER_COMMAND: usize = 8192;

#[derive(Clone)]
pub struct PortInfo {
    pub controller: usize,
    pub port: usize,
    pub sectors: u64,
    pub lba48: bool,
    pub model: String,
    pub serial: String,
}

struct Port {
    base: usize,
    command_list: usize,
    command_table: usize,
    lba48: bool,
}

struct Controller {
    abar: usize,
    ports: Vec<Option<Port>>,
}

static CONTROLLERS: Mutex<Vec<Controller>> = Mutex::new(Vec::new());
static PORTS: Mutex<Vec<PortInfo>> = Mutex::new(Vec::new());
static IO_LOCK: Mutex<()> = Mutex::new(());

pub fn lock_io() -> spin::MutexGuard<'static, ()> {
    loop {
        if let Some(guard) = IO_LOCK.try_lock() {
            return guard;
        }
        if crate::arch::interrupts_enabled() && crate::task::current_pid() != 0 {
            crate::task::yield_now();
        } else {
            core::hint::spin_loop();
        }
    }
}

fn read(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn write(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn delay_ms(ms: u64) {
    crate::arch::delay_ms(ms);
}

fn wait_until(timeout_ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let mut waited = 0u64;
    loop {
        if done() {
            return true;
        }
        if waited >= timeout_ms {
            return false;
        }
        let step = if waited < 20 { 1 } else { 10 };
        delay_ms(step);
        waited += step;
    }
}

fn alloc_page() -> Option<usize> {
    frame::alloc_zeroed_frame()
}

fn stop_port(port: usize) -> bool {
    let cmd = read(port, PX_CMD);
    write(port, PX_CMD, cmd & !CMD_ST);
    let stopped = wait_until(500, || read(port, PX_CMD) & CMD_CR == 0);
    let cmd = read(port, PX_CMD);
    write(port, PX_CMD, cmd & !CMD_FRE);
    stopped && wait_until(500, || read(port, PX_CMD) & CMD_FR == 0)
}

fn start_port(port: usize) {
    wait_until(500, || read(port, PX_CMD) & CMD_CR == 0);
    let cmd = read(port, PX_CMD);
    write(port, PX_CMD, cmd | CMD_FRE);
    let cmd = read(port, PX_CMD);
    write(port, PX_CMD, cmd | CMD_ST);
}

fn link_up(port: usize) -> bool {
    let ssts = read(port, PX_SSTS);
    ssts & 0xF == 3 && (ssts >> 8) & 0xF == 1
}

fn comreset(port: usize) {
    let sctl = read(port, PX_SCTL);
    write(port, PX_SCTL, (sctl & !0xF) | 1);
    delay_ms(20);
    let sctl = read(port, PX_SCTL);
    write(port, PX_SCTL, sctl & !0xF);
    wait_until(1000, || read(port, PX_SSTS) & 0xF == 3);
    write(port, PX_SERR, 0xFFFF_FFFF);
}

fn fill_fis(table: usize, command: u8, lba: u64, count: u16, lba_mode: bool) {
    let fis = table as *mut u8;
    unsafe {
        core::ptr::write_bytes(fis, 0, 0x80);
        *fis.add(0) = 0x27;
        *fis.add(1) = 0x80;
        *fis.add(2) = command;
        *fis.add(3) = 0;
        *fis.add(4) = lba as u8;
        *fis.add(5) = (lba >> 8) as u8;
        *fis.add(6) = (lba >> 16) as u8;
        *fis.add(7) = if lba_mode { 0x40 | if command == ATA_READ_DMA || command == ATA_WRITE_DMA { ((lba >> 24) & 0xF) as u8 } else { 0 } } else { 0 };
        *fis.add(8) = (lba >> 24) as u8;
        *fis.add(9) = (lba >> 32) as u8;
        *fis.add(10) = (lba >> 40) as u8;
        *fis.add(12) = count as u8;
        *fis.add(13) = (count >> 8) as u8;
    }
}

fn execute(p: &Port, command: u8, lba: u64, count: u16, buffer: *mut u8, bytes: usize, write_data: bool) -> Result<(), ()> {
    let port = p.base;
    if !wait_until(30_000, || read(port, PX_TFD) & (TFD_BSY | TFD_DRQ) == 0) {
        recover(p);
        return Err(());
    }
    fill_fis(p.command_table, command, lba, count, command != ATA_IDENTIFY);

    let mut chunks = 0usize;
    let mut offset = 0usize;
    while offset < bytes {
        let (address, run) = crate::memory::vmalloc::contiguous_run(buffer as usize + offset, bytes - offset);
        if run == 0 || chunks == PRD_MAX {
            return Err(());
        }
        let length = run.min(PRD_CHUNK);
        unsafe {
            let entry = (p.command_table + 0x80 + chunks * 16) as *mut u32;
            *entry.add(0) = address as u32;
            *entry.add(1) = (address as u64 >> 32) as u32;
            *entry.add(2) = 0;
            *entry.add(3) = (length - 1) as u32;
        }
        chunks += 1;
        offset += length;
    }
    unsafe {
        let header = p.command_list as *mut u32;
        let flags = 5u32 | if write_data { 1 << 6 } else { 0 } | ((chunks as u32) << 16);
        *header.add(0) = flags;
        *header.add(1) = 0;
        *header.add(2) = p.command_table as u32;
        *header.add(3) = (p.command_table as u64 >> 32) as u32;
    }

    write(port, PX_IS, 0xFFFF_FFFF);
    write(port, PX_CI, 1);

    let timeout = if command == ATA_FLUSH_EXT || command == ATA_FLUSH { 60_000 } else { 30_000 };
    let mut spins = 0u64;
    let started = crate::task::ticks();
    loop {
        let ci = read(port, PX_CI);
        let is = read(port, PX_IS);
        if is & IS_FATAL != 0 || (read(port, PX_TFD) & TFD_ERR != 0 && ci & 1 == 0) {
            recover(p);
            return Err(());
        }
        if ci & 1 == 0 {
            break;
        }
        spins += 1;
        if spins < 2000 {
            core::hint::spin_loop();
            continue;
        }
        let scheduled = crate::arch::interrupts_enabled() && crate::task::current_pid() != 0;
        let elapsed_ms = if scheduled { (crate::task::ticks() - started) * 1000 / crate::task::TICK_HZ } else { (spins - 2000) / 10 };
        if elapsed_ms > timeout {
            recover(p);
            return Err(());
        }
        if scheduled {
            crate::task::yield_now();
        } else {
            crate::arch::delay_us(100);
        }
    }
    if read(port, PX_IS) & IS_TFES != 0 || read(port, PX_TFD) & TFD_ERR != 0 {
        recover(p);
        return Err(());
    }
    let _ = HBA_IS;
    Ok(())
}

fn recover(p: &Port) {
    let port = p.base;
    stop_port(port);
    write(port, PX_SERR, 0xFFFF_FFFF);
    write(port, PX_IS, 0xFFFF_FFFF);
    if read(port, PX_TFD) & (TFD_BSY | TFD_DRQ) != 0 {
        comreset(port);
    }
    start_port(port);
}

fn ata_string(words: &[u16]) -> String {
    let mut out = String::new();
    for word in words {
        for byte in [(word >> 8) as u8, *word as u8] {
            if byte.is_ascii_graphic() || byte == b' ' {
                out.push(byte as char);
            }
        }
    }
    String::from(out.trim())
}

fn setup_port(abar: usize, index: usize, staggered: bool) -> Option<(Port, [u16; 256])> {
    let port = abar + 0x100 + index * 0x80;
    if !stop_port(port) {
        comreset(port);
        if !stop_port(port) {
            return None;
        }
    }

    let command_list = alloc_page()?;
    let fis = alloc_page()?;
    let command_table = alloc_page()?;
    write(port, PX_CLB, command_list as u32);
    write(port, PX_CLBU, (command_list as u64 >> 32) as u32);
    write(port, PX_FB, fis as u32);
    write(port, PX_FBU, (fis as u64 >> 32) as u32);
    write(port, PX_SERR, 0xFFFF_FFFF);
    write(port, PX_IS, 0xFFFF_FFFF);
    write(port, PX_IE, 0);

    let cmd = read(port, PX_CMD);
    write(port, PX_CMD, cmd | CMD_FRE);
    if staggered {
        let cmd = read(port, PX_CMD);
        write(port, PX_CMD, cmd | CMD_SUD | CMD_POD);
    }
    let cmd = read(port, PX_CMD);
    write(port, PX_CMD, (cmd & !(0xF << 28)) | CMD_ICC_ACTIVE);

    if !wait_until(100, || link_up(port)) {
        if read(port, PX_SSTS) & 0xF == 0 {
            frame::free_frame(command_list);
            frame::free_frame(fis);
            frame::free_frame(command_table);
            return None;
        }
        comreset(port);
        if !wait_until(1000, || link_up(port)) {
            frame::free_frame(command_list);
            frame::free_frame(fis);
            frame::free_frame(command_table);
            return None;
        }
    }
    write(port, PX_SERR, 0xFFFF_FFFF);
    wait_until(10_000, || read(port, PX_TFD) & (TFD_BSY | TFD_DRQ) == 0);

    let signature = read(port, PX_SIG);
    if signature == SIG_ATAPI || (signature != SIG_ATA && signature != 0xFFFF_FFFF) {
        stop_port(port);
        frame::free_frame(command_list);
        frame::free_frame(fis);
        frame::free_frame(command_table);
        return None;
    }

    start_port(port);
    let p = Port { base: port, command_list, command_table, lba48: true };
    let mut words = [0u16; 256];
    let buffer = alloc_page()?;
    let result = execute(&p, ATA_IDENTIFY, 0, 0, buffer as *mut u8, 512, false);
    if result.is_ok() {
        for (i, word) in words.iter_mut().enumerate() {
            *word = unsafe { core::ptr::read_unaligned((buffer + i * 2) as *const u16) };
        }
    }
    frame::free_frame(buffer);
    result.ok()?;
    Some((p, words))
}

fn attach(device: &PciDevice, controllers: &mut Vec<Controller>, ports_out: &mut Vec<PortInfo>) {
    let Some(abar) = device.mmio_bar(5) else {
        return;
    };
    if abar == 0 || abar >= 1 << 32 {
        crate::drivers::klog::log(&format!("ahci: {:04x}:{:04x} register window {:#x} is not reachable", device.vendor, device.device, abar));
        return;
    }
    let abar = abar as usize;
    let command = pci::read_config_u16(device.address, 0x04);
    pci::write_config_u16(device.address, 0x04, (command | 0x0006) & !0x0400);

    let cap2 = read(abar, HBA_CAP2);
    if cap2 & 1 != 0 {
        let bohc = read(abar, HBA_BOHC);
        write(abar, HBA_BOHC, bohc | 2);
        wait_until(25, || read(abar, HBA_BOHC) & 1 == 0);
        if read(abar, HBA_BOHC) & 0x10 != 0 {
            wait_until(2000, || read(abar, HBA_BOHC) & 1 == 0);
        }
    }

    write(abar, HBA_GHC, read(abar, HBA_GHC) | GHC_AE);
    let implemented = read(abar, HBA_PI);
    let cap = read(abar, HBA_CAP);
    let saved_pi = implemented;
    write(abar, HBA_GHC, read(abar, HBA_GHC) | GHC_HR);
    wait_until(1000, || read(abar, HBA_GHC) & GHC_HR == 0);
    write(abar, HBA_GHC, (read(abar, HBA_GHC) | GHC_AE) & !GHC_IE);
    if read(abar, HBA_PI) == 0 {
        write(abar, HBA_PI, saved_pi);
    }
    let implemented = read(abar, HBA_PI);
    let version = read(abar, HBA_VS);
    let staggered = cap & CAP_SSS != 0;
    let _ = CAP_S64A;

    let controller_index = controllers.len();
    let mut ports: Vec<Option<Port>> = Vec::new();
    let mut found = 0;
    for index in 0..32usize {
        if implemented & (1 << index) == 0 {
            ports.push(None);
            continue;
        }
        match setup_port(abar, index, staggered) {
            Some((mut port, words)) => {
                let lba48 = words[83] & (1 << 10) != 0;
                let sectors = if lba48 {
                    words[100] as u64 | (words[101] as u64) << 16 | (words[102] as u64) << 32 | (words[103] as u64) << 48
                } else {
                    words[60] as u64 | (words[61] as u64) << 16
                };
                port.lba48 = lba48;
                if sectors == 0 {
                    ports.push(None);
                    continue;
                }
                let info = PortInfo {
                    controller: controller_index,
                    port: index,
                    sectors,
                    lba48,
                    model: ata_string(&words[27..47]),
                    serial: ata_string(&words[10..20]),
                };
                crate::drivers::klog::log(&format!(
                    "ahci: port {} -- {} MiB {}",
                    index,
                    sectors / 2048,
                    info.model
                ));
                ports_out.push(info);
                ports.push(Some(port));
                found += 1;
            }
            None => ports.push(None),
        }
    }
    crate::drivers::klog::log(&format!(
        "ahci: {:04x}:{:04x} AHCI {}.{} at {:#x}, {} port(s), {} disk(s)",
        device.vendor,
        device.device,
        version >> 16,
        (version >> 8) & 0xFF,
        abar,
        implemented.count_ones(),
        found
    ));
    controllers.push(Controller { abar, ports });
}

pub fn init() {
    let mut controllers = Vec::new();
    let mut ports = Vec::new();
    for device in pci::enumerate() {
        let ahci = device.class == 0x01 && device.subclass == 0x06 && device.prog_if == 0x01;
        let intel_raid = device.class == 0x01 && device.subclass == 0x04 && device.vendor == 0x8086;
        if ahci || intel_raid {
            if let Some(owner) = crate::module::kpi::claimed(device.address) {
                crate::drivers::klog::log(&format!("ahci: {:04x}:{:04x} is driven by module {}", device.vendor, device.device, owner));
                continue;
            }
            attach(&device, &mut controllers, &mut ports);
        }
    }
    *CONTROLLERS.lock() = controllers;
    *PORTS.lock() = ports;
}

pub fn ports() -> Vec<PortInfo> {
    PORTS.lock().clone()
}

fn with_port<R>(controller: usize, port: usize, f: impl FnOnce(&Port) -> R) -> Option<R> {
    let _io = lock_io();
    let guard = CONTROLLERS.lock();
    let c = guard.get(controller)?;
    let _ = c.abar;
    let p = c.ports.get(port)?.as_ref()?;
    Some(f(p))
}

fn scattered_limit(buffer: *const u8, max: usize) -> usize {
    if crate::memory::vmalloc::contains(buffer as usize) { max.min((PRD_MAX - 1) * 8) } else { max }
}

pub fn read_sectors(controller: usize, port: usize, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
    let total = buf.len() / 512;
    let mut done = 0usize;
    while done < total {
        let count = (total - done).min(MAX_SECTORS_PER_COMMAND);
        let slice = &mut buf[done * 512..(done + count) * 512];
        with_port(controller, port, |p| {
            let (command, max) = if p.lba48 { (ATA_READ_DMA_EXT, 65535) } else { (ATA_READ_DMA, 255) };
            let max = scattered_limit(slice.as_ptr(), max);
            let mut off = 0usize;
            while off < count {
                let n = (count - off).min(max);
                execute(p, command, lba + (done + off) as u64, n as u16, slice[off * 512..].as_mut_ptr(), n * 512, false)?;
                off += n;
            }
            Ok(())
        })
        .ok_or(())??;
        done += count;
    }
    Ok(())
}

pub fn write_sectors(controller: usize, port: usize, lba: u64, buf: &[u8]) -> Result<(), ()> {
    let total = buf.len() / 512;
    let mut done = 0usize;
    while done < total {
        let count = (total - done).min(MAX_SECTORS_PER_COMMAND);
        let slice = &buf[done * 512..(done + count) * 512];
        with_port(controller, port, |p| {
            let (command, max) = if p.lba48 { (ATA_WRITE_DMA_EXT, 65535) } else { (ATA_WRITE_DMA, 255) };
            let max = scattered_limit(slice.as_ptr(), max);
            let mut off = 0usize;
            while off < count {
                let n = (count - off).min(max);
                execute(p, command, lba + (done + off) as u64, n as u16, slice[off * 512..].as_ptr() as *mut u8, n * 512, true)?;
                off += n;
            }
            Ok(())
        })
        .ok_or(())??;
        done += count;
    }
    Ok(())
}

pub fn flush(controller: usize, port: usize) -> Result<(), ()> {
    with_port(controller, port, |p| {
        let command = if p.lba48 { ATA_FLUSH_EXT } else { ATA_FLUSH };
        execute(p, command, 0, 0, core::ptr::null_mut(), 0, false)
    })
    .ok_or(())?
}
