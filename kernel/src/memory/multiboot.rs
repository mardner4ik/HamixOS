use super::{frame, kernel_end, modules, FramebufferInfo, ModuleInfo, CMDLINE, FRAMEBUFFER, MODULES, RSDP};

const MB2_FB_TYPE_RGB: u8 = 1;

const LOW_LIMIT: u64 = 1 << 32;
const HIGH_LIMIT: u64 = 1 << 36;

fn map_high_memory(end: u64) {
    const MASK: u64 = 0x000F_FFFF_FFFF_F000;
    let gigabytes = end.div_ceil(1 << 30).min(HIGH_LIMIT >> 30);
    unsafe {
        let p4 = (crate::arch::paging::read_cr3() & MASK) as *mut u64;
        let p3 = (*p4 & MASK) as *mut u64;
        for gb in 4..gigabytes {
            if *p3.add(gb as usize) & 1 != 0 {
                continue;
            }
            let Some(table) = frame::alloc_zeroed_frame() else {
                return;
            };
            let entries = table as *mut u64;
            for i in 0..512u64 {
                *entries.add(i as usize) = ((gb << 30) + (i << 21)) | 0x83;
            }
            *p3.add(gb as usize) = table as u64 | 0x3;
        }
    }
    crate::arch::paging::flush_tlb();
}

pub fn init(multiboot_info_ptr: usize) {
    frame::init();
    let mut high = [(0u64, 0u64); 32];
    let mut high_count = 0usize;

    unsafe {
        let header_ptr = multiboot_info_ptr as *const u32;
        let total_size = core::ptr::read_unaligned(header_ptr) as usize;
        let mut offset = 8usize;

        while offset < total_size {
            let tag_ptr = (multiboot_info_ptr + offset) as *const u32;
            let typ = core::ptr::read_unaligned(tag_ptr);
            let tag_size = core::ptr::read_unaligned(tag_ptr.add(1)) as usize;

            match typ {
                0 => break,
                1 => {
                    let mut text = alloc::string::String::new();
                    let str_ptr = (multiboot_info_ptr + offset + 8) as *const u8;
                    let mut i = 0usize;
                    while 8 + i < tag_size && i < 1024 {
                        let b = core::ptr::read_unaligned(str_ptr.add(i));
                        if b == 0 {
                            break;
                        }
                        text.push(b as char);
                        i += 1;
                    }
                    *CMDLINE.lock() = text;
                }
                6 => {
                    let entry_size = core::ptr::read_unaligned(tag_ptr.add(2)) as usize;
                    let mut e_off = 16usize;
                    while entry_size > 0 && e_off + entry_size <= tag_size {
                        let entry_ptr = (multiboot_info_ptr + offset + e_off) as *const u64;
                        let base_addr = core::ptr::read_unaligned(entry_ptr);
                        let length = core::ptr::read_unaligned(entry_ptr.add(1));
                        let entry_type = core::ptr::read_unaligned((entry_ptr as *const u32).add(4));
                        if entry_type == 1 && base_addr < LOW_LIMIT {
                            let end = (base_addr + length).min(LOW_LIMIT);
                            frame::add_region(base_addr as usize, (end - base_addr) as usize);
                        }
                        if entry_type == 1 && base_addr + length > LOW_LIMIT && high_count < high.len() {
                            let start = base_addr.max(LOW_LIMIT);
                            let end = (base_addr + length).min(HIGH_LIMIT);
                            if end > start {
                                high[high_count] = (start, end);
                                high_count += 1;
                            }
                        }
                        e_off += entry_size;
                    }
                }
                3 => {
                    let start = core::ptr::read_unaligned(tag_ptr.add(2));
                    let end = core::ptr::read_unaligned(tag_ptr.add(3));
                    let mut name = [0u8; 32];
                    let mut name_len = 0usize;
                    let str_ptr = (multiboot_info_ptr + offset + 16) as *const u8;
                    while name_len < 32 && 16 + name_len < tag_size {
                        let b = core::ptr::read_unaligned(str_ptr.add(name_len));
                        if b == 0 {
                            break;
                        }
                        name[name_len] = b;
                        name_len += 1;
                    }
                    let mut mods = MODULES.lock();
                    if let Some(slot) = mods.iter_mut().find(|s| s.is_none()) {
                        *slot = Some(ModuleInfo { start, end, name, name_len });
                    }
                }
                14 | 15 => {
                    let len = (tag_size - 8).min(36);
                    let mut rsdp = [0u8; 36];
                    core::ptr::copy_nonoverlapping((multiboot_info_ptr + offset + 8) as *const u8, rsdp.as_mut_ptr(), len);
                    let mut slot = RSDP.lock();
                    if slot.is_none() || typ == 15 {
                        *slot = Some(rsdp);
                    }
                }
                8 => {
                    let addr = core::ptr::read_unaligned((tag_ptr as *const u64).add(1));
                    let pitch = core::ptr::read_unaligned(tag_ptr.add(4));
                    let width = core::ptr::read_unaligned(tag_ptr.add(5));
                    let height = core::ptr::read_unaligned(tag_ptr.add(6));
                    let bpp = core::ptr::read_unaligned((tag_ptr as *const u8).add(28));
                    let fb_type = core::ptr::read_unaligned((tag_ptr as *const u8).add(29));
                    if fb_type == MB2_FB_TYPE_RGB {
                        *FRAMEBUFFER.lock() = Some(FramebufferInfo { addr, pitch, width, height, bpp });
                    }
                }
                _ => {}
            }

            offset += (tag_size + 7) & !7;
        }
    }

    frame::reserve_region(0, 0x10_0000);
    frame::reserve_region(0x10_0000, kernel_end() - 0x10_0000);
    for module in modules().into_iter().flatten() {
        frame::reserve_region(module.start as usize, module.len());
    }
    if let Some(fb) = *FRAMEBUFFER.lock() {
        if fb.addr < (1u64 << 32) {
            frame::reserve_region(fb.addr as usize, fb.byte_len() as usize);
        }
    }

    let top = high[..high_count].iter().map(|(_, end)| *end).max().unwrap_or(0);
    if top > LOW_LIMIT {
        map_high_memory(top);
        for (start, end) in high[..high_count].iter() {
            frame::add_region(*start as usize, (*end - *start) as usize);
        }
    }
}
