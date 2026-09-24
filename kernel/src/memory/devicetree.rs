use alloc::string::String;

use super::{frame, kernel_end, kernel_start, ModuleInfo, CMDLINE, MODULES};
use crate::fdt::Fdt;

pub struct Layout {
    pub banks: usize,
    pub reserved: usize,
}

fn reserve(base: u64, size: u64, layout: &mut Layout) {
    if size == 0 {
        return;
    }
    frame::reserve_region(base as usize, size as usize);
    layout.reserved += 1;
}

fn register_module(name: &str, start: u64, end: u64) {
    if end <= start || end > u32::MAX as u64 {
        return;
    }
    let mut label = [0u8; 32];
    let len = name.len().min(label.len());
    label[..len].copy_from_slice(&name.as_bytes()[..len]);
    let mut mods = MODULES.lock();
    if let Some(slot) = mods.iter_mut().find(|s| s.is_none()) {
        *slot = Some(ModuleInfo { start: start as u32, end: end as u32, name: label, name_len: len });
    }
}

static BOOT_INITRD: spin::Mutex<Option<(u64, u64)>> = spin::Mutex::new(None);

pub fn set_boot_initrd(start: u64, end: u64) {
    *BOOT_INITRD.lock() = Some((start, end));
}

fn register_initrd(start: u64, end: u64, layout: &mut Layout) {
    reserve(start, end - start, layout);
    let tar = unsafe { core::slice::from_raw_parts((start + 257) as *const u8, 5) } == b"ustar";
    register_module(if tar { "initramfs" } else { "hext" }, start, end);
}

pub fn init_devicetree(fdt: &Fdt) -> Layout {
    let mut layout = Layout { banks: 0, reserved: 0 };
    let (image_start, image_end) = (kernel_start() as u64, kernel_end() as u64);
    let mut image_bank = None;

    for node in fdt.nodes().filter(|n| n.str_property("device_type") == Some("memory") && n.enabled()) {
        for (base, size) in node.reg() {
            if size == 0 {
                continue;
            }
            frame::add_region(base as usize, size as usize);
            layout.banks += 1;
            if (base..base + size).contains(&image_start) {
                image_bank = Some(base);
            }
        }
    }

    let floor = image_bank.unwrap_or(image_start);
    reserve(floor, image_end - floor, &mut layout);
    reserve(fdt.address() as u64, fdt.total_size() as u64, &mut layout);
    for (base, size) in fdt.reservations() {
        reserve(base, size, &mut layout);
    }
    if let Some(reserved) = fdt.find("/reserved-memory") {
        for child in reserved.children() {
            for (base, size) in child.reg() {
                reserve(base, size, &mut layout);
            }
        }
    }

    if let Some(chosen) = fdt.chosen() {
        if let Some(args) = chosen.str_property("bootargs") {
            *CMDLINE.lock() = String::from(args);
        }
        let start = chosen.u64_property("linux,initrd-start");
        let end = chosen.u64_property("linux,initrd-end");
        if let (Some(start), Some(end)) = (start, end) {
            if end > start {
                register_initrd(start, end, &mut layout);
                return layout;
            }
        }
    }
    if let Some((start, end)) = *BOOT_INITRD.lock() {
        register_initrd(start, end, &mut layout);
    }
    layout
}
