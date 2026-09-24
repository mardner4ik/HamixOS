use core::sync::atomic::{AtomicBool, Ordering};

const EXT_BASE: usize = 0x10;
const EXT_TIME: usize = 0x5449_4D45;
const EXT_LEGACY_SET_TIMER: usize = 0x00;

static HAS_TIME: AtomicBool = AtomicBool::new(false);

fn call(eid: usize, fid: usize, arg0: usize) -> (isize, usize) {
    let (error, value): (isize, usize);
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") arg0 => error,
            lateout("a1") value,
            in("a6") fid,
            in("a7") eid,
            options(nostack),
        );
    }
    (error, value)
}

pub fn probe(eid: usize) -> bool {
    let (error, value) = call(EXT_BASE, 3, eid);
    error == 0 && value != 0
}

pub fn spec_version() -> (usize, usize) {
    let (_, value) = call(EXT_BASE, 0, 0);
    ((value >> 24) & 0x7F, value & 0xFF_FFFF)
}

pub fn implementation() -> usize {
    call(EXT_BASE, 1, 0).1
}

pub fn init() {
    HAS_TIME.store(probe(EXT_TIME), Ordering::Relaxed);
}

pub fn set_timer(when: u64) {
    if HAS_TIME.load(Ordering::Relaxed) {
        call(EXT_TIME, 0, when as usize);
    } else {
        call(EXT_LEGACY_SET_TIMER, 0, when as usize);
    }
}

const EXT_SRST: usize = 0x5352_5354;

pub fn system_reset(kind: usize) {
    if probe(EXT_SRST) {
        let _ = call2(EXT_SRST, 0, kind, 0);
    }
    call(0x08, 0, 0);
}

fn call2(eid: usize, fid: usize, arg0: usize, arg1: usize) -> (isize, usize) {
    let (error, value): (isize, usize);
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") arg0 => error,
            inlateout("a1") arg1 => value,
            in("a6") fid,
            in("a7") eid,
            options(nostack),
        );
    }
    (error, value)
}
