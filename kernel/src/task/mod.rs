use spin::Mutex;

pub mod elf;
pub mod usermode;

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
    let count = {
        let mut t = TICKS.lock();
        *t = t.wrapping_add(1);
        *t
    };
    if count % 2 == 0 {
        crate::drivers::usb::poll();
    }
}

pub fn uptime_ticks() -> u64 {
    // See keyboard::read_key()'s guard for why: timer_handler (100Hz,
    // interrupts hard-disabled for its whole body) also locks TICKS, so
    // any other caller running with interrupts on -- e.g. sys_clock_gettime,
    // now reachable from inside a syscall thanks to syscall_entry's sti --
    // must not hold this lock while interruptible, or a tick landing mid
    // critical-section deadlocks the machine forever.
    crate::arch::x86_64::without_interrupts(|| *TICKS.lock())
}
