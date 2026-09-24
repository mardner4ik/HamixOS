use alloc::vec::Vec;

use crate::sys::{syscall5, syscall6};

const SYS_PREAD64: u64 = 17;
const SYS_PWRITE64: u64 = 18;
const SYS_POLL: u64 = 7;
const SYS_FTRUNCATE: u64 = 77;
const SYS_MMAP: u64 = 9;
const SYS_MUNMAP: u64 = 11;
const SYS_FCNTL: u64 = 72;
const SYS_SOCKET: u64 = 41;
const SYS_CONNECT: u64 = 42;
const SYS_SENDMSG: u64 = 46;
const SYS_RECVMSG: u64 = 47;
const SYS_SHUTDOWN: u64 = 48;
const SYS_BIND: u64 = 49;
const SYS_LISTEN: u64 = 50;
const SYS_SOCKETPAIR: u64 = 53;
const SYS_GETSOCKOPT: u64 = 55;
const SYS_EPOLL_WAIT: u64 = 232;
const SYS_EPOLL_CTL: u64 = 233;
const SYS_ACCEPT4: u64 = 288;
const SYS_EPOLL_CREATE1: u64 = 291;
const SYS_MEMFD_CREATE: u64 = 319;
const SYS_HAMIX_MAILBOX_FD: u64 = 9024;

pub const AF_UNIX: u64 = 1;
pub const SOCK_STREAM: u64 = 1;
pub const SOCK_DGRAM: u64 = 2;
pub const SOCK_SEQPACKET: u64 = 5;
pub const SOCK_NONBLOCK: u64 = 0o4000;
pub const O_NONBLOCK: u64 = 0o4000;
pub const MSG_DONTWAIT: u64 = 0x40;
pub const MSG_NOSIGNAL: u64 = 0x4000;

pub const POLLIN: u16 = 1;
pub const POLLOUT: u16 = 4;
pub const POLLERR: u16 = 8;
pub const POLLHUP: u16 = 0x10;
pub const POLLNVAL: u16 = 0x20;

pub const EPOLLIN: u32 = 1;
pub const EPOLLOUT: u32 = 4;
pub const EPOLLERR: u32 = 8;
pub const EPOLLHUP: u32 = 0x10;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLONESHOT: u32 = 1 << 30;
pub const EPOLL_CTL_ADD: u64 = 1;
pub const EPOLL_CTL_DEL: u64 = 2;
pub const EPOLL_CTL_MOD: u64 = 3;

pub const EAGAIN: i64 = -11;

const SOL_SOCKET: u32 = 1;
const SCM_RIGHTS: u32 = 1;
const SO_PEERCRED: u64 = 17;
const MAX_FDS: usize = 28;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;
const MAP_SHARED: u64 = 1;
const PROT_RW: u64 = 3;

fn call(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    unsafe { syscall5(num, a1, a2, a3, a4, a5) }
}

fn sockaddr(path: &str) -> Result<([u8; 110], u64), i64> {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() > 107 {
        return Err(-22);
    }
    let mut addr = [0u8; 110];
    addr[0..2].copy_from_slice(&(AF_UNIX as u16).to_le_bytes());
    addr[2..2 + bytes.len()].copy_from_slice(bytes);
    let len = if bytes[0] == 0 { 2 + bytes.len() } else { 3 + bytes.len() };
    Ok((addr, len as u64))
}

pub fn socket(kind: u64) -> i64 {
    call(SYS_SOCKET, AF_UNIX, kind, 0, 0, 0)
}

pub fn socketpair(kind: u64) -> Result<(u64, u64), i64> {
    let mut raw = [0i32; 2];
    let r = call(SYS_SOCKETPAIR, AF_UNIX, kind, 0, raw.as_mut_ptr() as u64, 0);
    if r < 0 { Err(r) } else { Ok((raw[0] as u64, raw[1] as u64)) }
}

pub fn bind(fd: u64, path: &str) -> i64 {
    match sockaddr(path) {
        Ok((addr, len)) => call(SYS_BIND, fd, addr.as_ptr() as u64, len, 0, 0),
        Err(e) => e,
    }
}

pub fn listen(fd: u64, backlog: u64) -> i64 {
    call(SYS_LISTEN, fd, backlog, 0, 0, 0)
}

pub fn listen_at(path: &str, kind: u64) -> i64 {
    let fd = socket(kind);
    if fd < 0 {
        return fd;
    }
    let r = bind(fd as u64, path);
    let r = if r < 0 { r } else { listen(fd as u64, 16) };
    if r < 0 {
        crate::sys::close(fd as u64);
        return r;
    }
    fd
}

pub fn connect(fd: u64, path: &str) -> i64 {
    match sockaddr(path) {
        Ok((addr, len)) => call(SYS_CONNECT, fd, addr.as_ptr() as u64, len, 0, 0),
        Err(e) => e,
    }
}

pub fn connect_to(path: &str, kind: u64) -> i64 {
    let fd = socket(kind);
    if fd < 0 {
        return fd;
    }
    let r = connect(fd as u64, path);
    if r < 0 {
        crate::sys::close(fd as u64);
        return r;
    }
    fd
}

pub fn accept(fd: u64, flags: u64) -> i64 {
    call(SYS_ACCEPT4, fd, 0, 0, flags, 0)
}

pub fn shutdown(fd: u64, how: u64) -> i64 {
    call(SYS_SHUTDOWN, fd, how, 0, 0, 0)
}

pub fn send_with_fds(fd: u64, data: &[u8], fds: &[u64], flags: u64) -> i64 {
    if fds.len() > MAX_FDS {
        return -22;
    }
    let iov: [u64; 2] = [data.as_ptr() as u64, data.len() as u64];
    let mut control = [0u8; 16 + MAX_FDS * 4 + 4];
    let control_len = if fds.is_empty() {
        0
    } else {
        let len = 16 + fds.len() * 4;
        control[0..8].copy_from_slice(&(len as u64).to_le_bytes());
        control[8..12].copy_from_slice(&SOL_SOCKET.to_le_bytes());
        control[12..16].copy_from_slice(&SCM_RIGHTS.to_le_bytes());
        for (i, f) in fds.iter().enumerate() {
            control[16 + i * 4..20 + i * 4].copy_from_slice(&(*f as i32).to_le_bytes());
        }
        (len + 7) & !7
    };
    let mut msg = [0u8; 56];
    msg[16..24].copy_from_slice(&(iov.as_ptr() as u64).to_le_bytes());
    msg[24..32].copy_from_slice(&1u64.to_le_bytes());
    if control_len > 0 {
        msg[32..40].copy_from_slice(&(control.as_ptr() as u64).to_le_bytes());
        msg[40..48].copy_from_slice(&(control_len as u64).to_le_bytes());
    }
    call(SYS_SENDMSG, fd, msg.as_ptr() as u64, flags | MSG_NOSIGNAL, 0, 0)
}

pub fn recv_with_fds(fd: u64, buf: &mut [u8], flags: u64) -> Result<(usize, Vec<u64>), i64> {
    let iov: [u64; 2] = [buf.as_mut_ptr() as u64, buf.len() as u64];
    let mut control = [0u8; 16 + MAX_FDS * 4 + 4];
    let mut msg = [0u8; 56];
    msg[16..24].copy_from_slice(&(iov.as_ptr() as u64).to_le_bytes());
    msg[24..32].copy_from_slice(&1u64.to_le_bytes());
    msg[32..40].copy_from_slice(&(control.as_mut_ptr() as u64).to_le_bytes());
    msg[40..48].copy_from_slice(&(control.len() as u64).to_le_bytes());
    let n = call(SYS_RECVMSG, fd, msg.as_mut_ptr() as u64, flags, 0, 0);
    if n < 0 {
        return Err(n);
    }
    let used = u64::from_le_bytes(msg[40..48].try_into().unwrap()) as usize;
    let mut fds = Vec::new();
    let mut off = 0usize;
    while off + 16 <= used.min(control.len()) {
        let len = u64::from_le_bytes(control[off..off + 8].try_into().unwrap()) as usize;
        let level = u32::from_le_bytes(control[off + 8..off + 12].try_into().unwrap());
        let kind = u32::from_le_bytes(control[off + 12..off + 16].try_into().unwrap());
        if len < 16 || off + len > control.len() {
            break;
        }
        if level == SOL_SOCKET && kind == SCM_RIGHTS {
            for chunk in control[off + 16..off + len].chunks_exact(4) {
                fds.push(i32::from_le_bytes(chunk.try_into().unwrap()) as u64);
            }
        }
        off += (len + 7) & !7;
    }
    Ok((n as usize, fds))
}

pub struct PeerCred {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

pub fn peer_cred(fd: u64) -> Option<PeerCred> {
    let mut raw = [0u32; 3];
    let mut len: u32 = 12;
    let r = call(SYS_GETSOCKOPT, fd, SOL_SOCKET as u64, SO_PEERCRED, raw.as_mut_ptr() as u64, &mut len as *mut u32 as u64);
    if r < 0 { None } else { Some(PeerCred { pid: raw[0], uid: raw[1], gid: raw[2] }) }
}

pub fn set_nonblocking(fd: u64, on: bool) -> i64 {
    let flags = call(SYS_FCNTL, fd, F_GETFL, 0, 0, 0);
    if flags < 0 {
        return flags;
    }
    let flags = if on { flags as u64 | O_NONBLOCK } else { flags as u64 & !O_NONBLOCK };
    call(SYS_FCNTL, fd, F_SETFL, flags, 0, 0)
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PollFd {
    pub fd: i32,
    pub events: u16,
    pub revents: u16,
}

pub fn poll(fds: &mut [PollFd], timeout_ms: i32) -> i64 {
    call(SYS_POLL, fds.as_mut_ptr() as u64, fds.len() as u64, timeout_ms as i64 as u64, 0, 0)
}

pub fn epoll_create() -> i64 {
    call(SYS_EPOLL_CREATE1, 0, 0, 0, 0, 0)
}

pub fn epoll_ctl(epfd: u64, op: u64, fd: u64, events: u32, data: u64) -> i64 {
    let mut raw = [0u8; 12];
    raw[0..4].copy_from_slice(&events.to_le_bytes());
    raw[4..12].copy_from_slice(&data.to_le_bytes());
    call(SYS_EPOLL_CTL, epfd, op, fd, raw.as_ptr() as u64, 0)
}

pub fn epoll_wait(epfd: u64, out: &mut [(u32, u64)], timeout_ms: i32) -> i64 {
    let mut raw = alloc::vec![0u8; out.len() * 12];
    let n = call(SYS_EPOLL_WAIT, epfd, raw.as_mut_ptr() as u64, out.len() as u64, timeout_ms as i64 as u64, 0);
    for i in 0..n.max(0) as usize {
        let chunk = &raw[i * 12..i * 12 + 12];
        out[i] = (u32::from_le_bytes(chunk[0..4].try_into().unwrap()), u64::from_le_bytes(chunk[4..12].try_into().unwrap()));
    }
    n
}

pub fn mailbox_fd() -> i64 {
    call(SYS_HAMIX_MAILBOX_FD, 0, 0, 0, 0, 0)
}

pub fn memfd_create(name: &str) -> i64 {
    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(0);
    call(SYS_MEMFD_CREATE, bytes.as_ptr() as u64, 0, 0, 0, 0)
}

pub fn ftruncate(fd: u64, len: u64) -> i64 {
    call(SYS_FTRUNCATE, fd, len, 0, 0, 0)
}

pub fn map_shared(fd: u64, len: u64, offset: u64) -> Result<*mut u8, i64> {
    let r = unsafe { syscall6(SYS_MMAP, 0, len, PROT_RW, MAP_SHARED, fd, offset) };
    if r < 0 { Err(r) } else { Ok(r as *mut u8) }
}

pub fn unmap(addr: *mut u8, len: u64) -> i64 {
    call(SYS_MUNMAP, addr as u64, len, 0, 0, 0)
}

pub fn pread(fd: u64, buf: &mut [u8], offset: u64) -> i64 {
    call(SYS_PREAD64, fd, buf.as_mut_ptr() as u64, buf.len() as u64, offset, 0)
}

pub fn pwrite(fd: u64, data: &[u8], offset: u64) -> i64 {
    call(SYS_PWRITE64, fd, data.as_ptr() as u64, data.len() as u64, offset, 0)
}
