use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_long};

const O_RDONLY: c_int = 0;
const O_CLOEXEC: c_int = 0o2000000;
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

pub fn wait_readable(fds: &[i32], timeout_ms: i64) -> Vec<bool> {
    let mut pollfds: Vec<Pollfd> = fds.iter().map(|&fd| Pollfd { fd, events: POLLIN, revents: 0 }).collect();
    let ptr = if pollfds.is_empty() { core::ptr::null_mut() } else { pollfds.as_mut_ptr() };
    unsafe { poll(ptr, pollfds.len() as u64, timeout_ms.clamp(-1, i32::MAX as i64) as i32) };
    pollfds.iter().map(|fd| fd.revents & (POLLIN | 0x8 | 0x10) != 0).collect()
}

pub fn wait_writable(fd: i32, timeout_ms: i32) -> bool {
    let mut pollfd = Pollfd { fd, events: POLLOUT, revents: 0 };
    (unsafe { poll(&mut pollfd, 1, timeout_ms) }) > 0 && pollfd.revents & (POLLOUT | 0x8 | 0x10) != 0
}
