use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::io::{inb, inw, outb};

#[cfg(not(target_arch = "x86_64"))]
unsafe fn insw(port: u16, buf: *mut u8) {
    for i in 0..256 {
        let word = inw(port);
        unsafe { core::ptr::write_unaligned((buf as *mut u16).add(i), word) };
    }
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn outsw(port: u16, buf: *const u8) {
    for i in 0..256 {
        let word = unsafe { core::ptr::read_unaligned((buf as *const u16).add(i)) };
        crate::arch::io::outw(port, word);
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn insw(port: u16, buf: *mut u8) {
    unsafe {
        core::arch::asm!(
            "cld",
            "rep insw",
            in("dx") port,
            inout("rdi") buf => _,
            inout("rcx") 256usize => _,
            options(nostack)
        );
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn outsw(port: u16, buf: *const u8) {
    unsafe {
        core::arch::asm!(
            "cld",
            "rep outsw",
            in("dx") port,
            inout("rsi") buf => _,
            inout("rcx") 256usize => _,
            options(nostack)
        );
    }
}

const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_DF: u8 = 0x20;
const STATUS_BSY: u8 = 0x80;

#[derive(Clone, Copy, Debug)]
pub struct Drive {
    pub base: u16,
    pub control: u16,
    pub slave: bool,
    pub lba48: bool,
    pub sectors: u64,
}

#[derive(Clone)]
pub struct DriveInfo {
    pub drive: Drive,
    pub model: String,
}

static DRIVES: Mutex<Vec<DriveInfo>> = Mutex::new(Vec::new());
static IO_LOCK: Mutex<()> = Mutex::new(());

fn delay(control: u16) {
    for _ in 0..4 {
        inb(control);
    }
}

fn wait_not_busy(base: u16) -> Result<u8, ()> {
    for _ in 0..2_000_000u32 {
        let status = inb(base + 7);
        if status == 0xFF {
            return Err(());
        }
        if status & STATUS_BSY == 0 {
            return Ok(status);
        }
        core::hint::spin_loop();
    }
    Err(())
}

fn wait_data(base: u16) -> Result<(), ()> {
    for _ in 0..2_000_000u32 {
        let status = inb(base + 7);
        if status & STATUS_BSY != 0 {
            continue;
        }
        if status & (STATUS_ERR | STATUS_DF) != 0 {
            return Err(());
        }
        if status & STATUS_DRQ != 0 {
            return Ok(());
        }
    }
    Err(())
}

fn identify(base: u16, control: u16, slave: bool) -> Option<DriveInfo> {
    if inb(base + 7) == 0xFF {
        return None;
    }
    outb(base + 6, if slave { 0xB0 } else { 0xA0 });
    delay(control);
    outb(base + 2, 0);
    outb(base + 3, 0);
    outb(base + 4, 0);
    outb(base + 5, 0);
    outb(base + 7, 0xEC);
    delay(control);
    if inb(base + 7) == 0 {
        return None;
    }
    wait_not_busy(base).ok()?;
    if inb(base + 4) != 0 || inb(base + 5) != 0 {
        return None;
    }
    wait_data(base).ok()?;
    let mut words = [0u16; 256];
    for word in words.iter_mut() {
        *word = inw(base);
    }
    let lba48 = words[83] & (1 << 10) != 0;
    let sectors = if lba48 {
        words[100] as u64 | (words[101] as u64) << 16 | (words[102] as u64) << 32 | (words[103] as u64) << 48
    } else {
        words[60] as u64 | (words[61] as u64) << 16
    };
    if sectors == 0 {
        return None;
    }
    let mut model = String::new();
    for word in &words[27..47] {
        for byte in [(word >> 8) as u8, *word as u8] {
            if byte.is_ascii_graphic() || byte == b' ' {
                model.push(byte as char);
            }
        }
    }
    Some(DriveInfo {
        drive: Drive { base, control, slave, lba48, sectors },
        model: String::from(model.trim()),
    })
}

pub fn init() {
    if !crate::arch::io::PORTS {
        return;
    }
    let mut found = Vec::new();
    for (base, control) in [(0x1F0u16, 0x3F6u16), (0x170, 0x376)] {
        outb(control, 0x02);
        for slave in [false, true] {
            if let Some(info) = identify(base, control, slave) {
                crate::drivers::klog::log(&format!(
                    "ata: {} {} -- {} MiB{}",
                    if base == 0x1F0 { "primary" } else { "secondary" },
                    if slave { "slave" } else { "master" },
                    info.drive.sectors / 2048,
                    if info.model.is_empty() { String::new() } else { format!(" ({})", info.model) }
                ));
                found.push(info);
            }
        }
    }
    *DRIVES.lock() = found;
}

pub fn drives() -> Vec<DriveInfo> {
    DRIVES.lock().clone()
}

fn select(drive: &Drive, lba: u64, count: u16) {
    let base = drive.base;
    if drive.lba48 {
        outb(base + 6, if drive.slave { 0x50 } else { 0x40 });
        delay(drive.control);
        outb(base + 2, (count >> 8) as u8);
        outb(base + 3, (lba >> 24) as u8);
        outb(base + 4, (lba >> 32) as u8);
        outb(base + 5, (lba >> 40) as u8);
        outb(base + 2, count as u8);
        outb(base + 3, lba as u8);
        outb(base + 4, (lba >> 8) as u8);
        outb(base + 5, (lba >> 16) as u8);
    } else {
        outb(base + 6, (if drive.slave { 0xF0 } else { 0xE0 }) | ((lba >> 24) as u8 & 0x0F));
        delay(drive.control);
        outb(base + 2, count as u8);
        outb(base + 3, lba as u8);
        outb(base + 4, (lba >> 8) as u8);
        outb(base + 5, (lba >> 16) as u8);
    }
}

pub fn read(drive: &Drive, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
    let _guard = IO_LOCK.lock();
    let total = buf.len() / 512;
    let chunk_limit = if drive.lba48 { 1024 } else { 255 };
    let mut done = 0usize;
    while done < total {
        let count = (total - done).min(chunk_limit);
        wait_not_busy(drive.base)?;
        select(drive, lba + done as u64, count as u16);
        outb(drive.base + 7, if drive.lba48 { 0x24 } else { 0x20 });
        for s in 0..count {
            if s == 0 {
                delay(drive.control);
            }
            wait_data(drive.base)?;
            let offset = (done + s) * 512;
            unsafe { insw(drive.base, buf.as_mut_ptr().add(offset)) };
        }
        done += count;
    }
    Ok(())
}

pub fn write(drive: &Drive, lba: u64, buf: &[u8]) -> Result<(), ()> {
    let _guard = IO_LOCK.lock();
    let total = buf.len() / 512;
    let chunk_limit = if drive.lba48 { 1024 } else { 255 };
    let mut done = 0usize;
    while done < total {
        let count = (total - done).min(chunk_limit);
        wait_not_busy(drive.base)?;
        select(drive, lba + done as u64, count as u16);
        outb(drive.base + 7, if drive.lba48 { 0x34 } else { 0x30 });
        for s in 0..count {
            if s == 0 {
                delay(drive.control);
            }
            wait_data(drive.base)?;
            let offset = (done + s) * 512;
            unsafe { outsw(drive.base, buf.as_ptr().add(offset)) };
        }
        let status = wait_not_busy(drive.base)?;
        if status & (STATUS_ERR | STATUS_DF) != 0 {
            return Err(());
        }
        done += count;
    }
    Ok(())
}

pub fn flush(drive: &Drive) -> Result<(), ()> {
    let _guard = IO_LOCK.lock();
    wait_not_busy(drive.base)?;
    outb(drive.base + 6, if drive.slave { 0xB0 } else { 0xA0 });
    delay(drive.control);
    outb(drive.base + 7, if drive.lba48 { 0xEA } else { 0xE7 });
    delay(drive.control);
    let status = wait_not_busy(drive.base)?;
    if status & (STATUS_ERR | STATUS_DF) != 0 { Err(()) } else { Ok(()) }
}
