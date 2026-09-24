use alloc::string::String;
use alloc::vec::Vec;

use crate::syscall::{absolute, copy_out, euid, install_fd, target, user_slice, Target, EBADF, EFAULT, EINVAL, EMFILE, ENOENT};
use crate::fs;
use crate::net::inet::is_inet;
use crate::net::unix::{self, Cred, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM};
use crate::task::{self, OpenFile};

const AF_UNIX: u64 = 1;
const SOCK_NONBLOCK: u64 = 0o4000;
const SOCK_CLOEXEC: u64 = 0o2000000;
const MSG_PEEK: u64 = 2;
const MSG_TRUNC: u64 = 0x20;
const MSG_DONTWAIT: u64 = 0x40;
const MSG_WAITALL: u64 = 0x100;
const MSG_NOSIGNAL: u64 = 0x4000;
const MSG_CMSG_CLOEXEC: u64 = 0x4000_0000;
const MSG_CTRUNC: u32 = 8;
const SOL_SOCKET: u64 = 1;
const SCM_RIGHTS: u32 = 1;
const SCM_CREDENTIALS: u32 = 2;
const SO_PASSCRED: u64 = 16;
const SO_TYPE: u64 = 3;
const SO_ERROR: u64 = 4;
const SO_SNDBUF: u64 = 7;
const SO_RCVBUF: u64 = 8;
const SO_PEERCRED: u64 = 17;
const SO_ACCEPTCONN: u64 = 30;
const SO_PROTOCOL: u64 = 38;
const SO_DOMAIN: u64 = 39;
const SUN_PATH_MAX: usize = 108;
const MAX_IOV: u64 = 1024;
const MAX_MSG: usize = 16 << 20;
const MAX_PASSED_FDS: usize = 253;

const ENOTSOCK: i64 = -88;
const ENOPROTOOPT: i64 = -92;
const EPROTONOSUPPORT: i64 = -93;
const ESOCKTNOSUPPORT: i64 = -94;
const EAFNOSUPPORT: i64 = -97;
const EEXIST: i64 = -17;
const EADDRINUSE: i64 = -98;

fn cred() -> Cred {
    let (pid, uid) = task::with_current(|t| (t.pid, t.uid));
    Cred { pid, uid, gid: super::base::gid_for(uid) }
}

fn socket_of(fd: u64) -> Result<(u32, bool), i64> {
    match target(fd) {
        Target::Socket(id, nonblock) => Ok((id, nonblock)),
        Target::Bad => Err(EBADF),
        _ => Err(ENOTSOCK),
    }
}

fn wait_socket() -> Result<(), i64> {
    if super::signal::pending() {
        return Err(super::signal::EINTR);
    }
    task::block(task::WAIT_PIPE, Some(task::TICK_HZ / 5), task::input_seq());
    task::check_killed();
    crate::vt::service_pending();
    if super::signal::pending() {
        return Err(super::signal::EINTR);
    }
    Ok(())
}

fn install_pair(a: OpenFile, b: OpenFile) -> Result<(i64, i64), i64> {
    let fa = install_fd(a);
    if fa < 0 {
        b.release();
        return Err(fa);
    }
    let fb = install_fd(b);
    if fb < 0 {
        crate::syscall::sys_close(fa as u64);
        return Err(fb);
    }
    Ok((fa, fb))
}

fn sock_kind(kind: u64) -> Result<u8, i64> {
    match kind & 0xF {
        1 => Ok(SOCK_STREAM),
        2 => Ok(SOCK_DGRAM),
        5 => Ok(SOCK_SEQPACKET),
        _ => Err(ESOCKTNOSUPPORT),
    }
}

pub fn sys_socket(domain: u64, kind: u64, protocol: u64) -> i64 {
    if domain == super::netlink::AF_NETLINK {
        return match super::netlink::create(protocol, cred()) {
            Ok(id) => install_fd(OpenFile::Socket { id, nonblock: kind & SOCK_NONBLOCK != 0 }),
            Err(e) => e,
        };
    }
    if domain == super::inet::AF_INET || domain == super::inet::AF_INET6 {
        return super::inet::socket(domain, kind, protocol);
    }
    if domain != AF_UNIX {
        return EAFNOSUPPORT;
    }
    if protocol != 0 {
        return EPROTONOSUPPORT;
    }
    if kind & !(0xF | SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        return EINVAL;
    }
    let sk = match sock_kind(kind) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let id = unix::create(sk, cred());
    install_fd(OpenFile::Socket { id, nonblock: kind & SOCK_NONBLOCK != 0 })
}

pub fn sys_socketpair(domain: u64, kind: u64, protocol: u64, out: u64) -> i64 {
    if domain != AF_UNIX {
        return EAFNOSUPPORT;
    }
    if protocol != 0 {
        return EPROTONOSUPPORT;
    }
    let sk = match sock_kind(kind) {
        Ok(k) => k,
        Err(e) => return e,
    };
    if let Err(e) = user_slice(out, 8) {
        return e;
    }
    let nonblock = kind & SOCK_NONBLOCK != 0;
    let (a, b) = unix::pair(sk, cred());
    match install_pair(OpenFile::Socket { id: a, nonblock }, OpenFile::Socket { id: b, nonblock }) {
        Ok((fa, fb)) => {
            let mut raw = [0u8; 8];
            raw[0..4].copy_from_slice(&(fa as u32).to_le_bytes());
            raw[4..8].copy_from_slice(&(fb as u32).to_le_bytes());
            copy_out(out, 8, &raw).min(0)
        }
        Err(e) => e,
    }
}

enum Address {
    Path(String),
    Abstract(String),
    Unnamed,
}

impl Address {
    fn key(&self) -> Option<String> {
        match self {
            Address::Path(p) => Some(p.clone()),
            Address::Abstract(a) => Some(a.clone()),
            Address::Unnamed => None,
        }
    }
}

fn read_address(ptr: u64, len: u64) -> Result<Address, i64> {
    if ptr == 0 {
        return Err(EFAULT);
    }
    if !(2..=110).contains(&len) {
        return Err(EINVAL);
    }
    let raw = user_slice(ptr, len)?.to_vec();
    if u16::from_le_bytes([raw[0], raw[1]]) as u64 != AF_UNIX {
        return Err(EAFNOSUPPORT);
    }
    let path = &raw[2..];
    if path.is_empty() {
        return Ok(Address::Unnamed);
    }
    if path[0] == 0 {
        let mut name = String::from("\0");
        name.push_str(&String::from_utf8_lossy(&path[1..]));
        return Ok(Address::Abstract(name));
    }
    let end = path.iter().position(|b| *b == 0).unwrap_or(path.len());
    let text = String::from_utf8_lossy(&path[..end]).into_owned();
    Ok(Address::Path(absolute(&text)))
}

fn write_address(ptr: u64, len_ptr: u64, name: Option<&str>) -> i64 {
    if ptr == 0 || len_ptr == 0 {
        return 0;
    }
    let capacity = match user_slice(len_ptr, 4) {
        Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) as u64,
        Err(e) => return e,
    };
    let mut raw: Vec<u8> = Vec::with_capacity(2 + SUN_PATH_MAX);
    raw.extend_from_slice(&(AF_UNIX as u16).to_le_bytes());
    match name {
        Some(n) if n.starts_with('\0') => raw.extend_from_slice(n.as_bytes()),
        Some(n) => {
            raw.extend_from_slice(n.as_bytes());
            raw.push(0);
        }
        None => {}
    }
    raw.truncate(2 + SUN_PATH_MAX);
    if capacity > 0 {
        let r = copy_out(ptr, capacity.min(raw.len() as u64), &raw);
        if r < 0 {
            return r;
        }
    }
    copy_out(len_ptr, 4, &(raw.len() as u32).to_le_bytes()).min(0)
}

pub fn sys_bind(fd: u64, addr: u64, len: u64) -> i64 {
    let (id, _) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if super::netlink::is_netlink(id) {
        return 0;
    }
    if is_inet(id) {
        return super::inet::bind(id, addr, len);
    }
    let address = match read_address(addr, len) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let key = match &address {
        Address::Unnamed => return EINVAL,
        Address::Path(path) => {
            let uid = euid();
            let umask = task::with_current(|t| t.umask) as u16;
            let created = fs::VFS.lock().as_mut().map(|v| {
                if v.exists(0, path) {
                    return Err(EADDRINUSE);
                }
                v.create_file(0, path, Vec::new(), uid).map_err(crate::syscall::fs_error)?;
                let _ = v.chmod(0, path, uid, 0o777 & !umask);
                Ok(())
            });
            match created {
                Some(Ok(())) => {}
                Some(Err(e)) => return if e == EEXIST { EADDRINUSE } else { e },
                None => return crate::syscall::ENODEV,
            }
            path.clone()
        }
        Address::Abstract(name) => name.clone(),
    };
    match unix::bind(id, &key) {
        Ok(()) => 0,
        Err(e) => {
            if let Address::Path(path) = &address {
                if let Some(v) = fs::VFS.lock().as_mut() {
                    let _ = v.remove(0, path, 0);
                }
            }
            e
        }
    }
}

pub fn sys_listen(fd: u64, backlog: u64) -> i64 {
    match socket_of(fd) {
        Ok((id, _)) if is_inet(id) => super::inet::listen(id, backlog),
        Ok((id, _)) => unix::listen(id, (backlog as i32).max(1) as usize).map(|_| 0).unwrap_or_else(|e| e),
        Err(e) => e,
    }
}

pub fn sys_connect(fd: u64, addr: u64, len: u64) -> i64 {
    let (id, nonblock) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if is_inet(id) {
        return super::inet::connect(id, nonblock, addr, len);
    }
    let address = match read_address(addr, len) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let Some(key) = address.key() else {
        return EINVAL;
    };
    if let Address::Path(path) = &address {
        let exists = fs::VFS.lock().as_ref().map(|v| v.exists(0, path)).unwrap_or(false);
        if !exists {
            return ENOENT;
        }
    }
    let me = cred();
    loop {
        match unix::connect(id, &key, me) {
            Ok(()) => return 0,
            Err(unix::EAGAIN) if !nonblock => {
                if let Err(e) = wait_socket() {
                    return e;
                }
            }
            Err(e) => return e,
        }
    }
}

pub fn sys_accept4(fd: u64, addr: u64, addr_len: u64, flags: u64) -> i64 {
    if flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        return EINVAL;
    }
    let (id, nonblock) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if is_inet(id) {
        return super::inet::accept(id, nonblock, addr, addr_len, flags);
    }
    if !unix::is_listening(id) {
        return EINVAL;
    }
    let accepted = loop {
        match unix::accept(id) {
            Ok(new) => break new,
            Err(unix::EAGAIN) if !nonblock => {
                if let Err(e) = wait_socket() {
                    return e;
                }
            }
            Err(e) => return e,
        }
    };
    let peer = unix::names(accepted).1;
    let fd = install_fd(OpenFile::Socket { id: accepted, nonblock: flags & SOCK_NONBLOCK != 0 });
    if fd >= 0 {
        let r = write_address(addr, addr_len, peer.as_deref());
        if r < 0 {
            crate::syscall::sys_close(fd as u64);
            return r;
        }
    }
    fd
}

fn gather_iov(iov: u64, count: u64) -> Result<Vec<u8>, i64> {
    if count > MAX_IOV {
        return Err(EINVAL);
    }
    let entries = user_slice(iov, count * 16)?.to_vec();
    let mut data = Vec::new();
    for chunk in entries.chunks_exact(16) {
        let base = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(chunk[8..16].try_into().unwrap());
        if len == 0 {
            continue;
        }
        if data.len() + len as usize > MAX_MSG {
            return Err(unix::EMSGSIZE);
        }
        data.extend_from_slice(user_slice(base, len)?);
    }
    Ok(data)
}

fn scatter_iov(iov: u64, count: u64, data: &[u8]) -> Result<(), i64> {
    if count > MAX_IOV {
        return Err(EINVAL);
    }
    let entries = user_slice(iov, count * 16)?.to_vec();
    let mut done = 0usize;
    for chunk in entries.chunks_exact(16) {
        if done >= data.len() {
            break;
        }
        let base = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(chunk[8..16].try_into().unwrap()) as usize;
        let n = len.min(data.len() - done);
        if n > 0 {
            user_slice(base, n as u64)?.copy_from_slice(&data[done..done + n]);
            done += n;
        }
    }
    Ok(())
}

fn iov_capacity(iov: u64, count: u64) -> Result<usize, i64> {
    if count > MAX_IOV {
        return Err(EINVAL);
    }
    let entries = user_slice(iov, count * 16)?;
    Ok(entries.chunks_exact(16).map(|c| u64::from_le_bytes(c[8..16].try_into().unwrap()) as usize).sum::<usize>().min(MAX_MSG))
}

fn collect_fds(control: u64, control_len: u64) -> Result<Vec<OpenFile>, i64> {
    let mut files = Vec::new();
    if control == 0 || control_len == 0 {
        return Ok(files);
    }
    let raw = user_slice(control, control_len.min(64 * 1024))?.to_vec();
    let mut off = 0usize;
    let fail = |files: Vec<OpenFile>, e: i64| {
        for f in files {
            f.release();
        }
        Err(e)
    };
    while off + 16 <= raw.len() {
        let len = u64::from_le_bytes(raw[off..off + 8].try_into().unwrap()) as usize;
        let level = u32::from_le_bytes(raw[off + 8..off + 12].try_into().unwrap()) as u64;
        let kind = u32::from_le_bytes(raw[off + 12..off + 16].try_into().unwrap());
        if len < 16 || off + len > raw.len() {
            return fail(files, EINVAL);
        }
        if level == SOL_SOCKET && kind == SCM_RIGHTS {
            for chunk in raw[off + 16..off + len].chunks_exact(4) {
                let fd = i32::from_le_bytes(chunk.try_into().unwrap());
                if fd < 0 || files.len() >= MAX_PASSED_FDS {
                    return fail(files, if fd < 0 { EBADF } else { EINVAL });
                }
                match task::with_current(|t| super::base::open_file_for(t, fd as u64)) {
                    Some(file) => files.push(file),
                    None => return fail(files, EBADF),
                }
            }
        }
        off += (len + 7) & !7;
    }
    Ok(files)
}

fn send_all(id: u32, nonblock: bool, data: &[u8], mut files: Vec<OpenFile>, to: Option<&str>, flags: u64) -> i64 {
    if super::netlink::is_netlink(id) {
        for file in files {
            file.release();
        }
        return super::netlink::handle(id, data).map(|n| n as i64).unwrap_or_else(|e| e);
    }
    let nonblock = nonblock || flags & MSG_DONTWAIT != 0;
    let stream = unix::kind(id) == Some(SOCK_STREAM);
    let mut sent = 0usize;
    let result = loop {
        match unix::send(id, &data[sent..], &mut files, to) {
            Ok(n) => {
                sent += n;
                if !stream || sent >= data.len() || nonblock {
                    break sent as i64;
                }
            }
            Err(unix::EAGAIN) if !nonblock => {
                if let Err(e) = wait_socket() {
                    break if sent > 0 { sent as i64 } else { e };
                }
            }
            Err(e) => break if sent > 0 { sent as i64 } else { e },
        }
    };
    for file in files {
        file.release();
    }
    if result == unix::EPIPE && flags & MSG_NOSIGNAL == 0 && crate::syscall::linux_abi() {
        super::signal::send(task::current_pid(), super::signal::SIGPIPE);
    }
    result
}

fn resolve_destination(addr: u64, len: u64) -> Result<Option<String>, i64> {
    if addr == 0 {
        return Ok(None);
    }
    read_address(addr, len)?.key().map(Some).ok_or(EINVAL)
}

pub fn sys_sendto(fd: u64, buf: u64, len: u64, flags: u64, addr: u64, addr_len: u64) -> i64 {
    let (id, nonblock) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let data = match user_slice(buf, len.min(MAX_MSG as u64)) {
        Ok(d) => d.to_vec(),
        Err(e) => return e,
    };
    if is_inet(id) {
        let dest = if addr == 0 {
            None
        } else {
            match super::inet::read_address(addr, addr_len) {
                Ok(d) => d,
                Err(e) => return e,
            }
        };
        return super::inet::send(id, nonblock, &data, dest, flags);
    }
    if super::netlink::is_netlink(id) {
        return super::netlink::handle(id, &data).map(|n| n as i64).unwrap_or_else(|e| e);
    }
    let to = match resolve_destination(addr, addr_len) {
        Ok(t) => t,
        Err(e) => return e,
    };
    send_all(id, nonblock, &data, Vec::new(), to.as_deref(), flags)
}

pub fn send_bytes(id: u32, nonblock: bool, data: &[u8]) -> i64 {
    if is_inet(id) {
        return super::inet::send(id, nonblock, data, None, 0);
    }
    send_all(id, nonblock, data, Vec::new(), None, 0)
}

pub fn sys_sendmsg(fd: u64, msg: u64, flags: u64) -> i64 {
    let (id, nonblock) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let header = match user_slice(msg, 56) {
        Ok(h) => h.to_vec(),
        Err(e) => return e,
    };
    let field = |o: usize| u64::from_le_bytes(header[o..o + 8].try_into().unwrap());
    let name = field(0);
    let name_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
    let data = match gather_iov(field(16), field(24)) {
        Ok(d) => d,
        Err(e) => return e,
    };
    if is_inet(id) {
        let dest = if name == 0 {
            None
        } else {
            match super::inet::read_address(name, name_len) {
                Ok(d) => d,
                Err(e) => return e,
            }
        };
        return super::inet::send(id, nonblock, &data, dest, flags);
    }
    if super::netlink::is_netlink(id) {
        return super::netlink::handle(id, &data).map(|n| n as i64).unwrap_or_else(|e| e);
    }
    let to = match resolve_destination(name, name_len) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let files = match collect_fds(field(32), field(40)) {
        Ok(f) => f,
        Err(e) => return e,
    };
    send_all(id, nonblock, &data, files, to.as_deref(), flags)
}

fn receive(id: u32, nonblock: bool, max: usize, flags: u64) -> Result<unix::Received, i64> {
    let nonblock = nonblock || flags & MSG_DONTWAIT != 0;
    let peek = flags & MSG_PEEK != 0;
    let mut first = loop {
        match unix::recv(id, max, peek) {
            Ok(r) => break r,
            Err(unix::EAGAIN) if !nonblock => {
                wait_socket()?;
            }
            Err(e) => return Err(e),
        }
    };
    if flags & MSG_WAITALL != 0 && !peek && unix::kind(id) == Some(SOCK_STREAM) && first.fds.is_empty() {
        while first.data.len() < max && !first.data.is_empty() {
            match unix::recv(id, max - first.data.len(), false) {
                Ok(more) if more.data.is_empty() => break,
                Ok(more) => {
                    first.data.extend_from_slice(&more.data);
                    first.fds.extend(more.fds);
                    if !first.fds.is_empty() {
                        break;
                    }
                }
                Err(unix::EAGAIN) => {
                    let _ = wait_socket();
                }
                Err(_) => break,
            }
        }
        first.full_len = first.data.len();
    }
    Ok(first)
}

pub fn recv_bytes(id: u32, nonblock: bool, buf: u64, count: u64) -> i64 {
    if is_inet(id) {
        return super::inet::recv_bytes(id, nonblock, buf, count);
    }
    match receive(id, nonblock, count.min(MAX_MSG as u64) as usize, 0) {
        Ok(r) => {
            for f in r.fds {
                f.release();
            }
            copy_out(buf, count, &r.data).min(count as i64)
        }
        Err(e) => e,
    }
}

pub fn sys_recvfrom(fd: u64, buf: u64, len: u64, flags: u64, addr: u64, addr_len: u64) -> i64 {
    let (id, nonblock) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = user_slice(buf, len) {
        return e;
    }
    if is_inet(id) {
        return super::inet::recvfrom(id, nonblock, buf, len, flags, addr, addr_len);
    }
    match receive(id, nonblock, len.min(MAX_MSG as u64) as usize, flags) {
        Ok(r) => {
            for f in r.fds {
                f.release();
            }
            let r2 = copy_out(buf, len, &r.data);
            if r2 < 0 {
                return r2;
            }
            if super::netlink::is_netlink(id) {
                if addr != 0 && addr_len != 0 {
                    let capacity = match user_slice(addr_len, 4) {
                        Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) as u64,
                        Err(e) => return e,
                    };
                    let mut raw = [0u8; 12];
                    super::netlink::kernel_address(&mut raw);
                    copy_out(addr, capacity.min(12), &raw);
                    copy_out(addr_len, 4, &12u32.to_le_bytes());
                }
            } else {
                let w = write_address(addr, addr_len, r.from.as_deref());
                if w < 0 {
                    return w;
                }
            }
            if flags & MSG_TRUNC != 0 { r.full_len as i64 } else { r.data.len() as i64 }
        }
        Err(e) => e,
    }
}

pub fn sys_recvmsg(fd: u64, msg: u64, flags: u64) -> i64 {
    let (id, nonblock) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let header = match user_slice(msg, 56) {
        Ok(h) => h.to_vec(),
        Err(e) => return e,
    };
    let field = |o: usize| u64::from_le_bytes(header[o..o + 8].try_into().unwrap());
    let (iov, iov_count, control, control_len) = (field(16), field(24), field(32), field(40));
    let capacity = match iov_capacity(iov, iov_count) {
        Ok(c) => c,
        Err(e) => return e,
    };
    if is_inet(id) {
        let mut update = [0u8; 56];
        update.copy_from_slice(&header);
        let (data, full) = match super::inet::recvmsg_fill(id, nonblock, capacity, flags & !MSG_CMSG_CLOEXEC, &mut update) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if let Err(e) = scatter_iov(iov, iov_count, &data) {
            return e;
        }
        let r = copy_out(msg, 56, &update);
        if r < 0 {
            return r;
        }
        return if flags & MSG_TRUNC != 0 { full as i64 } else { data.len() as i64 };
    }
    let received = match receive(id, nonblock, capacity, flags & !MSG_CMSG_CLOEXEC) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if let Err(e) = scatter_iov(iov, iov_count, &received.data) {
        for f in received.fds {
            f.release();
        }
        return e;
    }
    let mut msg_flags = 0u32;
    if received.full_len > received.data.len() {
        msg_flags |= MSG_TRUNC as u32;
    }
    let mut control_used = 0u64;
    if !received.fds.is_empty() {
        let fit = if control_len >= 16 { ((control_len - 16) / 4) as usize } else { 0 };
        let mut files = received.fds;
        let extra: Vec<OpenFile> = if files.len() > fit { files.split_off(fit) } else { Vec::new() };
        if !extra.is_empty() {
            msg_flags |= MSG_CTRUNC;
        }
        for f in extra {
            f.release();
        }
        let mut numbers: Vec<i32> = Vec::new();
        let mut failed = false;
        for file in files {
            if failed {
                file.release();
                continue;
            }
            let n = install_fd(file);
            if n < 0 {
                failed = true;
                msg_flags |= MSG_CTRUNC;
            } else {
                numbers.push(n as i32);
            }
        }
        if !numbers.is_empty() {
            let len = 16 + numbers.len() * 4;
            let mut raw = Vec::with_capacity((len + 7) & !7);
            raw.extend_from_slice(&(len as u64).to_le_bytes());
            raw.extend_from_slice(&(SOL_SOCKET as u32).to_le_bytes());
            raw.extend_from_slice(&SCM_RIGHTS.to_le_bytes());
            for n in &numbers {
                raw.extend_from_slice(&n.to_le_bytes());
            }
            let space = ((len + 7) & !7).min(control_len as usize);
            raw.resize(space, 0);
            let r = copy_out(control, raw.len() as u64, &raw);
            if r < 0 {
                for n in numbers {
                    crate::syscall::sys_close(n as u64);
                }
                return r;
            }
            control_used = raw.len() as u64;
        } else if failed {
            return EMFILE;
        }
    }
    let netlink = super::netlink::is_netlink(id);
    let creds = if netlink && super::netlink::passcred(id) { Some(unix::Cred::default()) } else if netlink { None } else { received.cred };
    if let (Some(sender), true) = (creds, control != 0) {
        if control_len >= control_used + 32 {
            let mut raw = [0u8; 32];
            raw[0..8].copy_from_slice(&28u64.to_le_bytes());
            raw[8..12].copy_from_slice(&(SOL_SOCKET as u32).to_le_bytes());
            raw[12..16].copy_from_slice(&SCM_CREDENTIALS.to_le_bytes());
            raw[16..20].copy_from_slice(&sender.pid.to_le_bytes());
            raw[20..24].copy_from_slice(&sender.uid.to_le_bytes());
            raw[24..28].copy_from_slice(&sender.gid.to_le_bytes());
            let r = copy_out(control + control_used, 32, &raw);
            if r < 0 {
                return r;
            }
            control_used += 32;
        } else {
            msg_flags |= MSG_CTRUNC;
        }
    }
    let mut update = [0u8; 56];
    update.copy_from_slice(&header);
    update[40..48].copy_from_slice(&control_used.to_le_bytes());
    update[48..52].copy_from_slice(&msg_flags.to_le_bytes());
    let name = field(0);
    if name != 0 && netlink {
        let cap = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
        let mut raw = [0u8; 12];
        super::netlink::kernel_address(&mut raw);
        if cap > 0 {
            copy_out(name, cap.min(12), &raw);
        }
        update[8..12].copy_from_slice(&12u32.to_le_bytes());
    } else if name != 0 {
        let cap = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
        let mut addr = Vec::new();
        addr.extend_from_slice(&(AF_UNIX as u16).to_le_bytes());
        if let Some(from) = &received.from {
            addr.extend_from_slice(from.as_bytes());
            if !from.starts_with('\0') {
                addr.push(0);
            }
        }
        if cap > 0 {
            copy_out(name, cap.min(addr.len() as u64), &addr);
        }
        update[8..12].copy_from_slice(&(addr.len() as u32).to_le_bytes());
    }
    let r = copy_out(msg, 56, &update);
    if r < 0 {
        return r;
    }
    if flags & MSG_TRUNC != 0 { received.full_len as i64 } else { received.data.len() as i64 }
}

pub fn sys_shutdown(fd: u64, how: u64) -> i64 {
    match socket_of(fd) {
        Ok((id, _)) if is_inet(id) => super::inet::shutdown(id, how),
        Ok((id, _)) => unix::shutdown(id, how).map(|_| 0).unwrap_or_else(|e| e),
        Err(e) => e,
    }
}

pub fn sys_getsockname(fd: u64, addr: u64, len: u64, peer: bool) -> i64 {
    let (id, _) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if super::netlink::is_netlink(id) {
        let mut raw = [0u8; 12];
        super::netlink::address(&mut raw);
        let capacity = match user_slice(len, 4) {
            Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) as u64,
            Err(e) => return e,
        };
        copy_out(addr, capacity.min(12), &raw);
        copy_out(len, 4, &12u32.to_le_bytes());
        return 0;
    }
    if is_inet(id) {
        return super::inet::names(id, addr, len, peer);
    }
    let (own, remote) = unix::names(id);
    if peer && !unix::is_connected(id) && remote.is_none() {
        return unix::ENOTCONN;
    }
    write_address(addr, len, if peer { remote.as_deref() } else { own.as_deref() })
}

pub fn sys_setsockopt(fd: u64, level: u64, name: u64, value: u64, len: u64) -> i64 {
    match socket_of(fd) {
        Ok((id, _)) if is_inet(id) => super::inet::setsockopt(id, level, name, value, len),
        Ok((id, _)) if level == SOL_SOCKET && name == SO_PASSCRED => {
            let on = match user_slice(value, 4) {
                Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) != 0,
                Err(e) => return e,
            };
            if super::netlink::is_netlink(id) {
                super::netlink::set_passcred(id, on);
            } else {
                unix::set_passcred(id, on);
            }
            0
        }
        Ok(_) => 0,
        Err(e) => e,
    }
}

pub fn sys_getsockopt(fd: u64, level: u64, name: u64, value: u64, len_ptr: u64) -> i64 {
    let (id, _) = match socket_of(fd) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if is_inet(id) {
        return super::inet::getsockopt(id, level, name, value, len_ptr);
    }
    if level != SOL_SOCKET {
        return ENOPROTOOPT;
    }
    let capacity = match user_slice(len_ptr, 4) {
        Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) as u64,
        Err(e) => return e,
    };
    let bytes: Vec<u8> = match name {
        SO_TYPE => (unix::kind(id).unwrap_or(0) as u32).to_le_bytes().to_vec(),
        SO_ERROR | SO_PROTOCOL => 0u32.to_le_bytes().to_vec(),
        SO_DOMAIN => (AF_UNIX as u32).to_le_bytes().to_vec(),
        SO_SNDBUF | SO_RCVBUF => (256u32 * 1024).to_le_bytes().to_vec(),
        SO_ACCEPTCONN => (unix::is_listening(id) as u32).to_le_bytes().to_vec(),
        SO_PEERCRED => {
            let c = unix::peer_cred(id);
            let mut raw = Vec::with_capacity(12);
            raw.extend_from_slice(&c.pid.to_le_bytes());
            raw.extend_from_slice(&c.uid.to_le_bytes());
            raw.extend_from_slice(&c.gid.to_le_bytes());
            raw
        }
        _ => return ENOPROTOOPT,
    };
    let n = (bytes.len() as u64).min(capacity);
    let r = copy_out(value, n, &bytes[..n as usize]);
    if r < 0 {
        return r;
    }
    copy_out(len_ptr, 4, &(n as u32).to_le_bytes()).min(0)
}

pub fn unlink_hook(path: &str) {
    if unix::is_bound_name(path) {
        unix::forget_name(path);
    }
}

pub fn is_socket_path(path: &str) -> bool {
    unix::is_bound_name(path)
}
