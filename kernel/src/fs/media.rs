use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use spin::Mutex;

use super::hextfs::{self, MountRow, MountSave, NEXT_DEV, POINTS, SAVED};
use super::{FileBody, Node, NodeKind, VFS};
use crate::drivers::block;

const ISO_BLOCK: u64 = 2048;
const MAX_FILES: usize = 200_000;
const DIR_MODE: u16 = 0o555;
const FILE_MODE: u16 = 0o444;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Iso9660,
    Fat,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Iso9660 => "iso9660",
            Format::Fat => "vfat",
        }
    }
}

#[derive(Clone, Copy)]
enum Extent {
    Iso { block: u64, size: u64 },
    Fat { cluster: u32, size: u32 },
}

#[derive(Clone, Copy)]
struct FatGeometry {
    bits: u8,
    sector_bytes: u64,
    cluster_sectors: u64,
    fat_start: u64,
    data_start: u64,
    root_start: u64,
    root_sectors: u64,
    root_cluster: u32,
    clusters: u32,
}

struct Mount {
    dev: u16,
    point: usize,
    path: String,
    source: String,
    format: Format,
    label: String,
    disk: usize,
    offset: u64,
    sectors: u64,
    fat: Option<FatGeometry>,
    files: Vec<Extent>,
}

static MOUNTS: Mutex<Vec<Mount>> = Mutex::new(Vec::new());

struct Entry {
    name: String,
    dir: bool,
    extent: Extent,
    children: Vec<Entry>,
}

fn read(disk: usize, lba: u64, count: usize) -> Option<Vec<u8>> {
    let mut buf = alloc::vec![0u8; count * 512];
    block::read(disk, lba, &mut buf).ok()?;
    Some(buf)
}

fn le16(b: &[u8], at: usize) -> u64 {
    u16::from_le_bytes([b[at], b[at + 1]]) as u64
}

fn le32(b: &[u8], at: usize) -> u64 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as u64
}

fn clean_label(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_matches(|c: char| c == ' ' || c == '\0').to_string()
}

pub fn detect(disk: usize, offset: u64) -> Option<Format> {
    if let Some(pvd) = read(disk, offset + 16 * 4, 4) {
        if &pvd[1..6] == b"CD001" {
            return Some(Format::Iso9660);
        }
    }
    let boot = read(disk, offset, 1)?;
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return None;
    }
    let bps = le16(&boot, 11);
    let spc = boot[13] as u64;
    let fats = boot[16];
    if !matches!(bps, 512 | 1024 | 2048 | 4096) || spc == 0 || !spc.is_power_of_two() || fats == 0 || le16(&boot, 14) == 0 {
        return None;
    }
    if &boot[82..87] == b"FAT32" || &boot[54..59] == b"FAT12" || &boot[54..59] == b"FAT16" || &boot[54..57] == b"FAT" {
        return Some(Format::Fat);
    }
    None
}

fn iso_name(raw: &[u8], joliet: bool) -> String {
    let text = if joliet {
        let units: Vec<u16> = raw.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).to_ascii_lowercase()
    };
    let text = match text.rfind(';') {
        Some(i) => &text[..i],
        None => &text[..],
    };
    let text = if !joliet { text.trim_end_matches('.') } else { text };
    String::from(text)
}

fn iso_dir(disk: usize, offset: u64, block: u64, length: u64, joliet: bool, depth: usize, files: &mut Vec<Extent>) -> Vec<Entry> {
    let mut out = Vec::new();
    if depth > 32 {
        return out;
    }
    let blocks = length.div_ceil(ISO_BLOCK).min(256) as usize;
    let Some(data) = read(disk, offset + block * 4, blocks * 4) else {
        return out;
    };
    let mut at = 0usize;
    while at < data.len() {
        let len = data[at] as usize;
        if len == 0 {
            at = (at / ISO_BLOCK as usize + 1) * ISO_BLOCK as usize;
            continue;
        }
        if at + len > data.len() || len < 34 {
            break;
        }
        let record = &data[at..at + len];
        at += len;
        let name_len = record[32] as usize;
        if 33 + name_len > len {
            continue;
        }
        let raw = &record[33..33 + name_len];
        if raw == [0] || raw == [1] {
            continue;
        }
        let flags = record[25];
        if flags & 1 != 0 {
            continue;
        }
        let extent = le32(record, 2);
        let size = le32(record, 10);
        let name = iso_name(raw, joliet);
        if name.is_empty() || files.len() >= MAX_FILES {
            continue;
        }
        if flags & 2 != 0 {
            let children = iso_dir(disk, offset, extent, size, joliet, depth + 1, files);
            out.push(Entry { name, dir: true, extent: Extent::Iso { block: extent, size: 0 }, children });
        } else {
            files.push(Extent::Iso { block: extent, size });
            out.push(Entry { name, dir: false, extent: Extent::Iso { block: extent, size }, children: Vec::new() });
        }
    }
    out
}

fn iso_tree(disk: usize, offset: u64, files: &mut Vec<Extent>) -> Option<(String, Vec<Entry>)> {
    let mut primary = None;
    let mut joliet = None;
    for index in 16..32u64 {
        let desc = read(disk, offset + index * 4, 4)?;
        if &desc[1..6] != b"CD001" {
            break;
        }
        match desc[0] {
            1 if primary.is_none() => primary = Some(desc),
            2 if desc[88] == 0x25 && desc[89] == 0x2F && matches!(desc[90], 0x40 | 0x43 | 0x45) => joliet = Some(desc),
            255 => break,
            _ => {}
        }
    }
    let primary = primary?;
    let label = clean_label(&primary[40..72]);
    let (desc, is_joliet) = match joliet {
        Some(d) => (d, true),
        None => (primary, false),
    };
    let root = &desc[156..190];
    let tree = iso_dir(disk, offset, le32(root, 2), le32(root, 10), is_joliet, 0, files);
    Some((label, tree))
}

fn fat_geometry(boot: &[u8]) -> Option<FatGeometry> {
    let bps = le16(boot, 11);
    let spc = boot[13] as u64;
    let reserved = le16(boot, 14);
    let fats = boot[16] as u64;
    let root_entries = le16(boot, 17);
    let total16 = le16(boot, 19);
    let fat16 = le16(boot, 22);
    let total32 = le32(boot, 32);
    let fat_size = if fat16 != 0 { fat16 } else { le32(boot, 36) };
    let total = if total16 != 0 { total16 } else { total32 };
    let root_sectors = (root_entries * 32).div_ceil(bps);
    let data_start = reserved + fats * fat_size + root_sectors;
    let clusters = total.checked_sub(data_start)? / spc;
    let bits = if clusters < 4085 { 12 } else if clusters < 65525 { 16 } else { 32 };
    let root_cluster = if bits == 32 { le32(boot, 44) as u32 } else { 0 };
    Some(FatGeometry { bits, sector_bytes: bps, cluster_sectors: spc, fat_start: reserved, data_start, root_start: reserved + fats * fat_size, root_sectors, root_cluster, clusters: clusters as u32 })
}

struct FatReader {
    disk: usize,
    offset: u64,
    geo: FatGeometry,
    cached: Option<(u64, Vec<u8>)>,
}

impl FatReader {
    fn lba(&self, sector: u64) -> u64 {
        self.offset + sector * (self.geo.sector_bytes / 512)
    }

    fn sector(&mut self, sector: u64) -> Option<&[u8]> {
        if self.cached.as_ref().map(|(s, _)| *s != sector).unwrap_or(true) {
            let data = read(self.disk, self.lba(sector), (self.geo.sector_bytes / 512) as usize)?;
            self.cached = Some((sector, data));
        }
        self.cached.as_ref().map(|(_, d)| d.as_slice())
    }

    fn next(&mut self, cluster: u32) -> Option<u32> {
        let bps = self.geo.sector_bytes;
        let (byte, end) = match self.geo.bits {
            12 => ((cluster as u64 * 3) / 2, 0xFF8),
            16 => (cluster as u64 * 2, 0xFFF8),
            _ => (cluster as u64 * 4, 0x0FFF_FFF8),
        };
        let sector = self.geo.fat_start + byte / bps;
        let within = (byte % bps) as usize;
        let value = match self.geo.bits {
            12 => {
                let lo = self.sector(sector)?[within] as u32;
                let hi = if within + 1 < bps as usize { self.sector(sector)?[within + 1] as u32 } else { self.sector(sector + 1)?[0] as u32 };
                let pair = lo | (hi << 8);
                if cluster & 1 != 0 { pair >> 4 } else { pair & 0xFFF }
            }
            16 => le16(self.sector(sector)?, within) as u32,
            _ => le32(self.sector(sector)?, within) as u32 & 0x0FFF_FFFF,
        };
        if value < 2 || value >= end || value > self.geo.clusters + 1 {
            None
        } else {
            Some(value)
        }
    }

    fn cluster_bytes(&self) -> usize {
        (self.geo.sector_bytes * self.geo.cluster_sectors) as usize
    }

    fn read_cluster(&mut self, cluster: u32) -> Option<Vec<u8>> {
        let first = self.geo.data_start + (cluster as u64 - 2) * self.geo.cluster_sectors;
        read(self.disk, self.lba(first), self.cluster_bytes() / 512)
    }

    fn read_chain(&mut self, start: u32, limit: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cluster = start;
        let mut guard = 0u32;
        while cluster >= 2 && out.len() < limit && guard <= self.geo.clusters {
            match self.read_cluster(cluster) {
                Some(data) => out.extend_from_slice(&data),
                None => break,
            }
            guard += 1;
            match self.next(cluster) {
                Some(n) => cluster = n,
                None => break,
            }
        }
        out.truncate(limit);
        out
    }

    fn dir_bytes(&mut self, cluster: u32) -> Vec<u8> {
        if cluster == 0 && self.geo.bits != 32 {
            return read(self.disk, self.lba(self.geo.root_start), (self.geo.root_sectors * self.geo.sector_bytes / 512) as usize).unwrap_or_default();
        }
        let start = if cluster == 0 { self.geo.root_cluster } else { cluster };
        self.read_chain(start, 64 << 20)
    }
}

fn short_name(raw: &[u8]) -> String {
    let base = String::from_utf8_lossy(&raw[0..8]).trim_end().to_string();
    let ext = String::from_utf8_lossy(&raw[8..11]).trim_end().to_string();
    let lower_base = raw[12] & 0x08 != 0;
    let lower_ext = raw[12] & 0x10 != 0;
    let base = if lower_base { base.to_lowercase() } else { base };
    let ext = if lower_ext { ext.to_lowercase() } else { ext };
    if ext.is_empty() { base } else { format!("{}.{}", base, ext) }
}

fn fat_dir(reader: &mut FatReader, cluster: u32, depth: usize, files: &mut Vec<Extent>, label: &mut String) -> Vec<Entry> {
    let mut out = Vec::new();
    if depth > 32 {
        return out;
    }
    let data = reader.dir_bytes(cluster);
    let mut long: Vec<(u8, Vec<u16>)> = Vec::new();
    for raw in data.chunks(32) {
        if raw.len() < 32 || raw[0] == 0 {
            break;
        }
        if raw[0] == 0xE5 {
            long.clear();
            continue;
        }
        let attr = raw[11];
        if attr == 0x0F {
            let mut units = Vec::new();
            for range in [1..11usize, 14..26, 28..32] {
                for pair in raw[range].chunks(2) {
                    units.push(u16::from_le_bytes([pair[0], pair[1]]));
                }
            }
            long.push((raw[0] & 0x3F, units));
            continue;
        }
        if attr & 0x08 != 0 {
            if depth == 0 && label.is_empty() {
                *label = clean_label(&raw[0..11]);
            }
            long.clear();
            continue;
        }
        let name = if long.is_empty() {
            short_name(raw)
        } else {
            long.sort_by_key(|(order, _)| *order);
            let units: Vec<u16> = long.iter().flat_map(|(_, u)| u.iter().copied()).take_while(|u| *u != 0 && *u != 0xFFFF).collect();
            String::from_utf16_lossy(&units)
        };
        long.clear();
        if name == "." || name == ".." || name.is_empty() || files.len() >= MAX_FILES {
            continue;
        }
        let first = ((le16(raw, 20) << 16) | le16(raw, 26)) as u32;
        let size = le32(raw, 28) as u32;
        if attr & 0x10 != 0 {
            let children = if first >= 2 { fat_dir(reader, first, depth + 1, files, label) } else { Vec::new() };
            out.push(Entry { name, dir: true, extent: Extent::Fat { cluster: first, size: 0 }, children });
        } else {
            files.push(Extent::Fat { cluster: first, size });
            out.push(Entry { name, dir: false, extent: Extent::Fat { cluster: first, size }, children: Vec::new() });
        }
    }
    out
}

fn insert(vfs: &mut super::Vfs, dir: usize, entries: Vec<Entry>, dev: u16, next: &mut u32) {
    for entry in entries {
        if entry.name.contains('/') {
            continue;
        }
        let kind = if entry.dir {
            NodeKind::Dir(BTreeMap::new())
        } else {
            let size = match entry.extent {
                Extent::Iso { size, .. } => size as usize,
                Extent::Fat { size, .. } => size as usize,
            };
            *next += 1;
            NodeKind::File(FileBody::backed(size, *next))
        };
        let mode = if entry.dir { DIR_MODE } else { FILE_MODE };
        let id = hextfs::attach(vfs, dir, &entry.name, Node { kind, parent: dir, owner: 0, group: 0, mode, ino: 0, dirty: 0, dev });
        vfs.nodes[id].parent = dir;
        if entry.dir {
            insert(vfs, id, entry.children, dev, next);
        }
    }
}

fn flatten(entries: &[Entry], out: &mut Vec<Extent>) {
    for e in entries {
        if e.name.contains('/') {
            continue;
        }
        if e.dir {
            flatten(&e.children, out);
        } else {
            out.push(e.extent);
        }
    }
}

pub fn owns(dev: u16) -> bool {
    MOUNTS.lock().iter().any(|m| m.dev == dev)
}

pub fn load(dev: u16, id: u32) -> Option<Vec<u8>> {
    let (disk, offset, extent, fat) = {
        let mounts = MOUNTS.lock();
        let m = mounts.iter().find(|m| m.dev == dev)?;
        (m.disk, m.offset, *m.files.get(id.checked_sub(1)? as usize)?, m.fat)
    };
    match extent {
        Extent::Iso { block, size } => {
            let sectors = (size.div_ceil(512)) as usize;
            let mut data = read(disk, offset + block * 4, sectors.max(1))?;
            data.truncate(size as usize);
            Some(data)
        }
        Extent::Fat { cluster, size } => {
            if size == 0 || cluster < 2 {
                return Some(Vec::new());
            }
            let mut reader = FatReader { disk, offset, geo: fat?, cached: None };
            Some(reader.read_chain(cluster, size as usize))
        }
    }
}

pub fn mount(device: &str, path: &str) -> Result<String, &'static str> {
    let volume = block::find_volume(device).ok_or("no such block device")?;
    if MOUNTS.lock().iter().any(|m| m.disk == volume.disk && m.offset == volume.offset) {
        return Err("the device is already mounted");
    }
    let format = detect(volume.disk, volume.offset).ok_or("no ISO 9660 or FAT filesystem on the device")?;
    let mut files = Vec::new();
    let (label, tree, fat) = match format {
        Format::Iso9660 => {
            let (label, tree) = iso_tree(volume.disk, volume.offset, &mut files).ok_or("cannot read the ISO 9660 directory")?;
            (label, tree, None)
        }
        Format::Fat => {
            let boot = read(volume.disk, volume.offset, 1).ok_or("cannot read the boot sector")?;
            let geo = fat_geometry(&boot).ok_or("the FAT boot sector is damaged")?;
            let mut reader = FatReader { disk: volume.disk, offset: volume.offset, geo, cached: None };
            let mut label = String::new();
            let tree = fat_dir(&mut reader, 0, 0, &mut files, &mut label);
            if label.is_empty() || label == "NO NAME" {
                let field = if geo.bits == 32 { &boot[71..82] } else { &boot[43..54] };
                label = clean_label(field);
            }
            if label == "NO NAME" {
                label.clear();
            }
            (label, tree, Some(geo))
        }
    };
    let mut ordered = Vec::new();
    flatten(&tree, &mut ordered);
    let count = ordered.len();
    let dev = NEXT_DEV.fetch_add(1, Ordering::Relaxed);
    let point = {
        let mut guard = VFS.lock();
        let vfs = guard.as_mut().ok_or("no root filesystem")?;
        let point = vfs.resolve(0, path).ok_or("mount point does not exist")?;
        if point == 0 || !vfs.is_dir(point) {
            return Err("mount point must be a directory other than /");
        }
        if hextfs::is_mount_point(point) {
            return Err("something is already mounted there");
        }
        let children = match &mut vfs.nodes[point].kind {
            NodeKind::Dir(map) => core::mem::take(map),
            _ => BTreeMap::new(),
        };
        SAVED.lock().insert(point, MountSave { ino: vfs.nodes[point].ino, dev: vfs.nodes[point].dev, dirty: vfs.nodes[point].dirty, children });
        vfs.nodes[point].dev = dev;
        vfs.nodes[point].dirty = 0;
        let mut next = 0u32;
        insert(vfs, point, tree, dev, &mut next);
        crate::arch::without_interrupts(|| POINTS.lock().insert(point, dev));
        point
    };
    let path_text = {
        let guard = VFS.lock();
        guard.as_ref().map(|v| v.path_of(point)).unwrap_or_else(|| String::from(path))
    };
    MOUNTS.lock().push(Mount { dev, point, path: path_text.clone(), source: format!("/dev/{}", volume.name), format, label: label.clone(), disk: volume.disk, offset: volume.offset, sectors: volume.sectors, fat, files: ordered });
    crate::drivers::klog::log(&format!("media: /dev/{} ({} \"{}\") mounted read-only on {} ({} files)", volume.name, format.name(), label, path_text, count));
    Ok(format!("/dev/{} mounted on {} ({}, {} files)", volume.name, path_text, format.name(), count))
}

pub fn umount(path: &str) -> Result<(), &'static str> {
    let point = {
        let guard = VFS.lock();
        let vfs = guard.as_ref().ok_or("no root filesystem")?;
        vfs.resolve(0, path).ok_or("not mounted")?
    };
    let prefix = {
        let mounts = MOUNTS.lock();
        mounts.iter().find(|m| m.point == point).map(|m| m.path.clone()).ok_or("not mounted")?
    };
    let busy = crate::task::with_tasks(|tasks| tasks.values().any(|t| t.state != crate::task::State::Zombie && (t.cwd == prefix || t.cwd.starts_with(&format!("{}/", prefix)))));
    if busy {
        return Err("target is busy (a process is working inside it)");
    }
    MOUNTS.lock().retain(|m| m.point != point);
    let mut guard = VFS.lock();
    if let Some(vfs) = guard.as_mut() {
        hextfs::clear_subtree(vfs, point);
        if let Some(save) = SAVED.lock().remove(&point) {
            vfs.nodes[point].kind = NodeKind::Dir(save.children);
            vfs.nodes[point].ino = save.ino;
            vfs.nodes[point].dev = save.dev;
            vfs.nodes[point].dirty = save.dirty;
        }
    }
    drop(guard);
    crate::arch::without_interrupts(|| POINTS.lock().remove(&point));
    Ok(())
}

pub fn forget_disk(disk: usize) -> Vec<String> {
    let paths: Vec<String> = MOUNTS.lock().iter().filter(|m| m.disk == disk).map(|m| m.path.clone()).collect();
    for path in &paths {
        let _ = umount(path);
    }
    paths
}

pub fn rows() -> Vec<(MountRow, &'static str)> {
    MOUNTS
        .lock()
        .iter()
        .map(|m| {
            let total_kb = m.sectors / 2;
            (MountRow { source: m.source.clone(), path: m.path.clone(), persistent: false, label: m.label.clone(), total_kb, free_kb: 0, errors: 0, disk: Some((m.disk, m.offset, m.sectors)) }, m.format.name())
        })
        .collect()
}

pub fn locate_fat_file(disk: usize, offset: u64, path: &str) -> Result<(u64, u64), String> {
    let boot = read(disk, offset, 1).ok_or_else(|| format!("read error at sector {}", offset))?;
    if detect(disk, offset) != Some(Format::Fat) {
        return Err(String::from("not FAT"));
    }
    let geo = fat_geometry(&boot).ok_or_else(|| String::from("damaged FAT boot sector"))?;
    let mut reader = FatReader { disk, offset, geo, cached: None };
    let mut files = Vec::new();
    let mut label = String::new();
    let mut entries = fat_dir(&mut reader, 0, 0, &mut files, &mut label);
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut found = None;
    for (i, part) in parts.iter().enumerate() {
        let Some(position) = entries.iter().position(|e| e.name.eq_ignore_ascii_case(part)) else {
            return Err(format!("FAT \"{}\" has no {}", label, path));
        };
        let entry = entries.swap_remove(position);
        if i + 1 == parts.len() {
            found = Some(entry);
            break;
        }
        entries = entry.children;
    }
    let Some(Entry { dir: false, extent: Extent::Fat { cluster, size }, .. }) = found else {
        return Err(format!("FAT \"{}\" has no {}", label, path));
    };
    let clusters = (size as u64).div_ceil(reader.cluster_bytes() as u64);
    let mut current = cluster;
    for _ in 1..clusters {
        match reader.next(current) {
            Some(n) if n == current + 1 => current = n,
            _ => return Err(format!("{} is fragmented on FAT \"{}\" (copy it again or write the stick with dd)", path, label)),
        }
    }
    let first = geo.data_start + (cluster as u64 - 2) * geo.cluster_sectors;
    Ok((reader.lba(first), size as u64))
}
