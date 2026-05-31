mod frame;
mod heap;

pub use frame::memory_info;

const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

#[repr(C, packed)]
struct Mb2Header {
    total_size: u32,
    _reserved: u32,
}

#[repr(C, packed)]
struct Mb2Tag {
    typ: u32,
    size: u32,
}

#[repr(C, packed)]
struct Mb2MemMap {
    typ: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32,
}

#[repr(C, packed)]
struct Mb2MemEntry {
    base_addr: u64,
    length: u64,
    typ: u32,
    _reserved: u32,
}

#[repr(C, packed)]
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
    let mut frame_alloc = frame::BitmapFrameAllocator::new();

    unsafe {
        let header = &*(multiboot_info_ptr as *const Mb2Header);
        let total_size = header.total_size as usize;
        let mut offset = 8usize;

        while offset < total_size {
            let tag_ptr = (multiboot_info_ptr + offset) as *const Mb2Tag;
            let tag = &*tag_ptr;

            match tag.typ {
                0 => break,
                6 => {
                    let mmap = tag_ptr as *const Mb2MemMap;
                    let entry_size = (*mmap).entry_size as usize;
                    let tag_size = (*mmap).size as usize;
                    let mut e_off = 16usize;
                    while e_off + entry_size <= tag_size {
                        let entry_ptr = (tag_ptr as usize + e_off) as *const Mb2MemEntry;
                        let entry = &*entry_ptr;
                        if entry.typ == 1 {
                            frame_alloc.add_region(
                                entry.base_addr as usize,
                                entry.length as usize,
                            );
                        }
                        e_off += entry_size;
                    }
                }
                8 => {
                    let fb = tag_ptr as *const Mb2Framebuffer;
                    let fb = &*fb;
                    crate::drivers::video::vesa::setup_from_multiboot(
                        fb.addr,
                        fb.width as usize,
                        fb.height as usize,
                        fb.pitch as usize,
                        fb.bpp as usize,
                    );
                }
                _ => {}
            }

            let tag_size = tag.size as usize;
            offset += (tag_size + 7) & !7;
        }
    }

    frame::init(frame_alloc);
    heap::init();
}
