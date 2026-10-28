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
            target
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Personality {
    Hamix,
    Linux,
    Unknown,
}

const PT_NOTE: u32 = 4;

fn elf_personality(head: &[u8]) -> Personality {
    if head.len() < 64 || &head[0..4] != b"\x7fELF" || head[4] != 2 {
        return Personality::Unknown;
    }
    let u16_at = |o: usize| head.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| head.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let u64_at = |o: usize| head.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()) as usize);
    let (Some(phoff), Some(phentsize), Some(phnum)) = (u64_at(32), u16_at(54), u16_at(56)) else {
        return Personality::Unknown;
    };
    for i in 0..phnum.min(64) {
        let ph = phoff + i * phentsize;
        if u32_at(ph) != Some(PT_NOTE) {
            continue;
        }
        let (Some(offset), Some(size)) = (u64_at(ph + 8), u64_at(ph + 32)) else {
            continue;
        };
        let Some(note) = head.get(offset..offset.saturating_add(size)) else {
            continue;
        };
        if note.windows(6).any(|w| w == b"Hamix\0") {
            return Personality::Hamix;
        }
    }
    Personality::Linux
}

pub fn personality(path: &str) -> Personality {
    let mut current = follow_links(path);
    for _ in 0..4 {
        let Some(head) = read_prefix(&current, 16384) else {
            return Personality::Unknown;
        };
        if head.starts_with(b"#!") {
            let line = head[2..].split(|b| *b == b'\n').next().unwrap_or(&[]);
            let text = String::from_utf8_lossy(line);
            let interpreter = text.trim().split_whitespace().next().unwrap_or("");
            if interpreter.is_empty() {
                return Personality::Unknown;
            }
            current = follow_links(interpreter);
            continue;
        }
        return elf_personality(&head);
    }
    Personality::Unknown
}
