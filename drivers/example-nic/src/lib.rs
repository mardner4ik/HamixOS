#![no_std]

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use hamix_kpi::{self as kpi, PciHandle};

const VENDOR_INTEL: u16 = 0x8086;
const DEVICE_82540EM: u16 = 0x100E;

static POLLS: AtomicU64 = AtomicU64::new(0);
static mut STATE: State = State { handle: PciHandle { bus: 0, device: 0, function: 0, _pad: 0, vendor: 0, device_id: 0, irq: 0, _pad2: [0; 3] }, mmio: 0, mmio_len: 0 };

struct State {
    handle: PciHandle,
    mmio: u64,
    mmio_len: u64,
}

extern "C" fn poll(_context: *mut c_void) {
    POLLS.fetch_add(1, Ordering::Relaxed);
}

fn init() -> i32 {
    let Some(handle) = kpi::find_device(VENDOR_INTEL, DEVICE_82540EM) else {
        kpi::printk("example-nic: no 8086:100e on this machine");
        return -1;
    };
    let (base, len) = kpi::bar(&handle, 0);
    if base == 0 {
        kpi::printk("example-nic: device has no memory BAR");
        return -1;
    }
    unsafe {
        hamix_kpi::hamix_pci_enable(&handle);
        let state = &raw mut STATE;
        (*state).handle = handle;
        (*state).mmio = base;
        (*state).mmio_len = len;
    }
    if !kpi::claim(kpi::CLASS_NETWORK, "eth-kpi0", &handle, Some(poll), core::ptr::null_mut()) {
        kpi::printk("example-nic: the kernel refused the device claim");
        return -1;
    }
    kpi::printk("example-nic: claimed 8086:100e through the module KPI");
    0
}

fn exit() {
    kpi::printk("example-nic: released");
}

hamix_kpi::module!(init = init, exit = exit);
