use core::ffi::{c_char, c_int, c_void};

use hxlibc_sys::libc;

use super::*;

fn errno() -> i64 {
    -libc::errno()
}

fn address(ip: [u8; 4], port: u16) -> libc::sockaddr_in {
    libc::sockaddr_in { sin_family: libc::AF_INET as u16, sin_port: port.to_be(), sin_addr: ip, sin_zero: [0; 8] }
}

const SOCKADDR_IN_LEN: u32 = core::mem::size_of::<libc::sockaddr_in>() as u32;

fn wait_for(fd: c_int, events: i16, timeout_ms: u64) -> Result<(), i64> {
    let mut p = libc::pollfd { fd, events, revents: 0 };
    let timeout = timeout_ms.min(i32::MAX as u64) as c_int;
    loop {
        let r = unsafe { libc::poll(&mut p, 1, timeout) };
        if r < 0 && libc::errno() == libc::EINTR as i64 {
            continue;
        }
        return match r {
            n if n < 0 => Err(errno()),
            0 => Err(-(libc::ETIMEDOUT as i64)),
            _ => Ok(()),
        };
    }
}

pub fn resolve(host: &str) -> Result<[u8; 4], i64> {
    if let Some(ip) = parse_ipv4(host) {
        return Ok(ip);
    }
    let name = crate::sys::cstring(host);
    let hints = libc::addrinfo {
        ai_flags: 0,
        ai_family: libc::AF_INET,
        ai_socktype: libc::SOCK_STREAM,
        ai_protocol: 0,
        ai_addrlen: 0,
        ai_addr: core::ptr::null_mut(),
        ai_canonname: core::ptr::null_mut(),
        ai_next: core::ptr::null_mut(),
    };
    let mut list: *mut libc::addrinfo = core::ptr::null_mut();
    let r = unsafe { libc::getaddrinfo(name.as_ptr() as *const c_char, core::ptr::null(), &hints, &mut list) };
    if r != 0 || list.is_null() {
        return Err(-2);
    }
    let mut found = None;
    let mut item = list;
    while !item.is_null() {
        let entry = unsafe { &*item };
        if entry.ai_family == libc::AF_INET && !entry.ai_addr.is_null() {
            found = Some(unsafe { (*entry.ai_addr).sin_addr });
            break;
        }
        item = entry.ai_next;
    }
    unsafe { libc::freeaddrinfo(list) };
    found.ok_or(-2)
}

pub fn ping(ip: [u8; 4], sequence: u16, timeout_ms: u64) -> i64 {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, libc::IPPROTO_ICMP) };
    if fd < 0 {
        return errno();
    }
    let result = ping_on(fd, ip, sequence, timeout_ms);
    unsafe { libc::close(fd) };
    result
}

fn ping_on(fd: c_int, ip: [u8; 4], sequence: u16, timeout_ms: u64) -> i64 {
    let payload = b"HamixOS ping payload 0123456789";
    let mut packet = alloc::vec![8u8, 0, 0, 0, 0, 0];
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(payload);
    let target = address(ip, 0);
    let started = crate::sys::uptime_ms();
    if unsafe { libc::sendto(fd, packet.as_ptr() as *const c_void, packet.len(), 0, &target as *const libc::sockaddr_in as *const libc::sockaddr_un, SOCKADDR_IN_LEN) } < 0 {
        return errno();
    }
    let deadline = started + timeout_ms;
    let mut reply = [0u8; 256];
    loop {
        let now = crate::sys::uptime_ms();
        if now >= deadline {
            return -(libc::ETIMEDOUT as i64);
        }
        if let Err(e) = wait_for(fd, libc::POLLIN, deadline - now) {
            return e;
        }
        let mut from = libc::sockaddr_in::default();
        let mut from_len = SOCKADDR_IN_LEN;
        let n = unsafe { libc::recvfrom(fd, reply.as_mut_ptr() as *mut c_void, reply.len(), 0, &mut from, &mut from_len) };
        if n < 0 {
            return errno();
        }
        if n >= 8 && reply[0] == 0 && u16::from_be_bytes([reply[6], reply[7]]) == sequence && from.sin_addr == ip {
            return (crate::sys::uptime_ms() - started) as i64;
        }
    }
}

pub struct TcpStream {
    fd: c_int,
}

impl TcpStream {
    pub fn connect(ip: [u8; 4], port: u16, timeout_ms: u64) -> Result<TcpStream, i64> {
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(errno());
        }
        let stream = TcpStream { fd };
        let target = address(ip, port);
        if unsafe { libc::connect(fd, &target, SOCKADDR_IN_LEN) } < 0 {
            if libc::errno() != libc::EINPROGRESS as i64 {
                return Err(errno());
            }
            wait_for(fd, libc::POLLOUT, timeout_ms)?;
            let mut error: c_int = 0;
            let mut len = 4u32;
            unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_ERROR, &mut error as *mut c_int as *mut c_void, &mut len) };
            if error != 0 {
                return Err(-(error as i64));
            }
        }
        Ok(stream)
    }

    pub fn send(&mut self, data: &[u8]) -> i64 {
        let mut done = 0usize;
        while done < data.len() {
            let n = unsafe { libc::send(self.fd, data[done..].as_ptr() as *const c_void, data.len() - done, libc::MSG_NOSIGNAL) };
            if n < 0 {
                if libc::errno() == libc::EAGAIN as i64 {
                    if let Err(e) = wait_for(self.fd, libc::POLLOUT, 30_000) {
                        return e;
                    }
                    continue;
                }
                return errno();
            }
            done += n as usize;
        }
        done as i64
    }

    pub fn recv(&mut self, buf: &mut [u8], timeout_ms: u64) -> i64 {
        if let Err(e) = wait_for(self.fd, libc::POLLIN, timeout_ms) {
            return e;
        }
        let n = unsafe { libc::recv(self.fd, buf.as_mut_ptr() as *mut c_void, buf.len(), 0) };
        if n < 0 { errno() } else { n as i64 }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

pub struct UdpSocket {
    fd: c_int,
}

impl UdpSocket {
    pub fn bind(port: u16) -> Result<UdpSocket, i64> {
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(errno());
        }
        let socket = UdpSocket { fd };
        let local = address([0; 4], port);
        if unsafe { libc::bind(fd, &local as *const libc::sockaddr_in as *const libc::sockaddr_un, SOCKADDR_IN_LEN) } < 0 {
            return Err(errno());
        }
        Ok(socket)
    }

    pub fn send_to(&mut self, ip: [u8; 4], port: u16, data: &[u8]) -> i64 {
        let target = address(ip, port);
        let n = unsafe { libc::sendto(self.fd, data.as_ptr() as *const c_void, data.len(), 0, &target as *const libc::sockaddr_in as *const libc::sockaddr_un, SOCKADDR_IN_LEN) };
        if n < 0 { errno() } else { n as i64 }
    }

    pub fn recv_from(&mut self, buf: &mut [u8], timeout_ms: u64) -> Result<(usize, [u8; 4], u16), i64> {
        wait_for(self.fd, libc::POLLIN, timeout_ms)?;
        let mut from = libc::sockaddr_in::default();
        let mut from_len = SOCKADDR_IN_LEN;
        let n = unsafe { libc::recvfrom(self.fd, buf.as_mut_ptr() as *mut c_void, buf.len(), 0, &mut from, &mut from_len) };
        if n < 0 {
            return Err(errno());
        }
        Ok((n as usize, from.sin_addr, u16::from_be(from.sin_port)))
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}
