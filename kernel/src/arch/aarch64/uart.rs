const UART0_DR: *mut u8 = 0x0900_0000 as *mut u8;

pub fn putc(c: u8) {
    unsafe {
        core::ptr::write_volatile(UART0_DR, c);
    }
}

pub fn puts(s: &str) {
    for b in s.bytes() {
        if b == b'\n' {
            putc(b'\r');
        }
        putc(b);
    }
}
