use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{fs, sys};

use crate::Io;
use crate::{errln, outln};

const MEDIA: &str = "/media";
const MEDIA_TYPES: [&str; 2] = ["iso9660", "vfat"];
const UNMOUNTED: &str = "/run/hamix/media-unmounted";

fn unmounted() -> Vec<String> {
    fs::read_to_string(UNMOUNTED).unwrap_or_default().lines().map(String::from).filter(|l| !l.is_empty()).collect()
}

fn save_unmounted(list: &[String]) {
    sys::mkdir("/run/hamix");
    let text: String = list.iter().map(|l| format!("{}\n", l)).collect();
    fs::write(UNMOUNTED, text.as_bytes());
}

#[derive(Clone)]
pub struct Volume {
    pub device: String,
    pub disk: String,
    pub label: String,
    pub kind: String,
    pub optical: bool,
    pub mount: Option<String>,
}

struct MountLine {
    source: String,
    path: String,
    kind: String,
}

fn mounts() -> Vec<MountLine> {
    fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 3).then(|| MountLine { source: String::from(f[0]), path: String::from(f[1]), kind: String::from(f[2]) })
        })
        .collect()
}

fn disk_of(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("sr") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).take(1).collect();
        return format!("sr{}", digits);
    }
    String::from(name.trim_end_matches(|c: char| c.is_ascii_digit()))
}

fn removable(disk: &str) -> bool {
    disk.starts_with("sr") || fs::read_to_string(&format!("/sys/block/{}/removable", disk)).map(|v| v.trim() == "1").unwrap_or(false)
}

fn system_disks(lines: &[MountLine]) -> Vec<String> {
    lines
        .iter()
        .filter(|m| !m.path.starts_with("/media/") && m.source.starts_with("/dev/"))
        .map(|m| disk_of(m.source.trim_start_matches("/dev/").split('/').next().unwrap_or("")))
        .collect()
}

fn read_head(device: &str, bytes: usize) -> Option<Vec<u8>> {
    let fd = sys::open_with(&format!("/dev/{}", device), sys::O_RDONLY);
    if fd < 0 {
        return None;
    }
    let mut out = alloc::vec![0u8; bytes];
    let mut filled = 0usize;
    while filled < bytes {
        let n = sys::read(fd as u64, &mut out[filled..]);
        if n <= 0 {
            break;
        }
        filled += n as usize;
    }
    sys::close(fd as u64);
    out.truncate(filled);
    Some(out)
}

fn clean(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_matches(|c: char| c == ' ' || c == '\0').replace('/', "_")
}

pub fn probe(device: &str) -> Option<(String, String)> {
    let head = read_head(device, 34816)?;
    if head.len() >= 32816 && &head[32769..32774] == b"CD001" {
        return Some((String::from("iso9660"), clean(&head[32808..32840])));
    }
    if head.len() >= 512 && head[510] == 0x55 && head[511] == 0xAA {
        if &head[82..87] == b"FAT32" {
            return Some((String::from("vfat"), clean(&head[71..82])));
        }
        if &head[54..57] == b"FAT" {
            return Some((String::from("vfat"), clean(&head[43..54])));
        }
    }
    None
}

pub fn volumes() -> Vec<Volume> {
    let lines = mounts();
    let system = system_disks(&lines);
    let names: Vec<String> = fs::read_to_string("/proc/partitions")
        .unwrap_or_default()
        .lines()
        .skip(2)
        .filter_map(|l| l.split_whitespace().nth(3).map(String::from))
        .collect();
    let mut out = Vec::new();
    for name in names.iter() {
        let disk = disk_of(name);
        if system.contains(&disk) || !removable(&disk) {
            continue;
        }
        let optical = disk.starts_with("sr");
        let whole = *name == disk;
        let has_parts = names.iter().any(|n| n != &disk && disk_of(n) == disk);
        if optical && !whole || !optical && whole && has_parts {
            continue;
        }
        let Some((kind, label)) = probe(name) else {
            continue;
        };
        let mount = lines.iter().find(|m| m.source == format!("/dev/{}", name)).map(|m| m.path.clone());
        out.push(Volume { device: name.clone(), disk, label, kind, optical, mount });
    }
    out
}

fn free_target(label: &str, device: &str) -> String {
    let base = if label.is_empty() { String::from(device) } else { String::from(label) };
    let taken: Vec<String> = mounts().into_iter().map(|m| m.path).collect();
    let mut candidate = format!("{}/{}", MEDIA, base);
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{}/{} ({})", MEDIA, base, n);
        n += 1;
    }
    candidate
}

fn mount_volume(v: &Volume) -> Result<String, i64> {
    if let Some(path) = &v.mount {
        return Ok(path.clone());
    }
    let target = free_target(&v.label, &v.device);
    sys::mkdir(MEDIA);
    sys::mkdir(&target);
    let r = sys::mount(&format!("/dev/{}", v.device), &target);
    if r < 0 {
        sys::unlink(&target);
        return Err(r);
    }
    Ok(target)
}

fn unmount_path(path: &str) -> i64 {
    let r = sys::umount(path);
    if r == 0 && path.starts_with("/media/") {
        sys::unlink(path);
    }
    r
}

fn find(arg: &str) -> Option<Volume> {
    let name = arg.trim_start_matches("/dev/");
    volumes().into_iter().find(|v| v.device == name || v.mount.as_deref() == Some(arg) || (!v.label.is_empty() && v.label == arg))
}

pub fn hxmount(args: &[String], io: Io) -> i32 {
    match args.get(1).map(|s| s.as_str()) {
        Some("list") | None => {
            for v in volumes() {
                outln!(io, "{}\t{}\t{}\t{}\t{}", v.device, v.kind, if v.optical { "optical" } else { "usb" }, if v.label.is_empty() { "-" } else { &v.label }, v.mount.as_deref().unwrap_or("-"));
            }
            0
        }
        Some("mount") => {
            let Some(arg) = args.get(2) else {
                errln!(io, "usage: hxmount mount DEVICE");
                return 2;
            };
            let Some(v) = find(arg) else {
                errln!(io, "hxmount: {} is not a removable disk, CD/DVD or USB drive", arg);
                return 1;
            };
            let mut list = unmounted();
            if list.contains(&v.device) {
                list.retain(|d| d != &v.device);
                save_unmounted(&list);
            }
            match mount_volume(&v) {
                Ok(path) => {
                    outln!(io, "{}", path);
                    0
                }
                Err(e) => {
                    errln!(io, "hxmount: cannot mount /dev/{}: {}", v.device, sys::error_name(e));
                    1
                }
            }
        }
        Some("unmount") | Some("umount") => {
            let Some(arg) = args.get(2) else {
                errln!(io, "usage: hxmount unmount DEVICE|PATH");
                return 2;
            };
            let lines = mounts();
            let line = lines.iter().find(|m| m.path == *arg || m.source == format!("/dev/{}", arg.trim_start_matches("/dev/")));
            let Some(line) = line.filter(|m| m.path.starts_with("/media/") && MEDIA_TYPES.contains(&m.kind.as_str())) else {
                errln!(io, "hxmount: {} is not a mounted removable drive", arg);
                return 1;
            };
            let r = unmount_path(&line.path);
            if r < 0 {
                errln!(io, "hxmount: cannot unmount {}: {}", line.path, sys::error_name(r));
                return 1;
            }
            let device = String::from(line.source.trim_start_matches("/dev/"));
            let mut list = unmounted();
            if !list.contains(&device) {
                list.push(device);
                save_unmounted(&list);
            }
            0
        }
        Some("watch") => {
            let mut failed: Vec<String> = Vec::new();
            loop {
                let current = volumes();
                failed.retain(|d| current.iter().any(|v| &v.device == d));
                let mut skipped = unmounted();
                let before = skipped.len();
                skipped.retain(|d| current.iter().any(|v| &v.device == d));
                if skipped.len() != before {
                    save_unmounted(&skipped);
                }
                failed.extend(skipped.into_iter().filter(|d| !failed.contains(d)).collect::<Vec<_>>());
                let pending: Vec<&Volume> = current.iter().filter(|v| v.mount.is_none() && !failed.contains(&v.device)).collect();
                for v in pending {
                    match mount_volume(v) {
                        Ok(path) => outln!(io, "hxmount: /dev/{} ({}) on {}", v.device, v.kind, path),
                        Err(_) => failed.push(v.device.clone()),
                    }
                }
                let mounted: Vec<String> = mounts().into_iter().map(|m| m.path).collect();
                if let Some(entries) = sys::read_dir(MEDIA) {
                    for e in entries {
                        let path = format!("{}/{}", MEDIA, e.name);
                        if e.is_dir && !mounted.contains(&path) && sys::read_dir(&path).map(|l| l.is_empty()).unwrap_or(false) {
                            sys::unlink(&path);
                        }
                    }
                }
                sys::sleep_ms(2000);
            }
        }
        Some(other) => {
            errln!(io, "hxmount: unknown command {} (list, mount, unmount, watch)", other);
            2
        }
    }
}
