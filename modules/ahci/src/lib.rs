#![no_std]

use core::sync::atomic::{AtomicBool, Ordering};

use hamix_kpi::{self as kpi, BlockOps, PciHandle};

const HBA_CAP: usize = 0x00;
const HBA_GHC: usize = 0x04;
const HBA_PI: usize = 0x0C;
const HBA_VS: usize = 0x10;
const HBA_CAP2: usize = 0x24;
const HBA_BOHC: usize = 0x28;

const GHC_AE: u32 = 1 << 31;
const GHC_HR: u32 = 1 << 0;
const GHC_IE: u32 = 1 << 1;
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

const PRD_MAX: usize = 56;
const PRD_CHUNK: usize = 0x10000;
const MAX_PORTS: usize = 16;

#[derive(Clone, Copy)]
struct Port {
    base: usize,
    list_virt: usize,
    table: u64,
    table_virt: usize,
    lba48: bool,
    sectors: u64,
}

static mut PORTS: [Option<Port>; MAX_PORTS] = [None; MAX_PORTS];
static mut COUNT: usize = 0;
static BUSY: AtomicBool = AtomicBool::new(false);

fn ports() -> &'static mut [Option<Port>; MAX_PORTS] {
    unsafe { &mut *(&raw mut PORTS) }
}

fn read(base: usize, offset: usize) -> u32 {
    unsafe { kpi::hamix_readl((base + offset) as *const u32) }
}

fn write(base: usize, offset: usize, value: u32) {
    unsafe { kpi::hamix_writel(value, (base + offset) as *mut u32) }
}

fn sleep(ms: u64) {
    unsafe { kpi::hamix_mdelay(ms) }
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
        sleep(step);
        waited += step;
    }
}

fn dma_page() -> Option<(u64, usize)> {
    let mut phys = 0u64;
    let page = unsafe { kpi::hamix_dma_alloc(4096, &mut phys) };
    if page.is_null() {
        return None;
    }
    unsafe { core::ptr::write_bytes(page, 0, 4096) };
    Some((phys, page as usize))
}

fn stop_port(port: usize) -> bool {
    write(port, PX_CMD, read(port, PX_CMD) & !CMD_ST);
    let stopped = wait_until(500, || read(port, PX_CMD) & CMD_CR == 0);
    write(port, PX_CMD, read(port, PX_CMD) & !CMD_FRE);
    stopped && wait_until(500, || read(port, PX_CMD) & CMD_FR == 0)
}

fn start_port(port: usize) {
    wait_until(500, || read(port, PX_CMD) & CMD_CR == 0);
    write(port, PX_CMD, read(port, PX_CMD) | CMD_FRE);
    write(port, PX_CMD, read(port, PX_CMD) | CMD_ST);
}

fn link_up(port: usize) -> bool {
    let ssts = read(port, PX_SSTS);
    ssts & 0xF == 3 && (ssts >> 8) & 0xF == 1
}

fn comreset(port: usize) {
    write(port, PX_SCTL, (read(port, PX_SCTL) & !0xF) | 1);
    sleep(20);
    write(port, PX_SCTL, read(port, PX_SCTL) & !0xF);
    wait_until(1000, || read(port, PX_SSTS) & 0xF == 3);
    write(port, PX_SERR, 0xFFFF_FFFF);
}

fn recover(p: &Port) {
    stop_port(p.base);
    write(p.base, PX_SERR, 0xFFFF_FFFF);
    write(p.base, PX_IS, 0xFFFF_FFFF);
    if read(p.base, PX_TFD) & (TFD_BSY | TFD_DRQ) != 0 {
        comreset(p.base);
    }
    start_port(p.base);
}

fn fill_fis(table: usize, command: u8, lba: u64, count: u16) {
    let fis = table as *mut u8;
    let lba_mode = command != ATA_IDENTIFY;
    let short = command == ATA_READ_DMA || command == ATA_WRITE_DMA;
    unsafe {
        core::ptr::write_bytes(fis, 0, 0x80);
        *fis.add(0) = 0x27;
        *fis.add(1) = 0x80;
        *fis.add(2) = command;
        *fis.add(4) = lba as u8;
        *fis.add(5) = (lba >> 8) as u8;
        *fis.add(6) = (lba >> 16) as u8;
        *fis.add(7) = if lba_mode { 0x40 | if short { ((lba >> 24) & 0xF) as u8 } else { 0 } } else { 0 };
        *fis.add(8) = (lba >> 24) as u8;
        *fis.add(9) = (lba >> 32) as u8;
        *fis.add(10) = (lba >> 40) as u8;
        *fis.add(12) = count as u8;
        *fis.add(13) = (count >> 8) as u8;
    }
}

fn execute(p: &Port, command: u8, lba: u64, count: u16, buffer: *mut u8, bytes: usize, write_data: bool) -> bool {
    let port = p.base;
    if !wait_until(30_000, || read(port, PX_TFD) & (TFD_BSY | TFD_DRQ) == 0) {
        recover(p);
        return false;
    }
    fill_fis(p.table_virt, command, lba, count);
    let mut chunks = 0usize;
    let mut offset = 0usize;
    while offset < bytes {
        let (address, run) = kpi::dma_run(unsafe { buffer.add(offset) }, bytes - offset);
        if run == 0 || chunks == PRD_MAX {
            return false;
        }
        let length = run.min(PRD_CHUNK);
        unsafe {
            let entry = (p.table_virt + 0x80 + chunks * 16) as *mut u32;
            core::ptr::write_volatile(entry, address as u32);
            core::ptr::write_volatile(entry.add(1), (address >> 32) as u32);
            core::ptr::write_volatile(entry.add(2), 0);
            core::ptr::write_volatile(entry.add(3), (length - 1) as u32);
        }
        chunks += 1;
        offset += length;
    }
    unsafe {
        let header = p.list_virt as *mut u32;
        let flags = 5u32 | if write_data { 1 << 6 } else { 0 } | ((chunks as u32) << 16);
        core::ptr::write_volatile(header, flags);
        core::ptr::write_volatile(header.add(1), 0);
        core::ptr::write_volatile(header.add(2), p.table as u32);
        core::ptr::write_volatile(header.add(3), (p.table >> 32) as u32);
    }
    write(port, PX_IS, 0xFFFF_FFFF);
    write(port, PX_CI, 1);
    let timeout = if command == ATA_FLUSH_EXT || command == ATA_FLUSH { 60_000 } else { 30_000 };
    let started = unsafe { kpi::hamix_uptime_ms() };
    let mut spins = 0u64;
    loop {
        let ci = read(port, PX_CI);
        let is = read(port, PX_IS);
        if is & IS_FATAL != 0 || (read(port, PX_TFD) & TFD_ERR != 0 && ci & 1 == 0) {
            recover(p);
            return false;
        }
        if ci & 1 == 0 {
            break;
        }
        spins += 1;
        if spins < 2000 {
            core::hint::spin_loop();
            continue;
        }
        let now = unsafe { kpi::hamix_uptime_ms() };
        if now.saturating_sub(started) > timeout || (now == started && spins > 20_000_000) {
            recover(p);
            return false;
        }
        unsafe { kpi::hamix_udelay(50) };
    }
    if read(port, PX_IS) & IS_TFES != 0 || read(port, PX_TFD) & TFD_ERR != 0 {
        recover(p);
        return false;
    }
    true
}

fn setup_port(abar: usize, index: usize, staggered: bool) -> Option<(Port, [u16; 256])> {
    let port = abar + 0x100 + index * 0x80;
    if !stop_port(port) {
        comreset(port);
        if !stop_port(port) {
            return None;
        }
    }
    let (list, list_virt) = dma_page()?;
    let (fis, _) = dma_page()?;
    let (table, table_virt) = dma_page()?;
    write(port, PX_CLB, list as u32);
    write(port, PX_CLBU, (list >> 32) as u32);
    write(port, PX_FB, fis as u32);
    write(port, PX_FBU, (fis >> 32) as u32);
    write(port, PX_SERR, 0xFFFF_FFFF);
    write(port, PX_IS, 0xFFFF_FFFF);
    write(port, PX_IE, 0);
    write(port, PX_CMD, read(port, PX_CMD) | CMD_FRE);
    if staggered {
        write(port, PX_CMD, read(port, PX_CMD) | CMD_SUD | CMD_POD);
    }
    write(port, PX_CMD, (read(port, PX_CMD) & !(0xF << 28)) | CMD_ICC_ACTIVE);
    if !wait_until(100, || link_up(port)) {
        if read(port, PX_SSTS) & 0xF == 0 {
            return None;
        }
        comreset(port);
        if !wait_until(1000, || link_up(port)) {
            return None;
        }
    }
    write(port, PX_SERR, 0xFFFF_FFFF);
    wait_until(10_000, || read(port, PX_TFD) & (TFD_BSY | TFD_DRQ) == 0);
    let signature = read(port, PX_SIG);
    if signature == SIG_ATAPI || (signature != SIG_ATA && signature != 0xFFFF_FFFF) {
        stop_port(port);
        return None;
    }
    start_port(port);
    let p = Port { base: port, list_virt, table, table_virt, lba48: true, sectors: 0 };
    let (_, buffer) = dma_page()?;
    if !execute(&p, ATA_IDENTIFY, 0, 0, buffer as *mut u8, 512, false) {
        return None;
    }
    let mut words = [0u16; 256];
    for (i, word) in words.iter_mut().enumerate() {
        *word = unsafe { core::ptr::read_unaligned((buffer + i * 2) as *const u16) };
    }
    Some((p, words))
}

fn model_of(words: &[u16; 256]) -> [u8; 40] {
    let mut out = [0u8; 40];
    for i in 0..20 {
        out[i * 2] = (words[27 + i] >> 8) as u8;
        out[i * 2 + 1] = words[27 + i] as u8;
    }
    let mut end = out.len();
    while end > 0 && (out[end - 1] == b' ' || out[end - 1] == 0) {
        end -= 1;
    }
    for byte in out[end..].iter_mut() {
        *byte = 0;
    }
    out
}

fn lock() {
    while BUSY.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        sleep(1);
    }
}

fn unlock() {
    BUSY.store(false, Ordering::Release);
}

fn transfer(context: u64, lba: u64, count: u32, buffer: *mut u8, write_data: bool) -> i32 {
    let Some(p) = ports().get(context as usize).copied().flatten() else {
        return -19;
    };
    if lba + count as u64 > p.sectors {
        return -22;
    }
    lock();
    let (command, max) = match (p.lba48, write_data) {
        (true, false) => (ATA_READ_DMA_EXT, 65535usize),
        (true, true) => (ATA_WRITE_DMA_EXT, 65535),
        (false, false) => (ATA_READ_DMA, 255),
        (false, true) => (ATA_WRITE_DMA, 255),
    };
    let max = max.min((PRD_MAX - 1) * 8);
    let mut done = 0usize;
    let mut ok = true;
    while done < count as usize {
        let n = (count as usize - done).min(max);
        if !execute(&p, command, lba + done as u64, n as u16, unsafe { buffer.add(done * 512) }, n * 512, write_data) {
            ok = false;
            break;
        }
        done += n;
    }
    unlock();
    if ok { 0 } else { -5 }
}

extern "C" fn block_read(context: u64, lba: u64, count: u32, buffer: *mut u8) -> i32 {
    transfer(context, lba, count, buffer, false)
}

extern "C" fn block_write(context: u64, lba: u64, count: u32, buffer: *const u8) -> i32 {
    transfer(context, lba, count, buffer as *mut u8, true)
}

extern "C" fn block_flush(context: u64) -> i32 {
    let Some(p) = ports().get(context as usize).copied().flatten() else {
        return -19;
    };
    lock();
    let ok = execute(&p, if p.lba48 { ATA_FLUSH_EXT } else { ATA_FLUSH }, 0, 0, core::ptr::null_mut(), 0, false);
    unlock();
    if ok { 0 } else { -5 }
}

fn handoff(abar: usize) {
    if read(abar, HBA_CAP2) & 1 != 0 {
        write(abar, HBA_BOHC, read(abar, HBA_BOHC) | 2);
        wait_until(25, || read(abar, HBA_BOHC) & 1 == 0);
        if read(abar, HBA_BOHC) & 0x10 != 0 {
            wait_until(2000, || read(abar, HBA_BOHC) & 1 == 0);
        }
    }
}

fn attach(handle: &PciHandle) -> Option<usize> {
    let (base, len) = kpi::bar(handle, 5);
    if base == 0 || len == 0 {
        kpi::dev_warn("controller without an ABAR, skipped");
        return None;
    }
    let abar = unsafe { kpi::hamix_ioremap(base, len as usize) } as usize;
    if abar == 0 {
        kpi::dev_warn("the kernel refused to map the ABAR");
        return None;
    }
    let command = unsafe { kpi::hamix_pci_read16(handle, 0x04) };
    unsafe { kpi::hamix_pci_write16(handle, 0x04, (command | 0x0006) & !0x0400) };
    handoff(abar);
    write(abar, HBA_GHC, read(abar, HBA_GHC) | GHC_AE);
    let saved = read(abar, HBA_PI);
    let cap = read(abar, HBA_CAP);
    write(abar, HBA_GHC, read(abar, HBA_GHC) | GHC_HR);
    wait_until(1000, || read(abar, HBA_GHC) & GHC_HR == 0);
    write(abar, HBA_GHC, (read(abar, HBA_GHC) | GHC_AE) & !GHC_IE);
    if read(abar, HBA_PI) == 0 {
        write(abar, HBA_PI, saved);
    }
    let implemented = read(abar, HBA_PI);
    let _ = read(abar, HBA_VS);
    let staggered = cap & CAP_SSS != 0;
    let mut found = 0usize;
    for index in 0..32usize {
        if implemented & (1 << index) == 0 {
            continue;
        }
        let slot = unsafe { COUNT };
        if slot >= MAX_PORTS {
            break;
        }
        let Some((mut port, words)) = setup_port(abar, index, staggered) else {
            continue;
        };
        port.lba48 = words[83] & (1 << 10) != 0;
        port.sectors = if port.lba48 {
            words[100] as u64 | (words[101] as u64) << 16 | (words[102] as u64) << 32 | (words[103] as u64) << 48
        } else {
            words[60] as u64 | (words[61] as u64) << 16
        };
        if port.sectors == 0 {
            continue;
        }
        ports()[slot] = Some(port);
        unsafe { COUNT = slot + 1 };
        let ops = BlockOps {
            abi: kpi::BLOCK_ABI,
            sector_size: 512,
            sectors: port.sectors,
            context: slot as u64,
            read: Some(block_read),
            write: Some(block_write),
            flush: Some(block_flush),
            model: model_of(&words),
        };
        let mut name = *b"ahci-p00";
        name[6] = b'0' + (index / 10) as u8;
        name[7] = b'0' + (index % 10) as u8;
        if kpi::register_block(unsafe { core::str::from_utf8_unchecked(&name) }, &ops) >= 0 {
            found += 1;
        }
    }
    kpi::claim(kpi::CLASS_BLOCK, "ahci", handle, None, core::ptr::null_mut());
    Some(found)
}

fn init() -> i32 {
    let mut disks = 0usize;
    let mut controllers = 0usize;
    let mut claimed = 0usize;
    for (class, subclass, prog_if) in [(0x01u8, 0x06u8, 0x01u8), (0x01, 0x04, 0xFF)] {
        let mut index = 0u32;
        while let Some(handle) = kpi::find_class(class, subclass, prog_if, index) {
            index += 1;
            if subclass == 0x04 && handle.vendor != 0x8086 {
                continue;
            }
            controllers += 1;
            if let Some(found) = attach(&handle) {
                claimed += 1;
                disks += found;
            }
        }
    }
    if controllers == 0 {
        kpi::printk("ahci: no AHCI controller");
        return -1;
    }
    if claimed == 0 {
        kpi::printk("ahci: no controller could be taken over");
        return -1;
    }
    let mut line = [0u8; 48];
    let text = b"ahci: controllers ready, disks found: ";
    line[..text.len()].copy_from_slice(text);
    line[text.len()] = b'0' + (disks.min(9)) as u8;
    unsafe { kpi::hamix_printk(line.as_ptr(), text.len() + 1) };
    0
}

fn exit() {
    for slot in ports().iter_mut() {
        if let Some(p) = slot.take() {
            stop_port(p.base);
        }
    }
}

hamix_kpi::module!(init = init, exit = exit, version = "1.0");
