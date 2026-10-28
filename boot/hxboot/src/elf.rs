use crate::bios;
use crate::iso9660::Volume;
use crate::layout::{KERNEL_MIN, SCRATCH, SCRATCH_SIZE};

pub struct Kernel {
    pub entry: u32,
    pub end: u32,
}

pub fn load(volume: &Volume, path: &str) -> Result<Kernel, &'static str> {
    let file = volume.lookup(path)?;
    let head = SCRATCH_SIZE.min(file.size);
    volume.read(&file, 0, SCRATCH as *mut u8, head as usize)?;
    let ehdr = bios::bytes(SCRATCH, head as usize);
    if head < 64 || &ehdr[0..4] != b"\x7fELF" || ehdr[4] != 2 || ehdr[5] != 1 || u16::from_le_bytes([ehdr[18], ehdr[19]]) != 0x3e {
        return Err("the kernel is not an x86_64 ELF file");
    }
    let entry = u64::from_le_bytes(ehdr[24..32].try_into().unwrap());
    let phoff = u64::from_le_bytes(ehdr[32..40].try_into().unwrap()) as usize;
    let phentsize = u16::from_le_bytes([ehdr[54], ehdr[55]]) as usize;
    let phnum = u16::from_le_bytes([ehdr[56], ehdr[57]]) as usize;
    if phentsize < 56 || phoff + phentsize * phnum > head as usize || entry >= 1 << 32 {
        return Err("the kernel has a broken ELF header");
    }
    let mut segments = [(0u32, 0u32, 0u32, 0u32); 16];
    let mut count = 0;
    for i in 0..phnum {
        let ph = &ehdr[phoff + i * phentsize..phoff + (i + 1) * phentsize];
        if u32::from_le_bytes(ph[0..4].try_into().unwrap()) != 1 {
            continue;
        }
        let offset = u64::from_le_bytes(ph[8..16].try_into().unwrap());
        let paddr = u64::from_le_bytes(ph[24..32].try_into().unwrap());
        let filesz = u64::from_le_bytes(ph[32..40].try_into().unwrap());
        let memsz = u64::from_le_bytes(ph[40..48].try_into().unwrap());
        if paddr < KERNEL_MIN as u64 || paddr + memsz >= 1 << 32 || filesz > memsz || offset + filesz > file.size as u64 || count == segments.len() {
            return Err("the kernel has a segment hxboot cannot load");
        }
        segments[count] = (offset as u32, paddr as u32, filesz as u32, memsz as u32);
        count += 1;
    }
    if count == 0 {
        return Err("the kernel has nothing to load");
    }
    let mut end = 0u32;
    println!("  {} ({} KiB)", path, file.size / 1024);
    for &(offset, paddr, filesz, memsz) in &segments[..count] {
        if filesz > 0 {
            volume.read(&file, offset, paddr as *mut u8, filesz as usize)?;
        }
        if memsz > filesz {
            unsafe { core::ptr::write_bytes((paddr + filesz) as *mut u8, 0, (memsz - filesz) as usize) };
        }
        end = end.max(paddr + memsz);
    }
    Ok(Kernel { entry: entry as u32, end })
}
