use spin::Mutex;

static TICKS: Mutex<u64> = Mutex::new(0);

pub fn init() {
    setup_pit_100hz();
}

fn setup_pit_100hz() {
    use crate::arch::x86_64::outb;
    let divisor: u16 = 11932u16;
    outb(0x43, 0x36);
    outb(0x40, (divisor & 0xFF) as u8);
    outb(0x40, (divisor >> 8) as u8);
}

pub fn tick() {
    let mut t = TICKS.lock();
    *t = t.wrapping_add(1);
}

pub fn uptime_ticks() -> u64 {
    *TICKS.lock()
}
