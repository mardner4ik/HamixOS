use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_long, c_void};

pub const AF_UNIX: c_int = 1;
pub const SOCK_DGRAM: c_int = 2;
pub const SOCK_NONBLOCK: c_int = 0o4000;
pub const SOCK_CLOEXEC: c_int = 0o2000000;
pub const SOL_SOCKET: c_int = 1;
pub const SO_PASSCRED: c_int = 16;
pub const SCM_CREDENTIALS: c_int = 2;
pub const MSG_DONTWAIT: c_int = 0x40;
pub const MSG_NOSIGNAL: c_int = 0x4000;
pub const POLLIN: i16 = 1;
pub const POLLOUT: i16 = 4;
pub const O_RDONLY: c_int = 0;
pub const O_RDWR: c_int = 2;
pub const O_CREAT: c_int = 0o100;
pub const O_EXCL: c_int = 0o200;
pub const O_CLOEXEC: c_int = 0o2000000;
pub const PROT_READ: c_int = 1;
pub const PROT_WRITE: c_int = 2;
pub const MAP_SHARED: c_int = 1;
pub const EEXIST: i32 = 17;
pub const EINTR: i32 = 4;
pub const CLOCK_MONOTONIC: c_int = 1;

#[repr(C)]
pub struct SockaddrUn {
    pub family: u16,
    pub path: [u8; 108],
}

#[repr(C)]
pub struct Iovec {
    pub base: *mut c_void,
    pub len: usize,
}

#[repr(C)]
pub struct Msghdr {
    pub name: *mut c_void,
    pub namelen: u32,
    pub iov: *mut Iovec,
    pub iovlen: usize,
    pub control: *mut c_void,
    pub controllen: usize,
    pub flags: c_int,
}

#[repr(C)]
pub struct Pollfd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: c_long,
}

unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const SockaddrUn, len: u32) -> c_int;
    fn sendto(fd: c_int, buf: *const c_void, len: usize, flags: c_int, addr: *const SockaddrUn, addrlen: u32) -> isize;
    fn recvmsg(fd: c_int, msg: *mut Msghdr, flags: c_int) -> isize;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, len: u32) -> c_int;
    fn poll(fds: *mut Pollfd, count: u64, timeout: c_int) -> c_int;
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, len: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, len: usize) -> isize;
    fn close(fd: c_int) -> c_int;
    fn unlink(path: *const c_char) -> c_int;
    fn mkdir(path: *const c_char, mode: u32) -> c_int;
    fn chmod(path: *const c_char, mode: u32) -> c_int;
    fn access(path: *const c_char, mode: c_int) -> c_int;
    fn shm_open(name: *const c_char, flags: c_int, mode: u32) -> c_int;
    fn shm_unlink(name: *const c_char) -> c_int;
    fn ftruncate(fd: c_int, len: i64) -> c_int;
    fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn getpid() -> c_int;
    fn geteuid() -> u32;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn getenv(name: *const c_char) -> *const c_char;
    fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
    fn __errno_location() -> *mut c_int;
}

pub fn errno() -> i32 {
    unsafe { *__errno_location() }
}

pub fn cstr(text: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(text.len() + 1);
    v.extend_from_slice(text.as_bytes());
    v.push(0);
    v
}

pub fn env(name: &str) -> Option<String> {
    let key = cstr(name);
    let p = unsafe { getenv(key.as_ptr() as *const c_char) };
    if p.is_null() {
        return None;
    }
    let mut len = 0;
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(p as *const u8, len) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

pub fn pid() -> i32 {
    unsafe { getpid() }
}

pub fn euid() -> u32 {
    unsafe { geteuid() }
}

pub fn alive(pid: i64) -> bool {
    pid > 0 && (unsafe { kill(pid as c_int, 0) } == 0 || errno() == 1)
}

pub fn now_ms() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };
    ts.sec as u64 * 1000 + ts.nsec as u64 / 1_000_000
}

pub fn sleep_ms(ms: u64) {
    unsafe { poll(core::ptr::null_mut(), 0, ms.min(i32::MAX as u64) as c_int) };
}

pub fn read_file(path: &str) -> Option<Vec<u8>> {
    let p = cstr(path);
    let fd = unsafe { open(p.as_ptr() as *const c_char, O_RDONLY | O_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = unsafe { read(fd, chunk.as_mut_ptr() as *mut c_void, chunk.len()) };
        if n < 0 && errno() == EINTR {
            continue;
        }
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n as usize]);
    }
    unsafe { close(fd) };
    Some(out)
}

pub fn read_text(path: &str) -> Option<String> {
    read_file(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn write_stderr(text: &str) {
    unsafe { write(2, text.as_ptr() as *const c_void, text.len()) };
}

pub fn exists(path: &str) -> bool {
    let p = cstr(path);
    unsafe { access(p.as_ptr() as *const c_char, 0) == 0 }
}

pub fn ensure_dir(dir: &str, mode: u32) {
    let mut partial = String::new();
    for part in dir.split('/').filter(|p| !p.is_empty()) {
        partial.push('/');
        partial.push_str(part);
        if !exists(&partial) {
            let p = cstr(&partial);
            unsafe {
                mkdir(p.as_ptr() as *const c_char, mode);
                chmod(p.as_ptr() as *const c_char, mode);
            }
        }
    }
}

pub fn socket_address(path: &str) -> (SockaddrUn, u32) {
    let mut addr = SockaddrUn { family: AF_UNIX as u16, path: [0; 108] };
    let bytes = path.as_bytes();
    let n = bytes.len().min(107);
    addr.path[..n].copy_from_slice(&bytes[..n]);
    (addr, (2 + n + 1) as u32)
}

pub fn dgram_socket() -> i32 {
    unsafe { socket(AF_UNIX, SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0) }
}

pub fn pass_credentials(fd: i32) {
    let on: c_int = 1;
    unsafe { setsockopt(fd, SOL_SOCKET, SO_PASSCRED, &on as *const c_int as *const c_void, 4) };
}

pub fn bind_path(fd: i32, path: &str) -> bool {
    let p = cstr(path);
    unsafe { unlink(p.as_ptr() as *const c_char) };
    let (addr, len) = socket_address(path);
    if unsafe { bind(fd, &addr, len) } < 0 {
        return false;
    }
    unsafe { chmod(p.as_ptr() as *const c_char, 0o777) };
    true
}

pub fn close_fd(fd: i32) {
    unsafe { close(fd) };
}

pub fn remove(path: &str) {
    let p = cstr(path);
    unsafe { unlink(p.as_ptr() as *const c_char) };
}

pub fn send_to(fd: i32, path: &str, data: &[u8]) -> isize {
    let (addr, len) = socket_address(path);
    unsafe { sendto(fd, data.as_ptr() as *const c_void, data.len(), MSG_NOSIGNAL, &addr, len) }
}

pub fn receive(fd: i32, buf: &mut [u8]) -> Option<(i64, usize)> {
    let mut control = [0u64; 16];
    let mut iov = Iovec { base: buf.as_mut_ptr() as *mut c_void, len: buf.len() };
    let mut msg = Msghdr { name: core::ptr::null_mut(), namelen: 0, iov: &mut iov, iovlen: 1, control: control.as_mut_ptr() as *mut c_void, controllen: core::mem::size_of_val(&control), flags: 0 };
    let n = unsafe { recvmsg(fd, &mut msg, MSG_DONTWAIT) };
    if n < 0 {
        return None;
    }
    let raw = unsafe { core::slice::from_raw_parts(control.as_ptr() as *const u8, msg.controllen.min(128)) };
    let mut sender = 0i64;
    let mut at = 0usize;
    while at + 16 <= raw.len() {
        let len = u64::from_le_bytes(raw[at..at + 8].try_into().unwrap()) as usize;
        let level = i32::from_le_bytes(raw[at + 8..at + 12].try_into().unwrap());
        let kind = i32::from_le_bytes(raw[at + 12..at + 16].try_into().unwrap());
        if level == SOL_SOCKET && kind == SCM_CREDENTIALS && at + 20 <= raw.len() {
            sender = i32::from_le_bytes(raw[at + 16..at + 20].try_into().unwrap()) as i64;
        }
        if len < 16 {
            break;
        }
        at += (len + 7) & !7;
    }
    Some((sender, n as usize))
}

pub fn wait_readable(fds: &[i32], timeout_ms: i64) -> Vec<bool> {
    let mut pfds: Vec<Pollfd> = fds.iter().map(|fd| Pollfd { fd: *fd, events: POLLIN, revents: 0 }).collect();
    let timeout = if timeout_ms < 0 { -1 } else { timeout_ms.min(i32::MAX as i64) as c_int };
    let r = unsafe { poll(pfds.as_mut_ptr(), pfds.len() as u64, timeout) };
    if r <= 0 {
        return alloc::vec![false; fds.len()];
    }
    pfds.iter().map(|p| p.revents != 0).collect()
}

pub fn wait_writable(fd: i32, timeout_ms: i64) -> bool {
    let mut pfd = Pollfd { fd, events: POLLOUT, revents: 0 };
    let timeout = if timeout_ms < 0 { -1 } else { timeout_ms.min(i32::MAX as i64) as c_int };
    unsafe { poll(&mut pfd, 1, timeout) > 0 && pfd.revents & POLLOUT != 0 }
}

pub struct SharedMemory {
    pub id: u32,
    pub addr: *mut u8,
    pub len: usize,
}

fn shm_name(id: u32) -> Vec<u8> {
    cstr(&alloc::format!("/hx-{}", id))
}

static NEXT_SHM: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0x800);

impl SharedMemory {
    pub fn create(size: usize) -> Option<SharedMemory> {
        let len = (size.max(1) + 4095) & !4095;
        for _ in 0..2048 {
            let counter = NEXT_SHM.fetch_add(1, core::sync::atomic::Ordering::Relaxed) & 0xfff;
            let id = ((pid() as u32) << 12) | counter;
            if id == 0 {
                continue;
            }
            let name = shm_name(id);
            let fd = unsafe { shm_open(name.as_ptr() as *const c_char, O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, 0o600) };
            if fd < 0 {
                if errno() == EEXIST {
                    continue;
                }
                return None;
            }
            let ok = unsafe { ftruncate(fd, len as i64) } == 0;
            let addr = if ok { unsafe { mmap(core::ptr::null_mut(), len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) } } else { !0usize as *mut c_void };
            unsafe { close(fd) };
            if addr as usize == !0usize {
                unsafe { shm_unlink(name.as_ptr() as *const c_char) };
                return None;
            }
            return Some(SharedMemory { id, addr: addr as *mut u8, len });
        }
        None
    }

}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        unsafe {
            munmap(self.addr as *mut c_void, self.len);
            let name = shm_name(self.id);
            shm_unlink(name.as_ptr() as *const c_char);
        }
    }
}
