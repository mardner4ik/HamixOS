use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::sys;

pub struct Found {
    pub disk: String,
    pub signature: u32,
    pub partition: u8,
    pub system: &'static str,
}

fn identify(kind: u8, vbr: &[u8]) -> Option<&'static str> {
    let oem = &vbr[3..11];
    match kind {
        0x07 if oem == b"NTFS    " => Some("Windows"),
        0x07 => None,
        0xa5 => Some("FreeBSD"),
        0xa6 => Some("OpenBSD"),
        0xa9 => Some("NetBSD"),
        0x01 | 0x04 | 0x06 | 0x0b | 0x0c | 0x0e if oem.starts_with(b"MSWIN") || oem.starts_with(b"MSDOS") || oem.starts_with(b"FRDOS") => Some("DOS/Windows (FAT)"),
        0x83 if vbr[..440].iter().any(|&b| b != 0) => Some("Linux"),
        _ => None,
    }
}

pub fn disks() -> Vec<String> {
    let mut names: Vec<String> = sys::read_dir("/sys/block").unwrap_or_default().into_iter().map(|e| e.name).filter(|n| !n.starts_with("loop") && !n.starts_with("sr") && !n.starts_with("ram")).collect();
    names.sort();
    names
}

pub fn scan() -> Result<Vec<Found>, String> {
    let mut found = Vec::new();
    let mut readable = 0;
    for disk in disks() {
        let mut mbr = [0u8; 512];
        if sys::disk_read(&disk, 0, &mut mbr) != 0 {
            continue;
        }
        readable += 1;
        if mbr[510] != 0x55 || mbr[511] != 0xaa {
            continue;
        }
        let signature = u32::from_le_bytes([mbr[440], mbr[441], mbr[442], mbr[443]]);
        let mut any = false;
        let mut bootable = false;
        for p in 0..4 {
            let e = &mbr[446 + p * 16..462 + p * 16];
            let kind = e[4];
            let start = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as u64;
            if kind == 0 || start == 0 {
                continue;
            }
            any = true;
            if matches!(kind, 0x05 | 0x0f | 0x85 | 0xee | 0xef) {
                continue;
            }
            let mut vbr = [0u8; 512];
            if sys::disk_read(&disk, start, &mut vbr) != 0 || vbr[510] != 0x55 || vbr[511] != 0xaa {
                continue;
            }
            if let Some(system) = identify(kind, &vbr) {
                found.push(Found { disk: disk.clone(), signature, partition: p as u8 + 1, system });
                bootable = true;
            }
        }
        if any && !bootable && mbr[..440].iter().any(|&b| b != 0) {
            found.push(Found { disk: disk.clone(), signature, partition: 0, system: "its own boot loader" });
        }
    }
    if readable == 0 {
        return Err(String::from("no disk could be read (run as root: sudo hxbootctl probe)"));
    }
    Ok(found)
}

impl Found {
    pub fn chain(&self) -> String {
        format!("signature={:08x} partition={}", self.signature, self.partition)
    }

    pub fn title(&self) -> String {
        if self.partition == 0 {
            format!("Hard disk {} ({})", self.disk, self.system)
        } else {
            format!("{} ({}{})", self.system, self.disk, self.partition)
        }
    }
}
