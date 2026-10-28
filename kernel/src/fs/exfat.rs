use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::block;

pub struct FoundFile {
    pub path: String,
    pub first: u32,
    pub bytes: u64,
    pub contiguous: bool,
}

struct Volume {
    disk: usize,
    offset: u64,
    sector_units: u64,
    cluster_shift: u32,
    fat_offset: u64,
    heap_offset: u64,
    clusters: u32,
    root: u32,
    fat_cache: Option<(u64, Vec<u8>)>,
}

const MAX_DIR_BYTES: usize = 4 << 20;
const MAX_DEPTH: usize = 3;
const NO_FAT_CHAIN: u8 = 2;

fn read(disk: usize, lba: u64, sectors: usize) -> Option<Vec<u8>> {
    let mut buf = alloc::vec![0u8; sectors * 512];
    block::read(disk, lba, &mut buf).ok()?;
    Some(buf)
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap_or([0; 8]))
}

impl Volume {
    fn open(disk: usize, offset: u64) -> Result<Volume, String> {
        let boot = read(disk, offset, 1).ok_or_else(|| format!("read error at sector {}", offset))?;
        if &boot[3..11] != b"EXFAT   " {
            return Err(String::from("not exFAT"));
        }
        let sector_shift = boot[108] as u32;
        let cluster_shift = boot[109] as u32;
        if !(9..=12).contains(&sector_shift) || cluster_shift > 25 {
            return Err(String::from("damaged exFAT boot sector"));
        }
        Ok(Volume {
            disk,
            offset,
            sector_units: 1 << (sector_shift - 9),
            cluster_shift,
            fat_offset: le32(&boot, 80) as u64,
            heap_offset: le32(&boot, 88) as u64,
            clusters: le32(&boot, 92),
            root: le32(&boot, 96),
            fat_cache: None,
        })
    }

    fn cluster_lba(&self, cluster: u32) -> u64 {
        self.offset + (self.heap_offset + ((cluster as u64 - 2) << self.cluster_shift)) * self.sector_units
    }

    fn cluster_bytes(&self) -> u64 {
        (512 * self.sector_units) << self.cluster_shift
    }

    fn valid(&self, cluster: u32) -> bool {
        cluster >= 2 && cluster < self.clusters + 2
    }

    fn next(&mut self, cluster: u32) -> Option<u32> {
        let byte = cluster as u64 * 4;
        let lba = self.offset + self.fat_offset * self.sector_units + byte / 512;
        if self.fat_cache.as_ref().map(|(at, _)| *at != lba).unwrap_or(true) {
            self.fat_cache = Some((lba, read(self.disk, lba, 1)?));
        }
        let value = le32(&self.fat_cache.as_ref()?.1, (byte % 512) as usize);
        self.valid(value).then_some(value)
    }

    fn read_clusters(&mut self, first: u32, bytes: u64, contiguous: bool) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut cluster = first;
        let limit = (bytes as usize).min(MAX_DIR_BYTES);
        while out.len() < limit && self.valid(cluster) {
            let sectors = (self.cluster_bytes() / 512) as usize;
            out.extend_from_slice(&read(self.disk, self.cluster_lba(cluster), sectors)?);
            cluster = if contiguous {
                cluster + 1
            } else {
                match self.next(cluster) {
                    Some(n) => n,
                    None => break,
                }
            };
        }
        out.truncate(limit);
        Some(out)
    }

    fn runs(&mut self, file: &FoundFile) -> Option<Vec<(u64, u64)>> {
        let count = file.bytes.div_ceil(self.cluster_bytes());
        let per_cluster = self.cluster_bytes() / 512;
        if file.contiguous {
            return Some(alloc::vec![(self.cluster_lba(file.first), count * per_cluster)]);
        }
        let mut runs: Vec<(u64, u64)> = Vec::new();
        let mut cluster = file.first;
        for i in 0..count {
            let lba = self.cluster_lba(cluster);
            match runs.last_mut() {
                Some((start, sectors)) if *start + *sectors == lba => *sectors += per_cluster,
                _ => runs.push((lba, per_cluster)),
            }
            if i + 1 < count {
                cluster = self.next(cluster)?;
            }
        }
        Some(runs)
    }

    fn cluster_at(&mut self, file: &FoundFile, index: u64) -> Option<u32> {
        if file.contiguous {
            return Some(file.first + index as u32);
        }
        let mut cluster = file.first;
        for _ in 0..index {
            cluster = self.next(cluster)?;
        }
        Some(cluster)
    }

    fn walk(&mut self, data: &[u8], prefix: &str, depth: usize, suffix: &str, out: &mut Vec<FoundFile>) {
        let mut at = 0;
        while at + 32 <= data.len() {
            let kind = data[at];
            if kind == 0 {
                break;
            }
            if kind != 0x85 {
                at += 32;
                continue;
            }
            let secondary = data[at + 1] as usize;
            let attributes = le16(data, at + 4);
            let set_end = at + 32 * (secondary + 1);
            if secondary < 2 || set_end > data.len() || data[at + 32] != 0xC0 {
                at += 32;
                continue;
            }
            let stream = &data[at + 32..at + 64];
            let flags = stream[1];
            let name_len = stream[3] as usize;
            let first = le32(stream, 20);
            let bytes = le64(stream, 24);
            let mut units = Vec::new();
            let mut entry = at + 64;
            while entry < set_end && units.len() < name_len {
                if data[entry] == 0xC1 {
                    for i in 0..15 {
                        units.push(le16(data, entry + 2 + i * 2));
                    }
                }
                entry += 32;
            }
            units.truncate(name_len);
            let name = String::from_utf16_lossy(&units);
            let path = format!("{}/{}", prefix, name);
            let contiguous = flags & NO_FAT_CHAIN != 0;
            if attributes & 0x10 != 0 {
                if depth < MAX_DEPTH && self.valid(first) {
                    if let Some(child) = self.read_clusters(first, bytes, contiguous) {
                        self.walk(&child, &path, depth + 1, suffix, out);
                    }
                }
            } else if name.to_ascii_lowercase().ends_with(suffix) && self.valid(first) {
                out.push(FoundFile { path, first, bytes, contiguous });
            }
            at = set_end;
        }
    }
}

pub fn find_files(disk: usize, offset: u64, suffix: &str) -> Result<Vec<FoundFile>, String> {
    let mut volume = Volume::open(disk, offset)?;
    let root = volume.root;
    let data = volume.read_clusters(root, MAX_DIR_BYTES as u64, false).ok_or_else(|| String::from("cannot read the exFAT root directory"))?;
    let mut out = Vec::new();
    volume.walk(&data, "", 0, suffix, &mut out);
    Ok(out)
}

pub fn file_runs(disk: usize, offset: u64, file: &FoundFile) -> Option<Vec<(u64, u64)>> {
    Volume::open(disk, offset).ok()?.runs(file)
}

pub fn read_file_sector(disk: usize, offset: u64, file: &FoundFile, byte: u64) -> Option<Vec<u8>> {
    let mut volume = Volume::open(disk, offset).ok()?;
    let cluster = volume.cluster_at(file, byte / volume.cluster_bytes())?;
    let within = (byte % volume.cluster_bytes()) / 512;
    read(disk, volume.cluster_lba(cluster) + within, 1)
}
