use crate::sys::{syscall3, syscall5};

use super::*;

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
