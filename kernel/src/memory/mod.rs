pub mod frame;
mod heap;

use spin::Mutex;

#[allow(dead_code)]
const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

#[derive(Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
}

pub static FRAMEBUFFER: Mutex<Option<FramebufferInfo>> = Mutex::new(None);

#[derive(Clone, Copy)]
pub struct ModuleInfo {
    pub start: u32,
    pub end: u32,
}

pub static MODULES: Mutex<[Option<ModuleInfo>; 8]> = Mutex::new([None; 8]);

pub fn modules() -> [Option<ModuleInfo>; 8] {
    *MODULES.lock()
}

#[repr(C, packed)]
#[allow(dead_code)]
struct Mb2Header {
    total_size: u32,
    _reserved: u32,
}

#[repr(C, packed)]
#[allow(dead_code)]
struct Mb2Tag {
    typ: u32,
    size: u32,
}

#[repr(C, packed)]
#[allow(dead_code)]
struct Mb2MemMap {
    typ: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32,
}

#[repr(C, packed)]
#[allow(dead_code)]
struct Mb2MemEntry {
    base_addr: u64,
    length: u64,
    typ: u32,
    _reserved: u32,
}

#[repr(C, packed)]
#[allow(dead_code)]
struct Mb2Framebuffer {
    typ: u32,
    size: u32,
    addr: u64,
    pitch: u32,
    width: u32,
    height: u32,
    bpp: u8,
    fb_type: u8,
    _reserved: u16,
}

pub fn init(multiboot_info_ptr: usize) {
    crate::serial_println!("boot: memory start {:x}", multiboot_info_ptr);
    frame::init();

    unsafe {
        let header_ptr = multiboot_info_ptr as *const u32;
        let total_size = core::ptr::read_unaligned(header_ptr.add(0)) as usize;
        crate::serial_println!("boot: mb2 size {:x}", total_size);
        let mut offset = 8usize;

        while offset < total_size {
            let tag_ptr = (multiboot_info_ptr + offset) as *const u32;
            let typ = core::ptr::read_unaligned(tag_ptr.add(0)) as u32;
            let tag_size = core::ptr::read_unaligned(tag_ptr.add(1)) as usize;

            match typ {
                0 => break,
                6 => {
                    let entry_size = core::ptr::read_unaligned(tag_ptr.add(2)) as usize;
                    let mut e_off = 16usize;
                    while e_off + entry_size <= tag_size {
                        let entry_ptr = (multiboot_info_ptr + offset + e_off) as *const u64;
                        let base_addr = core::ptr::read_unaligned(entry_ptr.add(0)) as u64;
                        let length = core::ptr::read_unaligned(entry_ptr.add(1)) as u64;
                        let entry_type = core::ptr::read_unaligned((entry_ptr as *const u32).add(4)) as u32;
                        if entry_type == 1 {
                            frame::add_region(base_addr as usize, length as usize);
                        }
                        e_off += entry_size;
                    }
                }
                3 => {
                    let mod_start = core::ptr::read_unaligned(tag_ptr.add(2)) as u32;
                    let mod_end = core::ptr::read_unaligned(tag_ptr.add(3)) as u32;
                    let mut mods = MODULES.lock();
                    for slot in mods.iter_mut() {
                        if slot.is_none() {
                            *slot = Some(ModuleInfo { start: mod_start, end: mod_end });
                            break;
                        }
                    }
                }
                8 => {
                    let addr = core::ptr::read_unaligned((tag_ptr as *const u64).add(1));
                    let pitch = core::ptr::read_unaligned(tag_ptr.add(4));
                    let width = core::ptr::read_unaligned(tag_ptr.add(5));
                    let height = core::ptr::read_unaligned(tag_ptr.add(6));
                    let bpp = core::ptr::read_unaligned((tag_ptr as *const u8).add(28));
                    *FRAMEBUFFER.lock() = Some(FramebufferInfo { addr, pitch, width, height, bpp });
                }
                _ => {}
            }

            offset += (tag_size + 7) & !7;
        }
    }

    heap::init();
}
