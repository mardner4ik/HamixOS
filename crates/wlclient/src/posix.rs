use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_long};

const O_RDONLY: c_int = 0;
const O_CLOEXEC: c_int = 0o2000000;
const O_NONBLOCK: c_int = 0o4000;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const EINTR: i32 = 4;
const POLLIN: i16 = 1;
const POLLOUT: i16 = 4;

#[repr(C)]
pub struct SockaddrUn {
    family: u16,
    path: [u8; 108],
}

#[repr(C)]
struct Pollfd {
    fd: c_int,
    events: i16,
    revents: i16,
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: c_long,
}

unsafe extern "C" {
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn poll(fds: *mut Pollfd, count: u64, timeout: c_int) -> c_int;
    fn getenv(name: *const c_char) -> *const c_char;
    fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
    fn __errno_location() -> *mut c_int;
    fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
}

pub fn errno() -> i32 {
    unsafe { *__errno_location() }
}

fn cstr(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);
    bytes
}

pub fn env(name: &str) -> Option<String> {
    let key = cstr(name);
    let value = unsafe { getenv(key.as_ptr() as *const c_char) };
    if value.is_null() {
        return None;
    }
    let mut len = 0;
    while unsafe { *value.add(len) } != 0 {
        len += 1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(value as *const u8, len) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

pub fn exists(path: &str) -> bool {
    let path = cstr(path);
    let fd = unsafe { open(path.as_ptr() as *const c_char, O_RDONLY | O_CLOEXEC) };
    if fd < 0 {
        return false;
    }
    unsafe { close(fd) };
    true
}

pub fn now_ms() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(1, &mut ts) };
    ts.sec as u64 * 1000 + ts.nsec as u64 / 1_000_000
}

pub fn socket_address(path: &str) -> (SockaddrUn, u32) {
    let mut addr = SockaddrUn { family: 1, path: [0; 108] };
    let bytes = path.as_bytes();
    let len = bytes.len().min(107);
    addr.path[..len].copy_from_slice(&bytes[..len]);
    (addr, (2 + len + 1) as u32)
}

pub fn set_nonblocking(fd: i32) -> bool {
    let flags = unsafe { fcntl(fd, F_GETFL) };
    flags >= 0 && unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) } == 0
}

pub fn wait_connection(fd: i32, writable: bool, timeout_ms: i64) -> bool {
    let mut events = POLLIN;
    if writable {
        events |= POLLOUT;
    }
    let deadline = (timeout_ms >= 0).then(|| now_ms().saturating_add(timeout_ms as u64));
    let mut attempted = false;
    loop {
        let timeout = match deadline {
            Some(deadline) if attempted => {
                let remaining = deadline.saturating_sub(now_ms());
                if remaining == 0 {
                    return false;
                }
                remaining.min(i32::MAX as u64) as i32
            }
            Some(_) => timeout_ms.min(i32::MAX as i64) as i32,
            None => -1,
        };
        let mut pollfd = Pollfd { fd, events, revents: 0 };
        let result = unsafe { poll(&mut pollfd, 1, timeout) };
        if result > 0 {
            return pollfd.revents & (POLLIN | POLLOUT | 0x8 | 0x10) != 0;
        }
        if result == 0 || errno() != EINTR {
            return false;
        }
        attempted = true;
    }
}

pub fn wait_writable(fd: i32, timeout_ms: i32) -> bool {
    let deadline = (timeout_ms >= 0).then(|| now_ms().saturating_add(timeout_ms as u64));
    let mut attempted = false;
    loop {
        let timeout = match deadline {
            Some(deadline) if attempted => {
                let remaining = deadline.saturating_sub(now_ms());
                if remaining == 0 {
                    return false;
                }
                remaining.min(i32::MAX as u64) as i32
            }
            Some(_) => timeout_ms,
            None => -1,
        };
        let mut pollfd = Pollfd { fd, events: POLLOUT, revents: 0 };
        let result = unsafe { poll(&mut pollfd, 1, timeout) };
        if result > 0 {
            return pollfd.revents & (POLLOUT | 0x8 | 0x10) != 0;
        }
        if result == 0 || errno() != EINTR {
            return false;
        }
        attempted = true;
    }
}
