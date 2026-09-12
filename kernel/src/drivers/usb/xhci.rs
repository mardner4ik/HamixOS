use alloc::string::String;

use crate::drivers::usb::Controller;

const CAPLENGTH: usize = 0x00;
const HCSPARAMS1: usize = 0x04;
const HCIVERSION: usize = 0x02;

const OP_USBCMD: usize = 0x00;
const OP_USBSTS: usize = 0x04;

const CMD_RUN: u32 = 1 << 0;
const CMD_RESET: u32 = 1 << 1;
const STS_CONTROLLER_NOT_READY: u32 = 1 << 11;

unsafe fn read32(base: u64, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base as usize + offset) as *const u32) }
}

unsafe fn write32(base: u64, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base as usize + offset) as *mut u32, value) }
}

pub fn attach(controller: &mut Controller) {
    let Some(base) = controller.device.mmio_bar(0) else {
        controller.note = String::from("no MMIO BAR");
        return;
    };
    controller.base = base;

    if base == 0 || base >= 0x1_0000_0000 {
        controller.note = String::from("register window outside the identity map");
        return;
    }

    let capability_length = unsafe { read32(base, CAPLENGTH) & 0xFF } as usize;
    let version = unsafe { (read32(base, CAPLENGTH) >> 16) & 0xFFFF };
    let structural = unsafe { read32(base, HCSPARAMS1) };
    let ports = ((structural >> 24) & 0xFF) as u8;
    let slots = (structural & 0xFF) as u8;
    let _ = HCIVERSION;

    let operational = base + capability_length as u64;

    unsafe {
        let command = read32(operational, OP_USBCMD);
        write32(operational, OP_USBCMD, command & !CMD_RUN);
        for _ in 0..1000 {
            if read32(operational, OP_USBSTS) & 1 != 0 {
                break;
            }
        }
        write32(operational, OP_USBCMD, CMD_RESET);
        for _ in 0..100_000 {
            let status = read32(operational, OP_USBSTS);
            if read32(operational, OP_USBCMD) & CMD_RESET == 0
                && status & STS_CONTROLLER_NOT_READY == 0
            {
                break;
            }
        }
    }

    controller.ports = ports;
    controller.note = alloc::format!(
        "USB {}.{} spec, {} root port(s), {} device slot(s); reset done, transfer rings not implemented yet",
        version >> 8,
        (version >> 4) & 0xF,
        ports,
        slots
    );
}
