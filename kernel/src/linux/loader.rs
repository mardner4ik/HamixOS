use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::task::elf::{self, Elf, StackSpec, ET_DYN};
use crate::task::{Abi, UserImage};
use crate::arch::paging;
use crate::fs::{self, Vfs};

pub const MAIN_BIAS: u64 = paging::USER_BASE + 0x40_0000;
pub const INTERP_BIAS: u64 = paging::USER_BASE + (paging::USER_MMAP_BASE - paging::USER_BASE) / 2;

const MAX_LINK_HOPS: usize = 40;

pub const LINUX_ROOT: &str = "/linux";
const HAMIX_VIEW: &str = "hamix";
const SHARED: [&str; 9] = ["home", "root", "tmp", "run", "dev", "proc", "sys", "mnt", "media"];
const SHARED_ETC: [&str; 11] = ["passwd", "group", "shadow", "hostname", "hosts", "resolv.conf", "localtime", "timezone", "machine-id", "sudoers", "hamix"];

fn native_of(view: &[String]) -> String {
    match view.first().map(|s| s.as_str()) {
        None => String::from(LINUX_ROOT),
        Some(HAMIX_VIEW) => format!("/{}", view[1..].join("/")),
        Some(first) if SHARED.contains(&first) => format!("/{}", view.join("/")),
        Some("etc") if view.get(1).map(|n| SHARED_ETC.contains(&n.as_str())).unwrap_or(false) => format!("/{}", view.join("/")),
        Some(_) => format!("{}/{}", LINUX_ROOT, view.join("/")),
    }
}

pub fn to_native(linux_view: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in split_components(linux_view) {
        if component == ".." {
            parts.pop();
        } else {
            parts.push(component);
        }
    }
    native_of(&parts)
}

pub fn to_linux(native: &str) -> String {
    let parts: Vec<&str> = native.split('/').filter(|c| !c.is_empty()).collect();
    match parts.first().copied() {
        None => format!("/{}", HAMIX_VIEW),
        Some("linux") => format!("/{}", parts[1..].join("/")),
        Some(first) if SHARED.contains(&first) => format!("/{}", parts.join("/")),
        Some("etc") if parts.get(1).map(|n| SHARED_ETC.contains(n)).unwrap_or(false) => format!("/{}", parts.join("/")),
        Some(_) => format!("/{}/{}", HAMIX_VIEW, parts.join("/")),
    }
}

pub fn resolve_links_linux(vfs: &mut Vfs, path: &str, follow_last: bool) -> Result<String, &'static str> {
    let root = vfs.root_id();
    let mut pending = split_components(path);
    let mut resolved: Vec<String> = Vec::new();
    let mut hops = 0usize;
    while let Some(component) = pending.pop_front() {
        if component == ".." {
            resolved.pop();
            continue;
        }
        resolved.push(component);
        let candidate = native_of(&resolved);
        let Some(id) = vfs.resolve(root, &candidate) else {
            resolved.extend(pending.drain(..));
            break;
        };
        let last = pending.is_empty();
        let target = if !last || follow_last { vfs.link_target(id) } else { None };
        if let Some(target) = target {
            hops += 1;
            if hops > MAX_LINK_HOPS {
                return Err("too many levels of symbolic links");
            }
            resolved.pop();
            if target.starts_with('/') {
                resolved.clear();
            }
            let mut next = split_components(&target);
            next.extend(pending.drain(..));
            pending = next;
        }
    }
    Ok(native_of(&resolved))
}

pub fn follow_linux(path: &str, follow_last: bool) -> String {
    let mut guard = fs::VFS.lock();
    match guard.as_mut() {
        Some(vfs) => resolve_links_linux(vfs, path, follow_last).unwrap_or_else(|_| to_native(path)),
        None => to_native(path),
    }
}

fn split_components(path: &str) -> VecDeque<String> {
    path.split('/').filter(|c| !c.is_empty() && *c != ".").map(String::from).collect()
}

pub fn resolve_links(vfs: &mut Vfs, path: &str, follow_last: bool) -> Result<String, &'static str> {
    let root = vfs.root_id();
    let mut pending = split_components(path);
    let mut resolved: Vec<String> = Vec::new();
    let mut hops = 0usize;
    while let Some(component) = pending.pop_front() {
        if component == ".." {
            resolved.pop();
            continue;
        }
        let candidate = format!("/{}", resolved.iter().chain(core::iter::once(&component)).cloned().collect::<Vec<_>>().join("/"));
        let Some(id) = vfs.resolve(root, &candidate) else {
            resolved.push(component);
            resolved.extend(pending.drain(..));
            break;
        };
        let last = pending.is_empty();
        let target = if !last || follow_last { vfs.link_target(id) } else { None };
        match target {
            Some(target) => {
                hops += 1;
                if hops > MAX_LINK_HOPS {
                    return Err("too many levels of symbolic links");
                }
                let inside_linux = resolved.first().map(|c| c == "linux").unwrap_or(false);
                let mut next = if target.starts_with('/') {
                    resolved.clear();
                    split_components(&if inside_linux { to_native(&target) } else { target })
                } else {
                    split_components(&target)
                };
                next.extend(pending.drain(..));
                pending = next;
            }
            None => resolved.push(component),
        }
    }
    Ok(format!("/{}", resolved.join("/")))
}

pub fn follow(path: &str, follow_last: bool) -> String {
    let mut guard = fs::VFS.lock();
    match guard.as_mut() {
        Some(vfs) => resolve_links(vfs, path, follow_last).unwrap_or_else(|_| String::from(path)),
        None => String::from(path),
    }
}

fn read_interpreter(path: &str) -> Result<Vec<u8>, &'static str> {
    let mut guard = fs::VFS.lock();
    let vfs = guard.as_mut().ok_or("no filesystem mounted")?;
    let real = resolve_links(vfs, path, true)?;
    let id = vfs.resolve(vfs.root_id(), &real).ok_or("interpreter not found")?;
    if vfs.is_dir(id) {
        return Err("interpreter is a directory");
    }
    vfs.read(vfs.root_id(), &real).map_err(|_| "interpreter not found")
}

pub(crate) fn build(main: &Elf, spec: &StackSpec, abi: Abi) -> Result<UserImage, &'static str> {
    let main_bias = if main.header.e_type == ET_DYN { MAIN_BIAS } else { 0 };
    if main_bias == 0 && main.span().map(|(lo, _)| lo < paging::USER_BASE).unwrap_or(true) {
        return Err("static non-PIE Linux binaries are not supported (rebuild with -static-pie)");
    }
    let Some(interp_path) = main.interp()? else {
        return elf::assemble(main, main_bias, None, abi, spec);
    };
    if let Some((_, hi)) = main.span() {
        if main_bias.saturating_add(hi) > INTERP_BIAS {
            return Err("elf: executable too large");
        }
    }
    let interp_path = if abi == Abi::Linux { follow_linux(&interp_path, true) } else { interp_path };
    let data = read_interpreter(&interp_path)?;
    let interp = Elf::parse(&data)?;
    if interp.header.e_type != ET_DYN || interp.interp()?.is_some() {
        return Err("elf: interpreter must be a self-contained ET_DYN object");
    }
    crate::debug_println!("{}: {} via {}", abi.name(), spec.execfn, interp_path);
    elf::assemble(main, main_bias, Some((&interp, INTERP_BIAS)), abi, spec)
}
