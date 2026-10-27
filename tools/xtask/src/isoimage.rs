use std::fs;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::util::{file_name, Result};

const SECTOR: u64 = 2048;
const MBR_DAP_LBA: usize = 0x1a8;

pub struct Boot<'a> {
    pub file: &'a str,
    pub mbr: Option<&'a [u8]>,
    pub efi_file: Option<&'a str>,
}

struct Node {
    name: String,
    path: PathBuf,
    size: u64,
    children: Vec<usize>,
    parent: usize,
    dir: bool,
    lba: u32,
    iso_name: Vec<u8>,
    joliet_name: Vec<u8>,
    dir_size: [u32; 2],
    dir_lba: [u32; 2],
    number: u16,
}

fn d_name(name: &str, dir: bool) -> Vec<u8> {
    let upper: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else if c == '.' { '.' } else { '_' }).collect();
    if dir {
        return upper.replace('.', "_").into_bytes().into_iter().take(31).collect();
    }
    let (stem, ext) = match upper.rfind('.') {
        Some(i) => (upper[..i].replace('.', "_"), upper[i + 1..].to_string()),
        None => (upper.clone(), String::new()),
    };
    let ext: String = ext.chars().take(3).collect();
    let stem: String = stem.chars().take(30 - ext.len()).collect();
    format!("{}.{};1", stem, ext).into_bytes()
}

fn joliet(name: &str) -> Vec<u8> {
    name.encode_utf16().take(64).flat_map(|u| u.to_be_bytes()).collect()
}

fn both16(v: u16) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[..2].copy_from_slice(&v.to_le_bytes());
    b[2..].copy_from_slice(&v.to_be_bytes());
    b
}

fn both32(v: u32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&v.to_le_bytes());
    b[4..].copy_from_slice(&v.to_be_bytes());
    b
}

fn civil(epoch: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = (epoch / 86400) as i64;
    let rem = epoch % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u32;
    (year, month, day, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

fn now() -> u64 {
    if let Some(epoch) = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.parse().ok()) {
        return epoch;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn record_date(epoch: u64) -> [u8; 7] {
    let (y, m, d, h, mi, s) = civil(epoch);
    [(y - 1900) as u8, m as u8, d as u8, h as u8, mi as u8, s as u8, 0]
}

fn volume_date(epoch: u64) -> [u8; 17] {
    let (y, m, d, h, mi, s) = civil(epoch);
    let text = format!("{:04}{:02}{:02}{:02}{:02}{:02}00", y, m, d, h, mi, s);
    let mut out = [0u8; 17];
    out[..16].copy_from_slice(text.as_bytes());
    out
}

fn record(lba: u32, size: u32, dir: bool, name: &[u8], date: &[u8; 7]) -> Vec<u8> {
    let len = 33 + name.len() + (name.len() + 1) % 2;
    let mut r = vec![0u8; len];
    r[0] = len as u8;
    r[2..10].copy_from_slice(&both32(lba));
    r[10..18].copy_from_slice(&both32(size));
    r[18..25].copy_from_slice(date);
    r[25] = if dir { 2 } else { 0 };
    r[28..32].copy_from_slice(&both16(1));
    r[32] = name.len() as u8;
    r[33..33 + name.len()].copy_from_slice(name);
    r
}

fn pack(records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in records {
        let used = out.len() as u64 % SECTOR;
        if used + r.len() as u64 > SECTOR {
            out.resize(out.len() + (SECTOR - used) as usize, 0);
        }
        out.extend_from_slice(r);
    }
    let pad = (SECTOR - out.len() as u64 % SECTOR) % SECTOR;
    out.resize(out.len() + pad as usize, 0);
    out
}

struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn scan(root: &Path) -> Result<Tree> {
        let mut tree = Tree { nodes: Vec::new() };
        tree.nodes.push(Node { name: String::new(), path: root.to_path_buf(), size: 0, children: Vec::new(), parent: 0, dir: true, lba: 0, iso_name: vec![0], joliet_name: vec![0], dir_size: [0; 2], dir_lba: [0; 2], number: 1 });
        tree.add(0)?;
        Ok(tree)
    }

    fn add(&mut self, index: usize) -> Result {
        let mut entries: Vec<PathBuf> = fs::read_dir(&self.nodes[index].path).map_err(|e| format!("{}: {}", self.nodes[index].path.display(), e))?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        let mut seen = std::collections::BTreeSet::new();
        for path in entries {
            let meta = fs::metadata(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
            let name = file_name(&path);
            let iso_name = d_name(&name, meta.is_dir());
            if !seen.insert(iso_name.clone()) {
                return Err(format!("{}: two names map to the same ISO 9660 name", path.display()));
            }
            if meta.len() >= 1 << 32 {
                return Err(format!("{}: files of 4 GiB and larger are not supported", path.display()));
            }
            let child = self.nodes.len();
            self.nodes.push(Node {
                joliet_name: joliet(&name),
                name,
                size: if meta.is_dir() { 0 } else { meta.len() },
                children: Vec::new(),
                parent: index,
                dir: meta.is_dir(),
                lba: 0,
                iso_name,
                path: path.clone(),
                dir_size: [0; 2],
                dir_lba: [0; 2],
                number: 0,
            });
            self.nodes[index].children.push(child);
            if meta.is_dir() {
                self.add(child)?;
            }
        }
        Ok(())
    }

    fn dirs_bfs(&self) -> Vec<usize> {
        let mut order = vec![0usize];
        let mut i = 0;
        while i < order.len() {
            let mut kids: Vec<usize> = self.nodes[order[i]].children.iter().copied().filter(|&c| self.nodes[c].dir).collect();
            kids.sort_by(|a, b| self.nodes[*a].iso_name.cmp(&self.nodes[*b].iso_name));
            order.extend(kids);
            i += 1;
        }
        order
    }

    fn name(&self, index: usize, set: usize) -> &[u8] {
        if set == 0 { &self.nodes[index].iso_name } else { &self.nodes[index].joliet_name }
    }

    fn records(&self, index: usize, set: usize, date: &[u8; 7]) -> Vec<Vec<u8>> {
        let node = &self.nodes[index];
        let parent = &self.nodes[node.parent];
        let mut out = vec![record(node.dir_lba[set], node.dir_size[set], true, &[0], date), record(parent.dir_lba[set], parent.dir_size[set], true, &[1], date)];
        let mut kids = node.children.clone();
        kids.sort_by(|a, b| self.name(*a, set).cmp(self.name(*b, set)));
        for k in kids {
            let c = &self.nodes[k];
            let (lba, size) = if c.dir { (c.dir_lba[set], c.dir_size[set]) } else { (c.lba, c.size as u32) };
            out.push(record(lba, size, c.dir, self.name(k, set), date));
        }
        out
    }

    fn path_table(&self, order: &[usize], set: usize, big: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for &d in order {
            let node = &self.nodes[d];
            let name = if d == 0 { &[0u8][..] } else { self.name(d, set) };
            out.push(name.len() as u8);
            out.push(0);
            let lba = node.dir_lba[set];
            let parent = self.nodes[node.parent].number;
            if big {
                out.extend_from_slice(&lba.to_be_bytes());
                out.extend_from_slice(&parent.to_be_bytes());
            } else {
                out.extend_from_slice(&lba.to_le_bytes());
                out.extend_from_slice(&parent.to_le_bytes());
            }
            out.extend_from_slice(name);
            if name.len() % 2 == 1 {
                out.push(0);
            }
        }
        out
    }
}

fn text_field(out: &mut [u8], text: &str) {
    out.fill(b' ');
    let bytes = text.as_bytes();
    let n = bytes.len().min(out.len());
    out[..n].copy_from_slice(&bytes[..n]);
}

fn joliet_field(out: &mut [u8], text: &str) {
    for pair in out.chunks_mut(2) {
        pair.copy_from_slice(&[0, b' ']);
    }
    for (i, u) in text.encode_utf16().take(out.len() / 2).enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&u.to_be_bytes());
    }
}

fn sectors(bytes: u64) -> u32 {
    bytes.div_ceil(SECTOR) as u32
}

pub fn write(tree_root: &Path, out: &Path, volume: &str, boot: Option<&Boot>) -> Result {
    let mut tree = Tree::scan(tree_root)?;
    let epoch = now();
    let date = record_date(epoch);
    let boot_index = match boot {
        Some(boot) => {
            let mut current = 0usize;
            for part in boot.file.split('/').filter(|p| !p.is_empty()) {
                current = *tree.nodes[current].children.iter().find(|&&c| tree.nodes[c].name == part).ok_or_else(|| format!("{}: boot file {} is missing", tree_root.display(), boot.file))?;
            }
            Some(current)
        }
        None => None,
    };
    let efi_index = match boot.and_then(|boot| boot.efi_file) {
        Some(file) => {
            let mut current = 0usize;
            for part in file.split('/').filter(|p| !p.is_empty()) {
                current = *tree.nodes[current].children.iter().find(|&&c| tree.nodes[c].name == part).ok_or_else(|| format!("{}: EFI boot file {} is missing", tree_root.display(), file))?;
            }
            Some(current)
        }
        None => None,
    };
    let order = tree.dirs_bfs();
    for (i, &d) in order.iter().enumerate() {
        tree.nodes[d].number = (i + 1) as u16;
    }
    let mut next: u32 = 21;
    let mut tables = [[0u32; 2]; 2];
    let mut table_size = [0u32; 2];
    for set in 0..2 {
        let size = tree.path_table(&order, set, false).len() as u32;
        table_size[set] = size;
        for endian in 0..2 {
            tables[set][endian] = next;
            next += sectors(size as u64);
        }
    }
    for set in 0..2 {
        for _ in 0..2 {
            for &d in &order {
                let size = pack(&tree.records(d, set, &date)).len() as u32;
                tree.nodes[d].dir_size[set] = size;
            }
        }
        for &d in &order {
            tree.nodes[d].dir_lba[set] = next;
            next += sectors(tree.nodes[d].dir_size[set] as u64);
        }
    }
    let mut files: Vec<usize> = (0..tree.nodes.len()).filter(|&i| !tree.nodes[i].dir).collect();
    files.sort_by_key(|&i| if Some(i) == boot_index { 0 } else { 1 });
    for &f in &files {
        tree.nodes[f].lba = next;
        next += sectors(tree.nodes[f].size).max(1);
    }
    let total = next;
    let mut image = BufWriter::with_capacity(1 << 20, fs::File::create(out).map_err(|e| format!("{}: {}", out.display(), e))?);
    let put = |image: &mut BufWriter<fs::File>, lba: u32, data: &[u8]| -> Result {
        image.seek(SeekFrom::Start(lba as u64 * SECTOR)).map_err(|e| e.to_string())?;
        image.write_all(data).map_err(|e| e.to_string())
    };
    let root = |tree: &Tree, set: usize| record(tree.nodes[0].dir_lba[set], tree.nodes[0].dir_size[set], true, &[0], &date);
    for set in 0..2 {
        let mut vd = vec![0u8; SECTOR as usize];
        vd[0] = if set == 0 { 1 } else { 2 };
        vd[1..6].copy_from_slice(b"CD001");
        vd[6] = 1;
        if set == 0 {
            text_field(&mut vd[8..40], "");
            text_field(&mut vd[40..72], volume);
            text_field(&mut vd[190..318], "");
            text_field(&mut vd[318..446], "HAMIXOS");
            text_field(&mut vd[446..574], "HAMIXOS");
            text_field(&mut vd[574..702], "HAMIXOS XTASK");
            text_field(&mut vd[702..813], "");
        } else {
            joliet_field(&mut vd[8..40], "");
            joliet_field(&mut vd[40..72], volume);
            vd[88..91].copy_from_slice(&[0x25, 0x2f, 0x45]);
            joliet_field(&mut vd[190..318], "");
            joliet_field(&mut vd[318..446], "HamixOS");
            joliet_field(&mut vd[446..574], "HamixOS");
            joliet_field(&mut vd[574..702], "HamixOS xtask");
            joliet_field(&mut vd[702..812], "");
        }
        vd[80..88].copy_from_slice(&both32(total));
        vd[120..124].copy_from_slice(&both16(1));
        vd[124..128].copy_from_slice(&both16(1));
        vd[128..132].copy_from_slice(&both16(SECTOR as u16));
        vd[132..140].copy_from_slice(&both32(table_size[set]));
        vd[140..144].copy_from_slice(&tables[set][0].to_le_bytes());
        vd[148..152].copy_from_slice(&tables[set][1].to_be_bytes());
        let r = root(&tree, set);
        vd[156..156 + 34].copy_from_slice(&r[..34]);
        let stamp = volume_date(epoch);
        vd[813..830].copy_from_slice(&stamp);
        vd[830..847].copy_from_slice(&stamp);
        vd[847..864].copy_from_slice(&[b'0'; 16].iter().copied().chain([0]).collect::<Vec<u8>>());
        vd[864..881].copy_from_slice(&[b'0'; 16].iter().copied().chain([0]).collect::<Vec<u8>>());
        vd[881] = 1;
        put(&mut image, if set == 0 { 16 } else { 18 }, &vd)?;
    }
    if boot_index.is_some() {
        let mut br = vec![0u8; SECTOR as usize];
        br[1..6].copy_from_slice(b"CD001");
        br[6] = 1;
        br[7..7 + 23].copy_from_slice(b"EL TORITO SPECIFICATION");
        br[71..75].copy_from_slice(&20u32.to_le_bytes());
        put(&mut image, 17, &br)?;
    } else {
        let mut empty = vec![0u8; SECTOR as usize];
        empty[0] = 255;
        empty[1..6].copy_from_slice(b"CD001");
        empty[6] = 1;
        put(&mut image, 17, &empty)?;
    }
    let mut term = vec![0u8; SECTOR as usize];
    term[0] = 255;
    term[1..6].copy_from_slice(b"CD001");
    term[6] = 1;
    put(&mut image, 19, &term)?;
    let boot_lba = boot_index.map(|b| tree.nodes[b].lba).unwrap_or(0);
    let mut catalog = vec![0u8; SECTOR as usize];
    catalog[0] = 1;
    catalog[4..4 + 7].copy_from_slice(b"HamixOS");
    catalog[30] = 0x55;
    catalog[31] = 0xaa;
    let sum = catalog[..32].chunks(2).fold(0u16, |a, w| a.wrapping_add(u16::from_le_bytes([w[0], w[1]])));
    catalog[28..30].copy_from_slice(&0u16.wrapping_sub(sum).to_le_bytes());
    catalog[32] = 0x88;
    catalog[38..40].copy_from_slice(&4u16.to_le_bytes());
    catalog[40..44].copy_from_slice(&boot_lba.to_le_bytes());
    if let Some(index) = efi_index {
        catalog[64] = 0x91;
        catalog[65] = 0xef;
        catalog[66..68].copy_from_slice(&1u16.to_le_bytes());
        catalog[68] = 0x88;
        let node = &tree.nodes[index];
        catalog[74..76].copy_from_slice(&(node.size.div_ceil(512).min(u16::MAX as u64) as u16).to_le_bytes());
        catalog[76..80].copy_from_slice(&node.lba.to_le_bytes());
    }
    if boot_index.is_some() {
        put(&mut image, 20, &catalog)?;
    }
    for set in 0..2 {
        put(&mut image, tables[set][0], &tree.path_table(&order, set, false))?;
        put(&mut image, tables[set][1], &tree.path_table(&order, set, true))?;
        for &d in &order {
            put(&mut image, tree.nodes[d].dir_lba[set], &pack(&tree.records(d, set, &date)))?;
        }
    }
    for &f in &files {
        let node = &tree.nodes[f];
        if Some(f) == boot_index {
            let mut data = fs::read(&node.path).map_err(|e| format!("{}: {}", node.path.display(), e))?;
            if data.len() < 64 {
                return Err(format!("{}: boot file is too small", node.path.display()));
            }
            let len = data.len() as u32;
            data.resize(data.len().div_ceil(4) * 4, 0);
            let checksum = data[64..].chunks(4).fold(0u32, |a, w| a.wrapping_add(u32::from_le_bytes([w[0], w[1], w[2], w[3]])));
            data[8..12].copy_from_slice(&16u32.to_le_bytes());
            data[12..16].copy_from_slice(&node.lba.to_le_bytes());
            data[16..20].copy_from_slice(&len.to_le_bytes());
            data[20..24].copy_from_slice(&checksum.to_le_bytes());
            data.truncate(len as usize);
            put(&mut image, node.lba, &data)?;
        } else {
            image.seek(SeekFrom::Start(node.lba as u64 * SECTOR)).map_err(|e| e.to_string())?;
            let mut src = fs::File::open(&node.path).map_err(|e| format!("{}: {}", node.path.display(), e))?;
            std::io::copy(&mut src, &mut image).map_err(|e| format!("{}: {}", node.path.display(), e))?;
        }
    }
    let disk_sectors = (total as u64 * 4).div_ceil(2048) * 2048;
    let mbr = boot.and_then(|b| b.mbr);
    if let Some(code) = mbr {
        if code.len() > 0x1b8 {
            return Err(String::from("the hybrid MBR code is larger than 440 bytes"));
        }
        let mut mbr = vec![0u8; 512];
        mbr[..code.len()].copy_from_slice(code);
        mbr[MBR_DAP_LBA..MBR_DAP_LBA + 8].copy_from_slice(&(boot_lba as u64 * 4).to_le_bytes());
        mbr[0x1b8..0x1bc].copy_from_slice(&((epoch as u32) ^ 0x4858_4f53).to_le_bytes());
        let entry = &mut mbr[0x1be..0x1ce];
        entry[0] = 0x80;
        entry[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
        entry[4] = 0x17;
        entry[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
        entry[8..12].copy_from_slice(&0u32.to_le_bytes());
        entry[12..16].copy_from_slice(&(disk_sectors.min(u32::MAX as u64) as u32).to_le_bytes());
        mbr[510] = 0x55;
        mbr[511] = 0xaa;
        image.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        image.write_all(&mbr).map_err(|e| e.to_string())?;
    }
    image.flush().map_err(|e| e.to_string())?;
    let file = image.into_inner().map_err(|e| e.to_string())?;
    file.set_len(if mbr.is_some() { disk_sectors * 512 } else { total as u64 * SECTOR }).map_err(|e| e.to_string())?;
    Ok(())
}
