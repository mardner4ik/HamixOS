use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const LINES: usize = 1024;

static HANDLERS: [AtomicUsize; LINES] = [const { AtomicUsize::new(0) }; LINES];
static COUNTS: [AtomicU64; LINES] = [const { AtomicU64::new(0) }; LINES];
static SPURIOUS: AtomicU64 = AtomicU64::new(0);

pub fn register(irq: u32, handler: fn(u32)) -> bool {
    let Some(slot) = HANDLERS.get(irq as usize) else {
        return false;
    };
    slot.store(handler as usize, Ordering::Release);
    true
}

pub fn dispatch(irq: u32) -> bool {
    let Some(slot) = HANDLERS.get(irq as usize) else {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        return false;
    };
    let handler = slot.load(Ordering::Acquire);
    if handler == 0 {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    COUNTS[irq as usize].fetch_add(1, Ordering::Relaxed);
    let handler: fn(u32) = unsafe { core::mem::transmute(handler) };
    handler(irq);
    true
}

pub fn count(irq: u32) -> u64 {
    COUNTS.get(irq as usize).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
}

pub fn spurious() -> u64 {
    SPURIOUS.load(Ordering::Relaxed)
}
