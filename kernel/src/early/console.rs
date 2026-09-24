use core::fmt::{self, Write};
use core::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};

use crate::fdt::Fdt;

const PL011: u8 = 0;
const NS16550: u8 = 1;

#[cfg(target_arch = "aarch64")]
const DEFAULT: (usize, u8) = (0x0900_0000, PL011);
#[cfg(target_arch = "riscv64")]
const DEFAULT: (usize, u8) = (0x1000_0000, NS16550);

static BASE: AtomicUsize = AtomicUsize::new(DEFAULT.0);
static KIND: AtomicU8 = AtomicU8::new(DEFAULT.1);
static SHIFT: AtomicU8 = AtomicU8::new(0);
static WIDTH: AtomicU8 = AtomicU8::new(1);
static IRQ: AtomicU32 = AtomicU32::new(0);

const RING: usize = 256;
static INPUT: [AtomicU8; RING] = [const { AtomicU8::new(0) }; RING];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

const PL011_FR_RXFE: u32 = 1 << 4;
const PL011_IMSC: usize = 0x38;
const PL011_ICR: usize = 0x44;
const PL011_RX_INTERRUPTS: u32 = (1 << 4) | (1 << 6);

const PL011_CR: usize = 0x30;
const PL011_ENABLE: u32 = (1 << 0) | (1 << 8) | (1 << 9);

pub fn init() {
    if KIND.load(Ordering::Relaxed) != PL011 {
        return;
    }
    let control = (BASE.load(Ordering::Relaxed) + PL011_CR) as *mut u32;
    unsafe {
        let value = core::ptr::read_volatile(control);
        if value & PL011_ENABLE != PL011_ENABLE {
            core::ptr::write_volatile(control, value | PL011_ENABLE);
        }
    }
}

pub fn putc(byte: u8) {
    let base = BASE.load(Ordering::Relaxed);
    unsafe {
        if KIND.load(Ordering::Relaxed) == PL011 {
            while core::ptr::read_volatile((base + 0x18) as *const u32) & (1 << 5) != 0 {
                core::hint::spin_loop();
            }
            core::ptr::write_volatile(base as *mut u32, byte as u32);
            return;
        }
        let shift = SHIFT.load(Ordering::Relaxed) as usize;
        let lsr = base + (5 << shift);
        if WIDTH.load(Ordering::Relaxed) == 4 {
            while core::ptr::read_volatile(lsr as *const u32) & 0x20 == 0 {
                core::hint::spin_loop();
            }
            core::ptr::write_volatile(base as *mut u32, byte as u32);
        } else {
            while core::ptr::read_volatile(lsr as *const u8) & 0x20 == 0 {
                core::hint::spin_loop();
            }
            core::ptr::write_volatile(base as *mut u8, byte);
        }
    }
}

struct Console;

impl Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if byte == b'\n' {
                putc(b'\r');
            }
            putc(byte);
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    let _ = Console.write_fmt(args);
}

pub fn adopt<'a>(fdt: &Fdt<'a>) -> Option<&'a str> {
    let stdout = fdt.chosen().and_then(|c| c.str_property("stdout-path").or_else(|| c.str_property("linux,stdout-path")));
    let node = stdout
        .and_then(|path| fdt.resolve(path.split(':').next().unwrap_or(path)))
        .or_else(|| fdt.find_compatible("arm,pl011"))
        .or_else(|| fdt.find_compatible("ns16550a"))?;
    let kind = if node.compatible_with("arm,pl011") {
        PL011
    } else if node.compatible_with("ns16550a") || node.compatible_with("ns16550") || node.compatible_with("snps,dw-apb-uart") {
        NS16550
    } else {
        return None;
    };
    let (base, _) = node.reg().next()?;
    SHIFT.store(node.u32_property("reg-shift").unwrap_or(0) as u8, Ordering::Relaxed);
    WIDTH.store(node.u32_property("reg-io-width").unwrap_or(1) as u8, Ordering::Relaxed);
    KIND.store(kind, Ordering::Relaxed);
    BASE.store(base as usize, Ordering::Relaxed);
    IRQ.store(crate::arch::irq::interrupt_of(&node).unwrap_or(0), Ordering::Relaxed);
    init();
    Some(node.name)
}

fn register(offset: usize) -> usize {
    BASE.load(Ordering::Relaxed) + (offset << SHIFT.load(Ordering::Relaxed))
}

static FORWARD: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn forward_to_keyboard() {
    FORWARD.store(true, Ordering::Release);
    while let Some(byte) = take_input() {
        deliver(byte);
    }
}

static ESCAPE: AtomicU8 = AtomicU8::new(0);

const LETTERS: &[u8; 26] = &[0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C];

fn press(codes: &[u8]) {
    use crate::drivers::input::keyboard::inject_scancode;
    for &code in codes {
        inject_scancode(code);
    }
}

fn deliver(byte: u8) {
    let state = ESCAPE.load(Ordering::Relaxed);
    if state == 1 {
        ESCAPE.store(if byte == b'[' || byte == b'O' { 2 } else { 0 }, Ordering::Relaxed);
        if byte != b'[' && byte != b'O' {
            press(&[0x01, 0x81]);
        }
        return;
    }
    if state == 2 || state == 3 {
        ESCAPE.store(0, Ordering::Relaxed);
        let code = match byte {
            b'A' => 0x48,
            b'B' => 0x50,
            b'C' => 0x4D,
            b'D' => 0x4B,
            b'H' => 0x47,
            b'F' => 0x4F,
            b'3' => {
                ESCAPE.store(4, Ordering::Relaxed);
                return;
            }
            _ => return,
        };
        press(&[0xE0, code, 0xE0, code | 0x80]);
        return;
    }
    if state == 4 {
        ESCAPE.store(0, Ordering::Relaxed);
        press(&[0xE0, 0x53, 0xE0, 0xD3]);
        return;
    }
    match byte {
        0x1B => {
            ESCAPE.store(1, Ordering::Relaxed);
            return;
        }
        0x7F | 0x08 => press(&[0x0E, 0x8E]),
        b'\r' | b'\n' => press(&[0x1C, 0x9C]),
        b'\t' => press(&[0x0F, 0x8F]),
        0x01..=0x1A => {
            let letter = LETTERS[(byte - 1) as usize];
            press(&[0x1D, letter, letter | 0x80, 0x9D]);
        }
        _ => {
            let mut text = [0u8; 4];
            crate::drivers::input::keyboard::push_text((byte as char).encode_utf8(&mut text));
        }
    }
    crate::task::notify_input();
}

fn push(byte: u8) {
    if FORWARD.load(Ordering::Acquire) {
        deliver(byte);
        return;
    }
    let head = HEAD.load(Ordering::Relaxed);
    if head.wrapping_sub(TAIL.load(Ordering::Acquire)) < RING {
        INPUT[head % RING].store(byte, Ordering::Relaxed);
        HEAD.store(head.wrapping_add(1), Ordering::Release);
    }
}

pub fn take_input() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let byte = INPUT[tail % RING].load(Ordering::Relaxed);
    TAIL.store(tail.wrapping_add(1), Ordering::Release);
    Some(byte)
}

fn on_receive(_: u32) {
    let base = BASE.load(Ordering::Relaxed);
    unsafe {
        if KIND.load(Ordering::Relaxed) == PL011 {
            while core::ptr::read_volatile((base + 0x18) as *const u32) & PL011_FR_RXFE == 0 {
                push(core::ptr::read_volatile(base as *const u32) as u8);
            }
            core::ptr::write_volatile((base + PL011_ICR) as *mut u32, 0x7FF);
        } else {
            while core::ptr::read_volatile(register(5) as *const u8) & 1 != 0 {
                push(core::ptr::read_volatile(base as *const u8));
            }
        }
    }
}

pub fn enable_receive_interrupt() -> Option<u32> {
    let irq = IRQ.load(Ordering::Relaxed);
    if irq == 0 {
        return None;
    }
    crate::arch::irqtab::register(irq, on_receive);
    let base = BASE.load(Ordering::Relaxed);
    unsafe {
        if KIND.load(Ordering::Relaxed) == PL011 {
            let mask = (base + PL011_IMSC) as *mut u32;
            core::ptr::write_volatile(mask, core::ptr::read_volatile(mask) | PL011_RX_INTERRUPTS);
        } else {
            core::ptr::write_volatile(register(1) as *mut u8, 1);
        }
    }
    crate::arch::irq::enable(irq);
    Some(irq)
}

macro_rules! kprintln {
    () => {
        $crate::early::console::print(format_args!("\n"))
    };
    ($($arg:tt)*) => {{
        $crate::early::console::print(format_args!($($arg)*));
        $crate::early::console::print(format_args!("\n"));
    }};
}

pub(crate) use kprintln;
