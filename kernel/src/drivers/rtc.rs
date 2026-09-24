use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch::io::{inb, outb};
use crate::arch::without_interrupts;

static BOOT_EPOCH: AtomicU64 = AtomicU64::new(0);

fn read_register(register: u8) -> u8 {
    outb(0x70, register | 0x80);
    inb(0x71)
}

fn update_in_progress() -> bool {
    read_register(0x0A) & 0x80 != 0
}

fn snapshot() -> [u8; 7] {
    let mut spins = 0;
    while update_in_progress() && spins < 10_000 {
        spins += 1;
    }
    [
        read_register(0x00),
        read_register(0x02),
        read_register(0x04),
        read_register(0x07),
        read_register(0x08),
        read_register(0x09),
        read_register(0x32),
    ]
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

static DEVICE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static KIND: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub const PL031: u8 = 1;
pub const GOLDFISH: u8 = 2;

pub fn attach(base: u64, kind: u8) {
    KIND.store(kind, Ordering::Relaxed);
    DEVICE.store(base, Ordering::Relaxed);
}

fn device_epoch() -> Option<u64> {
    let base = DEVICE.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }
    Some(unsafe {
        match KIND.load(Ordering::Relaxed) {
            PL031 => core::ptr::read_volatile(base as *const u32) as u64,
            _ => {
                let low = core::ptr::read_volatile(base as *const u32) as u64;
                let high = core::ptr::read_volatile((base + 4) as *const u32) as u64;
                ((high << 32) | low) / 1_000_000_000
            }
        }
    })
}

pub fn init() {
    if let Some(epoch) = device_epoch() {
        BOOT_EPOCH.store(epoch.saturating_sub(crate::task::uptime_ms() / 1000), Ordering::Relaxed);
        return;
    }
    if !crate::arch::io::PORTS {
        return;
    }
    let epoch = without_interrupts(|| {
        let mut current = snapshot();
        for _ in 0..4 {
            let again = snapshot();
            if again == current {
                break;
            }
            current = again;
        }
        let status_b = read_register(0x0B);
        let binary = status_b & 0x04 != 0;
        let h24 = status_b & 0x02 != 0;
        let decode = |v: u8| if binary { v as i64 } else { ((v >> 4) * 10 + (v & 0x0F)) as i64 };
        let second = decode(current[0]);
        let minute = decode(current[1]);
        let pm = current[2] & 0x80 != 0;
        let mut hour = decode(current[2] & 0x7F);
        if !h24 {
            hour %= 12;
            if pm {
                hour += 12;
            }
        }
        let day = decode(current[3]);
        let month = decode(current[4]);
        let mut year = decode(current[5]);
        let century = decode(current[6]);
        year += if (19..=21).contains(&century) { century * 100 } else { 2000 };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return 0;
        }
        let days = days_from_civil(year, month, day);
        (days * 86_400 + hour * 3600 + minute * 60 + second).max(0) as u64
    });
    BOOT_EPOCH.store(epoch, Ordering::Relaxed);
}

pub fn boot_epoch() -> u64 {
    BOOT_EPOCH.load(Ordering::Relaxed)
}

pub fn now() -> u64 {
    boot_epoch() + crate::task::uptime_ms() / 1000
}

pub fn format_datetime(epoch: u64) -> alloc::string::String {
    let days = (epoch / 86_400) as i64;
    let secs = epoch % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    alloc::format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year,
        month,
        day,
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}
