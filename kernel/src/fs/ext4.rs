use alloc::string::String;
use alloc::vec::Vec;

const EXT4_MAGIC: u16 = 0xEF53;
const EXT4_EXTENTS_MAGIC: u16 = 0xF30A;
pub const ROOT_INODE: u32 = 2;

pub trait BlockDevice {
    fn read_at(&self, byte_offset: u64, buf: &mut [u8]);
    fn len_bytes(&self) -> u64;
}

pub struct MemBlockDevice<'a> {
    pub data: &'a [u8],
}

impl<'a> BlockDevice for MemBlockDevice<'a> {
    fn read_at(&self, byte_offset: u64, buf: &mut [u8]) {
        let start = byte_offset as usize;
        if start >= self.data.len() {
            for b in buf.iter_mut() {
                *b = 0;
            }
            return;
        }
        let end = (start + buf.len()).min(self.data.len());
        let n = end - start;
        buf[..n].copy_from_slice(&self.data[start..end]);
        for b in buf[n..].iter_mut() {
            *b = 0;
        }
    }

    fn len_bytes(&self) -> u64 {
        self.data.len() as u64
    }
}

pub struct Superblock {
    pub inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    pub desc_size: u16,
}

impl Superblock {
    pub fn block_size(&self) -> u64 {
        1024u64 << self.log_block_size
    }
}

pub fn read_superblock(dev: &dyn BlockDevice) -> Result<Superblock, &'static str> {
    let mut buf = [0u8; 1024];
    dev.read_at(1024, &mut buf);

    let magic = u16::from_le_bytes([buf[56], buf[57]]);
    if magic != EXT4_MAGIC {
        return Err("not an ext4 filesystem (bad superblock magic)");
    }

    let feature_incompat = u32::from_le_bytes(buf[96..100].try_into().unwrap());
    let feature_64bit = feature_incompat & 0x80 != 0;
    let raw_desc_size = u16::from_le_bytes(buf[254..256].try_into().unwrap());
    let desc_size = if feature_64bit && raw_desc_size >= 64 {
        raw_desc_size
    } else {
        32
    };

    Ok(Superblock {
        inodes_count: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
        first_data_block: u32::from_le_bytes(buf[20..24].try_into().unwrap()),
        log_block_size: u32::from_le_bytes(buf[24..28].try_into().unwrap()),
        inodes_per_group: u32::from_le_bytes(buf[40..44].try_into().unwrap()),
        inode_size: u16::from_le_bytes(buf[88..90].try_into().unwrap()),
        desc_size,
    })
}

struct GroupDesc {
    inode_table: u64,
}

fn read_group_desc(dev: &dyn BlockDevice, sb: &Superblock, group: u32) -> GroupDesc {
    let bs = sb.block_size();
    let gdt_block = sb.first_data_block as u64 + 1;
    let offset = gdt_block * bs + group as u64 * sb.desc_size as u64;

    let mut buf = [0u8; 64];
    let len = (sb.desc_size as usize).min(64);
    dev.read_at(offset, &mut buf[..len]);

    let lo = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as u64;
    let hi = if sb.desc_size >= 64 {
        u32::from_le_bytes(buf[40..44].try_into().unwrap()) as u64
    } else {
        0
    };

    GroupDesc {
        inode_table: lo | (hi << 32),
    }
}

pub struct Inode {
    pub mode: u16,
    pub size: u64,
    pub block: [u8; 60],
}

pub fn read_inode(dev: &dyn BlockDevice, sb: &Superblock, ino: u32) -> Option<Inode> {
    if ino == 0 || ino > sb.inodes_count {
        return None;
    }
    let group = (ino - 1) / sb.inodes_per_group;
    let index = (ino - 1) % sb.inodes_per_group;
    let gd = read_group_desc(dev, sb, group);
    let bs = sb.block_size();
    let offset = gd.inode_table * bs + index as u64 * sb.inode_size as u64;

    let mut buf = [0u8; 160];
    let read_len = (sb.inode_size as usize).min(160);
    dev.read_at(offset, &mut buf[..read_len]);

    let mode = u16::from_le_bytes(buf[0..2].try_into().unwrap());
    let size_lo = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as u64;
    let size_hi = u32::from_le_bytes(buf[108..112].try_into().unwrap()) as u64;
    let mut block = [0u8; 60];
    block.copy_from_slice(&buf[40..100]);

    Some(Inode {
        mode,
        size: size_lo | (size_hi << 32),
        block,
    })
}

pub fn is_dir(inode: &Inode) -> bool {
    inode.mode & 0xF000 == 0x4000
}

pub fn read_file_data(dev: &dyn BlockDevice, sb: &Superblock, inode: &Inode) -> Vec<u8> {
    let bs = sb.block_size();
    let mut data = Vec::with_capacity(inode.size as usize);

    let magic = u16::from_le_bytes([inode.block[0], inode.block[1]]);
    if magic == EXT4_EXTENTS_MAGIC {
        let entries = u16::from_le_bytes([inode.block[2], inode.block[3]]);
        let depth = u16::from_le_bytes([inode.block[6], inode.block[7]]);

        if depth == 0 {
            for i in 0..entries as usize {
                let off = 12 + i * 12;
                if off + 12 > 60 {
                    break;
                }
                let ee_len = u16::from_le_bytes(inode.block[off + 4..off + 6].try_into().unwrap());
                let ee_start_hi = u16::from_le_bytes(inode.block[off + 6..off + 8].try_into().unwrap());
                let ee_start_lo = u32::from_le_bytes(inode.block[off + 8..off + 12].try_into().unwrap());
                let start_block = ((ee_start_hi as u64) << 32) | ee_start_lo as u64;
                let len = (ee_len & 0x7FFF) as u64;

                for b in 0..len {
                    let mut blk = alloc::vec![0u8; bs as usize];
                    dev.read_at((start_block + b) * bs, &mut blk);
                    data.extend_from_slice(&blk);
                }
            }
        }
    } else {
        for i in 0..12usize {
            let ptr = u32::from_le_bytes(inode.block[i * 4..i * 4 + 4].try_into().unwrap());
            if ptr == 0 {
                break;
            }
            let mut blk = alloc::vec![0u8; bs as usize];
            dev.read_at(ptr as u64 * bs, &mut blk);
            data.extend_from_slice(&blk);
        }
    }

    data.truncate(inode.size as usize);
    data
}

pub struct DirEntry {
    pub name: String,
    pub inode: u32,
    pub is_dir: bool,
}

pub fn list_dir(dev: &dyn BlockDevice, sb: &Superblock, dir_inode: &Inode) -> Vec<DirEntry> {
    let data = read_file_data(dev, sb, dir_inode);
    let mut out = Vec::new();
    let mut off = 0usize;

    while off + 8 <= data.len() {
        let ino = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
        let rec_len = u16::from_le_bytes(data[off + 4..off + 6].try_into().unwrap()) as usize;
        let name_len = data[off + 6] as usize;
        let file_type = data[off + 7];

        if rec_len < 8 {
            break;
        }
        if ino != 0 && name_len > 0 && off + 8 + name_len <= data.len() {
            let name = String::from_utf8_lossy(&data[off + 8..off + 8 + name_len]).into_owned();
            out.push(DirEntry {
                name,
                inode: ino,
                is_dir: file_type == 2,
            });
        }

        off += rec_len;
    }

    out
}

pub struct Ext4Fs<'a> {
    pub dev: MemBlockDevice<'a>,
    pub sb: Superblock,
}

impl<'a> Ext4Fs<'a> {
    pub fn mount(image: &'a [u8]) -> Result<Self, &'static str> {
        let dev = MemBlockDevice { data: image };
        let sb = read_superblock(&dev)?;
        Ok(Self { dev, sb })
    }

    pub fn root(&self) -> Inode {
        read_inode(&self.dev, &self.sb, ROOT_INODE).expect("ext4: missing root inode")
    }

    pub fn list_root(&self) -> Vec<DirEntry> {
        list_dir(&self.dev, &self.sb, &self.root())
    }

    pub fn read_file(&self, ino: u32) -> Option<Vec<u8>> {
        let inode = read_inode(&self.dev, &self.sb, ino)?;
        Some(read_file_data(&self.dev, &self.sb, &inode))
    }

    pub fn find(&self, path: &str) -> Option<(u32, Inode)> {
        let mut cur_ino = ROOT_INODE;
        let mut cur_inode = self.root();

        for part in path.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            let entries = list_dir(&self.dev, &self.sb, &cur_inode);
            let hit = entries.into_iter().find(|e| e.name == part)?;
            cur_ino = hit.inode;
            cur_inode = read_inode(&self.dev, &self.sb, cur_ino)?;
        }

        Some((cur_ino, cur_inode))
    }
}
