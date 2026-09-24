#![no_std]

use core::ffi::c_void;

pub mod heap;
pub mod io;

pub const KPI_VERSION: u32 = 6;

pub const CLASS_NETWORK: u32 = 1;
pub const CLASS_DISPLAY: u32 = 2;
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

pub const DISPLAY_ABI: u32 = 3;

pub const CAP_FLUSH: u32 = 1 << 0;
pub const CAP_FILL: u32 = 1 << 1;
pub const CAP_COPY: u32 = 1 << 2;
pub const CAP_MODESET: u32 = 1 << 3;
pub const CAP_CURSOR: u32 = 1 << 4;
pub const CAP_REFRESH: u32 = 1 << 5;
pub const CAP_SCALE: u32 = 1 << 6;
pub const CAP_FLIP: u32 = 1 << 7;
pub const CAP_VBLANK: u32 = 1 << 8;
pub const CAP_CONNECTORS: u32 = 1 << 9;
pub const CAP_HOTPLUG: u32 = 1 << 10;

pub const CONNECTOR_UNKNOWN: u32 = 0;
pub const CONNECTOR_VGA: u32 = 1;
pub const CONNECTOR_DVI: u32 = 2;
pub const CONNECTOR_HDMI: u32 = 3;
pub const CONNECTOR_DP: u32 = 4;
pub const CONNECTOR_EDP: u32 = 5;
pub const CONNECTOR_LVDS: u32 = 6;
pub const CONNECTOR_VIRTUAL: u32 = 7;

pub const IRQ_NONE: i32 = 0;
pub const IRQ_HANDLED: i32 = 1;

pub const LEVEL_ERR: u32 = 0;
pub const LEVEL_WARN: u32 = 1;
pub const LEVEL_INFO: u32 = 2;
pub const LEVEL_DEBUG: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DisplayOps {
    pub abi: u32,
    pub caps: u32,
    pub context: u64,
    pub flush: Option<extern "C" fn(u64, u32, u32, u32, u32) -> i32>,
    pub fill: Option<extern "C" fn(u64, u32, u32, u32, u32, u32) -> i32>,
    pub copy: Option<extern "C" fn(u64, u32, u32, u32, u32, u32, u32) -> i32>,
    pub set_mode: Option<extern "C" fn(u64, u32, u32) -> i32>,
    pub set_refresh: Option<extern "C" fn(u64, u32) -> i32>,
    pub refresh_list: Option<extern "C" fn(u64, *mut u32, u32) -> i32>,
    pub mode_list: Option<extern "C" fn(u64, *mut u32, u32) -> i32>,
    pub cursor_set: Option<extern "C" fn(u64, *const u32, u32, u32, u32, u32) -> i32>,
    pub cursor_move: Option<extern "C" fn(u64, i32, i32) -> i32>,
    pub cursor_hide: Option<extern "C" fn(u64) -> i32>,
    pub connectors: Option<extern "C" fn(u64, *mut ConnectorDesc, u32) -> i32>,
    pub connector_modes: Option<extern "C" fn(u64, u32, *mut u32, u32) -> i32>,
    pub edid: Option<extern "C" fn(u64, u32, *mut u8, u32) -> i32>,
    pub set_output: Option<extern "C" fn(u64, u32, u32, u32) -> i32>,
    pub page_flip: Option<extern "C" fn(u64, u32) -> i32>,
    pub wait_vblank: Option<extern "C" fn(u64, u32) -> i32>,
    pub buffers: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ConnectorDesc {
    pub kind: u32,
    pub id: u32,
    pub status: u32,
    pub native_w: u32,
    pub native_h: u32,
    pub edid_len: u32,
    pub x: i32,
    pub y: i32,
    pub refresh: u32,
    pub primary: u32,
    pub name: [u8; 16],
}

impl ConnectorDesc {
    #[inline(always)]
    pub fn named(kind: u32, id: u32, name: &str) -> ConnectorDesc {
        let mut desc = ConnectorDesc { kind, id, ..ConnectorDesc::default() };
        let bytes = name.as_bytes();
        let mut i = 0;
        while i < bytes.len() && i < 15 {
            desc.name[i] = bytes[i];
            i += 1;
        }
        desc
    }
}

impl DisplayOps {
    pub const fn new(caps: u32, context: u64) -> DisplayOps {
        DisplayOps {
            abi: DISPLAY_ABI,
            caps,
            context,
            flush: None,
            fill: None,
            copy: None,
            set_mode: None,
            set_refresh: None,
            refresh_list: None,
            mode_list: None,
            cursor_set: None,
            cursor_move: None,
            cursor_hide: None,
            connectors: None,
            connector_modes: None,
            edid: None,
            set_output: None,
            page_flip: None,
            wait_vblank: None,
            buffers: 1,
            reserved: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FbDesc {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
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

pub const NET_ABI: u32 = 1;
pub const NET_ABI_WIFI: u32 = 2;
pub const NET_FLAG_WIFI: u32 = 1;

pub const WIFI_OPEN: u8 = 0;
pub const WIFI_WEP: u8 = 1;
pub const WIFI_WPA: u8 = 2;
pub const WIFI_WPA2: u8 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiBss {
    pub ssid: [u8; 32],
    pub ssid_len: u32,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub security: u8,
    pub signal: u32,
    pub _pad: u32,
    pub last_seen: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiStatus {
    pub associated: u32,
    pub signal: u32,
    pub ssid_len: u32,
    pub text_len: u32,
    pub ssid: [u8; 32],
    pub text: [u8; 96],
}

impl WifiStatus {
    #[inline(always)]
    pub fn set_ssid(&mut self, ssid: &[u8]) {
        let n = ssid.len().min(32);
        self.ssid[..n].copy_from_slice(&ssid[..n]);
        self.ssid_len = n as u32;
    }

    #[inline(always)]
    pub fn set_text(&mut self, text: &[u8]) {
        let n = text.len().min(96);
        self.text[..n].copy_from_slice(&text[..n]);
        self.text_len = n as u32;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiOps {
    pub scan: Option<extern "C" fn(u64)>,
    pub networks: Option<extern "C" fn(u64, *mut WifiBss, u32) -> i32>,
    pub connect: Option<extern "C" fn(u64, *const u8, usize, *const u8, usize) -> i32>,
    pub disconnect: Option<extern "C" fn(u64)>,
    pub status: Option<extern "C" fn(u64, *mut WifiStatus) -> i32>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiNetOps {
    pub abi: u32,
    pub flags: u32,
    pub context: u64,
    pub mac: [u8; 8],
    pub transmit: Option<extern "C" fn(u64, *const u8, usize) -> i32>,
    pub set_enabled: Option<extern "C" fn(u64, i32)>,
    pub wifi: WifiOps,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetOps {
    pub abi: u32,
    pub flags: u32,
    pub context: u64,
    pub mac: [u8; 8],
    pub transmit: Option<extern "C" fn(u64, *const u8, usize) -> i32>,
    pub set_enabled: Option<extern "C" fn(u64, i32)>,
}

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

unsafe extern "C" {
    pub fn hamix_kpi_version() -> u32;
    pub fn hamix_kmalloc(size: usize) -> *mut u8;
    pub fn hamix_kzalloc(size: usize) -> *mut u8;
    pub fn hamix_kfree(ptr: *mut u8);
    pub fn hamix_dma_alloc(len: usize, phys_out: *mut u64) -> *mut u8;
    pub fn hamix_dma_free(ptr: *mut u8, len: usize);
    pub fn hamix_virt_to_phys(addr: *const u8) -> u64;
    pub fn hamix_phys_to_virt(addr: u64) -> *mut u8;
    pub fn hamix_ioremap(phys: u64, len: usize) -> *mut u8;
    pub fn hamix_iounmap(addr: *mut u8, len: usize);
    pub fn hamix_readb(addr: *const u8) -> u8;
    pub fn hamix_readw(addr: *const u16) -> u16;
    pub fn hamix_readl(addr: *const u32) -> u32;
    pub fn hamix_readq(addr: *const u64) -> u64;
    pub fn hamix_writeb(value: u8, addr: *mut u8);
    pub fn hamix_writew(value: u16, addr: *mut u16);
    pub fn hamix_writel(value: u32, addr: *mut u32);
    pub fn hamix_writeq(value: u64, addr: *mut u64);
    pub fn hamix_inb(port: u16) -> u8;
    pub fn hamix_inw(port: u16) -> u16;
    pub fn hamix_inl(port: u16) -> u32;
    pub fn hamix_outb(value: u8, port: u16);
    pub fn hamix_outw(value: u16, port: u16);
    pub fn hamix_outl(value: u32, port: u16);
    pub fn hamix_pci_find(vendor: u16, device: u16, out: *mut PciHandle) -> i32;
    pub fn hamix_pci_read32(handle: *const PciHandle, offset: u8) -> u32;
    pub fn hamix_pci_write32(handle: *const PciHandle, offset: u8, value: u32);
    pub fn hamix_pci_read16(handle: *const PciHandle, offset: u8) -> u16;
    pub fn hamix_pci_write16(handle: *const PciHandle, offset: u8, value: u16);
    pub fn hamix_pci_bar(handle: *const PciHandle, index: u8, len_out: *mut u64) -> u64;
    pub fn hamix_pci_enable(handle: *const PciHandle);
    pub fn hamix_udelay(micros: u64);
    pub fn hamix_mdelay(millis: u64);
    pub fn hamix_uptime_ms() -> u64;
    pub fn hamix_printk(text: *const u8, len: usize);
    pub fn hamix_display_abi() -> u32;
    pub fn hamix_display_changed(fb: *const FbDesc) -> i32;
    pub fn hamix_display_current(out: *mut FbDesc) -> i32;
    pub fn hamix_display_describe(model: *const u8, model_len: usize, codename: *const u8, codename_len: usize, generation: *const u8, generation_len: usize) -> i32;
    pub fn hamix_budget_left() -> i64;
    pub fn hamix_register_display(name: *const u8, name_len: usize, ops: *const DisplayOps, fb: *const FbDesc) -> i32;
    pub fn hamix_display_hotplug();
    pub fn hamix_pci_find_class(class: u8, subclass: u8, prog_if: u8, index: u32, out: *mut PciHandle) -> i32;
    pub fn hamix_device_from_pci(pci: *const PciHandle, out: *mut DeviceHandle) -> i32;
    pub fn hamix_platform_find(compatible: *const u8, len: usize, index: u32, out: *mut DeviceHandle) -> i32;
    pub fn hamix_dma_run(addr: *const u8, len: usize, phys_out: *mut u64) -> usize;
    pub fn hamix_register_block(name: *const u8, name_len: usize, ops: *const BlockOps) -> i32;
    pub fn hamix_input_key(scancode: u8);
    pub fn hamix_input_pointer(dx: i32, dy: i32, buttons: u32, wheel: i32);
    pub fn hamix_input_absolute(x: u32, y: u32, max_x: u32, max_y: u32, buttons: u32);
    pub fn hamix_register_audio(ops: *const AudioOps) -> i32;
    pub fn hamix_bus_publish(bus: u32, vendor: u16, device: u16, class: u32) -> i32;
    pub fn hamix_usb_register_hcd(handle: *const PciHandle, kind: u32, ports: u32) -> i32;
    pub fn hamix_usb_add_device(port: u32, vendor: u16, product: u16, interface: u32, speed: u32) -> i32;
    pub fn hamix_usb_remove_device(index: u32);
    pub fn hamix_usb_hid_report(index: u32, kind: u32, report: *const u8, len: usize);
    pub fn hamix_register_netdev(driver: *const u8, len: usize, ops: *const NetOps) -> i32;
    pub fn hamix_unregister_netdev(id: i32);
    pub fn hamix_net_receive(id: i32, frame: *const u8, len: usize);
    pub fn hamix_net_carrier(id: i32, up: i32);
    pub fn hamix_edid_modes(edid: *const u8, len: usize, out: *mut u32, max: u32) -> i32;
    pub fn hamix_request_irq(handle: *const PciHandle, handler: Option<extern "C" fn(*mut c_void) -> i32>, context: *mut c_void) -> i32;
    pub fn hamix_free_irq();
    pub fn hamix_pci_msi_enable(handle: *const PciHandle) -> i32;
    pub fn hamix_schedule_work(work: Option<extern "C" fn(*mut c_void)>, context: *mut c_void) -> i32;
    pub fn hamix_in_interrupt() -> i32;
    pub fn hamix_param_u32(name: *const u8, name_len: usize, fallback: u32) -> u32;
    pub fn hamix_dev_log(level: u32, text: *const u8, len: usize);
    pub fn hamix_claim_device(
        class: u32,
        name: *const u8,
        name_len: usize,
        handle: *const PciHandle,
        poll: Option<extern "C" fn(*mut c_void)>,
        context: *mut c_void,
    ) -> i32;
}

#[inline(always)]
pub fn printk(text: &str) {
    unsafe { hamix_printk(text.as_ptr(), text.len()) }
}

#[inline(always)]
pub fn request_irq(handle: &PciHandle, handler: extern "C" fn(*mut c_void) -> i32, context: *mut c_void) -> Option<u8> {
    let vector = unsafe { hamix_request_irq(handle, Some(handler), context) };
    if vector >= 0 { Some(vector as u8) } else { None }
}

#[inline(always)]
pub fn schedule_work(work: extern "C" fn(*mut c_void), context: *mut c_void) -> bool {
    unsafe { hamix_schedule_work(Some(work), context) == 0 }
}

#[inline(always)]
pub fn param(name: &str, fallback: u32) -> u32 {
    unsafe { hamix_param_u32(name.as_ptr(), name.len(), fallback) }
}

#[inline(always)]
pub fn dev_log(level: u32, text: &str) {
    unsafe { hamix_dev_log(level, text.as_ptr(), text.len()) }
}

#[inline(always)]
pub fn dev_err(text: &str) {
    dev_log(LEVEL_ERR, text)
}

#[inline(always)]
pub fn dev_warn(text: &str) {
    dev_log(LEVEL_WARN, text)
}

#[inline(always)]
pub fn dev_info(text: &str) {
    dev_log(LEVEL_INFO, text)
}

#[inline(always)]
pub fn dev_dbg(text: &str) {
    dev_log(LEVEL_DEBUG, text)
}

#[inline(always)]
pub fn find_device(vendor: u16, device: u16) -> Option<PciHandle> {
    let mut handle = PciHandle::default();
    if unsafe { hamix_pci_find(vendor, device, &mut handle) } == 0 { Some(handle) } else { None }
}

#[inline(always)]
pub fn claim(class: u32, name: &str, handle: &PciHandle, poll: Option<extern "C" fn(*mut c_void)>, context: *mut c_void) -> bool {
    unsafe { hamix_claim_device(class, name.as_ptr(), name.len(), handle, poll, context) == 0 }
}

#[inline(always)]
pub fn bar(handle: &PciHandle, index: u8) -> (u64, u64) {
    let mut len = 0u64;
    let base = unsafe { hamix_pci_bar(handle, index, &mut len) };
    (base, len)
}

#[inline(always)]
pub fn current_framebuffer() -> Option<FbDesc> {
    let mut fb = FbDesc::default();
    if unsafe { hamix_display_current(&mut fb) } == 0 { Some(fb) } else { None }
}

#[inline(always)]
pub fn display_describe(model: &str, codename: &str, generation: &str) {
    unsafe { hamix_display_describe(model.as_ptr(), model.len(), codename.as_ptr(), codename.len(), generation.as_ptr(), generation.len()) };
}

#[inline(always)]
pub fn display_changed(fb: &FbDesc) -> bool {
    unsafe { hamix_display_changed(fb) == 0 }
}

#[inline(always)]
pub fn find_class(class: u8, subclass: u8, prog_if: u8, index: u32) -> Option<PciHandle> {
    let mut handle = PciHandle::default();
    if unsafe { hamix_pci_find_class(class, subclass, prog_if, index, &mut handle) } == 0 { Some(handle) } else { None }
}

#[inline(always)]
pub fn dma_run(addr: *const u8, len: usize) -> (u64, usize) {
    let mut phys = 0u64;
    let run = unsafe { hamix_dma_run(addr, len, &mut phys) };
    (phys, run)
}

#[inline(always)]
pub fn register_block(name: &str, ops: &BlockOps) -> i32 {
    unsafe { hamix_register_block(name.as_ptr(), name.len(), ops) }
}

#[inline(always)]
pub fn bus_publish(bus: u32, vendor: u16, device: u16, class: u32) -> bool {
    unsafe { hamix_bus_publish(bus, vendor, device, class) == 0 }
}

#[inline(always)]
pub fn register_netdev(driver: &str, ops: &NetOps) -> i32 {
    unsafe { hamix_register_netdev(driver.as_ptr(), driver.len(), ops) }
}

#[inline(always)]
pub fn register_wifi(driver: &str, ops: &WifiNetOps) -> i32 {
    unsafe { hamix_register_netdev(driver.as_ptr(), driver.len(), ops as *const WifiNetOps as *const NetOps) }
}

#[inline(always)]
pub fn net_receive(id: i32, frame: &[u8]) {
    unsafe { hamix_net_receive(id, frame.as_ptr(), frame.len()) }
}

#[inline(always)]
pub fn net_carrier(id: i32, up: bool) {
    unsafe { hamix_net_carrier(id, up as i32) }
}

#[inline(always)]
pub fn display_hotplug() {
    unsafe { hamix_display_hotplug() }
}

#[inline(always)]
pub fn edid_modes(edid: &[u8], out: &mut [u32]) -> usize {
    let count = unsafe { hamix_edid_modes(edid.as_ptr(), edid.len(), out.as_mut_ptr(), (out.len() / 3) as u32) };
    if count < 0 { 0 } else { count as usize }
}

#[inline(always)]
pub fn register_display(name: &str, ops: &DisplayOps, fb: &FbDesc) -> bool {
    unsafe { hamix_register_display(name.as_ptr(), name.len(), ops, fb) == 0 }
}

#[macro_export]
macro_rules! module {
    (init = $init:path, exit = $exit:path $(,)?) => {
        $crate::module!(init = $init, exit = $exit, version = "0");
    };
    (init = $init:path, exit = $exit:path, version = $version:literal $(,)?) => {
        #[unsafe(no_mangle)]
        pub static hamix_module_version: [u8; $version.len() + 1] = {
            let mut out = [0u8; $version.len() + 1];
            let source = $version.as_bytes();
            let mut at = 0;
            while at < source.len() {
                out[at] = source[at];
                at += 1;
            }
            out
        };

        #[unsafe(no_mangle)]
        pub extern "C" fn hamix_module_init() -> i32 {
            if unsafe { $crate::hamix_kpi_version() } < $crate::KPI_VERSION {
                return -1;
            }
            let f: fn() -> i32 = $init;
            f()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn hamix_module_exit() {
            let f: fn() = $exit;
            f()
        }

        #[panic_handler]
        fn __module_panic(_: &core::panic::PanicInfo) -> ! {
            $crate::printk("module panicked");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    (init = $init:path, exit = $exit:path, version = $version:literal, suspend = $suspend:path, resume = $resume:path $(,)?) => {
        $crate::module!(init = $init, exit = $exit, version = $version);

        #[unsafe(no_mangle)]
        pub extern "C" fn hamix_module_suspend() {
            let f: fn() = $suspend;
            f()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn hamix_module_resume() -> i32 {
            let f: fn() -> i32 = $resume;
            f()
        }
    };
}
