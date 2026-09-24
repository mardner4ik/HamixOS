use alloc::string::String;
use alloc::vec::Vec;

pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_SCHED_YIELD: u64 = 24;
pub const SYS_GETPID: u64 = 39;
pub const SYS_EXIT: u64 = 60;
pub const SYS_UNAME: u64 = 63;
pub const SYS_GETCWD: u64 = 79;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_MKDIR: u64 = 83;
pub const SYS_UNLINK: u64 = 87;
pub const SYS_GETUID: u64 = 102;
pub const SYS_GETEUID: u64 = 107;
pub const SYS_GETPPID: u64 = 110;
pub const SYS_CLOCK_GETTIME: u64 = 228;
pub const SYS_HAMIX_FBMAP: u64 = 9001;
pub const SYS_HAMIX_READKEY: u64 = 9002;
pub const SYS_HAMIX_TRUNCATE: u64 = 9003;
pub const SYS_HAMIX_MOUSE: u64 = 9004;
pub const SYS_HAMIX_POLLKEY: u64 = 9005;
pub const SYS_HAMIX_RELEASE_FB: u64 = 9006;
pub const SYS_HAMIX_SPAWN: u64 = 9010;
pub const SYS_HAMIX_WAITPID: u64 = 9012;
pub const SYS_HAMIX_SLEEP: u64 = 9013;
pub const SYS_HAMIX_WAIT_EVENT: u64 = 9014;
pub const SYS_HAMIX_KILL: u64 = 9015;
pub const SYS_HAMIX_PROC_LIST: u64 = 9016;
pub const SYS_HAMIX_SYSINFO: u64 = 9017;
pub const SYS_HAMIX_PROC_ALIVE: u64 = 9018;
pub const SYS_HAMIX_MSG_SEND: u64 = 9020;
pub const SYS_HAMIX_MSG_RECV: u64 = 9021;
pub const SYS_HAMIX_SERVICE_REGISTER: u64 = 9022;
pub const SYS_HAMIX_SERVICE_LOOKUP: u64 = 9023;
pub const SYS_HAMIX_SHM_CREATE: u64 = 9030;
pub const SYS_HAMIX_SHM_MAP: u64 = 9031;
pub const SYS_HAMIX_SHM_RELEASE: u64 = 9032;
pub const SYS_HAMIX_CMD_REGISTER: u64 = 9040;
pub const SYS_HAMIX_CMD_UNREGISTER: u64 = 9041;
pub const SYS_HAMIX_CMD_LIST: u64 = 9042;
pub const SYS_HAMIX_CMD_RUN: u64 = 9043;
pub const SYS_HAMIX_SYNC: u64 = 9050;
pub const SYS_HAMIX_READDIR: u64 = 9051;
pub const SYS_HAMIX_REALTIME: u64 = 9060;

pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_CREAT: u64 = 0o100;
pub const O_TRUNC: u64 = 0o1000;
pub const O_APPEND: u64 = 0o2000;

pub const SPAWN_DETACH: u64 = 1;
pub const SPAWN_FOREGROUND: u64 = 2;

pub const EVENT_INPUT: u64 = 1;
pub const EVENT_MESSAGE: u64 = 2;

pub const EAGAIN: i64 = -11;

#[cfg(target_arch = "x86_64")]
pub const ARCH: &str = "x86_64";
#[cfg(target_arch = "aarch64")]
pub const ARCH: &str = "aarch64";
#[cfg(target_arch = "riscv64")]
pub const ARCH: &str = "riscv64";

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") num,
            inlateout("x0") a1 as i64 => ret,
            in("x1") a2,
            in("x2") a3,
            in("x3") a4,
            in("x4") a5,
            in("x5") a6,
            options(nostack),
        );
    }
    ret
}

#[cfg(target_arch = "riscv64")]
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") num,
            inlateout("a0") a1 as i64 => ret,
            in("a1") a2,
            in("a2") a3,
            in("a3") a4,
            in("a4") a5,
            in("a5") a6,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
pub unsafe fn syscall5(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    unsafe { syscall6(num, a1, a2, a3, a4, a5, 0) }
}

#[inline(always)]
pub unsafe fn syscall3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    unsafe { syscall5(num, a1, a2, a3, 0, 0) }
}

#[inline(always)]
unsafe fn syscall1(num: u64, a1: u64) -> i64 {
    unsafe { syscall5(num, a1, 0, 0, 0, 0) }
}

#[inline(always)]
unsafe fn syscall0(num: u64) -> i64 {
    unsafe { syscall5(num, 0, 0, 0, 0, 0) }
}

fn cstring(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);
    bytes
}

fn pack_args<S: AsRef<str>>(args: &[S]) -> Vec<u8> {
    let mut block = Vec::new();
    for arg in args {
        block.extend_from_slice(arg.as_ref().as_bytes());
        block.push(0);
    }
    block
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

pub fn open(path: &str) -> i64 {
    open_with(path, O_RDWR)
}

pub fn create(path: &str) -> i64 {
    open_with(path, O_RDWR | O_CREAT | O_TRUNC)
}

pub fn truncate(path: &str) -> i64 {
    let c = cstring(path);
    unsafe { syscall1(SYS_HAMIX_TRUNCATE, c.as_ptr() as u64) }
}

pub fn close(fd: u64) -> i64 {
    unsafe { syscall1(SYS_CLOSE, fd) }
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

pub fn munmap(addr: u64, len: u64) -> i64 {
    unsafe { syscall3(SYS_MUNMAP, addr, len, 0) }
}

pub fn brk(addr: usize) -> i64 {
    unsafe { syscall1(SYS_BRK, addr as u64) }
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

#[derive(Clone, Copy, Default)]
pub struct Timespec {
    pub sec: i64,
    pub nsec: i64,
}

pub fn clock_gettime() -> Timespec {
    clock(1)
}

pub fn realtime() -> Timespec {
    clock(0)
}

fn clock(id: u64) -> Timespec {
    let mut buf = [0u8; 16];
    unsafe { syscall3(SYS_CLOCK_GETTIME, id, buf.as_mut_ptr() as u64, 0) };
    Timespec {
        sec: i64::from_le_bytes(buf[0..8].try_into().unwrap()),
        nsec: i64::from_le_bytes(buf[8..16].try_into().unwrap()),
    }
}

pub fn uptime_ms() -> u64 {
    let t = clock_gettime();
    t.sec as u64 * 1000 + t.nsec as u64 / 1_000_000
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct HamixFbInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
}

pub fn fbmap(info: &mut HamixFbInfo) -> i64 {
    unsafe { syscall1(SYS_HAMIX_FBMAP, info as *mut HamixFbInfo as u64) }
}

pub fn release_framebuffer() {
    unsafe { syscall0(SYS_HAMIX_RELEASE_FB) };
}

pub const MOUSE_LEFT: u32 = 1;
pub const MOUSE_RIGHT: u32 = 2;
pub const MOUSE_MIDDLE: u32 = 4;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: u32,
    pub wheel: i32,
}

pub fn mouse(state: &mut MouseState) -> i64 {
    unsafe { syscall1(SYS_HAMIX_MOUSE, state as *mut MouseState as u64) }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    AltTab,
    Super,
    AltF4,
    Resize,
    SnapLeft,
    SnapRight,
    SnapUp,
    SnapDown,
    Insert,
    F(u8),
}

pub const MOD_SHIFT: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_CTRL: u8 = 4;

pub fn key_base(code: i64) -> i64 {
    if code > u32::MAX as i64 { code as u32 as i32 as i64 } else { code }
}

pub fn key_modifiers(code: i64) -> u8 {
    if code > u32::MAX as i64 { ((code >> 32) & 0xFF) as u8 } else { 0 }
}

impl Key {
    pub fn code(self) -> i64 {
        match self {
            Key::Char(c) => c as i64,
            Key::Enter => 10,
            Key::Backspace => 8,
            Key::Tab => 9,
            Key::Escape => 27,
            Key::Up => -1,
            Key::Down => -2,
            Key::Left => -3,
            Key::Right => -4,
            Key::Home => -5,
            Key::End => -6,
            Key::Delete => -7,
            Key::PageUp => -8,
            Key::PageDown => -9,
            Key::AltTab => -20,
            Key::Super => -21,
            Key::AltF4 => -22,
            Key::Resize => -30,
            Key::SnapLeft => -23,
            Key::SnapRight => -24,
            Key::SnapUp => -25,
            Key::SnapDown => -26,
            Key::Insert => -53,
            Key::F(n) => -40 - n as i64,
        }
    }

    pub fn from_code(code: i64) -> Key {
        match key_base(code) {
            10 => Key::Enter,
            8 => Key::Backspace,
            9 => Key::Tab,
            27 => Key::Escape,
            -1 => Key::Up,
            -2 => Key::Down,
            -3 => Key::Left,
            -4 => Key::Right,
            -5 => Key::Home,
            -6 => Key::End,
            -7 => Key::Delete,
            -8 => Key::PageUp,
            -9 => Key::PageDown,
            -20 => Key::AltTab,
            -21 => Key::Super,
            -22 => Key::AltF4,
            -30 => Key::Resize,
            -23 => Key::SnapLeft,
            -24 => Key::SnapRight,
            -25 => Key::SnapUp,
            -26 => Key::SnapDown,
            -53 => Key::Insert,
            c @ -52..=-41 => Key::F((-40 - c) as u8),
            c if (0..=255).contains(&c) => Key::Char(c as u8),
            _ => Key::Char(0),
        }
    }
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

pub fn wait_event(mask: u64, timeout_ms: i64) -> u64 {
    let r = unsafe { syscall3(SYS_HAMIX_WAIT_EVENT, mask, timeout_ms as u64, 0) };
    if r < 0 { 0 } else { r as u64 }
}

pub fn spawn<S: AsRef<str>>(program: &str, args: &[S], flags: u64) -> i64 {
    let block = pack_args(args);
    unsafe {
        syscall5(
            SYS_HAMIX_SPAWN,
            program.as_ptr() as u64,
            program.len() as u64,
            block.as_ptr() as u64,
            block.len() as u64 | (flags << 32),
            0,
        )
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

fn read_text2(num: u64) -> String {
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

pub struct ProcInfo {
    pub pid: i64,
    pub parent: i64,
    pub state: char,
    pub tty: u32,
    pub name: String,
    pub heap_kb: u64,
    pub cpu_ms: u64,
    pub uid: u32,
    pub cpu: i32,
    pub abi: String,
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

#[derive(Clone, Copy, Default)]
pub struct CpuStat {
    pub index: u32,
    pub busy_ms: u64,
    pub total_ms: u64,
    pub apic_id: u32,
    pub online: bool,
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

#[derive(Clone, Copy, Default)]
pub struct SysInfo {
    pub mem_total: u64,
    pub mem_free: u64,
    pub heap_total: u64,
    pub heap_free: u64,
    pub uptime_ms: u64,
    pub processes: u64,
    pub realtime: u64,
    pub file_cache: u64,
}

impl SysInfo {
    pub fn mem_used(&self) -> u64 {
        self.mem_total.saturating_sub(self.mem_free).saturating_sub(self.file_cache)
    }
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

pub fn cmd_register<S: AsRef<str>>(name: &str, program: &str, args: &[S]) -> i64 {
    let mut block = pack_args(&[program]);
    block.extend_from_slice(&pack_args(args));
    unsafe {
        syscall5(
            SYS_HAMIX_CMD_REGISTER,
            name.as_ptr() as u64,
            name.len() as u64,
            block.as_ptr() as u64,
            block.len() as u64,
            0,
        )
    }
}

pub fn cmd_unregister(name: &str) -> i64 {
    unsafe { syscall3(SYS_HAMIX_CMD_UNREGISTER, name.as_ptr() as u64, name.len() as u64, 0) }
}

pub struct CommandEntry {
    pub name: String,
    pub path: String,
    pub args: Vec<String>,
}

pub fn cmd_list() -> Vec<CommandEntry> {
    read_text2(SYS_HAMIX_CMD_LIST)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = String::from(fields.next()?);
            let path = String::from(fields.next()?);
            Some(CommandEntry { name, path, args: fields.map(String::from).collect() })
        })
        .collect()
}

pub fn cmd_run<S: AsRef<str>>(name: &str, extra: &[S], flags: u64) -> i64 {
    let block = pack_args(extra);
    unsafe {
        syscall5(
            SYS_HAMIX_CMD_RUN,
            name.as_ptr() as u64,
            name.len() as u64,
            block.as_ptr() as u64,
            block.len() as u64 | (flags << 32),
            0,
        )
    }
}

pub fn sync() {
    unsafe { syscall0(SYS_HAMIX_SYNC) };
}

pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub kind: char,
    pub mode: u32,
    pub owner: u32,
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

pub const SYS_RENAME: u64 = 82;
pub const SYS_CHMOD: u64 = 90;
pub const SYS_CHOWN: u64 = 92;
pub const SYS_HAMIX_STAT: u64 = 9052;
pub const SYS_HAMIX_DISK_LIST: u64 = 9070;
pub const SYS_HAMIX_DISK_IO: u64 = 9071;
pub const SYS_HAMIX_DISK_RESCAN: u64 = 9072;
pub const SYS_HAMIX_MKFS: u64 = 9073;
pub const SYS_HAMIX_MOUNT: u64 = 9074;
pub const SYS_HAMIX_UMOUNT: u64 = 9075;
pub const SYS_HAMIX_PIPE: u64 = 9090;
pub const SYS_HAMIX_ISATTY: u64 = 9093;
pub const SYS_HAMIX_TERMSIZE: u64 = 9094;
pub const SYS_HAMIX_SET_TERMINAL: u64 = 9095;
pub const SYS_HAMIX_SET_FOREGROUND: u64 = 9096;
pub const SYS_HAMIX_AUTH: u64 = 9100;
pub const SYS_HAMIX_USERS_RELOAD: u64 = 9101;
pub const SYS_HAMIX_POWER: u64 = 9110;
pub const SYS_HAMIX_CPU_STATS: u64 = 9120;

pub const SPAWN_ROOT: u64 = 4;
pub const SPAWN_ENV: u64 = 8;
pub const EVENT_PIPE: u64 = 4;

pub const KIND_FILE: u32 = 1;
pub const KIND_DIR: u32 = 2;
pub const KIND_DEVICE: u32 = 3;
pub const KIND_PROC: u32 = 4;

pub const POWER_HALT: u64 = 0;
pub const POWER_REBOOT: u64 = 1;
pub const POWER_OFF: u64 = 2;

pub fn error_name(code: i64) -> &'static str {
    match code {
        -1 => "operation not permitted",
        -2 => "no such file or directory",
        -3 => "no such process",
        -5 => "input/output error",
        -8 => "exec format error",
        -9 => "bad file descriptor",
        -10 => "no child processes",
        -11 => "resource temporarily unavailable",
        -12 => "out of memory",
        -13 => "permission denied",
        -14 => "bad address",
        -16 => "device or resource busy",
        -17 => "file exists",
        -19 => "no such device",
        -20 => "not a directory",
        -21 => "is a directory",
        -22 => "invalid argument",
        -24 => "too many open files",
        -25 => "not a terminal",
        -32 => "broken pipe",
        -36 => "file name too long",
        -38 => "function not implemented",
        _ => "unknown error",
    }
}

#[derive(Clone, Copy, Default)]
pub struct Stat {
    pub kind: u32,
    pub mode: u32,
    pub owner: u32,
    pub dev: u32,
    pub size: u64,
}

impl Stat {
    pub fn is_dir(&self) -> bool {
        self.kind == KIND_DIR
    }
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

pub fn spawn_io<S: AsRef<str>>(program: &str, args: &[S], flags: u64, stdin: Option<u64>, stdout: Option<u64>, stderr: Option<u64>) -> i64 {
    spawn_io_env::<S, &str>(program, args, None, flags, stdin, stdout, stderr)
}

pub fn spawn_io_env<S: AsRef<str>, E: AsRef<str>>(
    program: &str,
    args: &[S],
    env: Option<&[E]>,
    flags: u64,
    stdin: Option<u64>,
    stdout: Option<u64>,
    stderr: Option<u64>,
) -> i64 {
    spawn_fds_env(program, args, env, flags, [stdin, stdout, stderr, None])
}

pub fn spawn_fds_env<S: AsRef<str>, E: AsRef<str>>(program: &str, args: &[S], env: Option<&[E]>, flags: u64, fds: [Option<u64>; 4]) -> i64 {
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

pub fn isatty(fd: u64) -> bool {
    unsafe { syscall1(SYS_HAMIX_ISATTY, fd) == 1 }
}

pub fn termsize(fd: u64) -> Option<(u16, u16)> {
    let r = unsafe { syscall1(SYS_HAMIX_TERMSIZE, fd) };
    if r < 0 { None } else { Some(((r >> 16) as u16, r as u16)) }
}

pub fn set_terminal(fd: u64, cols: u16, rows: u16) -> i64 {
    unsafe { syscall3(SYS_HAMIX_SET_TERMINAL, fd, cols as u64, rows as u64) }
}

pub fn set_foreground(pid: i64) -> i64 {
    unsafe { syscall1(SYS_HAMIX_SET_FOREGROUND, pid.max(0) as u64) }
}

pub fn auth(user: &str, password: &str) -> i64 {
    let u = cstring(user);
    let p = cstring(password);
    unsafe { syscall3(SYS_HAMIX_AUTH, u.as_ptr() as u64, p.as_ptr() as u64, 0) }
}

pub fn users_reload() {
    unsafe { syscall0(SYS_HAMIX_USERS_RELOAD) };
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

pub fn mkfs(device: &str, label: &str) -> i64 {
    let d = cstring(device);
    let l = cstring(label);
    unsafe { syscall3(SYS_HAMIX_MKFS, d.as_ptr() as u64, l.as_ptr() as u64, 0) }
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

pub fn exit(code: i32) -> ! {
    unsafe {
        syscall1(SYS_EXIT, code as u32 as u64);
    }
    loop {
        core::hint::spin_loop();
    }
}

pub const SYS_HAMIX_SET_OWN_PASSWORD: u64 = 9102;
pub const SYS_HAMIX_TRACE: u64 = 9103;

pub fn trace_syscalls(name: &str) -> i64 {
    unsafe { syscall3(SYS_HAMIX_TRACE, name.as_ptr() as u64, name.len() as u64, 0) }
}

pub fn set_own_password(old: &str, new: &str) -> i64 {
    let o = cstring(old);
    let n = cstring(new);
    unsafe { syscall3(SYS_HAMIX_SET_OWN_PASSWORD, o.as_ptr() as u64, n.as_ptr() as u64, 0) }
}
