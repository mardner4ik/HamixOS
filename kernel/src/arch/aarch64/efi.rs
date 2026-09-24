use core::ffi::c_void;

const KERNEL_BASE: u64 = 0x4020_0000;
const BUFFER_TOO_SMALL: usize = (1 << 63) | 5;
const LOADER_CODE: u32 = 1;
const LOADER_DATA: u32 = 2;
const ALLOCATE_MAX_ADDRESS: u32 = 1;
const ALLOCATE_ADDRESS: u32 = 2;
const FILE_MODE_READ: u64 = 1;

type Guid = [u8; 16];

const LOADED_IMAGE: Guid = guid(0x5B1B_31A1, 0x9562, 0x11D2, [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B]);
const SIMPLE_FILE_SYSTEM: Guid = guid(0x964E_5B22, 0x6459, 0x11D2, [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B]);
const FILE_INFO: Guid = guid(0x0957_6E92, 0x6D3F, 0x11D2, [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B]);
const DEVICE_TREE: Guid = guid(0xB1B6_21D5, 0xF19C, 0x41A5, [0x83, 0x0B, 0xD9, 0x15, 0x2C, 0x69, 0xAA, 0xE0]);

const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> Guid {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
}

const fn wide<const N: usize>(text: &str) -> [u16; N] {
    let bytes = text.as_bytes();
    let mut out = [0u16; N];
    let mut i = 0;
    while i < bytes.len() && i < N - 1 {
        out[i] = bytes[i] as u16;
        i += 1;
    }
    out
}

static HELLO: [u16; 40] = wide("HamixOS EFI stub: loading the kernel\r\n");
static NO_MEMORY: [u16; 56] = wide("HamixOS: cannot claim memory at 0x40200000 for the kernel\r\n");
static NO_INITRD: [u16; 56] = wide("HamixOS: no hext.img next to the kernel, booting without\r\n");
static NO_DTB: [u16; 48] = wide("HamixOS: the firmware provides no device tree\r\n");
static NAMES: [[u16; 32]; 4] = [wide("\\hext.img"), wide("\\EFI\\hamix\\hext.img"), wide("\\EFI\\BOOT\\hext.img"), wide("\\boot\\hext.img")];

unsafe fn field<T: Copy>(base: *const u8, offset: usize) -> T {
    unsafe { core::ptr::read_unaligned(base.add(offset) as *const T) }
}

struct Firmware {
    boot: *const u8,
    system: *const u8,
}

impl Firmware {
    unsafe fn service<T: Copy>(&self, offset: usize) -> T {
        unsafe { field(self.boot, offset) }
    }

    unsafe fn print(&self, text: &[u16]) {
        unsafe {
            let out: *const u8 = field(self.system, 64);
            if out.is_null() {
                return;
            }
            let output: extern "efiapi" fn(*const u8, *const u16) -> usize = field(out, 8);
            output(out, text.as_ptr());
        }
    }

    unsafe fn handle_protocol(&self, handle: *mut c_void, guid: &Guid) -> *const u8 {
        unsafe {
            let call: extern "efiapi" fn(*mut c_void, *const Guid, *mut *const u8) -> usize = self.service(152);
            let mut out = core::ptr::null();
            if call(handle, guid, &mut out) != 0 {
                return core::ptr::null();
            }
            out
        }
    }

    unsafe fn allocate(&self, kind: u32, memory: u32, pages: usize, address: &mut u64) -> bool {
        unsafe {
            let call: extern "efiapi" fn(u32, u32, usize, *mut u64) -> usize = self.service(40);
            call(kind, memory, pages, address) == 0
        }
    }

    unsafe fn device_tree(&self) -> u64 {
        unsafe {
            let count: usize = field(self.system, 104);
            let table: *const u8 = field(self.system, 112);
            for i in 0..count {
                let entry = table.add(i * 24);
                let id: Guid = field(entry, 0);
                if id == DEVICE_TREE {
                    return field::<u64>(entry, 16);
                }
            }
            0
        }
    }

    unsafe fn load_initrd(&self, image: *mut c_void) -> Option<(u64, u64)> {
        unsafe {
            let loaded = self.handle_protocol(image, &LOADED_IMAGE);
            if loaded.is_null() {
                return None;
            }
            let device: *mut c_void = field(loaded, 24);
            let fs = self.handle_protocol(device, &SIMPLE_FILE_SYSTEM);
            if fs.is_null() {
                return None;
            }
            let open_volume: extern "efiapi" fn(*const u8, *mut *const u8) -> usize = field(fs, 8);
            let mut root = core::ptr::null();
            if open_volume(fs, &mut root) != 0 || root.is_null() {
                return None;
            }
            let open: extern "efiapi" fn(*const u8, *mut *const u8, *const u16, u64, u64) -> usize = field(root, 8);
            let mut file = core::ptr::null();
            let mut found = false;
            for name in NAMES.iter() {
                if open(root, &mut file, name.as_ptr(), FILE_MODE_READ, 0) == 0 && !file.is_null() {
                    found = true;
                    break;
                }
            }
            if !found {
                return None;
            }
            let get_info: extern "efiapi" fn(*const u8, *const Guid, *mut usize, *mut u8) -> usize = field(file, 64);
            let mut info = [0u8; 512];
            let mut info_len = info.len();
            if get_info(file, &FILE_INFO, &mut info_len, info.as_mut_ptr()) != 0 {
                return None;
            }
            let size: u64 = field(info.as_ptr(), 8);
            if size == 0 {
                return None;
            }
            let mut address = 0xFFFF_FFFFu64;
            if !self.allocate(ALLOCATE_MAX_ADDRESS, LOADER_DATA, size.div_ceil(4096) as usize, &mut address) {
                return None;
            }
            let read: extern "efiapi" fn(*const u8, *mut usize, *mut u8) -> usize = field(file, 32);
            let mut done = 0u64;
            while done < size {
                let mut chunk = (size - done).min(16 << 20) as usize;
                if read(file, &mut chunk, (address + done) as *mut u8) != 0 || chunk == 0 {
                    return None;
                }
                done += chunk as u64;
            }
            Some((address, address + size))
        }
    }

    unsafe fn exit(&self, image: *mut c_void) -> bool {
        unsafe {
            let get_map: extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> usize = self.service(56);
            let pool: extern "efiapi" fn(u32, usize, *mut *mut u8) -> usize = self.service(64);
            let exit: extern "efiapi" fn(*mut c_void, usize) -> usize = self.service(232);
            let mut size = 0usize;
            let (mut key, mut desc_size, mut version) = (0usize, 0usize, 0u32);
            if get_map(&mut size, core::ptr::null_mut(), &mut key, &mut desc_size, &mut version) != BUFFER_TOO_SMALL {
                return false;
            }
            let capacity = size + 8 * desc_size.max(48);
            let mut map = core::ptr::null_mut();
            if pool(LOADER_DATA, capacity, &mut map) != 0 {
                return false;
            }
            for _ in 0..4 {
                size = capacity;
                if get_map(&mut size, map, &mut key, &mut desc_size, &mut version) != 0 {
                    return false;
                }
                if exit(image, key) == 0 {
                    return true;
                }
            }
            false
        }
    }
}

fn clean_range(start: u64, end: u64) {
    let mut at = start & !63;
    while at < end {
        unsafe { core::arch::asm!("dc civac, {}", in(reg) at, options(nostack)) };
        at += 64;
    }
}

#[unsafe(no_mangle)]
pub extern "efiapi" fn hamix_efi_entry(image: *mut c_void, system: *const u8) -> usize {
    unsafe {
        let firmware = Firmware { boot: field(system, 96), system };
        firmware.print(&HELLO);
        let loaded = firmware.handle_protocol(image, &LOADED_IMAGE);
        if loaded.is_null() {
            return 1;
        }
        let image_base: u64 = field(loaded, 64);
        let image_size: u64 = field(loaded, 72);
        let dtb = firmware.device_tree();
        if dtb == 0 {
            firmware.print(&NO_DTB);
            return 1;
        }
        let mut target = KERNEL_BASE;
        let pages = image_size.div_ceil(4096) as usize;
        if image_base != KERNEL_BASE && !firmware.allocate(ALLOCATE_ADDRESS, LOADER_CODE, pages, &mut target) {
            firmware.print(&NO_MEMORY);
            return 1;
        }
        let initrd = firmware.load_initrd(image);
        if initrd.is_none() {
            firmware.print(&NO_INITRD);
        }
        let (initrd_start, initrd_end) = initrd.unwrap_or((0, 0));
        if !firmware.exit(image) {
            return 1;
        }
        core::arch::asm!("msr daifset, #0xf", options(nostack));
        if image_base != KERNEL_BASE {
            core::ptr::copy(image_base as *const u8, KERNEL_BASE as *mut u8, image_size as usize);
        }
        clean_range(KERNEL_BASE, KERNEL_BASE + pages as u64 * 4096);
        clean_range(initrd_start, initrd_end);
        let dtb_size = u32::from_be(field::<u32>(dtb as *const u8, 4)) as u64;
        clean_range(dtb, dtb + dtb_size);
        core::arch::asm!("dsb sy", "ic iallu", "dsb sy", "isb", options(nostack));
        let entry: extern "C" fn(u64, u64, u64, u64) -> ! = core::mem::transmute(KERNEL_BASE as usize);
        entry(dtb, initrd_start, initrd_end, super::EFI_BOOT_MAGIC)
    }
}
