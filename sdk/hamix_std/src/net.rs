use alloc::string::String;
use alloc::vec::Vec;

use crate::sys::{syscall3, syscall5};

pub const SYS_NET_STATUS: u64 = 9130;
pub const SYS_NET_SET_MODE: u64 = 9131;
pub const SYS_WIFI_SCAN: u64 = 9132;
pub const SYS_WIFI_CONNECT: u64 = 9133;
pub const SYS_WIFI_DISCONNECT: u64 = 9134;
pub const SYS_NET_CONFIGURE: u64 = 9135;
pub const SYS_WIFI_KNOWN: u64 = 9136;
pub const SYS_WIFI_FORGET: u64 = 9137;
pub const SYS_WIFI_AUTOJOIN: u64 = 9138;
pub const SYS_SOCKET: u64 = 9140;
pub const SYS_SOCKET_CONNECT: u64 = 9141;
pub const SYS_SOCKET_SEND: u64 = 9142;
pub const SYS_SOCKET_RECV: u64 = 9143;
pub const SYS_SOCKET_CLOSE: u64 = 9144;
pub const SYS_RESOLVE: u64 = 9145;
pub const SYS_PING: u64 = 9146;
pub const SYS_SOCKET_LISTEN: u64 = 9147;
pub const SYS_SOCKET_STATE: u64 = 9148;
pub const SYS_SOCKET_SENDTO: u64 = 9149;
pub const SYS_SOCKET_RECVFROM: u64 = 9150;

pub const MODE_OFF: u64 = 0;
pub const MODE_ETHERNET: u64 = 1;
pub const MODE_WIFI: u64 = 2;

pub const SOCK_TCP: u64 = 1;
pub const SOCK_UDP: u64 = 2;

#[derive(Clone, Default)]
pub struct Interface {
    pub name: String,
    pub kind: String,
    pub driver: String,
    pub mac: String,
    pub link: bool,
    pub address: String,
    pub gateway: String,
    pub dns: String,
    pub state: String,
    pub ssid: String,
    pub signal: u32,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

#[derive(Clone, Default)]
pub struct Status {
    pub available: bool,
    pub mode: String,
    pub interfaces: Vec<Interface>,
}

impl Status {
    pub fn active(&self) -> Option<&Interface> {
        let wanted = if self.mode == "wifi" { "wifi" } else { "ethernet" };
        self.interfaces.iter().find(|i| i.kind == wanted && i.link && !i.address.is_empty())
    }

    pub fn has_kind(&self, kind: &str) -> bool {
        self.interfaces.iter().any(|i| i.kind == kind)
    }

    pub fn connected(&self) -> bool {
        self.active().is_some()
    }
}

#[derive(Clone, Default)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub channel: u32,
    pub signal: u32,
    pub security: String,
    pub connected: bool,
}

impl WifiNetwork {
    pub fn secured(&self) -> bool {
        self.security != "open"
    }
}

fn read_text(num: u64, arg: u64) -> Option<String> {
    let mut size = 4096usize;
    loop {
        let mut buf = alloc::vec![0u8; size];
        let n = unsafe { syscall3(num, buf.as_mut_ptr() as u64, buf.len() as u64, arg) };
        if n < 0 {
            return None;
        }
        if (n as usize) <= size {
            buf.truncate(n as usize);
            return Some(String::from_utf8_lossy(&buf).into_owned());
        }
        size = n as usize;
    }
}

pub fn status() -> Status {
    let Some(text) = read_text(SYS_NET_STATUS, 0) else {
        return Status::default();
    };
    let mut status = Status { available: true, mode: String::from("off"), interfaces: Vec::new() };
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("mode") if f.len() > 1 => status.mode = String::from(f[1]),
            Some("iface") if f.len() >= 12 => status.interfaces.push(Interface {
                name: String::from(f[1]),
                kind: String::from(f[2]),
                driver: String::from(f[3]),
                mac: String::from(f[4]),
                link: f[5] == "up",
                address: String::from(f[6]),
                gateway: String::from(f[7]),
                dns: String::from(f[8]),
                state: String::from(f[9]),
                ssid: String::from(f[10]),
                signal: f[11].parse().unwrap_or(0),
                rx_packets: f.get(12).and_then(|v| v.parse().ok()).unwrap_or(0),
                tx_packets: f.get(13).and_then(|v| v.parse().ok()).unwrap_or(0),
            }),
            _ => {}
        }
    }
    status
}

pub fn set_mode(mode: u64) -> i64 {
    unsafe { syscall3(SYS_NET_SET_MODE, mode, 0, 0) }
}

pub fn wifi_scan(refresh: bool) -> Vec<WifiNetwork> {
    let Some(text) = read_text(SYS_WIFI_SCAN, refresh as u64) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 6 {
                return None;
            }
            Some(WifiNetwork {
                ssid: String::from(f[0]),
                bssid: String::from(f[1]),
                channel: f[2].parse().unwrap_or(0),
                signal: f[3].parse().unwrap_or(0),
                security: String::from(f[4]),
                connected: f[5] == "1",
            })
        })
        .collect()
}

pub fn wifi_connect(ssid: &str, passphrase: &str) -> i64 {
    unsafe { syscall5(SYS_WIFI_CONNECT, ssid.as_ptr() as u64, ssid.len() as u64, passphrase.as_ptr() as u64, passphrase.len() as u64, 0) }
}

#[derive(Clone, Default)]
pub struct SavedNetwork {
    pub ssid: String,
    pub automatic: bool,
    pub secured: bool,
    pub last_used: u64,
}

pub fn wifi_known() -> Vec<SavedNetwork> {
    let Some(text) = read_text(SYS_WIFI_KNOWN, 0) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 3 {
                return None;
            }
            Some(SavedNetwork {
                ssid: f[0].replace("\\t", "\t").replace("\\\\", "\\"),
                automatic: f[1] == "1",
                secured: f[2] == "saved",
                last_used: f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0),
            })
        })
        .collect()
}

pub fn wifi_forget(ssid: &str) -> i64 {
    unsafe { syscall5(SYS_WIFI_FORGET, ssid.as_ptr() as u64, ssid.len() as u64, 0, 0, 0) }
}

pub fn wifi_autojoin(ssid: &str, automatic: bool) -> i64 {
    unsafe { syscall5(SYS_WIFI_AUTOJOIN, ssid.as_ptr() as u64, ssid.len() as u64, automatic as u64, 0, 0) }
}

pub fn wifi_disconnect() -> i64 {
    unsafe { syscall3(SYS_WIFI_DISCONNECT, 0, 0, 0) }
}

pub fn configure(interface: &str, spec: &str) -> i64 {
    unsafe { syscall5(SYS_NET_CONFIGURE, interface.as_ptr() as u64, interface.len() as u64, spec.as_ptr() as u64, spec.len() as u64, 0) }
}

pub fn parse_ipv4(text: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = text.trim().split('.');
    for slot in out.iter_mut() {
        *slot = parts.next()?.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

pub fn format_ipv4(ip: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

pub fn resolve(host: &str) -> Result<[u8; 4], i64> {
    if let Some(ip) = parse_ipv4(host) {
        return Ok(ip);
    }
    let mut out = [0u8; 4];
    let r = unsafe { syscall5(SYS_RESOLVE, host.as_ptr() as u64, host.len() as u64, out.as_mut_ptr() as u64, 0, 0) };
    if r < 0 { Err(r) } else { Ok(out) }
}

pub fn ping(ip: [u8; 4], sequence: u16, timeout_ms: u64) -> i64 {
    unsafe { syscall3(SYS_PING, u32::from_be_bytes(ip) as u64, sequence as u64, timeout_ms) }
}

pub struct TcpStream {
    handle: i64,
}

impl TcpStream {
    pub fn connect(ip: [u8; 4], port: u16, timeout_ms: u64) -> Result<TcpStream, i64> {
        let handle = unsafe { syscall3(SYS_SOCKET, SOCK_TCP, 0, 0) };
        if handle < 0 {
            return Err(handle);
        }
        let r = unsafe { syscall3(SYS_SOCKET_CONNECT, handle as u64, ((u32::from_be_bytes(ip) as u64) << 16) | port as u64, timeout_ms) };
        if r < 0 {
            unsafe { syscall3(SYS_SOCKET_CLOSE, handle as u64, 0, 0) };
            return Err(r);
        }
        Ok(TcpStream { handle })
    }

    pub fn send(&mut self, data: &[u8]) -> i64 {
        let mut done = 0usize;
        while done < data.len() {
            let n = unsafe { syscall3(SYS_SOCKET_SEND, self.handle as u64, data[done..].as_ptr() as u64, (data.len() - done) as u64) };
            if n < 0 {
                return n;
            }
            done += n as usize;
        }
        done as i64
    }

    pub fn recv(&mut self, buf: &mut [u8], timeout_ms: u64) -> i64 {
        unsafe { syscall5(SYS_SOCKET_RECV, self.handle as u64, buf.as_mut_ptr() as u64, buf.len() as u64, timeout_ms, 0) }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe { syscall3(SYS_SOCKET_CLOSE, self.handle as u64, 0, 0) };
    }
}

pub struct UdpSocket {
    handle: i64,
}

impl UdpSocket {
    pub fn bind(port: u16) -> Result<UdpSocket, i64> {
        let handle = unsafe { syscall3(SYS_SOCKET, SOCK_UDP, port as u64, 0) };
        if handle < 0 { Err(handle) } else { Ok(UdpSocket { handle }) }
    }

    pub fn send_to(&mut self, ip: [u8; 4], port: u16, data: &[u8]) -> i64 {
        unsafe { syscall5(SYS_SOCKET_SENDTO, self.handle as u64, ((u32::from_be_bytes(ip) as u64) << 16) | port as u64, data.as_ptr() as u64, data.len() as u64, 0) }
    }

    pub fn recv_from(&mut self, buf: &mut [u8], timeout_ms: u64) -> Result<(usize, [u8; 4], u16), i64> {
        let mut from = [0u8; 8];
        let n = unsafe { syscall5(SYS_SOCKET_RECVFROM, self.handle as u64, buf.as_mut_ptr() as u64, buf.len() as u64, timeout_ms, from.as_mut_ptr() as u64) };
        if n < 0 {
            return Err(n);
        }
        Ok((n as usize, [from[0], from[1], from[2], from[3]], u16::from_le_bytes([from[4], from[5]])))
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        unsafe { syscall3(SYS_SOCKET_CLOSE, self.handle as u64, 0, 0) };
    }
}
