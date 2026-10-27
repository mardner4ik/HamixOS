#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};
use hxproto::client::posix;

pub const DISPLAY: u32 = 1;

const AF_UNIX: c_int = 1;
const SOCK_STREAM: c_int = 1;
const SOCK_CLOEXEC: c_int = 0o2000000;
const SOL_SOCKET: c_int = 1;
const SCM_RIGHTS: c_int = 1;
const MSG_DONTWAIT: c_int = 0x40;
const MSG_NOSIGNAL: c_int = 0x4000;
const MFD_CLOEXEC: u32 = 1;
const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_SHARED: c_int = 1;
const EAGAIN: i32 = 11;
const EINTR: i32 = 4;

#[repr(C)]
struct Iovec {
    base: *mut c_void,
    len: usize,
}

#[repr(C)]
struct Msghdr {
    name: *mut c_void,
    namelen: u32,
    iov: *mut Iovec,
    iovlen: usize,
    control: *mut c_void,
    controllen: usize,
    flags: c_int,
}

unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn connect(fd: c_int, addr: *const posix::SockaddrUn, len: u32) -> c_int;
    fn sendmsg(fd: c_int, msg: *const Msghdr, flags: c_int) -> isize;
    fn recvmsg(fd: c_int, msg: *mut Msghdr, flags: c_int) -> isize;
    fn close(fd: c_int) -> c_int;
    fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    fn ftruncate(fd: c_int, len: i64) -> c_int;
    fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
}

pub fn socket_path() -> Option<String> {
    let display = posix::env("WAYLAND_DISPLAY").filter(|d| !d.is_empty()).unwrap_or_else(|| String::from("wayland-0"));
    if display.starts_with('/') {
        return Some(display);
    }
    let runtime = posix::env("XDG_RUNTIME_DIR").filter(|d| !d.is_empty())?;
    Some(alloc::format!("{}/{}", runtime, display))
}

pub fn available() -> bool {
    socket_path().map(|p| posix::exists(&p)).unwrap_or(false)
}

pub fn now_ms() -> u64 {
    posix::now_ms()
}

pub use wlwire::{Message, Reader};

pub struct Event {
    pub object: u32,
    pub opcode: u16,
    pub body: Vec<u8>,
}

impl Event {
    pub fn reader(&self) -> Reader<'_> {
        Reader::new(&self.body)
    }
}

pub struct Connection {
    fd: c_int,
    next_id: u32,
    input: Vec<u8>,
    output: Vec<u8>,
    events: VecDeque<Event>,
    alive: bool,
}

impl Connection {
    pub fn connect() -> Result<Connection, &'static str> {
        let path = socket_path().ok_or("XDG_RUNTIME_DIR is not set")?;
        let fd = unsafe { socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err("cannot create a socket");
        }
        let (addr, len) = posix::socket_address(&path);
        if unsafe { connect(fd, &addr, len) } < 0 {
            unsafe { close(fd) };
            return Err("cannot connect to the Wayland compositor");
        }
        Ok(Connection { fd, next_id: 2, input: Vec::new(), output: Vec::new(), events: VecDeque::new(), alive: true })
    }

    pub fn new_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }

    pub fn alive(&self) -> bool {
        self.alive
    }

    pub fn send(&mut self, bytes: Vec<u8>) {
        self.output.extend_from_slice(&bytes);
    }

    pub fn flush(&mut self) {
        while !self.output.is_empty() && self.alive {
            let mut iov = Iovec { base: self.output.as_mut_ptr() as *mut c_void, len: self.output.len() };
            let msg = Msghdr { name: core::ptr::null_mut(), namelen: 0, iov: &mut iov, iovlen: 1, control: core::ptr::null_mut(), controllen: 0, flags: 0 };
            let n = unsafe { sendmsg(self.fd, &msg, MSG_NOSIGNAL) };
            if n < 0 {
                match posix::errno() {
                    EINTR => continue,
                    EAGAIN => {
                        posix::wait_readable(&[], 1);
                        continue;
                    }
                    _ => {
                        self.alive = false;
                        self.output.clear();
                        return;
                    }
                }
            }
            self.output.drain(..n as usize);
        }
    }

    pub fn send_fd(&mut self, bytes: Vec<u8>, fd: i32) {
        self.flush();
        let mut data = bytes;
        let mut iov = Iovec { base: data.as_mut_ptr() as *mut c_void, len: data.len() };
        let mut control = [0u64; 3];
        let raw = control.as_mut_ptr() as *mut u8;
        unsafe {
            core::ptr::write_unaligned(raw as *mut u64, 20);
            core::ptr::write_unaligned(raw.add(8) as *mut i32, SOL_SOCKET);
            core::ptr::write_unaligned(raw.add(12) as *mut i32, SCM_RIGHTS);
            core::ptr::write_unaligned(raw.add(16) as *mut i32, fd);
        }
        let msg = Msghdr { name: core::ptr::null_mut(), namelen: 0, iov: &mut iov, iovlen: 1, control: raw as *mut c_void, controllen: 24, flags: 0 };
        if unsafe { sendmsg(self.fd, &msg, MSG_NOSIGNAL) } < 0 {
            self.alive = false;
        }
    }

    pub fn wait(&self, timeout_ms: i64) -> bool {
        posix::wait_readable(&[self.fd], timeout_ms).first().copied().unwrap_or(false)
    }

    pub fn read(&mut self) {
        let mut buf = [0u8; 8192];
        loop {
            let mut iov = Iovec { base: buf.as_mut_ptr() as *mut c_void, len: buf.len() };
            let mut control = [0u64; 16];
            let mut msg = Msghdr { name: core::ptr::null_mut(), namelen: 0, iov: &mut iov, iovlen: 1, control: control.as_mut_ptr() as *mut c_void, controllen: core::mem::size_of_val(&control), flags: 0 };
            let n = unsafe { recvmsg(self.fd, &mut msg, MSG_DONTWAIT) };
            if n == 0 {
                self.alive = false;
                break;
            }
            if n < 0 {
                if posix::errno() == EINTR {
                    continue;
                }
                break;
            }
            close_received(&control, msg.controllen);
            self.input.extend_from_slice(&buf[..n as usize]);
        }
        while self.input.len() >= wlwire::HEADER_SIZE {
            let Some(header) = wlwire::decode_header(&self.input) else {
                self.input.clear();
                break;
            };
            if self.input.len() < header.size {
                break;
            }
            let body = self.input[wlwire::HEADER_SIZE..header.size].to_vec();
            self.input.drain(..header.size);
            self.events.push_back(Event { object: header.object, opcode: header.opcode, body });
        }
    }

    pub fn next(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn dispatch_pending(&mut self, timeout_ms: i64) -> Option<Event> {
        if let Some(e) = self.events.pop_front() {
            return Some(e);
        }
        self.flush();
        self.read();
        if let Some(e) = self.events.pop_front() {
            return Some(e);
        }
        if timeout_ms != 0 && self.wait(timeout_ms) {
            self.read();
        }
        self.events.pop_front()
    }

    pub fn sync(&mut self) -> u32 {
        let id = self.new_id();
        self.send(Message::new(DISPLAY, 0).uint(id).finish());
        id
    }

    pub fn roundtrip(&mut self, mut handle: impl FnMut(&mut Connection, Event)) -> Result<(), &'static str> {
        let callback = self.sync();
        let deadline = now_ms() + 5000;
        loop {
            self.flush();
            while let Some(e) = self.events.pop_front() {
                if e.object == callback && e.opcode == 0 {
                    return Ok(());
                }
                handle(self, e);
            }
            if now_ms() > deadline {
                return Err("the compositor does not answer");
            }
            if !self.alive {
                return Err("the compositor closed the connection");
            }
            if self.wait(50) {
                self.read();
            }
        }
    }

    pub fn bind(&mut self, registry: u32, name: u32, interface: &str, version: u32) -> u32 {
        let id = self.new_id();
        self.send(Message::new(registry, 0).uint(name).string(interface).uint(version).uint(id).finish());
        id
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.flush();
        unsafe { close(self.fd) };
    }
}

fn close_received(control: &[u64; 16], len: usize) {
    let raw = unsafe { core::slice::from_raw_parts(control.as_ptr() as *const u8, len.min(128)) };
    let mut at = 0usize;
    while at + 16 <= raw.len() {
        let clen = u64::from_le_bytes(raw[at..at + 8].try_into().unwrap()) as usize;
        let level = i32::from_le_bytes(raw[at + 8..at + 12].try_into().unwrap());
        let kind = i32::from_le_bytes(raw[at + 12..at + 16].try_into().unwrap());
        if level == SOL_SOCKET && kind == SCM_RIGHTS && clen >= 16 {
            let mut p = at + 16;
            while p + 4 <= at + clen && p + 4 <= raw.len() {
                let fd = i32::from_le_bytes(raw[p..p + 4].try_into().unwrap());
                unsafe { close(fd) };
                p += 4;
            }
        }
        if clen < 16 {
            break;
        }
        at += (clen + 7) & !7;
    }
}

pub struct ShmBuffer {
    pub id: u32,
    pub pool: u32,
    addr: *mut u8,
    bytes: usize,
    pub width: u32,
    pub height: u32,
    pub busy: bool,
}

impl ShmBuffer {
    pub fn create(conn: &mut Connection, shm: u32, width: u32, height: u32, format: u32) -> Option<ShmBuffer> {
        let bytes = width as usize * height as usize * 4;
        if bytes == 0 {
            return None;
        }
        let name = b"wlclient\0";
        let fd = unsafe { memfd_create(name.as_ptr() as *const c_char, MFD_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        if unsafe { ftruncate(fd, bytes as i64) } < 0 {
            unsafe { close(fd) };
            return None;
        }
        let addr = unsafe { mmap(core::ptr::null_mut(), bytes, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) };
        if addr as usize == !0usize {
            unsafe { close(fd) };
            return None;
        }
        let pool = conn.new_id();
        conn.send_fd(Message::new(shm, 0).uint(pool).int(bytes as i32).finish(), fd);
        unsafe { close(fd) };
        let id = conn.new_id();
        conn.send(Message::new(pool, 0).uint(id).int(0).int(width as i32).int(height as i32).int(width as i32 * 4).uint(format).finish());
        Some(ShmBuffer { id, pool, addr: addr as *mut u8, bytes, width, height, busy: false })
    }

    pub fn pixels(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.addr as *mut u32, self.bytes / 4) }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u32 {
        self.addr as *mut u32
    }

    pub fn destroy(self, conn: &mut Connection) {
        conn.send(Message::new(self.id, 0).finish());
        conn.send(Message::new(self.pool, 1).finish());
        unsafe { munmap(self.addr as *mut c_void, self.bytes) };
    }
}

pub const PLAIN: &[u8; 58] = b"\x00\x001234567890-=\x08\tqwertyuiop[]\n\x00asdfghjkl;'`\x00\\zxcvbnm,./\x00*\x00 ";
pub const SHIFTED: &[u8; 58] = b"\x00\x00!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\x00ASDFGHJKL:\"~\x00|ZXCVBNM<>?\x00*\x00 ";

pub fn translate_key(key: u32, shift: bool) -> Option<i32> {
    match key {
        1 => Some(27),
        2..=57 => {
            let table = if shift { SHIFTED } else { PLAIN };
            let c = table[key as usize];
            if c == 0 { None } else { Some(c as i32) }
        }
        59..=68 => Some(-41 - (key as i32 - 59)),
        87 => Some(-51),
        88 => Some(-52),
        96 => Some(10),
        102 => Some(-5),
        103 => Some(-1),
        104 => Some(-8),
        105 => Some(-3),
        106 => Some(-4),
        107 => Some(-6),
        108 => Some(-2),
        109 => Some(-9),
        110 => Some(-53),
        111 => Some(-7),
        125 | 126 => Some(-21),
        _ => None,
    }
}
