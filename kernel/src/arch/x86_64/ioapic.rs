use alloc::vec::Vec;
use spin::Mutex;

const REG_VERSION: u32 = 0x01;
const REG_REDIRECT: u32 = 0x10;
const MASKED: u32 = 1 << 16;
const LEVEL: u32 = 1 << 15;
const ACTIVE_LOW: u32 = 1 << 13;

#[derive(Clone, Copy)]
struct Chip {
    base: u64,
    gsi_base: u32,
    pins: u32,
}

static CHIPS: Mutex<Option<Vec<Chip>>> = Mutex::new(None);

fn read(base: u64, index: u32) -> u32 {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, index);
        core::ptr::read_volatile((base + 0x10) as *const u32)
    }
}

fn write(base: u64, index: u32, value: u32) {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, index);
        core::ptr::write_volatile((base + 0x10) as *mut u32, value);
    }
}

fn chips() -> Vec<Chip> {
    let mut guard = CHIPS.lock();
    if guard.is_none() {
        let mut found = Vec::new();
        if let Some(madt) = super::acpi::madt() {
            for (_, base, gsi_base) in madt.ioapics {
                if base == 0 {
                    continue;
                }
                let version = read(base as u64, REG_VERSION);
                let pins = ((version >> 16) & 0xFF) + 1;
                if version == 0xFFFF_FFFF || pins > 240 {
                    continue;
                }
                found.push(Chip { base: base as u64, gsi_base, pins });
            }
        }
        *guard = Some(found);
    }
    guard.clone().unwrap_or_default()
}

fn find(gsi: u32) -> Option<(u64, u32)> {
    chips().into_iter().find(|c| gsi >= c.gsi_base && gsi < c.gsi_base + c.pins).map(|c| (c.base, gsi - c.gsi_base))
}

pub fn covers(gsi: u32) -> bool {
    find(gsi).is_some()
}

pub fn summary() -> Vec<(u64, u32, u32)> {
    chips().into_iter().map(|c| (c.base, c.gsi_base, c.pins)).collect()
}

pub fn route_pci(gsi: u32, vector: u8, destination: u8) -> bool {
    let Some((base, pin)) = find(gsi) else {
        return false;
    };
    let index = REG_REDIRECT + pin * 2;
    write(base, index, MASKED);
    write(base, index + 1, (destination as u32) << 24);
    write(base, index, vector as u32 | LEVEL | ACTIVE_LOW);
    true
}

pub fn set_masked(gsi: u32, masked: bool) {
    let Some((base, pin)) = find(gsi) else {
        return;
    };
    let index = REG_REDIRECT + pin * 2;
    let low = read(base, index);
    write(base, index, if masked { low | MASKED } else { low & !MASKED });
}

pub fn mask_from_interrupt(gsi: u32) -> bool {
    let Some(guard) = CHIPS.try_lock() else {
        return false;
    };
    let Some(chip) = guard.as_ref().and_then(|list| list.iter().copied().find(|c| gsi >= c.gsi_base && gsi < c.gsi_base + c.pins)) else {
        return false;
    };
    drop(guard);
    let index = REG_REDIRECT + (gsi - chip.gsi_base) * 2;
    let low = read(chip.base, index);
    write(chip.base, index, low | MASKED);
    true
}
