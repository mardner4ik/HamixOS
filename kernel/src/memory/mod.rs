pub mod aspace;
pub mod frame;
mod heap;
pub mod vmalloc;
#[cfg(target_arch = "x86_64")]
mod multiboot;
#[cfg(not(target_arch = "x86_64"))]
mod devicetree;

#[cfg(not(target_arch = "x86_64"))]
pub use devicetree::{init_devicetree, set_boot_initrd};
#[cfg(target_arch = "x86_64")]
pub use multiboot::init;

use spin::Mutex;

pub fn early_heap_init() {
    heap::init();
}

#[derive(Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
}

impl FramebufferInfo {
    pub fn byte_len(&self) -> u64 {
        self.pitch as u64 * self.height as u64
    }
}

pub static FRAMEBUFFER: Mutex<Option<FramebufferInfo>> = Mutex::new(None);

#[derive(Clone, Copy)]
pub struct ModuleInfo {
    pub start: u32,
    pub end: u32,
    name: [u8; 32],
    name_len: usize,
}

impl ModuleInfo {
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn matches(&self, key: &str) -> bool {
        self.name().contains(key)
    }
}

pub static RSDP: Mutex<Option<[u8; 36]>> = Mutex::new(None);

pub static MODULES: Mutex<[Option<ModuleInfo>; 8]> = Mutex::new([None; 8]);
static CMDLINE: Mutex<alloc::string::String> = Mutex::new(alloc::string::String::new());

pub fn cmdline() -> alloc::string::String {
    CMDLINE.lock().clone()
}

pub fn cmdline_value(key: &str) -> Option<alloc::string::String> {
    let line = cmdline();
    line.split_whitespace().find_map(|word| {
        let (k, v) = word.split_once('=')?;
        if k == key { Some(alloc::string::String::from(v)) } else { None }
    })
}

pub fn modules() -> [Option<ModuleInfo>; 8] {
    *MODULES.lock()
}

pub fn find_module(key: &str) -> Option<ModuleInfo> {
    modules().into_iter().flatten().find(|m| m.matches(key))
}

unsafe extern "C" {
    static __kernel_end: u8;
    #[cfg(not(target_arch = "x86_64"))]
    static __kernel_start: u8;
}

#[cfg(not(target_arch = "x86_64"))]
pub fn kernel_start() -> usize {
    (&raw const __kernel_start) as usize
}

pub fn kernel_end() -> usize {
    (&raw const __kernel_end) as usize
}

pub fn heap_stats() -> (usize, usize) {
    heap::stats()
}

pub fn heap_trim() -> usize {
    heap::trim()
}

pub fn release_module(key: &str) -> usize {
    let mut mods = MODULES.lock();
    let Some(slot) = mods.iter_mut().find(|m| m.map(|m| m.matches(key)).unwrap_or(false)) else {
        return 0;
    };
    let module = slot.take().unwrap();
    drop(mods);
    let start = (module.start as usize).div_ceil(frame::PAGE_SIZE) * frame::PAGE_SIZE;
    let end = (module.end as usize) / frame::PAGE_SIZE * frame::PAGE_SIZE;
    let mut freed = 0;
    let mut page = start;
    while page < end {
        frame::free_frame(page);
        page += frame::PAGE_SIZE;
        freed += frame::PAGE_SIZE;
    }
    freed
}
