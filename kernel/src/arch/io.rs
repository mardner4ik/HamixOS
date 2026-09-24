#[cfg(target_arch = "x86_64")]
pub use super::x86_64::{inb, inl, inw, outb, outl, outw};

#[cfg(not(target_arch = "x86_64"))]
mod none {
    #[inline]
    pub fn inb(_port: u16) -> u8 {
        0xFF
    }

    #[inline]
    pub fn inw(_port: u16) -> u16 {
        0xFFFF
    }

    #[inline]
    pub fn inl(_port: u16) -> u32 {
        0xFFFF_FFFF
    }

    #[inline]
    pub fn outb(_port: u16, _value: u8) {}

    #[inline]
    pub fn outw(_port: u16, _value: u16) {}

    #[inline]
    pub fn outl(_port: u16, _value: u32) {}
}

#[cfg(not(target_arch = "x86_64"))]
pub use none::*;

pub const PORTS: bool = cfg!(target_arch = "x86_64");
