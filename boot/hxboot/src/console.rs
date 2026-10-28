use core::fmt::{self, Write};

use crate::bios;

const VGA: u32 = 0xb8000;
const COLS: u32 = 80;
const ROWS: u32 = 25;
const COM1: u16 = 0x3f8;

static mut ROW: u32 = 0;
static mut COL: u32 = 0;
static mut COLOR: u8 = 0x07;
static mut TEXT: bool = true;

pub fn init() {
    bios::outb(COM1 + 1, 0x00);
    bios::outb(COM1 + 3, 0x80);
    bios::outb(COM1, 0x01);
    bios::outb(COM1 + 1, 0x00);
    bios::outb(COM1 + 3, 0x03);
    bios::outb(COM1 + 2, 0xc7);
    bios::outb(COM1 + 4, 0x03);
    hide_cursor();
    clear();
}

fn hide_cursor() {
    let mut r = bios::Regs { eax: 0x0100, ecx: 0x2000, ..bios::Regs::default() };
    bios::int(0x10, &mut r);
}

pub fn banner() {
    set_color(0x1f);
    for c in 0..COLS {
        put_at(0, c, b' ', 0x1f);
    }
    unsafe {
        ROW = 0;
        COL = 1;
    }
    print_str("HamixOS boot loader (hxboot)");
    set_color(0x07);
    unsafe {
        ROW = 2;
        COL = 0;
    }
}

pub fn graphics() {
    unsafe { TEXT = false };
}

pub fn set_color(color: u8) {
    unsafe { COLOR = color };
}

pub fn clear() {
    if !unsafe { TEXT } {
        return;
    }
    for r in 0..ROWS {
        for c in 0..COLS {
            put_at(r, c, b' ', 0x07);
        }
    }
    unsafe {
        ROW = 0;
        COL = 0;
    }
}

pub fn put_at(row: u32, col: u32, byte: u8, color: u8) {
    if !unsafe { TEXT } || row >= ROWS || col >= COLS {
        return;
    }
    let cell = VGA + (row * COLS + col) * 2;
    unsafe { core::ptr::write_volatile(cell as *mut u16, byte as u16 | (color as u16) << 8) };
}

pub fn write_at(row: u32, col: u32, text: &str, color: u8) {
    let mut c = col;
    for byte in text.bytes() {
        put_at(row, c, printable(byte), color);
        c += 1;
    }
}

fn printable(byte: u8) -> u8 {
    if (0x20..0x7f).contains(&byte) { byte } else { b'?' }
}

fn serial(byte: u8) {
    let mut spins = 0;
    while bios::inb(COM1 + 5) & 0x20 == 0 && spins < 100000 {
        spins += 1;
    }
    bios::outb(COM1, byte);
}

fn scroll() {
    for r in 1..ROWS {
        for c in 0..COLS {
            let from = VGA + (r * COLS + c) * 2;
            let to = VGA + ((r - 1) * COLS + c) * 2;
            unsafe { core::ptr::write_volatile(to as *mut u16, core::ptr::read_volatile(from as *const u16)) };
        }
    }
    for c in 0..COLS {
        put_at(ROWS - 1, c, b' ', 0x07);
    }
}

fn put(byte: u8) {
    if byte == b'\n' {
        serial(b'\r');
    }
    serial(byte);
    unsafe {
        if byte == b'\n' {
            COL = 0;
            ROW += 1;
        } else {
            put_at(ROW, COL, printable(byte), COLOR);
            COL += 1;
            if COL >= COLS {
                COL = 0;
                ROW += 1;
            }
        }
        if ROW >= ROWS {
            if TEXT {
                scroll();
            }
            ROW = ROWS - 1;
        }
    }
}

pub fn serial_line(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' {
            serial(b'\r');
        }
        serial(byte);
    }
    serial(b'\r');
    serial(b'\n');
}

pub fn print_str(text: &str) {
    for byte in text.bytes() {
        put(byte);
    }
}

pub struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        print_str(s);
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    let _ = Console.write_fmt(args);
}

pub fn error(reason: &str) {
    set_color(0x4f);
    print_str("\nhxboot: ");
    print_str(reason);
    print_str("\nThe computer can be switched off. Try another USB port, write the stick again or boot from a DVD.\n");
    set_color(0x07);
}

#[macro_export]
macro_rules! println {
    () => ($crate::console::print_str("\n"));
    ($($arg:tt)*) => ({
        $crate::console::print(format_args!($($arg)*));
        $crate::console::print_str("\n");
    });
}
