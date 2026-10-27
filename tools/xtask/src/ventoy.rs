use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::util::Result;

const GUID: [u8; 16] = [0x20, 0x20, 0x77, 0x77, 0x77, 0x2e, 0x76, 0x65, 0x6e, 0x74, 0x6f, 0x79, 0x2e, 0x6e, 0x65, 0x74];
const PARTITION_START: u64 = 2048;
const FRAGMENTS: u64 = 5;
const GAP_SECTORS: u64 = 2048;
pub const PARAM_ADDRESS: u32 = 0x80000;
const PLACEHOLDER: &[u8; 16] = b"HXVTOYPLACEHOLDR";

pub fn param_len() -> usize {
    512 + 28 + 16 * FRAGMENTS as usize
}

pub fn placeholder() -> Vec<u8> {
    let mut data = vec![0u8; param_len()];
    data[..16].copy_from_slice(PLACEHOLDER);
    data
}

pub fn patch_placeholder(iso: &Path, param: &[u8]) -> Result {
    if param.len() != param_len() {
        return Err(format!("the Ventoy table is {} bytes, expected {}", param.len(), param_len()));
    }
    let mut file = fs::OpenOptions::new().read(true).write(true).open(iso).map_err(|e| format!("{}: {}", iso.display(), e))?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut base = 0u64;
    loop {
        file.seek(SeekFrom::Start(base)).map_err(|e| e.to_string())?;
        let n = std::io::Read::read(&mut file, &mut buffer).map_err(|e| e.to_string())?;
        if n < PLACEHOLDER.len() {
            return Err(format!("{}: no Ventoy table placeholder", iso.display()));
        }
        let mut at = 0;
        while at + PLACEHOLDER.len() <= n {
            if &buffer[at..at + PLACEHOLDER.len()] == PLACEHOLDER && (base + at as u64) % 2048 == 0 {
                file.seek(SeekFrom::Start(base + at as u64)).map_err(|e| e.to_string())?;
                return file.write_all(param).map_err(|e| e.to_string());
            }
            at += 2048 - ((base + at as u64) % 2048) as usize;
        }
        base += n as u64;
    }
}
const SIGNATURE: [u8; 4] = *b"VTOY";

pub fn fake_windows_disk(disk: &Path) -> Result {
    let mut out = fs::File::create(disk).map_err(|e| format!("{}: {}", disk.display(), e))?;
    out.set_len(64 << 20).map_err(|e| e.to_string())?;
    let mut mbr = vec![0u8; 512];
    mbr[440..444].copy_from_slice(&0x1a2b3c4du32.to_le_bytes());
    let entry = &mut mbr[462..478];
    entry[4] = 0x07;
    entry[8..12].copy_from_slice(&2048u32.to_le_bytes());
    entry[12..16].copy_from_slice(&65536u32.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    out.write_all(&mbr).map_err(|e| e.to_string())?;
    let mut vbr = vec![0u8; 512];
    vbr[0..3].copy_from_slice(&[0xeb, 0x52, 0x90]);
    vbr[3..11].copy_from_slice(b"NTFS    ");
    let code = [0xbe, 0x70, 0x7c, 0xba, 0xf8, 0x03, 0xac, 0x84, 0xc0, 0x74, 0x03, 0xee, 0xeb, 0xf8, 0xf4, 0xeb, 0xfd];
    vbr[0x54..0x54 + code.len()].copy_from_slice(&code);
    let message = b"CHAIN OK: hxboot started the Windows boot sector\r\n\0";
    vbr[0x70..0x70 + message.len()].copy_from_slice(message);
    vbr[510] = 0x55;
    vbr[511] = 0xaa;
    out.seek(SeekFrom::Start(2048 * 512)).map_err(|e| e.to_string())?;
    out.write_all(&vbr).map_err(|e| e.to_string())
}

pub fn ntfs_stick(stick: &Path, iso: &Path, param: &Path) -> Result {
    let size = fs::metadata(iso).map_err(|e| format!("{}: {}", iso.display(), e))?.len();
    let image_sectors = size.div_ceil(2048);
    let per = image_sectors.div_ceil(FRAGMENTS);
    let mut regions = Vec::new();
    let mut disk_at = PARTITION_START + 2048;
    let mut placed = Vec::new();
    for k in (0..FRAGMENTS).rev() {
        let start = k * per;
        if start >= image_sectors {
            continue;
        }
        let count = per.min(image_sectors - start);
        placed.push((k, start, count, disk_at));
        disk_at += count * 4 + GAP_SECTORS;
    }
    placed.sort_by_key(|p| p.0);
    let total_sectors = disk_at + 2048;
    let mut out = fs::File::create(stick).map_err(|e| format!("{}: {}", stick.display(), e))?;
    out.set_len(total_sectors * 512).map_err(|e| e.to_string())?;
    let junk = vec![0xa5u8; (GAP_SECTORS * 512) as usize];
    let mut src = fs::File::open(iso).map_err(|e| format!("{}: {}", iso.display(), e))?;
    for &(_, start, count, at) in &placed {
        src.seek(SeekFrom::Start(start * 2048)).map_err(|e| e.to_string())?;
        out.seek(SeekFrom::Start(at * 512)).map_err(|e| e.to_string())?;
        std::io::copy(&mut std::io::Read::take(&mut src, count * 2048), &mut out).map_err(|e| e.to_string())?;
        out.seek(SeekFrom::Start((at + count * 4) * 512)).map_err(|e| e.to_string())?;
        out.write_all(&junk).map_err(|e| e.to_string())?;
        regions.push((count as u32, start as u32, at));
    }
    let mut mbr = vec![0u8; 512];
    mbr[440..444].copy_from_slice(&SIGNATURE);
    let entry = &mut mbr[446..462];
    entry[4] = 0x07;
    entry[8..12].copy_from_slice(&(PARTITION_START as u32).to_le_bytes());
    entry[12..16].copy_from_slice(&((total_sectors - PARTITION_START) as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    out.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    out.write_all(&mbr).map_err(|e| e.to_string())?;
    let mut ntfs = vec![0u8; 512];
    ntfs[3..11].copy_from_slice(b"NTFS    ");
    out.seek(SeekFrom::Start(PARTITION_START * 512)).map_err(|e| e.to_string())?;
    out.write_all(&ntfs).map_err(|e| e.to_string())?;
    let mut location = Vec::new();
    location.extend_from_slice(&GUID);
    location.extend_from_slice(&2048u32.to_le_bytes());
    location.extend_from_slice(&512u32.to_le_bytes());
    location.extend_from_slice(&(regions.len() as u32).to_le_bytes());
    for (count, start, at) in &regions {
        location.extend_from_slice(&count.to_le_bytes());
        location.extend_from_slice(&start.to_le_bytes());
        location.extend_from_slice(&at.to_le_bytes());
    }
    let mut table = vec![0u8; 512];
    table[0..16].copy_from_slice(&GUID);
    table[33..41].copy_from_slice(&(total_sectors * 512).to_le_bytes());
    table[41..43].copy_from_slice(&1u16.to_le_bytes());
    table[43..45].copy_from_slice(&1u16.to_le_bytes());
    let path = b"/HamixOS.iso";
    table[45..45 + path.len()].copy_from_slice(path);
    table[429..437].copy_from_slice(&size.to_le_bytes());
    table[437..445].copy_from_slice(&((PARAM_ADDRESS + 512) as u64).to_le_bytes());
    table[445..449].copy_from_slice(&(location.len() as u32).to_le_bytes());
    table[481..485].copy_from_slice(&SIGNATURE);
    let sum = table.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    table[16] = 0u8.wrapping_sub(sum);
    table.extend_from_slice(&location);
    fs::write(param, &table).map_err(|e| format!("{}: {}", param.display(), e))
}
