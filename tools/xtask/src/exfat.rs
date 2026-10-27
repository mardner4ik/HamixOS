use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::util::{command, run, Result};

const PARTITION_START: u64 = 2048;

fn le32(b: &[u8], at: usize) -> u64 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as u64
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn rotate_sum16(sum: u16, byte: u8) -> u16 {
    (sum >> 1 | sum << 15).wrapping_add(byte as u16)
}

fn entry_set(name: &str, first_cluster: u32, size: u64, chained: bool) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let name_entries = units.len().div_ceil(15);
    let mut set = vec![0u8; 32 * (2 + name_entries)];
    set[0] = 0x85;
    set[1] = (1 + name_entries) as u8;
    set[4..6].copy_from_slice(&0x20u16.to_le_bytes());
    let stream = &mut set[32..64];
    stream[0] = 0xC0;
    stream[1] = if chained { 0x01 } else { 0x03 };
    stream[3] = units.len() as u8;
    let mut hash = 0u16;
    for unit in name.to_uppercase().encode_utf16() {
        for byte in unit.to_le_bytes() {
            hash = rotate_sum16(hash, byte);
        }
    }
    stream[4..6].copy_from_slice(&hash.to_le_bytes());
    stream[8..16].copy_from_slice(&size.to_le_bytes());
    stream[20..24].copy_from_slice(&first_cluster.to_le_bytes());
    stream[24..32].copy_from_slice(&size.to_le_bytes());
    for (i, chunk) in units.chunks(15).enumerate() {
        let entry = &mut set[64 + i * 32..96 + i * 32];
        entry[0] = 0xC1;
        for (k, unit) in chunk.iter().enumerate() {
            entry[2 + k * 2..4 + k * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    let mut checksum = 0u16;
    for (i, byte) in set.iter().enumerate() {
        if i != 2 && i != 3 {
            checksum = rotate_sum16(checksum, *byte);
        }
    }
    set[2..4].copy_from_slice(&checksum.to_le_bytes());
    set
}

fn add_file(partition: &Path, name: &str, source: &Path, fragments: u64) -> Result {
    let err = |e: std::io::Error| format!("{}: {}", partition.display(), e);
    let mut image = fs::OpenOptions::new().read(true).write(true).open(partition).map_err(err)?;
    let mut boot = vec![0u8; 512];
    image.read_exact(&mut boot).map_err(err)?;
    if &boot[3..11] != b"EXFAT   " {
        return Err(format!("{} is not exFAT", partition.display()));
    }
    let sector = 1u64 << boot[108];
    let cluster = sector << boot[109];
    let heap = le32(&boot, 88) * sector;
    let clusters = le32(&boot, 92);
    let root = le32(&boot, 96);
    let at_cluster = |c: u64| heap + (c - 2) * cluster;
    let mut dir = vec![0u8; cluster as usize];
    image.seek(SeekFrom::Start(at_cluster(root))).map_err(err)?;
    image.read_exact(&mut dir).map_err(err)?;
    let bitmap = dir.chunks(32).find(|e| e[0] == 0x81).ok_or("no allocation bitmap in the exFAT root")?;
    let (bitmap_at, bitmap_len) = (at_cluster(le32(bitmap, 20)), le64(bitmap, 24) as usize);
    let mut bits = vec![0u8; bitmap_len];
    image.seek(SeekFrom::Start(bitmap_at)).map_err(err)?;
    image.read_exact(&mut bits).map_err(err)?;
    let used = (0..clusters).rev().find(|i| bits[*i as usize / 8] & (1 << (i % 8)) != 0).map(|i| i + 1).unwrap_or(0);
    let size = fs::metadata(source).map_err(|e| e.to_string())?.len();
    let count = size.div_ceil(cluster).max(1);
    let fragments = fragments.clamp(1, count);
    let per = count.div_ceil(fragments);
    if used + count + fragments > clusters {
        return Err(format!("{} is too small for {}", partition.display(), source.display()));
    }
    let mut chain = Vec::with_capacity(count as usize);
    let mut next_free = used;
    for piece in 0..fragments {
        let take = per.min(count - piece * per);
        for i in 0..take {
            chain.push(next_free + i + 2);
        }
        next_free += take + 1;
    }
    for c in &chain {
        let bit = c - 2;
        bits[bit as usize / 8] |= 1 << (bit % 8);
    }
    let fat_at = le32(&boot, 80) * sector;
    if fragments > 1 {
        for (i, c) in chain.iter().enumerate() {
            let value: u32 = chain.get(i + 1).map(|n| *n as u32).unwrap_or(0xFFFF_FFFF);
            image.seek(SeekFrom::Start(fat_at + c * 4)).map_err(err)?;
            image.write_all(&value.to_le_bytes()).map_err(err)?;
        }
    }
    let set = entry_set(name, chain[0] as u32, size, fragments > 1);
    let slot = (0..dir.len() / 32).find(|i| dir[i * 32..].iter().take(set.len()).all(|b| *b == 0) && dir[i * 32] == 0).ok_or("the exFAT root directory is full")?;
    dir[slot * 32..slot * 32 + set.len()].copy_from_slice(&set);
    image.seek(SeekFrom::Start(bitmap_at)).map_err(err)?;
    image.write_all(&bits).map_err(err)?;
    image.seek(SeekFrom::Start(at_cluster(root))).map_err(err)?;
    image.write_all(&dir).map_err(err)?;
    let mut data = fs::File::open(source).map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; cluster as usize];
    for c in &chain {
        buffer.fill(0);
        let mut filled = 0;
        while filled < buffer.len() {
            let n = data.read(&mut buffer[filled..]).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        image.seek(SeekFrom::Start(at_cluster(*c))).map_err(err)?;
        image.write_all(&buffer).map_err(err)?;
    }
    Ok(())
}

pub fn ventoy_stick(disk: &Path, iso: &Path, iso_name: &str) -> Result {
    let size = fs::metadata(iso).map_err(|e| e.to_string())?.len();
    let partition_bytes = (size + (80 << 20)).div_ceil(1 << 20) << 20;
    let partition = disk.with_extension("part");
    let _ = fs::remove_file(&partition);
    fs::File::create(&partition).and_then(|f| f.set_len(partition_bytes)).map_err(|e| e.to_string())?;
    run(command("mkfs.exfat").args(["-L", "Ventoy"]).arg(&partition))?;
    let decoy = disk.with_extension("decoy");
    fs::write(&decoy, vec![0u8; 1 << 20]).map_err(|e| e.to_string())?;
    add_file(&partition, "another-system.iso", &decoy, 1)?;
    let _ = fs::remove_file(&decoy);
    add_file(&partition, iso_name, iso, 5)?;
    let mut out = fs::File::create(disk).map_err(|e| e.to_string())?;
    let mut mbr = vec![0u8; (PARTITION_START * 512) as usize];
    let entry = &mut mbr[446..462];
    entry[4] = 0x07;
    entry[8..12].copy_from_slice(&(PARTITION_START as u32).to_le_bytes());
    entry[12..16].copy_from_slice(&((partition_bytes / 512) as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    out.write_all(&mbr).map_err(|e| e.to_string())?;
    let mut body = fs::File::open(&partition).map_err(|e| e.to_string())?;
    std::io::copy(&mut body, &mut out).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&partition);
    Ok(())
}
