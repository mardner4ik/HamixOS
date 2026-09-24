use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::drivers::{ahci, ata};

#[derive(Clone, Copy)]
pub enum Backend {
    Ata(ata::Drive),
    Ahci { controller: usize, port: usize },
    Virtio(usize),
    Module(usize),
}

struct ModuleDisk {
    owner: String,
    name: String,
    model: String,
    ops: crate::module::classes::BlockOps,
    alive: bool,
}

static MODULE_DISKS: Mutex<Vec<ModuleDisk>> = Mutex::new(Vec::new());
static INITIALISED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn module_disk(slot: usize, owner: &str) -> Disk {
    let guard = MODULE_DISKS.lock();
    let d = &guard[slot];
    let _ = owner;
    Disk { name: String::new(), model: if d.model.is_empty() { d.name.clone() } else { d.model.clone() }, sectors: d.ops.sectors, bus: "module", backend: Backend::Module(slot) }
}

pub fn register_module_disk(owner: &str, name: &str, model: &str, ops: crate::module::classes::BlockOps) -> usize {
    let slot = {
        let mut list = MODULE_DISKS.lock();
        list.push(ModuleDisk { owner: String::from(owner), name: String::from(name), model: String::from(model), ops, alive: true });
        list.len() - 1
    };
    crate::drivers::klog::log(&format!("block: {} registered {} ({}, {})", owner, name, model, human_size(ops.sectors * 512)));
    if INITIALISED.load(core::sync::atomic::Ordering::Acquire) {
        let mut disk = module_disk(slot, owner);
        let mut disks = DISKS.lock();
        disk.name = disk_letter(disks.len());
        disks.push(disk);
        drop(disks);
        rescan();
    }
    slot
}

pub fn forget_module(owner: &str) {
    for d in MODULE_DISKS.lock().iter_mut().filter(|d| d.owner == owner) {
        d.alive = false;
    }
}

fn module_ops(slot: usize) -> Option<crate::module::classes::BlockOps> {
    MODULE_DISKS.lock().get(slot).filter(|d| d.alive).map(|d| d.ops)
}

fn module_io(slot: usize, lba: u64, buf: *mut u8, sectors: usize, write: bool) -> Result<(), ()> {
    let ops = module_ops(slot).ok_or(())?;
    let mut done = 0usize;
    while done < sectors {
        let count = (sectors - done).min(128);
        let at = unsafe { buf.add(done * 512) };
        let status = if write {
            ops.write.ok_or(())?(ops.context, lba + done as u64, count as u32, at)
        } else {
            ops.read.ok_or(())?(ops.context, lba + done as u64, count as u32, at)
        };
        if status != 0 {
            return Err(());
        }
        done += count;
    }
    Ok(())
}

#[derive(Clone)]
pub struct Disk {
    pub name: String,
    pub model: String,
    pub sectors: u64,
    pub bus: &'static str,
    pub backend: Backend,
}

#[derive(Clone)]
pub struct Partition {
    pub name: String,
    pub number: u32,
    pub start: u64,
    pub sectors: u64,
    pub kind: String,
    pub bootable: bool,
}

#[derive(Clone)]
pub struct Volume {
    pub disk: usize,
    pub offset: u64,
    pub sectors: u64,
    pub name: String,
}

static DISKS: Mutex<Vec<Disk>> = Mutex::new(Vec::new());

fn disk_letter(index: usize) -> String {
    let mut name = String::from("sd");
    if index >= 26 {
        name.push((b'a' + (index / 26 - 1) as u8) as char);
    }
    name.push((b'a' + (index % 26) as u8) as char);
    name
}

pub fn init() {
    ahci::init();
    ata::init();
    let mut disks = Vec::new();
    for port in ahci::ports() {
        disks.push(Disk {
            name: disk_letter(disks.len()),
            model: port.model.clone(),
            sectors: port.sectors,
            bus: "sata",
            backend: Backend::Ahci { controller: port.controller, port: port.port },
        });
    }
    for drive in ata::drives() {
        disks.push(Disk {
            name: disk_letter(disks.len()),
            model: drive.model.clone(),
            sectors: drive.drive.sectors,
            bus: "ata",
            backend: Backend::Ata(drive.drive),
        });
    }
    let module_slots = MODULE_DISKS.lock().len();
    for slot in 0..module_slots {
        if module_ops(slot).is_some() {
            let mut disk = module_disk(slot, "");
            disk.name = disk_letter(disks.len());
            disks.push(disk);
        }
    }
    for (index, sectors, location) in crate::drivers::virtio::blk::init() {
        disks.push(Disk {
            name: String::from("vd") + &String::from((b'a' + index as u8) as char),
            model: format!("virtio disk ({})", location),
            sectors,
            bus: "virtio",
            backend: Backend::Virtio(index),
        });
    }
    for disk in &disks {
        crate::drivers::klog::log(&format!(
            "block: {} {} {} ({})",
            disk.name,
            human_size(disk.sectors * 512),
            if disk.model.is_empty() { "disk" } else { disk.model.as_str() },
            disk.bus
        ));
    }
    *DISKS.lock() = disks;
    INITIALISED.store(true, core::sync::atomic::Ordering::Release);
    rescan();
}

pub fn disks() -> Vec<Disk> {
    DISKS.lock().clone()
}

pub fn disk(index: usize) -> Option<Disk> {
    DISKS.lock().get(index).cloned()
}

pub fn read(index: usize, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
    let disk = disk(index).ok_or(())?;
    if buf.len() % 512 != 0 || lba + (buf.len() / 512) as u64 > disk.sectors {
        return Err(());
    }
    match disk.backend {
        Backend::Ata(drive) => ata::read(&drive, lba, buf),
        Backend::Ahci { controller, port } => ahci::read_sectors(controller, port, lba, buf),
        Backend::Virtio(index) => crate::drivers::virtio::blk::read(index, lba, buf),
        Backend::Module(slot) => module_io(slot, lba, buf.as_mut_ptr(), buf.len() / 512, false),
    }
}

pub fn write(index: usize, lba: u64, buf: &[u8]) -> Result<(), ()> {
    let disk = disk(index).ok_or(())?;
    if buf.len() % 512 != 0 || lba + (buf.len() / 512) as u64 > disk.sectors {
        return Err(());
    }
    match disk.backend {
        Backend::Ata(drive) => ata::write(&drive, lba, buf),
        Backend::Ahci { controller, port } => ahci::write_sectors(controller, port, lba, buf),
        Backend::Virtio(index) => crate::drivers::virtio::blk::write(index, lba, buf),
        Backend::Module(slot) => module_io(slot, lba, buf.as_ptr() as *mut u8, buf.len() / 512, true),
    }
}

pub fn flush(index: usize) -> Result<(), ()> {
    let disk = disk(index).ok_or(())?;
    match disk.backend {
        Backend::Ata(drive) => ata::flush(&drive),
        Backend::Ahci { controller, port } => ahci::flush(controller, port),
        Backend::Virtio(index) => crate::drivers::virtio::blk::flush(index),
        Backend::Module(slot) => match module_ops(slot).and_then(|o| o.flush.map(|f| f(o.context))) {
            Some(0) | None => Ok(()),
            Some(_) => Err(()),
        },
    }
}

fn u32_at(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}

fn u64_at(buf: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap())
}

fn guid_kind(guid: &[u8]) -> String {
    const LINUX: [u8; 16] = [0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4];
    const EFI: [u8; 16] = [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B];
    const MSDATA: [u8; 16] = [0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7];
    const BIOSBOOT: [u8; 16] = [0x48, 0x61, 0x68, 0x21, 0x49, 0x64, 0x6F, 0x6E, 0x74, 0x4E, 0x65, 0x65, 0x64, 0x45, 0x46, 0x49];
    const SWAP: [u8; 16] = [0x6D, 0xFD, 0x57, 0x06, 0xAB, 0xA4, 0xC4, 0x43, 0x84, 0xE5, 0x09, 0x33, 0xC8, 0x4B, 0x4F, 0x4F];
    String::from(match guid {
        g if g == LINUX => "linux",
        g if g == EFI => "efi",
        g if g == MSDATA => "msdata",
        g if g == BIOSBOOT => "bios-boot",
        g if g == SWAP => "swap",
        _ => "gpt",
    })
}

fn mbr_kind(kind: u8) -> String {
    String::from(match kind {
        0x83 => "linux",
        0x82 => "swap",
        0x07 => "ntfs",
        0x0B | 0x0C => "fat32",
        0x04 | 0x06 | 0x0E => "fat16",
        0xEF => "efi",
        0x05 | 0x0F | 0x85 => "extended",
        0xA5 | 0xA6 | 0xA9 => "bsd",
        _ => return format!("0x{:02x}", kind),
    })
}

pub fn partitions(index: usize) -> Vec<Partition> {
    let Some(disk) = disk(index) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut mbr = alloc::vec![0u8; 512];
    if read(index, 0, &mut mbr).is_err() || mbr[510] != 0x55 || mbr[511] != 0xAA {
        return out;
    }
    let protective = (0..4).any(|i| mbr[446 + i * 16 + 4] == 0xEE);
    if protective {
        let mut header = alloc::vec![0u8; 512];
        if read(index, 1, &mut header).is_ok() && &header[0..8] == b"EFI PART" {
            let entries_lba = u64_at(&header, 72);
            let count = u32_at(&header, 80).min(256) as usize;
            let size = (u32_at(&header, 84) as usize).max(128);
            let sectors = (count * size).div_ceil(512);
            let mut table = alloc::vec![0u8; sectors * 512];
            if read(index, entries_lba, &mut table).is_ok() {
                for i in 0..count {
                    let entry = &table[i * size..i * size + size];
                    if entry[0..16].iter().all(|b| *b == 0) {
                        continue;
                    }
                    let first = u64_at(entry, 32);
                    let last = u64_at(entry, 40);
                    if last < first || last >= disk.sectors {
                        continue;
                    }
                    out.push(Partition {
                        name: format!("{}{}", disk.name, i + 1),
                        number: i as u32 + 1,
                        start: first,
                        sectors: last - first + 1,
                        kind: guid_kind(&entry[0..16]),
                        bootable: false,
                    });
                }
            }
            return out;
        }
    }
    let mut extended = None;
    for i in 0..4 {
        let entry = &mbr[446 + i * 16..462 + i * 16];
        let kind = entry[4];
        let start = u32_at(entry, 8) as u64;
        let count = u32_at(entry, 12) as u64;
        if kind == 0 || count == 0 || start == 0 || start + count > disk.sectors {
            continue;
        }
        if matches!(kind, 0x05 | 0x0F | 0x85) {
            extended = Some(start);
        }
        out.push(Partition {
            name: format!("{}{}", disk.name, i + 1),
            number: i as u32 + 1,
            start,
            sectors: count,
            kind: mbr_kind(kind),
            bootable: entry[0] & 0x80 != 0,
        });
    }
    if let Some(base) = extended {
        let mut ebr_lba = base;
        let mut number = 5;
        let mut sector = alloc::vec![0u8; 512];
        for _ in 0..32 {
            if read(index, ebr_lba, &mut sector).is_err() || sector[510] != 0x55 || sector[511] != 0xAA {
                break;
            }
            let entry = &sector[446..462];
            let start = u32_at(entry, 8) as u64;
            let count = u32_at(entry, 12) as u64;
            if entry[4] != 0 && count > 0 && ebr_lba + start + count <= disk.sectors {
                out.push(Partition {
                    name: format!("{}{}", disk.name, number),
                    number,
                    start: ebr_lba + start,
                    sectors: count,
                    kind: mbr_kind(entry[4]),
                    bootable: false,
                });
                number += 1;
            }
            let next = &sector[462..478];
            let next_start = u32_at(next, 8) as u64;
            if next[4] == 0 || next_start == 0 {
                break;
            }
            ebr_lba = base + next_start;
        }
    }
    out
}

#[derive(Clone)]
pub struct VolumeInfo {
    pub volume: Volume,
    pub partition: Option<Partition>,
    pub probe: Option<FsProbe>,
}

static VOLUME_CACHE: Mutex<Vec<VolumeInfo>> = Mutex::new(Vec::new());

pub fn rescan() {
    let mut out = Vec::new();
    for (index, disk) in disks().iter().enumerate() {
        let whole = Volume { disk: index, offset: 0, sectors: disk.sectors, name: disk.name.clone() };
        let parts = partitions(index);
        let whole_fs = if parts.is_empty() { probe(&whole) } else { None };
        out.push(VolumeInfo { volume: whole, partition: None, probe: whole_fs });
        for part in parts {
            let volume = Volume { disk: index, offset: part.start, sectors: part.sectors, name: part.name.clone() };
            let part_fs = probe(&volume);
            out.push(VolumeInfo { volume, partition: Some(part), probe: part_fs });
        }
    }
    *VOLUME_CACHE.lock() = out;
}

pub fn volume_infos() -> Vec<VolumeInfo> {
    VOLUME_CACHE.lock().clone()
}

pub fn volumes() -> Vec<Volume> {
    volume_infos().into_iter().map(|v| v.volume).collect()
}

pub fn find_volume(name: &str) -> Option<Volume> {
    let name = name.trim_start_matches("/dev/");
    volumes().into_iter().find(|v| v.name == name)
}

#[derive(Clone)]
pub struct FsProbe {
    pub kind: &'static str,
    pub label: String,
    pub uuid: String,
}

pub fn uuid_string(raw: &[u8]) -> String {
    let mut out = String::new();
    for (i, b) in raw.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub fn probe(volume: &Volume) -> Option<FsProbe> {
    let mut head = alloc::vec![0u8; 4096];
    if volume.sectors < 8 || read(volume.disk, volume.offset, &mut head).is_err() {
        return None;
    }
    if u16::from_le_bytes([head[1024 + 56], head[1024 + 57]]) == 0xEF53 {
        let sb = &head[1024..2048];
        let label_raw = &sb[120..136];
        let end = label_raw.iter().position(|b| *b == 0).unwrap_or(16);
        let label = String::from_utf8_lossy(&label_raw[..end]).into_owned();
        let hext = &sb[0x300..0x304] == b"HEXT";
        let incompat = u32_at(sb, 96);
        let compat = u32_at(sb, 92);
        let kind = if hext {
            "hext"
        } else if incompat & 0x40 != 0 || incompat & 0x80 != 0 {
            "ext4"
        } else if compat & 0x4 != 0 {
            "ext3"
        } else {
            "ext2"
        };
        return Some(FsProbe { kind, label, uuid: uuid_string(&sb[104..120]) });
    }
    if &head[3..11] == b"NTFS    " {
        return Some(FsProbe { kind: "ntfs", label: String::new(), uuid: String::new() });
    }
    if &head[82..87] == b"FAT32" {
        let label = String::from(String::from_utf8_lossy(&head[71..82]).trim());
        return Some(FsProbe { kind: "vfat", label, uuid: format!("{:08x}", u32_at(&head, 67)) });
    }
    if &head[54..59] == b"FAT16" || &head[54..59] == b"FAT12" {
        let label = String::from(String::from_utf8_lossy(&head[43..54]).trim());
        return Some(FsProbe { kind: "vfat", label, uuid: format!("{:08x}", u32_at(&head, 39)) });
    }
    if &head[4086..4096] == b"SWAPSPACE2" {
        return Some(FsProbe { kind: "swap", label: String::new(), uuid: String::new() });
    }
    None
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}B", bytes)
    } else if value >= 100.0 {
        format!("{}{}", value as u64, UNITS[unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        format!("{}.{}{}", tenths / 10, tenths % 10, UNITS[unit])
    }
}
