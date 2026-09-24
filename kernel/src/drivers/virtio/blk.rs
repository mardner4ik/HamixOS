use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::{Found, Queue, Transport};
use crate::net::dma::DmaRegion;

const REQ_IN: u32 = 0;
const REQ_OUT: u32 = 1;
const REQ_FLUSH: u32 = 4;
const F_FLUSH: u64 = 1 << 9;
const CHUNK: usize = 64 * 1024;

pub struct Device {
    transport: Box<dyn Transport>,
    queue: Queue,
    header: DmaRegion,
    data: DmaRegion,
    pub sectors: u64,
    pub location: String,
    flush: bool,
}

static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());

fn attach(found: Found) -> Option<Device> {
    let mut transport = found.transport;
    let accepted = super::negotiate(transport.as_mut(), F_FLUSH)?;
    let queue = Queue::new(transport.as_mut(), 0, 64)?;
    super::finish(transport.as_mut());
    let sectors = transport.config64(0);
    Some(Device { transport, queue, header: DmaRegion::new(4096)?, data: DmaRegion::new(CHUNK)?, sectors, location: found.location, flush: accepted & F_FLUSH != 0 })
}

pub fn init() -> Vec<(usize, u64, String)> {
    let mut devices = DEVICES.lock();
    for found in super::take(super::ID_BLOCK) {
        if let Some(device) = attach(found) {
            devices.push(device);
        }
    }
    devices.iter().enumerate().map(|(i, d)| (i, d.sectors, d.location.clone())).collect()
}

impl Device {
    fn request(&mut self, kind: u32, sector: u64, len: usize) -> Result<(), ()> {
        let header = self.header.slice(0, 32);
        header[0..4].copy_from_slice(&kind.to_le_bytes());
        header[4..8].fill(0);
        header[8..16].copy_from_slice(&sector.to_le_bytes());
        header[16] = 0xFF;
        let head = self.header.phys;
        let status = self.header.phys + 16;
        let parts: Vec<(u64, u32, bool)> = if kind == REQ_FLUSH {
            alloc::vec![(head, 16, false), (status, 1, true)]
        } else {
            alloc::vec![(head, 16, false), (self.data.phys, len as u32, kind == REQ_IN), (status, 1, true)]
        };
        self.queue.push(&parts).ok_or(())?;
        self.queue.kick(self.transport.as_ref());
        self.queue.wait(self.transport.as_ref(), 5000).ok_or(())?;
        if self.header.slice(16, 1)[0] == 0 { Ok(()) } else { Err(()) }
    }
}

pub fn read(index: usize, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
    let mut devices = DEVICES.lock();
    let device = devices.get_mut(index).ok_or(())?;
    let mut done = 0usize;
    while done < buf.len() {
        let n = (buf.len() - done).min(CHUNK);
        device.request(REQ_IN, lba + (done / 512) as u64, n)?;
        buf[done..done + n].copy_from_slice(device.data.slice(0, n));
        done += n;
    }
    Ok(())
}

pub fn write(index: usize, lba: u64, buf: &[u8]) -> Result<(), ()> {
    let mut devices = DEVICES.lock();
    let device = devices.get_mut(index).ok_or(())?;
    let mut done = 0usize;
    while done < buf.len() {
        let n = (buf.len() - done).min(CHUNK);
        device.data.slice(0, n).copy_from_slice(&buf[done..done + n]);
        device.request(REQ_OUT, lba + (done / 512) as u64, n)?;
        done += n;
    }
    Ok(())
}

pub fn flush(index: usize) -> Result<(), ()> {
    let mut devices = DEVICES.lock();
    let device = devices.get_mut(index).ok_or(())?;
    if !device.flush {
        return Ok(());
    }
    device.request(REQ_FLUSH, 0, 0)
}
