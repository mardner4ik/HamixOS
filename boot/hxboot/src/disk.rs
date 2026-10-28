use crate::bios::{self, Regs};
use crate::layout::{BOUNCE, BOUNCE_SIZE, LOW};

pub struct Disk {
    drive: u8,
    shift: u32,
}

const DAP: u32 = LOW;
const PARAMS: u32 = LOW + 0x40;

impl Disk {
    pub fn new(drive: u8, shift: u32) -> Disk {
        let mut shift = if shift == 9 || shift == 11 { shift } else { 11 };
        bios::bytes_mut(PARAMS, 0x42).fill(0);
        bios::write_u16(PARAMS, 0x42);
        let mut r = Regs { eax: 0x4800, edx: drive as u32, esi: bios::offset(PARAMS), ds: bios::segment(PARAMS), ..Regs::default() };
        bios::int(0x13, &mut r);
        if !r.carry() {
            match bios::read_u16(PARAMS + 0x18) {
                512 => shift = 9,
                2048 => shift = 11,
                _ => {}
            }
        }
        Disk { drive, shift }
    }

    pub fn drive(&self) -> u8 {
        self.drive
    }

    pub fn sector_size(&self) -> u32 {
        1 << self.shift
    }

    fn read_sectors(&self, lba: u64, count: u32) -> Result<(), &'static str> {
        for attempt in 0..4 {
            bios::bytes_mut(DAP, 16).fill(0);
            bios::write_u16(DAP, 16);
            bios::write_u16(DAP + 2, count as u16);
            bios::write_u16(DAP + 4, bios::offset(BOUNCE) as u16);
            bios::write_u16(DAP + 6, bios::segment(BOUNCE) as u16);
            bios::write_u32(DAP + 8, lba as u32);
            bios::write_u32(DAP + 12, (lba >> 32) as u32);
            let mut r = Regs { eax: 0x4200, edx: self.drive as u32, esi: bios::offset(DAP), ds: bios::segment(DAP), ..Regs::default() };
            bios::int(0x13, &mut r);
            if !r.carry() {
                return Ok(());
            }
            if attempt < 3 {
                let mut reset = Regs { eax: 0, edx: self.drive as u32, ..Regs::default() };
                bios::int(0x13, &mut reset);
            }
        }
        Err("the BIOS could not read the boot medium")
    }

    pub fn read(&self, offset: u64, dst: *mut u8, len: usize) -> Result<(), &'static str> {
        let size = self.sector_size() as u64;
        let per = (BOUNCE_SIZE as u64 / size).min(127);
        let mut done = 0usize;
        while done < len {
            let at = offset + done as u64;
            let lba = at >> self.shift;
            let within = (at & (size - 1)) as usize;
            let wanted = len - done;
            let count = ((within + wanted) as u64).div_ceil(size).min(per);
            self.read_sectors(lba, count as u32)?;
            let n = (count as usize * size as usize - within).min(wanted);
            unsafe { core::ptr::copy_nonoverlapping((BOUNCE as usize + within) as *const u8, dst.add(done), n) };
            done += n;
        }
        Ok(())
    }
}
