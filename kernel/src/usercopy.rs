use alloc::string::String;
use alloc::vec::Vec;

use crate::arch::paging::{self, PAGE_SIZE};
use crate::arch::pte;
use crate::errno::{EFAULT, ENAMETOOLONG};
use crate::task;

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".global hamix_copy_user",
    ".global hamix_copy_user_fault",
    ".global hamix_copy_user_end",
    "hamix_copy_user:",
    "mov rcx, rdx",
    "rep movsb",
    "xor eax, eax",
    "ret",
    "hamix_copy_user_fault:",
    "mov eax, 1",
    "ret",
    "hamix_copy_user_end:",
);

#[cfg(target_arch = "x86_64")]
unsafe extern "C" {
    fn hamix_copy_user(dst: *mut u8, src: *const u8, len: usize) -> u32;
    fn hamix_copy_user_fault();
    fn hamix_copy_user_end();
}

#[cfg(target_arch = "x86_64")]
pub fn fixup(ip: u64) -> Option<u64> {
    let start = hamix_copy_user as *const () as u64;
    let end = hamix_copy_user_end as *const () as u64;
    (ip >= start && ip < end).then(|| hamix_copy_user_fault as *const () as u64)
}

#[cfg(target_arch = "x86_64")]
fn copy_to_user(ptr: u64, data: &[u8]) -> bool {
    unsafe { hamix_copy_user(ptr as *mut u8, data.as_ptr(), data.len()) == 0 }
}

#[cfg(not(target_arch = "x86_64"))]
fn copy_to_user(ptr: u64, data: &[u8]) -> bool {
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len()) };
    true
}

fn locked<R>(f: impl FnOnce() -> R) -> R {
    if task::bkl::held() || task::current_pid() == 0 {
        return f();
    }
    task::bkl::acquire();
    let result = f();
    task::bkl::release();
    result
}

fn readable(ptr: u64, len: u64) -> bool {
    task::with_current(|t| t.aspace.as_ref().map(|a| a.is_readable(ptr, len)).unwrap_or(false))
}

fn check_readable(ptr: u64, len: u64) -> Result<(), i64> {
    if !paging::is_user_range(ptr, len) {
        return Err(EFAULT);
    }
    if !readable(ptr, len) && !locked(|| crate::syscall::base::fault_in_range(ptr, len) && readable(ptr, len)) {
        return Err(EFAULT);
    }
    Ok(())
}

fn writable(ptr: u64, len: u64) -> bool {
    task::with_current(|t| t.aspace.as_ref().map(|a| a.is_writable(ptr, len)).unwrap_or(false))
}

fn make_writable(ptr: u64, len: u64) -> Result<(), i64> {
    if !paging::is_user_range(ptr, len) {
        return Err(EFAULT);
    }
    if !writable(ptr, len) && !locked(|| crate::syscall::base::fault_in_range(ptr, len) && writable(ptr, len)) {
        return Err(EFAULT);
    }
    if !pte::COW_SUPPORTED {
        return Ok(());
    }
    let root = pte::read_root();
    let resolved = locked(|| {
        let mut page = ptr & !(PAGE_SIZE - 1);
        while page < ptr + len {
            if !crate::memory::aspace::resolve_cow(root, page) {
                return false;
            }
            page += PAGE_SIZE;
        }
        true
    });
    if !resolved {
        return Err(EFAULT);
    }
    Ok(())
}

pub(crate) fn user_slice<'a>(ptr: u64, len: u64) -> Result<&'a [u8], i64> {
    if len == 0 {
        return Ok(&[]);
    }
    check_readable(ptr, len)?;
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) })
}

pub(crate) fn user_slice_mut<'a>(ptr: u64, len: u64) -> Result<&'a mut [u8], i64> {
    if len == 0 {
        return Ok(&mut []);
    }
    make_writable(ptr, len)?;
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) })
}

pub fn user_stack_words(sp: u64, count: usize) -> Vec<u64> {
    let mut out = Vec::new();
    let mut at = sp & !7;
    while out.len() < count && paging::is_user_range(at, 8) {
        if !task::with_current(|t| t.aspace.as_ref().map(|a| a.is_mapped(at, 8)).unwrap_or(false)) {
            break;
        }
        out.push(unsafe { core::ptr::read_volatile(at as *const u64) });
        at += 8;
    }
    out
}

pub(crate) fn user_cstr(ptr: u64) -> Result<String, i64> {
    let mut out: Vec<u8> = Vec::new();
    let mut addr = ptr;
    loop {
        let page_end = (addr & !(PAGE_SIZE - 1)) + PAGE_SIZE;
        let chunk = user_slice(addr, page_end - addr)?;
        match chunk.iter().position(|b| *b == 0) {
            Some(n) => {
                out.extend_from_slice(&chunk[..n]);
                break;
            }
            None => out.extend_from_slice(chunk),
        }
        if out.len() > 4096 {
            return Err(ENAMETOOLONG);
        }
        addr = page_end;
    }
    match String::from_utf8(out) {
        Ok(text) => Ok(text),
        Err(e) => Ok(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

pub(crate) fn copy_out(ptr: u64, len: u64, data: &[u8]) -> i64 {
    let n = data.len().min(len as usize);
    if n == 0 {
        return match user_slice(ptr, len) {
            Ok(_) => data.len() as i64,
            Err(e) => e,
        };
    }
    if let Err(e) = make_writable(ptr, n as u64) {
        return e;
    }
    if copy_to_user(ptr, &data[..n]) { data.len() as i64 } else { EFAULT }
}
