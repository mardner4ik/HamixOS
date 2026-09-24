use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::kpi::PciHandle;
use crate::drivers::pci;

pub const CLASS_BLOCK: u32 = 3;
pub const CLASS_INPUT: u32 = 4;
pub const CLASS_AUDIO: u32 = 5;
pub const CLASS_USB_HCD: u32 = 6;

pub const BUS_PCI: u32 = 1;
pub const BUS_PLATFORM: u32 = 2;
pub const BUS_VIRTUAL: u32 = 3;
pub const BUS_USB: u32 = 4;

pub const BLOCK_ABI: u32 = 1;
pub const AUDIO_ABI: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DeviceHandle {
    pub bus: u32,
    pub id: u32,
    pub pci: PciHandle,
    pub base: u64,
    pub size: u64,
    pub irq: u32,
    pub class: u32,
    pub vendor: u16,
    pub device: u16,
    pub _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockOps {
    pub abi: u32,
    pub sector_size: u32,
    pub sectors: u64,
    pub context: u64,
    pub read: Option<extern "C" fn(u64, u64, u32, *mut u8) -> i32>,
    pub write: Option<extern "C" fn(u64, u64, u32, *const u8) -> i32>,
    pub flush: Option<extern "C" fn(u64) -> i32>,
    pub model: [u8; 40],
}

unsafe impl Send for BlockOps {}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioOps {
    pub abi: u32,
    pub rate: u32,
    pub context: u64,
    pub ring: *mut i16,
    pub ring_frames: u32,
    pub _pad: u32,
    pub position: Option<extern "C" fn(u64) -> u32>,
    pub start: Option<extern "C" fn(u64)>,
    pub stop: Option<extern "C" fn(u64)>,
    pub poll: Option<extern "C" fn(u64)>,
    pub name: [u8; 40],
}

unsafe impl Send for AudioOps {}

#[derive(Clone)]
pub struct Published {
    pub bus: u32,
    pub vendor: u16,
    pub device: u16,
    pub class: u32,
    pub owner: String,
    pub claimed: bool,
}

static PUBLISHED: Mutex<Vec<Published>> = Mutex::new(Vec::new());

fn text(raw: &[u8]) -> String {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().into()
}

fn name_arg(name: *const u8, len: usize) -> Option<String> {
    if name.is_null() {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(name, len.min(64)) };
    core::str::from_utf8(bytes).ok().map(String::from)
}

fn handle_of(dev: &pci::PciDevice) -> PciHandle {
    PciHandle {
        bus: dev.address.bus,
        device: dev.address.device,
        function: dev.address.function,
        _pad: 0,
        vendor: dev.vendor,
        device_id: dev.device,
        irq: dev.interrupt_line,
        _pad2: [0; 3],
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_pci_find_class(class: u8, subclass: u8, prog_if: u8, index: u32, out: *mut PciHandle) -> i32 {
    if out.is_null() {
        return -22;
    }
    let found = pci::devices()
        .into_iter()
        .filter(|d| (class == 0xFF || d.class == class) && (subclass == 0xFF || d.subclass == subclass) && (prog_if == 0xFF || d.prog_if == prog_if))
        .nth(index as usize);
    match found {
        Some(dev) => {
            unsafe { core::ptr::write_unaligned(out, handle_of(&dev)) };
            0
        }
        None => -19,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_device_from_pci(pci: *const PciHandle, out: *mut DeviceHandle) -> i32 {
    if pci.is_null() || out.is_null() {
        return -22;
    }
    let pci = unsafe { core::ptr::read_unaligned(pci) };
    let class = pci::devices()
        .into_iter()
        .find(|d| d.address.bus == pci.bus && d.address.device == pci.device && d.address.function == pci.function)
        .map(|d| ((d.class as u32) << 16) | ((d.subclass as u32) << 8) | d.prog_if as u32)
        .unwrap_or(0);
    let handle = DeviceHandle { bus: BUS_PCI, id: ((pci.bus as u32) << 8) | ((pci.device as u32) << 3) | pci.function as u32, pci, irq: pci.irq as u32, class, vendor: pci.vendor, device: pci.device_id, ..DeviceHandle::default() };
    unsafe { core::ptr::write_unaligned(out, handle) };
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_platform_find(compatible: *const u8, len: usize, index: u32, out: *mut DeviceHandle) -> i32 {
    let Some(wanted) = name_arg(compatible, len) else {
        return -22;
    };
    if out.is_null() {
        return -22;
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let Some(fdt) = crate::fdt::current() else {
            return -19;
        };
        let Some(node) = fdt.nodes().filter(|n| n.compatible_with(&wanted) && n.enabled()).nth(index as usize) else {
            return -19;
        };
        let (base, size) = node.reg().next().unwrap_or((0, 0));
        let irq = crate::arch::irq::interrupt_of(&node).unwrap_or(0);
        let handle = DeviceHandle { bus: BUS_PLATFORM, id: index, base, size, irq, ..DeviceHandle::default() };
        unsafe { core::ptr::write_unaligned(out, handle) };
        return 0;
    }
    #[cfg(target_arch = "x86_64")]
    {
        let _ = (wanted, index);
        -19
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_dma_run(addr: *const u8, len: usize, phys_out: *mut u64) -> usize {
    if addr.is_null() || phys_out.is_null() || len == 0 {
        return 0;
    }
    let (phys, run) = crate::memory::vmalloc::contiguous_run(addr as usize, len);
    unsafe { core::ptr::write_unaligned(phys_out, phys as u64) };
    run
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_register_block(name: *const u8, name_len: usize, ops: *const BlockOps) -> i32 {
    let Some(owner) = super::kpi::loading_module() else {
        return -1;
    };
    let Some(name) = name_arg(name, name_len) else {
        return -22;
    };
    if ops.is_null() {
        return -22;
    }
    let ops = unsafe { core::ptr::read_unaligned(ops) };
    if ops.abi != BLOCK_ABI || ops.read.is_none() || ops.sectors == 0 || ops.sector_size != 512 {
        return -22;
    }
    crate::drivers::block::register_module_disk(&owner, &name, &text(&ops.model), ops) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_input_key(scancode: u8) {
    crate::drivers::input::keyboard::inject_scancode(scancode);
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_input_pointer(dx: i32, dy: i32, buttons: u32, wheel: i32) {
    crate::drivers::input::mouse::inject(dx, dy, buttons as u8, wheel);
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_input_absolute(x: u32, y: u32, max_x: u32, max_y: u32, buttons: u32) {
    let (w, h) = crate::drivers::input::mouse::bounds();
    let px = (x as u64 * (w.max(1) as u64 - 1) / max_x.max(1) as u64) as i32;
    let py = (y as u64 * (h.max(1) as u64 - 1) / max_y.max(1) as u64) as i32;
    crate::drivers::input::mouse::inject_absolute(px, py, buttons as u8, 0);
}

struct ModuleAudio {
    ops: AudioOps,
    owner: String,
}

unsafe impl Send for ModuleAudio {}

impl crate::drivers::audio::AudioDevice for ModuleAudio {
    fn name(&self) -> String {
        text(&self.ops.name)
    }

    fn driver(&self) -> &'static str {
        "module"
    }

    fn rate(&self) -> u32 {
        self.ops.rate
    }

    fn ring_frames(&self) -> usize {
        self.ops.ring_frames as usize
    }

    fn ring(&mut self) -> &mut [i16] {
        unsafe { core::slice::from_raw_parts_mut(self.ops.ring, self.ops.ring_frames as usize * 2) }
    }

    fn position(&mut self) -> usize {
        self.ops.position.map(|f| f(self.ops.context) as usize).unwrap_or(0) % self.ops.ring_frames.max(1) as usize
    }

    fn start(&mut self) {
        if let Some(f) = self.ops.start {
            f(self.ops.context);
        }
    }

    fn stop(&mut self) {
        if let Some(f) = self.ops.stop {
            f(self.ops.context);
        }
    }

    fn poll(&mut self) {
        if let Some(f) = self.ops.poll {
            f(self.ops.context);
        }
    }

    fn outputs(&self) -> String {
        alloc::format!("from module {}", self.owner)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_register_audio(ops: *const AudioOps) -> i32 {
    let Some(owner) = super::kpi::loading_module() else {
        return -1;
    };
    if ops.is_null() {
        return -22;
    }
    let ops = unsafe { core::ptr::read_unaligned(ops) };
    if ops.abi != AUDIO_ABI || ops.ring.is_null() || ops.ring_frames == 0 || ops.rate == 0 || ops.position.is_none() {
        return -22;
    }
    crate::drivers::audio::register_module_device(Box::new(ModuleAudio { ops, owner }));
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_bus_publish(bus: u32, vendor: u16, device: u16, class: u32) -> i32 {
    let owner = super::kpi::loading_module().unwrap_or_default();
    let mut list = PUBLISHED.lock();
    if list.iter().any(|p| p.bus == bus && p.vendor == vendor && p.device == device && p.class == class && p.owner == owner) {
        return 0;
    }
    list.push(Published { bus, vendor, device, class, owner, claimed: false });
    drop(list);
    crate::drivers::irq::schedule("bus", bus_autoload, 0);
    0
}

extern "C" fn bus_autoload(_: *mut core::ffi::c_void) {
    let loaded = super::autoload();
    if loaded > 0 {
        crate::drivers::klog::log(&alloc::format!("module: {} module(s) loaded for devices published on a module bus", loaded));
    }
}

pub fn published() -> Vec<Published> {
    PUBLISHED.lock().clone()
}

pub fn forget(owner: &str) {
    PUBLISHED.lock().retain(|p| p.owner != owner);
}

pub fn bus_name(bus: u32) -> &'static str {
    match bus {
        BUS_PCI => "pci",
        BUS_PLATFORM => "platform",
        BUS_VIRTUAL => "virtual",
        BUS_USB => "usb",
        _ => "bus",
    }
}

static USB_KEYS: Mutex<Vec<(usize, [u8; 8])>> = Mutex::new(Vec::new());

#[unsafe(no_mangle)]
pub extern "C" fn hamix_usb_register_hcd(handle: *const PciHandle, kind: u32, ports: u32) -> i32 {
    let Some(owner) = super::kpi::loading_module() else {
        return -1;
    };
    if handle.is_null() {
        return -22;
    }
    let handle = unsafe { core::ptr::read_unaligned(handle) };
    crate::drivers::usb::adopt_controller(handle.bus, handle.device, handle.function, kind, ports, &owner);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_usb_add_device(port: u32, vendor: u16, product: u16, interface: u32, speed: u32) -> i32 {
    let owner = super::kpi::loading_module().unwrap_or_default();
    let index = crate::drivers::usb::add_module_device(port as u8, vendor, product, interface, speed, &owner);
    let mut list = PUBLISHED.lock();
    if !list.iter().any(|p| p.bus == BUS_USB && p.vendor == vendor && p.device == product) {
        list.push(Published { bus: BUS_USB, vendor, device: product, class: interface, owner, claimed: false });
    }
    index as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_usb_remove_device(index: u32) {
    crate::drivers::usb::remove_module_device(index as usize);
    USB_KEYS.lock().retain(|(i, _)| *i != index as usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_usb_hid_report(index: u32, kind: u32, report: *const u8, len: usize) {
    if report.is_null() || len == 0 {
        return;
    }
    let data = unsafe { core::slice::from_raw_parts(report, len.min(64)) };
    match kind {
        1 => {
            let mut keys = USB_KEYS.lock();
            let slot = match keys.iter().position(|(i, _)| *i == index as usize) {
                Some(p) => p,
                None => {
                    keys.push((index as usize, [0; 8]));
                    keys.len() - 1
                }
            };
            let mut previous = keys[slot].1;
            crate::drivers::usb::hid::on_boot_keyboard_report(data, &mut previous);
            keys[slot].1 = previous;
        }
        2 => crate::drivers::usb::hid::on_boot_mouse_report(data),
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_register_netdev(driver: *const u8, len: usize, ops: *const crate::net::module::NetOps) -> i32 {
    if ops.is_null() || driver.is_null() {
        return -22;
    }
    let owner = super::kpi::loading_module().unwrap_or_default();
    let driver = core::str::from_utf8(unsafe { core::slice::from_raw_parts(driver, len.min(64)) }).unwrap_or("module");
    let abi = unsafe { core::ptr::read_unaligned(ops as *const u32) };
    let ops = match abi {
        crate::net::module::NET_ABI => crate::net::module::NetOpsV2::from(unsafe { core::ptr::read_unaligned(ops) }),
        crate::net::module::NET_ABI_WIFI => unsafe { core::ptr::read_unaligned(ops as *const crate::net::module::NetOpsV2) },
        _ => return -22,
    };
    crate::net::module::register(driver, ops, &owner)
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_unregister_netdev(id: i32) {
    if id > 0 {
        crate::net::module::unregister(id as u32);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_net_receive(id: i32, frame: *const u8, len: usize) {
    if id <= 0 || frame.is_null() || len == 0 || len > 16384 {
        return;
    }
    crate::net::module::receive(id as u32, unsafe { core::slice::from_raw_parts(frame, len) });
}

#[unsafe(no_mangle)]
pub extern "C" fn hamix_net_carrier(id: i32, up: i32) {
    if id > 0 {
        crate::net::module::carrier(id as u32, up != 0);
    }
}
