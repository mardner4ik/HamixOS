use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};

use hxlibc_sys::libc;

use super::*;

fn ok(ret: c_int) -> i64 {
    libc::result(ret as i64)
}

fn path(text: &str) -> Vec<u8> {
    cstring(text)
}

pub fn write(fd: u64, buf: &[u8]) -> i64 {
    libc::result(unsafe { libc::write(fd as c_int, buf.as_ptr() as *const c_void, buf.len()) } as i64)
}

pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
    libc::result(unsafe { libc::read(fd as c_int, buf.as_mut_ptr() as *mut c_void, buf.len()) } as i64)
}

pub fn open_with(file: &str, flags: u64) -> i64 {
    let c = path(file);
    ok(unsafe { libc::open(c.as_ptr() as *const c_char, flags as c_int, 0o644u32) })
}

pub fn truncate(file: &str) -> i64 {
    let fd = open_with(file, O_WRONLY | O_CREAT | O_TRUNC);
    if fd < 0 {
        return fd;
    }
    close(fd as u64);
    0
}

pub fn close(fd: u64) -> i64 {
    ok(unsafe { libc::close(fd as c_int) })
}

pub fn dup(fd: u64) -> i64 {
    ok(unsafe { libc::dup(fd as c_int) })
}

pub fn lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    libc::result(unsafe { libc::lseek(fd as c_int, offset, whence as c_int) })
}

pub fn fstat_size(fd: u64) -> i64 {
    let mut st = libc::stat::zeroed();
    let r = ok(unsafe { libc::fstat(fd as c_int, &mut st) });
    if r < 0 { r } else { st.st_size }
}

pub fn mkdir(dir: &str) -> i64 {
    let c = path(dir);
    ok(unsafe { libc::mkdir(c.as_ptr() as *const c_char, 0o755) })
}

pub fn unlink(file: &str) -> i64 {
    let c = path(file);
    let r = ok(unsafe { libc::unlink(c.as_ptr() as *const c_char) });
    if r == -21 || r == -1 {
        let d = ok(unsafe { libc::rmdir(c.as_ptr() as *const c_char) });
        if d == 0 {
            return 0;
        }
    }
    r
}

pub fn chdir(dir: &str) -> i64 {
    let c = path(dir);
    ok(unsafe { libc::chdir(c.as_ptr() as *const c_char) })
}

pub fn getcwd() -> String {
    let mut buf = [0u8; 1024];
    let p = unsafe { libc::getcwd(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if p.is_null() {
        return String::from("/");
    }
    let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

pub fn mmap_anon(len: usize) -> i64 {
    let p = unsafe { libc::mmap(core::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) };
    if p == libc::MAP_FAILED { -libc::errno() } else { p as i64 }
}

pub fn mmap_shared_fd(fd: u64, len: usize, offset: u64) -> i64 {
    let p = unsafe { libc::mmap(core::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd as c_int, offset as i64) };
    if p == libc::MAP_FAILED { -libc::errno() } else { p as i64 }
}

pub fn ioctl(fd: u64, request: u64, arg: *mut u8) -> i64 {
    ok(unsafe { libc::ioctl(fd as c_int, request as core::ffi::c_long, arg) })
}

pub fn ioctl_value(fd: u64, request: u64, value: u64) -> i64 {
    ok(unsafe { libc::ioctl(fd as c_int, request as core::ffi::c_long, value) })
}

pub fn munmap(addr: u64, len: u64) -> i64 {
    ok(unsafe { libc::munmap(addr as *mut c_void, len as usize) })
}

pub fn getuid() -> i64 {
    unsafe { libc::getuid() as i64 }
}

pub fn geteuid() -> i64 {
    unsafe { libc::geteuid() as i64 }
}

pub fn getpid() -> i64 {
    unsafe { libc::getpid() as i64 }
}

pub fn getppid() -> i64 {
    unsafe { libc::getppid() as i64 }
}

pub fn setuid(uid: u32) -> i64 {
    ok(unsafe { libc::setuid(uid) })
}

pub fn getgid() -> u32 {
    unsafe { libc::getgid() }
}

pub fn getegid() -> u32 {
    unsafe { libc::getegid() }
}

pub fn groups() -> Vec<u32> {
    let mut list = [0u32; 64];
    let n = unsafe { libc::getgroups(list.len() as c_int, list.as_mut_ptr()) };
    if n < 0 { Vec::new() } else { list[..n as usize].to_vec() }
}

pub fn set_groups(list: &[u32]) -> i64 {
    ok(unsafe { libc::setgroups(list.len(), list.as_ptr()) })
}

pub fn init_groups(user: &str, gid: u32) -> i64 {
    let name = cstring(user);
    ok(unsafe { libc::initgroups(name.as_ptr() as *const c_char, gid) })
}

pub fn chown_group(file: &str, uid: Option<u32>, gid: Option<u32>) -> i64 {
    let c = path(file);
    ok(unsafe { libc::chown(c.as_ptr() as *const c_char, uid.unwrap_or(u32::MAX), gid.unwrap_or(u32::MAX)) })
}

pub fn setgid(gid: u32) -> i64 {
    ok(unsafe { libc::setgid(gid) })
}

pub fn yield_now() {
    unsafe { libc::sched_yield() };
}

pub fn sleep_ms(ms: u64) {
    let mut request = libc::timespec { tv_sec: (ms / 1000) as i64, tv_nsec: ((ms % 1000) * 1_000_000) as i64 };
    let mut remaining = libc::timespec::default();
    while unsafe { libc::nanosleep(&request, &mut remaining) } < 0 && libc::errno() == libc::EINTR as i64 {
        request = remaining;
    }
}

pub(crate) fn clock(id: u64) -> Timespec {
    let mut ts = libc::timespec::default();
    unsafe { libc::clock_gettime(if id == 0 { libc::CLOCK_REALTIME } else { libc::CLOCK_MONOTONIC }, &mut ts) };
    Timespec { sec: ts.tv_sec, nsec: ts.tv_nsec }
}

pub fn waitpid(pid: i64, nohang: bool) -> i64 {
    let mut status: c_int = 0;
    loop {
        let r = unsafe { libc::waitpid(pid as c_int, &mut status, if nohang { libc::WNOHANG } else { 0 }) };
        if r < 0 && libc::errno() == libc::EINTR as i64 {
            continue;
        }
        if r < 0 {
            return -libc::errno();
        }
        if r == 0 {
            return EAGAIN;
        }
        let signal = status & 0x7f;
        return if signal == 0 { ((status >> 8) & 0xff) as i64 } else { 128 + signal as i64 };
    }
}

pub fn kill(pid: i64) -> i64 {
    ok(unsafe { libc::kill(pid as c_int, libc::SIGKILL) })
}

pub const R_OK: i32 = 4;
pub const W_OK: i32 = 2;
pub const X_OK: i32 = 1;

pub fn access(file: &str, mode: i32) -> bool {
    let p = path(file);
    unsafe { libc::access(p.as_ptr() as *const c_char, mode) == 0 }
}

pub const SIGHUP: i32 = libc::SIGHUP;
pub const SIGTERM: i32 = libc::SIGTERM;

pub fn send_signal(pid: i64, signal: i32) -> i64 {
    ok(unsafe { libc::kill(pid as c_int, signal) })
}

static HANGUP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

extern "C" fn hangup(_: c_int) {
    HANGUP.store(true, core::sync::atomic::Ordering::Relaxed);
}

pub fn catch_hangup() {
    unsafe { libc::signal(libc::SIGHUP, hangup as *const () as usize) };
}

pub fn take_hangup() -> bool {
    HANGUP.swap(false, core::sync::atomic::Ordering::Relaxed)
}

static mut DETACHED: Vec<i32> = Vec::new();

fn reap_detached() {
    unsafe {
        let list = &mut *(&raw mut DETACHED);
        list.retain(|pid| {
            let mut status: c_int = 0;
            libc::waitpid(*pid, &mut status, libc::WNOHANG) == 0
        });
    }
}

pub fn proc_alive(pid: i64) -> bool {
    reap_detached();
    pid > 0 && (unsafe { libc::kill(pid as c_int, 0) } == 0 || libc::errno() == libc::EPERM as i64)
}

fn resolve_program(program: &str) -> Option<String> {
    let candidates: Vec<String> = if program.contains('/') {
        let absolute = if program.starts_with('/') { String::from(program) } else { alloc::format!("{}/{}", getcwd(), program) };
        alloc::vec![crate::fs::normalize(&absolute)]
    } else {
        ["/usr/bin", "/bin", "/sbin"].iter().map(|dir| alloc::format!("{}/{}", dir, program)).collect()
    };
    candidates.into_iter().find(|p| stat(p).map(|s| !s.is_dir()).unwrap_or(false))
}

fn c_strings<S: AsRef<str>>(items: &[S]) -> Vec<Vec<u8>> {
    items.iter().map(|s| cstring(s.as_ref())).collect()
}

fn pointers(items: &[Vec<u8>]) -> Vec<*const c_char> {
    let mut out: Vec<*const c_char> = items.iter().map(|s| s.as_ptr() as *const c_char).collect();
    out.push(core::ptr::null());
    out
}

pub fn spawn_fds_env<S: AsRef<str>, E: AsRef<str>>(program: &str, args: &[S], env: Option<&[E]>, flags: u64, fds: [Option<u64>; 4]) -> i64 {
    if flags & SPAWN_FOREGROUND != 0 {
        terminal_restore();
    }
    if flags & SPAWN_ROOT != 0 {
        if geteuid() != 0 {
            return -1;
        }
        if getuid() != 0 {
            init_groups("root", 0);
            setgid(0);
            if setuid(0) < 0 {
                return -1;
            }
        }
    }
    reap_detached();
    let Some(path) = resolve_program(program) else {
        return -2;
    };
    let mut argv_text = alloc::vec![cstring(&path)];
    argv_text.extend(c_strings(args));
    let argv = pointers(&argv_text);
    let env_text = env.map(c_strings);
    let env_pointers = env_text.as_ref().map(|e| pointers(e));
    let envp = match env_pointers.as_ref() {
        Some(e) => e.as_ptr(),
        None => unsafe { libc::environ },
    };
    let mut actions = libc::posix_spawn_file_actions_t::zeroed();
    unsafe {
        libc::posix_spawn_file_actions_init(&mut actions);
        for (slot, fd) in fds.iter().enumerate() {
            if let Some(fd) = fd {
                libc::posix_spawn_file_actions_adddup2(&mut actions, *fd as c_int, 200 + slot as c_int);
            }
        }
        for (slot, fd) in fds.iter().enumerate() {
            if fd.is_some() {
                libc::posix_spawn_file_actions_adddup2(&mut actions, 200 + slot as c_int, slot as c_int);
            }
        }
        libc::posix_spawn_file_actions_addclosefrom_np(&mut actions, if fds[3].is_some() { 4 } else { 3 });
    }
    let mut attr = libc::posix_spawnattr_t::zeroed();
    unsafe {
        libc::posix_spawnattr_init(&mut attr);
        libc::posix_spawnattr_setflags(&mut attr, libc::POSIX_SPAWN_SETPGROUP);
        libc::posix_spawnattr_setpgroup(&mut attr, 0);
    }
    let mut pid: c_int = 0;
    let r = unsafe { libc::posix_spawn(&mut pid, argv_text[0].as_ptr() as *const c_char, &actions, &attr, argv.as_ptr(), envp) };
    unsafe {
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attr);
    }
    if r != 0 {
        return -(r as i64);
    }
    if flags & SPAWN_DETACH != 0 {
        unsafe { (*(&raw mut DETACHED)).push(pid) };
    }
    if flags & SPAWN_FOREGROUND != 0 {
        let tty = fds[0].unwrap_or(0) as c_int;
        if unsafe { libc::isatty(tty) } == 1 {
            unsafe { libc::tcsetpgrp(tty, pid) };
        }
    }
    pid as i64
}

pub fn set_foreground(pid: i64) -> i64 {
    let group = if pid <= 0 { unsafe { libc::getpgrp() } } else { unsafe { libc::getpgid(pid as c_int) } };
    if group < 0 {
        return -libc::errno();
    }
    ok(unsafe { libc::tcsetpgrp(0, group) })
}

pub fn set_terminal(fd: u64, cols: u16, rows: u16) -> i64 {
    let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    ok(unsafe { libc::ioctl(fd as c_int, libc::TIOCSWINSZ, &size as *const libc::winsize) })
}

pub fn open_pty() -> Option<(u64, u64)> {
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if master < 0 {
        return None;
    }
    let mut name = [0u8; 64];
    let ready = unsafe { libc::grantpt(master) == 0 && libc::unlockpt(master) == 0 && libc::ptsname_r(master, name.as_mut_ptr() as *mut c_char, name.len()) == 0 };
    let slave = if ready { unsafe { libc::open(name.as_ptr() as *const c_char, libc::O_RDWR | libc::O_NOCTTY, 0u32) } } else { -1 };
    if slave < 0 {
        unsafe { libc::close(master) };
        return None;
    }
    Some((master as u64, slave as u64))
}

pub fn power(mode: u64) -> i64 {
    sync();
    let cmd = match mode {
        POWER_REBOOT => libc::RB_AUTOBOOT,
        POWER_OFF => libc::RB_POWER_OFF,
        _ => libc::RB_HALT_SYSTEM,
    };
    ok(unsafe { libc::reboot(cmd) })
}

pub fn mount(device: &str, target: &str) -> i64 {
    let d = path(device);
    let t = path(target);
    ok(unsafe { libc::mount(d.as_ptr() as *const c_char, t.as_ptr() as *const c_char, b"auto\0".as_ptr() as *const c_char, 0, core::ptr::null()) })
}

pub fn umount(target: &str) -> i64 {
    let t = path(target);
    ok(unsafe { libc::umount2(t.as_ptr() as *const c_char, 0) })
}

fn device_path(device: &str) -> String {
    if device.starts_with('/') { String::from(device) } else { alloc::format!("/dev/{}", device) }
}

fn disk_io(device: &str, lba: u64, buf: *mut u8, len: usize, write: bool) -> i64 {
    let dev = path(&device_path(device));
    let fd = unsafe { libc::open(dev.as_ptr() as *const c_char, if write { libc::O_RDWR } else { 0 } | libc::O_CLOEXEC, 0u32) };
    if fd < 0 {
        return -libc::errno();
    }
    let mut done = 0usize;
    let mut result = 0i64;
    while done < len {
        let offset = (lba * 512 + done as u64) as i64;
        let n = unsafe {
            if write {
                libc::pwrite(fd, buf.add(done) as *const c_void, len - done, offset)
            } else {
                libc::pread(fd, buf.add(done) as *mut c_void, len - done, offset)
            }
        };
        if n < 0 {
            result = -libc::errno();
            break;
        }
        if n == 0 {
            result = -5;
            break;
        }
        done += n as usize;
    }
    unsafe { libc::close(fd) };
    result
}

pub fn disk_read(device: &str, lba: u64, buf: &mut [u8]) -> i64 {
    disk_io(device, lba, buf.as_mut_ptr(), buf.len() / 512 * 512, false)
}

pub fn disk_write(device: &str, lba: u64, buf: &[u8]) -> i64 {
    disk_io(device, lba, buf.as_ptr() as *mut u8, buf.len() / 512 * 512, true)
}

const BLKRRPART: u64 = 0x125F;

pub fn disk_rescan() {
    let Some(first) = read_dir("/sys/block").and_then(|d| d.into_iter().map(|e| e.name).min()) else {
        return;
    };
    let dev = path(&device_path(&first));
    let fd = unsafe { libc::open(dev.as_ptr() as *const c_char, libc::O_CLOEXEC, 0u32) };
    if fd >= 0 {
        unsafe {
            libc::ioctl(fd, BLKRRPART as core::ffi::c_long, 0u64);
            libc::close(fd);
        }
    }
}

fn sys_attr(dir: &str, name: &str) -> String {
    crate::fs::read_to_string(&alloc::format!("{}/{}", dir, name)).map(|t| String::from(t.trim())).unwrap_or_default()
}

fn uevent_value(dir: &str, key: &str) -> String {
    let text = crate::fs::read_to_string(&alloc::format!("{}/uevent", dir)).unwrap_or_default();
    text.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('='))).map(String::from).unwrap_or_default()
}

fn mount_point(name: &str, mounts: &str) -> String {
    let dev = device_path(name);
    mounts
        .lines()
        .find_map(|l| {
            let mut f = l.split_whitespace();
            let source = f.next()?;
            let target = f.next()?;
            (source == dev || source == name).then(|| String::from(target))
        })
        .unwrap_or_default()
}

pub fn disk_listing() -> String {
    let mounts = crate::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut disks: Vec<String> = read_dir("/sys/block").unwrap_or_default().into_iter().map(|e| e.name).collect();
    disks.sort();
    let mut out = String::new();
    for disk in disks {
        let dir = alloc::format!("/sys/block/{}", disk);
        out.push_str(&alloc::format!(
            "disk\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            disk,
            sys_attr(&dir, "size"),
            uevent_value(&dir, "ID_BUS"),
            sys_attr(&dir, "device/model"),
            uevent_value(&dir, "ID_FS_TYPE"),
            uevent_value(&dir, "ID_FS_LABEL"),
            uevent_value(&dir, "ID_FS_UUID"),
            mount_point(&disk, &mounts)
        ));
        let mut parts: Vec<(u32, String)> = read_dir(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.is_dir && e.name.starts_with(disk.as_str()))
            .map(|e| (sys_attr(&alloc::format!("{}/{}", dir, e.name), "partition").parse().unwrap_or(0), e.name))
            .collect();
        parts.sort();
        for (_, part) in parts {
            let pdir = alloc::format!("{}/{}", dir, part);
            out.push_str(&alloc::format!(
                "part\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                part,
                disk,
                sys_attr(&pdir, "start"),
                sys_attr(&pdir, "size"),
                uevent_value(&pdir, "ID_PART_ENTRY_TYPE"),
                uevent_value(&pdir, "ID_FS_TYPE"),
                uevent_value(&pdir, "ID_FS_LABEL"),
                uevent_value(&pdir, "ID_FS_UUID"),
                mount_point(&part, &mounts),
                if uevent_value(&pdir, "ID_PART_ENTRY_FLAGS") == "0x80" { "boot" } else { "" }
            ));
        }
    }
    out
}

const TERMIOS_WORDS: usize = 16;
const LFLAG_WORD: usize = 3;
const ICANON: u32 = 0o2;
const ECHO: u32 = 0o10;
const ISIG: u32 = 0o1;
const CC_OFFSET: usize = 17;
const VTIME: usize = 5;
const VMIN: usize = 6;

pub struct RawTerminal {
    fd: c_int,
    saved: [u32; TERMIOS_WORDS],
    active: bool,
}

impl RawTerminal {
    pub fn enter(fd: u64, keep_signals: bool) -> RawTerminal {
        let mut saved = [0u32; TERMIOS_WORDS];
        let fd = fd as c_int;
        let active = unsafe { libc::tcgetattr(fd, saved.as_mut_ptr() as *mut u8) } == 0;
        if active {
            let mut raw = saved;
            raw[LFLAG_WORD] &= !(ICANON | ECHO | if keep_signals { 0 } else { ISIG });
            let bytes = unsafe { core::slice::from_raw_parts_mut(raw.as_mut_ptr() as *mut u8, TERMIOS_WORDS * 4) };
            bytes[CC_OFFSET + VMIN] = 1;
            bytes[CC_OFFSET + VTIME] = 0;
            unsafe { libc::tcsetattr(fd, 0, raw.as_ptr() as *const u8) };
        }
        RawTerminal { fd, saved, active }
    }

    pub fn set_echo(fd: u64, echo: bool) {
        let mut state = [0u32; TERMIOS_WORDS];
        if unsafe { libc::tcgetattr(fd as c_int, state.as_mut_ptr() as *mut u8) } == 0 {
            if echo {
                state[LFLAG_WORD] |= ECHO;
            } else {
                state[LFLAG_WORD] &= !ECHO;
            }
            unsafe { libc::tcsetattr(fd as c_int, 0, state.as_ptr() as *const u8) };
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if self.active {
            unsafe { libc::tcsetattr(self.fd, 0, self.saved.as_ptr() as *const u8) };
        }
    }
}

fn input_ready(timeout_ms: i32) -> bool {
    let mut p = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, timeout_ms) > 0 && p.revents & libc::POLLIN != 0 }
}

fn next_byte(timeout_ms: i32) -> Option<u8> {
    if timeout_ms >= 0 && !input_ready(timeout_ms) {
        return None;
    }
    let mut b = [0u8; 1];
    loop {
        let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut c_void, 1) };
        if n == 1 {
            return Some(b[0]);
        }
        if n < 0 && libc::errno() == libc::EINTR as i64 {
            continue;
        }
        return None;
    }
}

fn tilde_key(number: u32) -> Key {
    match number {
        1 | 7 => Key::Home,
        2 => Key::Insert,
        3 => Key::Delete,
        4 | 8 => Key::End,
        5 => Key::PageUp,
        6 => Key::PageDown,
        11..=15 => Key::F((number - 10) as u8),
        17..=21 => Key::F((number - 11) as u8),
        23 | 24 => Key::F((number - 12) as u8),
        _ => Key::Escape,
    }
}

fn escape_key() -> Key {
    match next_byte(30) {
        None => Key::Escape,
        Some(b'O') => match next_byte(30) {
            Some(c @ b'P'..=b'S') => Key::F(c - b'P' + 1),
            Some(b'H') => Key::Home,
            Some(b'F') => Key::End,
            _ => Key::Escape,
        },
        Some(b'[') => {
            let mut number = 0u32;
            loop {
                match next_byte(30) {
                    Some(d @ b'0'..=b'9') => number = number * 10 + (d - b'0') as u32,
                    Some(b';') => number = 0,
                    Some(b'A') => return Key::Up,
                    Some(b'B') => return Key::Down,
                    Some(b'C') => return Key::Right,
                    Some(b'D') => return Key::Left,
                    Some(b'H') => return Key::Home,
                    Some(b'F') => return Key::End,
                    Some(b'~') => return tilde_key(number),
                    _ => return Key::Escape,
                }
            }
        }
        Some(_) => Key::Escape,
    }
}

fn decode_key(byte: u8) -> Key {
    match byte {
        0x1b => escape_key(),
        b'\r' | b'\n' => Key::Enter,
        0x7f | 0x08 => Key::Backspace,
        b'\t' => Key::Tab,
        c => Key::Char(c),
    }
}

static mut KEY_TERMINAL: Option<RawTerminal> = None;

fn key_mode(keep_signals: bool) {
    unsafe {
        let slot = &mut *(&raw mut KEY_TERMINAL);
        if slot.is_none() && isatty(0) {
            *slot = Some(RawTerminal::enter(0, keep_signals));
        }
    }
}

pub fn terminal_restore() {
    unsafe { (*(&raw mut KEY_TERMINAL)).take() };
}

pub fn read_key() -> Key {
    key_mode(false);
    loop {
        if let Some(b) = next_byte(-1) {
            return decode_key(b);
        }
        if !isatty(0) {
            return Key::Char(4);
        }
    }
}

pub fn poll_key() -> Option<Key> {
    key_mode(true);
    next_byte(0).map(decode_key)
}

pub fn poll_key_code() -> Option<i64> {
    poll_key().map(|k| k.code())
}

pub fn sync() {
    unsafe { libc::sync() };
}

fn stat_of(st: &libc::stat, proc_file: bool) -> Stat {
    let kind = match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => KIND_DIR,
        libc::S_IFCHR | libc::S_IFBLK => KIND_DEVICE,
        _ if proc_file => KIND_PROC,
        _ => KIND_FILE,
    };
    Stat { kind, mode: st.st_mode & 0o7777, owner: st.st_uid, dev: st.st_dev as u32, size: st.st_size.max(0) as u64 }
}

fn under_proc(file: &str) -> bool {
    file == "/proc" || file.starts_with("/proc/")
}

pub fn stat(file: &str) -> Result<Stat, i64> {
    let c = path(file);
    let mut st = libc::stat::zeroed();
    let r = ok(unsafe { libc::stat(c.as_ptr() as *const c_char, &mut st) });
    if r < 0 {
        return Err(r);
    }
    Ok(stat_of(&st, under_proc(file)))
}

pub fn read_dir(dir: &str) -> Option<Vec<DirEntry>> {
    let c = path(dir);
    let handle = unsafe { libc::opendir(c.as_ptr() as *const c_char) };
    if handle.is_null() {
        return None;
    }
    let fd = unsafe { libc::dirfd(handle) };
    let proc_dir = under_proc(dir);
    let mut out = Vec::new();
    loop {
        let entry = unsafe { libc::readdir(handle) };
        if entry.is_null() {
            break;
        }
        let raw = unsafe { &(*entry).d_name };
        let len = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
        let bytes: Vec<u8> = raw[..len].iter().map(|b| *b as u8).collect();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let name = String::from_utf8_lossy(&bytes).into_owned();
        let mut st = libc::stat::zeroed();
        let mut cname = bytes.clone();
        cname.push(0);
        let info = if unsafe { libc::fstatat(fd, cname.as_ptr() as *const c_char, &mut st, 0) } == 0 {
            stat_of(&st, proc_dir)
        } else {
            Stat { kind: if unsafe { (*entry).d_type } == libc::DT_DIR { KIND_DIR } else { KIND_FILE }, mode: 0o644, owner: 0, dev: 0, size: 0 }
        };
        let kind = match info.kind {
            KIND_DIR => 'd',
            KIND_DEVICE => 'c',
            KIND_PROC => 'p',
            _ => 'f',
        };
        out.push(DirEntry { name, is_dir: info.kind == KIND_DIR, size: info.size, kind, mode: info.mode, owner: info.owner });
    }
    unsafe { libc::closedir(handle) };
    Some(out)
}

pub fn chmod(file: &str, mode: u32) -> i64 {
    let c = path(file);
    ok(unsafe { libc::chmod(c.as_ptr() as *const c_char, mode) })
}

pub fn chown(file: &str, uid: u32) -> i64 {
    let c = path(file);
    ok(unsafe { libc::chown(c.as_ptr() as *const c_char, uid, u32::MAX) })
}

pub fn rename(from: &str, to: &str) -> i64 {
    let a = path(from);
    let b = path(to);
    ok(unsafe { libc::rename(a.as_ptr() as *const c_char, b.as_ptr() as *const c_char) })
}

pub fn pipe_flags(flags: u64) -> Result<(u64, u64), i64> {
    let mut fds = [0 as c_int; 2];
    let r = ok(unsafe { libc::pipe2(fds.as_mut_ptr(), flags as c_int) });
    if r < 0 {
        return Err(r);
    }
    Ok((fds[0] as u64, fds[1] as u64))
}

pub fn pipe() -> Result<(u64, u64), i64> {
    pipe_flags(0)
}

pub fn set_nonblocking(fd: u64) -> i64 {
    let flags = unsafe { libc::fcntl(fd as c_int, libc::F_GETFL) };
    if flags < 0 {
        return -libc::errno();
    }
    ok(unsafe { libc::fcntl(fd as c_int, libc::F_SETFL, flags | libc::O_NONBLOCK) })
}

pub fn isatty(fd: u64) -> bool {
    unsafe { libc::isatty(fd as c_int) == 1 }
}

pub fn termsize(fd: u64) -> Option<(u16, u16)> {
    let mut size = libc::winsize::default();
    if unsafe { libc::ioctl(fd as c_int, libc::TIOCGWINSZ, &mut size as *mut libc::winsize) } < 0 || size.ws_col == 0 {
        return None;
    }
    Some((size.ws_col, size.ws_row))
}

pub fn exit(code: i32) -> ! {
    terminal_restore();
    unsafe { libc::exit(code) }
}

fn proc_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key).and_then(|rest| rest.strip_prefix(':')).map(|v| v.trim()))
}

fn kib_field(text: &str, key: &str) -> u64 {
    proc_field(text, key).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn process_info(pid: i64) -> Option<ProcInfo> {
    let stat = crate::fs::read_to_string(&alloc::format!("/proc/{}/stat", pid))?;
    let status = crate::fs::read_to_string(&alloc::format!("/proc/{}/status", pid)).unwrap_or_default();
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = &stat[open + 1..close];
    let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    let field = |i: usize| fields.get(i).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let flags = field(6) as u64;
    let kernel = flags & 0x0020_0000 != 0;
    let tty_nr = field(4) as u64;
    let tty = if (tty_nr >> 8) == 4 { (tty_nr & 0xff) as u32 } else { 0 };
    let uid = proc_field(&status, "Uid").and_then(|v| v.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0);
    let name = proc_field(&status, "Name").map(String::from).unwrap_or_else(|| String::from(comm));
    Some(ProcInfo {
        pid,
        parent: field(1),
        state: fields.first().and_then(|s| s.chars().next()).unwrap_or('?'),
        tty,
        name: if kernel { alloc::format!("[{}]", name) } else { name },
        heap_kb: kib_field(&status, "VmRSS"),
        cpu_ms: (field(11) as u64) * 10,
        uid,
        cpu: fields.get(36).and_then(|v| v.parse().ok()).unwrap_or(-1),
        abi: String::from(proc_field(&status, "Abi").unwrap_or(if kernel { "kernel" } else { "hamix" })),
    })
}

fn process_ids() -> Vec<i64> {
    let mut pids: Vec<i64> = read_dir("/proc").unwrap_or_default().iter().filter_map(|e| e.name.parse().ok()).collect();
    pids.sort_unstable();
    pids
}

pub fn proc_list() -> Vec<ProcInfo> {
    process_ids().into_iter().filter_map(process_info).collect()
}

pub fn cpu_stats() -> Vec<CpuStat> {
    let stat = crate::fs::read_to_string("/proc/stat").unwrap_or_default();
    let cpuinfo = crate::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let apic: Vec<u32> = cpuinfo.lines().filter_map(|l| l.strip_prefix("apicid").and_then(|r| r.split(':').nth(1)).and_then(|v| v.trim().parse().ok())).collect();
    stat.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let label = parts.next()?;
            let index: u32 = label.strip_prefix("cpu")?.parse().ok()?;
            let values: Vec<u64> = parts.map(|v| v.parse().unwrap_or(0)).collect();
            let busy = values.first().copied().unwrap_or(0) + values.get(1).copied().unwrap_or(0) + values.get(2).copied().unwrap_or(0);
            let idle = values.get(3).copied().unwrap_or(0);
            Some(CpuStat { index, busy_ms: busy * 10, total_ms: (busy + idle) * 10, apic_id: apic.get(index as usize).copied().unwrap_or(index), online: true })
        })
        .collect()
}

pub fn sysinfo() -> SysInfo {
    let mem = crate::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    SysInfo {
        mem_total: kib_field(&mem, "MemTotal") * 1024,
        mem_free: kib_field(&mem, "MemFree") * 1024,
        heap_total: kib_field(&mem, "KernelHeap") * 1024,
        heap_free: kib_field(&mem, "HeapFree") * 1024,
        uptime_ms: uptime_ms(),
        processes: process_ids().len() as u64,
        realtime: realtime().sec.max(0) as u64,
        file_cache: kib_field(&mem, "FileCache") * 1024,
    }
}

const IPC_DIR: &str = "/run/hamix/ipc";
const SYSTEM_SERVICES: &str = "/run/hamix/services";
const ACTIVATORS: &str = "/run/hamix/activators";
const ACTIVATION_TIMEOUT_MS: u64 = 5000;

struct Mailbox {
    fd: c_int,
    deferred: alloc::collections::VecDeque<(i64, Vec<u8>)>,
    trusted: Vec<i64>,
}

static mut MAILBOX: Option<Mailbox> = None;

fn ensure_dir(dir: &str, mode: u32) {
    let mut partial = String::new();
    for part in dir.split('/').filter(|p| !p.is_empty()) {
        partial.push('/');
        partial.push_str(part);
        if stat(&partial).is_err() {
            mkdir(&partial);
            chmod(&partial, mode);
        }
    }
}

fn socket_address(path: &str) -> (libc::sockaddr_un, u32) {
    let mut addr = libc::sockaddr_un { sun_family: libc::AF_UNIX as u16, sun_path: [0; 108] };
    let bytes = path.as_bytes();
    let n = bytes.len().min(107);
    addr.sun_path[..n].copy_from_slice(&bytes[..n]);
    (addr, (2 + n + 1) as u32)
}

fn mailbox() -> Option<&'static mut Mailbox> {
    unsafe {
        let slot = &mut *(&raw mut MAILBOX);
        if slot.is_none() {
            ensure_dir("/run/hamix", 0o755);
            ensure_dir(IPC_DIR, 0o1777);
            let fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0);
            if fd < 0 {
                return None;
            }
            let on: c_int = 1;
            libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_PASSCRED, &on as *const c_int as *const c_void, 4);
            let path = alloc::format!("{}/{}", IPC_DIR, getpid());
            unlink(&path);
            let (addr, len) = socket_address(&path);
            if libc::bind(fd, &addr, len) < 0 {
                libc::close(fd);
                return None;
            }
            chmod(&path, 0o777);
            *slot = Some(Mailbox { fd, deferred: alloc::collections::VecDeque::new(), trusted: Vec::new() });
        }
        slot.as_mut()
    }
}

pub fn mailbox_fd() -> i64 {
    mailbox().map(|m| m.fd as i64).unwrap_or(-9)
}

fn uid_of(pid: i64) -> Option<u32> {
    let status = crate::fs::read_to_string(&alloc::format!("/proc/{}/status", pid))?;
    proc_field(&status, "Uid").and_then(|v| v.split_whitespace().nth(1)).and_then(|v| v.parse().ok())
}

pub fn msg_send(pid: i64, data: &[u8]) -> i64 {
    let Some(mb) = mailbox() else {
        return -9;
    };
    let path = alloc::format!("{}/{}", IPC_DIR, pid);
    if !mb.trusted.contains(&pid) {
        let owner = stat(&path).map(|s| s.owner).ok();
        match (owner, uid_of(pid)) {
            (Some(owner), Some(uid)) if owner == uid || owner == 0 => {
                if mb.trusted.len() > 256 {
                    mb.trusted.remove(0);
                }
                mb.trusted.push(pid);
            }
            _ => return -3,
        }
    }
    let (addr, len) = socket_address(&path);
    let r = unsafe { libc::sendto(mb.fd, data.as_ptr() as *const c_void, data.len(), libc::MSG_NOSIGNAL, &addr, len) } as i64;
    if r < 0 {
        let e = libc::errno();
        mb.trusted.retain(|p| *p != pid);
        return if e == 2 || e == 111 { -3 } else { -e };
    }
    r
}

fn receive(mb: &mut Mailbox, timeout_ms: i64) -> Option<(i64, Vec<u8>)> {
    let deadline = if timeout_ms < 0 { None } else { Some(uptime_ms() + timeout_ms as u64) };
    loop {
        let mut buf = alloc::vec![0u8; 65536];
        let mut control = [0u64; 16];
        let mut iov = libc::iovec { iov_base: buf.as_mut_ptr() as *mut c_void, iov_len: buf.len() };
        let mut msg = libc::msghdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: &mut iov,
            msg_iovlen: 1,
            msg_control: control.as_mut_ptr() as *mut c_void,
            msg_controllen: core::mem::size_of_val(&control),
            msg_flags: 0,
        };
        let n = unsafe { libc::recvmsg(mb.fd, &mut msg, libc::MSG_DONTWAIT) };
        if n >= 0 {
            let raw = unsafe { core::slice::from_raw_parts(control.as_ptr() as *const u8, msg.msg_controllen.min(128)) };
            let mut sender = 0i64;
            let mut at = 0usize;
            while at + 16 <= raw.len() {
                let len = u64::from_le_bytes(raw[at..at + 8].try_into().unwrap()) as usize;
                let level = i32::from_le_bytes(raw[at + 8..at + 12].try_into().unwrap());
                let kind = i32::from_le_bytes(raw[at + 12..at + 16].try_into().unwrap());
                if level == libc::SOL_SOCKET && kind == libc::SCM_CREDENTIALS && at + 20 <= raw.len() {
                    sender = i32::from_le_bytes(raw[at + 16..at + 20].try_into().unwrap()) as i64;
                }
                if len < 16 {
                    break;
                }
                at += (len + 7) & !7;
            }
            buf.truncate(n as usize);
            return Some((sender, buf));
        }
        let wait = match deadline {
            None => -1,
            Some(d) => {
                let now = uptime_ms();
                if now >= d {
                    return None;
                }
                (d - now).min(i32::MAX as u64) as c_int
            }
        };
        let mut pfd = libc::pollfd { fd: mb.fd, events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut pfd, 1, wait) };
    }
}

pub fn msg_recv(buf: &mut [u8], timeout_ms: i64) -> Option<(i64, usize)> {
    let mb = mailbox()?;
    let (sender, data) = match mb.deferred.pop_front() {
        Some(message) => message,
        None => receive(mb, timeout_ms)?,
    };
    let n = data.len().min(buf.len());
    buf[..n].copy_from_slice(&data[..n]);
    Some((sender, n))
}

pub fn msg_recv_from(from: i64, buf: &mut [u8], timeout_ms: i64) -> Option<usize> {
    let mb = mailbox()?;
    let take = |data: Vec<u8>, buf: &mut [u8]| {
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        n
    };
    if let Some(i) = mb.deferred.iter().position(|(s, _)| *s == from) {
        let (_, data) = mb.deferred.remove(i)?;
        return Some(take(data, buf));
    }
    let deadline = if timeout_ms < 0 { None } else { Some(uptime_ms() + timeout_ms as u64) };
    loop {
        let remaining = match deadline {
            None => -1,
            Some(d) => d.saturating_sub(uptime_ms()) as i64,
        };
        let (sender, data) = receive(mb, remaining)?;
        if sender == from {
            return Some(take(data, buf));
        }
        mb.deferred.push_back((sender, data));
        if deadline.map(|d| uptime_ms() >= d).unwrap_or(false) {
            return None;
        }
    }
}

fn runtime_dir() -> String {
    match crate::env::var("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => String::from(dir),
        _ => alloc::format!("/tmp/runtime-{}", geteuid()),
    }
}

fn user_services() -> String {
    alloc::format!("{}/hamix-services", runtime_dir())
}

fn service_dir(root: bool) -> Option<String> {
    if root {
        ensure_dir(SYSTEM_SERVICES, 0o755);
        return Some(String::from(SYSTEM_SERVICES));
    }
    let runtime = runtime_dir();
    if stat(&runtime).is_err() {
        ensure_dir(&runtime, 0o700);
    }
    let dir = user_services();
    if stat(&dir).is_err() {
        mkdir(&dir);
        chmod(&dir, 0o700);
    }
    match stat(&dir) {
        Ok(s) if s.owner as i64 == geteuid() => Some(dir),
        _ => None,
    }
}

fn write_pid_file(dir: &str, name: &str, pid: i64) -> i64 {
    let tmp = alloc::format!("{}/.{}.{}", dir, name, pid);
    if !crate::fs::write(&tmp, alloc::format!("{}\n", pid).as_bytes()) {
        return -13;
    }
    rename(&tmp, &alloc::format!("{}/{}", dir, name))
}

fn read_pid_file(path: &str, owner: Option<u32>) -> Option<i64> {
    let st = stat(path).ok()?;
    if let Some(expected) = owner {
        if st.owner != expected && st.owner != 0 {
            return None;
        }
    }
    let pid: i64 = crate::fs::read_to_string(path)?.trim().parse().ok()?;
    (pid > 0 && proc_alive(pid)).then_some(pid)
}

pub fn service_register(name: &str) -> i64 {
    if mailbox().is_none() {
        return -9;
    }
    let Some(dir) = service_dir(geteuid() == 0) else {
        return -13;
    };
    write_pid_file(&dir, name, getpid())
}

fn lookup_registered(name: &str) -> Option<i64> {
    let uid = geteuid() as u32;
    let owner = if uid == 0 { None } else { Some(uid) };
    if let Some(pid) = read_pid_file(&alloc::format!("{}/{}", user_services(), name), owner) {
        return Some(pid);
    }
    read_pid_file(&alloc::format!("{}/{}", SYSTEM_SERVICES, name), Some(0))
}

pub fn service_lookup(name: &str) -> i64 {
    if let Some(pid) = lookup_registered(name) {
        return pid;
    }
    let Some(activator) = read_pid_file(&alloc::format!("{}/{}", ACTIVATORS, name), Some(0)) else {
        return -2;
    };
    if msg_send(activator, alloc::format!("activate {}", name).as_bytes()) < 0 {
        return -2;
    }
    let deadline = uptime_ms() + ACTIVATION_TIMEOUT_MS;
    while uptime_ms() < deadline {
        sleep_ms(20);
        if let Some(pid) = lookup_registered(name) {
            return pid;
        }
    }
    -2
}

pub fn service_activator(name: &str) -> i64 {
    if geteuid() != 0 {
        return -1;
    }
    mailbox();
    ensure_dir(ACTIVATORS, 0o755);
    write_pid_file(ACTIVATORS, name, getpid())
}

struct Shared {
    id: u32,
    addr: usize,
    len: usize,
    owned: bool,
}

static mut SHARED: Vec<Shared> = Vec::new();
static mut NEXT_SHM: u32 = 0;

fn shm_name(id: u32) -> Vec<u8> {
    cstring(&alloc::format!("/hx-{}", id))
}

fn map_fd(fd: c_int, len: usize) -> Option<usize> {
    let p = unsafe { libc::mmap(core::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
    (p != libc::MAP_FAILED).then_some(p as usize)
}

pub fn shm_create(size: usize) -> i64 {
    let len = (size.max(1) + 4095) & !4095;
    for _ in 0..4096 {
        let counter = unsafe {
            NEXT_SHM = (NEXT_SHM + 1) & 0xfff;
            NEXT_SHM
        };
        let id = ((getpid() as u32) << 12) | counter;
        if id == 0 {
            continue;
        }
        let name = shm_name(id);
        let fd = unsafe { libc::shm_open(name.as_ptr() as *const c_char, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC, 0o600) };
        if fd < 0 {
            if libc::errno() == 17 {
                continue;
            }
            return -libc::errno();
        }
        let ok = unsafe { libc::ftruncate(fd, len as i64) } == 0;
        let addr = if ok { map_fd(fd, len) } else { None };
        unsafe { libc::close(fd) };
        let Some(addr) = addr else {
            unsafe { libc::shm_unlink(name.as_ptr() as *const c_char) };
            return -12;
        };
        unsafe { (*(&raw mut SHARED)).push(Shared { id, addr, len, owned: true }) };
        return id as i64;
    }
    -17
}

pub fn shm_map(id: u32) -> Option<(*mut u8, usize)> {
    unsafe {
        if let Some(s) = (*(&raw const SHARED)).iter().find(|s| s.id == id) {
            return Some((s.addr as *mut u8, s.len));
        }
    }
    let name = shm_name(id);
    let fd = unsafe { libc::shm_open(name.as_ptr() as *const c_char, libc::O_RDWR | libc::O_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    let mut st = libc::stat::zeroed();
    let len = if unsafe { libc::fstat(fd, &mut st) } == 0 { st.st_size.max(0) as usize } else { 0 };
    let addr = if len > 0 { map_fd(fd, len) } else { None };
    unsafe { libc::close(fd) };
    let addr = addr?;
    unsafe { (*(&raw mut SHARED)).push(Shared { id, addr, len, owned: false }) };
    Some((addr as *mut u8, len))
}

pub fn shm_release(id: u32) -> i64 {
    let taken = unsafe {
        let list = &mut *(&raw mut SHARED);
        list.iter().position(|s| s.id == id).map(|i| list.remove(i))
    };
    let Some(shared) = taken else {
        return -22;
    };
    unsafe { libc::munmap(shared.addr as *mut c_void, shared.len) };
    if shared.owned {
        let name = shm_name(id);
        unsafe { libc::shm_unlink(name.as_ptr() as *const c_char) };
    }
    0
}

pub fn shm_sweep() {
    for entry in read_dir("/dev/shm").unwrap_or_default() {
        let Some(id) = entry.name.strip_prefix("hx-").and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        let creator = (id >> 12) as i64;
        if !proc_alive(creator) {
            let name = shm_name(id);
            unsafe { libc::shm_unlink(name.as_ptr() as *const c_char) };
        }
    }
}

pub fn wait_event(mask: u64, timeout_ms: i64) -> u64 {
    let mut entries = [WaitEntry::mailbox()];
    if mask & EVENT_MESSAGE == 0 {
        sleep_ms(timeout_ms.max(0) as u64);
        return 0;
    }
    if wait_any(&mut entries, if timeout_ms < 0 { None } else { Some(timeout_ms as u64) }) > 0 { EVENT_MESSAGE } else { 0 }
}

static CHILD_PIPE: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(-1);

extern "C" fn child_exited(_: c_int) {
    let fd = CHILD_PIPE.load(core::sync::atomic::Ordering::Relaxed);
    if fd >= 0 {
        let byte = 1u8;
        unsafe { libc::write(fd, &byte as *const u8 as *const c_void, 1) };
    }
}

fn child_pipe() -> c_int {
    static READ_END: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(-1);
    let existing = READ_END.load(core::sync::atomic::Ordering::Relaxed);
    if existing >= 0 {
        return existing;
    }
    let Ok((read_end, write_end)) = pipe_flags(O_NONBLOCK | O_CLOEXEC) else {
        return -1;
    };
    CHILD_PIPE.store(write_end as c_int, core::sync::atomic::Ordering::Relaxed);
    READ_END.store(read_end as c_int, core::sync::atomic::Ordering::Relaxed);
    unsafe { libc::signal(libc::SIGCHLD, child_exited as *const () as usize) };
    read_end as c_int
}

fn drain(fd: c_int) {
    let mut buf = [0u8; 64];
    while unsafe { libc::read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) } > 0 {}
}

pub fn wait_any(entries: &mut [WaitEntry], timeout_ms: Option<u64>) -> i64 {
    let mailbox_fd = if entries.iter().any(|e| e.kind == WAIT_MAILBOX) { mailbox().map(|m| if m.deferred.is_empty() { m.fd } else { -2 }) } else { None };
    if mailbox_fd == Some(-2) {
        for e in entries.iter_mut() {
            e.ready = (e.kind == WAIT_MAILBOX) as u64;
        }
        return 1;
    }
    let children = if entries.iter().any(|e| e.kind == WAIT_CHILD) { child_pipe() } else { -1 };
    let now = uptime_ms();
    let mut timeout = timeout_ms.map(|t| t as i64).unwrap_or(-1);
    let mut pfds: Vec<libc::pollfd> = Vec::with_capacity(entries.len());
    let mut slots: Vec<Option<usize>> = Vec::with_capacity(entries.len());
    for e in entries.iter() {
        let fd = match e.kind {
            WAIT_FD => e.id as c_int,
            WAIT_MAILBOX => mailbox_fd.unwrap_or(-1),
            WAIT_CHILD => children,
            WAIT_TIMER => {
                let deadline = e.id / 1_000_000;
                let left = deadline.saturating_sub(now) as i64;
                timeout = if timeout < 0 { left } else { timeout.min(left) };
                -1
            }
            _ => -1,
        };
        if fd < 0 {
            slots.push(None);
            continue;
        }
        let mut events = 0i16;
        if e.kind != WAIT_FD || e.flags & POLL_IN != 0 || e.flags == 0 {
            events |= libc::POLLIN;
        }
        if e.kind == WAIT_FD && e.flags & POLL_OUT != 0 {
            events |= libc::POLLOUT;
        }
        slots.push(Some(pfds.len()));
        pfds.push(libc::pollfd { fd, events, revents: 0 });
    }
    let r = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as u64, timeout.min(i32::MAX as i64) as c_int) };
    let interrupted = r < 0 && libc::errno() == libc::EINTR as i64;
    let after = uptime_ms();
    let mut ready = 0i64;
    for (e, slot) in entries.iter_mut().zip(slots.iter()) {
        e.ready = match (e.kind, slot) {
            (WAIT_TIMER, _) => (after >= e.id / 1_000_000) as u64,
            (WAIT_CHILD, Some(i)) => {
                let hit = pfds[*i].revents != 0 || interrupted;
                if hit {
                    drain(pfds[*i].fd);
                }
                hit as u64
            }
            (_, Some(i)) => {
                let revents = pfds[*i].revents;
                let mut flags = 0u64;
                if revents & libc::POLLIN != 0 {
                    flags |= POLL_IN as u64;
                }
                if revents & libc::POLLOUT != 0 {
                    flags |= POLL_OUT as u64;
                }
                if revents & !(libc::POLLIN | libc::POLLOUT) != 0 {
                    flags |= POLL_HUP as u64;
                }
                flags
            }
            _ => 0,
        };
        if e.ready != 0 {
            ready += 1;
        }
    }
    ready
}

pub fn crypt(key: &str, setting: &str) -> Option<String> {
    let key = cstring(key);
    let setting = cstring(setting);
    let bytes = unsafe {
        let out = libc::crypt(key.as_ptr() as *const c_char, setting.as_ptr() as *const c_char);
        if out.is_null() {
            return None;
        }
        let len = libc::strlen(out);
        let copy = core::slice::from_raw_parts(out as *const u8, len).to_vec();
        core::ptr::write_bytes(out, 0, len);
        copy
    };
    String::from_utf8(bytes).ok()
}
