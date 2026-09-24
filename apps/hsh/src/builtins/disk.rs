use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{fs, sys};

use super::files::human;
use super::{parse_opts, Builtin};
use crate::shell::{Io, Shell};
use crate::{errln, outln};

pub const BOOT_DIR: &str = "/usr/lib/hamix/boot";

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "diskls", usage: "diskls [-f]", help: "list disks and partitions (like lsblk)", group: "disk", run: diskls },
        Builtin { name: "lsblk", usage: "lsblk", help: "same as diskls", group: "disk", run: diskls },
        Builtin { name: "blkid", usage: "blkid [device]", help: "filesystem types, labels and UUIDs", group: "disk", run: blkid },
        Builtin { name: "diskpart", usage: "diskpart disk [print|wipe|mbr|add [size|rest] [type]|delete N|boot N]", help: "edit an MBR partition table", group: "disk", run: diskpart },
        Builtin { name: "mkfs.hext", usage: "mkfs.hext [-L label] device", help: "create a hext filesystem", group: "disk", run: mkfs },
        Builtin { name: "mkfs", usage: "mkfs [-t hext] [-L label] device", help: "create a filesystem", group: "disk", run: mkfs },
        Builtin { name: "mount", usage: "mount [device dir]", help: "mount a hext volume or list mounts", group: "disk", run: mount },
        Builtin { name: "umount", usage: "umount dir|device", help: "flush and detach a mounted volume", group: "disk", run: umount },
        Builtin { name: "df", usage: "df [-h]", help: "space on mounted volumes", group: "disk", run: df },
        Builtin { name: "bootinstall", usage: "bootinstall disk [--root /mnt]", help: "install the GRUB boot loader and grub.cfg", group: "disk", run: bootinstall },
    ]
}

#[derive(Clone, Default)]
pub struct DiskRow {
    pub name: String,
    pub sectors: u64,
    pub bus: String,
    pub model: String,
    pub fstype: String,
    pub label: String,
    pub uuid: String,
    pub mount: String,
    pub partitions: Vec<PartRow>,
}

#[derive(Clone, Default)]
pub struct PartRow {
    pub name: String,
    pub start: u64,
    pub sectors: u64,
    pub kind: String,
    pub fstype: String,
    pub label: String,
    pub uuid: String,
    pub mount: String,
    pub boot: bool,
}

pub fn disks() -> Vec<DiskRow> {
    let text = sys::disk_listing();
    let mut out: Vec<DiskRow> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let get = |i: usize| String::from(f.get(i).copied().unwrap_or(""));
        match f.first().copied() {
            Some("disk") => out.push(DiskRow {
                name: get(1),
                sectors: get(2).parse().unwrap_or(0),
                bus: get(3),
                model: get(4),
                fstype: get(5),
                label: get(6),
                uuid: get(7),
                mount: get(8),
                partitions: Vec::new(),
            }),
            Some("part") => {
                let row = PartRow {
                    name: get(1),
                    start: get(3).parse().unwrap_or(0),
                    sectors: get(4).parse().unwrap_or(0),
                    kind: get(5),
                    fstype: get(6),
                    label: get(7),
                    uuid: get(8),
                    mount: get(9),
                    boot: get(10) == "boot",
                };
                let parent = get(2);
                if let Some(disk) = out.iter_mut().find(|d| d.name == parent) {
                    disk.partitions.push(row);
                }
            }
            _ => {}
        }
    }
    out
}

fn diskls(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let full = args.iter().any(|a| a == "-f" || a == "--fs");
    let list = disks();
    if list.is_empty() {
        outln!(io, "no disks found");
        return 0;
    }
    if full {
        outln!(io, "\x1b[1m{:<9} {:>6} {:<4} {:<6} {:<36} {}\x1b[0m", "NAME", "SIZE", "TYPE", "FSTYPE", "UUID", "MOUNTPOINT");
    } else {
        outln!(io, "\x1b[1m{:<10} {:>7} {:<5} {:<7} {}\x1b[0m", "NAME", "SIZE", "TYPE", "FSTYPE", "MOUNTPOINT");
    }
    for disk in &list {
        let size = human(disk.sectors * 512);
        if full {
            outln!(io, "{:<9} {:>6} {:<4} {:<6} {:<36} {}", disk.name, size, "disk", disk.fstype, disk.uuid, disk.mount);
        } else {
            outln!(io, "{:<10} {:>7} {:<5} {:<7} {}", disk.name, size, "disk", disk.fstype, disk.mount);
        }
        for (i, part) in disk.partitions.iter().enumerate() {
            let branch = if i + 1 == disk.partitions.len() { "`-" } else { "|-" };
            let name = format!("{}{}", branch, part.name);
            let size = human(part.sectors * 512);
            if full {
                outln!(io, "{:<9} {:>6} {:<4} {:<6} {:<36} {}", name, size, "part", part.fstype, part.uuid, part.mount);
            } else {
                outln!(io, "{:<10} {:>7} {:<5} {:<7} {}", name, size, "part", part.fstype, part.mount);
            }
        }
    }
    0
}

fn blkid(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let filter = args.get(1).map(|s| String::from(s.trim_start_matches("/dev/")));
    for disk in disks() {
        let mut rows: Vec<(String, String, String, String)> = Vec::new();
        if !disk.fstype.is_empty() {
            rows.push((disk.name.clone(), disk.fstype.clone(), disk.label.clone(), disk.uuid.clone()));
        }
        for p in &disk.partitions {
            if !p.fstype.is_empty() {
                rows.push((p.name.clone(), p.fstype.clone(), p.label.clone(), p.uuid.clone()));
            }
        }
        for (name, kind, label, uuid) in rows {
            if filter.as_ref().map(|f| f != &name).unwrap_or(false) {
                continue;
            }
            if filter.is_some() && args.iter().any(|a| a == "-s") {
                outln!(io, "{}", uuid);
                continue;
            }
            outln!(io, "/dev/{}: LABEL=\"{}\" UUID=\"{}\" TYPE=\"{}\"", name, label, uuid, kind);
        }
    }
    0
}

fn parse_size(text: &str, total: u64) -> Option<u64> {
    if text == "rest" || text == "100%" {
        return Some(total);
    }
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let value: u64 = number.parse().ok()?;
    let multiplier = match unit.trim_end_matches("iB").trim_end_matches('B') {
        "" | "s" => return Some(value),
        "K" | "k" => 1u64 << 10,
        "M" | "m" => 1 << 20,
        "G" | "g" => 1 << 30,
        "T" | "t" => 1 << 40,
        _ => return None,
    };
    Some(value * multiplier / 512)
}

fn type_byte(name: &str) -> Option<u8> {
    Some(match name {
        "hext" | "linux" | "83" => 0x83,
        "swap" | "82" => 0x82,
        "fat32" | "vfat" | "0c" => 0x0C,
        "efi" | "ef" => 0xEF,
        "ntfs" | "07" => 0x07,
        _ => return None,
    })
}

fn put_entry(mbr: &mut [u8], slot: usize, boot: bool, kind: u8, start: u64, sectors: u64) {
    let e = &mut mbr[446 + slot * 16..462 + slot * 16];
    e.fill(0);
    e[0] = if boot { 0x80 } else { 0 };
    e[1] = 0xFE;
    e[2] = 0xFF;
    e[3] = 0xFF;
    e[4] = kind;
    e[5] = 0xFE;
    e[6] = 0xFF;
    e[7] = 0xFF;
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(sectors.min(u32::MAX as u64) as u32).to_le_bytes());
}

fn read_sectors(device: &str, lba: u64, count: usize) -> Result<Vec<u8>, i64> {
    let mut buf = alloc::vec![0u8; count * 512];
    let r = sys::disk_read(device, lba, &mut buf);
    if r < 0 { Err(r) } else { Ok(buf) }
}

fn diskpart(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(disk_name) = args.get(1).map(|d| String::from(d.trim_start_matches("/dev/"))) else {
        errln!(io, "usage: diskpart disk [print|wipe|mbr|add [size|rest] [type]|delete N|boot N]");
        return 1;
    };
    let Some(disk) = disks().into_iter().find(|d| d.name == disk_name) else {
        errln!(io, "diskpart: {}: no such disk", disk_name);
        return 1;
    };
    let action = args.get(2).map(|s| s.as_str()).unwrap_or("print");
    let fail = |io: Io, what: &str, e: i64| {
        errln!(io, "diskpart: {}: {}{}", what, sys::error_name(e), if e == -1 { " (try sudo)" } else if e == -16 { " (unmount it first)" } else { "" });
        1
    };
    match action {
        "print" => {
            outln!(io, "Disk /dev/{}: {} ({} sectors), {} {}", disk.name, human(disk.sectors * 512), disk.sectors, disk.bus, disk.model);
            if disk.partitions.is_empty() {
                outln!(io, "no partitions");
            } else {
                outln!(io, "\x1b[1m{:<8} {:<4} {:>10} {:>10} {:>7} {:<8} {}\x1b[0m", "DEVICE", "BOOT", "START", "SECTORS", "SIZE", "TYPE", "FS");
                for p in &disk.partitions {
                    outln!(io, "{:<8} {:<4} {:>10} {:>10} {:>7} {:<8} {}", p.name, if p.boot { "*" } else { "" }, p.start, p.sectors, human(p.sectors * 512), p.kind, p.fstype);
                }
            }
            0
        }
        "wipe" => {
            let zeros = alloc::vec![0u8; 2048 * 512];
            let r = sys::disk_write(&disk.name, 0, &zeros);
            if r < 0 {
                return fail(io, "wipe", r);
            }
            let tail = alloc::vec![0u8; 34 * 512];
            if disk.sectors > 4096 {
                sys::disk_write(&disk.name, disk.sectors - 34, &tail);
            }
            let gap = alloc::vec![0u8; 8 * 512];
            if disk.sectors > 4096 {
                sys::disk_write(&disk.name, 2048, &gap);
            }
            sys::disk_rescan();
            outln!(io, "/dev/{}: partition table and boot area erased", disk.name);
            0
        }
        "mbr" | "mklabel" => {
            let mut mbr = match read_sectors(&disk.name, 0, 1) {
                Ok(m) => m,
                Err(e) => return fail(io, "read", e),
            };
            mbr[440..510].fill(0);
            let signature = hamix_std::users::random_salt();
            mbr[440..444].copy_from_slice(&signature.to_le_bytes());
            mbr[510] = 0x55;
            mbr[511] = 0xAA;
            let r = sys::disk_write(&disk.name, 0, &mbr);
            if r < 0 {
                return fail(io, "write", r);
            }
            sys::disk_rescan();
            outln!(io, "/dev/{}: new empty MBR partition table", disk.name);
            0
        }
        "add" | "new" => {
            let mut mbr = match read_sectors(&disk.name, 0, 1) {
                Ok(m) => m,
                Err(e) => return fail(io, "read", e),
            };
            if mbr[510] != 0x55 || mbr[511] != 0xAA {
                errln!(io, "diskpart: /dev/{} has no MBR -- run: diskpart {} mbr", disk.name, disk.name);
                return 1;
            }
            let Some(slot) = (0..4).find(|i| mbr[446 + i * 16 + 4] == 0) else {
                errln!(io, "diskpart: all four primary slots are used");
                return 1;
            };
            let mut start = 2048u64;
            for i in 0..4 {
                let e = &mbr[446 + i * 16..462 + i * 16];
                if e[4] != 0 {
                    let s = u32::from_le_bytes(e[8..12].try_into().unwrap()) as u64;
                    let n = u32::from_le_bytes(e[12..16].try_into().unwrap()) as u64;
                    start = start.max((s + n).div_ceil(2048) * 2048);
                }
            }
            if start >= disk.sectors {
                errln!(io, "diskpart: no free space left");
                return 1;
            }
            let available = disk.sectors.min(u32::MAX as u64) - start;
            let size = args.get(3).map(|s| s.as_str()).unwrap_or("rest");
            let Some(sectors) = parse_size(size, available) else {
                errln!(io, "diskpart: bad size {}", size);
                return 1;
            };
            let sectors = sectors.min(available);
            let kind = args.get(4).map(|s| s.as_str()).unwrap_or("hext");
            let Some(kind_byte) = type_byte(kind) else {
                errln!(io, "diskpart: unknown type {} (hext, linux, swap, fat32, efi, ntfs)", kind);
                return 1;
            };
            let first = (0..4).all(|i| mbr[446 + i * 16 + 4] == 0);
            put_entry(&mut mbr, slot, first, kind_byte, start, sectors);
            let r = sys::disk_write(&disk.name, 0, &mbr);
            if r < 0 {
                return fail(io, "write", r);
            }
            sys::disk_rescan();
            outln!(io, "/dev/{}{}: {} at sector {}, type {}", disk.name, slot + 1, human(sectors * 512), start, kind);
            0
        }
        "delete" | "boot" => {
            let Some(number) = args.get(3).and_then(|s| s.parse::<usize>().ok()).filter(|n| (1..=4).contains(n)) else {
                errln!(io, "usage: diskpart disk {} N", action);
                return 1;
            };
            let mut mbr = match read_sectors(&disk.name, 0, 1) {
                Ok(m) => m,
                Err(e) => return fail(io, "read", e),
            };
            let offset = 446 + (number - 1) * 16;
            if action == "delete" {
                mbr[offset..offset + 16].fill(0);
            } else {
                for i in 0..4 {
                    mbr[446 + i * 16] = 0;
                }
                mbr[offset] = 0x80;
            }
            let r = sys::disk_write(&disk.name, 0, &mbr);
            if r < 0 {
                return fail(io, "write", r);
            }
            sys::disk_rescan();
            0
        }
        other => {
            errln!(io, "diskpart: unknown action {}", other);
            1
        }
    }
}

fn mkfs(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["L", "t"]);
    if let Some(kind) = opts.value("t") {
        if kind != "hext" {
            errln!(io, "mkfs: only hext filesystems can be created");
            return 1;
        }
    }
    let Some(device) = opts.rest.first() else {
        errln!(io, "usage: mkfs.hext [-L label] device");
        return 1;
    };
    let label = opts.value("L").unwrap_or_else(|| String::from("hamix"));
    outln!(io, "Creating hext filesystem on {} (label {})...", device, label);
    let r = sys::mkfs(device, &label);
    if r < 0 {
        errln!(io, "mkfs: {}: {}{}", device, sys::error_name(r), if r == -1 { " (try sudo)" } else { "" });
        return 1;
    }
    let name = device.trim_start_matches("/dev/");
    for disk in disks() {
        for p in disk.partitions.iter().filter(|p| p.name == name) {
            outln!(io, "{}: hext, {}, UUID {}", device, human(p.sectors * 512), p.uuid);
        }
        if disk.name == name {
            outln!(io, "{}: hext, {}, UUID {}", device, human(disk.sectors * 512), disk.uuid);
        }
    }
    0
}

fn mount(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let rest: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with('-')).collect();
    if rest.len() < 2 {
        for line in fs::read_to_string("/proc/mounts").unwrap_or_default().lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() >= 4 {
                outln!(io, "{} on {} type {} ({})", f[0], f[1], f[2], f[3]);
            }
        }
        return 0;
    }
    let device = if rest[0].starts_with("/dev/") { rest[0].clone() } else { format!("/dev/{}", rest[0]) };
    let r = sys::mount(&device, rest[1]);
    if r < 0 {
        errln!(io, "mount: {} on {}: {}{}", device, rest[1], sys::error_name(r), if r == -1 { " (try sudo)" } else if r == -22 { " (no hext filesystem)" } else { "" });
        return 1;
    }
    0
}

fn umount(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(target) = args.get(1) else {
        errln!(io, "usage: umount dir|device");
        return 1;
    };
    let mut path = target.clone();
    if target.starts_with("/dev/") {
        for line in fs::read_to_string("/proc/mounts").unwrap_or_default().lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() >= 2 && f[0] == target {
                path = String::from(f[1]);
            }
        }
    }
    let r = sys::umount(&path);
    if r < 0 {
        errln!(io, "umount: {}: {}{}", target, sys::error_name(r), if r == -1 { " (try sudo)" } else { "" });
        return 1;
    }
    0
}

fn df(_: &mut Shell, _: &[String], io: Io) -> i32 {
    outln!(io, "\x1b[1m{:<12} {:>7} {:>7} {:>7} {:>4}  {}\x1b[0m", "FILESYSTEM", "SIZE", "USED", "AVAIL", "USE%", "MOUNTED ON");
    for line in fs::read_to_string("/proc/mounts").unwrap_or_default().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let total: u64 = f[4].parse().unwrap_or(0);
        let free: u64 = f[5].parse().unwrap_or(0);
        let used = total.saturating_sub(free);
        let percent = if total > 0 { used * 100 / total } else { 0 };
        outln!(io, "{:<12} {:>7} {:>7} {:>7} {:>3}%  {}", f[0], human(total * 1024), human(used * 1024), human(free * 1024), percent, f[1]);
    }
    0
}

pub fn grub_cfg(uuid: &str) -> String {
    format!(
        "set timeout=3\nset default=0\n\ninsmod all_video\ninsmod gfxterm\nset gfxmode=1024x768x32,1024x768,800x600x32,800x600,auto\nset gfxpayload=keep\n\nmenuentry \"HamixOS 0.6\" {{\n    multiboot2 /boot/kernel.bin root=UUID={}\n    module2 /boot/kmods.tar kmods\n    boot\n}}\n\nmenuentry \"HamixOS 0.6 (text console)\" {{\n    set gfxpayload=text\n    multiboot2 /boot/kernel.bin root=UUID={}\n    module2 /boot/kmods.tar kmods\n    boot\n}}\n",
        uuid, uuid
    )
}

fn bootinstall(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root"]);
    let Some(disk_name) = opts.rest.first().map(|d| String::from(d.trim_start_matches("/dev/"))) else {
        errln!(io, "usage: bootinstall disk [--root /mnt]");
        return 1;
    };
    let root = opts.value("root").unwrap_or_else(|| String::from("/mnt"));
    let root = String::from(root.trim_end_matches('/'));
    let Some(disk) = disks().into_iter().find(|d| d.name == disk_name) else {
        errln!(io, "bootinstall: {}: no such disk", disk_name);
        return 1;
    };
    let Some(part) = disk.partitions.iter().find(|p| p.mount == root || (root.is_empty() && p.mount == "/")) else {
        errln!(io, "bootinstall: no partition of /dev/{} is mounted on {}", disk.name, if root.is_empty() { "/" } else { &root });
        return 1;
    };
    if part.fstype != "hext" {
        errln!(io, "bootinstall: /dev/{} is not a hext volume", part.name);
        return 1;
    }
    let (Some(boot_img), Some(core_img)) = (fs::read(&format!("{}/boot.img", BOOT_DIR)), fs::read(&format!("{}/core.img", BOOT_DIR))) else {
        errln!(io, "bootinstall: {}/boot.img or core.img is missing", BOOT_DIR);
        return 1;
    };
    let first_start = disk.partitions.iter().map(|p| p.start).min().unwrap_or(0);
    let core_sectors = core_img.len().div_ceil(512) as u64;
    if boot_img.len() < 512 || 1 + core_sectors > first_start {
        errln!(io, "bootinstall: not enough room before the first partition ({} sectors needed)", core_sectors + 1);
        return 1;
    }
    let mut mbr = match read_sectors(&disk.name, 0, 1) {
        Ok(m) => m,
        Err(e) => {
            errln!(io, "bootinstall: {}", sys::error_name(e));
            return 1;
        }
    };
    mbr[..440].copy_from_slice(&boot_img[..440]);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    let mut core = core_img.clone();
    core.resize(core_sectors as usize * 512, 0);
    let r = sys::disk_write(&disk.name, 1, &core);
    if r < 0 {
        errln!(io, "bootinstall: writing core.img: {}", sys::error_name(r));
        return 1;
    }
    let r = sys::disk_write(&disk.name, 0, &mbr);
    if r < 0 {
        errln!(io, "bootinstall: writing the MBR: {}", sys::error_name(r));
        return 1;
    }
    outln!(io, "GRUB boot code written to /dev/{} ({} sectors)", disk.name, core_sectors + 1);

    let boot = format!("{}/boot", root);
    sys::mkdir(&boot);
    sys::mkdir(&format!("{}/grub", boot));
    let kernel_target = format!("{}/kernel.bin", boot);
    if sys::stat(&kernel_target).is_err() {
        match fs::read("/boot/kernel.bin") {
            Some(kernel) if fs::write(&kernel_target, &kernel) => {}
            _ => {
                errln!(io, "bootinstall: cannot copy /boot/kernel.bin");
                return 1;
            }
        }
    }
    if !fs::write(&format!("{}/grub/grub.cfg", boot), grub_cfg(&part.uuid).as_bytes()) {
        errln!(io, "bootinstall: cannot write grub.cfg");
        return 1;
    }
    let mut mbr = match read_sectors(&disk.name, 0, 1) {
        Ok(m) => m,
        Err(_) => return 1,
    };
    let slot = (part.name.trim_start_matches(&disk.name).parse::<usize>().unwrap_or(1) - 1).min(3);
    for i in 0..4 {
        mbr[446 + i * 16] = if i == slot { 0x80 } else { 0 };
    }
    sys::disk_write(&disk.name, 0, &mbr);
    sys::sync();
    outln!(io, "boot configuration written to {}/grub/grub.cfg (root=UUID={})", boot, part.uuid);
    0
}
