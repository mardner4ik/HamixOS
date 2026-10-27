use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::net::unix::{self, Cred, SOCK_DGRAM};

pub const AF_NETLINK: u64 = 16;
const NETLINK_ROUTE: u64 = 0;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_MULTI: u16 = 2;
const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_GETADDR: u16 = 22;
const RTM_GETROUTE: u16 = 26;

static SOCKETS: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());
static PASSCRED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub fn create(protocol: u64, cred: Cred) -> Result<u32, i64> {
    if protocol != NETLINK_ROUTE {
        return Err(-93);
    }
    let stale: Vec<u32> = without_interrupts(|| {
        let mut table = SOCKETS.lock();
        let dead: Vec<(u32, u32)> = table.iter().filter(|(user, _)| unix::kind(**user).is_none()).map(|(u, k)| (*u, *k)).collect();
        for (user, _) in dead.iter() {
            table.remove(user);
        }
        PASSCRED.lock().retain(|i| !dead.iter().any(|(u, _)| u == i));
        dead.into_iter().map(|(_, k)| k).collect()
    });
    for kernel in stale {
        unix::release(kernel);
    }
    let (user, kernel) = unix::pair(SOCK_DGRAM, cred);
    without_interrupts(|| SOCKETS.lock().insert(user, kernel));
    Ok(user)
}

pub fn set_passcred(id: u32, on: bool) {
    without_interrupts(|| {
        let mut list = PASSCRED.lock();
        list.retain(|i| *i != id);
        if on {
            list.push(id);
        }
    });
}

pub fn passcred(id: u32) -> bool {
    without_interrupts(|| PASSCRED.lock().contains(&id))
}

pub fn is_netlink(id: u32) -> bool {
    without_interrupts(|| SOCKETS.lock().contains_key(&id))
}

fn peer(id: u32) -> Option<u32> {
    without_interrupts(|| SOCKETS.lock().get(&id).copied())
}

fn align(n: usize) -> usize {
    (n + 3) & !3
}

struct Builder {
    buf: Vec<u8>,
    start: usize,
}

impl Builder {
    fn begin(&mut self, kind: u16, flags: u16, seq: u32, pid: u32) {
        self.start = self.buf.len();
        self.buf.extend_from_slice(&0u32.to_le_bytes());
        self.buf.extend_from_slice(&kind.to_le_bytes());
        self.buf.extend_from_slice(&flags.to_le_bytes());
        self.buf.extend_from_slice(&seq.to_le_bytes());
        self.buf.extend_from_slice(&pid.to_le_bytes());
    }

    fn bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    fn attr(&mut self, kind: u16, data: &[u8]) {
        let len = 4 + data.len();
        self.buf.extend_from_slice(&(len as u16).to_le_bytes());
        self.buf.extend_from_slice(&kind.to_le_bytes());
        self.buf.extend_from_slice(data);
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
    }

    fn end(&mut self) {
        let len = (self.buf.len() - self.start) as u32;
        self.buf[self.start..self.start + 4].copy_from_slice(&len.to_le_bytes());
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
    }
}

fn links(b: &mut Builder, seq: u32, pid: u32) {
    for iface in crate::net::interfaces() {
        b.begin(RTM_NEWLINK, NLM_F_MULTI, seq, pid);
        let mut flags: u32 = if iface.up { 0x1 | 0x40 | 0x10000 } else { 0x1 };
        flags |= if iface.loopback { 0x8 } else { 0x2 | 0x1000 };
        b.bytes(&[0, 0]);
        b.bytes(&(if iface.loopback { 772u16 } else { 1u16 }).to_le_bytes());
        b.bytes(&(iface.index as i32).to_le_bytes());
        b.bytes(&flags.to_le_bytes());
        b.bytes(&0xFFFF_FFFFu32.to_le_bytes());
        let mut name = iface.name.clone().into_bytes();
        name.push(0);
        b.attr(3, &name);
        b.attr(4, &(if iface.loopback { 65536u32 } else { 1500u32 }).to_le_bytes());
        b.attr(1, &iface.mac);
        b.attr(2, &if iface.loopback { [0u8; 6] } else { [0xFF; 6] });
        let mut stats = [0u8; 24 * 4];
        stats[8..12].copy_from_slice(&(iface.rx as u32).to_le_bytes());
        stats[12..16].copy_from_slice(&(iface.tx as u32).to_le_bytes());
        b.attr(7, &stats);
        b.end();
    }
}

fn addresses(b: &mut Builder, family: u8, seq: u32, pid: u32) {
    if family != 0 && family != 2 {
        return;
    }
    for iface in crate::net::interfaces() {
        let Some((ip, prefix)) = iface.address else {
            continue;
        };
        b.begin(RTM_NEWADDR, NLM_F_MULTI, seq, pid);
        b.bytes(&[2, prefix, 0x80, if iface.loopback { 254 } else { 0 }]);
        b.bytes(&iface.index.to_le_bytes());
        b.attr(1, &ip);
        b.attr(2, &ip);
        if !iface.loopback {
            let mask: u32 = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix as u32) };
            let bcast = (u32::from_be_bytes(ip) | !mask).to_be_bytes();
            b.attr(4, &bcast);
        }
        let mut name = iface.name.clone().into_bytes();
        name.push(0);
        b.attr(3, &name);
        b.end();
    }
}

pub fn handle(id: u32, data: &[u8]) -> Result<usize, i64> {
    let Some(kernel) = peer(id) else {
        return Err(-9);
    };
    let pid = crate::task::current_pid();
    let mut offset = 0usize;
    let mut reply = Builder { buf: Vec::new(), start: 0 };
    while offset + 16 <= data.len() {
        let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        if len < 16 || offset + len > data.len() {
            break;
        }
        let kind = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let seq = u32::from_le_bytes(data[offset + 8..offset + 12].try_into().unwrap());
        let family = data.get(offset + 16).copied().unwrap_or(0);
        match kind {
            RTM_GETLINK => links(&mut reply, seq, pid),
            RTM_GETADDR => addresses(&mut reply, family, seq, pid),
            RTM_GETROUTE => {}
            _ => {
                reply.begin(NLMSG_ERROR, 0, seq, pid);
                reply.bytes(&(-95i32).to_le_bytes());
                reply.bytes(&data[offset..offset + 16]);
                reply.end();
                offset += align(len);
                continue;
            }
        }
        reply.begin(NLMSG_DONE, NLM_F_MULTI, seq, pid);
        reply.bytes(&0i32.to_le_bytes());
        reply.end();
        offset += align(len);
    }
    if !reply.buf.is_empty() {
        let mut files = Vec::new();
        let _ = unix::send(kernel, &reply.buf, &mut files, None);
    }
    Ok(data.len())
}

pub fn kernel_address(buf: &mut [u8]) -> usize {
    let mut raw = [0u8; 12];
    raw[0..2].copy_from_slice(&(AF_NETLINK as u16).to_le_bytes());
    let n = buf.len().min(12);
    buf[..n].copy_from_slice(&raw[..n]);
    12
}

pub fn address(buf: &mut [u8]) -> usize {
    let pid = crate::task::current_pid();
    let mut raw = [0u8; 12];
    raw[0..2].copy_from_slice(&(AF_NETLINK as u16).to_le_bytes());
    raw[4..8].copy_from_slice(&pid.to_le_bytes());
    let n = buf.len().min(12);
    buf[..n].copy_from_slice(&raw[..n]);
    12
}
