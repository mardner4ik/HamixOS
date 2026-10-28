use alloc::collections::BTreeMap;
use alloc::string::String;
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::drivers::block::{self, Volume};

pub const SCSI_DISK_MAJOR: u32 = 8;
pub const SCSI_CDROM_MAJOR: u32 = 11;

const SECTOR: u64 = 512;
const CHUNK: u64 = 1 << 20;

const BLKRRPART: u64 = 0x125F;
const BLKGETSIZE: u64 = 0x1260;
const BLKFLSBUF: u64 = 0x1261;
const BLKSSZGET: u64 = 0x1268;
const BLKPBSZGET: u64 = 0x127B;
const BLKIOMIN: u64 = 0x1278;
const BLKIOOPT: u64 = 0x1279;
const BLKGETSIZE64: u64 = 0x8008_1272;

use crate::errno::EIO;
use crate::errno::ENODEV;
use crate::errno::EBUSY;
use crate::errno::EINVAL;
use crate::errno::ENOTTY;
use crate::errno::EFAULT;

struct Open {
    name: String,
    pos: u64,
}

static OPEN: Mutex<BTreeMap<u32, Open>> = Mutex::new(BTreeMap::new());

fn with<R>(f: impl FnOnce(&mut BTreeMap<u32, Open>) -> R) -> R {
    without_interrupts(|| f(&mut OPEN.lock()))
}

pub fn opened(id: u32, name: &str) -> i64 {
    if block::find_volume(name).is_none() {
        return ENODEV;
    }
    with(|t| t.insert(id, Open { name: String::from(name), pos: 0 }));
    0
}

pub fn closed(id: u32) {
    with(|t| t.remove(&id));
}

pub fn name(id: u32) -> Option<String> {
    with(|t| t.get(&id).map(|o| o.name.clone()))
}

fn volume(id: u32) -> Result<Volume, i64> {
    let name = name(id).ok_or(ENODEV)?;
    block::find_volume(&name).ok_or(ENODEV)
}

pub fn rdev(id: u32) -> u64 {
    let Some(name) = name(id) else {
        return 0;
    };
    if let Some(n) = name.strip_prefix("sr").and_then(|n| n.parse::<u32>().ok()) {
        return crate::fs::encode_dev(SCSI_CDROM_MAJOR, n);
    }
    minor_of(&name).map(|minor| crate::fs::encode_dev(SCSI_DISK_MAJOR, minor)).unwrap_or(0)
}

pub fn minor_of(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("sd")?;
    let letters = rest.trim_end_matches(|c: char| c.is_ascii_digit());
    let partition: u32 = rest[letters.len()..].parse().unwrap_or(0);
    let bytes = letters.as_bytes();
    let disk = match bytes {
        [a] => (a - b'a') as u32,
        [a, b] => (a - b'a' + 1) as u32 * 26 + (b - b'a') as u32,
        _ => return None,
    };
    Some(disk * 16 + partition.min(15))
}

pub fn size(id: u32) -> u64 {
    volume(id).map(|v| v.sectors * SECTOR).unwrap_or(0)
}

fn transfer(volume: &Volume, offset: u64, data: &mut [u8], write: bool) -> Result<usize, i64> {
    let total = volume.sectors * SECTOR;
    if offset >= total {
        return Ok(0);
    }
    let len = (data.len() as u64).min(total - offset) as usize;
    let mut done = 0usize;
    while done < len {
        let at = offset + done as u64;
        let first = at / SECTOR;
        let skip = (at % SECTOR) as usize;
        let span = ((len - done) as u64).min(CHUNK) as usize;
        let sectors = (skip + span).div_ceil(SECTOR as usize) as u64;
        let mut buf = alloc::vec![0u8; (sectors * SECTOR) as usize];
        let lba = volume.offset + first;
        let partial = skip != 0 || span % SECTOR as usize != 0;
        if !write || partial {
            block::read(volume.disk, lba, &mut buf).map_err(|_| EIO)?;
        }
        if write {
            if crate::fs::hextfs::overlaps_mounted(volume.disk, lba, sectors) {
                return Err(EBUSY);
            }
            buf[skip..skip + span].copy_from_slice(&data[done..done + span]);
            block::write(volume.disk, lba, &buf).map_err(|_| EIO)?;
        } else {
            data[done..done + span].copy_from_slice(&buf[skip..skip + span]);
        }
        done += span;
    }
    if write {
        let _ = block::flush(volume.disk);
    }
    Ok(done)
}

pub fn read_at(id: u32, offset: u64, out: &mut [u8]) -> i64 {
    match volume(id).and_then(|v| transfer(&v, offset, out, false)) {
        Ok(n) => n as i64,
        Err(e) => e,
    }
}

pub fn write_at(id: u32, offset: u64, data: &[u8]) -> i64 {
    let mut copy = data.to_vec();
    match volume(id).and_then(|v| transfer(&v, offset, &mut copy, true)) {
        Ok(n) => n as i64,
        Err(e) => e,
    }
}

pub fn read(id: u32, out: &mut [u8]) -> i64 {
    let pos = with(|t| t.get(&id).map(|o| o.pos)).unwrap_or(0);
    let n = read_at(id, pos, out);
    if n > 0 {
        with(|t| t.get_mut(&id).map(|o| o.pos = pos + n as u64));
    }
    n
}

pub fn write(id: u32, data: &[u8]) -> i64 {
    let pos = with(|t| t.get(&id).map(|o| o.pos)).unwrap_or(0);
    let n = write_at(id, pos, data);
    if n > 0 {
        with(|t| t.get_mut(&id).map(|o| o.pos = pos + n as u64));
    }
    n
}

pub fn seek(id: u32, offset: i64, whence: u64) -> i64 {
    let end = size(id) as i64;
    with(|t| {
        let Some(open) = t.get_mut(&id) else {
            return EINVAL;
        };
        let base = match whence {
            0 => 0,
            1 => open.pos as i64,
            2 => end,
            _ => return EINVAL,
        };
        let target = base + offset;
        if target < 0 {
            return EINVAL;
        }
        open.pos = target as u64;
        target
    })
}

pub fn ioctl(id: u32, request: u64, arg: u64) -> i64 {
    let put = |bytes: &[u8]| -> i64 { if crate::syscall::copy_out(arg, bytes.len() as u64, bytes) < 0 { EFAULT } else { 0 } };
    match request & 0xFFFF_FFFF {
        BLKGETSIZE64 => put(&size(id).to_le_bytes()),
        BLKGETSIZE => put(&(size(id) / SECTOR).to_le_bytes()),
        BLKSSZGET | BLKPBSZGET | BLKIOMIN => put(&(SECTOR as u32).to_le_bytes()),
        BLKIOOPT => put(&0u32.to_le_bytes()),
        BLKFLSBUF => match volume(id) {
            Ok(v) => {
                let _ = block::flush(v.disk);
                0
            }
            Err(e) => e,
        },
        BLKRRPART => {
            if volume(id).is_err() {
                return EINVAL;
            }
            block::rescan();
            crate::fs::refresh_block_nodes();
            0
        }
        _ => ENOTTY,
    }
}

fn put(vfs: &mut crate::fs::Vfs, dir: &str, files: &[(&str, String)]) {
    if vfs.mkdir_all(dir, 0).is_err() {
        return;
    }
    for (name, value) in files {
        let _ = vfs.write(0, &alloc::format!("{}/{}", dir, name), value.as_bytes(), false, 0);
    }
}

fn link(vfs: &mut crate::fs::Vfs, path: &str, target: &str) {
    let mut body = alloc::vec::Vec::from(crate::fs::LINK_MAGIC);
    body.extend_from_slice(target.as_bytes());
    let _ = vfs.write(0, path, &body, false, 0);
    let _ = vfs.chmod(0, path, 0, 0o777);
}

fn probe_properties(probe: Option<&block::FsProbe>) -> String {
    match probe {
        Some(p) => alloc::format!("ID_FS_TYPE={}\nID_FS_LABEL={}\nID_FS_UUID={}\n", p.kind, p.label, p.uuid),
        None => String::new(),
    }
}

pub fn populate_sysfs(vfs: &mut crate::fs::Vfs) {
    let tracking = vfs.tracking;
    vfs.tracking = false;
    for dir in ["/sys/block", "/sys/class/block"] {
        let _ = vfs.remove(0, dir, 0);
        let _ = vfs.mkdir_all(dir, 0);
    }
    let _ = vfs.mkdir_all("/sys/dev/block", 0);
    let infos = block::volume_infos();
    for (index, disk) in block::disks().iter().enumerate() {
        let Some(minor) = minor_of(&disk.name) else {
            continue;
        };
        let base = alloc::format!("/sys/block/{}", disk.name);
        let whole = infos.iter().find(|v| v.volume.disk == index && v.partition.is_none());
        put(
            vfs,
            &base,
            &[
                ("dev", alloc::format!("{}:{}\n", SCSI_DISK_MAJOR, minor)),
                ("size", alloc::format!("{}\n", disk.sectors)),
                ("ro", String::from("0\n")),
                ("removable", alloc::format!("{}\n", (disk.bus == "usb" || disk.name.starts_with("sr")) as u8)),
                (
                    "uevent",
                    alloc::format!(
                        "MAJOR={}\nMINOR={}\nDEVNAME={}\nDEVTYPE=disk\nID_BUS={}\n{}",
                        SCSI_DISK_MAJOR,
                        minor,
                        disk.name,
                        disk.bus,
                        probe_properties(whole.and_then(|w| w.probe.as_ref()))
                    ),
                ),
            ],
        );
        put(vfs, &alloc::format!("{}/device", base), &[("model", alloc::format!("{}\n", disk.model)), ("vendor", String::from("\n"))]);
        put(vfs, &alloc::format!("{}/queue", base), &[("logical_block_size", String::from("512\n")), ("hw_sector_size", String::from("512\n")), ("rotational", String::from("0\n"))]);
        link(vfs, &alloc::format!("/sys/class/block/{}", disk.name), &base);
        link(vfs, &alloc::format!("/sys/dev/block/{}:{}", SCSI_DISK_MAJOR, minor), &base);
        for info in infos.iter().filter(|v| v.volume.disk == index) {
            let Some(part) = &info.partition else {
                continue;
            };
            let part_minor = minor + part.number.min(15);
            let dir = alloc::format!("{}/{}", base, part.name);
            put(
                vfs,
                &dir,
                &[
                    ("dev", alloc::format!("{}:{}\n", SCSI_DISK_MAJOR, part_minor)),
                    ("partition", alloc::format!("{}\n", part.number)),
                    ("start", alloc::format!("{}\n", part.start)),
                    ("size", alloc::format!("{}\n", part.sectors)),
                    ("ro", String::from("0\n")),
                    (
                        "uevent",
                        alloc::format!(
                            "MAJOR={}\nMINOR={}\nDEVNAME={}\nDEVTYPE=partition\nPARTN={}\nID_PART_ENTRY_TYPE={}\nID_PART_ENTRY_FLAGS={}\n{}",
                            SCSI_DISK_MAJOR,
                            part_minor,
                            part.name,
                            part.number,
                            part.kind,
                            if part.bootable { "0x80" } else { "0x0" },
                            probe_properties(info.probe.as_ref())
                        ),
                    ),
                ],
            );
            link(vfs, &alloc::format!("/sys/class/block/{}", part.name), &dir);
            link(vfs, &alloc::format!("/sys/dev/block/{}:{}", SCSI_DISK_MAJOR, part_minor), &dir);
        }
    }
    vfs.tracking = tracking;
}
