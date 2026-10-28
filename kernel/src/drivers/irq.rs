use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::{self, lapic, outb};

pub const LEGACY_BASE: u8 = 32;
pub const MSI_FIRST: u8 = 0x50;
pub const MSI_LAST: u8 = 0x6F;
pub const VECTORS: usize = 256;

pub const HANDLED: i32 = 1;

pub const STALL_MS: u64 = 250;

pub const STORM_LIMIT: u32 = 20_000;

pub type Handler = extern "C" fn(*mut c_void) -> i32;
pub type Work = extern "C" fn(*mut c_void);

pub struct Line {
    pub vector: u8,
    pub owner: String,
    pub handler: Handler,
    pub context: u64,
    pub legacy: Option<u8>,
    pub gsi: Option<u32>,
}

struct Job {
    owner: String,
    work: Work,
    context: u64,
}

static LINES: Mutex<Vec<Line>> = Mutex::new(Vec::new());
static QUEUE: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
static COUNTS: [AtomicU64; VECTORS] = [const { AtomicU64::new(0) }; VECTORS];
static SPURIOUS: AtomicU64 = AtomicU64::new(0);
static WORKED: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static UNHANDLED: [AtomicU32; VECTORS] = [const { AtomicU32::new(0) }; VECTORS];
static STORMED: AtomicU32 = AtomicU32::new(0);
static REPORTED: AtomicU32 = AtomicU32::new(0);
static STORMED_GSI: AtomicU64 = AtomicU64::new(0);
static REPORTED_GSI: AtomicU64 = AtomicU64::new(0);

fn report_storms() {
    let gsis = STORMED_GSI.load(Ordering::Relaxed);
    let fresh_gsi = gsis & !REPORTED_GSI.fetch_or(gsis, Ordering::Relaxed);
    for gsi in (0..64u32).filter(|g| fresh_gsi & (1 << g) != 0) {
        let owners: Vec<String> = LINES.lock().iter().filter(|l| l.gsi == Some(gsi)).map(|l| l.owner.clone()).collect();
        crate::drivers::klog::log(&alloc::format!("irq: GSI {} fired {} times in a row with no handler claiming it; masked in the IOAPIC, {} fall back to polling", gsi, STORM_LIMIT, owners.join(", ")));
    }
    let mask = STORMED.load(Ordering::Relaxed);
    let fresh = mask & !REPORTED.fetch_or(mask, Ordering::Relaxed);
    for line in (0..16u8).filter(|l| fresh & (1 << l) != 0) {
        let owners: Vec<String> = LINES.lock().iter().filter(|l| l.legacy == Some(line)).map(|l| l.owner.clone()).collect();
        crate::drivers::klog::log(&alloc::format!(
            "irq: line {} fired {} times in a row with no handler claiming it (shared with a device nobody drives?); masked, {} fall back to polling",
            line,
            STORM_LIMIT,
            if owners.is_empty() { String::from("no driver") } else { owners.join(", ") }
        ));
    }
}

pub fn count(vector: u8) -> u64 {
    COUNTS[vector as usize].load(Ordering::Relaxed)
}

pub fn stats() -> (u64, u64, u64) {
    (SPURIOUS.load(Ordering::Relaxed), WORKED.load(Ordering::Relaxed), DROPPED.load(Ordering::Relaxed))
}

pub fn in_handler() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[cfg(not(target_arch = "x86_64"))]
fn on_line(line: u32) {
    dispatch(line as u8);
}

#[cfg(not(target_arch = "x86_64"))]
fn mask_legacy(line: u8, masked: bool) {
    if masked {
        crate::arch::irq::disable(line as u32);
    } else {
        crate::arch::irqtab::register(line as u32, on_line);
        crate::arch::irq::enable(line as u32);
    }
}

#[cfg(target_arch = "x86_64")]
fn mask_legacy(line: u8, masked: bool) {
    let (port, shift) = if line < 8 { (0x21u16, line) } else { (0xA1u16, line - 8) };
    let value = x86_64::inb(port);
    let updated = if masked { value | (1 << shift) } else { value & !(1 << shift) };
    outb(port, updated);
    if line >= 8 && !masked {
        let master = x86_64::inb(0x21);
        outb(0x21, master & !(1 << 2));
    }
}

fn free_vector() -> Option<u8> {
    let lines = LINES.lock();
    (MSI_FIRST..=MSI_LAST).find(|v| !lines.iter().any(|l| l.vector == *v))
}

pub fn request(owner: &str, vector: u8, legacy: Option<u8>, handler: Handler, context: u64) -> bool {
    if vector < LEGACY_BASE && cfg!(target_arch = "x86_64") {
        return false;
    }
    let mut lines = LINES.lock();
    if lines.iter().any(|l| l.owner == owner && l.vector == vector) {
        return false;
    }
    lines.push(Line { vector, owner: owner.to_string(), handler, context, legacy, gsi: None });
    drop(lines);
    if let Some(line) = legacy {
        mask_legacy(line, false);
    }
    true
}

#[cfg(not(target_arch = "x86_64"))]
pub fn request_legacy(owner: &str, line: u8, handler: Handler, context: u64) -> Option<u8> {
    if request(owner, line, Some(line), handler, context) { Some(line) } else { None }
}

#[cfg(target_arch = "x86_64")]
pub fn request_legacy(owner: &str, line: u8, handler: Handler, context: u64) -> Option<u8> {
    if line > 15 {
        return None;
    }
    let vector = LEGACY_BASE + line;
    if request(owner, vector, Some(line), handler, context) { Some(vector) } else { None }
}

#[cfg(target_arch = "x86_64")]
pub fn request_gsi(owner: &str, gsi: u32, handler: Handler, context: u64) -> Option<u8> {
    use crate::arch::x86_64::{ioapic, lapic};
    if gsi < 16 || !ioapic::covers(gsi) {
        return None;
    }
    let shared = LINES.lock().iter().find(|l| l.gsi == Some(gsi)).map(|l| l.vector);
    let vector = match shared {
        Some(vector) => vector,
        None => free_vector()?,
    };
    {
        let mut lines = LINES.lock();
        if lines.iter().any(|l| l.owner == owner && l.vector == vector) {
            return None;
        }
        lines.push(Line { vector, owner: owner.to_string(), handler, context, legacy: None, gsi: Some(gsi) });
    }
    if shared.is_none() && !ioapic::route_pci(gsi, vector, lapic::id()) {
        LINES.lock().retain(|l| !(l.owner == owner && l.vector == vector));
        return None;
    }
    Some(vector)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn request_gsi(_owner: &str, _gsi: u32, _handler: Handler, _context: u64) -> Option<u8> {
    None
}

#[cfg(target_arch = "x86_64")]
fn mask_gsi(gsi: u32) {
    crate::arch::x86_64::ioapic::set_masked(gsi, true);
}

#[cfg(not(target_arch = "x86_64"))]
fn mask_gsi(_gsi: u32) {}

pub fn request_message(owner: &str, handler: Handler, context: u64) -> Option<u8> {
    let vector = free_vector()?;
    if request(owner, vector, None, handler, context) { Some(vector) } else { None }
}

pub fn release_vector(owner: &str, vector: u8) {
    let mut lines = LINES.lock();
    let legacy: Vec<Option<u8>> = lines.iter().filter(|l| l.owner == owner && l.vector == vector).map(|l| l.legacy).collect();
    lines.retain(|l| !(l.owner == owner && l.vector == vector));
    let still: Vec<u8> = lines.iter().filter_map(|l| l.legacy).collect();
    drop(lines);
    for line in legacy.into_iter().flatten() {
        if !still.contains(&line) {
            mask_legacy(line, true);
        }
    }
}

pub fn release(owner: &str) {
    let mut lines = LINES.lock();
    let freed: Vec<Option<u8>> = lines.iter().filter(|l| l.owner == owner).map(|l| l.legacy).collect();
    let freed_gsi: Vec<u32> = lines.iter().filter(|l| l.owner == owner).filter_map(|l| l.gsi).collect();
    lines.retain(|l| l.owner != owner);
    let still: Vec<u8> = lines.iter().filter_map(|l| l.legacy).collect();
    let still_gsi: Vec<u32> = lines.iter().filter_map(|l| l.gsi).collect();
    drop(lines);
    for gsi in freed_gsi {
        if !still_gsi.contains(&gsi) {
            mask_gsi(gsi);
        }
    }
    for line in freed.into_iter().flatten() {
        if !still.contains(&line) {
            mask_legacy(line, true);
        }
    }
    crate::arch::without_interrupts(|| QUEUE.lock().retain(|job| job.owner != owner));
}

pub fn lines() -> Vec<(u8, String, u64, bool)> {
    LINES.lock().iter().map(|l| (l.vector, l.owner.clone(), count(l.vector), l.legacy.is_some())).collect()
}

pub fn schedule(owner: &str, work: Work, context: u64) -> bool {
    let owner = owner.to_string();
    crate::arch::without_interrupts(|| {
        let mut queue = QUEUE.lock();
        if queue.len() >= 256 {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        queue.push_back(Job { owner, work, context });
        true
    })
}

pub fn run_work() {
    if STORMED.load(Ordering::Relaxed) != REPORTED.load(Ordering::Relaxed) || STORMED_GSI.load(Ordering::Relaxed) != REPORTED_GSI.load(Ordering::Relaxed) {
        report_storms();
    }
    loop {
        let job = crate::arch::without_interrupts(|| QUEUE.lock().pop_front());
        let Some(job) = job else {
            return;
        };
        (job.work)(job.context as *mut c_void);
        WORKED.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn eoi(_vector: u8) {}

#[cfg(target_arch = "x86_64")]
fn eoi(vector: u8) {
    if vector >= LEGACY_BASE && vector < LEGACY_BASE + 16 {
        let line = vector - LEGACY_BASE;
        if line >= 8 {
            outb(0xA0, 0x20);
        }
        outb(0x20, 0x20);
    } else {
        lapic::eoi();
    }
}

pub fn dispatch(vector: u8) {
    COUNTS[vector as usize].fetch_add(1, Ordering::Relaxed);
    let mut served = false;
    ACTIVE.store(true, Ordering::Relaxed);
    let start = crate::task::uptime_ms();
    let taken = LINES.try_lock();
    if let Some(lines) = taken {
        for line in lines.iter().filter(|l| l.vector == vector) {
            if (line.handler)(line.context as *mut c_void) == HANDLED {
                served = true;
            }
        }
    }
    let spent = crate::task::uptime_ms().saturating_sub(start);
    ACTIVE.store(false, Ordering::Relaxed);
    if served {
        UNHANDLED[vector as usize].store(0, Ordering::Relaxed);
    } else {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        let streak = UNHANDLED[vector as usize].fetch_add(1, Ordering::Relaxed) + 1;
        if streak >= STORM_LIMIT && !(vector >= LEGACY_BASE && vector < LEGACY_BASE + 16) {
            let gsi = LINES.try_lock().and_then(|l| l.iter().find(|l| l.vector == vector).and_then(|l| l.gsi));
            #[cfg(target_arch = "x86_64")]
            if let Some(gsi) = gsi.filter(|g| *g < 64) {
                if crate::arch::x86_64::ioapic::mask_from_interrupt(gsi) {
                    STORMED_GSI.fetch_or(1 << gsi, Ordering::Relaxed);
                    UNHANDLED[vector as usize].store(0, Ordering::Relaxed);
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            let _ = gsi;
        }
        if streak >= STORM_LIMIT && vector >= LEGACY_BASE && vector < LEGACY_BASE + 16 {
            let line = vector - LEGACY_BASE;
            if !(cfg!(target_arch = "x86_64") && (line == 0 || line == 2)) {
                mask_legacy(line, true);
                STORMED.fetch_or(1 << line, Ordering::Relaxed);
                UNHANDLED[vector as usize].store(0, Ordering::Relaxed);
            }
        }
    }
    if spent >= STALL_MS {
        let owner = LINES.try_lock().and_then(|l| l.iter().find(|l| l.vector == vector).map(|l| l.owner.clone()));
        if let Some(owner) = owner {
            crate::module::mark_stalled(&owner, spent);
        }
    }
    eoi(vector);
}
