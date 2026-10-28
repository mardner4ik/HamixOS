use alloc::string::String;
use alloc::vec::Vec;

pub struct Uevent {
    pub action: String,
    pub devpath: String,
    pub properties: Vec<(String, String)>,
}

impl Uevent {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn subsystem(&self) -> &str {
        self.get("SUBSYSTEM").unwrap_or("")
    }

    pub fn parse(raw: &[u8]) -> Option<Uevent> {
        let mut fields = raw.split(|b| *b == 0).filter(|f| !f.is_empty()).map(|f| String::from_utf8_lossy(f).into_owned());
        let header = fields.next()?;
        let (action, devpath) = header.split_once('@')?;
        let properties = fields.filter_map(|f| f.split_once('=').map(|(k, v)| (String::from(k), String::from(v)))).collect();
        Some(Uevent { action: String::from(action), devpath: String::from(devpath), properties })
    }
}

#[cfg(target_os = "hamix")]
mod imp {
    use core::ffi::{c_int, c_void};

    use hxlibc_sys::libc;

    use super::Uevent;

    const AF_NETLINK: c_int = 16;
    const NETLINK_KOBJECT_UEVENT: c_int = 15;

    #[repr(C)]
    struct SockaddrNl {
        family: u16,
        pad: u16,
        pid: u32,
        groups: u32,
    }

    pub struct Monitor {
        fd: c_int,
    }

    impl Monitor {
        pub fn open() -> Option<Monitor> {
            let fd = unsafe { libc::socket(AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT) };
            if fd < 0 {
                return None;
            }
            let addr = SockaddrNl { family: AF_NETLINK as u16, pad: 0, pid: 0, groups: 1 };
            unsafe { libc::bind(fd, &addr as *const SockaddrNl as *const libc::sockaddr_un, core::mem::size_of::<SockaddrNl>() as u32) };
            Some(Monitor { fd })
        }

        pub fn fd(&self) -> u64 {
            self.fd as u64
        }

        pub fn next(&mut self) -> Option<Uevent> {
            let mut buf = [0u8; 4096];
            let n = unsafe { libc::recv(self.fd, buf.as_mut_ptr() as *mut c_void, buf.len(), libc::MSG_DONTWAIT) };
            if n <= 0 {
                return None;
            }
            Uevent::parse(&buf[..n as usize])
        }
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            unsafe { libc::close(self.fd) };
        }
    }
}

#[cfg(not(target_os = "hamix"))]
mod imp {
    use super::Uevent;

    pub struct Monitor;

    impl Monitor {
        pub fn open() -> Option<Monitor> {
            None
        }

        pub fn fd(&self) -> u64 {
            u64::MAX
        }

        pub fn next(&mut self) -> Option<Uevent> {
            None
        }
    }
}

pub use imp::Monitor;
