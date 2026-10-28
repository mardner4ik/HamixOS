use crate::bios;
use crate::disk::Disk;
use crate::layout::{SCRATCH, SCRATCH_SIZE};

const SECTOR: u64 = 2048;

pub struct Volume<'a> {
    disk: &'a Disk,
    root: File,
}

#[derive(Clone, Copy)]
pub struct File {
    pub lba: u32,
    pub size: u32,
    pub dir: bool,
}

fn name_matches(raw: &[u8], wanted: &str) -> bool {
    let end = raw.iter().position(|&b| b == b';').unwrap_or(raw.len());
    let mut name = &raw[..end];
    while let [rest @ .., b'.'] = name {
        name = rest;
    }
    name.eq_ignore_ascii_case(wanted.as_bytes())
}

impl<'a> Volume<'a> {
    pub fn open(disk: &'a Disk) -> Result<Volume<'a>, &'static str> {
        disk.read(16 * SECTOR, SCRATCH as *mut u8, SECTOR as usize)?;
        let pvd = bios::bytes(SCRATCH, SECTOR as usize);
        if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
            return Err("the boot medium has no ISO 9660 volume");
        }
        let root = File { lba: u32::from_le_bytes(pvd[158..162].try_into().unwrap()), size: u32::from_le_bytes(pvd[166..170].try_into().unwrap()), dir: true };
        Ok(Volume { disk, root })
    }

    fn find_in(&self, dir: &File, wanted: &str) -> Result<File, &'static str> {
        let sectors = (dir.size as u64).div_ceil(SECTOR);
        for s in 0..sectors {
            self.disk.read((dir.lba as u64 + s) * SECTOR, SCRATCH as *mut u8, SECTOR as usize)?;
            let data = bios::bytes(SCRATCH, SECTOR as usize);
            let mut at = 0usize;
            while at < SECTOR as usize {
                let len = data[at] as usize;
                if len == 0 || at + len > SECTOR as usize || len < 34 {
                    break;
                }
                let record = &data[at..at + len];
                let name_len = record[32] as usize;
                if 33 + name_len <= len {
                    let name = &record[33..33 + name_len];
                    if !(name_len == 1 && name[0] <= 1) && name_matches(name, wanted) {
                        return Ok(File {
                            lba: u32::from_le_bytes(record[2..6].try_into().unwrap()),
                            size: u32::from_le_bytes(record[10..14].try_into().unwrap()),
                            dir: record[25] & 2 != 0,
                        });
                    }
                }
                at += len;
            }
        }
        Err("file not found on the boot medium")
    }

    pub fn lookup(&self, path: &str) -> Result<File, &'static str> {
        let mut current = self.root;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            if !current.dir {
                return Err("file not found on the boot medium");
            }
            current = self.find_in(&current, part)?;
        }
        Ok(current)
    }

    pub fn read(&self, file: &File, offset: u32, dst: *mut u8, len: usize) -> Result<(), &'static str> {
        if offset as u64 + len as u64 > file.size as u64 {
            return Err("read past the end of a file");
        }
        self.disk.read(file.lba as u64 * SECTOR + offset as u64, dst, len)
    }

    pub fn read_small(&self, path: &str, at: u32, max: u32) -> Result<&'static str, &'static str> {
        let file = self.lookup(path)?;
        if file.size >= max || file.size >= SCRATCH_SIZE * 8 {
            return Err("the configuration file is too large");
        }
        self.read(&file, 0, at as *mut u8, file.size as usize)?;
        core::str::from_utf8(bios::bytes(at, file.size as usize)).map_err(|_| "the configuration file is not UTF-8")
    }
}
