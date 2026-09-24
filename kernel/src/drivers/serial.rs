use core::fmt;
use spin::Mutex;
#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::{inb, outb};

const COM1: u16 = 0x3F8;

pub struct SerialPort {
    port: u16,
}

impl SerialPort {
    pub const fn new(port: u16) -> Self {
        Self { port }
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub fn init(&self) {
        let _ = self.port;
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub fn write_byte(&self, byte: u8) {
        crate::early::console::putc(byte);
    }

    #[cfg(target_arch = "x86_64")]
    pub fn init(&self) {
        outb(self.port + 1, 0x00);
        outb(self.port + 3, 0x80);
        outb(self.port + 0, 0x03);
        outb(self.port + 1, 0x00);
        outb(self.port + 3, 0x03);
        outb(self.port + 2, 0xC7);
        outb(self.port + 4, 0x0B);
    }

    #[cfg(target_arch = "x86_64")]
    fn is_transmit_empty(&self) -> bool {
        inb(self.port + 5) & 0x20 != 0
    }

    #[cfg(target_arch = "x86_64")]
    pub fn write_byte(&self, byte: u8) {
        while !self.is_transmit_empty() {}
        outb(self.port, byte);
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' && !cfg!(target_arch = "x86_64") {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

pub static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1));

pub fn init() {
    SERIAL.lock().init();
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            $crate::arch::without_interrupts(|| {
                let _ = write!($crate::drivers::serial::SERIAL.lock(), $($arg)*);
            });
        }
    };
}

#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            $crate::arch::without_interrupts(|| {
                let _ = writeln!($crate::drivers::serial::SERIAL.lock(), $($arg)*);
            });
        }
    };
}

#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        {
            if $crate::drivers::video::text_mode::serial_mirror() {
                $crate::drivers::klog::record(&alloc::format!($($arg)*));
            } else {
                $crate::serial_println!($($arg)*);
            }
        }
    };
}
