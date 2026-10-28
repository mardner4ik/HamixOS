use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::block;

const BLOCK: u64 = 2048;

fn read_block(disk: usize, base: u64, block_number: u64, count: usize) -> Option<Vec<u8>> {
    let mut buf = alloc::vec![0u8; count * BLOCK as usize];
    block::read(disk, base + block_number * (BLOCK / 512), &mut buf).ok()?;
    Some(buf)
}

fn le32(buf: &[u8], at: usize) -> u64 {
    u32::from_le_bytes(buf[at..at + 4].try_into().unwrap()) as u64
}

fn plain_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let text = text.split(';').next().unwrap_or("");
    String::from(text.trim_end_matches('.')).to_ascii_lowercase()
}

fn find_in(disk: usize, base: u64, extent: u64, length: u64, name: &str) -> Option<(u64, u64, bool)> {
    let blocks = length.div_ceil(BLOCK) as usize;
    let data = read_block(disk, base, extent, blocks.min(64))?;
    let mut at = 0usize;
    while at < data.len() {
        let len = data[at] as usize;
        if len == 0 {
            at = (at / BLOCK as usize + 1) * BLOCK as usize;
            continue;
        }
        if at + len > data.len() || len < 34 {
            break;
        }
        let record = &data[at..at + len];
        let name_len = record[32] as usize;
        if 33 + name_len <= len {
            let raw = &record[33..33 + name_len];
            if raw != [0] && raw != [1] && plain_name(raw) == name {
                return Some((le32(record, 2), le32(record, 10), record[25] & 2 != 0));
            }
        }
        at += len;
    }
    None
}

pub fn find_file(disk: usize, path: &str) -> Option<(u64, u64)> {
    find_file_at(disk, 0, path)
}

pub fn find_file_at(disk: usize, base: u64, path: &str) -> Option<(u64, u64)> {
    let pvd = read_block(disk, base, 16, 1)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    let root = &pvd[156..190];
    let (mut extent, mut length) = (le32(root, 2), le32(root, 10));
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        let (next_extent, next_length, is_dir) = find_in(disk, base, extent, length, &part.to_ascii_lowercase())?;
        if (i + 1 < parts.len()) != is_dir {
            return None;
        }
        extent = next_extent;
        length = next_length;
    }
    Some((base + extent * (BLOCK / 512), length))
}
