use alloc::string::String;
use crate::{Event, Request, MESSAGE_MAX};

use super::posix;

const IPC_DIR: &str = "/run/hamix/ipc";
const SYSTEM_SERVICES: &str = "/run/hamix/services";
const ACTIVATORS: &str = "/run/hamix/activators";
const ACTIVATION_TIMEOUT_MS: u64 = 5000;

fn runtime_dir() -> String {
    match posix::env("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => dir,
        _ => alloc::format!("/tmp/runtime-{}", posix::euid()),
    }
}

fn read_pid(path: &str) -> Option<i64> {
    let pid: i64 = posix::read_text(path)?.trim().parse().ok()?;
    posix::alive(pid).then_some(pid)
}

fn registered(name: &str) -> Option<i64> {
    read_pid(&alloc::format!("{}/hamix-services/{}", runtime_dir(), name)).or_else(|| read_pid(&alloc::format!("{}/{}", SYSTEM_SERVICES, name)))
}

pub struct Connection {
    fd: i32,
    path: String,
    pub server: i64,
}

impl Connection {
    pub fn open() -> Option<Connection> {
        posix::ensure_dir(IPC_DIR, 0o1777);
        let fd = posix::dgram_socket();
        if fd < 0 {
            return None;
        }
        posix::pass_credentials(fd);
        let path = alloc::format!("{}/{}", IPC_DIR, posix::pid());
        if !posix::bind_path(fd, &path) {
            return None;
        }
        let mut conn = Connection { fd, path, server: 0 };
        conn.server = conn.lookup(crate::SERVICE)?;
        Some(conn)
    }

    fn lookup(&mut self, name: &str) -> Option<i64> {
        if let Some(pid) = registered(name) {
            return Some(pid);
        }
        let activator = read_pid(&alloc::format!("{}/{}", ACTIVATORS, name))?;
        self.send_raw(activator, alloc::format!("activate {}", name).as_bytes());
        let deadline = posix::now_ms() + ACTIVATION_TIMEOUT_MS;
        while posix::now_ms() < deadline {
            posix::sleep_ms(20);
            if let Some(pid) = registered(name) {
                return Some(pid);
            }
        }
        None
    }

    fn send_raw(&self, pid: i64, data: &[u8]) -> bool {
        posix::send_to(self.fd, &alloc::format!("{}/{}", IPC_DIR, pid), data) >= 0
    }

    pub fn send(&self, request: Request) -> bool {
        let mut buf = [0u8; MESSAGE_MAX];
        let len = request.encode(&mut buf);
        self.send_raw(self.server, &buf[..len])
    }

    pub fn server_alive(&self) -> bool {
        posix::alive(self.server)
    }

    pub fn poll_event(&self) -> Option<Event> {
        let mut buf = [0u8; MESSAGE_MAX + 64];
        while let Some((sender, n)) = posix::receive(self.fd, &mut buf) {
            if sender == self.server {
                if let Some(event) = Event::decode(&buf[..n]) {
                    return Some(event);
                }
            }
        }
        None
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }

    pub fn wait(&self, timeout_ms: i64) -> bool {
        posix::wait_readable(&[self.fd], timeout_ms)[0]
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        posix::close_fd(self.fd);
        posix::remove(&self.path);
    }
}

pub fn server_running() -> bool {
    registered(crate::SERVICE).is_some() || posix::exists(&alloc::format!("{}/{}", ACTIVATORS, crate::SERVICE))
}
