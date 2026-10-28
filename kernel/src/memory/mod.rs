pub mod aspace;
pub mod frame;
pub mod heap;
pub mod page_cache;
pub mod tlb;

pub static BOOT_INFO_SIZE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
pub static BOOT_INFO_ADDR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
pub mod vmalloc;
#[cfg(target_arch = "x86_64")]
mod multiboot;
#[cfg(not(target_arch = "x86_64"))]
mod devicetree;

#[cfg(not(target_arch = "x86_64"))]
pub use devicetree::{init_devicetree, set_boot_initrd};
#[cfg(target_arch = "x86_64")]
pub use multiboot::init;

use alloc::vec::Vec;
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

pub const TAG_VENTOY: u32 = 0x4858_5601;
static VENTOY: Mutex<Option<Vec<u8>>> = Mutex::new(None);

pub fn set_ventoy(data: Vec<u8>) {
    *VENTOY.lock() = Some(data);
}

pub fn ventoy() -> Option<Vec<u8>> {
    VENTOY.lock().clone()
}

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

pub fn reclaim() {
    crate::drivers::block::trim_read_cache(16);
    if let Some(mut guard) = crate::fs::VFS.try_lock() {
        if let Some(vfs) = guard.as_mut() {
            vfs.evict_files(0);
        }
    }
    heap::trim();
}

pub fn try_reserved<T>(len: usize) -> Result<Vec<T>, i64> {
    let mut out = Vec::new();
    if out.try_reserve_exact(len).is_err() {
        reclaim();
        out.try_reserve_exact(len).map_err(|_| crate::errno::ENOMEM)?;
    }
    Ok(out)
}

pub fn try_zeroed(len: usize) -> Result<Vec<u8>, i64> {
    let mut out = try_reserved(len)?;
    out.resize(len, 0);
    Ok(out)
}

pub fn try_copy(data: &[u8]) -> Result<Vec<u8>, i64> {
    let mut out = try_reserved(data.len())?;
    out.extend_from_slice(data);
    Ok(out)
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
