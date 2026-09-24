use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::sys;
use crate::unix::{self, PollFd, POLLERR, POLLHUP, POLLIN};

pub const SYSROOT: &str = "/opt/linux";
pub const HELPER_FD: u64 = 3;
pub const MESSAGE_MAX: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    NotFound(String),
    Spawn(i64),
    Connect(i64),
    Io(i64),
    Closed,
    TooLarge,
    Timeout,
}

impl Error {
    pub fn as_str(&self) -> String {
        match self {
            Error::Unavailable => String::from("no Linux userland is installed under /opt/linux"),
            Error::NotFound(p) => format!("{} is not installed", p),
            Error::Spawn(e) => format!("cannot start the helper (error {})", e),
            Error::Connect(e) => format!("cannot reach the helper (error {})", e),
            Error::Io(e) => format!("bridge i/o failed (error {})", e),
            Error::Closed => String::from("the helper closed the connection"),
            Error::TooLarge => String::from("message is larger than the bridge allows"),
            Error::Timeout => String::from("the helper did not answer in time"),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.as_str())
    }
}

pub fn available() -> bool {
    sys::stat(SYSROOT).is_ok()
}

pub fn resolve(program: &str) -> Option<String> {
    if program.starts_with('/') {
        let direct = format!("{}{}", SYSROOT, program);
        if sys::stat(&direct).is_ok() {
            return Some(direct);
        }
        return if sys::stat(program).is_ok() { Some(program.to_string()) } else { None };
    }
    for dir in ["/usr/bin", "/bin", "/usr/sbin", "/sbin", "/usr/libexec"] {
        let path = format!("{}{}/{}", SYSROOT, dir, program);
        if sys::stat(&path).is_ok() {
            return Some(path);
        }
    }
    None
}

pub struct Buffer {
    fd: u64,
    len: u64,
    addr: *mut u8,
    owned: bool,
}

impl Buffer {
    pub fn create(name: &str, len: u64) -> Result<Buffer, Error> {
        let fd = unix::memfd_create(name);
        if fd < 0 {
            return Err(Error::Io(fd));
        }
        let fd = fd as u64;
        let r = unix::ftruncate(fd, len);
        if r < 0 {
            sys::close(fd);
            return Err(Error::Io(r));
        }
        match unix::map_shared(fd, len, 0) {
            Ok(addr) => Ok(Buffer { fd, len, addr, owned: true }),
            Err(e) => {
                sys::close(fd);
                Err(Error::Io(e))
            }
        }
    }

    pub fn adopt(fd: u64, len: u64) -> Result<Buffer, Error> {
        match unix::map_shared(fd, len, 0) {
            Ok(addr) => Ok(Buffer { fd, len, addr, owned: true }),
            Err(e) => {
                sys::close(fd);
                Err(Error::Io(e))
            }
        }
    }

    pub fn fd(&self) -> u64 {
        self.fd
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.addr, self.len as usize) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.addr, self.len as usize) }
    }

    pub fn pixels(&self) -> &[u32] {
        unsafe { core::slice::from_raw_parts(self.addr as *const u32, self.len as usize / 4) }
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.addr as *mut u32, self.len as usize / 4) }
    }

    pub fn into_fd(mut self) -> u64 {
        self.owned = false;
        let fd = self.fd;
        unix::unmap(self.addr, self.len);
        fd
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unix::unmap(self.addr, self.len);
        if self.owned {
            sys::close(self.fd);
        }
    }
}

pub struct Helper {
    pid: i64,
    sock: u64,
    name: String,
}

impl Helper {
    pub fn spawn(program: &str, args: &[&str]) -> Result<Helper, Error> {
        Helper::spawn_env(program, args, &[])
    }

    pub fn spawn_env(program: &str, args: &[&str], env: &[String]) -> Result<Helper, Error> {
        if !available() {
            return Err(Error::Unavailable);
        }
        let path = resolve(program).ok_or_else(|| Error::NotFound(program.to_string()))?;
        let (ours, theirs) = unix::socketpair(unix::SOCK_SEQPACKET).map_err(Error::Io)?;
        let mut full: Vec<String> = Vec::with_capacity(env.len() + 1);
        full.push(format!("HAMIX_BRIDGE_FD={}", HELPER_FD));
        full.extend(env.iter().cloned());
        let pid = sys::spawn_fds_env(&path, args, Some(&full), sys::SPAWN_DETACH, [None, None, None, Some(theirs)]);
        sys::close(theirs);
        if pid < 0 {
            sys::close(ours);
            return Err(Error::Spawn(pid));
        }
        Ok(Helper { pid, sock: ours, name: program.to_string() })
    }

    pub fn connect(path: &str) -> Result<Helper, Error> {
        let fd = unix::connect_to(path, unix::SOCK_SEQPACKET);
        if fd < 0 {
            return Err(Error::Connect(fd));
        }
        Ok(Helper { pid: -1, sock: fd as u64, name: path.to_string() })
    }

    pub fn adopt(sock: u64, name: &str) -> Helper {
        Helper { pid: -1, sock, name: name.to_string() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fd(&self) -> u64 {
        self.sock
    }

    pub fn pid(&self) -> i64 {
        self.pid
    }

    pub fn alive(&self) -> bool {
        self.pid < 0 || sys::proc_alive(self.pid)
    }

    pub fn send(&self, message: &[u8]) -> Result<(), Error> {
        self.send_with(message, &[])
    }

    pub fn send_with(&self, message: &[u8], fds: &[u64]) -> Result<(), Error> {
        if message.len() > MESSAGE_MAX {
            return Err(Error::TooLarge);
        }
        let n = unix::send_with_fds(self.sock, message, fds, 0);
        if n < 0 {
            return Err(Error::Io(n));
        }
        Ok(())
    }

    pub fn send_buffer(&self, message: &[u8], buffer: &Buffer) -> Result<(), Error> {
        self.send_with(message, &[buffer.fd()])
    }

    pub fn recv(&self, buf: &mut [u8]) -> Result<(usize, Vec<u64>), Error> {
        match unix::recv_with_fds(self.sock, buf, 0) {
            Ok((0, _)) => Err(Error::Closed),
            Ok(other) => Ok(other),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn wait(&self, timeout_ms: i32) -> Result<bool, Error> {
        let mut fds = [PollFd { fd: self.sock as i32, events: POLLIN, revents: 0 }];
        let n = unix::poll(&mut fds, timeout_ms);
        if n < 0 {
            return Err(Error::Io(n));
        }
        if n == 0 {
            return Ok(false);
        }
        if fds[0].revents & (POLLERR | POLLHUP) != 0 && fds[0].revents & POLLIN == 0 {
            return Err(Error::Closed);
        }
        Ok(true)
    }

    pub fn recv_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<(usize, Vec<u64>), Error> {
        if !self.wait(timeout_ms)? {
            return Err(Error::Timeout);
        }
        self.recv(buf)
    }

    pub fn request(&self, message: &[u8], buf: &mut [u8], timeout_ms: i32) -> Result<(usize, Vec<u64>), Error> {
        self.send(message)?;
        self.recv_timeout(buf, timeout_ms)
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        unix::shutdown(self.sock, 2);
        sys::close(self.sock);
        if self.pid > 0 && sys::proc_alive(self.pid) {
            sys::kill(self.pid);
            sys::waitpid(self.pid, false);
        }
    }
}

pub fn inherited() -> Option<Helper> {
    let fd = match crate::env::var("HAMIX_BRIDGE_FD") {
        Some(v) => v.parse::<u64>().ok()?,
        None => return None,
    };
    Some(Helper::adopt(fd, "parent"))
}
