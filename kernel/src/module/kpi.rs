use alloc::alloc::{alloc, alloc_zeroed, dealloc, Layout};
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::arch::io;
use crate::drivers::pci::{self, PciAddress};
use crate::memory::frame;

pub const KPI_VERSION: u32 = 6;

pub const CLASS_NETWORK: u32 = 1;
pub const CLASS_DISPLAY: u32 = 2;

pub use crate::drivers::video::gpu::{DisplayOps, FbDesc};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceId {
    pub vendor: u16,
    pub device: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PciHandle {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub _pad: u8,
    pub vendor: u16,
    pub device_id: u16,
    pub irq: u8,
    pub _pad2: [u8; 3],
}

impl PciHandle {
    fn address(&self) -> PciAddress {
        PciAddress { bus: self.bus, device: self.device, function: self.function }
    }
}

pub struct Claim {
    pub module: String,
    pub class: u32,
    pub name: String,
    pub handle: PciHandle,
    pub poll: Option<extern "C" fn(*mut core::ffi::c_void)>,
    pub context: u64,
}

pub static CLAIMS: Mutex<Vec<Claim>> = Mutex::new(Vec::new());
static ALLOCATED: AtomicU32 = AtomicU32::new(0);
static LOADING: Mutex<Option<String>> = Mutex::new(None);
static PER_MODULE: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());
static WINDOWS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static LAST_LINE: Mutex<Option<(String, String)>> = Mutex::new(None);
static BARS: Mutex<Vec<(PciAddress, u8, pci::BarInfo)>> = Mutex::new(Vec::new());

pub fn loading_module() -> Option<String> {
    LOADING.lock().clone()
}

pub fn claimed(address: PciAddress) -> Option<String> {
    CLAIMS.lock().iter().find(|c| c.handle.address() == address).map(|c| c.module.clone())
}

pub fn loading() -> bool {
    LOADING.lock().is_some()
}

fn current_module() -> Option<String> {
    LOADING.lock().clone()
}

fn charge(bytes: usize) -> bool {
    let Some(name) = current_module() else {
        return true;
    };
    let limit = crate::module::limit_of(&name);
    let mut table = PER_MODULE.lock();
    match table.iter_mut().find(|(owner, _)| *owner == name) {
        Some((_, used)) => {
            if *used + bytes > limit {
                return false;
            }
            *used += bytes;
        }
        None => {
            if bytes > limit {
                return false;
            }
            table.push((name, bytes));
        }
    }
    true
}

fn refund(bytes: usize) {
    let Some(name) = current_module() else {
        return;
    };
    if let Some((_, used)) = PER_MODULE.lock().iter_mut().find(|(owner, _)| *owner == name) {
        *used = used.saturating_sub(bytes);
    }
}

pub fn allocated_by(name: &str) -> usize {
    PER_MODULE.lock().iter().find(|(owner, _)| owner == name).map(|(_, used)| *used).unwrap_or(0)
}

pub fn forget(name: &str) {
    PER_MODULE.lock().retain(|(owner, _)| owner != name);
}

pub fn device_windows() -> Vec<(u64, u64)> {
    WINDOWS.lock().clone()
}

pub fn scan_windows() {
    let mut windows = Vec::new();
    let mut bars = Vec::new();
    for dev in pci::devices() {
        for (index, info) in pci::bars(dev.address) {
            bars.push((dev.address, index, info));
            if !info.io && info.base != 0 && info.len != 0 {
                windows.push((info.base, info.len));
            }
        }
    }
    *BARS.lock() = bars;
    if let Some(fb) = *crate::memory::FRAMEBUFFER.lock() {
        windows.push((fb.addr, fb.byte_len()));
    }
    *WINDOWS.lock() = windows;
}

fn window_allows(phys: u64, len: usize) -> bool {
    let end = phys.saturating_add(len as u64);
    WINDOWS.lock().iter().any(|(base, size)| phys >= *base && end <= base + size)
}

pub fn begin(name: &str) {
    *LOADING.lock() = Some(String::from(name));
}

fn remember_line(text: &str) {
    if let Some(module) = current_module() {
        *LAST_LINE.lock() = Some((module, String::from(text)));
    }
}

pub fn last_line(module: &str) -> Option<String> {
    LAST_LINE.lock().as_ref().filter(|(owner, _)| owner == module).map(|(_, text)| text.clone())
}

pub fn end() {
    *LOADING.lock() = None;
}

pub fn allocated_bytes() -> usize {
    ALLOCATED.load(Ordering::Relaxed) as usize
}

pub fn drop_claims(module: &str) {
    CLAIMS.lock().retain(|c| c.module != module);
}

pub fn poll_all() {
    let entries: Vec<(String, Option<extern "C" fn(*mut core::ffi::c_void)>, u64)> =
        CLAIMS.lock().iter().map(|c| (c.module.clone(), c.poll, c.context)).collect();
    for (module, poll, context) in entries {
        let Some(poll) = poll else {
            continue;
        };
        let start = crate::task::uptime_ms();
        poll(context as *mut core::ffi::c_void);
        let spent = crate::task::uptime_ms().saturating_sub(start);
        if spent >= crate::drivers::irq::STALL_MS {
            crate::module::mark_stalled(&module, spent);
        }
    }
}

fn header_layout(size: usize) -> Option<(Layout, usize)> {
    let total = size.checked_add(16)?;
    Layout::from_size_align(total, 16).ok().map(|l| (l, total))
}

unsafe fn tag(ptr: *mut u8, total: usize) -> *mut u8 {
    unsafe {
        core::ptr::write_unaligned(ptr as *mut usize, total);
        ptr.add(16)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_kmalloc(size: usize) -> *mut u8 {
    let Some((layout, total)) = header_layout(size) else {
        return core::ptr::null_mut();
    };
    if !charge(total) {
        return core::ptr::null_mut();
    }
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        return ptr;
    }
    ALLOCATED.fetch_add(total as u32, Ordering::Relaxed);
    unsafe { tag(ptr, total) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_kzalloc(size: usize) -> *mut u8 {
    let Some((layout, total)) = header_layout(size) else {
        return core::ptr::null_mut();
    };
    if !charge(total) {
        return core::ptr::null_mut();
    }
    let ptr = unsafe { alloc_zeroed(layout) };
    if ptr.is_null() {
        refund(total);
        return ptr;
    }
    ALLOCATED.fetch_add(total as u32, Ordering::Relaxed);
    unsafe { tag(ptr, total) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_kfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let base = ptr.sub(16);
        let total = core::ptr::read_unaligned(base as *const usize);
        if let Ok(layout) = Layout::from_size_align(total, 16) {
            dealloc(base, layout);
            ALLOCATED.fetch_sub(total as u32, Ordering::Relaxed);
            refund(total);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_dma_alloc(len: usize, phys_out: *mut u64) -> *mut u8 {
    let pages = len.div_ceil(frame::PAGE_SIZE).max(1);
    if (pages * frame::PAGE_SIZE) as isize > crate::module::budget_left() {
        return core::ptr::null_mut();
    }
    if !charge(pages * frame::PAGE_SIZE) {
        return core::ptr::null_mut();
    }
    let Some(base) = frame::alloc_contiguous(pages, 1usize << 32) else {
        return core::ptr::null_mut();
    };
    unsafe {
        core::ptr::write_bytes(base as *mut u8, 0, pages * frame::PAGE_SIZE);
        if !phys_out.is_null() {
            core::ptr::write_unaligned(phys_out, base as u64);
        }
    }
    ALLOCATED.fetch_add((pages * frame::PAGE_SIZE) as u32, Ordering::Relaxed);
    base as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_dma_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    let pages = len.div_ceil(frame::PAGE_SIZE).max(1);
    for page in 0..pages {
        frame::free_frame(ptr as usize + page * frame::PAGE_SIZE);
    }
    ALLOCATED.fetch_sub((pages * frame::PAGE_SIZE) as u32, Ordering::Relaxed);
    refund(pages * frame::PAGE_SIZE);
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_virt_to_phys(addr: *const u8) -> u64 {
    crate::memory::vmalloc::translate(addr as usize).unwrap_or(0) as u64
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_phys_to_virt(addr: u64) -> *mut u8 {
    addr as *mut u8
}

pub const IDENTITY_LIMIT: u64 = 1 << 32;

#[unsafe(no_mangle)]
pub extern "C" fn hamix_ioremap(phys: u64, len: usize) -> *mut u8 {
    if phys == 0 || len == 0 {
        return core::ptr::null_mut();
    }
    if !window_allows(phys, len) {
        if let Some(name) = current_module() {
            crate::drivers::klog::log(&alloc::format!("module {}: refused ioremap of {:#x}+{:#x}, not a device window", name, phys, len));
        }
        return core::ptr::null_mut();
    }
    if phys + len as u64 > IDENTITY_LIMIT && !crate::arch::paging::map_kernel_mmio(phys, len as u64) {
        if let Some(name) = current_module() {
            crate::drivers::klog::log(&alloc::format!("module {}: cannot map the device window at {:#x}+{:#x}", name, phys, len));
        }
        return core::ptr::null_mut();
    }
    phys as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_iounmap(_addr: *mut u8, _len: usize) {}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_readb(addr: *const u8) -> u8 {
    unsafe { core::ptr::read_volatile(addr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_readw(addr: *const u16) -> u16 {
    unsafe { core::ptr::read_volatile(addr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_readl(addr: *const u32) -> u32 {
    unsafe { core::ptr::read_volatile(addr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_readq(addr: *const u64) -> u64 {
    unsafe { core::ptr::read_volatile(addr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_writeb(value: u8, addr: *mut u8) {
    unsafe { core::ptr::write_volatile(addr, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_writew(value: u16, addr: *mut u16) {
    unsafe { core::ptr::write_volatile(addr, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_writel(value: u32, addr: *mut u32) {
    unsafe { core::ptr::write_volatile(addr, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_writeq(value: u64, addr: *mut u64) {
    unsafe { core::ptr::write_volatile(addr, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_inb(port: u16) -> u8 {
    io::inb(port)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_inw(port: u16) -> u16 {
    io::inw(port)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_inl(port: u16) -> u32 {
    io::inl(port)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_outb(value: u8, port: u16) {
    io::outb(port, value)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_outw(value: u16, port: u16) {
    io::outw(port, value)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_outl(value: u32, port: u16) {
    io::outl(port, value)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_find(vendor: u16, device: u16, out: *mut PciHandle) -> i32 {
    for dev in pci::devices() {
        if dev.vendor == vendor && dev.device == device {
            unsafe {
                core::ptr::write_unaligned(
                    out,
                    PciHandle {
                        bus: dev.address.bus,
                        device: dev.address.device,
                        function: dev.address.function,
                        _pad: 0,
                        vendor: dev.vendor,
                        device_id: dev.device,
                        irq: dev.interrupt_line,
                        _pad2: [0; 3],
                    },
                )
            };
            return 0;
        }
    }
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_read32(handle: *const PciHandle, offset: u8) -> u32 {
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    pci::read_config_u32(handle.address(), offset)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_write32(handle: *const PciHandle, offset: u8, value: u32) {
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    pci::write_config_u32(handle.address(), offset, value)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_read16(handle: *const PciHandle, offset: u8) -> u16 {
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    pci::read_config_u16(handle.address(), offset)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_write16(handle: *const PciHandle, offset: u8, value: u16) {
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    pci::write_config_u16(handle.address(), offset, value)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_bar(handle: *const PciHandle, index: u8, len_out: *mut u64) -> u64 {
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    let address = handle.address();
    let cached = BARS.lock().iter().find(|(a, i, _)| *a == address && *i == index).map(|(_, _, info)| *info);
    let info = cached.or_else(|| pci::bar_info(address, index));
    if !len_out.is_null() {
        unsafe { core::ptr::write_unaligned(len_out, info.map(|i| i.len).unwrap_or(0)) };
    }
    info.map(|i| i.base).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_enable(handle: *const PciHandle) {
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    pci::enable_bus_master(handle.address());
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_udelay(micros: u64) {
    crate::arch::delay_us(micros)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_mdelay(millis: u64) {
    crate::arch::delay_ms(millis)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_uptime_ms() -> u64 {
    crate::task::uptime_ms()
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_printk(text: *const u8, len: usize) {
    if text.is_null() {
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(text, len.min(1024)) };
    if let Ok(text) = core::str::from_utf8(bytes) {
        remember_line(text);
        crate::drivers::klog::log(text);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_kpi_version() -> u32 {
    KPI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_claim_device(
    class: u32,
    name: *const u8,
    name_len: usize,
    handle: *const PciHandle,
    poll: Option<extern "C" fn(*mut core::ffi::c_void)>,
    context: *mut core::ffi::c_void,
) -> i32 {
    let Some(module) = LOADING.lock().clone() else {
        return -1;
    };
    if handle.is_null() || name.is_null() {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(name, name_len.min(64)) };
    let Ok(name) = core::str::from_utf8(bytes) else {
        return -1;
    };
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    CLAIMS.lock().push(Claim {
        module,
        class,
        name: String::from(name),
        handle,
        poll,
        context: context as u64,
    });
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_register_display(name: *const u8, name_len: usize, ops: *const DisplayOps, fb: *const FbDesc) -> i32 {
    if LOADING.lock().is_none() {
        return -1;
    }
    if name.is_null() || ops.is_null() || fb.is_null() {
        return -22;
    }
    let bytes = unsafe { core::slice::from_raw_parts(name, name_len.min(64)) };
    let Ok(name) = core::str::from_utf8(bytes) else {
        return -22;
    };
    let abi = unsafe { core::ptr::read_unaligned(ops as *const u32) };
    let ops: DisplayOps = match abi {
        crate::drivers::video::gpu::ABI => unsafe { core::ptr::read_unaligned(ops) },
        crate::drivers::video::gpu::ABI_V2 => unsafe { core::ptr::read_unaligned(ops as *const crate::drivers::video::gpu::DisplayOpsV2) }.into(),
        _ => return -22,
    };
    let fb = unsafe { core::ptr::read_unaligned(fb) };
    match crate::drivers::video::gpu::register(name, ops, fb) {
        Ok(()) => 0,
        Err(e) => {
            crate::drivers::klog::log(e);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_display_hotplug() {
    crate::drivers::video::gpu::hotplug();
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_edid_modes(edid: *const u8, len: usize, out: *mut u32, max: u32) -> i32 {
    if edid.is_null() || out.is_null() || len < 128 {
        return -22;
    }
    let raw = unsafe { core::slice::from_raw_parts(edid, len.min(4096)) };
    let Some(parsed) = crate::drivers::video::edid::parse(raw) else {
        return -22;
    };
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 3) };
    let mut count = 0usize;
    for mode in parsed.modes.iter().take(max as usize) {
        target[count * 3] = mode.width;
        target[count * 3 + 1] = mode.height;
        target[count * 3 + 2] = mode.refresh;
        count += 1;
    }
    count as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_display_changed(fb: *const FbDesc) -> i32 {
    if fb.is_null() {
        return -22;
    }
    let fb = unsafe { core::ptr::read_unaligned(fb) };
    if fb.addr == 0 || fb.width == 0 || fb.height == 0 || fb.bpp != 32 || fb.pitch < fb.width * 4 {
        return -22;
    }
    crate::drivers::video::gpu::adopt(crate::memory::FramebufferInfo {
        addr: fb.addr,
        pitch: fb.pitch,
        width: fb.width,
        height: fb.height,
        bpp: 32,
    });
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_display_current(out: *mut FbDesc) -> i32 {
    if out.is_null() {
        return -22;
    }
    let Some(fb) = *crate::memory::FRAMEBUFFER.lock() else {
        return -19;
    };
    if fb.bpp != 32 {
        return -95;
    }
    unsafe { core::ptr::write_unaligned(out, FbDesc { addr: fb.addr, pitch: fb.pitch, width: fb.width, height: fb.height, bpp: fb.bpp as u32 }) };
    0
}

fn text_arg(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len.min(96)) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_display_describe(model: *const u8, model_len: usize, codename: *const u8, codename_len: usize, generation: *const u8, generation_len: usize) -> i32 {
    let Some(model) = text_arg(model, model_len) else {
        return -22;
    };
    crate::drivers::video::gpu::set_identity(model, text_arg(codename, codename_len).unwrap_or_default(), text_arg(generation, generation_len).unwrap_or_default());
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_budget_left() -> i64 {
    crate::module::budget_left() as i64
}

fn msi_capability(address: PciAddress) -> Option<(u8, u8)> {
    if !cfg!(target_arch = "x86_64") {
        return None;
    }
    if pci::read_config_u16(address, 0x06) & (1 << 4) == 0 {
        return None;
    }
    let mut offset = (pci::read_config_u32(address, 0x34) & 0xFC) as u8;
    let mut hops = 0;
    while offset != 0 && hops < 48 {
        let header = pci::read_config_u32(address, offset);
        let id = (header & 0xFF) as u8;
        if id == 0x05 || id == 0x11 {
            return Some((id, offset));
        }
        offset = ((header >> 8) & 0xFC) as u8;
        hops += 1;
    }
    None
}

fn enable_msi(address: PciAddress, offset: u8, vector: u8) -> bool {
    let control = pci::read_config_u16(address, offset + 2);
    let wide = control & (1 << 7) != 0;
    #[cfg(target_arch = "x86_64")]
    let target = 0xFEE0_0000u32 | ((crate::arch::x86_64::lapic::id() as u32) << 12);
    #[cfg(not(target_arch = "x86_64"))]
    let target = 0u32;
    pci::write_config_u32(address, offset + 4, target);
    if wide {
        pci::write_config_u32(address, offset + 8, 0);
        pci::write_config_u32(address, offset + 12, vector as u32);
    } else {
        pci::write_config_u32(address, offset + 8, vector as u32);
    }
    pci::write_config_u16(address, offset + 2, (control & !(0x7 << 4)) | 1);
    let command = pci::read_config_u16(address, 0x04);
    pci::write_config_u16(address, 0x04, (command | (1 << 10)) & !0u16);
    pci::read_config_u16(address, offset + 2) & 1 != 0
}

fn disable_intx(address: PciAddress, disable: bool) {
    let command = pci::read_config_u16(address, 0x04);
    let updated = if disable { command | (1 << 10) } else { command & !(1 << 10) };
    pci::write_config_u16(address, 0x04, updated);
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_request_irq(handle: *const PciHandle, handler: Option<crate::drivers::irq::Handler>, context: *mut c_void) -> i32 {
    let Some(name) = current_module() else {
        return -1;
    };
    let (Some(handler), false) = (handler, handle.is_null()) else {
        return -22;
    };
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    let address = handle.address();
    if let Some((0x05, offset)) = msi_capability(address) {
        if let Some(vector) = crate::drivers::irq::request_message(&name, handler, context as u64) {
            if enable_msi(address, offset, vector) {
                disable_intx(address, true);
                crate::drivers::klog::log(&alloc::format!("module {}: MSI vector {:#x} for {:04x}:{:04x}", name, vector, handle.vendor, handle.device_id));
                return vector as i32;
            }
            crate::drivers::irq::release_vector(&name, vector);
        }
    }
    if handle.irq == 0 || (handle.irq > 15 && cfg!(target_arch = "x86_64")) || handle.irq == 0xFF {
        return -19;
    }
    disable_intx(address, false);
    match crate::drivers::irq::request_legacy(&name, handle.irq, handler, context as u64) {
        Some(vector) => {
            crate::drivers::klog::log(&alloc::format!("module {}: IRQ line {} (vector {:#x}) for {:04x}:{:04x}", name, handle.irq, vector, handle.vendor, handle.device_id));
            vector as i32
        }
        None => -16,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_free_irq() {
    if let Some(name) = current_module() {
        crate::drivers::irq::release(&name);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_msi_enable(handle: *const PciHandle) -> i32 {
    if handle.is_null() {
        return -22;
    }
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    match msi_capability(handle.address()) {
        Some((0x11, _)) => 2,
        Some(_) => 1,
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_schedule_work(work: Option<crate::drivers::irq::Work>, context: *mut c_void) -> i32 {
    let Some(work) = work else {
        return -22;
    };
    let name = current_module().unwrap_or_default();
    if crate::drivers::irq::schedule(&name, work, context as u64) { 0 } else { -11 }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_in_interrupt() -> i32 {
    crate::drivers::irq::in_handler() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_param_u32(name: *const u8, name_len: usize, fallback: u32) -> u32 {
    let Some(module) = current_module() else {
        return fallback;
    };
    if name.is_null() {
        return fallback;
    }
    let bytes = unsafe { core::slice::from_raw_parts(name, name_len.min(64)) };
    let Ok(key) = core::str::from_utf8(bytes) else {
        return fallback;
    };
    crate::module::param_of(&module, key).map(|v| v as u32).unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_dev_log(level: u32, text: *const u8, len: usize) {
    if text.is_null() {
        return;
    }
    let module = current_module().unwrap_or_default();
    if !module.is_empty() && level > crate::module::level_of(&module) {
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(text, len.min(1024)) };
    let Ok(text) = core::str::from_utf8(bytes) else {
        return;
    };
    if level <= crate::module::LEVEL_WARN {
        remember_line(text);
    }
    let tag = match level {
        crate::module::LEVEL_ERR => "error",
        crate::module::LEVEL_WARN => "warning",
        crate::module::LEVEL_DEBUG => "debug",
        _ => "info",
    };
    if module.is_empty() {
        crate::drivers::klog::log(&alloc::format!("{}: {}", tag, text));
    } else {
        crate::drivers::klog::log(&alloc::format!("{} {}: {}", module, tag, text));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_display_abi() -> u32 {
    crate::drivers::video::gpu::ABI
}
