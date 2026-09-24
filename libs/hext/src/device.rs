use alloc::vec::Vec;

use crate::Error;

pub const SECTOR: usize = 512;

pub trait BlockDevice {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), Error>;
    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), Error>;
    fn flush(&mut self) -> Result<(), Error>;
    fn sectors(&self) -> u64;
}

pub struct MemDevice {
    base: *mut u8,
    len: usize,
}

unsafe impl Send for MemDevice {}

impl MemDevice {
    pub unsafe fn new(base: *mut u8, len: usize) -> Self {
        Self { base, len }
    }
}

impl BlockDevice for MemDevice {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), Error> {
        let start = sector as usize * SECTOR;
        if start + buf.len() > self.len {
            return Err(Error::Io);
        }
        unsafe { core::ptr::copy_nonoverlapping(self.base.add(start), buf.as_mut_ptr(), buf.len()) };
        Ok(())
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), Error> {
        let start = sector as usize * SECTOR;
        if start + buf.len() > self.len {
            return Err(Error::Io);
        }
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), self.base.add(start), buf.len()) };
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn sectors(&self) -> u64 {
        (self.len / SECTOR) as u64
    }
}

pub struct VecDevice {
    pub data: Vec<u8>,
}

impl VecDevice {
    pub fn new(size: usize) -> Self {
        Self { data: alloc::vec![0u8; size / SECTOR * SECTOR] }
    }
}

impl BlockDevice for VecDevice {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), Error> {
        let start = sector as usize * SECTOR;
        let end = start.checked_add(buf.len()).ok_or(Error::Io)?;
        if end > self.data.len() {
            return Err(Error::Io);
        }
        buf.copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), Error> {
        let start = sector as usize * SECTOR;
        let end = start.checked_add(buf.len()).ok_or(Error::Io)?;
        if end > self.data.len() {
            return Err(Error::Io);
        }
        self.data[start..end].copy_from_slice(buf);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn sectors(&self) -> u64 {
        (self.data.len() / SECTOR) as u64
    }
}
