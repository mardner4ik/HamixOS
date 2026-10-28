use core::fmt::Write;

use crate::bios;
use crate::config::{Config, Entry, Kind, EMPTY_ENTRY};
use crate::disk::Disk;
use crate::layout::SCRATCH;

const NAMES: usize = 16;
const NAME_LEN: usize = 72;

static mut NAME_POOL: [[u8; NAME_LEN]; NAMES] = [[0; NAME_LEN]; NAMES];
static mut NAME_USED: usize = 0;

struct Slice<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Write for Slice<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

fn name(args: core::fmt::Arguments) -> Option<&'static str> {
    let index = unsafe { NAME_USED };
    if index >= NAMES {
        return None;
    }
    unsafe { NAME_USED += 1 };
    let buf = unsafe { &mut (*(&raw mut NAME_POOL))[index] };
    let mut w = Slice { buf, len: 0 };
    let _ = w.write_fmt(args);
    let len = w.len;
    core::str::from_utf8(&buf[..len]).ok()
}

pub fn hard_disks() -> u8 {
    bios::read_u8(0x475).min(8)
}

pub fn read_mbr(drive: u8, at: u32) -> bool {
    let disk = Disk::new(drive, 9);
    if disk.sector_size() != 512 || disk.read(0, at as *mut u8, 512).is_err() {
        return false;
    }
    bios::read_u16(at + 510) == 0xaa55
}

pub fn find_drive(signature: u32) -> Option<u8> {
    for i in 0..hard_disks() {
        let drive = 0x80 + i;
        if read_mbr(drive, SCRATCH) && bios::read_u32(SCRATCH + 440) == signature {
            return Some(drive);
        }
    }
    None
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

pub fn add_other_systems(config: &mut Config, boot_drive: u8) {
    let vbr_at = SCRATCH + 512;
    for i in 0..hard_disks() {
        let drive = 0x80 + i;
        if drive == boot_drive || !read_mbr(drive, SCRATCH) {
            continue;
        }
        let mbr = bios::bytes(SCRATCH, 512);
        let mut table = [0u8; 64];
        table.copy_from_slice(&mbr[446..510]);
        let signature = bios::read_u32(SCRATCH + 440);
        let has_code = mbr[..440].iter().any(|&b| b != 0);
        let disk = Disk::new(drive, 9);
        let mut found = false;
        let mut any = false;
        for p in 0..4 {
            let e = &table[p * 16..p * 16 + 16];
            let kind = e[4];
            let start = u32::from_le_bytes([e[8], e[9], e[10], e[11]]);
            if kind == 0 || start == 0 {
                continue;
            }
            any = true;
            if matches!(kind, 0x05 | 0x0f | 0x85 | 0xee | 0xef) {
                continue;
            }
            if disk.read(start as u64 * 512, vbr_at as *mut u8, 512).is_err() || bios::read_u16(vbr_at + 510) != 0xaa55 {
                continue;
            }
            let Some(os) = identify(kind, bios::bytes(vbr_at, 512)) else {
                continue;
            };
            let Some(label) = name(format_args!("{} (disk {}, partition {})", os, i + 1, p + 1)) else {
                return;
            };
            let kind = Kind::Chain { drive, signature, partition: p as u8 + 1 };
            if !config.push(Entry { name: label, kind, ..EMPTY_ENTRY }) {
                return;
            }
            found = true;
        }
        if !found && any && has_code {
            let Some(label) = name(format_args!("Hard disk {} (its own boot loader)", i + 1)) else {
                return;
            };
            if !config.push(Entry { name: label, kind: Kind::Chain { drive, signature, partition: 0 }, ..EMPTY_ENTRY }) {
                return;
            }
        }
    }
}
