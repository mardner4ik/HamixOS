use alloc::string::String;
use alloc::vec::Vec;

use crate::sys;

pub fn read(path: &str) -> Option<Vec<u8>> {
    let fd = sys::open_with(path, sys::O_RDONLY);
    if fd < 0 {
        return None;
    }
    let size = sys::fstat_size(fd as u64).max(0) as usize;
    let mut buffer = alloc::vec![0u8; size.max(64)];
    let mut total = 0usize;
    loop {
        if total == buffer.len() {
            buffer.resize(buffer.len() * 2, 0);
        }
        let n = sys::read(fd as u64, &mut buffer[total..]);
        if n <= 0 {
            break;
        }
        total += n as usize;
    }
    sys::close(fd as u64);
    buffer.truncate(total);
    Some(buffer)
}

pub fn read_prefix(path: &str, len: usize) -> Option<Vec<u8>> {
    let fd = sys::open_with(path, sys::O_RDONLY);
    if fd < 0 {
        return None;
    }
    let mut buffer = alloc::vec![0u8; len];
    let mut total = 0usize;
    while total < len {
        let n = sys::read(fd as u64, &mut buffer[total..]);
        if n <= 0 {
            break;
        }
        total += n as usize;
    }
    sys::close(fd as u64);
    buffer.truncate(total);
    Some(buffer)
}

pub fn append(path: &str, data: &[u8]) -> bool {
    let fd = sys::open_with(path, sys::O_WRONLY | sys::O_CREAT | sys::O_APPEND);
    if fd < 0 {
        return false;
    }
    let ok = sys::write(fd as u64, data) == data.len() as i64;
    sys::close(fd as u64);
    ok
}

pub fn read_to_string(path: &str) -> Option<String> {
    read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn write(path: &str, data: &[u8]) -> bool {
    let fd = sys::create(path);
    if fd < 0 {
        return false;
    }
    let mut done = 0usize;
    while done < data.len() {
        let n = sys::write(fd as u64, &data[done..]);
        if n <= 0 {
            sys::close(fd as u64);
            return false;
        }
        done += n as usize;
    }
    sys::close(fd as u64);
    true
}

pub fn exists(path: &str) -> bool {
    let fd = sys::open_with(path, sys::O_RDONLY);
    if fd >= 0 {
        sys::close(fd as u64);
        return true;
    }
    fd == -21
}

pub const LINK_MAGIC: &[u8] = b"\x7fHXLINK\n";
const LINUX_ROOT: &str = "/opt/linux";

pub fn link_target(path: &str) -> Option<String> {
    let head = read_prefix(path, LINK_MAGIC.len() + 4096)?;
    if head.len() > LINK_MAGIC.len() && head.starts_with(LINK_MAGIC) {
        Some(String::from_utf8_lossy(&head[LINK_MAGIC.len()..]).into_owned())
    } else {
        None
    }
}

pub fn follow_links(path: &str) -> String {
    let mut current = String::from(path);
    for _ in 0..16 {
        let Some(target) = link_target(&current) else {
            return current;
        };
        current = if target.starts_with('/') {
            if current.starts_with("/opt/linux/") && !target.starts_with("/opt/linux/") {
                alloc::format!("{}{}", LINUX_ROOT, target)
            } else {
                target
            }
        } else {
            let dir = match current.rfind('/') {
                Some(i) => &current[..i],
                None => "",
            };
            normalize(&alloc::format!("{}/{}", dir, target))
        };
    }
    current
}

pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut out = String::from("/");
    out.push_str(&parts.join("/"));
    out
}

pub fn read_following(path: &str) -> Option<Vec<u8>> {
    read(&follow_links(path))
}
