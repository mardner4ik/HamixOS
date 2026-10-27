use alloc::vec::Vec;

use crate::syscall::{copy_out, target, user_slice, Target, EBADF, EFAULT, EINVAL, EPERM, ESRCH};
use crate::fs;
use crate::task::{self, Pid};

const ENOTSUP: i64 = -95;
const RLIM_INFINITY: u64 = u64::MAX;

fn valid_fd(fd: u64) -> bool {
    !matches!(target(fd), Target::Bad)
}

pub fn sys_fsync(fd: u64) -> i64 {
    if !valid_fd(fd) {
        return EBADF;
    }
    fs::request_sync();
    0
}

pub fn sys_sync() -> i64 {
    fs::request_sync();
    0
}

pub fn sys_flock(fd: u64) -> i64 {
    if valid_fd(fd) { 0 } else { EBADF }
}

fn limit_for(resource: u64) -> Option<(u64, u64)> {
    Some(match resource {
        3 => (8 << 20, RLIM_INFINITY),
        6 => (4096, 4096),
        7 => (256, 256),
        0..=15 => (RLIM_INFINITY, RLIM_INFINITY),
        _ => return None,
    })
}

pub fn sys_prlimit64(pid: u64, resource: u64, _new: u64, old: u64) -> i64 {
    if pid != 0 && !task::exists(pid as Pid) {
        return ESRCH;
    }
    let Some((cur, max)) = limit_for(resource) else {
        return EINVAL;
    };
    if old != 0 {
        let mut raw = [0u8; 16];
        raw[0..8].copy_from_slice(&cur.to_le_bytes());
        raw[8..16].copy_from_slice(&max.to_le_bytes());
        if copy_out(old, 16, &raw) < 0 {
            return EFAULT;
        }
    }
    0
}

pub fn sys_getrusage(who: u64, usage: u64) -> i64 {
    let ticks = if who == 0 || who == 1 { task::with_current(|t| t.cpu_ticks) } else { 0 };
    let us = ticks * 1_000_000 / task::TICK_HZ;
    let mut raw = [0u8; 144];
    raw[0..8].copy_from_slice(&(us / 1_000_000).to_le_bytes());
    raw[8..16].copy_from_slice(&(us % 1_000_000).to_le_bytes());
    copy_out(usage, 144, &raw).min(0)
}

pub fn sys_times(buf: u64) -> i64 {
    let cpu = task::with_current(|t| t.cpu_ticks) * 100 / task::TICK_HZ;
    if buf != 0 {
        let mut raw = [0u8; 32];
        raw[0..8].copy_from_slice(&cpu.to_le_bytes());
        if copy_out(buf, 32, &raw) < 0 {
            return EFAULT;
        }
    }
    (task::ticks() * 100 / task::TICK_HZ) as i64
}

fn realtime_us() -> u64 {
    crate::drivers::rtc::boot_epoch() * 1_000_000 + task::uptime_ms() * 1000
}

pub fn sys_gettimeofday(tv: u64, tz: u64) -> i64 {
    if tv != 0 {
        let us = realtime_us();
        let mut raw = [0u8; 16];
        raw[0..8].copy_from_slice(&(us / 1_000_000).to_le_bytes());
        raw[8..16].copy_from_slice(&(us % 1_000_000).to_le_bytes());
        if copy_out(tv, 16, &raw) < 0 {
            return EFAULT;
        }
    }
    if tz != 0 && copy_out(tz, 8, &[0u8; 8]) < 0 {
        return EFAULT;
    }
    0
}

pub fn sys_time(tloc: u64) -> i64 {
    let secs = realtime_us() / 1_000_000;
    if tloc != 0 && copy_out(tloc, 8, &secs.to_le_bytes()) < 0 {
        return EFAULT;
    }
    secs as i64
}

pub fn sys_clock_getres(_clock: u64, res: u64) -> i64 {
    if res != 0 {
        let mut raw = [0u8; 16];
        raw[8..16].copy_from_slice(&1_000_000u64.to_le_bytes());
        if copy_out(res, 16, &raw) < 0 {
            return EFAULT;
        }
    }
    0
}

pub fn sys_setuid(uid: u64) -> i64 {
    let uid = uid as u32;
    task::with_current(|t| {
        if t.uid == 0 {
            t.uid = uid;
            t.ruid = uid;
            0
        } else if uid == t.ruid || uid == t.uid {
            t.uid = uid;
            0
        } else {
            EPERM
        }
    })
}

pub fn sys_setreuid(ruid: u64, euid: u64) -> i64 {
    let (cur_r, cur_e) = task::with_current(|t| (t.ruid, t.uid));
    let want_r = if ruid as u32 == u32::MAX { cur_r } else { ruid as u32 };
    let want_e = if euid as u32 == u32::MAX { cur_e } else { euid as u32 };
    if cur_e != 0 && !([cur_r, cur_e].contains(&want_r) && [cur_r, cur_e].contains(&want_e)) {
        return EPERM;
    }
    task::with_current(|t| {
        t.ruid = want_r;
        t.uid = want_e;
    });
    0
}

pub fn sys_setresuid(ruid: u64, euid: u64, _suid: u64) -> i64 {
    sys_setreuid(ruid, euid)
}

pub fn sys_getresuid(r: u64, e: u64, s: u64) -> i64 {
    let (ruid, uid) = task::with_current(|t| (t.ruid, t.uid));
    for (ptr, value) in [(r, ruid), (e, uid), (s, uid)] {
        if copy_out(ptr, 4, &value.to_le_bytes()) < 0 {
            return EFAULT;
        }
    }
    0
}

pub fn sys_getresgid(r: u64, e: u64, s: u64) -> i64 {
    let rgid = super::base::sys_getgid() as u32;
    let egid = super::base::sys_getegid() as u32;
    for (ptr, value) in [(r, rgid), (e, egid), (s, egid)] {
        if copy_out(ptr, 4, &value.to_le_bytes()) < 0 {
            return EFAULT;
        }
    }
    0
}

pub fn sys_setgid(_gid: u64) -> i64 {
    0
}

pub fn sys_setfsuid(_uid: u64) -> i64 {
    task::with_current(|t| t.uid) as i64
}

pub fn sys_setfsgid(_gid: u64) -> i64 {
    super::base::sys_getegid()
}

pub fn sys_setpgid(pid: u64, pgid: u64) -> i64 {
    let me = task::current_pid();
    let target_pid = if pid == 0 { me } else { pid as Pid };
    let group = if pgid == 0 { target_pid } else { pgid as Pid };
    let allowed = task::with_task(target_pid, |t| t.pid == me || t.parent == me).unwrap_or(false);
    if !allowed {
        return ESRCH;
    }
    task::with_task(target_pid, |t| t.pgid = group);
    0
}

pub fn sys_getpgid(pid: u64) -> i64 {
    let target_pid = if pid == 0 { task::current_pid() } else { pid as Pid };
    task::with_task(target_pid, |t| t.pgid as i64).unwrap_or(ESRCH)
}

pub fn sys_setsid() -> i64 {
    let me = task::current_pid();
    task::with_current(|t| t.pgid = me);
    me as i64
}

pub fn sys_getsid(pid: u64) -> i64 {
    let target_pid = if pid == 0 { task::current_pid() } else { pid as Pid };
    task::with_task(target_pid, |t| t.pgid as i64).unwrap_or(ESRCH)
}

pub fn sys_capget(_header: u64, data: u64) -> i64 {
    if data != 0 && copy_out(data, 24, &[0u8; 24]) < 0 {
        return EFAULT;
    }
    0
}

pub fn sys_getpriority() -> i64 {
    20
}

pub fn sys_sched_getparam(param: u64) -> i64 {
    copy_out(param, 4, &[0u8; 4]).min(0)
}

pub fn sys_sched_rr_get_interval(ts: u64) -> i64 {
    let mut raw = [0u8; 16];
    raw[8..16].copy_from_slice(&6_000_000u64.to_le_bytes());
    copy_out(ts, 16, &raw).min(0)
}

pub fn sys_prctl(option: u64, arg2: u64) -> i64 {
    match option {
        15 => {
            let name = match crate::syscall::user_cstr(arg2) {
                Ok(n) => n,
                Err(e) => return e,
            };
            let short: alloc::string::String = name.chars().take(15).collect();
            task::with_current_thread(|t| t.comm = Some(short));
            0
        }
        16 => {
            let name = task::with_current_thread(|t| t.comm.clone()).unwrap_or_else(|| {
                let full = task::with_current(|t| t.name.clone());
                full.rsplit('/').next().unwrap_or("").chars().take(15).collect()
            });
            let mut raw = [0u8; 16];
            let bytes = name.as_bytes();
            raw[..bytes.len().min(15)].copy_from_slice(&bytes[..bytes.len().min(15)]);
            copy_out(arg2, 16, &raw).min(0)
        }
        3 => 1,
        1 | 4 | 22 | 29 | 36 | 38 | 0x5356_4d41 => 0,
        2 | 37 | 39 | 23 => {
            if option == 2 && arg2 != 0 {
                return copy_out(arg2, 4, &[0u8; 4]).min(0);
            }
            0
        }
        _ => EINVAL,
    }
}

pub fn sys_mincore(addr: u64, len: u64, vec: u64) -> i64 {
    if addr & (crate::arch::paging::PAGE_SIZE - 1) != 0 {
        return EINVAL;
    }
    let page_size = crate::arch::paging::PAGE_SIZE;
    let pages = len.div_ceil(page_size);
    if pages == 0 {
        return 0;
    }
    if !crate::arch::paging::is_user_range(addr, pages * page_size) {
        return crate::syscall::ENOMEM;
    }
    let mut state = alloc::vec![0u8; pages as usize];
    for (i, slot) in state.iter_mut().enumerate() {
        let page = addr + i as u64 * page_size;
        let (present, lazy) = crate::task::with_current(|t| {
            t.aspace.as_ref().map(|a| (a.translate(page).is_some(), a.lazy_at(page).is_some())).unwrap_or((false, false))
        });
        if !present && !lazy {
            return crate::syscall::ENOMEM;
        }
        *slot = present as u8;
    }
    match user_slice(vec, pages) {
        Ok(out) => {
            out.copy_from_slice(&state);
            0
        }
        Err(e) => e,
    }
}

pub fn sys_get_mempolicy(mode: u64, nodemask: u64, maxnode: u64) -> i64 {
    if mode != 0 && copy_out(mode, 4, &[0u8; 4]) < 0 {
        return EFAULT;
    }
    if nodemask != 0 && maxnode >= 8 {
        let mut first = [0u8; 8];
        first[0] = 1;
        if copy_out(nodemask, 8, &first) < 0 {
            return EFAULT;
        }
    }
    0
}

pub fn sys_getcpu(cpu: u64, node: u64) -> i64 {
    let id = crate::arch::smp::cpu_id() as u32;
    if cpu != 0 && copy_out(cpu, 4, &id.to_le_bytes()) < 0 {
        return EFAULT;
    }
    if node != 0 && copy_out(node, 4, &[0u8; 4]) < 0 {
        return EFAULT;
    }
    0
}

pub fn sys_xattr_get() -> i64 {
    -61
}

pub fn sys_xattr_list() -> i64 {
    0
}

pub fn sys_xattr_set() -> i64 {
    ENOTSUP
}

pub fn sys_mremap(old: u64, old_size: u64, new_size: u64, flags: u64, new_addr: u64) -> i64 {
    const PAGE: u64 = 4096;
    if old & (PAGE - 1) != 0 || new_size == 0 {
        return EINVAL;
    }
    let old_len = old_size.div_ceil(PAGE) * PAGE;
    let new_len = new_size.div_ceil(PAGE) * PAGE;
    if flags & 2 != 0 {
        return EINVAL;
    }
    let _ = new_addr;
    if !crate::arch::paging::is_user_range(old, old_len.max(PAGE)) {
        return EFAULT;
    }
    let mut page = old;
    while page < old + old_len.max(PAGE) {
        let known = crate::task::with_current(|t| t.aspace.as_ref().map(|a| a.translate(page).is_some() || a.lazy_at(page).is_some()).unwrap_or(false));
        if !known {
            return EFAULT;
        }
        page += PAGE;
    }
    if new_len <= old_len {
        if new_len < old_len {
            crate::syscall::sys_munmap(old + new_len, old_len - new_len);
        }
        return old as i64;
    }
    if flags & 1 == 0 {
        return -12;
    }
    let fresh = super::base::sys_mmap(0, new_len, 3, 0x22, u64::MAX, 0);
    if fresh < 0 {
        return fresh;
    }
    let copy: Vec<u8> = match user_slice(old, old_len) {
        Ok(b) => b.to_vec(),
        Err(e) => {
            crate::syscall::sys_munmap(fresh as u64, new_len);
            return e;
        }
    };
    if copy_out(fresh as u64, old_len, &copy) < 0 {
        crate::syscall::sys_munmap(fresh as u64, new_len);
        return EFAULT;
    }
    crate::syscall::sys_munmap(old, old_len);
    fresh
}
