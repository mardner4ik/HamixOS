use alloc::string::String;
use alloc::vec::Vec;

use super::*;

const SYS_HAMIX_MAILBOX_FD: u64 = 9024;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;

fn read_text(num: u64, a1: u64, a2: u64) -> String {
    let mut size = 4096usize;
    loop {
        let mut buf = alloc::vec![0u8; size];
        let n = unsafe { syscall5(num, a1, a2, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
        if n < 0 {
            return String::new();
        }
        if (n as usize) <= size {
            buf.truncate(n as usize);
            return String::from_utf8_lossy(&buf).into_owned();
        }
        size = n as usize;
    }
}

pub fn write(fd: u64, buf: &[u8]) -> i64 {
    unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64) }
}

pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
    unsafe { syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn open_with(path: &str, flags: u64) -> i64 {
    let c = cstring(path);
    unsafe { syscall3(SYS_OPEN, c.as_ptr() as u64, flags, 0o644) }
}

pub fn truncate(path: &str) -> i64 {
    let c = cstring(path);
    unsafe { syscall1(SYS_HAMIX_TRUNCATE, c.as_ptr() as u64) }
}

pub fn close(fd: u64) -> i64 {
    unsafe { syscall1(SYS_CLOSE, fd) }
}

pub fn dup(fd: u64) -> i64 {
    unsafe { syscall1(SYS_DUP, fd) }
}

pub fn lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    unsafe { syscall3(SYS_LSEEK, fd, offset as u64, whence) }
}

pub fn fstat_size(fd: u64) -> i64 {
    let mut buf = [0u8; 144];
    let r = unsafe { syscall3(SYS_FSTAT, fd, buf.as_mut_ptr() as u64, 0) };
    if r < 0 {
        return r;
    }
    i64::from_le_bytes(buf[48..56].try_into().unwrap())
}

pub fn mkdir(path: &str) -> i64 {
    let c = cstring(path);
    unsafe { syscall1(SYS_MKDIR, c.as_ptr() as u64) }
}

pub fn unlink(path: &str) -> i64 {
    let c = cstring(path);
    unsafe { syscall1(SYS_UNLINK, c.as_ptr() as u64) }
}

pub fn chdir(path: &str) -> i64 {
    let c = cstring(path);
    unsafe { syscall1(SYS_CHDIR, c.as_ptr() as u64) }
}

pub fn getcwd() -> String {
    let mut buf = [0u8; 512];
    let n = unsafe { syscall3(SYS_GETCWD, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
    if n <= 0 {
        return String::from("/");
    }
    let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

pub fn mmap_anon(len: usize) -> i64 {
    unsafe { syscall3(SYS_MMAP, 0, len as u64, 0) }
}

pub fn mmap_shared_fd(_fd: u64, _len: usize, _offset: u64) -> i64 {
    -38
}

pub fn ioctl(_fd: u64, _request: u64, _arg: *mut u8) -> i64 {
    -38
}

pub fn ioctl_value(_fd: u64, _request: u64, _value: u64) -> i64 {
    -38
}

pub fn munmap(addr: u64, len: u64) -> i64 {
    unsafe { syscall3(SYS_MUNMAP, addr, len, 0) }
}

pub fn getuid() -> i64 {
    unsafe { syscall0(SYS_GETUID) }
}

pub fn geteuid() -> i64 {
    unsafe { syscall0(SYS_GETEUID) }
}

pub fn getpid() -> i64 {
    unsafe { syscall0(SYS_GETPID) }
}

pub fn getppid() -> i64 {
    unsafe { syscall0(SYS_GETPPID) }
}

pub fn yield_now() {
    unsafe { syscall0(SYS_SCHED_YIELD) };
}

pub fn sleep_ms(ms: u64) {
    unsafe { syscall1(SYS_HAMIX_SLEEP, ms) };
}

pub(crate) fn clock(id: u64) -> Timespec {
    let mut buf = [0u8; 16];
    unsafe { syscall3(SYS_CLOCK_GETTIME, id, buf.as_mut_ptr() as u64, 0) };
    Timespec {
        sec: i64::from_le_bytes(buf[0..8].try_into().unwrap()),
        nsec: i64::from_le_bytes(buf[8..16].try_into().unwrap()),
    }
}

pub fn waitpid(pid: i64, nohang: bool) -> i64 {
    unsafe { syscall3(SYS_HAMIX_WAITPID, pid as u64, nohang as u64, 0) }
}

pub fn kill(pid: i64) -> i64 {
    unsafe { syscall1(SYS_HAMIX_KILL, pid as u64) }
}

pub fn proc_alive(pid: i64) -> bool {
    unsafe { syscall1(SYS_HAMIX_PROC_ALIVE, pid as u64) == 1 }
}

pub fn sync() {
    unsafe { syscall0(SYS_HAMIX_SYNC) };
}

pub fn read_dir(path: &str) -> Option<Vec<DirEntry>> {
    let text = read_text(SYS_HAMIX_READDIR, path.as_ptr() as u64, path.len() as u64);
    let probe = unsafe { syscall5(SYS_HAMIX_READDIR, path.as_ptr() as u64, path.len() as u64, 0, 0, 0) };
    if probe < 0 {
        return None;
    }
    Some(
        text.lines()
            .filter_map(|line| {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 3 {
                    return None;
                }
                Some(DirEntry {
                    name: String::from(f[0]),
                    is_dir: f[1] == "d",
                    size: f[2].parse().unwrap_or(0),
                    kind: f[1].chars().next().unwrap_or('f'),
                    mode: f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0o644),
                    owner: f.get(4).and_then(|v| v.parse().ok()).unwrap_or(0),
                })
            })
            .collect(),
    )
}

pub fn stat(path: &str) -> Result<Stat, i64> {
    let c = cstring(path);
    let mut raw = [0u8; 24];
    let r = unsafe { syscall3(SYS_HAMIX_STAT, c.as_ptr() as u64, raw.as_mut_ptr() as u64, 0) };
    if r < 0 {
        return Err(r);
    }
    let u = |i: usize| u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
    Ok(Stat { kind: u(0), mode: u(4), owner: u(8), dev: u(12), size: u64::from_le_bytes(raw[16..24].try_into().unwrap()) })
}

pub fn chmod(path: &str, mode: u32) -> i64 {
    let c = cstring(path);
    unsafe { syscall3(SYS_CHMOD, c.as_ptr() as u64, mode as u64, 0) }
}

pub fn chown(path: &str, uid: u32) -> i64 {
    let c = cstring(path);
    unsafe { syscall3(SYS_CHOWN, c.as_ptr() as u64, uid as u64, 0) }
}

pub fn rename(from: &str, to: &str) -> i64 {
    let a = cstring(from);
    let b = cstring(to);
    unsafe { syscall3(SYS_RENAME, a.as_ptr() as u64, b.as_ptr() as u64, 0) }
}

pub fn pipe() -> Result<(u64, u64), i64> {
    let mut raw = [0u8; 8];
    let r = unsafe { syscall1(SYS_HAMIX_PIPE, raw.as_mut_ptr() as u64) };
    if r < 0 {
        return Err(r);
    }
    Ok((
        u32::from_le_bytes(raw[0..4].try_into().unwrap()) as u64,
        u32::from_le_bytes(raw[4..8].try_into().unwrap()) as u64,
    ))
}

pub fn isatty(fd: u64) -> bool {
    unsafe { syscall1(SYS_HAMIX_ISATTY, fd) == 1 }
}

pub fn termsize(fd: u64) -> Option<(u16, u16)> {
    let r = unsafe { syscall1(SYS_HAMIX_TERMSIZE, fd) };
    if r < 0 { None } else { Some(((r >> 16) as u16, r as u16)) }
}

pub fn exit(code: i32) -> ! {
    unsafe {
        syscall1(SYS_EXIT, code as u32 as u64);
        loop {
            core::hint::spin_loop();
        }
    }
}

pub fn setuid(uid: u32) -> i64 {
    unsafe { syscall1(SYS_SETUID, uid as u64) }
}

pub fn setgid(gid: u32) -> i64 {
    unsafe { syscall1(SYS_SETGID, gid as u64) }
}

pub fn pipe_flags(flags: u64) -> Result<(u64, u64), i64> {
    let mut raw = [0i32; 2];
    let r = unsafe { syscall3(SYS_PIPE2, raw.as_mut_ptr() as u64, flags, 0) };
    if r < 0 {
        return Err(r);
    }
    Ok((raw[0] as u64, raw[1] as u64))
}

pub fn set_nonblocking(fd: u64) -> i64 {
    let flags = unsafe { syscall3(SYS_FCNTL, fd, F_GETFL, 0) };
    if flags < 0 {
        return flags;
    }
    unsafe { syscall3(SYS_FCNTL, fd, F_SETFL, flags as u64 | O_NONBLOCK) }
}

pub fn spawn_fds_env<S: AsRef<str>, E: AsRef<str>>(program: &str, args: &[S], env: Option<&[E]>, flags: u64, fds: [Option<u64>; 4]) -> i64 {
    native_spawn(program, args, env, flags, fds)
}

pub fn proc_list() -> Vec<ProcInfo> {
    read_text2(SYS_HAMIX_PROC_LIST)
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            Some(ProcInfo {
                pid: f[0].parse().ok()?,
                parent: f[1].parse().ok()?,
                state: f[2].chars().next().unwrap_or('?'),
                tty: f[3].parse().unwrap_or(0),
                name: String::from(f[4]),
                heap_kb: f[5].parse().unwrap_or(0),
                cpu_ms: f[6].parse().unwrap_or(0),
                uid: f.get(7).and_then(|v| v.parse().ok()).unwrap_or(0),
                cpu: f.get(8).and_then(|v| v.parse().ok()).unwrap_or(-1),
                abi: String::from(f.get(9).copied().unwrap_or("native")),
            })
        })
        .collect()
}

pub fn cpu_stats() -> Vec<CpuStat> {
    read_text2(SYS_HAMIX_CPU_STATS)
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 5 {
                return None;
            }
            Some(CpuStat {
                index: f[0].parse().ok()?,
                busy_ms: f[1].parse().unwrap_or(0),
                total_ms: f[2].parse().unwrap_or(0),
                apic_id: f[3].parse().unwrap_or(0),
                online: f[4] == "1",
            })
        })
        .collect()
}

pub fn sysinfo() -> SysInfo {
    let mut buf = [0u8; 64];
    unsafe { syscall3(SYS_HAMIX_SYSINFO, buf.as_mut_ptr() as u64, 64, 0) };
    let v = |i: usize| u64::from_le_bytes(buf[i * 8..i * 8 + 8].try_into().unwrap());
    SysInfo {
        mem_total: v(0),
        mem_free: v(1),
        heap_total: v(2),
        heap_free: v(3),
        uptime_ms: v(4),
        processes: v(5),
        realtime: v(6),
        file_cache: v(7),
    }
}

pub fn msg_send(pid: i64, data: &[u8]) -> i64 {
    unsafe { syscall3(SYS_HAMIX_MSG_SEND, pid as u64, data.as_ptr() as u64, data.len() as u64) }
}

pub fn msg_recv(buf: &mut [u8], timeout_ms: i64) -> Option<(i64, usize)> {
    let mut sender = 0u32;
    let n = unsafe {
        syscall5(
            SYS_HAMIX_MSG_RECV,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            &mut sender as *mut u32 as u64,
            timeout_ms as u64,
            0,
        )
    };
    if n < 0 {
        None
    } else {
        Some((sender as i64, (n as usize).min(buf.len())))
    }
}

pub fn service_register(name: &str) -> i64 {
    unsafe { syscall3(SYS_HAMIX_SERVICE_REGISTER, name.as_ptr() as u64, name.len() as u64, 0) }
}

pub fn service_lookup(name: &str) -> i64 {
    unsafe { syscall3(SYS_HAMIX_SERVICE_LOOKUP, name.as_ptr() as u64, name.len() as u64, 0) }
}

pub fn shm_create(size: usize) -> i64 {
    unsafe { syscall1(SYS_HAMIX_SHM_CREATE, size as u64) }
}

pub fn shm_map(id: u32) -> Option<(*mut u8, usize)> {
    let mut len = 0u64;
    let addr = unsafe { syscall3(SYS_HAMIX_SHM_MAP, id as u64, &mut len as *mut u64 as u64, 0) };
    if addr <= 0 {
        None
    } else {
        Some((addr as *mut u8, len as usize))
    }
}

pub fn shm_release(id: u32) -> i64 {
    unsafe { syscall1(SYS_HAMIX_SHM_RELEASE, id as u64) }
}

pub fn wait_event(mask: u64, timeout_ms: i64) -> u64 {
    let r = unsafe { syscall3(SYS_HAMIX_WAIT_EVENT, mask, timeout_ms as u64, 0) };
    if r < 0 { 0 } else { r as u64 }
}

pub fn msg_recv_from(from: i64, buf: &mut [u8], timeout_ms: i64) -> Option<usize> {
    let mut sender = 0u32;
    let n = unsafe { syscall5(SYS_HAMIX_MSG_RECV, buf.as_mut_ptr() as u64, buf.len() as u64, &mut sender as *mut u32 as u64, timeout_ms as u64, from as u64) };
    if n < 0 { None } else { Some((n as usize).min(buf.len())) }
}

pub fn service_activator(name: &str) -> i64 {
    unsafe { syscall3(SYS_HAMIX_SERVICE_ACTIVATOR, name.as_ptr() as u64, name.len() as u64, 0) }
}

pub fn wait_any(entries: &mut [WaitEntry], timeout_ms: Option<u64>) -> i64 {
    let timeout = timeout_ms.map(|ms| ms.saturating_mul(1_000_000)).unwrap_or(u64::MAX);
    unsafe { syscall3(SYS_HAMIX_WAIT_ANY, entries.as_mut_ptr() as u64, entries.len() as u64, timeout) }
}

pub fn mailbox_fd() -> i64 {
    unsafe { syscall0(SYS_HAMIX_MAILBOX_FD) }
}

pub fn shm_sweep() {}

pub fn set_terminal(fd: u64, cols: u16, rows: u16) -> i64 {
    unsafe { syscall3(SYS_HAMIX_SET_TERMINAL, fd, cols as u64, rows as u64) }
}

pub fn set_foreground(pid: i64) -> i64 {
    unsafe { syscall1(SYS_HAMIX_SET_FOREGROUND, pid.max(0) as u64) }
}

pub fn power(mode: u64) -> i64 {
    unsafe { syscall1(SYS_HAMIX_POWER, mode) }
}

pub fn disk_listing() -> String {
    read_text2(SYS_HAMIX_DISK_LIST)
}

pub fn disk_read(device: &str, lba: u64, buf: &mut [u8]) -> i64 {
    let c = cstring(device);
    unsafe { syscall5(SYS_HAMIX_DISK_IO, c.as_ptr() as u64, lba, buf.as_mut_ptr() as u64, (buf.len() / 512) as u64, 0) }
}

pub fn disk_write(device: &str, lba: u64, buf: &[u8]) -> i64 {
    let c = cstring(device);
    unsafe { syscall5(SYS_HAMIX_DISK_IO, c.as_ptr() as u64, lba, buf.as_ptr() as u64, (buf.len() / 512) as u64, 1) }
}

pub fn disk_rescan() {
    unsafe { syscall0(SYS_HAMIX_DISK_RESCAN) };
}

pub fn mount(device: &str, path: &str) -> i64 {
    let d = cstring(device);
    let p = cstring(path);
    unsafe { syscall3(SYS_HAMIX_MOUNT, d.as_ptr() as u64, p.as_ptr() as u64, 0) }
}

pub fn umount(path: &str) -> i64 {
    let p = cstring(path);
    unsafe { syscall1(SYS_HAMIX_UMOUNT, p.as_ptr() as u64) }
}

pub fn poll_key() -> Option<Key> {
    let code = unsafe { syscall0(SYS_HAMIX_POLLKEY) };
    if code == -100 {
        return None;
    }
    Some(Key::from_code(code))
}

pub fn poll_key_code() -> Option<i64> {
    let code = unsafe { syscall0(SYS_HAMIX_POLLKEY) };
    if code == -100 { None } else { Some(code) }
}

pub fn read_key() -> Key {
    Key::from_code(unsafe { syscall0(SYS_HAMIX_READKEY) })
}

pub fn open_pty() -> Option<(u64, u64)> {
    let master = open("/dev/ptmx");
    if master < 0 {
        return None;
    }
    let mut index = 0u32;
    let r = unsafe { syscall3(16, master as u64, 0x8004_5430, &mut index as *mut u32 as u64) };
    let slave = if r >= 0 { open(&alloc::format!("/dev/pts/{}", index)) } else { -1 };
    if slave < 0 {
        close(master as u64);
        return None;
    }
    Some((master as u64, slave as u64))
}

fn pack_args<S: AsRef<str>>(args: &[S]) -> Vec<u8> {
    let mut block = Vec::new();
    for arg in args {
        block.extend_from_slice(arg.as_ref().as_bytes());
        block.push(0);
    }
    block
}

pub(crate) fn native_spawn<S: AsRef<str>, E: AsRef<str>>(program: &str, args: &[S], env: Option<&[E]>, flags: u64, fds: [Option<u64>; 4]) -> i64 {
    let block = pack_args(args);
    let encode = |fd: Option<u64>| fd.map(|f| f + 1).unwrap_or(0) & 0xFFFF;
    let stdio = encode(fds[0]) | (encode(fds[1]) << 16) | (encode(fds[2]) << 32) | (encode(fds[3]) << 48);
    let env_block = env.map(pack_args).unwrap_or_default();
    let header: [u64; 2] = [env_block.as_ptr() as u64, env_block.len() as u64];
    let flags = if env.is_some() { flags | SPAWN_ENV } else { flags & !SPAWN_ENV };
    unsafe {
        syscall6(
            SYS_HAMIX_SPAWN,
            program.as_ptr() as u64,
            program.len() as u64,
            block.as_ptr() as u64,
            block.len() as u64 | (flags << 32),
            stdio,
            header.as_ptr() as u64,
        )
    }
}

pub(crate) fn read_text2(num: u64) -> String {
    let mut size = 4096usize;
    loop {
        let mut buf = alloc::vec![0u8; size];
        let n = unsafe { syscall3(num, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
        if n < 0 {
            return String::new();
        }
        if (n as usize) <= size {
            buf.truncate(n as usize);
            return String::from_utf8_lossy(&buf).into_owned();
        }
        size = n as usize;
    }
}

pub fn terminal_restore() {}

pub fn getgid() -> u32 {
    unsafe { syscall0(104) as u32 }
}

pub fn getegid() -> u32 {
    unsafe { syscall0(108) as u32 }
}

pub fn groups() -> Vec<u32> {
    let mut list = [0u32; 64];
    let n = unsafe { syscall3(115, list.len() as u64, list.as_mut_ptr() as u64, 0) };
    if n < 0 { Vec::new() } else { list[..n as usize].to_vec() }
}

pub fn set_groups(list: &[u32]) -> i64 {
    unsafe { syscall3(116, list.len() as u64, list.as_ptr() as u64, 0) }
}

pub fn init_groups(_user: &str, _gid: u32) -> i64 {
    0
}

pub fn chown_group(file: &str, uid: Option<u32>, gid: Option<u32>) -> i64 {
    let c = cstring(file);
    unsafe { syscall3(92, c.as_ptr() as u64, uid.unwrap_or(u32::MAX) as u64, gid.unwrap_or(u32::MAX) as u64) }
}

pub const SIGHUP: i32 = 1;
pub const SIGTERM: i32 = 15;

pub fn send_signal(pid: i64, _signal: i32) -> i64 {
    kill(pid)
}

pub fn catch_hangup() {}

pub fn take_hangup() -> bool {
    false
}

pub const R_OK: i32 = 4;
pub const W_OK: i32 = 2;
pub const X_OK: i32 = 1;

pub fn access(file: &str, _mode: i32) -> bool {
    stat(file).is_ok()
}

pub fn crypt(_key: &str, _setting: &str) -> Option<String> {
    None
}
