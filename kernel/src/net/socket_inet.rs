use alloc::vec::Vec;

use crate::syscall::{copy_out, install_fd, user_slice, EFAULT, EINVAL};
use crate::net::inet::{self, Endpoint, Proto};
use crate::task::{self, OpenFile};

pub const AF_INET: u64 = 2;
pub const AF_INET6: u64 = 10;
const AF_UNSPEC: u16 = 0;
const SOCK_STREAM: u64 = 1;
const SOCK_DGRAM: u64 = 2;
const SOCK_NONBLOCK: u64 = 0o4000;
const SOCK_CLOEXEC: u64 = 0o2000000;
const IPPROTO_IP: u64 = 0;
const IPPROTO_TCP: u64 = 6;
const IPPROTO_UDP: u64 = 17;
const SOL_SOCKET: u64 = 1;
const SO_REUSEADDR: u64 = 2;
const SO_TYPE: u64 = 3;
const SO_ERROR: u64 = 4;
const SO_BROADCAST: u64 = 6;
const SO_SNDBUF: u64 = 7;
const SO_RCVBUF: u64 = 8;
const SO_KEEPALIVE: u64 = 9;
const SO_RCVTIMEO: u64 = 20;
const SO_SNDTIMEO: u64 = 21;
const SO_ACCEPTCONN: u64 = 30;
const SO_PROTOCOL: u64 = 38;
const SO_DOMAIN: u64 = 39;
const TCP_NODELAY: u64 = 1;
const MSG_PEEK: u64 = 2;
const MSG_TRUNC: u64 = 0x20;
const MSG_DONTWAIT: u64 = 0x40;
const MSG_WAITALL: u64 = 0x100;
const MSG_NOSIGNAL: u64 = 0x4000;
const EAFNOSUPPORT: i64 = -97;
const EPROTONOSUPPORT: i64 = -93;
const ESOCKTNOSUPPORT: i64 = -94;
const ENOPROTOOPT: i64 = -92;
const MAX_CHUNK: usize = 16 << 20;

fn wait(deadline: u64) -> Result<(), i64> {
    if super::signal::pending() {
        return Err(super::signal::EINTR);
    }
    if deadline != 0 && task::uptime_ms() >= deadline {
        return Err(inet::EAGAIN);
    }
    task::block(task::WAIT_PIPE, Some(4), task::input_seq());
    task::check_killed();
    if super::signal::pending() {
        return Err(super::signal::EINTR);
    }
    Ok(())
}

fn deadline_after(ms: u64) -> u64 {
    if ms == 0 { 0 } else { task::uptime_ms() + ms }
}

pub fn socket(domain: u64, kind: u64, protocol: u64) -> i64 {
    if domain == AF_INET6 {
        return EAFNOSUPPORT;
    }
    if kind & !(0xF | SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        return EINVAL;
    }
    let proto = match (kind & 0xF, protocol) {
        (SOCK_STREAM, IPPROTO_IP) | (SOCK_STREAM, IPPROTO_TCP) => Proto::Tcp,
        (SOCK_DGRAM, IPPROTO_IP) | (SOCK_DGRAM, IPPROTO_UDP) => Proto::Udp,
        (SOCK_STREAM, _) | (SOCK_DGRAM, _) => return EPROTONOSUPPORT,
        _ => return ESOCKTNOSUPPORT,
    };
    let id = inet::create(proto);
    let fd = install_fd(OpenFile::Socket { id, nonblock: kind & SOCK_NONBLOCK != 0 });
    if fd < 0 {
        inet::release(id);
    }
    fd
}

pub fn read_address(ptr: u64, len: u64) -> Result<Option<Endpoint>, i64> {
    if ptr == 0 {
        return Err(EFAULT);
    }
    if len < 2 {
        return Err(EINVAL);
    }
    let raw = user_slice(ptr, len.min(128))?.to_vec();
    let family = u16::from_le_bytes([raw[0], raw[1]]);
    match family as u64 {
        AF_INET if raw.len() >= 8 => {
            let port = u16::from_be_bytes([raw[2], raw[3]]);
            Ok(Some(([raw[4], raw[5], raw[6], raw[7]], port)))
        }
        AF_INET6 if raw.len() >= 24 => {
            let port = u16::from_be_bytes([raw[2], raw[3]]);
            let addr = &raw[8..24];
            if addr[..10].iter().all(|b| *b == 0) && addr[10] == 0xFF && addr[11] == 0xFF {
                Ok(Some(([addr[12], addr[13], addr[14], addr[15]], port)))
            } else {
                Err(EAFNOSUPPORT)
            }
        }
        _ if family == AF_UNSPEC => Ok(None),
        AF_INET | AF_INET6 => Err(EINVAL),
        _ => Err(EAFNOSUPPORT),
    }
}

fn encode(ep: Endpoint) -> [u8; 16] {
    let mut raw = [0u8; 16];
    raw[0..2].copy_from_slice(&(AF_INET as u16).to_le_bytes());
    raw[2..4].copy_from_slice(&ep.1.to_be_bytes());
    raw[4..8].copy_from_slice(&ep.0);
    raw
}

pub fn write_address(ptr: u64, len_ptr: u64, ep: Endpoint) -> i64 {
    if ptr == 0 || len_ptr == 0 {
        return 0;
    }
    let capacity = match user_slice(len_ptr, 4) {
        Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) as u64,
        Err(e) => return e,
    };
    let raw = encode(ep);
    if capacity > 0 {
        let r = copy_out(ptr, capacity.min(16), &raw[..capacity.min(16) as usize]);
        if r < 0 {
            return r;
        }
    }
    copy_out(len_ptr, 4, &16u32.to_le_bytes()).min(0)
}

pub fn bind(id: u32, addr: u64, len: u64) -> i64 {
    match read_address(addr, len) {
        Ok(Some(ep)) => inet::bind(id, ep).map(|_| 0).unwrap_or_else(|e| e),
        Ok(None) => EAFNOSUPPORT,
        Err(e) => e,
    }
}

pub fn listen(id: u32, backlog: u64) -> i64 {
    inet::listen(id, (backlog as i32).max(1) as usize).map(|_| 0).unwrap_or_else(|e| e)
}

pub fn connect(id: u32, nonblock: bool, addr: u64, len: u64) -> i64 {
    let remote = match read_address(addr, len) {
        Ok(r) => r,
        Err(e) => return e,
    };
    match inet::connect(id, remote) {
        Ok(()) => return 0,
        Err(inet::EINPROGRESS) => {}
        Err(e) => return e,
    }
    inet::kick();
    if nonblock {
        return inet::EINPROGRESS;
    }
    loop {
        if let Some(result) = inet::connect_result(id) {
            return result;
        }
        if let Err(e) = wait(0) {
            return if e == inet::EAGAIN { inet::EINPROGRESS } else { e };
        }
    }
}

pub fn accept(id: u32, nonblock: bool, addr: u64, addr_len: u64, flags: u64) -> i64 {
    let (recv_timeout, _) = inet::timeouts(id);
    let deadline = deadline_after(recv_timeout);
    let (new_id, peer) = loop {
        match inet::accept(id) {
            Ok(v) => break v,
            Err(inet::EAGAIN) if !nonblock => {
                if let Err(e) = wait(deadline) {
                    return e;
                }
            }
            Err(e) => return e,
        }
    };
    let fd = install_fd(OpenFile::Socket { id: new_id, nonblock: flags & SOCK_NONBLOCK != 0 });
    if fd < 0 {
        inet::release(new_id);
        return fd;
    }
    let r = write_address(addr, addr_len, peer);
    if r < 0 {
        crate::syscall::sys_close(fd as u64);
        return r;
    }
    fd
}

pub fn send(id: u32, nonblock: bool, data: &[u8], dest: Option<Endpoint>, flags: u64) -> i64 {
    let nonblock = nonblock || flags & MSG_DONTWAIT != 0;
    let stream = inet::proto(id) == Some(Proto::Tcp);
    let (_, send_timeout) = inet::timeouts(id);
    let deadline = deadline_after(send_timeout);
    let mut sent = 0usize;
    let result = loop {
        match inet::send(id, &data[sent..], dest) {
            Ok(n) => {
                sent += n;
                inet::kick();
                if !stream || sent >= data.len() || nonblock {
                    break sent as i64;
                }
            }
            Err(inet::EAGAIN) if !nonblock => {
                inet::kick();
                if let Err(e) = wait(deadline) {
                    break if sent > 0 { sent as i64 } else { e };
                }
            }
            Err(e) => break if sent > 0 { sent as i64 } else { e },
        }
    };
    if result == inet::EPIPE && flags & MSG_NOSIGNAL == 0 && crate::syscall::linux_abi() {
        super::signal::send(task::current_pid(), super::signal::SIGPIPE);
    }
    result
}

pub fn receive(id: u32, nonblock: bool, max: usize, flags: u64) -> Result<inet::Datagram, i64> {
    let nonblock = nonblock || flags & MSG_DONTWAIT != 0;
    let peek = flags & MSG_PEEK != 0;
    let (recv_timeout, _) = inet::timeouts(id);
    let deadline = deadline_after(recv_timeout);
    let mut first = loop {
        match inet::recv(id, max, peek) {
            Ok(d) => break d,
            Err(inet::EAGAIN) if !nonblock => wait(deadline)?,
            Err(e) => return Err(e),
        }
    };
    if flags & MSG_WAITALL != 0 && !peek && inet::proto(id) == Some(Proto::Tcp) {
        while first.data.len() < max && !first.data.is_empty() {
            match inet::recv(id, max - first.data.len(), false) {
                Ok(more) if more.data.is_empty() => break,
                Ok(more) => first.data.extend_from_slice(&more.data),
                Err(inet::EAGAIN) => {
                    if wait(deadline).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        first.full_len = first.data.len();
    }
    if !peek && inet::proto(id) == Some(Proto::Tcp) && !first.data.is_empty() {
        inet::kick();
    }
    Ok(first)
}

pub fn recv_bytes(id: u32, nonblock: bool, buf: u64, count: u64) -> i64 {
    match receive(id, nonblock, count.min(MAX_CHUNK as u64) as usize, 0) {
        Ok(d) => copy_out(buf, d.data.len() as u64, &d.data).min(d.data.len() as i64),
        Err(e) => e,
    }
}

pub fn recvfrom(id: u32, nonblock: bool, buf: u64, len: u64, flags: u64, addr: u64, addr_len: u64) -> i64 {
    match receive(id, nonblock, len.min(MAX_CHUNK as u64) as usize, flags) {
        Ok(d) => {
            let r = copy_out(buf, d.data.len() as u64, &d.data);
            if r < 0 {
                return r;
            }
            if let Some(from) = d.from {
                let w = write_address(addr, addr_len, from);
                if w < 0 {
                    return w;
                }
            }
            if flags & MSG_TRUNC != 0 { d.full_len as i64 } else { d.data.len() as i64 }
        }
        Err(e) => e,
    }
}

pub fn recvmsg_fill(id: u32, nonblock: bool, capacity: usize, flags: u64, header: &mut [u8; 56]) -> Result<(Vec<u8>, usize), i64> {
    let d = receive(id, nonblock, capacity, flags)?;
    let name = u64::from_le_bytes(header[0..8].try_into().unwrap());
    if name != 0 {
        let cap = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
        if let Some(from) = d.from {
            let raw = encode(from);
            if cap > 0 {
                copy_out(name, cap.min(16), &raw[..cap.min(16) as usize]);
            }
            header[8..12].copy_from_slice(&16u32.to_le_bytes());
        } else {
            header[8..12].copy_from_slice(&0u32.to_le_bytes());
        }
    }
    header[40..48].copy_from_slice(&0u64.to_le_bytes());
    let msg_flags: u32 = if d.full_len > d.data.len() { MSG_TRUNC as u32 } else { 0 };
    header[48..52].copy_from_slice(&msg_flags.to_le_bytes());
    let full = d.full_len;
    Ok((d.data, full))
}

pub fn shutdown(id: u32, how: u64) -> i64 {
    inet::shutdown(id, how).map(|_| 0).unwrap_or_else(|e| e)
}

pub fn names(id: u32, addr: u64, len: u64, peer: bool) -> i64 {
    match inet::names(id) {
        Ok((local, remote)) => {
            if peer {
                match remote {
                    Some(r) => write_address(addr, len, r),
                    None => inet::ENOTCONN,
                }
            } else {
                write_address(addr, len, local)
            }
        }
        Err(e) => e,
    }
}

fn read_int(value: u64, len: u64) -> Result<u32, i64> {
    if value == 0 || len < 4 {
        return Err(EINVAL);
    }
    let raw = user_slice(value, 4)?;
    Ok(u32::from_le_bytes((&*raw).try_into().unwrap()))
}

fn read_timeval_ms(value: u64, len: u64) -> Result<u64, i64> {
    if value == 0 || len < 16 {
        return Err(EINVAL);
    }
    let raw = user_slice(value, 16)?.to_vec();
    let sec = i64::from_le_bytes(raw[0..8].try_into().unwrap()).max(0) as u64;
    let usec = i64::from_le_bytes(raw[8..16].try_into().unwrap()).max(0) as u64;
    Ok(sec * 1000 + usec.div_ceil(1000))
}

pub fn setsockopt(id: u32, level: u64, name: u64, value: u64, len: u64) -> i64 {
    let result = match (level, name) {
        (SOL_SOCKET, SO_KEEPALIVE) => read_int(value, len).map(|v| inet::set_keepalive(id, v != 0)),
        (SOL_SOCKET, SO_RCVTIMEO) => read_timeval_ms(value, len).map(|ms| inet::set_timeouts(id, Some(ms), None)),
        (SOL_SOCKET, SO_SNDTIMEO) => read_timeval_ms(value, len).map(|ms| inet::set_timeouts(id, None, Some(ms))),
        (SOL_SOCKET, _) => Ok(()),
        (IPPROTO_TCP, TCP_NODELAY) => read_int(value, len).map(|v| inet::set_nodelay(id, v != 0)),
        (IPPROTO_TCP, _) | (IPPROTO_IP, _) | (IPPROTO_UDP, _) => Ok(()),
        _ => Err(ENOPROTOOPT),
    };
    result.map(|_| 0).unwrap_or_else(|e| e)
}

pub fn getsockopt(id: u32, level: u64, name: u64, value: u64, len_ptr: u64) -> i64 {
    let capacity = match user_slice(len_ptr, 4) {
        Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) as u64,
        Err(e) => return e,
    };
    let proto = inet::proto(id);
    let int = |v: u32| v.to_le_bytes().to_vec();
    let bytes: Vec<u8> = match (level, name) {
        (SOL_SOCKET, SO_TYPE) => int(if proto == Some(Proto::Tcp) { SOCK_STREAM as u32 } else { SOCK_DGRAM as u32 }),
        (SOL_SOCKET, SO_ERROR) => int((-inet::take_error(id)) as u32),
        (SOL_SOCKET, SO_PROTOCOL) => int(if proto == Some(Proto::Tcp) { IPPROTO_TCP as u32 } else { IPPROTO_UDP as u32 }),
        (SOL_SOCKET, SO_DOMAIN) => int(AF_INET as u32),
        (SOL_SOCKET, SO_SNDBUF) => int(128 * 1024),
        (SOL_SOCKET, SO_RCVBUF) => int(256 * 1024),
        (SOL_SOCKET, SO_ACCEPTCONN) => int(inet::is_listening(id) as u32),
        (SOL_SOCKET, SO_KEEPALIVE) => int(inet::keepalive(id) as u32),
        (SOL_SOCKET, SO_REUSEADDR) | (SOL_SOCKET, SO_BROADCAST) => int(0),
        (SOL_SOCKET, SO_RCVTIMEO) | (SOL_SOCKET, SO_SNDTIMEO) => {
            let (r, s) = inet::timeouts(id);
            let ms = if name == SO_RCVTIMEO { r } else { s };
            let mut raw = Vec::with_capacity(16);
            raw.extend_from_slice(&((ms / 1000) as i64).to_le_bytes());
            raw.extend_from_slice(&(((ms % 1000) * 1000) as i64).to_le_bytes());
            raw
        }
        (IPPROTO_TCP, TCP_NODELAY) => int(inet::nodelay(id) as u32),
        _ => return ENOPROTOOPT,
    };
    let n = (bytes.len() as u64).min(capacity);
    let r = copy_out(value, n, &bytes[..n as usize]);
    if r < 0 {
        return r;
    }
    copy_out(len_ptr, 4, &(n as u32).to_le_bytes()).min(0)
}
