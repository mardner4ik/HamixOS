use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::procfs::{self, Node};
use crate::syscall::{
    absolute, copy_out, euid, install_fd, is_tty, node_stat, open_path, sys_fstat, sys_read, target, term_size, user_cstr, user_slice, write_data, write_stat, LinuxStat, Target,
    EACCES, EAGAIN, EBADF, EEXIST, EINVAL, EISDIR, EMFILE, ENODEV, ENOENT, ENOMEM, ENOTDIR, ENOTTY, EPERM, O_ACCMODE,
};
use crate::arch::paging::{self, AddressSpace, PAGE_SIZE};
use crate::fs::{self, Vfs};
use crate::task::{self, linux_loader, pipe, Abi, DirEntry, OpenFile};

pub const AT_FDCWD: u64 = -100i64 as u64;
pub const AT_EMPTY_PATH: u64 = 0x1000;
pub const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
const AT_REMOVEDIR: u64 = 0x200;
const O_CLOEXEC_FLAG: u64 = 0o2000000;
const MAX_FDS: usize = 256;
const MAX_LINKS: usize = 8;
const ENOTEMPTY: i64 = -39;

const EINTR: i64 = -4;
const EDEADLK: i64 = -35;
const ESPIPE: i64 = -29;
const ETIMEDOUT: i64 = -110;

const MAP_SHARED: u64 = 0x01;
const MAP_FIXED: u64 = 0x10;
const MAP_ANONYMOUS: u64 = 0x20;
const MAP_FIXED_NOREPLACE: u64 = 0x10_0000;

const F_DUPFD: u64 = 0;
const F_GETFD: u64 = 1;
const F_SETFD: u64 = 2;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;
const F_DUPFD_CLOEXEC: u64 = 1030;
const F_GETLK: u64 = 5;
const F_SETLK: u64 = 6;
const F_SETLKW: u64 = 7;
const F_SETOWN: u64 = 8;
const F_GETOWN: u64 = 9;
const F_SETSIG: u64 = 10;
const F_GETSIG: u64 = 11;
const F_OFD_GETLK: u64 = 36;
const F_OFD_SETLK: u64 = 37;
const F_OFD_SETLKW: u64 = 38;
const F_NOTIFY: u64 = 1026;
const F_SETPIPE_SZ: u64 = 1031;
const F_GETPIPE_SZ: u64 = 1032;
const F_ADD_SEALS: u64 = 1033;
const F_GET_SEALS: u64 = 1034;
const MFD_ALLOWED: u64 = 1 | 2 | 4;

const TCGETS: u64 = 0x5401;
const TCSETS: u64 = 0x5402;
const TCSETSW: u64 = 0x5403;
const TCSETSF: u64 = 0x5404;
const TIOCGPGRP: u64 = 0x540F;
const TIOCSPGRP: u64 = 0x5410;
const TIOCGWINSZ: u64 = 0x5413;
const TIOCSWINSZ: u64 = 0x5414;
const FIONREAD: u64 = 0x541B;
const FIONBIO: u64 = 0x5421;
const FIONCLEX: u64 = 0x5450;
const FIOCLEX: u64 = 0x5451;

const ARCH_SET_GS: u64 = 0x1001;
const ARCH_SET_FS: u64 = 0x1002;
const ARCH_GET_FS: u64 = 0x1003;
const ARCH_GET_GS: u64 = 0x1004;

const FUTEX_WAIT: u64 = 0;
const FUTEX_REQUEUE: u64 = 3;
const FUTEX_CMP_REQUEUE: u64 = 4;
const FUTEX_WAKE_OP: u64 = 5;
const FUTEX_WAKE: u64 = 1;
const FUTEX_WAIT_BITSET: u64 = 9;
const FUTEX_WAKE_BITSET: u64 = 10;
const FUTEX_LOCK_PI: u64 = 6;
const FUTEX_UNLOCK_PI: u64 = 7;
const FUTEX_TRYLOCK_PI: u64 = 8;
const FUTEX_LOCK_PI2: u64 = 13;
const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;
const FUTEX_WAITERS: u32 = 0x8000_0000;
const FUTEX_OWNER_DIED: u32 = 0x4000_0000;

fn is_linux() -> bool {
    task::with_current(|t| t.abi) == Abi::Linux
}

pub fn guest_path(path: &str) -> String {
    guest_path_with(path, true)
}

fn guest_path_with(path: &str, follow_last: bool) -> String {
    if is_linux() {
        let mapped = linux_loader::resolve(path);
        linux_loader::follow(&mapped, follow_last)
    } else {
        String::from(path)
    }
}

fn raw_at_path(dirfd: u64, ptr: u64) -> Result<String, i64> {
    let path = user_cstr(ptr)?;
    if path.is_empty() {
        return Err(ENOENT);
    }
    if path.starts_with('/') || dirfd == AT_FDCWD {
        return Ok(absolute(&path));
    }
    match target(dirfd) {
        Target::Dir(base) => Ok(fs::absolute(&base, &path)),
        Target::Bad => Err(EBADF),
        _ => Err(ENOTDIR),
    }
}

fn at_path(dirfd: u64, ptr: u64) -> Result<String, i64> {
    raw_at_path(dirfd, ptr).map(|p| guest_path(&p))
}

fn at_path_nofollow(dirfd: u64, ptr: u64) -> Result<String, i64> {
    raw_at_path(dirfd, ptr).map(|p| guest_path_with(&p, false))
}

pub fn sys_openat(dirfd: u64, path_ptr: u64, flags: u64) -> i64 {
    match at_path(dirfd, path_ptr) {
        Ok(path) => open_path(&path, flags),
        Err(e) => e,
    }
}

fn proc_first(path: &str) -> bool {
    path.starts_with("/proc/") && (is_linux() || fs::VFS.lock().as_ref().map(|v| v.resolve(v.root_id(), path).is_none()).unwrap_or(true))
}

enum Found {
    Vfs(LinuxStat),
    Proc(procfs::Lookup),
}

fn find(path: &str, follow: bool) -> Result<(String, Found), i64> {
    let mut path = String::from(path);
    for _ in 0..MAX_LINKS {
        if proc_first(&path) {
            if let Some(found) = procfs::lookup(&path) {
                match &found.node {
                    Node::Link(target) if follow => {
                        path = guest_path(target);
                        continue;
                    }
                    _ => return Ok((path, Found::Proc(found))),
                }
            }
        }
        let guard = fs::VFS.lock();
        let vfs = guard.as_ref().ok_or(ENODEV)?;
        let id = vfs.resolve(vfs.root_id(), &path).ok_or(ENOENT)?;
        let st = node_stat(vfs, id);
        return Ok((path, Found::Vfs(st)));
    }
    Err(-40)
}

pub fn stat_path(path: &str, follow: bool) -> Result<LinuxStat, i64> {
    find(path, follow).map(|(_, found)| match found {
        Found::Vfs(st) => st,
        Found::Proc(p) => p.stat,
    })
}

pub fn vfs_dir_entries(vfs: &Vfs, id: usize) -> Vec<DirEntry> {
    let self_ino = id as u64 + 1;
    let parent_path = {
        let path = vfs.path_of(id);
        match path.rfind('/') {
            Some(0) | None => String::from("/"),
            Some(i) => String::from(&path[..i]),
        }
    };
    let parent_ino = vfs.resolve(vfs.root_id(), &parent_path).map(|p| p as u64 + 1).unwrap_or(self_ino);
    let mut entries = alloc::vec![
        DirEntry { name: String::from("."), ino: self_ino, kind: procfs::DT_DIR },
        DirEntry { name: String::from(".."), ino: parent_ino, kind: procfs::DT_DIR },
    ];
    if let Ok(children) = vfs.list_detailed(id, "") {
        for (name, child) in children {
            let st = node_stat(vfs, child);
            entries.push(DirEntry { name, ino: st.ino, kind: procfs::dirent_kind(st.mode) });
        }
    }
    entries
}

pub fn merge_proc_entries(path: &str, mut entries: Vec<DirEntry>) -> Vec<DirEntry> {
    if path == "/proc" {
        for extra in procfs::pid_entries() {
            if !entries.iter().any(|e| e.name == extra.name) {
                entries.push(extra);
            }
        }
    }
    entries
}

static SHM_NAMES: spin::Mutex<alloc::collections::BTreeMap<String, u32>> = spin::Mutex::new(alloc::collections::BTreeMap::new());

fn shm_name(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/opt/linux").unwrap_or(path).strip_prefix("/dev/shm/")?;
    (!rest.is_empty() && !rest.contains('/')).then_some(rest)
}

fn open_shm(name: &str, flags: u64) -> i64 {
    const O_CREAT: u64 = 0o100;
    const O_EXCL: u64 = 0o200;
    const O_TRUNC: u64 = 0o1000;
    let existing = crate::arch::without_interrupts(|| SHM_NAMES.lock().get(name).copied());
    let id = match existing {
        Some(_) if flags & O_CREAT != 0 && flags & O_EXCL != 0 => return EEXIST,
        Some(id) => id,
        None if flags & O_CREAT != 0 => {
            let id = crate::task::ipc::memfd_create(task::current_pid());
            crate::arch::without_interrupts(|| SHM_NAMES.lock().insert(String::from(name), id));
            id
        }
        None => return ENOENT,
    };
    if flags & O_TRUNC != 0 {
        crate::task::ipc::shm_set_len(id, 0);
    }
    crate::task::ipc::retain(id);
    let access = (flags & 3).min(2) as u8;
    let r = crate::syscall::install_fd(OpenFile::Memfd { id, pos: 0, name: alloc::format!("/dev/shm/{}", name), access });
    if r < 0 {
        crate::task::ipc::release(id);
    }
    r
}

fn unlink_shm(name: &str) -> Option<i64> {
    let id = crate::arch::without_interrupts(|| SHM_NAMES.lock().remove(name))?;
    crate::task::ipc::release(id);
    Some(0)
}

pub fn open_special(path: &str, flags: u64) -> Option<i64> {
    if let Some(name) = shm_name(path) {
        return Some(open_shm(name, flags));
    }
    if !proc_first(path) {
        return None;
    }
    let (real, found) = match find(path, true) {
        Ok((real, Found::Proc(found))) => (real, found),
        Ok((real, Found::Vfs(_))) if real != path => return Some(open_path(&real, flags)),
        Ok(_) => return None,
        Err(ENOENT) if procfs::lookup(path).is_some() => return Some(ENOENT),
        Err(_) => return None,
    };
    if flags & O_ACCMODE != 0 {
        return Some(EACCES);
    }
    Some(match found.node {
        Node::Dir(entries) => {
            let mut all = alloc::vec![
                DirEntry { name: String::from("."), ino: found.stat.ino, kind: procfs::DT_DIR },
                DirEntry { name: String::from(".."), ino: 1, kind: procfs::DT_DIR },
            ];
            all.extend(entries);
            install_fd(OpenFile::Dir { path: real, entries: Arc::new(all), pos: 0 })
        }
        Node::File(data) => {
            if flags & crate::syscall::O_DIRECTORY != 0 {
                ENOTDIR
            } else {
                install_fd(OpenFile::Mem { path: real, data: Arc::new(data), pos: 0 })
            }
        }
        Node::Link(_) => -40,
    })
}

pub fn sys_fstatat(dirfd: u64, path_ptr: u64, statbuf: u64, flags: u64) -> i64 {
    if flags & AT_EMPTY_PATH != 0 && matches!(user_cstr(path_ptr).as_deref(), Ok("")) {
        return sys_fstat(dirfd, statbuf);
    }
    let follow = flags & AT_SYMLINK_NOFOLLOW == 0;
    let path = match if follow { at_path(dirfd, path_ptr) } else { at_path_nofollow(dirfd, path_ptr) } {
        Ok(p) => p,
        Err(e) => return e,
    };
    match stat_path(&path, follow) {
        Ok(st) => write_stat(statbuf, &st),
        Err(e) => e,
    }
}

pub fn sys_lstat(path_ptr: u64, statbuf: u64) -> i64 {
    sys_fstatat(AT_FDCWD, path_ptr, statbuf, AT_SYMLINK_NOFOLLOW)
}

pub fn sys_faccessat(dirfd: u64, path_ptr: u64, mode: u64) -> i64 {
    let path = match at_path(dirfd, path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let uid = euid();
    let path = match find(&path, true) {
        Ok((real, Found::Vfs(_))) => real,
        Ok((_, Found::Proc(found))) => {
            return if mode & 2 != 0 { EACCES } else if mode & 1 != 0 && found.stat.mode & 0o111 == 0 { EACCES } else { 0 };
        }
        Err(e) => return e,
    };
    let guard = fs::VFS.lock();
    let Some(vfs) = guard.as_ref() else {
        return ENODEV;
    };
    let Some(id) = vfs.resolve(vfs.root_id(), &path) else {
        return ENOENT;
    };
    let exec_ok = uid == 0 || vfs.is_dir(id) || vfs.stat(id).mode & 0o111 != 0;
    if (mode & 4 != 0 && !vfs.can_read(id, uid)) || (mode & 2 != 0 && !vfs.can_write(id, uid)) || (mode & 1 != 0 && !exec_ok) {
        return EACCES;
    }
    0
}

pub fn sys_readlinkat(dirfd: u64, path_ptr: u64, buf: u64, len: u64) -> i64 {
    let raw = match raw_at_path(dirfd, path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if len == 0 {
        return EINVAL;
    }
    if raw.starts_with("/proc/") {
        if let Some(procfs::Lookup { node: Node::Link(target), .. }) = procfs::lookup(&raw) {
            let n = copy_out(buf, len.min(target.len() as u64), target.as_bytes());
            return if n < 0 { n } else { n.min(len as i64) };
        }
    }
    let path = guest_path_with(&raw, false);
    let target = {
        let mut guard = fs::VFS.lock();
        let Some(vfs) = guard.as_mut() else {
            return ENODEV;
        };
        match vfs.resolve(vfs.root_id(), &path) {
            Some(id) => vfs.link_target(id),
            None => return ENOENT,
        }
    };
    match target {
        Some(t) => {
            let n = copy_out(buf, len.min(t.len() as u64), t.as_bytes());
            if n < 0 { n } else { n.min(len as i64) }
        }
        None => EINVAL,
    }
}

pub fn sys_pread(fd: u64, buf: u64, count: u64, offset: u64) -> i64 {
    let node = match target(fd) {
        Target::File(node, _, _) => node,
        Target::Mem(_, data, _) => {
            let start = (offset as usize).min(data.len());
            return copy_out(buf, count, &data[start..]).min(count as i64);
        }
        Target::Memfd(id, _) => {
            let mut tmp = alloc::vec![0u8; count.min(64 << 20) as usize];
            return match crate::task::ipc::shm_io(id, offset, &mut tmp, false) {
                Ok(n) => copy_out(buf, n as u64, &tmp[..n]).min(n as i64),
                Err(e) => e,
            };
        }
        Target::Dir(_) => return EISDIR,
        Target::Bad => return EBADF,
        _ => return ESPIPE,
    };
    let out = match user_slice(buf, count.min(64 << 20)) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let mut guard = fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return ENODEV;
    };
    match vfs.read_node_at(node, offset as usize, out) {
        Ok(n) => n as i64,
        Err(_) => crate::syscall::EISDIR,
    }
}

pub fn sys_readv(fd: u64, iov: u64, count: u64) -> i64 {
    let entries = match user_slice(iov, count.saturating_mul(16)) {
        Ok(e) => e.to_vec(),
        Err(e) => return e,
    };
    let mut total = 0i64;
    for chunk in entries.chunks_exact(16) {
        let base = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(chunk[8..16].try_into().unwrap());
        if len == 0 {
            continue;
        }
        let n = sys_read(fd, base, len);
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
        if (n as u64) < len {
            break;
        }
    }
    total
}

pub fn sys_mprotect(addr: u64, len: u64) -> i64 {
    if addr & (PAGE_SIZE - 1) != 0 || !paging::is_user_range(addr, len) {
        return EINVAL;
    }
    0
}

pub(super) fn open_file_for(t: &task::Task, fd: u64) -> Option<OpenFile> {
    match t.fds.get(fd as usize) {
        Some(Some(file)) => Some(file.duplicate()),
        _ if fd < 3 && !t.kernel_thread => Some(OpenFile::Console),
        _ => None,
    }
}

fn dup_from(fd: u64, min: u64) -> i64 {
    let min = min as usize;
    if min >= MAX_FDS {
        return EINVAL;
    }
    let (result, unused) = task::with_current(|t| {
        let Some(file) = open_file_for(t, fd) else {
            return (EBADF, None);
        };
        if t.fds.len() < MAX_FDS && !(min..t.fds.len()).any(|i| t.fds[i].is_none() && i >= 3) {
            let grow = (t.fds.len() + 16).min(MAX_FDS);
            t.fds.resize_with(grow, || None);
        }
        match (min.max(0)..t.fds.len()).find(|i| t.fds[*i].is_none() && (*i >= 3 || !matches!(file, OpenFile::Console) && *i >= min)) {
            Some(slot) => {
                t.fds[slot] = Some(file);
                (slot as i64, None)
            }
            None => (EMFILE, Some(file)),
        }
    });
    if let Some(file) = unused {
        file.release();
    }
    result
}

pub fn sys_dup(fd: u64) -> i64 {
    let r = dup_from(fd, 3);
    if r >= 0 {
        crate::syscall::set_fd_access(r as u64, crate::syscall::fd_access(fd));
    }
    r
}

pub fn sys_dup3(old: u64, new: u64, flags: u64) -> i64 {
    if flags & !O_CLOEXEC_FLAG != 0 || old == new {
        return EINVAL;
    }
    let r = sys_dup2(old, new);
    if r >= 0 {
        crate::syscall::set_cloexec(new, flags & O_CLOEXEC_FLAG != 0);
    }
    r
}

pub fn sys_dup2(old: u64, new: u64) -> i64 {
    if new as usize >= MAX_FDS {
        return EBADF;
    }
    let (result, replaced, unused) = task::with_current(|t| {
        let Some(file) = open_file_for(t, old) else {
            return (EBADF, None, None);
        };
        if old == new {
            return (new as i64, None, Some(file));
        }
        if t.fds.len() <= new as usize {
            t.fds.resize_with(new as usize + 1, || None);
        }
        let replaced = t.fds[new as usize].replace(file);
        (new as i64, replaced, None)
    });
    for file in [replaced, unused].into_iter().flatten() {
        if file.release() {
            fs::request_sync();
        }
    }
    if result >= 0 && old != new {
        crate::syscall::set_cloexec(new, false);
        crate::syscall::set_fd_nonblock(new, crate::syscall::fd_nonblock(old));
        crate::syscall::set_fd_access(new, crate::syscall::fd_access(old));
    }
    result
}

pub fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> i64 {
    let kind = target(fd);
    if matches!(kind, Target::Bad) {
        return EBADF;
    }
    let nonblock = if crate::syscall::fd_nonblock(fd) { 0o4000 } else { 0 };
    match cmd {
        F_SETFL => {
            set_nonblock(fd, arg & 0o4000 != 0);
            0
        }
        F_ADD_SEALS | F_GET_SEALS if matches!(kind, Target::Memfd(..)) => 0,
        F_GETFD => task::with_current(|t| crate::syscall::fd_flag(&t.cloexec, fd)) as i64,
        F_SETFD => {
            crate::syscall::set_cloexec(fd, arg & 1 != 0);
            0
        }
        F_GETFL => nonblock | match kind {
            Target::File(_, _, append) => crate::syscall::fd_access(fd) as i64 | if append { 0o2000 } else { 0 },
            Target::Dir(_) => crate::syscall::O_DIRECTORY as i64,
            Target::Mem(..) => 0,
            Target::Socket(_, nonblock) => 2 | if nonblock { 0o4000 } else { 0 },
            Target::Memfd(..) => crate::syscall::fd_access(fd) as i64,
            Target::PipeRead(_) => 0,
            Target::PipeWrite(_) => 1,
            Target::Pty(..) | Target::PtyMaster(..) => 2,
            _ => 2,
        },
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let r = dup_from(fd, arg);
            if r >= 0 {
                crate::syscall::set_cloexec(r as u64, cmd == F_DUPFD_CLOEXEC);
                crate::syscall::set_fd_nonblock(r as u64, nonblock != 0);
                crate::syscall::set_fd_access(r as u64, crate::syscall::fd_access(fd));
            }
            r
        }
        F_GETLK => {
            if let Ok(b) = user_slice(arg, 2) {
                b.copy_from_slice(&2u16.to_le_bytes());
            }
            0
        }
        F_SETLK | F_SETLKW | F_OFD_GETLK | F_OFD_SETLK | F_OFD_SETLKW => 0,
        F_SETOWN | F_GETOWN | F_SETSIG | F_GETSIG | F_SETPIPE_SZ | F_NOTIFY => 0,
        F_GETPIPE_SZ => 65536,
        _ => EINVAL,
    }
}

pub(super) fn gid_for(uid: u32) -> u32 {
    gid_of(uid)
}

fn gid_of(uid: u32) -> u32 {
    crate::users::name_of(uid).and_then(|n| crate::users::find_by_name(&n)).map(|u| u.gid).unwrap_or(uid)
}

pub fn sys_getgid() -> i64 {
    gid_of(task::with_current(|t| t.ruid)) as i64
}

pub fn sys_getegid() -> i64 {
    gid_of(euid()) as i64
}

pub fn sys_getgroups(size: u64, list: u64) -> i64 {
    if size == 0 {
        return 1;
    }
    let r = copy_out(list, 4, &gid_of(euid()).to_le_bytes());
    if r < 0 { r } else { 1 }
}

fn read_u64(ptr: u64) -> Result<u64, i64> {
    user_slice(ptr, 8).map(|b| u64::from_le_bytes((&*b).try_into().unwrap()))
}

pub fn sys_futex(addr: u64, op: u64, val: u64, timeout: u64, addr2: u64, val3: u64) -> i64 {
    let leader = task::current_pid();
    match op & 0x7F {
        FUTEX_WAKE | FUTEX_WAKE_BITSET => task::wake_futex(leader, addr, (val as u32).min(i32::MAX as u32) as usize) as i64,
        FUTEX_REQUEUE | FUTEX_CMP_REQUEUE => {
            if op & 0x7F == FUTEX_CMP_REQUEUE {
                match user_slice(addr, 4) {
                    Ok(b) => {
                        if u32::from_le_bytes((&*b).try_into().unwrap()) != val3 as u32 {
                            return EAGAIN;
                        }
                    }
                    Err(e) => return e,
                }
            }
            let woken = task::wake_futex(leader, addr, val as usize);
            let moved = task::with_tasks(|tasks| {
                let mut moved = 0usize;
                for t in tasks.values_mut() {
                    if moved >= timeout as usize {
                        break;
                    }
                    if t.leader == leader && t.futex == addr {
                        t.futex = addr2;
                        moved += 1;
                    }
                }
                moved
            });
            (woken + moved) as i64
        }
        FUTEX_WAKE_OP => {
            let woken = task::wake_futex(leader, addr, val as usize);
            (woken + task::wake_futex(leader, addr2, timeout as usize)) as i64
        }
        cmd @ (FUTEX_WAIT | FUTEX_WAIT_BITSET) => {
            let current = match user_slice(addr, 4) {
                Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()),
                Err(e) => return e,
            };
            if current != val as u32 {
                return EAGAIN;
            }
            let deadline = if timeout == 0 {
                None
            } else {
                let (secs, nanos) = match (read_u64(timeout), read_u64(timeout + 8)) {
                    (Ok(s), Ok(n)) => (s, n),
                    (Err(e), _) | (_, Err(e)) => return e,
                };
                let mut ms = secs.saturating_mul(1000).saturating_add(nanos.div_ceil(1_000_000));
                if cmd == FUTEX_WAIT_BITSET {
                    let now = task::uptime_ms();
                    ms = ms.saturating_sub(if op & 256 != 0 { crate::drivers::rtc::boot_epoch() * 1000 + now } else { now });
                }
                Some(task::ticks() + task::ms_to_ticks(ms))
            };
            task::with_current_thread(|t| t.futex = addr);
            loop {
                let still = task::with_current_thread(|t| t.futex == addr);
                if !still {
                    return 0;
                }
                if super::signal::pending() {
                    task::with_current_thread(|t| t.futex = 0);
                    return EINTR;
                }
                let now = task::ticks();
                if let Some(d) = deadline {
                    if now >= d {
                        task::with_current_thread(|t| t.futex = 0);
                        return ETIMEDOUT;
                    }
                }
                let slice = deadline.map(|d| d - now).unwrap_or(task::TICK_HZ).clamp(1, task::TICK_HZ);
                task::block(task::WAIT_FUTEX, Some(slice), task::input_seq());
                task::check_killed();
            }
        }
        FUTEX_LOCK_PI | FUTEX_LOCK_PI2 | FUTEX_TRYLOCK_PI => {
            let tid = task::current_tid() as u32 & FUTEX_TID_MASK;
            let blocking = op & 0x7F != FUTEX_TRYLOCK_PI;
            let deadline = if blocking && timeout != 0 {
                match (read_u64(timeout), read_u64(timeout + 8)) {
                    (Ok(secs), Ok(nanos)) => Some(task::ticks() + task::ms_to_ticks(secs.saturating_mul(1000).saturating_add(nanos.div_ceil(1_000_000)))),
                    (Err(e), _) | (_, Err(e)) => return e,
                }
            } else {
                None
            };
            loop {
                let outcome = match user_slice(addr, 4) {
                    Ok(cell) => {
                        let current = u32::from_le_bytes((&*cell).try_into().unwrap());
                        let owner = current & FUTEX_TID_MASK;
                        if owner == 0 {
                            cell.copy_from_slice(&((current & FUTEX_OWNER_DIED) | tid).to_le_bytes());
                            Ok(true)
                        } else if owner == tid {
                            Err(EDEADLK)
                        } else if !blocking {
                            Err(EAGAIN)
                        } else {
                            cell.copy_from_slice(&(current | FUTEX_WAITERS).to_le_bytes());
                            Ok(false)
                        }
                    }
                    Err(e) => Err(e),
                };
                match outcome {
                    Ok(true) => return 0,
                    Err(e) => return e,
                    Ok(false) => {}
                }
                if super::signal::pending() {
                    task::with_current_thread(|t| t.futex = 0);
                    return EINTR;
                }
                let now = task::ticks();
                if let Some(d) = deadline {
                    if now >= d {
                        task::with_current_thread(|t| t.futex = 0);
                        return ETIMEDOUT;
                    }
                }
                task::with_current_thread(|t| t.futex = addr);
                let slice = deadline.map(|d| d - now).unwrap_or(task::TICK_HZ).clamp(1, task::TICK_HZ);
                task::block(task::WAIT_FUTEX, Some(slice), task::input_seq());
                task::with_current_thread(|t| t.futex = 0);
                task::check_killed();
            }
        }
        FUTEX_UNLOCK_PI => {
            let tid = task::current_tid() as u32 & FUTEX_TID_MASK;
            let waiters = match user_slice(addr, 4) {
                Ok(cell) => {
                    let current = u32::from_le_bytes((&*cell).try_into().unwrap());
                    if current & FUTEX_TID_MASK != tid {
                        return EPERM;
                    }
                    cell.copy_from_slice(&0u32.to_le_bytes());
                    current & FUTEX_WAITERS != 0
                }
                Err(e) => return e,
            };
            if waiters {
                task::wake_futex(leader, addr, 1);
            }
            0
        }
        _ => crate::syscall::ENOSYS,
    }
}

pub fn sys_getrandom(buf: u64, len: u64) -> i64 {
    let len = len.min(1 << 25);
    let mut done = 0u64;
    while done < len {
        let n = (len - done).min(PAGE_SIZE * 16);
        match user_slice(buf + done, n) {
            Ok(out) => crate::random::fill(out),
            Err(e) => return if done > 0 { done as i64 } else { e },
        }
        done += n;
    }
    len as i64
}

pub fn sys_ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    let request = request & 0xFFFF_FFFF;
    let kind = target(fd);
    if matches!(kind, Target::Bad) {
        return EBADF;
    }
    match request {
        FIONBIO => {
            let on = match user_slice(arg, 4) {
                Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) != 0,
                Err(e) => return e,
            };
            set_nonblock(fd, on);
            return 0;
        }
        FIONREAD if matches!(kind, Target::Socket(..)) => {
            let Target::Socket(id, _) = kind else { unreachable!() };
            let pending = if crate::net::inet::is_inet(id) { crate::net::inet::readiness(id).pending as u32 } else { crate::net::unix::readiness(id).pending as u32 };
            return copy_out(arg, 4, &pending.to_le_bytes()).min(0);
        }
        FIOCLEX | FIONCLEX => return 0,
        FIONREAD if matches!(kind, Target::Mem(..)) => {
            let Target::Mem(_, data, pos) = kind else { unreachable!() };
            return copy_out(arg, 4, &(data.len().saturating_sub(pos) as u32).to_le_bytes()).min(0);
        }
        FIONREAD => {
            let pending = match kind {
                Target::PipeRead(id) | Target::Pty(id, _) => pipe::available(id) as u64,
                Target::PtyMaster(_, id) => pipe::available(id) as u64,
                Target::File(node, pos, _) => fs::VFS.lock().as_ref().map(|v| v.node_size(node).saturating_sub(pos)).unwrap_or(0) as u64,
                _ => 0,
            };
            return copy_out(arg, 4, &(pending.min(i32::MAX as u64) as u32).to_le_bytes()).min(0);
        }
        _ => {}
    }
    if !is_tty(fd) {
        return ENOTTY;
    }
    match request {
        TCGETS => super::tty::get(arg),
        TCSETS | TCSETSW => super::tty::set(arg, false),
        TCSETSF => super::tty::set(arg, true),
        TIOCSPGRP => 0,
        TIOCSWINSZ => {
            let raw = match user_slice(arg, 8) {
                Ok(b) => b.to_vec(),
                Err(e) => return e,
            };
            let rows = u16::from_le_bytes([raw[0], raw[1]]);
            let cols = u16::from_le_bytes([raw[2], raw[3]]);
            match kind {
                Target::PipeRead(id) | Target::PipeWrite(id) => {
                    if pipe::set_terminal(id, cols, rows) {
                        crate::syscall::notify_resize(id);
                    }
                }
                Target::Pty(input, output) | Target::PtyMaster(input, output) => {
                    pipe::set_terminal(output, cols, rows);
                    if pipe::set_terminal(input, cols, rows) {
                        crate::syscall::notify_resize(input);
                    }
                }
                _ => {}
            }
            0
        }
        TIOCGWINSZ => {
            let (cols, rows) = term_size(fd).unwrap_or((80, 25));
            let mut raw = [0u8; 8];
            raw[0..2].copy_from_slice(&rows.to_le_bytes());
            raw[2..4].copy_from_slice(&cols.to_le_bytes());
            copy_out(arg, 8, &raw).min(0)
        }
        TIOCGPGRP => copy_out(arg, 4, &task::current_pid().to_le_bytes()).min(0),
        0x8004_5430 => {
            let index = task::with_current(|t| match t.fds.get(fd as usize) {
                Some(Some(OpenFile::PtyMaster { index, .. })) => Some(*index),
                _ => None,
            });
            match index {
                Some(i) => copy_out(arg, 4, &i.to_le_bytes()).min(0),
                None => ENOTTY,
            }
        }
        0x5441 => {
            let index = task::with_current(|t| match t.fds.get(fd as usize) {
                Some(Some(OpenFile::PtyMaster { index, .. })) => Some(*index),
                _ => None,
            });
            match index.and_then(|i| task::pty::open_slave(i).map(|(input, output)| (i, input, output))) {
                Some((index, input, output)) => crate::syscall::install_fd(OpenFile::PtySlave { index, input, output }),
                None => ENOTTY,
            }
        }
        0x4004_5431 | 0x540E | 0x5422 | 0x540B | 0x540A | 0x5409 | 0x5425 | 0x5427 => 0,
        0x5429 => copy_out(arg, 4, &task::with_current(|t| t.pgid).to_le_bytes()).min(0),
        0x5411 => copy_out(arg, 4, &0u32.to_le_bytes()).min(0),
        0x5420 => {
            let index = task::with_current(|t| match t.fds.get(fd as usize) {
                Some(Some(OpenFile::PtyMaster { index, .. })) => Some(*index),
                _ => None,
            });
            let on = match user_slice(arg, 4) {
                Ok(b) => u32::from_le_bytes((&*b).try_into().unwrap()) != 0,
                Err(e) => return e,
            };
            match index {
                Some(i) if task::pty::set_packet(i, on) => 0,
                _ => ENOTTY,
            }
        }
        _ => ENOTTY,
    }
}

pub fn sys_arch_prctl(code: u64, addr: u64) -> i64 {
    match code {
        ARCH_SET_FS => {
            if addr >= 0x0000_8000_0000_0000 {
                return EPERM;
            }
            task::with_current_thread(|t| t.fs_base = addr);
            task::set_fs_base(addr);
            0
        }
        ARCH_GET_FS => {
            let value = task::with_current_thread(|t| t.fs_base);
            copy_out(addr, 8, &value.to_le_bytes()).min(0)
        }
        ARCH_GET_GS => copy_out(addr, 8, &0u64.to_le_bytes()).min(0),
        ARCH_SET_GS => EINVAL,
        _ => EINVAL,
    }
}

fn any_mapped(aspace: &AddressSpace, start: u64, len: u64) -> bool {
    (0..len.div_ceil(PAGE_SIZE)).any(|i| aspace.occupied(start + i * PAGE_SIZE))
}

fn claim_range(t: &mut task::Task, start: u64, len: u64) {
    let end = start + len;
    if end <= paging::USER_MMAP_BASE || start >= paging::USER_SHM_BASE {
        return;
    }
    let mut kept = Vec::with_capacity(t.mmap_free.len() + 2);
    for &(s, l) in t.mmap_free.iter() {
        let e = s + l;
        if e <= start || s >= end {
            kept.push((s, l));
            continue;
        }
        if s < start {
            kept.push((s, start - s));
        }
        if e > end {
            kept.push((end, e - end));
        }
    }
    if end > t.mmap_next {
        let next = t.mmap_next.max(paging::USER_MMAP_BASE);
        if start > next {
            kept.push((next, start - next));
        }
        t.mmap_next = end + PAGE_SIZE;
    }
    kept.retain(|(_, l)| *l > PAGE_SIZE);
    t.mmap_free = kept;
}

fn find_free(t: &mut task::Task, size: u64) -> Option<u64> {
    let aspace = t.aspace.as_ref()?;
    let reuse = t.mmap_free.iter().position(|&(start, free)| free >= size + PAGE_SIZE && !any_mapped(aspace, start, size));
    if let Some(i) = reuse {
        let (start, free) = t.mmap_free[i];
        let used = size + PAGE_SIZE;
        if free == used {
            t.mmap_free.remove(i);
        } else {
            t.mmap_free[i] = (start + used, free - used);
        }
        return Some(start);
    }
    let base = t.mmap_next.max(paging::USER_MMAP_BASE);
    if base + size >= paging::USER_SHM_BASE {
        return None;
    }
    t.mmap_next = base + size + PAGE_SIZE;
    Some(base)
}

fn read_file(node: usize, offset: u64, len: u64) -> Result<Vec<u8>, i64> {
    let mut guard = fs::VFS.lock();
    let vfs = guard.as_mut().ok_or(ENODEV)?;
    if vfs.is_dir(node) {
        return Err(ENODEV);
    }
    let size = vfs.node_size(node) as u64;
    if offset >= size {
        return Ok(Vec::new());
    }
    let mut data = alloc::vec![0u8; (size - offset).min(len) as usize];
    let n = vfs.read_node_at(node, offset as usize, &mut data).map_err(|_| ENODEV)?;
    data.truncate(n);
    Ok(data)
}

const FAULT_AROUND: u64 = 16;
const ANON_FAULT_AROUND: u64 = 4;

pub fn lazy_fault(addr: u64) -> bool {
    let page = addr & !(PAGE_SIZE - 1);
    let Some((index, region)) = task::with_current(|t| t.aspace.as_ref().and_then(|a| a.lazy_at(page))) else {
        return false;
    };
    let limit = if region.node == paging::ANON_NODE { ANON_FAULT_AROUND } else { FAULT_AROUND };
    let mut count = 1u64;
    while count < limit {
        let next = page + count * PAGE_SIZE;
        let same = task::with_current(|t| t.aspace.as_ref().and_then(|a| a.lazy_at(next)).map(|(i, _)| i == index).unwrap_or(false));
        if !same {
            break;
        }
        count += 1;
    }
    let file_offset = region.offset + (page - region.start);
    let data = if region.node != paging::ANON_NODE && file_offset < region.file_len {
        read_file(region.node, file_offset, (count * PAGE_SIZE).min(region.file_len - file_offset)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let filled = task::with_current(|t| {
        let Some(aspace) = t.aspace.as_mut() else {
            return false;
        };
        let mut any = false;
        for i in 0..count {
            let from = ((i * PAGE_SIZE) as usize).min(data.len());
            let to = (((i + 1) * PAGE_SIZE) as usize).min(data.len());
            any |= aspace.fill_lazy(page + i * PAGE_SIZE, index, &data[from..to]);
        }
        any || aspace.translate(page).is_some()
    });
    paging::flush_tlb();
    filled
}

pub fn fault_in_range(addr: u64, len: u64) -> bool {
    let mut page = addr & !(PAGE_SIZE - 1);
    let end = addr.saturating_add(len.max(1));
    while page < end {
        let present = task::with_current(|t| t.aspace.as_ref().map(|a| a.translate(page).is_some()).unwrap_or(false));
        if !present && !lazy_fault(page) {
            return false;
        }
        page += PAGE_SIZE;
    }
    true
}

pub fn sys_mmap(addr: u64, len: u64, _prot: u64, flags: u64, fd: u64, offset: u64) -> i64 {
    if len == 0 || len > 1 << 36 || offset & (PAGE_SIZE - 1) != 0 {
        return EINVAL;
    }
    let size = (len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let fixed = flags & (MAP_FIXED | MAP_FIXED_NOREPLACE) != 0;
    let noreplace = flags & MAP_FIXED_NOREPLACE != 0;
    if fixed && addr & (PAGE_SIZE - 1) != 0 {
        return EINVAL;
    }
    if fixed && (addr < paging::USER_BASE || addr.saturating_add(size) > paging::USER_SHM_BASE) {
        return if noreplace { EEXIST } else { ENOMEM };
    }
    let mut lazy_file: Option<(usize, u64)> = None;
    let contents = if flags & MAP_ANONYMOUS != 0 {
        Vec::new()
    } else {
        match target(fd) {
            Target::File(node, _, _) => {
                let size = {
                    let guard = fs::VFS.lock();
                    match guard.as_ref() {
                        Some(v) if !v.is_dir(node) => v.node_size(node) as u64,
                        _ => return ENODEV,
                    }
                };
                lazy_file = Some((node, size));
                Vec::new()
            }
            Target::Memfd(id, _) if flags & MAP_SHARED != 0 => return map_memfd(id, fixed.then_some(addr), noreplace, offset, size),
            Target::Memfd(id, _) => {
                let mut data = alloc::vec![0u8; size as usize];
                match crate::task::ipc::shm_io(id, offset, &mut data, false) {
                    Ok(n) => data.truncate(n),
                    Err(e) => return e,
                }
                data
            }
            Target::Bad => return EBADF,
            _ => return ENODEV,
        }
    };
    let placed = task::with_current(|t| {
        let base = if fixed {
            let aspace = t.aspace.as_mut().ok_or(ENOMEM)?;
            if noreplace && any_mapped(aspace, addr, size) {
                return Err(EEXIST);
            }
            aspace.unmap_range(addr, size);
            claim_range(t, addr, size);
            addr
        } else {
            find_free(t, size).ok_or(ENOMEM)?
        };
        let aspace = t.aspace.as_mut().ok_or(ENOMEM)?;
        let ok = match lazy_file {
            Some((node, file_len)) => aspace.map_lazy(base, size, paging::LazyRegion { node, start: base, offset, file_len }),
            None => aspace.map_zero(base, size),
        };
        if !ok {
            aspace.unmap_range(base, size);
            return Err(ENOMEM);
        }
        Ok(base)
    });
    paging::flush_tlb();
    let base = match placed {
        Ok(base) => base,
        Err(e) => return e,
    };
    if !contents.is_empty() {
        match user_slice(base, contents.len() as u64) {
            Ok(out) => out.copy_from_slice(&contents),
            Err(e) => return e,
        }
    }
    base as i64
}

pub fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 {
    let taken = task::with_current(|t| match t.fds.get(fd as usize) {
        Some(Some(OpenFile::Dir { entries, pos, .. })) => Ok((entries.clone(), *pos)),
        Some(Some(_)) => Err(ENOTDIR),
        _ => Err(EBADF),
    });
    let (entries, start) = match taken {
        Ok(v) => v,
        Err(e) => return e,
    };
    let out = match user_slice(buf, count.min(1 << 20)) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let mut used = 0usize;
    let mut index = start;
    while let Some(entry) = entries.get(index) {
        let reclen = (19 + entry.name.len() + 1 + 7) & !7;
        if used + reclen > out.len() {
            if used == 0 {
                return EINVAL;
            }
            break;
        }
        let rec = &mut out[used..used + reclen];
        rec.fill(0);
        rec[0..8].copy_from_slice(&entry.ino.to_le_bytes());
        rec[8..16].copy_from_slice(&((index + 1) as u64).to_le_bytes());
        rec[16..18].copy_from_slice(&(reclen as u16).to_le_bytes());
        rec[18] = entry.kind;
        rec[19..19 + entry.name.len()].copy_from_slice(entry.name.as_bytes());
        used += reclen;
        index += 1;
    }
    task::with_current(|t| {
        if let Some(Some(OpenFile::Dir { pos, .. })) = t.fds.get_mut(fd as usize) {
            *pos = index;
        }
    });
    used as i64
}

pub fn sys_fchdir(fd: u64) -> i64 {
    match target(fd) {
        Target::Dir(path) => {
            let real = fs::VFS.lock().as_ref().map(|v| v.resolve(v.root_id(), &path).map(|id| v.is_dir(id)).unwrap_or(false)).unwrap_or(false);
            if !real {
                return EACCES;
            }
            task::with_current(|t| t.cwd = path);
            0
        }
        Target::Bad => EBADF,
        _ => ENOTDIR,
    }
}

fn vfs_result(result: Option<Result<(), &'static str>>) -> i64 {
    match result {
        Some(Ok(())) => {
            fs::request_sync();
            0
        }
        Some(Err(e)) => crate::syscall::fs_error(e),
        None => ENODEV,
    }
}

fn with_vfs<R>(f: impl FnOnce(&mut Vfs, u32) -> R) -> Option<R> {
    let uid = euid();
    fs::VFS.lock().as_mut().map(|v| f(v, uid))
}

pub fn sys_mkdirat(dirfd: u64, path_ptr: u64, mode: u64) -> i64 {
    let path = match at_path_nofollow(dirfd, path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let umask = task::with_current(|t| t.umask) as u64;
    vfs_result(with_vfs(|v, uid| {
        if v.exists(0, &path) {
            return Err("already exists");
        }
        v.mkdir(0, &path, uid)?;
        v.chmod(0, &path, uid, ((mode & !umask) & 0o7777) as u16)
    }))
}

pub fn sys_unlinkat(dirfd: u64, path_ptr: u64, flags: u64) -> i64 {
    let path = match at_path_nofollow(dirfd, path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let remove_dir = flags & AT_REMOVEDIR != 0;
    if let Some(name) = shm_name(&path) {
        if let Some(r) = unlink_shm(name) {
            return r;
        }
    }
    let outcome = with_vfs(|v, uid| {
        let Some(id) = v.resolve(0, &path) else {
            return Err(ENOENT);
        };
        match (v.is_dir(id), remove_dir) {
            (true, false) => return Err(EISDIR),
            (false, true) => return Err(ENOTDIR),
            (true, true) if !v.dir_is_empty(id) => return Err(ENOTEMPTY),
            (true, true) if id == v.root_id() => return Err(-16),
            _ => {}
        }
        v.remove(0, &path, uid).map_err(crate::syscall::fs_error)
    });
    if matches!(outcome, Some(Ok(()))) {
        super::socket::unlink_hook(&path);
    }
    match outcome {
        Some(Ok(())) => {
            fs::request_sync();
            0
        }
        Some(Err(e)) => e,
        None => ENODEV,
    }
}

pub fn sys_renameat(olddir: u64, old_ptr: u64, newdir: u64, new_ptr: u64, flags: u64) -> i64 {
    if flags & !1 != 0 {
        return EINVAL;
    }
    let (from, to) = match (at_path_nofollow(olddir, old_ptr), at_path_nofollow(newdir, new_ptr)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    vfs_result(with_vfs(|v, uid| {
        if flags & 1 != 0 && v.exists(0, &to) {
            return Err("already exists");
        }
        if let (Some(src), Some(dst)) = (v.resolve(0, &from), v.resolve(0, &to)) {
            if src != dst && !v.is_dir(src) && !v.is_dir(dst) {
                v.remove(0, &to, uid)?;
            }
        }
        v.rename(0, &from, &to, uid)
    }))
}

pub fn stat_at(dirfd: u64, path_ptr: u64, follow: bool) -> Result<LinuxStat, i64> {
    let path = if follow { at_path(dirfd, path_ptr)? } else { at_path_nofollow(dirfd, path_ptr)? };
    stat_path(&path, follow)
}

pub fn sys_linkat(olddirfd: u64, old_ptr: u64, newdirfd: u64, new_ptr: u64, flags: u64) -> i64 {
    const AT_SYMLINK_FOLLOW: u64 = 0x400;
    let old = match if flags & AT_SYMLINK_FOLLOW != 0 { at_path(olddirfd, old_ptr) } else { at_path_nofollow(olddirfd, old_ptr) } {
        Ok(p) => p,
        Err(e) => return e,
    };
    let new = match at_path_nofollow(newdirfd, new_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let outcome = with_vfs(|v, uid| {
        let Some(id) = v.resolve(0, &old) else {
            return Err(ENOENT);
        };
        if v.is_dir(id) {
            return Err(EPERM);
        }
        if v.exists(0, &new) {
            return Err(EEXIST);
        }
        let mode = v.stat(id).mode;
        let data = v.read(0, &old).map_err(|_| EACCES)?;
        v.create_file(0, &new, data, uid).map_err(crate::syscall::fs_error)?;
        v.chmod(0, &new, uid, mode).map_err(crate::syscall::fs_error)
    });
    match outcome {
        Some(Ok(())) => {
            fs::request_sync();
            0
        }
        Some(Err(e)) => e,
        None => ENODEV,
    }
}

pub fn sys_symlinkat(target_ptr: u64, newdirfd: u64, new_ptr: u64) -> i64 {
    let target = match user_cstr(target_ptr) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => return ENOENT,
        Err(e) => return e,
    };
    let new = match at_path_nofollow(newdirfd, new_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut body = Vec::with_capacity(fs::LINK_MAGIC.len() + target.len());
    body.extend_from_slice(fs::LINK_MAGIC);
    body.extend_from_slice(target.as_bytes());
    let outcome = with_vfs(|v, uid| {
        if v.exists(0, &new) {
            return Err(EEXIST);
        }
        v.create_file(0, &new, body, uid).map_err(crate::syscall::fs_error)?;
        v.chmod(0, &new, uid, 0o777).map_err(crate::syscall::fs_error)
    });
    match outcome {
        Some(Ok(())) => {
            fs::request_sync();
            0
        }
        Some(Err(e)) => e,
        None => ENODEV,
    }
}

fn node_path(fd: u64) -> Result<String, i64> {
    match target(fd) {
        Target::File(node, _, _) => fs::VFS.lock().as_ref().map(|v| v.path_of(node)).ok_or(ENODEV),
        Target::Dir(path) => Ok(path),
        Target::Bad => Err(EBADF),
        _ => Err(EPERM),
    }
}

pub fn sys_fchmodat(dirfd: u64, path_ptr: u64, mode: u64) -> i64 {
    match at_path(dirfd, path_ptr) {
        Ok(path) => vfs_result(with_vfs(|v, uid| v.chmod(0, &path, uid, (mode & 0o7777) as u16))),
        Err(e) => e,
    }
}

pub fn sys_fchmod(fd: u64, mode: u64) -> i64 {
    match node_path(fd) {
        Ok(path) => vfs_result(with_vfs(|v, uid| v.chmod(0, &path, uid, (mode & 0o7777) as u16))),
        Err(e) => e,
    }
}

fn chown_path(path: &str, owner: u64) -> i64 {
    if owner as u32 == u32::MAX {
        return if fs::VFS.lock().as_ref().map(|v| v.exists(0, path)).unwrap_or(false) { 0 } else { ENOENT };
    }
    vfs_result(with_vfs(|v, uid| v.chown(0, path, uid, owner as u32)))
}

pub fn sys_fchownat(dirfd: u64, path_ptr: u64, owner: u64) -> i64 {
    match at_path(dirfd, path_ptr) {
        Ok(path) => chown_path(&path, owner),
        Err(e) => e,
    }
}

pub fn sys_fchown(fd: u64, owner: u64) -> i64 {
    match node_path(fd) {
        Ok(path) => chown_path(&path, owner),
        Err(e) => e,
    }
}

pub fn sys_umask(mask: u64) -> i64 {
    task::with_current(|t| core::mem::replace(&mut t.umask, (mask & 0o777) as u32)) as i64
}

pub fn sys_utimensat(dirfd: u64, path_ptr: u64, flags: u64) -> i64 {
    if path_ptr == 0 {
        return match target(dirfd) {
            Target::Bad => EBADF,
            _ => 0,
        };
    }
    if flags & AT_EMPTY_PATH != 0 && matches!(user_cstr(path_ptr).as_deref(), Ok("")) {
        return match target(dirfd) {
            Target::Bad => EBADF,
            _ => 0,
        };
    }
    match at_path(dirfd, path_ptr) {
        Ok(path) => stat_path(&path, flags & AT_SYMLINK_NOFOLLOW == 0).map(|_| 0).unwrap_or_else(|e| e),
        Err(e) => e,
    }
}

pub fn sys_truncate(path_ptr: u64, len: u64) -> i64 {
    match at_path(AT_FDCWD, path_ptr) {
        Ok(path) => set_len_path(&path, len),
        Err(e) => e,
    }
}

pub fn sys_fallocate(fd: u64, mode: u64, offset: u64, len: u64) -> i64 {
    if mode != 0 {
        return -95;
    }
    let (offset, len) = (offset as i64, len as i64);
    if offset < 0 || len <= 0 {
        return EINVAL;
    }
    let end = (offset + len) as u64;
    match target(fd) {
        Target::File(node, _, _) => {
            let current = fs::VFS.lock().as_ref().map(|v| v.stat(node).size as u64).unwrap_or(0);
            if end <= current {
                return 0;
            }
            vfs_result(with_vfs(|v, uid| v.set_len(node, end as usize, uid)))
        }
        Target::Memfd(id, _) => {
            if crate::task::ipc::shm_len(id).map(|l| l >= end).unwrap_or(false) {
                return 0;
            }
            crate::task::ipc::shm_set_len(id, end)
        }
        Target::Bad => EBADF,
        _ => -19,
    }
}

pub fn sys_ftruncate(fd: u64, len: u64) -> i64 {
    match target(fd) {
        Target::File(node, _, _) => vfs_result(with_vfs(|v, uid| v.set_len(node, len as usize, uid))),
        Target::Memfd(id, _) => crate::task::ipc::shm_set_len(id, len),
        Target::Bad => EBADF,
        _ => EINVAL,
    }
}

fn set_len_path(path: &str, len: u64) -> i64 {
    if len > 1 << 32 {
        return -27;
    }
    let outcome = with_vfs(|v, uid| match v.resolve(0, path) {
        Some(id) if v.is_dir(id) => Err(EISDIR),
        Some(id) => v.set_len(id, len as usize, uid).map_err(|_| EACCES),
        None => Err(ENOENT),
    });
    match outcome {
        Some(Ok(())) => {
            fs::request_sync();
            0
        }
        Some(Err(e)) => e,
        None => ENODEV,
    }
}

pub fn sys_sendfile(out_fd: u64, in_fd: u64, offset_ptr: u64, count: u64) -> i64 {
    let explicit = if offset_ptr != 0 {
        match read_u64(offset_ptr) {
            Ok(v) => Some(v as usize),
            Err(e) => return e,
        }
    } else {
        None
    };
    let source = target(in_fd);
    let (mut pos, total) = match &source {
        Target::File(node, pos, _) => (explicit.unwrap_or(*pos), fs::VFS.lock().as_ref().map(|v| v.node_size(*node)).unwrap_or(0)),
        Target::Mem(_, data, pos) => (explicit.unwrap_or(*pos), data.len()),
        Target::Bad => return EBADF,
        _ => return EINVAL,
    };
    let limit = (count as usize).min(64 << 20);
    let mut sent = 0usize;
    let mut chunk = alloc::vec![0u8; 64 * 1024];
    while sent < limit && pos < total {
        let want = (limit - sent).min(chunk.len()).min(total - pos);
        let got = match &source {
            Target::File(node, _, _) => match fs::VFS.lock().as_mut().map(|v| v.read_node_at(*node, pos, &mut chunk[..want])) {
                Some(Ok(n)) => n,
                _ => break,
            },
            Target::Mem(_, data, _) => {
                chunk[..want].copy_from_slice(&data[pos..pos + want]);
                want
            }
            _ => break,
        };
        if got == 0 {
            break;
        }
        let written = write_data(out_fd, &chunk[..got]);
        if written < 0 {
            if sent == 0 {
                return written;
            }
            break;
        }
        let written = (written as usize).min(got);
        sent += written;
        pos += written;
        if written < got {
            break;
        }
    }
    if offset_ptr != 0 {
        copy_out(offset_ptr, 8, &(pos as u64).to_le_bytes());
    } else {
        task::with_current(|t| match t.fds.get_mut(in_fd as usize) {
            Some(Some(OpenFile::File { pos: p, .. })) | Some(Some(OpenFile::Mem { pos: p, .. })) => *p = pos,
            _ => {}
        });
    }
    sent as i64
}

fn read_timespec(ptr: u64) -> Result<u64, i64> {
    let secs = read_u64(ptr)? as i64;
    let nanos = read_u64(ptr + 8)? as i64;
    if secs < 0 || !(0..1_000_000_000).contains(&nanos) {
        return Err(EINVAL);
    }
    Ok((secs as u64).saturating_mul(1000).saturating_add((nanos as u64).div_ceil(1_000_000)))
}

fn clear_timespec(ptr: u64) {
    if ptr != 0 {
        copy_out(ptr, 16, &[0u8; 16]);
    }
}

fn write_remaining(rem: u64, ms: u64) {
    if rem != 0 {
        let mut raw = [0u8; 16];
        raw[0..8].copy_from_slice(&(ms / 1000).to_le_bytes());
        raw[8..16].copy_from_slice(&((ms % 1000) * 1_000_000).to_le_bytes());
        copy_out(rem, 16, &raw);
    }
}

pub fn sys_nanosleep(req: u64, rem: u64) -> i64 {
    match read_timespec(req) {
        Ok(ms) => {
            if ms == 0 {
                task::yield_now();
            } else if let Err(left) = crate::syscall::sleep_interruptible(ms) {
                write_remaining(rem, left);
                return EINTR;
            }
            clear_timespec(rem);
            0
        }
        Err(e) => e,
    }
}

fn clock_now_ms(clock: u64) -> u64 {
    let up = task::uptime_ms();
    if clock == 0 { crate::drivers::rtc::boot_epoch() * 1000 + up } else { up }
}

pub fn sys_clock_nanosleep(clock: u64, flags: u64, req: u64, rem: u64) -> i64 {
    let ms = match read_timespec(req) {
        Ok(ms) => ms,
        Err(e) => return e,
    };
    let ms = if flags & 1 != 0 { ms.saturating_sub(clock_now_ms(clock)) } else { ms };
    if ms > 0 {
        if let Err(left) = crate::syscall::sleep_interruptible(ms) {
            if flags & 1 == 0 {
                write_remaining(rem, left);
            }
            return EINTR;
        }
    }
    if flags & 1 == 0 {
        clear_timespec(rem);
    }
    0
}

pub fn sys_sysinfo(buf: u64) -> i64 {
    let (free, total) = crate::memory::frame::memory_info();
    let procs = task::list().len() as u16;
    let mut raw = [0u8; 112];
    raw[0..8].copy_from_slice(&(task::uptime_ms() / 1000).to_le_bytes());
    raw[32..40].copy_from_slice(&(total as u64).to_le_bytes());
    raw[40..48].copy_from_slice(&(free as u64).to_le_bytes());
    raw[56..64].copy_from_slice(&(fs::file_cache_bytes() as u64).to_le_bytes());
    raw[80..82].copy_from_slice(&procs.to_le_bytes());
    raw[104..108].copy_from_slice(&1u32.to_le_bytes());
    copy_out(buf, 112, &raw).min(0)
}

pub fn sys_sched_getaffinity(len: u64, mask: u64) -> i64 {
    if len < 8 {
        return EINVAL;
    }
    let cpus = crate::arch::smp::count().clamp(1, 64);
    let bits: u64 = if cpus == 64 { u64::MAX } else { (1u64 << cpus) - 1 };
    let r = copy_out(mask, 8, &bits.to_le_bytes());
    if r < 0 { r } else { 8 }
}

fn fill_statfs(buf: u64, path: &str) -> i64 {
    let (free, total) = crate::memory::frame::memory_info();
    let proc = path == "/proc" || path.starts_with("/proc/");
    let magic: u64 = if proc { 0x9FA0 } else { 0x4858_4654 };
    let mut raw = [0u8; 120];
    let blocks = if proc { 0 } else { total as u64 / PAGE_SIZE };
    let bfree = if proc { 0 } else { free as u64 / PAGE_SIZE };
    let fields: [(usize, u64); 9] = [(0, magic), (8, PAGE_SIZE), (16, blocks), (24, bfree), (32, bfree), (40, 65536), (48, 32768), (64, 255), (72, PAGE_SIZE)];
    for (off, value) in fields {
        raw[off..off + 8].copy_from_slice(&value.to_le_bytes());
    }
    copy_out(buf, 120, &raw).min(0)
}

pub fn sys_statfs(path_ptr: u64, buf: u64) -> i64 {
    let path = match at_path(AT_FDCWD, path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match stat_path(&path, true) {
        Ok(_) => fill_statfs(buf, &path),
        Err(e) => e,
    }
}

pub fn sys_fstatfs(fd: u64, buf: u64) -> i64 {
    match target(fd) {
        Target::Bad => EBADF,
        Target::Dir(path) | Target::Mem(path, _, _) => fill_statfs(buf, &path),
        _ => fill_statfs(buf, "/"),
    }
}

fn map_memfd(id: u32, fixed: Option<u64>, noreplace: bool, offset: u64, size: u64) -> i64 {
    let pid = task::current_pid();
    if let Some(addr) = fixed {
        let clash = task::with_current(|t| t.aspace.as_ref().map(|a| any_mapped(a, addr, size)).unwrap_or(false));
        if noreplace && clash {
            return EEXIST;
        }
        crate::task::ipc::unmap_range(pid, addr, size);
        task::with_current(|t| claim_range(t, addr, size));
    }
    match crate::task::ipc::shm_map_at(pid, id, fixed, offset, size) {
        Ok(base) => base as i64,
        Err(e) => e,
    }
}

fn set_nonblock(fd: u64, on: bool) {
    crate::syscall::set_fd_nonblock(fd, on);
    task::with_current(|t| {
        if let Some(Some(OpenFile::Socket { nonblock, .. })) = t.fds.get_mut(fd as usize) {
            *nonblock = on;
        }
    });
}

pub fn sys_memfd_create(name_ptr: u64, flags: u64) -> i64 {
    if flags & !MFD_ALLOWED != 0 {
        return EINVAL;
    }
    let name = match user_cstr(name_ptr) {
        Ok(n) if n.len() <= 249 => n,
        Ok(_) => return EINVAL,
        Err(e) => return e,
    };
    let id = crate::task::ipc::memfd_create(task::current_pid());
    crate::syscall::install_fd(OpenFile::Memfd { id, pos: 0, name, access: 2 })
}

pub fn sys_pwrite(fd: u64, buf: u64, count: u64, offset: u64) -> i64 {
    let data = match user_slice(buf, count.min(64 << 20)) {
        Ok(d) => d.to_vec(),
        Err(e) => return e,
    };
    match target(fd) {
        Target::File(node, _, _) => {
            let outcome = with_vfs(|v, uid| v.write_node_at(node, offset as usize, &data, uid));
            match outcome {
                Some(Ok(())) => {
                    fs::request_sync();
                    data.len() as i64
                }
                Some(Err(e)) => crate::syscall::fs_error(e),
                None => ENODEV,
            }
        }
        Target::Memfd(id, _) => {
            let end = offset + data.len() as u64;
            if crate::task::ipc::shm_len(id).map(|l| l < end).unwrap_or(false) {
                let r = crate::task::ipc::shm_set_len(id, end);
                if r < 0 {
                    return r;
                }
            }
            let mut copy = data;
            crate::task::ipc::shm_io(id, offset, &mut copy, true).map(|n| n as i64).unwrap_or_else(|e| e)
        }
        Target::Bad => EBADF,
        Target::Dir(_) => EISDIR,
        _ => ESPIPE,
    }
}
