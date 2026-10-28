use alloc::string::String;
use alloc::vec::Vec;

pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_DUP: u64 = 32;
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

pub use hxlibc_sys::{syscall0, syscall1, syscall3, syscall5, syscall6};

#[cfg(target_os = "hamix")]
#[path = "sys/posix.rs"]
mod imp;

#[cfg(not(target_os = "hamix"))]
#[path = "sys/legacy.rs"]
mod imp;

pub use imp::*;

pub(crate) fn cstring(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);
    bytes
}


pub fn open(path: &str) -> i64 {
    open_with(path, O_RDWR)
}

pub fn create(path: &str) -> i64 {
    open_with(path, O_RDWR | O_CREAT | O_TRUNC)
}

pub fn brk(addr: usize) -> i64 {
    unsafe { syscall1(SYS_BRK, addr as u64) }
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





pub fn spawn<S: AsRef<str>>(program: &str, args: &[S], flags: u64) -> i64 {
    spawn_fds_env::<S, &str>(program, args, None, flags, [None, None, None, None])
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


#[derive(Clone, Copy, Default)]
pub struct CpuStat {
    pub index: u32,
    pub busy_ms: u64,
    pub total_ms: u64,
    pub apic_id: u32,
    pub online: bool,
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









pub const COMMANDS_DIR: &str = "/var/lib/hamix/commands";
pub const COMMANDS_BIN: &str = "/var/lib/hamix/commands/bin";
const LEGACY_COMMANDS: &str = "/etc/commands";
const RESERVED_COMMANDS: &[&str] = &[
    "help", "clear", "edit", "echo", "uname", "whoami", "id", "meminfo", "uptime", "pwd", "ls", "cd",
    "cat", "mkdir", "touch", "rm", "chmod", "chown", "tree", "fb", "gpuinfo", "drivers", "usb", "mouse",
    "exec", "startx", "diskls", "diskcat", "hostname", "date", "sudo", "passwd", "useradd", "halt",
    "poweroff", "reboot", "version", "cpuinfo", "history", "dmesg", "hexdump", "inport", "outport",
    "regs", "alloctest", "crash", "logout", "exit", "ps", "kill", "cmd", "sync", "hext", "free",
];

pub fn cmd_valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn parse_command(line: &str) -> Option<(u32, CommandEntry)> {
    let fields: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
    if fields.len() < 3 || !cmd_valid_name(fields[0]) {
        return None;
    }
    Some((fields[1].parse().unwrap_or(0), CommandEntry { name: String::from(fields[0]), path: String::from(fields[2]), args: fields[3..].iter().map(|a| String::from(*a)).collect() }))
}

fn command_owner(name: &str) -> Option<u32> {
    let text = crate::fs::read_to_string(&alloc::format!("{}/{}", COMMANDS_DIR, name))?;
    parse_command(&text).map(|(owner, _)| owner)
}

pub fn cmd_register<S: AsRef<str>>(name: &str, program: &str, args: &[S]) -> i64 {
    if !cmd_valid_name(name) || RESERVED_COMMANDS.contains(&name) {
        return -22;
    }
    let path = crate::fs::normalize(&if program.starts_with('/') { String::from(program) } else { alloc::format!("{}/{}", getcwd(), program) });
    if args.iter().any(|a| a.as_ref().contains('\t') || a.as_ref().contains('\n')) || path.contains('\t') {
        return -22;
    }
    if !crate::fs::exists(&path) {
        return -2;
    }
    let uid = getuid().max(0) as u32;
    if let Some(owner) = command_owner(name) {
        if owner != uid && uid != 0 {
            return -13;
        }
    }
    mkdir(COMMANDS_DIR);
    let mut line = alloc::format!("{}\t{}\t{}", name, uid, path);
    for a in args {
        line.push('\t');
        line.push_str(a.as_ref());
    }
    line.push('\n');
    let file = alloc::format!("{}/{}", COMMANDS_DIR, name);
    if !crate::fs::write(&file, line.as_bytes()) {
        return -13;
    }
    chmod(&file, 0o644);
    mkdir(COMMANDS_BIN);
    chmod(COMMANDS_BIN, 0o1777);
    let mut script = alloc::format!("#!/bin/sh\nexec {}", shell_quote(&path));
    for a in args {
        script.push(' ');
        script.push_str(&shell_quote(a.as_ref()));
    }
    script.push_str(" \"$@\"\n");
    let wrapper = alloc::format!("{}/{}", COMMANDS_BIN, name);
    if crate::fs::write(&wrapper, script.as_bytes()) {
        chmod(&wrapper, 0o755);
    }
    0
}

fn shell_quote(arg: &str) -> String {
    alloc::format!("'{}'", arg.replace('\'', "'\\''"))
}

pub fn cmd_unregister(name: &str) -> i64 {
    let uid = getuid().max(0) as u32;
    match command_owner(name) {
        None => -2,
        Some(owner) if owner != uid && uid != 0 => -13,
        Some(_) => {
            unlink(&alloc::format!("{}/{}", COMMANDS_BIN, name));
            unlink(&alloc::format!("{}/{}", COMMANDS_DIR, name))
        }
    }
}

pub struct CommandEntry {
    pub name: String,
    pub path: String,
    pub args: Vec<String>,
}

pub fn cmd_list() -> Vec<CommandEntry> {
    let mut out: Vec<CommandEntry> = Vec::new();
    for entry in read_dir(COMMANDS_DIR).unwrap_or_default() {
        if entry.is_dir {
            continue;
        }
        if let Some((_, command)) = crate::fs::read_to_string(&alloc::format!("{}/{}", COMMANDS_DIR, entry.name)).as_deref().and_then(parse_command) {
            out.push(command);
        }
    }
    if let Some(text) = crate::fs::read_to_string(LEGACY_COMMANDS) {
        for line in text.lines() {
            if let Some((_, command)) = parse_command(line) {
                if !out.iter().any(|c| c.name == command.name) {
                    out.push(command);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn cmd_run<S: AsRef<str>>(name: &str, extra: &[S], flags: u64) -> i64 {
    let Some(command) = cmd_list().into_iter().find(|c| c.name == name) else {
        return -2;
    };
    let mut args: Vec<String> = command.args.clone();
    args.extend(extra.iter().map(|a| String::from(a.as_ref())));
    spawn(&command.path, &args, flags & !4)
}

pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub kind: char,
    pub mode: u32,
    pub owner: u32,
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
pub const SYS_HAMIX_POWER: u64 = 9110;
pub const SYS_HAMIX_CPU_STATS: u64 = 9120;

pub const SPAWN_ROOT: u64 = 4;
pub const SPAWN_ENV: u64 = 8;
pub const EVENT_PIPE: u64 = 4;
pub const EVENT_SOCKET: u64 = 8;

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




pub const AUTH_SERVICE: &str = "hamix.auth";
const AUTH_TIMEOUT_MS: i64 = 10000;


pub fn auth_request(fields: &[&str]) -> i64 {
    let pid = service_lookup(AUTH_SERVICE);
    if pid < 0 {
        return -2;
    }
    let mut request = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            request.push(0);
        }
        request.extend_from_slice(f.as_bytes());
    }
    let r = msg_send(pid, &request);
    if r < 0 {
        return r;
    }
    let mut buf = [0u8; 64];
    match msg_recv_from(pid, &mut buf, AUTH_TIMEOUT_MS) {
        Some(n) => core::str::from_utf8(&buf[..n]).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(-5),
        None => -110,
    }
}

pub fn auth(user: &str, password: &str) -> i64 {
    auth_request(&["auth", user, password])
}







struct Volume {
    name: String,
    sectors: u64,
    failure: i64,
}

const IO_CHUNK_SECTORS: usize = 2048;

impl hext::BlockDevice for Volume {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), hext::Error> {
        for (i, chunk) in buf.chunks_mut(IO_CHUNK_SECTORS * 512).enumerate() {
            let r = disk_read(&self.name, sector + (i * IO_CHUNK_SECTORS) as u64, chunk);
            if r < 0 {
                self.failure = r;
                return Err(hext::Error::Io);
            }
        }
        Ok(())
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), hext::Error> {
        for (i, chunk) in buf.chunks(IO_CHUNK_SECTORS * 512).enumerate() {
            let r = disk_write(&self.name, sector + (i * IO_CHUNK_SECTORS) as u64, chunk);
            if r < 0 {
                self.failure = r;
                return Err(hext::Error::Io);
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), hext::Error> {
        Ok(())
    }

    fn sectors(&self) -> u64 {
        self.sectors
    }
}

fn volume_sectors(name: &str) -> Option<u64> {
    disk_listing().lines().find_map(|line| {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["disk", n, sectors, ..] if *n == name => sectors.parse().ok(),
            ["part", n, _, _, sectors, ..] if *n == name => sectors.parse().ok(),
            _ => None,
        }
    })
}

pub fn mkfs(device: &str, label: &str) -> i64 {
    if geteuid() != 0 {
        return -1;
    }
    let name = device.trim_start_matches("/dev/");
    let Some(sectors) = volume_sectors(name) else {
        return -19;
    };
    let mut volume = Volume { name: String::from(name), sectors, failure: 0 };
    let now = realtime().sec as u32;
    let seed = uptime_ms() ^ ((now as u64) << 20) ^ sectors;
    let label = if label.is_empty() { "hamix" } else { label };
    let formatted = match hext::Hext::format(&mut volume, hext::FormatOptions { label: String::from(label), seed, now }) {
        Ok(fs) => {
            drop(fs);
            true
        }
        Err(_) => false,
    };
    if formatted {
        disk_rescan();
        0
    } else if volume.failure < 0 {
        volume.failure
    } else {
        -5
    }
}



pub const SYS_HAMIX_TRACE: u64 = 9103;

pub fn trace_syscalls(name: &str) -> i64 {
    unsafe { syscall3(SYS_HAMIX_TRACE, name.as_ptr() as u64, name.len() as u64, 0) }
}

pub fn set_own_password(old: &str, new: &str) -> i64 {
    auth_request(&["passwd", old, new])
}


pub const SYS_SETUID: u64 = 105;
pub const SYS_SETGID: u64 = 106;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_PIPE2: u64 = 293;
pub const SYS_FCNTL: u64 = 72;
pub const SYS_HAMIX_FUTEX_WAIT: u64 = 9025;
pub const SYS_HAMIX_FUTEX_WAKE: u64 = 9026;
pub const SYS_HAMIX_WAIT_ANY: u64 = 9027;
pub const SYS_HAMIX_ABI_INFO: u64 = 9028;
pub const SYS_HAMIX_SERVICE_ACTIVATOR: u64 = 9029;
pub const SYS_HAMIX_SESSION_ATTACH: u64 = 9098;
pub const SYS_HAMIX_LINUX_CONTROL: u64 = 9111;

pub const O_NONBLOCK: u64 = 0o4000;
pub const O_CLOEXEC: u64 = 0o2000000;

pub const WAIT_FD: u32 = 1;
pub const WAIT_MAILBOX: u32 = 2;
pub const WAIT_CHILD: u32 = 3;
pub const WAIT_TIMER: u32 = 4;
pub const POLL_IN: u32 = 1;
pub const POLL_OUT: u32 = 4;
pub const POLL_HUP: u32 = 16;

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct WaitEntry {
    pub kind: u32,
    pub flags: u32,
    pub id: u64,
    pub ready: u64,
}

impl WaitEntry {
    pub fn fd(fd: u64) -> WaitEntry {
        WaitEntry { kind: WAIT_FD, flags: POLL_IN, id: fd, ready: 0 }
    }

    pub fn mailbox() -> WaitEntry {
        WaitEntry { kind: WAIT_MAILBOX, flags: 0, id: 0, ready: 0 }
    }

    pub fn child(pid: u64) -> WaitEntry {
        WaitEntry { kind: WAIT_CHILD, flags: 0, id: pid, ready: 0 }
    }

    pub fn timer_at_ms(deadline_ms: u64) -> WaitEntry {
        WaitEntry { kind: WAIT_TIMER, flags: 0, id: deadline_ms.saturating_mul(1_000_000), ready: 0 }
    }
}


pub fn futex_wait(word: &core::sync::atomic::AtomicU32, expected: u32, timeout_ns: Option<u64>) -> i64 {
    unsafe { syscall3(SYS_HAMIX_FUTEX_WAIT, word.as_ptr() as u64, expected as u64, timeout_ns.unwrap_or(u64::MAX)) }
}

pub fn futex_wake(word: &core::sync::atomic::AtomicU32, count: u32) -> i64 {
    unsafe { syscall3(SYS_HAMIX_FUTEX_WAKE, word.as_ptr() as u64, count as u64, 0) }
}

pub fn abi_info() -> (u32, String) {
    let mut buf = [0u8; 256];
    let version = unsafe { syscall3(SYS_HAMIX_ABI_INFO, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
    let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    (version.max(0) as u32, String::from_utf8_lossy(&buf[..len]).into_owned())
}


pub fn session_attach(tty: u32) -> i64 {
    unsafe { syscall1(SYS_HAMIX_SESSION_ATTACH, tty as u64) }
}

pub const LINUX_STATUS: u64 = 0;
pub const LINUX_ON: u64 = 1;
pub const LINUX_OFF: u64 = 2;

pub fn linux_control(op: u64) -> i64 {
    unsafe { syscall1(SYS_HAMIX_LINUX_CONTROL, op) }
}

pub fn exec<S: AsRef<str>, E: AsRef<str>>(path: &str, args: &[S], env: &[E]) -> i64 {
    terminal_restore();
    let path_c = cstring(path);
    let arg_c: Vec<Vec<u8>> = args.iter().map(|a| cstring(a.as_ref())).collect();
    let env_c: Vec<Vec<u8>> = env.iter().map(|e| cstring(e.as_ref())).collect();
    let mut argv: Vec<u64> = arg_c.iter().map(|a| a.as_ptr() as u64).collect();
    argv.push(0);
    let mut envp: Vec<u64> = env_c.iter().map(|e| e.as_ptr() as u64).collect();
    envp.push(0);
    unsafe { syscall3(SYS_EXECVE, path_c.as_ptr() as u64, argv.as_ptr() as u64, envp.as_ptr() as u64) }
}
