#![no_std]

extern crate alloc;

mod crc;
mod device;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub use crc::crc32;
pub use device::{BlockDevice, MemDevice, VecDevice, SECTOR};

pub const ROOT_INO: u32 = 2;
pub const S_IFMT: u16 = 0xF000;
pub const S_IFDIR: u16 = 0x4000;
pub const S_IFREG: u16 = 0x8000;
pub const S_IFLNK: u16 = 0xA000;

const CLEAN_BLOCKS: usize = 256;
const EXT2_MAGIC: u16 = 0xEF53;
const HEXT_MAGIC: u32 = 0x5458_4548;
const HEXT_VERSION: u32 = 1;
const HEXT_OFFSET: usize = 0x300;
const JOURNAL_INO: u32 = 5;
const LOST_FOUND_INO: u32 = 11;
const FIRST_INO: u32 = 11;
const INODE_SIZE: usize = 128;
const FEATURE_INCOMPAT_FILETYPE: u32 = 0x2;
const FEATURE_RO_SPARSE_SUPER: u32 = 0x1;
const FEATURE_RO_LARGE_FILE: u32 = 0x2;
const JOURNAL_HEADER: u32 = 0x484A_5848;
const JOURNAL_DESCRIPTOR: u32 = 0x444A_5848;
const JOURNAL_COMMIT: u32 = 0x434A_5848;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io,
    NotHext,
    Corrupt,
    NoSpace,
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    InvalidName,
    TooLarge,
}

pub type Result<T> = core::result::Result<T, Error>;

fn get16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn get32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn get64(b: &[u8], o: usize) -> u64 {
    get32(b, o) as u64 | (get32(b, o + 4) as u64) << 32
}

fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

#[derive(Clone, Debug)]
pub struct Inode {
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub links: u16,
    pub blocks512: u32,
    pub flags: u32,
    pub block: [u32; 15],
}

impl Inode {
    fn empty() -> Self {
        Self { mode: 0, uid: 0, gid: 0, size: 0, atime: 0, ctime: 0, mtime: 0, dtime: 0, links: 0, blocks512: 0, flags: 0, block: [0; 15] }
    }

    fn decode(b: &[u8]) -> Self {
        let mut block = [0u32; 15];
        for (i, slot) in block.iter_mut().enumerate() {
            *slot = get32(b, 40 + i * 4);
        }
        Self {
            mode: get16(b, 0),
            uid: get16(b, 2) as u32 | (get16(b, 120) as u32) << 16,
            size: get32(b, 4) as u64 | (get32(b, 108) as u64) << 32,
            atime: get32(b, 8),
            ctime: get32(b, 12),
            mtime: get32(b, 16),
            dtime: get32(b, 20),
            gid: get16(b, 24) as u32 | (get16(b, 122) as u32) << 16,
            links: get16(b, 26),
            blocks512: get32(b, 28),
            flags: get32(b, 32),
            block,
        }
    }

    fn encode(&self, b: &mut [u8]) {
        b[..INODE_SIZE].fill(0);
        put16(b, 0, self.mode);
        put16(b, 2, self.uid as u16);
        put32(b, 4, self.size as u32);
        put32(b, 8, self.atime);
        put32(b, 12, self.ctime);
        put32(b, 16, self.mtime);
        put32(b, 20, self.dtime);
        put16(b, 24, self.gid as u16);
        put16(b, 26, self.links);
        put32(b, 28, self.blocks512);
        put32(b, 32, self.flags);
        for (i, v) in self.block.iter().enumerate() {
            put32(b, 40 + i * 4, *v);
        }
        put32(b, 108, (self.size >> 32) as u32);
        put16(b, 120, (self.uid >> 16) as u16);
        put16(b, 122, (self.gid >> 16) as u16);
    }

    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub ino: u32,
    pub file_type: u8,
}

impl DirEntry {
    pub fn is_dir(&self) -> bool {
        self.file_type == 2
    }
}

#[derive(Clone, Copy)]
struct Group {
    block_bitmap: u32,
    inode_bitmap: u32,
    inode_table: u32,
    free_blocks: u16,
    free_inodes: u16,
    used_dirs: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub block_size: u32,
    pub blocks: u32,
    pub free_blocks: u32,
    pub inodes: u32,
    pub free_inodes: u32,
    pub journal_blocks: u32,
    pub sequence: u64,
    pub replayed: bool,
    pub generation: u64,
}

pub struct Hext<D: BlockDevice> {
    dev: D,
    bs: usize,
    sb: Vec<u8>,
    groups: Vec<Group>,
    blocks_per_group: u32,
    inodes_per_group: u32,
    first_data_block: u32,
    blocks_count: u32,
    inodes_count: u32,
    cache: BTreeMap<u32, Vec<u8>>,
    clean: BTreeMap<u32, Vec<u8>>,
    fresh: BTreeSet<u32>,
    freed: BTreeSet<u32>,
    journal: Vec<u32>,
    next_seq: u64,
    replayed: bool,
    now: u32,
    hint: u32,
}

fn is_sparse_group(g: u32) -> bool {
    if g <= 1 {
        return true;
    }
    for base in [3u32, 5, 7] {
        let mut p = base;
        while p < g {
            p = match p.checked_mul(base) {
                Some(v) => v,
                None => break,
            };
        }
        if p == g {
            return true;
        }
    }
    false
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn file_type_of(mode: u16) -> u8 {
    match mode & S_IFMT {
        S_IFDIR => 2,
        S_IFLNK => 7,
        _ => 1,
    }
}

pub struct FormatOptions {
    pub label: String,
    pub seed: u64,
    pub now: u32,
}

impl<D: BlockDevice> Hext<D> {
    pub fn device(&mut self) -> &mut D {
        &mut self.dev
    }

    pub fn into_device(self) -> D {
        self.dev
    }

    pub fn set_time(&mut self, now: u32) {
        self.now = now;
    }

    pub fn block_size(&self) -> usize {
        self.bs
    }

    fn read_raw(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
        let sectors_per_block = (self.bs / SECTOR) as u64;
        self.dev.read(block as u64 * sectors_per_block, buf)
    }

    fn write_raw(&mut self, block: u32, buf: &[u8]) -> Result<()> {
        if !self.clean.is_empty() {
            let count = buf.len().div_ceil(self.bs) as u32;
            let stale: Vec<u32> = self.clean.range(block..block + count.max(1)).map(|(k, _)| *k).collect();
            for key in stale {
                self.clean.remove(&key);
            }
        }
        let sectors_per_block = (self.bs / SECTOR) as u64;
        self.dev.write(block as u64 * sectors_per_block, buf)
    }

    fn read_block(&mut self, block: u32) -> Result<Vec<u8>> {
        if block >= self.blocks_count {
            return Err(Error::Corrupt);
        }
        if let Some(data) = self.cache.get(&block) {
            return Ok(data.clone());
        }
        if let Some(data) = self.clean.get(&block) {
            return Ok(data.clone());
        }
        let mut buf = vec![0u8; self.bs];
        self.read_raw(block, &mut buf)?;
        if self.clean.len() >= CLEAN_BLOCKS {
            self.clean.clear();
        }
        self.clean.insert(block, buf.clone());
        Ok(buf)
    }

    fn stage(&mut self, block: u32, data: Vec<u8>) {
        self.cache.insert(block, data);
    }

    pub fn format(dev: D, options: FormatOptions) -> Result<Self> {
        let total_bytes = dev.sectors() * SECTOR as u64;
        if total_bytes < 2 * 1024 * 1024 {
            return Err(Error::NoSpace);
        }
        let bs: usize = if total_bytes <= 512 * 1024 * 1024 { 1024 } else { 4096 };
        let first_data_block: u32 = if bs == 1024 { 1 } else { 0 };
        let mut blocks_count = (total_bytes / bs as u64).min(u32::MAX as u64 - 1) as u32;
        let blocks_per_group = (bs * 8) as u32;
        let inodes_per_block = (bs / INODE_SIZE) as u32;

        let mut groups_count = (blocks_count - first_data_block).div_ceil(blocks_per_group);
        let gdt_blocks = (groups_count as usize * 32).div_ceil(bs) as u32;
        let bytes_per_inode = if bs == 1024 { 4096u64 } else { 16384 };
        let mut inodes_per_group = ((blocks_per_group as u64 * bs as u64) / bytes_per_inode) as u32;
        inodes_per_group = inodes_per_group.div_ceil(inodes_per_block) * inodes_per_block;
        inodes_per_group = inodes_per_group.min(blocks_per_group).max(inodes_per_block * 4);
        let itable_blocks = inodes_per_group / inodes_per_block;

        let overhead = |g: u32| -> u32 {
            let sb = if is_sparse_group(g) { 1 + gdt_blocks } else { 0 };
            sb + 2 + itable_blocks
        };
        let last_start = first_data_block + (groups_count - 1) * blocks_per_group;
        if blocks_count - last_start < overhead(groups_count - 1) + 64 {
            if groups_count == 1 {
                return Err(Error::NoSpace);
            }
            groups_count -= 1;
            blocks_count = first_data_block + groups_count * blocks_per_group;
        }

        let mut fs = Hext {
            dev,
            bs,
            sb: vec![0u8; 1024],
            groups: Vec::new(),
            blocks_per_group,
            inodes_per_group,
            first_data_block,
            blocks_count,
            inodes_count: inodes_per_group * groups_count,
            cache: BTreeMap::new(),
            clean: BTreeMap::new(),
            fresh: BTreeSet::new(),
            freed: BTreeSet::new(),
            journal: Vec::new(),
            next_seq: 1,
            replayed: false,
            now: options.now,
            hint: 0,
        };

        let zero = vec![0u8; bs];
        for g in 0..groups_count {
            let base = first_data_block + g * blocks_per_group;
            let meta = base + if is_sparse_group(g) { 1 + gdt_blocks } else { 0 };
            let group_blocks = (blocks_count - base).min(blocks_per_group);
            let used = overhead(g);
            fs.groups.push(Group {
                block_bitmap: meta,
                inode_bitmap: meta + 1,
                inode_table: meta + 2,
                free_blocks: (group_blocks - used) as u16,
                free_inodes: inodes_per_group as u16,
                used_dirs: 0,
            });

            let mut bitmap = vec![0u8; bs];
            for bit in 0..used {
                bitmap[(bit / 8) as usize] |= 1 << (bit % 8);
            }
            for bit in group_blocks..(bs as u32 * 8) {
                bitmap[(bit / 8) as usize] |= 1 << (bit % 8);
            }
            fs.write_raw(meta, &bitmap)?;
            let mut ibitmap = vec![0u8; bs];
            for bit in inodes_per_group..(bs as u32 * 8) {
                ibitmap[(bit / 8) as usize] |= 1 << (bit % 8);
            }
            fs.write_raw(meta + 1, &ibitmap)?;
            let chunk = 64usize;
            let mut t = 0u32;
            while t < itable_blocks {
                let n = (itable_blocks - t).min(chunk as u32);
                let big = vec![0u8; bs * n as usize];
                fs.write_raw(meta + 2 + t, &big)?;
                t += n;
            }
        }
        let _ = zero;

        let sb = &mut fs.sb;
        put32(sb, 0, fs.inodes_count);
        put32(sb, 4, blocks_count);
        put32(sb, 8, blocks_count / 100);
        put32(sb, 20, first_data_block);
        put32(sb, 24, (bs / 1024).trailing_zeros());
        put32(sb, 28, (bs / 1024).trailing_zeros());
        put32(sb, 32, blocks_per_group);
        put32(sb, 36, blocks_per_group);
        put32(sb, 40, inodes_per_group);
        put32(sb, 44, options.now);
        put32(sb, 48, options.now);
        put16(sb, 54, 0xFFFF);
        put16(sb, 56, EXT2_MAGIC);
        put16(sb, 58, 1);
        put16(sb, 60, 1);
        put32(sb, 64, options.now);
        put32(sb, 76, 1);
        put32(sb, 84, FIRST_INO);
        put16(sb, 88, INODE_SIZE as u16);
        put32(sb, 96, FEATURE_INCOMPAT_FILETYPE);
        put32(sb, 100, FEATURE_RO_SPARSE_SUPER | FEATURE_RO_LARGE_FILE);
        let mut state = options.seed ^ 0x9E37_79B9_7F4A_7C15;
        for i in 0..16 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            sb[104 + i] = state as u8;
        }
        for (i, b) in options.label.bytes().take(16).enumerate() {
            sb[120 + i] = b;
        }
        put32(sb, HEXT_OFFSET, HEXT_MAGIC);
        put32(sb, HEXT_OFFSET + 4, HEXT_VERSION);
        put32(sb, HEXT_OFFSET + 8, JOURNAL_INO);

        for ino in 1..FIRST_INO {
            fs.mark_inode_used(ino, false)?;
        }

        let root = Inode {
            mode: S_IFDIR | 0o755,
            links: 2,
            atime: options.now,
            ctime: options.now,
            mtime: options.now,
            ..Inode::empty()
        };
        fs.write_inode(ROOT_INO, &root)?;
        fs.groups[0].used_dirs += 1;
        fs.init_directory(ROOT_INO, ROOT_INO)?;

        let journal_len = (blocks_count / 64).clamp(256, 8192).min(blocks_count / 4);
        let mut journal_inode = Inode {
            mode: S_IFREG | 0o600,
            links: 1,
            atime: options.now,
            ctime: options.now,
            mtime: options.now,
            ..Inode::empty()
        };
        let mut journal_blocks = Vec::with_capacity(journal_len as usize);
        for _ in 0..journal_len {
            journal_blocks.push(fs.alloc_block(0)?);
        }
        let mut header = vec![0u8; bs];
        put32(&mut header, 0, JOURNAL_HEADER);
        put32(&mut header, 4, 1);
        put64(&mut header, 8, 1);
        let crc = crc32(&header[..16]);
        put32(&mut header, 16, crc);
        fs.write_raw(journal_blocks[0], &header)?;
        let indirect = fs.build_pointers(&mut journal_inode, &journal_blocks, true)?;
        journal_inode.size = journal_len as u64 * bs as u64;
        journal_inode.blocks512 = ((journal_len as usize + indirect) * bs / 512) as u32;
        fs.write_inode(JOURNAL_INO, &journal_inode)?;
        fs.journal = journal_blocks;

        fs.create(ROOT_INO, "lost+found", S_IFDIR | 0o700, 0, 0)?;

        fs.flush_metadata_direct()?;
        fs.write_backups(gdt_blocks)?;
        fs.fresh.clear();
        fs.freed.clear();
        fs.dev.flush()?;
        Ok(fs)
    }

    fn write_backups(&mut self, gdt_blocks: u32) -> Result<()> {
        let gdt = self.encode_groups();
        for g in 1..self.groups.len() as u32 {
            if !is_sparse_group(g) {
                continue;
            }
            let base = self.first_data_block + g * self.blocks_per_group;
            let mut sb = self.sb.clone();
            put16(&mut sb, 90, g as u16);
            let mut block = vec![0u8; self.bs];
            if self.bs == 1024 {
                block.copy_from_slice(&sb);
            } else {
                block[..1024].copy_from_slice(&sb);
            }
            self.write_raw(base, &block)?;
            for i in 0..gdt_blocks {
                let start = i as usize * self.bs;
                let mut chunk = vec![0u8; self.bs];
                let end = (start + self.bs).min(gdt.len());
                if start < gdt.len() {
                    chunk[..end - start].copy_from_slice(&gdt[start..end]);
                }
                self.write_raw(base + 1 + i, &chunk)?;
            }
        }
        Ok(())
    }

    fn encode_groups(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.groups.len() * 32];
        for (i, g) in self.groups.iter().enumerate() {
            let o = i * 32;
            put32(&mut out, o, g.block_bitmap);
            put32(&mut out, o + 4, g.inode_bitmap);
            put32(&mut out, o + 8, g.inode_table);
            put16(&mut out, o + 12, g.free_blocks);
            put16(&mut out, o + 14, g.free_inodes);
            put16(&mut out, o + 16, g.used_dirs);
        }
        out
    }

    fn stage_superblock_and_groups(&mut self) -> Result<()> {
        let free_blocks: u32 = self.groups.iter().map(|g| g.free_blocks as u32).sum();
        let free_inodes: u32 = self.groups.iter().map(|g| g.free_inodes as u32).sum();
        put32(&mut self.sb, 12, free_blocks);
        put32(&mut self.sb, 16, free_inodes);
        put32(&mut self.sb, 48, self.now);
        put16(&mut self.sb, 90, 0);
        put16(&mut self.sb, 58, 1);
        put32(&mut self.sb, HEXT_OFFSET + 16, 0);
        let crc = crc32(&self.sb[..HEXT_OFFSET + 16]);
        put32(&mut self.sb, HEXT_OFFSET + 16, crc);

        let sb_block = if self.bs == 1024 { 1 } else { 0 };
        let mut block = self.read_block(sb_block)?;
        if self.bs == 1024 {
            block.copy_from_slice(&self.sb);
        } else {
            block[1024..2048].copy_from_slice(&self.sb);
        }
        self.stage(sb_block, block);

        let gdt = self.encode_groups();
        let gdt_start = self.first_data_block + 1;
        for (i, chunk) in gdt.chunks(self.bs).enumerate() {
            let mut block = self.read_block(gdt_start + i as u32)?;
            block[..chunk.len()].copy_from_slice(chunk);
            self.stage(gdt_start + i as u32, block);
        }
        Ok(())
    }

    fn flush_metadata_direct(&mut self) -> Result<()> {
        self.stage_superblock_and_groups()?;
        let cache = core::mem::take(&mut self.cache);
        for (block, data) in cache {
            self.write_raw(block, &data)?;
        }
        Ok(())
    }

    pub fn probe(dev: &mut D) -> bool {
        let mut buf = [0u8; 1024];
        if dev.read(2, &mut buf).is_err() {
            return false;
        }
        get16(&buf, 56) == EXT2_MAGIC && get32(&buf, HEXT_OFFSET) == HEXT_MAGIC
    }

    pub fn mount(mut dev: D, now: u32) -> Result<Self> {
        let mut sb = vec![0u8; 1024];
        dev.read(2, &mut sb)?;
        if get16(&sb, 56) != EXT2_MAGIC || get32(&sb, HEXT_OFFSET) != HEXT_MAGIC {
            return Err(Error::NotHext);
        }
        let log = get32(&sb, 24);
        if log > 2 || get16(&sb, 88) as usize != INODE_SIZE {
            return Err(Error::Corrupt);
        }
        let bs = 1024usize << log;
        let blocks_count = get32(&sb, 4);
        let first_data_block = get32(&sb, 20);
        let blocks_per_group = get32(&sb, 32);
        let inodes_per_group = get32(&sb, 40);
        if blocks_per_group == 0 || inodes_per_group == 0 || (blocks_count as u64 * bs as u64) > dev.sectors() * SECTOR as u64 {
            return Err(Error::Corrupt);
        }
        let groups_count = (blocks_count - first_data_block).div_ceil(blocks_per_group);
        let mut fs = Hext {
            dev,
            bs,
            sb,
            groups: Vec::new(),
            blocks_per_group,
            inodes_per_group,
            first_data_block,
            blocks_count,
            inodes_count: get32(&[0u8; 4], 0),
            cache: BTreeMap::new(),
            clean: BTreeMap::new(),
            fresh: BTreeSet::new(),
            freed: BTreeSet::new(),
            journal: Vec::new(),
            next_seq: 1,
            replayed: false,
            now,
            hint: 0,
        };
        fs.inodes_count = get32(&fs.sb, 0);

        fs.load_groups(groups_count)?;
        let journal_ino = get32(&fs.sb, HEXT_OFFSET + 8);
        let journal_inode = fs.read_inode(journal_ino)?;
        fs.journal = fs.block_list(&journal_inode, false)?;
        if fs.journal.len() < 8 {
            return Err(Error::Corrupt);
        }
        fs.replay()?;
        if fs.replayed {
            fs.load_groups(groups_count)?;
        }
        let generation = get64(&fs.sb, HEXT_OFFSET + 20).wrapping_add(1);
        put64(&mut fs.sb, HEXT_OFFSET + 20, generation);
        let mounts = get16(&fs.sb, 52).wrapping_add(1);
        put16(&mut fs.sb, 52, mounts);
        put32(&mut fs.sb, 44, now);
        Ok(fs)
    }

    fn load_groups(&mut self, groups_count: u32) -> Result<()> {
        let gdt_blocks = (groups_count as usize * 32).div_ceil(self.bs);
        let mut gdt = Vec::with_capacity(gdt_blocks * self.bs);
        for i in 0..gdt_blocks {
            let block = self.read_block(self.first_data_block + 1 + i as u32)?;
            gdt.extend_from_slice(&block);
        }
        self.groups.clear();
        for g in 0..groups_count as usize {
            let o = g * 32;
            self.groups.push(Group {
                block_bitmap: get32(&gdt, o),
                inode_bitmap: get32(&gdt, o + 4),
                inode_table: get32(&gdt, o + 8),
                free_blocks: get16(&gdt, o + 12),
                free_inodes: get16(&gdt, o + 14),
                used_dirs: get16(&gdt, o + 16),
            });
        }
        Ok(())
    }

    fn replay(&mut self) -> Result<()> {
        let header = self.read_block(self.journal[0])?;
        if get32(&header, 0) != JOURNAL_HEADER || crc32(&header[..16]) != get32(&header, 16) {
            return Err(Error::Corrupt);
        }
        self.next_seq = get64(&header, 8);
        let seq = self.next_seq;
        let mut position = 1usize;
        let mut writes: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut data_crc_input: Vec<u8> = Vec::new();
        loop {
            if position >= self.journal.len() {
                return Ok(());
            }
            let desc = self.read_block(self.journal[position])?;
            if get32(&desc, 0) == JOURNAL_COMMIT {
                if get64(&desc, 4) != seq
                    || get32(&desc, 12) as usize != writes.len()
                    || get32(&desc, 16) != crc32(&data_crc_input)
                    || get32(&desc, 20) != crc32(&desc[..20])
                {
                    return Ok(());
                }
                break;
            }
            if get32(&desc, 0) != JOURNAL_DESCRIPTOR || get64(&desc, 4) != seq {
                return Ok(());
            }
            let count = get32(&desc, 12) as usize;
            let capacity = (self.bs - 24) / 4;
            if count > capacity || get32(&desc, 16) != crc32(&desc[..16]) {
                return Ok(());
            }
            let targets: Vec<u32> = (0..count).map(|i| get32(&desc, 24 + i * 4)).collect();
            if get32(&desc, 20) != crc32(&desc[24..24 + count * 4]) {
                return Ok(());
            }
            position += 1;
            for target in targets {
                if position >= self.journal.len() || target >= self.blocks_count {
                    return Ok(());
                }
                let data = self.read_block(self.journal[position])?;
                data_crc_input.extend_from_slice(&data);
                writes.push((target, data));
                position += 1;
            }
        }
        for (target, data) in &writes {
            self.write_raw(*target, data)?;
        }
        self.dev.flush()?;
        self.next_seq += 1;
        self.write_journal_header()?;
        self.dev.flush()?;
        self.replayed = true;
        let mut sb = vec![0u8; 1024];
        self.dev.read(2, &mut sb)?;
        self.sb = sb;
        Ok(())
    }

    fn write_journal_header(&mut self) -> Result<()> {
        let mut header = vec![0u8; self.bs];
        put32(&mut header, 0, JOURNAL_HEADER);
        put32(&mut header, 4, 1);
        put64(&mut header, 8, self.next_seq);
        let crc = crc32(&header[..16]);
        put32(&mut header, 16, crc);
        let block = self.journal[0];
        self.write_raw(block, &header)
    }

    pub fn dirty(&self) -> bool {
        !self.cache.is_empty() || !self.freed.is_empty()
    }

    pub fn commit(&mut self) -> Result<()> {
        if !self.dirty() {
            self.fresh.clear();
            return Ok(());
        }
        self.stage_superblock_and_groups()?;
        self.dev.flush()?;

        let entries: Vec<(u32, Vec<u8>)> = core::mem::take(&mut self.cache).into_iter().collect();
        let capacity = (self.bs - 24) / 4;
        let journal_room = self.journal.len() - 2;
        let mut start = 0usize;
        while start < entries.len() {
            let mut end = start;
            let mut used = 0usize;
            while end < entries.len() {
                let descriptors_after = (end - start + 1).div_ceil(capacity);
                if descriptors_after + (end - start + 1) > journal_room {
                    break;
                }
                end += 1;
                used = descriptors_after + (end - start);
            }
            if end == start {
                return Err(Error::TooLarge);
            }
            let _ = used;
            self.write_transaction(&entries[start..end])?;
            start = end;
        }
        self.fresh.clear();
        self.freed.clear();
        Ok(())
    }

    fn write_transaction(&mut self, entries: &[(u32, Vec<u8>)]) -> Result<()> {
        let seq = self.next_seq;
        let capacity = (self.bs - 24) / 4;
        let mut position = 1usize;
        let mut data_crc_input = Vec::with_capacity(entries.len() * self.bs);
        for chunk in entries.chunks(capacity) {
            let mut desc = vec![0u8; self.bs];
            put32(&mut desc, 0, JOURNAL_DESCRIPTOR);
            put64(&mut desc, 4, seq);
            put32(&mut desc, 12, chunk.len() as u32);
            for (i, (target, _)) in chunk.iter().enumerate() {
                put32(&mut desc, 24 + i * 4, *target);
            }
            let crc_head = crc32(&desc[..16]);
            put32(&mut desc, 16, crc_head);
            let crc_targets = crc32(&desc[24..24 + chunk.len() * 4]);
            put32(&mut desc, 20, crc_targets);
            let block = self.journal[position];
            self.write_raw(block, &desc)?;
            position += 1;
            for (_, data) in chunk {
                let block = self.journal[position];
                self.write_raw(block, data)?;
                data_crc_input.extend_from_slice(data);
                position += 1;
            }
        }
        self.dev.flush()?;
        let mut commit = vec![0u8; self.bs];
        put32(&mut commit, 0, JOURNAL_COMMIT);
        put64(&mut commit, 4, seq);
        put32(&mut commit, 12, entries.len() as u32);
        put32(&mut commit, 16, crc32(&data_crc_input));
        let crc = crc32(&commit[..20]);
        put32(&mut commit, 20, crc);
        let block = self.journal[position];
        self.write_raw(block, &commit)?;
        self.dev.flush()?;

        for (target, data) in entries {
            self.write_raw(*target, data)?;
        }
        self.dev.flush()?;
        self.next_seq += 1;
        self.write_journal_header()?;
        self.dev.flush()
    }

    fn group_of_block(&self, block: u32) -> (usize, u32) {
        let rel = block - self.first_data_block;
        ((rel / self.blocks_per_group) as usize, rel % self.blocks_per_group)
    }

    fn alloc_block(&mut self, goal: usize) -> Result<u32> {
        let count = self.groups.len();
        for step in 0..count {
            let g = (goal + step) % count;
            if self.groups[g].free_blocks == 0 {
                continue;
            }
            let bitmap_block = self.groups[g].block_bitmap;
            let mut bitmap = self.read_block(bitmap_block)?;
            let base = self.first_data_block + g as u32 * self.blocks_per_group;
            let start = if step == 0 { self.hint.min(self.blocks_per_group) } else { 0 };
            for pass in 0..2 {
                let (lo, hi) = if pass == 0 { (start, self.blocks_per_group) } else { (0, start) };
                let mut bit = lo;
                while bit < hi {
                    let byte = bitmap[(bit / 8) as usize];
                    if byte == 0xFF {
                        bit = (bit / 8 + 1) * 8;
                        continue;
                    }
                    if byte & (1 << (bit % 8)) == 0 {
                        let block = base + bit;
                        if block < self.blocks_count && !self.freed.contains(&block) {
                            bitmap[(bit / 8) as usize] |= 1 << (bit % 8);
                            self.stage(bitmap_block, bitmap);
                            self.groups[g].free_blocks -= 1;
                            self.fresh.insert(block);
                            self.hint = bit + 1;
                            return Ok(block);
                        }
                    }
                    bit += 1;
                }
            }
        }
        Err(Error::NoSpace)
    }

    fn free_block(&mut self, block: u32) -> Result<()> {
        if block < self.first_data_block || block >= self.blocks_count {
            return Err(Error::Corrupt);
        }
        let (g, bit) = self.group_of_block(block);
        let bitmap_block = self.groups[g].block_bitmap;
        let mut bitmap = self.read_block(bitmap_block)?;
        if bitmap[(bit / 8) as usize] & (1 << (bit % 8)) != 0 {
            bitmap[(bit / 8) as usize] &= !(1 << (bit % 8));
            self.stage(bitmap_block, bitmap);
            self.groups[g].free_blocks += 1;
            if !self.fresh.remove(&block) {
                self.freed.insert(block);
            }
            self.cache.remove(&block).map(|_| ());
        }
        Ok(())
    }

    fn mark_inode_used(&mut self, ino: u32, dir: bool) -> Result<()> {
        let g = ((ino - 1) / self.inodes_per_group) as usize;
        let bit = (ino - 1) % self.inodes_per_group;
        let bitmap_block = self.groups[g].inode_bitmap;
        let mut bitmap = self.read_block(bitmap_block)?;
        if bitmap[(bit / 8) as usize] & (1 << (bit % 8)) == 0 {
            bitmap[(bit / 8) as usize] |= 1 << (bit % 8);
            self.stage(bitmap_block, bitmap);
            self.groups[g].free_inodes -= 1;
            if dir {
                self.groups[g].used_dirs += 1;
            }
        }
        Ok(())
    }

    fn alloc_inode(&mut self, goal: usize, dir: bool) -> Result<u32> {
        let count = self.groups.len();
        for step in 0..count {
            let g = (goal + step) % count;
            if self.groups[g].free_inodes == 0 {
                continue;
            }
            let bitmap = self.read_block(self.groups[g].inode_bitmap)?;
            for bit in 0..self.inodes_per_group {
                if bitmap[(bit / 8) as usize] & (1 << (bit % 8)) == 0 {
                    let ino = g as u32 * self.inodes_per_group + bit + 1;
                    if ino < FIRST_INO {
                        continue;
                    }
                    self.mark_inode_used(ino, dir)?;
                    return Ok(ino);
                }
            }
        }
        Err(Error::NoSpace)
    }

    fn free_inode(&mut self, ino: u32, dir: bool) -> Result<()> {
        let g = ((ino - 1) / self.inodes_per_group) as usize;
        let bit = (ino - 1) % self.inodes_per_group;
        let bitmap_block = self.groups[g].inode_bitmap;
        let mut bitmap = self.read_block(bitmap_block)?;
        if bitmap[(bit / 8) as usize] & (1 << (bit % 8)) != 0 {
            bitmap[(bit / 8) as usize] &= !(1 << (bit % 8));
            self.stage(bitmap_block, bitmap);
            self.groups[g].free_inodes += 1;
            if dir {
                self.groups[g].used_dirs = self.groups[g].used_dirs.saturating_sub(1);
            }
        }
        Ok(())
    }

    fn inode_location(&self, ino: u32) -> Result<(u32, usize)> {
        if ino == 0 || ino > self.inodes_count {
            return Err(Error::Corrupt);
        }
        let g = ((ino - 1) / self.inodes_per_group) as usize;
        let index = ((ino - 1) % self.inodes_per_group) as usize;
        let per_block = self.bs / INODE_SIZE;
        Ok((self.groups[g].inode_table + (index / per_block) as u32, (index % per_block) * INODE_SIZE))
    }

    pub fn read_inode(&mut self, ino: u32) -> Result<Inode> {
        let (block, offset) = self.inode_location(ino)?;
        let data = self.read_block(block)?;
        Ok(Inode::decode(&data[offset..offset + INODE_SIZE]))
    }

    fn write_inode(&mut self, ino: u32, inode: &Inode) -> Result<()> {
        let (block, offset) = self.inode_location(ino)?;
        let mut data = self.read_block(block)?;
        inode.encode(&mut data[offset..offset + INODE_SIZE]);
        self.stage(block, data);
        Ok(())
    }

    fn pointers(&mut self, block: u32) -> Result<Vec<u32>> {
        let data = self.read_block(block)?;
        Ok(data.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    fn collect_tree(&mut self, block: u32, depth: u32, data: &mut Vec<u32>, meta: &mut Vec<u32>, limit: usize) -> Result<()> {
        if block == 0 || data.len() >= limit {
            return Ok(());
        }
        if block >= self.blocks_count {
            return Err(Error::Corrupt);
        }
        if depth == 0 {
            data.push(block);
            return Ok(());
        }
        meta.push(block);
        for p in self.pointers(block)? {
            if p == 0 {
                break;
            }
            self.collect_tree(p, depth - 1, data, meta, limit)?;
        }
        Ok(())
    }

    fn block_list_split(&mut self, inode: &Inode) -> Result<(Vec<u32>, Vec<u32>)> {
        let limit = (inode.size.div_ceil(self.bs as u64)) as usize;
        let mut data = Vec::with_capacity(limit);
        let mut meta = Vec::new();
        for i in 0..12 {
            if data.len() >= limit || inode.block[i] == 0 {
                break;
            }
            data.push(inode.block[i]);
        }
        for (slot, depth) in [(12usize, 1u32), (13, 2), (14, 3)] {
            if data.len() >= limit {
                break;
            }
            self.collect_tree(inode.block[slot], depth, &mut data, &mut meta, limit)?;
        }
        data.truncate(limit);
        Ok((data, meta))
    }

    fn block_list(&mut self, inode: &Inode, with_meta: bool) -> Result<Vec<u32>> {
        let (mut data, meta) = self.block_list_split(inode)?;
        if with_meta {
            data.extend(meta);
        }
        Ok(data)
    }

    fn build_pointers(&mut self, inode: &mut Inode, blocks: &[u32], direct: bool) -> Result<usize> {
        inode.block = [0; 15];
        let per = self.bs / 4;
        let mut used_meta = 0usize;
        let mut rest = blocks;
        let head = rest.len().min(12);
        inode.block[..head].copy_from_slice(&rest[..head]);
        rest = &rest[head..];
        for (slot, depth) in [(12usize, 1u32), (13, 2), (14, 3)] {
            if rest.is_empty() {
                break;
            }
            let capacity = per.pow(depth);
            let take = rest.len().min(capacity);
            let (block, meta) = self.build_level(&rest[..take], depth, direct)?;
            inode.block[slot] = block;
            used_meta += meta;
            rest = &rest[take..];
        }
        if !rest.is_empty() {
            return Err(Error::TooLarge);
        }
        Ok(used_meta)
    }

    fn build_level(&mut self, blocks: &[u32], depth: u32, direct: bool) -> Result<(u32, usize)> {
        let per = self.bs / 4;
        let goal = self.group_of_block(blocks[0]).0;
        let table = self.alloc_block(goal)?;
        let mut data = vec![0u8; self.bs];
        let mut meta = 1usize;
        if depth == 1 {
            for (i, b) in blocks.iter().enumerate() {
                put32(&mut data, i * 4, *b);
            }
        } else {
            let span = per.pow(depth - 1);
            for (i, chunk) in blocks.chunks(span).enumerate() {
                let (child, child_meta) = self.build_level(chunk, depth - 1, direct)?;
                put32(&mut data, i * 4, child);
                meta += child_meta;
            }
        }
        if direct {
            self.write_raw(table, &data)?;
        } else {
            self.stage(table, data);
        }
        Ok((table, meta))
    }

    pub fn read_file(&mut self, ino: u32) -> Result<Vec<u8>> {
        let inode = self.read_inode(ino)?;
        if inode.is_dir() {
            return Err(Error::IsDir);
        }
        self.read_inode_data(&inode)
    }

    pub fn file_blocks(&mut self, ino: u32) -> Result<(Vec<u32>, u64)> {
        let inode = self.read_inode(ino)?;
        if inode.is_dir() {
            return Err(Error::IsDir);
        }
        Ok((self.block_list(&inode, false)?, inode.size))
    }

    pub fn read_range(&mut self, blocks: &[u32], size: u64, offset: u64, out: &mut [u8]) -> Result<usize> {
        if offset >= size {
            return Ok(0);
        }
        let len = (out.len() as u64).min(size - offset) as usize;
        let bs = self.bs as u64;
        let first = (offset / bs) as usize;
        let last = ((offset + len as u64).div_ceil(bs) as usize).min(blocks.len());
        if first >= last {
            return Ok(0);
        }
        let mut scratch = vec![0u8; (last - first) * self.bs];
        let mut i = first;
        while i < last {
            if let Some(cached) = self.cache.get(&blocks[i]) {
                let at = (i - first) * self.bs;
                scratch[at..at + self.bs].copy_from_slice(cached);
                i += 1;
                continue;
            }
            let mut run = 1usize;
            while i + run < last && blocks[i + run] == blocks[i] + run as u32 && run < 256 && !self.cache.contains_key(&(blocks[i] + run as u32)) {
                run += 1;
            }
            let at = (i - first) * self.bs;
            if blocks[i] != 0 {
                self.read_raw(blocks[i], &mut scratch[at..at + run * self.bs])?;
            }
            i += run;
        }
        let skip = (offset - first as u64 * bs) as usize;
        let n = len.min(scratch.len() - skip);
        out[..n].copy_from_slice(&scratch[skip..skip + n]);
        Ok(n)
    }

    fn read_inode_data(&mut self, inode: &Inode) -> Result<Vec<u8>> {
        let size = inode.size as usize;
        let blocks = self.block_list(inode, false)?;
        let mut out = vec![0u8; blocks.len() * self.bs];
        let mut i = 0usize;
        while i < blocks.len() {
            let mut run = 1usize;
            while i + run < blocks.len() && blocks[i + run] == blocks[i] + run as u32 && run < 256 && !self.cache.contains_key(&(blocks[i] + run as u32)) {
                run += 1;
            }
            if let Some(cached) = self.cache.get(&blocks[i]) {
                out[i * self.bs..(i + 1) * self.bs].copy_from_slice(cached);
                i += 1;
                continue;
            }
            let start = blocks[i];
            self.read_raw(start, &mut out[i * self.bs..(i + run) * self.bs])?;
            i += run;
        }
        out.truncate(size);
        Ok(out)
    }

    pub fn write_file(&mut self, ino: u32, content: &[u8]) -> Result<()> {
        let mut inode = self.read_inode(ino)?;
        if inode.is_dir() {
            return Err(Error::IsDir);
        }
        let old = self.block_list(&inode, true)?;
        let count = content.len().div_ceil(self.bs);
        let goal = ((ino - 1) / self.inodes_per_group) as usize;
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            blocks.push(self.alloc_block(goal)?);
        }
        let mut i = 0usize;
        while i < count {
            let mut run = 1usize;
            while i + run < count && blocks[i + run] == blocks[i] + run as u32 && run < 256 {
                run += 1;
            }
            let start = i * self.bs;
            let end = ((i + run) * self.bs).min(content.len());
            if end - start == run * self.bs {
                self.write_raw(blocks[i], &content[start..end])?;
            } else {
                let mut padded = vec![0u8; run * self.bs];
                padded[..end - start].copy_from_slice(&content[start..end]);
                self.write_raw(blocks[i], &padded)?;
            }
            i += run;
        }
        let meta = self.build_pointers(&mut inode, &blocks, true)?;
        inode.size = content.len() as u64;
        inode.blocks512 = ((count + meta) * self.bs / 512) as u32;
        inode.mtime = self.now;
        inode.ctime = self.now;
        self.write_inode(ino, &inode)?;
        for block in old {
            self.free_block(block)?;
        }
        Ok(())
    }

    pub fn set_attributes(&mut self, ino: u32, mode: u16, uid: u32, gid: u32) -> Result<()> {
        let mut inode = self.read_inode(ino)?;
        let new_mode = (inode.mode & S_IFMT) | (mode & 0o7777);
        if inode.mode == new_mode && inode.uid == uid && inode.gid == gid {
            return Ok(());
        }
        inode.mode = new_mode;
        inode.uid = uid;
        inode.gid = gid;
        inode.ctime = self.now;
        self.write_inode(ino, &inode)
    }

    fn init_directory(&mut self, ino: u32, parent: u32) -> Result<()> {
        let mut inode = self.read_inode(ino)?;
        let block = self.alloc_block(((ino - 1) / self.inodes_per_group) as usize)?;
        let mut data = vec![0u8; self.bs];
        put32(&mut data, 0, ino);
        put16(&mut data, 4, 12);
        data[6] = 1;
        data[7] = 2;
        data[8] = b'.';
        put32(&mut data, 12, parent);
        put16(&mut data, 16, (self.bs - 12) as u16);
        data[18] = 2;
        data[19] = 2;
        data[20] = b'.';
        data[21] = b'.';
        self.stage(block, data);
        inode.block = [0; 15];
        inode.block[0] = block;
        inode.size = self.bs as u64;
        inode.blocks512 = (self.bs / 512) as u32;
        self.write_inode(ino, &inode)
    }

    pub fn read_dir(&mut self, ino: u32) -> Result<Vec<DirEntry>> {
        let inode = self.read_inode(ino)?;
        if !inode.is_dir() {
            return Err(Error::NotDir);
        }
        let blocks = self.block_list(&inode, false)?;
        let mut out = Vec::new();
        for block in blocks {
            let data = self.read_block(block)?;
            let mut offset = 0usize;
            while offset + 8 <= self.bs {
                let child = get32(&data, offset);
                let rec_len = get16(&data, offset + 4) as usize;
                let name_len = data[offset + 6] as usize;
                if rec_len < 8 || offset + rec_len > self.bs {
                    return Err(Error::Corrupt);
                }
                if child != 0 && offset + 8 + name_len <= self.bs {
                    let name = String::from_utf8_lossy(&data[offset + 8..offset + 8 + name_len]).into_owned();
                    if name != "." && name != ".." {
                        out.push(DirEntry { name, ino: child, file_type: data[offset + 7] });
                    }
                }
                offset += rec_len;
            }
        }
        Ok(out)
    }

    pub fn lookup(&mut self, dir: u32, name: &str) -> Result<Option<u32>> {
        Ok(self.read_dir(dir)?.into_iter().find(|e| e.name == name).map(|e| e.ino))
    }

    pub fn resolve(&mut self, path: &str) -> Result<u32> {
        let mut ino = ROOT_INO;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            ino = self.lookup(ino, part)?.ok_or(Error::NotFound)?;
        }
        Ok(ino)
    }

    fn add_entry(&mut self, dir: u32, name: &str, child: u32, file_type: u8) -> Result<()> {
        let needed = align4(8 + name.len());
        let mut inode = self.read_inode(dir)?;
        let blocks = self.block_list(&inode, false)?;
        for block in &blocks {
            let mut data = self.read_block(*block)?;
            let mut offset = 0usize;
            while offset + 8 <= self.bs {
                let entry_ino = get32(&data, offset);
                let rec_len = get16(&data, offset + 4) as usize;
                if rec_len < 8 {
                    return Err(Error::Corrupt);
                }
                let used = if entry_ino == 0 { 0 } else { align4(8 + data[offset + 6] as usize) };
                if rec_len >= used + needed {
                    let (at, len) = if entry_ino == 0 {
                        (offset, rec_len)
                    } else {
                        put16(&mut data, offset + 4, used as u16);
                        (offset + used, rec_len - used)
                    };
                    put32(&mut data, at, child);
                    put16(&mut data, at + 4, len as u16);
                    data[at + 6] = name.len() as u8;
                    data[at + 7] = file_type;
                    data[at + 8..at + 8 + name.len()].copy_from_slice(name.as_bytes());
                    self.stage(*block, data);
                    return Ok(());
                }
                offset += rec_len;
            }
        }
        let goal = ((dir - 1) / self.inodes_per_group) as usize;
        let block = self.alloc_block(goal)?;
        let mut data = vec![0u8; self.bs];
        put32(&mut data, 0, child);
        put16(&mut data, 4, self.bs as u16);
        data[6] = name.len() as u8;
        data[7] = file_type;
        data[8..8 + name.len()].copy_from_slice(name.as_bytes());
        self.stage(block, data);
        let (mut all, meta) = self.block_list_split(&inode)?;
        all.push(block);
        for m in meta {
            self.free_block(m)?;
        }
        let meta_count = self.build_pointers(&mut inode, &all, false)?;
        inode.size = all.len() as u64 * self.bs as u64;
        inode.blocks512 = ((all.len() + meta_count) * self.bs / 512) as u32;
        inode.mtime = self.now;
        self.write_inode(dir, &inode)
    }

    fn remove_entry(&mut self, dir: u32, name: &str) -> Result<()> {
        let inode = self.read_inode(dir)?;
        let blocks = self.block_list(&inode, false)?;
        for block in blocks {
            let mut data = self.read_block(block)?;
            let mut offset = 0usize;
            let mut previous: Option<usize> = None;
            while offset + 8 <= self.bs {
                let entry_ino = get32(&data, offset);
                let rec_len = get16(&data, offset + 4) as usize;
                if rec_len < 8 {
                    return Err(Error::Corrupt);
                }
                let name_len = data[offset + 6] as usize;
                if entry_ino != 0 && &data[offset + 8..offset + 8 + name_len] == name.as_bytes() {
                    match previous {
                        Some(prev) => {
                            let merged = get16(&data, prev + 4) as usize + rec_len;
                            put16(&mut data, prev + 4, merged as u16);
                        }
                        None => put32(&mut data, offset, 0),
                    }
                    self.stage(block, data);
                    return Ok(());
                }
                previous = Some(offset);
                offset += rec_len;
            }
        }
        Err(Error::NotFound)
    }

    pub fn create(&mut self, parent: u32, name: &str, mode: u16, uid: u32, gid: u32) -> Result<u32> {
        if name.is_empty() || name.len() > 255 || name.contains('/') || name == "." || name == ".." {
            return Err(Error::InvalidName);
        }
        let mut parent_inode = self.read_inode(parent)?;
        if !parent_inode.is_dir() {
            return Err(Error::NotDir);
        }
        if self.lookup(parent, name)?.is_some() {
            return Err(Error::Exists);
        }
        let dir = mode & S_IFMT == S_IFDIR;
        let goal = ((parent - 1) / self.inodes_per_group) as usize;
        let ino = self.alloc_inode(goal, dir)?;
        let inode = Inode {
            mode,
            uid,
            gid,
            links: if dir { 2 } else { 1 },
            atime: self.now,
            ctime: self.now,
            mtime: self.now,
            ..Inode::empty()
        };
        self.write_inode(ino, &inode)?;
        if dir {
            self.init_directory(ino, parent)?;
            parent_inode = self.read_inode(parent)?;
            parent_inode.links += 1;
            self.write_inode(parent, &parent_inode)?;
        }
        self.add_entry(parent, name, ino, file_type_of(mode))?;
        Ok(ino)
    }

    pub fn unlink(&mut self, parent: u32, name: &str) -> Result<()> {
        let ino = self.lookup(parent, name)?.ok_or(Error::NotFound)?;
        if ino == LOST_FOUND_INO && parent == ROOT_INO {
            return Err(Error::InvalidName);
        }
        let inode = self.read_inode(ino)?;
        if inode.is_dir() {
            if !self.read_dir(ino)?.is_empty() {
                return Err(Error::NotEmpty);
            }
            let mut parent_inode = self.read_inode(parent)?;
            parent_inode.links = parent_inode.links.saturating_sub(1);
            self.write_inode(parent, &parent_inode)?;
        }
        self.remove_entry(parent, name)?;
        let blocks = self.block_list(&inode, true)?;
        for block in blocks {
            self.free_block(block)?;
        }
        let mut dead = Inode::empty();
        dead.dtime = self.now;
        self.write_inode(ino, &dead)?;
        self.free_inode(ino, inode.is_dir())
    }

    pub fn remove_tree(&mut self, parent: u32, name: &str) -> Result<()> {
        let ino = self.lookup(parent, name)?.ok_or(Error::NotFound)?;
        if self.read_inode(ino)?.is_dir() {
            for child in self.read_dir(ino)? {
                self.remove_tree(ino, &child.name)?;
            }
        }
        self.unlink(parent, name)
    }

    pub fn stats(&self) -> Stats {
        Stats {
            block_size: self.bs as u32,
            blocks: self.blocks_count,
            free_blocks: self.groups.iter().map(|g| g.free_blocks as u32).sum(),
            inodes: self.inodes_count,
            free_inodes: self.groups.iter().map(|g| g.free_inodes as u32).sum(),
            journal_blocks: self.journal.len() as u32,
            sequence: self.next_seq,
            replayed: self.replayed,
            generation: get64(&self.sb, HEXT_OFFSET + 20),
        }
    }

    pub fn label(&self) -> String {
        let raw = &self.sb[120..136];
        let end = raw.iter().position(|b| *b == 0).unwrap_or(16);
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }
}
