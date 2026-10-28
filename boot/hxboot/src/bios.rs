#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
    pub ebp: u32,
    pub ds: u32,
    pub es: u32,
    pub eflags: u32,
}

impl Regs {
    pub fn carry(&self) -> bool {
        self.eflags & 1 != 0
    }

    pub fn zero(&self) -> bool {
        self.eflags & 0x40 != 0
    }
}

unsafe extern "C" {
    fn bios_int(number: u32, regs: *mut Regs);
    fn port_out8(port: u32, value: u32);
    fn port_in8(port: u32) -> u32;
}

pub fn int(number: u8, regs: &mut Regs) {
    unsafe { bios_int(number as u32, regs) }
}

pub fn segment(address: u32) -> u32 {
    address >> 4
}

pub fn offset(address: u32) -> u32 {
    address & 0xf
}

pub fn far_to_linear(far: u32) -> u32 {
    ((far >> 16) << 4) + (far & 0xffff)
}

pub fn outb(port: u16, value: u8) {
    unsafe { port_out8(port as u32, value as u32) }
}

pub fn inb(port: u16) -> u8 {
    unsafe { port_in8(port as u32) as u8 }
}

pub fn read_u8(address: u32) -> u8 {
    unsafe { core::ptr::read_volatile(address as *const u8) }
}

pub fn read_u16(address: u32) -> u16 {
    unsafe { core::ptr::read_unaligned(address as *const u16) }
}

pub fn read_u32(address: u32) -> u32 {
    unsafe { core::ptr::read_unaligned(address as *const u32) }
}

pub fn read_u64(address: u32) -> u64 {
    unsafe { core::ptr::read_unaligned(address as *const u64) }
}

pub fn write_u16(address: u32, value: u16) {
    unsafe { core::ptr::write_unaligned(address as *mut u16, value) }
}

pub fn write_u32(address: u32, value: u32) {
    unsafe { core::ptr::write_unaligned(address as *mut u32, value) }
}

pub fn bytes(address: u32, len: usize) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(address as *const u8, len) }
}

pub fn bytes_mut(address: u32, len: usize) -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(address as *mut u8, len) }
}

pub fn wait_ms(ms: u32) {
    let us = ms * 1000;
    let mut r = Regs { eax: 0x8600, ecx: us >> 16, edx: us & 0xffff, ..Regs::default() };
    int(0x15, &mut r);
}

pub fn key_ready() -> Option<(u8, u8)> {
    let mut r = Regs { eax: 0x0100, ..Regs::default() };
    int(0x16, &mut r);
    if r.zero() {
        return None;
    }
    let mut r = Regs { eax: 0x0000, ..Regs::default() };
    int(0x16, &mut r);
    Some(((r.eax >> 8) as u8, r.eax as u8))
}

pub fn shift_held() -> bool {
    read_u8(0x417) & 0x03 != 0
}
