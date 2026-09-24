use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::elf::{self, Elf, StackSpec, ET_DYN};
use super::{Abi, UserImage};
use crate::arch::paging;
use crate::fs::{self, Vfs};

pub const SYSROOT: &str = "/opt/linux";
pub const MAIN_BIAS: u64 = paging::USER_BASE + 0x40_0000;
pub const INTERP_BIAS: u64 = paging::USER_BASE + (paging::USER_MMAP_BASE - paging::USER_BASE) / 2;

pub fn sysroot_path(vfs: &mut Vfs, path: &str) -> Option<String> {
    if !path.starts_with('/') || path == SYSROOT || path.starts_with("/opt/linux/") {
        return None;
    }
    let candidate = format!("{}{}", SYSROOT, path);
    let root = vfs.root_id();
    if vfs.resolve(root, &candidate).is_some() {
        return Some(candidate);
    }
    if let Ok(real) = resolve_links(vfs, &candidate, true) {
        if vfs.resolve(root, &real).is_some() {
            return Some(candidate);
        }
    }
    if vfs.resolve(root, path).is_some() || vfs.resolve(root, parent_of(path)).is_some() {
        return None;
    }
    let parent = resolve_links(vfs, parent_of(&candidate), true).ok()?;
    if vfs.resolve(root, &parent).map(|id| vfs.is_dir(id)).unwrap_or(false) { Some(candidate) } else { None }
}

fn parent_of(path: &str) -> &str {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

pub fn resolve(path: &str) -> String {
    let mut guard = fs::VFS.lock();
    guard.as_mut().and_then(|v| sysroot_path(v, path)).unwrap_or_else(|| String::from(path))
}

const MAX_LINK_HOPS: usize = 40;

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
                let inside_sysroot = candidate.starts_with("/opt/linux/");
                let mut next = if target.starts_with('/') {
                    resolved.clear();
                    if inside_sysroot && !target.starts_with("/opt/linux/") {
                        split_components(&format!("{}{}", SYSROOT, target))
                    } else {
                        split_components(&target)
                    }
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
    let real = sysroot_path(vfs, path).unwrap_or_else(|| String::from(path));
    let real = resolve_links(vfs, &real, true)?;
    let id = vfs.resolve(vfs.root_id(), &real).ok_or("interpreter not found")?;
    if vfs.is_dir(id) {
        return Err("interpreter is a directory");
    }
    vfs.read(vfs.root_id(), &real).map_err(|_| "interpreter not found")
}

pub(super) fn build(main: &Elf, spec: &StackSpec) -> Result<UserImage, &'static str> {
    let main_bias = if main.header.e_type == ET_DYN { MAIN_BIAS } else { 0 };
    if main_bias == 0 && main.span().map(|(lo, _)| lo < paging::USER_BASE).unwrap_or(true) {
        return Err("static non-PIE Linux binaries are not supported (rebuild with -static-pie)");
    }
    let Some(interp_path) = main.interp()? else {
        return elf::assemble(main, main_bias, None, Abi::Linux, spec);
    };
    if let Some((_, hi)) = main.span() {
        if main_bias.saturating_add(hi) > INTERP_BIAS {
            return Err("elf: executable too large");
        }
    }
    let data = read_interpreter(&interp_path)?;
    let interp = Elf::parse(&data)?;
    if interp.header.e_type != ET_DYN || interp.interp()?.is_some() {
        return Err("elf: interpreter must be a self-contained ET_DYN object");
    }
    crate::debug_println!("linux: {} via {}", spec.execfn, interp_path);
    elf::assemble(main, main_bias, Some((&interp, INTERP_BIAS)), Abi::Linux, spec)
}
