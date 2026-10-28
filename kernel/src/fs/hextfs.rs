use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use spin::Mutex;

use hext::{BlockDevice, FormatOptions, Hext, MemDevice, ROOT_INO, S_IFDIR, S_IFREG};

use super::{Node, NodeKind, Vfs, DIRTY_DATA, DIRTY_META, VFS, VOLATILE};
use crate::drivers::block;

const VOLATILE_DIRS: [&str; 5] = ["dev", "proc", "sys", "run", "tmp"];
const SYNC_INTERVAL_MS: u64 = 400;

pub enum KernelDevice {
    Memory(MemDevice),
    Block { disk: usize, offset: u64, sectors: u64 },
}

impl BlockDevice for KernelDevice {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), hext::Error> {
        match self {
            KernelDevice::Memory(m) => m.read(sector, buf),
            KernelDevice::Block { disk, offset, sectors } => {
                if sector + (buf.len() / 512) as u64 > *sectors {
                    return Err(hext::Error::Io);
                }
                block::read(*disk, *offset + sector, buf).map_err(|_| hext::Error::Io)
            }
        }
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), hext::Error> {
        match self {
            KernelDevice::Memory(m) => m.write(sector, buf),
            KernelDevice::Block { disk, offset, sectors } => {
                if sector + (buf.len() / 512) as u64 > *sectors {
                    return Err(hext::Error::Io);
                }
                for (i, chunk) in buf.chunks(256 * 512).enumerate() {
                    block::write(*disk, *offset + sector + (i * 256) as u64, chunk).map_err(|_| hext::Error::Io)?;
                    if crate::task::current_pid() != 0 && crate::arch::interrupts_enabled() {
                        crate::task::yield_now();
                    }
                }
                Ok(())
            }
        }
    }

    fn flush(&mut self) -> Result<(), hext::Error> {
        match self {
            KernelDevice::Memory(_) => Ok(()),
            KernelDevice::Block { disk, .. } => block::flush(*disk).map_err(|_| hext::Error::Io),
        }
    }

    fn sectors(&self) -> u64 {
        match self {
            KernelDevice::Memory(m) => m.sectors(),
            KernelDevice::Block { sectors, .. } => *sectors,
        }
    }
}

pub struct Backend {
    pub fs: Hext<KernelDevice>,
    pub source: String,
    pub path: String,
    pub persistent: bool,
    pub errors: u32,
    pub dev: u16,
    pub point: usize,
    pub disk: Option<(usize, u64, u64)>,
    pub blocks: BTreeMap<u32, (Vec<u32>, u64)>,
}

pub(crate) struct MountSave {
    pub(crate) ino: u32,
    pub(crate) dev: u16,
    pub(crate) dirty: u8,
    pub(crate) children: BTreeMap<String, usize>,
}

#[derive(Clone)]
pub struct MountRow {
    pub source: String,
    pub path: String,
    pub persistent: bool,
    pub label: String,
    pub total_kb: u64,
    pub free_kb: u64,
    pub errors: u32,
    pub disk: Option<(usize, u64, u64)>,
}

static BACKENDS: Mutex<Vec<Backend>> = Mutex::new(Vec::new());
pub(crate) static SAVED: Mutex<BTreeMap<usize, MountSave>> = Mutex::new(BTreeMap::new());
pub(crate) static POINTS: Mutex<BTreeMap<usize, u16>> = Mutex::new(BTreeMap::new());
static TABLE: Mutex<Vec<MountRow>> = Mutex::new(Vec::new());
pub(crate) static NEXT_DEV: AtomicU16 = AtomicU16::new(1);
static SYNC_REQUESTED: AtomicBool = AtomicBool::new(false);
static LAST_SYNC: AtomicU64 = AtomicU64::new(0);
static ROOT_DESCRIPTION: Mutex<String> = Mutex::new(String::new());

fn now() -> u32 {
    crate::drivers::rtc::now() as u32
}

pub fn is_mount_point(node: usize) -> bool {
    crate::arch::without_interrupts(|| POINTS.lock().contains_key(&node))
}

static LIVE_ROW: Mutex<Option<MountRow>> = Mutex::new(None);

fn refresh_table(backends: &[Backend]) {
    let mut rows: Vec<MountRow> = LIVE_ROW.lock().iter().cloned().collect();
    rows.extend(backends
        .iter()
        .map(|b| {
            let s = b.fs.stats();
            MountRow {
                source: b.source.clone(),
                path: b.path.clone(),
                persistent: b.persistent,
                label: b.fs.label(),
                total_kb: s.blocks as u64 * s.block_size as u64 / 1024,
                free_kb: s.free_blocks as u64 * s.block_size as u64 / 1024,
                errors: b.errors,
                disk: b.disk,
            }
        }));
    *TABLE.lock() = rows;
}

pub fn mount_table() -> Vec<MountRow> {
    let mut rows = TABLE.lock().clone();
    rows.extend(super::media::rows().into_iter().map(|(row, _)| row));
    rows
}

pub fn mounts_text() -> String {
    let mut out = String::new();
    for row in TABLE.lock().iter() {
        out.push_str(&format!(
            "{}\t{}\thext\t{}\t{}\t{}\t{}\n",
            row.source,
            row.path,
            if row.persistent { "rw" } else { "ram" },
            row.total_kb,
            row.free_kb,
            row.label
        ));
    }
    for (row, kind) in super::media::rows() {
        out.push_str(&format!("{}\t{}\t{}\tro\t{}\t{}\t{}\n", row.source, row.path, kind, row.total_kb, row.free_kb, row.label));
    }
    out
}

pub fn mount_point_known(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    mount_table().iter().any(|r| r.path == path)
}

pub fn mount_path_of(disk: usize, offset: u64) -> Option<String> {
    mount_table().into_iter().find(|r| matches!(r.disk, Some((d, o, _)) if d == disk && o == offset)).map(|r| r.path)
}

pub fn overlaps_mounted(disk: usize, start: u64, sectors: u64) -> bool {
    mount_table().iter().any(|r| match r.disk {
        Some((d, o, n)) => d == disk && start < o + n && o < start + sectors,
        None => false,
    })
}

fn module_device() -> Option<KernelDevice> {
    let module = crate::memory::find_module("hext")?;
    let device = unsafe { MemDevice::new(module.start as usize as *mut u8, module.len()) };
    Some(KernelDevice::Memory(device))
}

fn child(vfs: &Vfs, dir: usize, name: &str) -> Option<usize> {
    match &vfs.nodes[dir].kind {
        NodeKind::Dir(map) => map.get(name).copied(),
        _ => None,
    }
}

pub(crate) fn attach(vfs: &mut Vfs, dir: usize, name: &str, node: Node) -> usize {
    if let Some(existing) = child(vfs, dir, name) {
        let same_kind = matches!(
            (&vfs.nodes[existing].kind, &node.kind),
            (NodeKind::Dir(_), NodeKind::Dir(_)) | (NodeKind::File(_), NodeKind::File(_))
        );
        if same_kind {
            let target = &mut vfs.nodes[existing];
            if let (NodeKind::File(old), NodeKind::File(new)) = (&mut target.kind, node.kind) {
                *old = new;
            }
            target.ino = node.ino;
            target.mode = node.mode;
            target.owner = node.owner;
            target.group = node.group;
            target.dirty = node.dirty;
            target.dev = node.dev;
            return existing;
        }
    }
    let id = vfs.push_node(node);
    if let NodeKind::Dir(map) = &mut vfs.nodes[dir].kind {
        map.insert(name.to_string(), id);
    }
    id
}

struct Loaded {
    name: String,
    ino: u32,
    uid: u32,
    gid: u32,
    mode: u16,
    dir: bool,
    body: crate::fs::FileBody,
    children: Vec<Loaded>,
}

fn read_tree(fs: &mut Hext<KernelDevice>, ino: u32, top: bool, files: &mut usize, lazy: bool) -> Vec<Loaded> {
    let Ok(entries) = fs.read_dir(ino) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries {
        if entry.name == "lost+found" && ino == ROOT_INO {
            continue;
        }
        let Ok(inode) = fs.read_inode(entry.ino) else {
            continue;
        };
        if inode.is_dir() {
            let children = if top && VOLATILE_DIRS.contains(&entry.name.as_str()) {
                Vec::new()
            } else {
                read_tree(fs, entry.ino, false, files, lazy)
            };
            out.push(Loaded { name: entry.name, ino: entry.ino, uid: inode.uid, gid: inode.gid, mode: inode.mode & 0o7777, dir: true, body: crate::fs::FileBody::held(Vec::new()), children });
        } else if inode.is_file() {
            let link_candidate = inode.size as usize <= crate::fs::EAGER_FILE_MAX && inode.mode & 0o777 == 0o777;
            let body = if lazy && !link_candidate {
                crate::fs::FileBody::backed(inode.size as usize, entry.ino)
            } else {
                crate::fs::FileBody::held(fs.read_file(entry.ino).unwrap_or_default())
            };
            *files += 1;
            out.push(Loaded { name: entry.name, ino: entry.ino, uid: inode.uid, gid: inode.gid, mode: inode.mode & 0o7777, dir: false, body, children: Vec::new() });
        }
    }
    out
}

fn insert_tree(vfs: &mut Vfs, dir: usize, entries: Vec<Loaded>, dev: u16) {
    for entry in entries {
        let kind = if entry.dir { NodeKind::Dir(BTreeMap::new()) } else { NodeKind::File(entry.body) };
        let id = attach(vfs, dir, &entry.name, Node { kind, parent: dir, owner: entry.uid, group: entry.gid, mode: entry.mode, ino: entry.ino, dirty: 0, dev });
        vfs.nodes[id].parent = dir;
        if entry.dir {
            insert_tree(vfs, id, entry.children, dev);
        }
    }
}

fn overlay_tree(vfs: &mut Vfs, root: &mut Hext<KernelDevice>, fs: &mut Hext<KernelDevice>, dir: usize, ino: u32) {
    let Ok(entries) = fs.read_dir(ino) else {
        return;
    };
    for entry in entries {
        let Ok(inode) = fs.read_inode(entry.ino) else {
            continue;
        };
        let existing = child(vfs, dir, &entry.name);
        if inode.is_dir() {
            let id = match existing {
                Some(id) if vfs.is_dir(id) => id,
                _ => {
                    let id = vfs.push_node(Node {
                        kind: NodeKind::Dir(BTreeMap::new()),
                        parent: dir,
                        owner: inode.uid,
                        group: inode.gid,
                        mode: inode.mode & 0o7777,
                        ino: 0,
                        dirty: DIRTY_META,
                        dev: 0,
                    });
                    if let NodeKind::Dir(map) = &mut vfs.nodes[dir].kind {
                        map.insert(entry.name.clone(), id);
                    }
                    id
                }
            };
            overlay_tree(vfs, root, fs, id, entry.ino);
        } else if inode.is_file() {
            let data = fs.read_file(entry.ino).unwrap_or_default();
            match existing {
                Some(id) => {
                    let on_disk = vfs.nodes[id].ino;
                    let unchanged = match &vfs.nodes[id].kind {
                        NodeKind::File(old) if old.is_resident() => !old.differs(&data),
                        NodeKind::File(old) => old.len() == data.len() && on_disk != 0 && root.read_file(on_disk).map(|d| d == data).unwrap_or(false),
                        _ => true,
                    };
                    if unchanged {
                        continue;
                    }
                    if let NodeKind::File(old) = &mut vfs.nodes[id].kind {
                        old.replace_from_overlay(data);
                        crate::memory::page_cache::invalidate(id);
                        vfs.nodes[id].dirty |= DIRTY_DATA;
                    }
                }
                None => {
                    let id = vfs.push_node(Node {
                        kind: NodeKind::File(crate::fs::FileBody::held(data)),
                        parent: dir,
                        owner: inode.uid,
                        group: inode.gid,
                        mode: inode.mode & 0o7777,
                        ino: 0,
                        dirty: DIRTY_DATA | DIRTY_META,
                        dev: 0,
                    });
                    if let NodeKind::Dir(map) = &mut vfs.nodes[dir].kind {
                        map.insert(entry.name.clone(), id);
                    }
                }
            }
        }
    }
}

fn volume_matches(spec: &str, volume: &block::Volume, probe: &block::FsProbe) -> bool {
    if let Some(uuid) = spec.strip_prefix("UUID=") {
        return probe.uuid.eq_ignore_ascii_case(uuid);
    }
    if let Some(label) = spec.strip_prefix("LABEL=") {
        return probe.label == label;
    }
    spec.trim_start_matches("/dev/") == volume.name
}

fn find_root_volume(spec: &str) -> Option<block::Volume> {
    for info in block::volume_infos() {
        let Some(probe) = info.probe.as_ref() else {
            continue;
        };
        if probe.kind != "hext" {
            continue;
        }
        if spec == "auto" || volume_matches(spec, &info.volume, probe) {
            return Some(info.volume);
        }
    }
    None
}

static MEDIA_REPORT: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn media_report() -> Vec<String> {
    MEDIA_REPORT.lock().clone()
}

fn removable(index: usize) -> bool {
    let usb = block::disk(index).map(|d| d.bus == "usb").unwrap_or(false);
    let only_usb = crate::memory::cmdline_value("livemedia").as_deref() == Some("usb");
    usb || (!only_usb && block::is_optical(index))
}

static MEDIA_FOUND: Mutex<Option<(usize, u64, u64)>> = Mutex::new(None);

fn volume_label(sector: &[u8]) -> String {
    String::from_utf8_lossy(&sector[40..72]).trim().to_string()
}

fn image_in_iso_file(index: usize, start: u64, part: &str, image: &super::exfat::FoundFile) -> Option<(usize, u64, u64)> {
    let runs = super::exfat::file_runs(index, start, image)?;
    let (disk, base) = if runs.len() == 1 {
        (index, runs[0].0)
    } else {
        (block::register_mapped_disk(index, &format!("{}{}", part, image.path), runs), 0)
    };
    let (offset, bytes) = super::iso9660::find_file_at(disk, base, LIVE_IMAGE)?;
    crate::drivers::klog::log(&format!("hext: {} found inside /dev/{}{} (Ventoy-style ISO file)", LIVE_IMAGE, part, image.path));
    Some((disk, offset, bytes))
}

const VENTOY_GUID: [u8; 16] = [0x20, 0x20, 0x77, 0x77, 0x77, 0x2e, 0x76, 0x65, 0x6e, 0x74, 0x6f, 0x79, 0x2e, 0x6e, 0x65, 0x74];
const VENTOY_PARAM: usize = 512;
const VENTOY_DISK_SIZE: usize = 33;
const VENTOY_SIGNATURE: usize = 481;
const VENTOY_DISK_SIGNATURE: usize = 440;

fn le32(data: &[u8], at: usize) -> u64 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as u64
}

fn le64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

static VENTOY_DISK: Mutex<Option<(usize, usize)>> = Mutex::new(None);

fn ventoy_image(index: usize) -> Option<(usize, u64, u64)> {
    let param = crate::memory::ventoy()?;
    if param.len() < VENTOY_PARAM + 28 || param[..16] != VENTOY_GUID || param[VENTOY_PARAM..VENTOY_PARAM + 16] != VENTOY_GUID {
        return None;
    }
    let disk = block::disk(index)?;
    let mut mbr = alloc::vec![0u8; 512];
    block::read(index, 0, &mut mbr).ok()?;
    let same_signature = mbr[VENTOY_DISK_SIGNATURE..VENTOY_DISK_SIGNATURE + 4] == param[VENTOY_SIGNATURE..VENTOY_SIGNATURE + 4] && param[VENTOY_SIGNATURE..VENTOY_SIGNATURE + 4] != [0; 4];
    let same_size = disk.sectors * 512 == le64(&param, VENTOY_DISK_SIZE);
    if !same_signature && !same_size {
        return None;
    }
    let location = &param[VENTOY_PARAM..];
    let image_sector = le32(location, 16).max(1);
    let disk_sector = le32(location, 20).max(1);
    let count = le32(location, 24) as usize;
    if 28 + count * 16 > location.len() || count == 0 || image_sector % 512 != 0 || disk_sector % 512 != 0 {
        return None;
    }
    let mut runs = Vec::with_capacity(count);
    for i in 0..count {
        let region = &location[28 + i * 16..44 + i * 16];
        let sectors = le32(region, 0) * (image_sector / 512);
        let start = le64(region, 8) * (disk_sector / 512);
        if sectors == 0 || start + sectors > disk.sectors {
            return None;
        }
        runs.push((start, sectors));
    }
    let mapped = {
        let mut known = VENTOY_DISK.lock();
        match *known {
            Some((parent, mapped)) if parent == index => mapped,
            _ => {
                let mapped = block::register_mapped_disk(index, "Ventoy image", runs);
                *known = Some((index, mapped));
                mapped
            }
        }
    };
    let (offset, bytes) = super::iso9660::find_file_at(mapped, 0, LIVE_IMAGE)?;
    crate::drivers::klog::log(&format!("hext: {} found through the Ventoy image table on /dev/{} ({} fragments)", LIVE_IMAGE, disk.name, count));
    Some((mapped, offset, bytes))
}

fn find_media_image(index: usize) -> Result<(usize, u64, u64), String> {
    if let Some(found) = ventoy_image(index) {
        return Ok(found);
    }
    let name = block::disk(index).map(|d| d.name).unwrap_or_default();
    let mut probe = alloc::vec![0u8; 2048];
    if block::read(index, 64, &mut probe).is_err() {
        return Err(format!("{}: read error at sector 64", name));
    }
    if let Some((offset, bytes)) = super::iso9660::find_file(index, LIVE_IMAGE) {
        return Ok((index, offset, bytes));
    }
    let mut notes = Vec::new();
    if &probe[1..6] == b"CD001" {
        notes.push(format!("ISO 9660 without {}", LIVE_IMAGE));
    }
    let mut starts: Vec<(String, u64)> = block::partitions(index).into_iter().map(|p| (p.name, p.start)).collect();
    if starts.is_empty() {
        starts.push((name.clone(), 0));
    }
    for (part, start) in starts {
        match super::media::locate_fat_file(index, start, LIVE_IMAGE) {
            Ok((offset, bytes)) => {
                crate::drivers::klog::log(&format!("hext: {} found on FAT /dev/{}", LIVE_IMAGE, part));
                return Ok((index, offset, bytes));
            }
            Err(e) if e == "not FAT" => {}
            Err(e) => notes.push(format!("{}: {}", part, e)),
        }
        match super::exfat::find_files(index, start, ".iso") {
            Ok(mut images) => {
                if images.is_empty() {
                    notes.push(format!("{} is exFAT without .iso files", part));
                }
                images.sort_by_key(|i| !i.path.to_ascii_lowercase().contains("hamix"));
                let mut skipped = 0;
                for image in &images {
                    if !image.path.to_ascii_lowercase().contains("hamix") {
                        let label = super::exfat::read_file_sector(index, start, image, 32768).map(|s| volume_label(&s)).unwrap_or_default();
                        if label != "HAMIXOS" && label != "ISOIMAGE" {
                            skipped += 1;
                            continue;
                        }
                    }
                    match image_in_iso_file(index, start, &part, image) {
                        Some(found) => return Ok(found),
                        None => notes.push(format!("{}{} holds no {}", part, image.path, LIVE_IMAGE)),
                    }
                }
                if skipped > 0 {
                    notes.push(format!("{} other .iso file(s) on {} are not HamixOS", skipped, part));
                }
            }
            Err(e) if e == "not exFAT" => notes.push(format!("{} is neither FAT nor exFAT", part)),
            Err(e) => notes.push(format!("{}: {}", part, e)),
        }
    }
    Err(format!("{}: {}", name, notes.join(", ")))
}

fn scan_media() -> bool {
    if MEDIA_FOUND.lock().is_some() {
        return true;
    }
    let mut report = Vec::new();
    for index in 0..block::disks().len() {
        if !removable(index) {
            continue;
        }
        match find_media_image(index) {
            Ok(found) => {
                *MEDIA_FOUND.lock() = Some(found);
                return true;
            }
            Err(e) => report.push(e),
        }
    }
    for line in &report {
        crate::drivers::klog::log(&format!("hext: {}", line));
    }
    *MEDIA_REPORT.lock() = report;
    false
}

fn any_media_image() -> bool {
    scan_media()
}

const USB_ROOT_WAIT_MS: u64 = 10_000;

fn wait_for_usb_image() {
    crate::drivers::usb::init();
    let started = crate::task::uptime_ms();
    loop {
        if crate::drivers::usb::scan() && any_media_image() {
            return;
        }
        if crate::task::uptime_ms() - started >= USB_ROOT_WAIT_MS {
            crate::drivers::klog::log("hext: no USB stick with the live image appeared");
            return;
        }
        crate::task::sleep_ticks((crate::task::TICK_HZ / 4).max(1));
    }
}

pub fn mount_root() -> Option<String> {
    let spec = crate::memory::cmdline_value("root").unwrap_or_else(|| String::from("auto"));
    let no_image = crate::memory::find_module("hext").is_none();
    let live = spec == "live" || spec == "cdrom" || (spec == "auto" && no_image);
    if live && no_image && !any_media_image() {
        wait_for_usb_image();
    }
    let mut guard = VFS.lock();
    let vfs = guard.as_mut()?;

    if spec != "live" && spec != "ram" && spec != "cdrom" {
        if let Some(volume) = find_root_volume(&spec) {
            let device = KernelDevice::Block { disk: volume.disk, offset: volume.offset, sectors: volume.sectors };
            match Hext::mount(device, now()) {
                Ok(mut fs) => {
                    let replayed = fs.stats().replayed;
                    let mut files = 0;
                    let tree = read_tree(&mut fs, ROOT_INO, true, &mut files, true);
                    insert_tree(vfs, 0, tree, 0);
                    vfs.nodes[0].ino = ROOT_INO;
                    let overlay = crate::memory::cmdline_value("overlay").map(|v| v == "usr").unwrap_or(spec == "auto");
                    if overlay {
                        if let Some(module) = module_device() {
                            if let Ok(mut system) = Hext::mount(module, now()) {
                                if let Ok(Some(usr)) = system.lookup(ROOT_INO, "usr") {
                                    let usr_node = match child(vfs, 0, "usr") {
                                        Some(id) => id,
                                        None => vfs.mkdir(0, "/usr", 0).unwrap_or(0),
                                    };
                                    overlay_tree(vfs, &mut fs, &mut system, usr_node, usr);
                                }
                            }
                        }
                    }
                    vfs.tracking = true;
                    crate::memory::release_module("hext");
                    let description = format!(
                        "hext: mounted /dev/{} as / (persistent, {} files{})",
                        volume.name,
                        files,
                        if replayed { ", journal replayed" } else { "" }
                    );
                    let backend = Backend {
                        fs,
                        source: format!("/dev/{}", volume.name),
                        path: String::from("/"),
                        persistent: true,
                        errors: 0,
                        dev: 0,
                        point: 0,
                        disk: Some((volume.disk, volume.offset, volume.sectors)),
                        blocks: BTreeMap::new(),
                    };
                    let mut backends = BACKENDS.lock();
                    backends.push(backend);
                    refresh_table(&backends);
                    SYNC_REQUESTED.store(true, Ordering::Relaxed);
                    DISK_ROOT.store(true, Ordering::Relaxed);
                    *ROOT_DESCRIPTION.lock() = description.clone();
                    return Some(description);
                }
                Err(e) => crate::drivers::klog::log(&format!("hext: /dev/{} is damaged ({:?}), not mounting it", volume.name, e)),
            }
        } else if spec != "auto" {
            crate::drivers::klog::log(&format!("hext: root={} not found, falling back to the boot image", spec));
        }
    }

    if live {
        if let Some(description) = mount_media_root(vfs) {
            return Some(description);
        }
    }

    let device = module_device()?;
    match Hext::mount(device, now()) {
        Ok(mut fs) => {
            let mut files = 0;
            let tree = read_tree(&mut fs, ROOT_INO, true, &mut files, false);
            insert_tree(vfs, 0, tree, 0);
            vfs.nodes[0].ino = ROOT_INO;
            vfs.tracking = true;
            let stats = fs.stats();
            let row = MountRow {
                source: String::from("live"),
                path: String::from("/"),
                persistent: false,
                label: fs.label(),
                total_kb: stats.blocks as u64 * stats.block_size as u64 / 1024,
                free_kb: stats.free_blocks as u64 * stats.block_size as u64 / 1024,
                errors: 0,
                disk: None,
            };
            drop(fs);
            *LIVE_ROW.lock() = Some(row.clone());
            *TABLE.lock() = alloc::vec![row];
            let freed = crate::memory::release_module("hext");
            let description = format!("hext: loaded the boot image into RAM as / (live, {} files, {} MiB image released)", files, freed >> 20);
            *ROOT_DESCRIPTION.lock() = description.clone();
            Some(description)
        }
        Err(e) => {
            crate::drivers::klog::log(&format!("hext: boot image unusable ({:?})", e));
            None
        }
    }
}

const LIVE_IMAGE: &str = "/boot/hext.img";

fn mount_media_root(vfs: &mut Vfs) -> Option<String> {
    let found = *MEDIA_FOUND.lock();
    if let Some((index, offset, bytes)) = found {
        let disk = block::disk(index)?;
        DISK_ROOT.store(true, Ordering::Relaxed);
        let sectors = bytes / 512;
        let device = KernelDevice::Block { disk: index, offset, sectors };
        let mut fs = match Hext::mount(device, now()) {
            Ok(fs) => fs,
            Err(e) => {
                crate::drivers::klog::log(&format!("hext: {}{} on /dev/{} is unusable ({:?})", "", LIVE_IMAGE, disk.name, e));
                return None;
            }
        };
        let mut files = 0;
        let tree = read_tree(&mut fs, ROOT_INO, true, &mut files, true);
        insert_tree(vfs, 0, tree, 0);
        vfs.nodes[0].ino = ROOT_INO;
        vfs.tracking = true;
        let freed = crate::memory::release_module("hext");
        let backend = Backend {
            fs,
            source: format!("live:/dev/{}", disk.name),
            path: String::from("/"),
            persistent: false,
            errors: 0,
            dev: 0,
            point: 0,
            disk: Some((index, offset, sectors)),
            blocks: BTreeMap::new(),
        };
        let mut backends = BACKENDS.lock();
        backends.push(backend);
        refresh_table(&backends);
        drop(backends);
        let description = format!(
            "hext: live system from /dev/{}{} ({} files read on demand, changes stay in RAM{})",
            disk.name,
            LIVE_IMAGE,
            files,
            if freed > 0 { format!(", {} MiB boot image released", freed >> 20) } else { String::new() }
        );
        *ROOT_DESCRIPTION.lock() = description.clone();
        return Some(description);
    }
    None
}

pub fn root_is_live() -> bool {
    mount_table().first().map(|r| !r.persistent).unwrap_or(true)
}

struct Item {
    node: usize,
    parent: usize,
    name: String,
    parent_ino: u32,
    ino: u32,
    is_dir: bool,
    data: Option<Vec<u8>>,
    mode: u16,
    owner: u32,
    group: u32,
    meta: bool,
}

struct Work {
    removed: Vec<(u32, String)>,
    items: Vec<Item>,
}

fn collect(vfs: &mut Vfs, dir: usize, dev: u16, top: bool, items: &mut Vec<Item>) {
    let children: Vec<(String, usize)> = match &vfs.nodes[dir].kind {
        NodeKind::Dir(map) => map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        _ => return,
    };
    let dir_ino = vfs.nodes[dir].ino;
    for (name, id) in children {
        let node = &vfs.nodes[id];
        if node.ino == VOLATILE || node.dev != dev {
            continue;
        }
        let is_dir = matches!(node.kind, NodeKind::Dir(_));
        let is_file = matches!(node.kind, NodeKind::File(_));
        if !is_dir && !is_file {
            continue;
        }
        let fresh = node.ino == 0;
        if fresh || node.dirty != 0 {
            let data = match &node.kind {
                NodeKind::File(body) if fresh || node.dirty & DIRTY_DATA != 0 => Some(body.snapshot()),
                _ => None,
            };
            items.push(Item {
                node: id,
                parent: dir,
                name: name.clone(),
                parent_ino: dir_ino,
                ino: node.ino,
                is_dir,
                data,
                mode: node.mode,
                owner: node.owner,
                group: node.group,
                meta: fresh || node.dirty & DIRTY_META != 0,
            });
            vfs.nodes[id].dirty = 0;
        }
        if is_dir && !(top && VOLATILE_DIRS.contains(&name.as_str())) {
            collect(vfs, id, dev, false, items);
        }
    }
}

fn snapshot(dev: u16, point: usize) -> Option<Work> {
    let mut guard = VFS.lock();
    let vfs = guard.as_mut()?;
    if vfs.nodes[point].ino != ROOT_INO || vfs.nodes[point].dev != dev {
        return None;
    }
    let mut removed = Vec::new();
    vfs.removed.retain(|(d, ino, name)| {
        if *d == dev {
            removed.push((*ino, name.clone()));
            false
        } else {
            true
        }
    });
    let mut items = Vec::new();
    collect(vfs, point, dev, point == 0, &mut items);
    Some(Work { removed, items })
}

fn apply(backend: &mut Backend, work: &mut Work) -> (Vec<(usize, u32)>, Vec<usize>) {
    backend.blocks.clear();
    backend.fs.set_time(now());
    for (parent, name) in work.removed.drain(..) {
        let _ = backend.fs.remove_tree(parent, &name);
    }
    let mut assigned: BTreeMap<usize, u32> = BTreeMap::new();
    let mut failed = Vec::new();
    for item in work.items.iter_mut() {
        let parent_ino = if item.parent_ino == 0 { assigned.get(&item.parent).copied().unwrap_or(0) } else { item.parent_ino };
        if parent_ino == 0 {
            failed.push(item.node);
            continue;
        }
        if item.ino == 0 {
            let kind = if item.is_dir { S_IFDIR } else { S_IFREG };
            let created = match backend.fs.create(parent_ino, &item.name, kind | item.mode, item.owner, item.group) {
                Ok(ino) => Some(ino),
                Err(hext::Error::Exists) => {
                    let _ = backend.fs.remove_tree(parent_ino, &item.name);
                    backend.fs.create(parent_ino, &item.name, kind | item.mode, item.owner, item.group).ok()
                }
                Err(_) => None,
            };
            match created {
                Some(ino) => {
                    item.ino = ino;
                    assigned.insert(item.node, ino);
                }
                None => {
                    failed.push(item.node);
                    continue;
                }
            }
        }
        if let Some(data) = item.data.take() {
            if backend.fs.write_file(item.ino, &data).is_err() {
                failed.push(item.node);
                continue;
            }
        }
        if item.meta && backend.fs.set_attributes(item.ino, item.mode, item.owner, item.group).is_err() {
            failed.push(item.node);
        }
        if crate::arch::interrupts_enabled() {
            crate::task::yield_now();
        }
    }
    if let Err(e) = backend.fs.commit() {
        crate::drivers::klog::log(&format!("hext: commit on {} failed: {:?}", backend.source, e));
        failed.extend(work.items.iter().map(|i| i.node));
    }
    if !failed.is_empty() {
        backend.errors += failed.len() as u32;
    }
    (assigned.into_iter().collect(), failed)
}

fn finish(work: &Work, dev: u16, assigned: &[(usize, u32)], failed: &[usize]) {
    let mut guard = VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    for (node, ino) in assigned {
        let item = work.items.iter().find(|i| i.node == *node).unwrap();
        let attached = child(vfs, item.parent, &item.name) == Some(*node) && vfs.nodes[*node].dev == dev;
        vfs.nodes[*node].ino = *ino;
        if !attached {
            let parent_ino = vfs.nodes[item.parent].ino;
            if parent_ino != 0 && parent_ino != VOLATILE {
                vfs.removed.push((dev, parent_ino, item.name.clone()));
            }
        }
    }
    for node in failed {
        if vfs.nodes[*node].ino != VOLATILE {
            vfs.nodes[*node].dirty |= DIRTY_DATA | DIRTY_META;
        }
    }
    for item in work.items.iter() {
        if item.is_dir || failed.contains(&item.node) || item.ino == 0 {
            continue;
        }
        if vfs.nodes[item.node].dev == dev && vfs.nodes[item.node].dirty == 0 {
            vfs.set_backing(item.node, item.ino);
        }
    }
}

fn lock_backends() -> spin::MutexGuard<'static, Vec<Backend>> {
    loop {
        if let Some(guard) = BACKENDS.try_lock() {
            return guard;
        }
        if crate::arch::interrupts_enabled() && crate::task::current_pid() != 0 {
            crate::task::yield_now();
        } else {
            core::hint::spin_loop();
        }
    }
}

fn sync_one(index: usize) -> bool {
    let (dev, point) = {
        let backends = lock_backends();
        match backends.get(index) {
            Some(b) if b.persistent => (b.dev, b.point),
            _ => return true,
        }
    };
    let Some(mut work) = snapshot(dev, point) else {
        return true;
    };
    let outcome = {
        let mut backends = lock_backends();
        let Some(backend) = backends.get_mut(index) else {
            return true;
        };
        if work.items.is_empty() && work.removed.is_empty() && !backend.fs.dirty() {
            None
        } else {
            Some(apply(backend, &mut work))
        }
    };
    let Some((assigned, failed)) = outcome else {
        return true;
    };
    finish(&work, dev, &assigned, &failed);
    failed.is_empty()
}

fn run_sync() {
    SYNC_REQUESTED.store(false, Ordering::Relaxed);
    let count = lock_backends().len();
    let mut ok = true;
    for index in 0..count {
        ok &= sync_one(index);
    }
    let backends = lock_backends();
    refresh_table(&backends);
    drop(backends);
    if !ok {
        SYNC_REQUESTED.store(true, Ordering::Relaxed);
    }
}

pub fn load_range(dev: u16, ino: u32, offset: u64, out: &mut [u8]) -> Option<usize> {
    let mut backends = lock_backends();
    let backend = backends.iter_mut().find(|b| b.dev == dev && b.disk.is_some())?;
    if !backend.blocks.contains_key(&ino) {
        let list = backend.fs.file_blocks(ino).ok()?;
        if backend.blocks.len() >= 16 {
            let first = *backend.blocks.keys().next()?;
            backend.blocks.remove(&first);
        }
        backend.blocks.insert(ino, list);
    }
    let (blocks, size) = backend.blocks.get(&ino)?.clone();
    backend.fs.read_range(&blocks, size, offset, out).ok()
}

pub fn load_backing(dev: u16, ino: u32) -> Option<Vec<u8>> {
    let mut backends = lock_backends();
    let backend = backends.iter_mut().find(|b| b.dev == dev && b.disk.is_some())?;
    backend.fs.read_file(ino).ok()
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static DAEMON: AtomicBool = AtomicBool::new(false);

extern "C" fn daemon(_: u64) -> ! {
    loop {
        let wanted = GENERATION.load(Ordering::Relaxed);
        let due = SYNC_REQUESTED.load(Ordering::Relaxed)
            && crate::task::uptime_ms().saturating_sub(LAST_SYNC.load(Ordering::Relaxed)) >= SYNC_INTERVAL_MS;
        if wanted != COMPLETED.load(Ordering::Relaxed) || due {
            LAST_SYNC.store(crate::task::uptime_ms(), Ordering::Relaxed);
            run_sync();
            COMPLETED.store(wanted, Ordering::Relaxed);
        } else {
            let now = crate::task::uptime_ms();
            if now.saturating_sub(LAST_TRIM.load(Ordering::Relaxed)) >= TRIM_MS {
                LAST_TRIM.store(now, Ordering::Relaxed);
                trim_file_cache();
            }
            if now.saturating_sub(LAST_ORPHAN_SCAN.load(Ordering::Relaxed)) >= ORPHAN_SCAN_MS {
                LAST_ORPHAN_SCAN.store(now, Ordering::Relaxed);
                release_orphans();
            }
            crate::task::sleep_ticks(crate::task::TICK_HZ / 10);
        }
    }
}

const FILE_CACHE_FLOOR: usize = 512 * 1024;
const FILE_CACHE_FLOOR_MAX: usize = 4 * 1024 * 1024;
const FILE_CACHE_MAX: usize = 96 * 1024 * 1024;
const CACHE_IDLE_MS: u64 = 4000;
const ORPHAN_SCAN_MS: u64 = 3000;
const TRIM_MS: u64 = 1000;
static LAST_TRIM: AtomicU64 = AtomicU64::new(0);
static LAST_ORPHAN_SCAN: AtomicU64 = AtomicU64::new(0);
static ORPHAN_CANDIDATES: Mutex<alloc::collections::BTreeSet<usize>> = Mutex::new(alloc::collections::BTreeSet::new());

static DISK_ROOT: AtomicBool = AtomicBool::new(false);

fn file_cache_limit() -> usize {
    let (free, total) = crate::memory::frame::memory_info();
    if DISK_ROOT.load(Ordering::Relaxed) {
        let ceiling = (total / 4).max(FILE_CACHE_FLOOR_MAX);
        return (free / 2).clamp(FILE_CACHE_FLOOR_MAX, ceiling);
    }
    let floor = (total / 128).clamp(FILE_CACHE_FLOOR, FILE_CACHE_FLOOR_MAX);
    let now = crate::task::uptime_ms();
    if now.saturating_sub(crate::fs::last_file_load_ms()) >= CACHE_IDLE_MS {
        return floor;
    }
    let ceiling = (total / 8).clamp(floor, FILE_CACHE_MAX);
    (free / 4).clamp(floor, ceiling)
}

fn trim_file_cache() {
    if crate::task::uptime_ms().saturating_sub(crate::fs::last_file_load_ms()) >= CACHE_IDLE_MS {
        block::trim_read_cache(16);
    }
    let limit = file_cache_limit();
    let mut guard = VFS.lock();
    if let Some(vfs) = guard.as_mut() {
        vfs.evict_files(limit);
    }
}

fn release_orphans() {
    let mut open = crate::task::open_file_nodes();
    crate::net::unix::inflight_file_nodes(&mut open);
    let pinned: Vec<usize> = SAVED.lock().values().flat_map(|save| save.children.values().copied().collect::<Vec<_>>()).collect();
    let mut candidates = ORPHAN_CANDIDATES.lock();
    let mut guard = VFS.lock();
    if let Some(vfs) = guard.as_mut() {
        vfs.release_orphans(&open, &pinned, &mut candidates);
    }
}

pub fn start_daemon() {
    if !DAEMON.swap(true, Ordering::Relaxed) {
        crate::task::spawn_kernel_thread("hextd", 0, daemon, 0);
    }
}

pub fn sync() {
    if !DAEMON.load(Ordering::Relaxed) || crate::task::current_pid() == 0 {
        run_sync();
        return;
    }
    SYNC_REQUESTED.store(true, Ordering::Relaxed);
    let target = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    while COMPLETED.load(Ordering::Relaxed) < target {
        crate::task::sleep_ticks(2);
    }
}

pub fn request_sync() {
    SYNC_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn root_description() -> String {
    ROOT_DESCRIPTION.lock().clone()
}

pub fn mkfs(device: &str, label: &str) -> Result<String, &'static str> {
    let volume = block::find_volume(device).ok_or("no such block device")?;
    if overlaps_mounted(volume.disk, volume.offset, volume.sectors) {
        return Err("the device is mounted");
    }
    let target = KernelDevice::Block { disk: volume.disk, offset: volume.offset, sectors: volume.sectors };
    let label = if label.is_empty() { "hamix" } else { label };
    let fs = Hext::format(
        target,
        FormatOptions { label: String::from(label), seed: crate::task::ticks() ^ (now() as u64) << 20 ^ volume.offset, now: now() },
    )
    .map_err(|_| "format failed")?;
    let stats = fs.stats();
    drop(fs);
    let _ = block::flush(volume.disk);
    block::rescan();
    super::refresh_block_nodes();
    Ok(format!(
        "/dev/{}: hext, {} MiB, block size {}, journal {} blocks",
        volume.name,
        stats.blocks as u64 * stats.block_size as u64 >> 20,
        stats.block_size,
        stats.journal_blocks
    ))
}

pub(crate) fn clear_subtree(vfs: &mut Vfs, id: usize) {
    let children: Vec<usize> = match &mut vfs.nodes[id].kind {
        NodeKind::Dir(map) => core::mem::take(map).into_values().collect(),
        NodeKind::File(body) => {
            body.discard();
            crate::memory::page_cache::invalidate(id);
            Vec::new()
        }
        _ => Vec::new(),
    };
    for child in children {
        clear_subtree(vfs, child);
    }
}

pub fn mount(device: &str, path: &str) -> Result<String, &'static str> {
    let volume = block::find_volume(device).ok_or("no such block device")?;
    if overlaps_mounted(volume.disk, volume.offset, volume.sectors) {
        return Err("the device is already mounted");
    }
    let target = KernelDevice::Block { disk: volume.disk, offset: volume.offset, sectors: volume.sectors };
    let mut probe_device = KernelDevice::Block { disk: volume.disk, offset: volume.offset, sectors: volume.sectors };
    if !Hext::probe(&mut probe_device) {
        return Err("no hext filesystem on the device");
    }
    let mut fs = Hext::mount(target, now()).map_err(|_| "the filesystem is damaged")?;
    let mut files = 0;
    let tree = read_tree(&mut fs, ROOT_INO, false, &mut files, true);
    let dev = NEXT_DEV.fetch_add(1, Ordering::Relaxed);
    let backend = {
        let mut guard = VFS.lock();
        let vfs = guard.as_mut().ok_or("no root filesystem")?;
        let point = vfs.resolve(0, path).ok_or("mount point does not exist")?;
        if point == 0 || !vfs.is_dir(point) {
            return Err("mount point must be a directory other than /");
        }
        if is_mount_point(point) {
            return Err("something is already mounted there");
        }
        let children = match &mut vfs.nodes[point].kind {
            NodeKind::Dir(map) => core::mem::take(map),
            _ => BTreeMap::new(),
        };
        let save = MountSave { ino: vfs.nodes[point].ino, dev: vfs.nodes[point].dev, dirty: vfs.nodes[point].dirty, children };
        SAVED.lock().insert(point, save);
        vfs.nodes[point].ino = ROOT_INO;
        vfs.nodes[point].dev = dev;
        vfs.nodes[point].dirty = 0;
        insert_tree(vfs, point, tree, dev);
        crate::arch::without_interrupts(|| POINTS.lock().insert(point, dev));
        Backend {
            fs,
            source: format!("/dev/{}", volume.name),
            path: vfs.path_of(point),
            persistent: true,
            errors: 0,
            dev,
            point,
            disk: Some((volume.disk, volume.offset, volume.sectors)),
            blocks: BTreeMap::new(),
        }
    };
    let mut backends = lock_backends();
    backends.push(backend);
    refresh_table(&backends);
    drop(backends);
    start_daemon();
    Ok(format!("/dev/{} mounted on {} ({} files)", volume.name, path, files))
}

pub fn umount(path: &str) -> Result<(), &'static str> {
    let point = {
        let guard = VFS.lock();
        let vfs = guard.as_ref().ok_or("no root filesystem")?;
        vfs.resolve(0, path).ok_or("not mounted")?
    };
    let (index, prefix) = {
        let backends = lock_backends();
        let index = backends.iter().position(|b| b.point == point && b.dev != 0).ok_or("not mounted")?;
        (index, backends[index].path.clone())
    };
    let busy = crate::task::with_tasks(|tasks| tasks.values().any(|t| t.state != crate::task::State::Zombie && (t.cwd == prefix || t.cwd.starts_with(&format!("{}/", prefix)))));
    if busy {
        return Err("target is busy (a process is working inside it)");
    }
    if !sync_one(index) {
        sync_one(index);
    }
    let mut backends = lock_backends();
    if index >= backends.len() {
        return Err("not mounted");
    }
    let _ = backends[index].fs.commit();
    let _ = backends[index].fs.device().flush();
    let backend = backends.remove(index);
    drop(backends);
    {
        let mut guard = VFS.lock();
        if let Some(vfs) = guard.as_mut() {
            clear_subtree(vfs, point);
            let dev = backend.dev;
            vfs.removed.retain(|(d, _, _)| *d != dev);
            if let Some(save) = SAVED.lock().remove(&point) {
                vfs.nodes[point].kind = NodeKind::Dir(save.children);
                vfs.nodes[point].ino = save.ino;
                vfs.nodes[point].dev = save.dev;
                vfs.nodes[point].dirty = save.dirty;
            }
        }
    }
    crate::arch::without_interrupts(|| POINTS.lock().remove(&point));
    let backends = lock_backends();
    refresh_table(&backends);
    Ok(())
}
