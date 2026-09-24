use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::arch::paging::{self, PAGE_SIZE};
use crate::drivers::audio;
use crate::drivers::input::keyboard::{self, Key};
use crate::drivers::video::text_mode::TEXT_CONSOLE;
use crate::fs;
use crate::task::switch::InterruptFrame;
use crate::task::{self, ipc, pipe, OpenFile, Pid};


pub mod linux;
mod native;

const NATIVE_BASE: u64 = 9000;

pub fn init() {
    init_cpu();
}

#[cfg(not(target_arch = "x86_64"))]
pub fn init_cpu() {}

#[cfg(target_arch = "x86_64")]
pub fn init_cpu() {
    use crate::arch::x86_64::{read_msr, write_msr};
    const IA32_EFER: u32 = 0xC0000080;
    const IA32_STAR: u32 = 0xC0000081;
    const IA32_LSTAR: u32 = 0xC0000082;
    const IA32_FMASK: u32 = 0xC0000084;

    let efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer | 1);
    write_msr(IA32_STAR, (0x0008u64 << 32) | (0x0018u64 << 48));
    write_msr(IA32_LSTAR, syscall_entry as *const () as u64);
    write_msr(IA32_FMASK, 0x200 | 0x400 | 0x100);
}

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;
const SYS_MMAP: u64 = 9;
const SYS_MUNMAP: u64 = 11;
const SYS_BRK: u64 = 12;
const SYS_SCHED_YIELD: u64 = 24;
const SYS_GETPID: u64 = 39;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_KILL: u64 = 62;
const SYS_UNAME: u64 = 63;
const SYS_GETCWD: u64 = 79;
const SYS_CHDIR: u64 = 80;
const SYS_RENAME: u64 = 82;
const SYS_MKDIR: u64 = 83;
const SYS_UNLINK: u64 = 87;
const SYS_CHMOD: u64 = 90;
const SYS_CHOWN: u64 = 92;
const SYS_GETUID: u64 = 102;
const SYS_GETEUID: u64 = 107;
const SYS_GETPPID: u64 = 110;
const SYS_CLOCK_GETTIME: u64 = 228;




const ENOENT: i64 = -2;
const ENOEXEC: i64 = -8;
const EPIPE: i64 = -32;
const ESRCH: i64 = -3;
const EIO: i64 = -5;
const EBADF: i64 = -9;
const ECHILD: i64 = -10;
const EAGAIN: i64 = -11;
const ENOMEM: i64 = -12;
const ENOSPC: i64 = -28;
const WRITE_CHUNK: usize = 1 << 20;
const EACCES: i64 = -13;
const EFAULT: i64 = -14;
const EBUSY: i64 = -16;
const EEXIST: i64 = -17;
const ENODEV: i64 = -19;
const ENOTDIR: i64 = -20;
const EISDIR: i64 = -21;
const EINVAL: i64 = -22;
const EMFILE: i64 = -24;
const ENOTTY: i64 = -25;
const EPERM: i64 = -1;
const ENOSYS: i64 = -38;
const ENAMETOOLONG: i64 = -36;

const O_ACCMODE: u64 = 3;
const O_WRONLY: u64 = 1;
const O_CREAT: u64 = 0o100;
const O_TRUNC: u64 = 0o1000;
const O_APPEND: u64 = 0o2000;
const O_DIRECTORY: u64 = 0o200000;



const FIRST_USER_FD: usize = 3;

fn user_slice<'a>(ptr: u64, len: u64) -> Result<&'a mut [u8], i64> {
    if len == 0 {
        return Ok(&mut []);
    }
    if !paging::is_user_range(ptr, len) {
        return Err(EFAULT);
    }
    let mapped = task::with_current(|t| t.aspace.as_ref().map(|a| a.is_mapped(ptr, len)).unwrap_or(false));
    if !mapped && !(linux::base::fault_in_range(ptr, len) && task::with_current(|t| t.aspace.as_ref().map(|a| a.is_mapped(ptr, len)).unwrap_or(false))) {
        return Err(EFAULT);
    }
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

fn user_string(ptr: u64, len: u64) -> Result<String, i64> {
    if len > 4096 {
        return Err(ENAMETOOLONG);
    }
    let bytes = user_slice(ptr, len)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn user_cstr(ptr: u64) -> Result<String, i64> {
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

fn user_args(ptr: u64, len: u64) -> Result<Vec<String>, i64> {
    if len > 64 * 1024 {
        return Err(EINVAL);
    }
    let bytes = user_slice(ptr, len)?;
    Ok(bytes.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect())
}

fn copy_out(ptr: u64, len: u64, data: &[u8]) -> i64 {
    match user_slice(ptr, len) {
        Ok(buf) => {
            let n = data.len().min(buf.len());
            buf[..n].copy_from_slice(&data[..n]);
            data.len() as i64
        }
        Err(e) => e,
    }
}

fn current_identity() -> (Pid, usize, u32, String) {
    task::with_current(|t| (t.pid, t.vt, t.uid, t.cwd.clone()))
}

fn euid() -> u32 {
    task::with_current(|t| t.uid)
}

fn absolute(path: &str) -> String {
    if path.starts_with('/') {
        return fs::absolute("", path);
    }
    let cwd = task::with_current(|t| t.cwd.clone());
    fs::absolute(&cwd, path)
}

fn path_arg(ptr: u64) -> Result<String, i64> {
    user_cstr(ptr).map(|p| absolute(&p))
}

fn lookup_arg(ptr: u64) -> Result<String, i64> {
    path_arg(ptr).map(|p| linux::base::guest_path(&p))
}

#[cfg(target_arch = "x86_64")]
fn decode(frame: &InterruptFrame) -> (u64, [u64; 6], Option<()>) {
    (frame.syscall_number(), frame.args(), None)
}

#[cfg(not(target_arch = "x86_64"))]
fn decode(frame: &InterruptFrame) -> (u64, [u64; 6], Option<linux::generic::Fixup>) {
    let number = frame.syscall_number();
    let args = frame.args();
    if task::current_tid() == 0 || !linux_abi() {
        return (number, args, None);
    }
    let call = linux::generic::translate(number, args);
    (call.number, call.args, Some(call.fixup))
}

#[cfg(target_arch = "x86_64")]
fn finish(_: Option<()>, result: i64) -> i64 {
    result
}

#[cfg(not(target_arch = "x86_64"))]
fn finish(fixup: Option<linux::generic::Fixup>, result: i64) -> i64 {
    match fixup {
        Some(fixup) => linux::generic::finish(fixup, result),
        None => result,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn handle_syscall(frame: *mut InterruptFrame, fx: *mut u8) {
    let frame = unsafe { &mut *frame };
    let (number, args, fixup) = decode(frame);
    let [a1, a2, a3, a4, a5, a6] = args;
    if let Some(result) = lockless(number, a1, a2, a3) {
        frame.set_ret(result as u64);
        return;
    }
    task::bkl::acquire();
    task::check_killed();
    crate::vt::service_pending();
    task::with_current_thread(|t| {
        t.frame = frame as *mut InterruptFrame as u64;
        t.frame_fx = fx as u64;
    });
    if number == linux::SYS_RT_SIGRETURN {
        linux::signal::sys_rt_sigreturn(frame, fx);
        linux::signal::deliver(frame, fx, None);
        task::check_killed();
        task::bkl::release();
        return;
    }
    let traced = TRACING.load(core::sync::atomic::Ordering::Relaxed) && traced_task();
    if traced {
        let path_arg = match number {
            2 | 4 | 6 | 21 | 59 | 80 | 83 | 84 | 87 | 89 => user_cstr(a1).ok(),
            257 | 262 | 263 | 267 | 269 | 332 | 439 | 258 => user_cstr(a2).ok(),
            _ => None,
        };
        crate::serial_println!("trace {}: {}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}) {} ...", task::current_tid(), number, a1, a2, a3, a4, a5, path_arg.unwrap_or_default());
    }
    let result = finish(fixup, dispatch(number, a1, a2, a3, a4, a5, a6));
    if traced {
        crate::serial_println!("trace {}: {} = {} @{}", task::current_tid(), number, result, task::uptime_ms());
    }
    task::check_killed();
    if linux_abi() && linux::signal::maybe_pending() {
        linux::signal::deliver(frame, fx, Some((number, result)));
    } else {
        frame.set_ret(result as u64);
    }
    task::bkl::release();
}

pub fn deliver_from_trap(frame: &mut InterruptFrame, fx: *mut u8) {
    if !frame.from_user() || task::current_tid() == 0 || !linux::signal::maybe_pending() {
        return;
    }
    if !task::bkl::try_acquire() {
        return;
    }
    if linux_abi() {
        linux::signal::deliver(frame, fx, None);
    }
    task::bkl::release();
}

static TRACING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static TRACE_NAME: spin::Mutex<String> = spin::Mutex::new(String::new());

fn traced_task() -> bool {
    let name = task::with_current_thread(|t| t.name.clone());
    let wanted = crate::arch::without_interrupts(|| TRACE_NAME.lock().clone());
    !wanted.is_empty() && name.contains(wanted.as_str())
}

fn sys_trace(ptr: u64, len: u64) -> i64 {
    if euid() != 0 {
        return EPERM;
    }
    let name = match user_string(ptr, len) {
        Ok(n) => n,
        Err(e) => return e,
    };
    TRACING.store(!name.is_empty(), core::sync::atomic::Ordering::Relaxed);
    crate::arch::without_interrupts(|| *TRACE_NAME.lock() = name);
    0
}

fn lockless(number: u64, a1: u64, a2: u64, a3: u64) -> Option<i64> {
    Some(match number {
        SYS_CLOCK_GETTIME => sys_clock_gettime(a1, a2),
        SYS_GETPID => task::current_pid() as i64,
        linux::SYS_GETTID => task::current_tid() as i64,
        linux::SYS_MADVISE
        | linux::SYS_MSYNC
        | linux::SYS_FADVISE64
        | linux::SYS_SYNC_FILE_RANGE
        | linux::SYS_MLOCK
        | linux::SYS_MUNLOCK
        | linux::SYS_MLOCKALL
        | linux::SYS_MUNLOCKALL
        | linux::SYS_SET_ROBUST_LIST
        | linux::SYS_SETRLIMIT
        | linux::SYS_CAPSET
        | linux::SYS_PERSONALITY
        | linux::SYS_MEMBARRIER
        | linux::SYS_SETPRIORITY
        | linux::SYS_SCHED_SETPARAM
        | linux::SYS_SCHED_SETSCHEDULER
        | linux::SYS_SCHED_GETSCHEDULER
        | linux::SYS_SCHED_GET_PRIORITY_MIN
        | linux::SYS_SCHED_GET_PRIORITY_MAX
        | linux::SYS_SCHED_SETAFFINITY => 0,
        SYS_GETPPID => task::with_current(|t| t.parent) as i64,
        SYS_GETUID => task::with_current(|t| t.ruid) as i64,
        SYS_GETEUID => euid() as i64,
        linux::SYS_GETGID => linux::base::sys_getgid(),
        linux::SYS_GETEGID => linux::base::sys_getegid(),
        linux::SYS_GETTIMEOFDAY => linux::misc::sys_gettimeofday(a1, a2),
        linux::SYS_TIME => linux::misc::sys_time(a1),
        linux::SYS_CLOCK_GETRES => linux::misc::sys_clock_getres(a1, a2),
        linux::SYS_GETCPU => linux::misc::sys_getcpu(a1, a2),
        native::SYS_HAMIX_REALTIME => crate::drivers::rtc::now() as i64,
        native::SYS_FB_GENERATION => crate::drivers::video::modes::generation() as i64,
        native::SYS_HAMIX_PROC_ALIVE => task::exists(a1 as Pid) as i64,
        native::SYS_AUDIO_VOLUME if a1 == 0 => {
            if audio::present() { audio::volume_word() } else { ENODEV }
        }
        native::SYS_AUDIO_STATUS => match audio::status(task::current_pid(), a1 as u32) {
            Some(values) => {
                let mut bytes = [0u8; 64];
                for (i, v) in values.iter().enumerate() {
                    bytes[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
                }
                copy_out(a2, a3.min(64), &bytes).min(0)
            }
            None => EBADF,
        },
        _ => return None,
    })
}

fn linux_abi() -> bool {
    task::with_current(|t| t.abi) == task::Abi::Linux
}

fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    if let Some(result) = shared(number, a1, a2, a3, a4, a5, a6) {
        return result;
    }
    #[cfg(not(target_arch = "x86_64"))]
    if let Some(result) = linux::generic::special(number) {
        return result;
    }
    let handled = if number >= NATIVE_BASE && number < 0xFFFF_0000 {
        native::dispatch(number, a1, a2, a3, a4, a5, a6)
    } else {
        linux::dispatch(number, a1, a2, a3, a4, a5, a6)
    };
    match handled {
        Some(result) => result,
        None => {
            let (pid, abi) = task::with_current(|t| (t.pid, t.abi));
            crate::debug_println!("syscall: unimplemented {} (pid {}, abi {})", number, pid, abi.name());
            ENOSYS
        }
    }
}

fn shared(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> Option<i64> {
    Some(match number {
        SYS_READ => sys_read(a1, a2, a3),
        SYS_WRITE => sys_write(a1, a2, a3),
        SYS_OPEN => with_open_flags(sys_open(a1, a2), a2),
        SYS_CLOSE => sys_close(a1),
        SYS_FSTAT => sys_fstat(a1, a2),
        SYS_LSEEK => sys_lseek(a1, a2 as i64, a3),
        SYS_MMAP => {
            if linux_abi() || a4 & 0x23 != 0 {
                linux::base::sys_mmap(a1, a2, a3, a4, a5, a6)
            } else {
                sys_mmap(a2)
            }
        }
        SYS_MUNMAP => sys_munmap(a1, a2),
        SYS_BRK => sys_brk(a1),
        SYS_SCHED_YIELD => {
            task::yield_now();
            0
        }
        SYS_GETPID => task::current_pid() as i64,
        SYS_GETPPID => task::with_current(|t| t.parent) as i64,
        SYS_GETUID => task::with_current(|t| t.ruid) as i64,
        SYS_GETEUID => euid() as i64,
        SYS_EXIT => linux::process::sys_exit(a1),
        SYS_WAIT4 if linux_abi() => linux::process::sys_wait4(a1, a2, a3, a4),
        SYS_WAIT4 => native::sys_waitpid(a1, 0),
        SYS_KILL if linux_abi() => linux::signal::sys_kill(a1 as i64, a2),
        SYS_KILL => native::sys_kill(a1),
        SYS_UNAME => sys_uname(a1),
        SYS_GETCWD => {
            let mut cwd = task::with_current(|t| t.cwd.clone()).into_bytes();
            cwd.push(0);
            copy_out(a1, a2, &cwd)
        }
        SYS_CHDIR => sys_chdir(a1),
        SYS_MKDIR if linux_abi() => linux::base::sys_mkdirat(linux::base::AT_FDCWD, a1, a2),
        SYS_MKDIR => native::sys_mkdir(a1),
        SYS_UNLINK if linux_abi() => linux::base::sys_unlinkat(linux::base::AT_FDCWD, a1, 0),
        SYS_UNLINK => native::sys_unlink(a1),
        SYS_RENAME if linux_abi() => linux::base::sys_renameat(linux::base::AT_FDCWD, a1, linux::base::AT_FDCWD, a2, 0),
        SYS_RENAME => native::sys_rename(a1, a2),
        SYS_CHMOD if linux_abi() => linux::base::sys_fchmodat(linux::base::AT_FDCWD, a1, a2),
        SYS_CHMOD => native::sys_chmod(a1, a2),
        SYS_CHOWN if linux_abi() => linux::base::sys_fchownat(linux::base::AT_FDCWD, a1, a2),
        SYS_CHOWN => native::sys_chown(a1, a2),
        SYS_CLOCK_GETTIME => sys_clock_gettime(a1, a2),
        _ => return None,
    })
}

enum Target {
    Console,
    File(usize, usize, bool),
    PipeRead(u32),
    PipeWrite(u32),
    Dir(String),
    Mem(String, alloc::sync::Arc<Vec<u8>>, usize),
    Socket(u32, bool),
    Memfd(u32, usize),
    Epoll(u32),
    Mailbox,
    Special(u32),
    Pty(u32, u32),
    PtyMaster(u32, u32),
    Bad,
}

fn is_tty(fd: u64) -> bool {
    match target(fd) {
        Target::Console => true,
        Target::PipeRead(id) | Target::PipeWrite(id) | Target::Pty(id, _) | Target::PtyMaster(id, _) => pipe::terminal(id).is_some(),
        _ => false,
    }
}

fn term_size(fd: u64) -> Option<(u16, u16)> {
    match target(fd) {
        Target::Console => Some((80, 25)),
        Target::PipeRead(id) | Target::PipeWrite(id) | Target::Pty(id, _) | Target::PtyMaster(id, _) => pipe::terminal(id),
        _ => None,
    }
}

fn target(fd: u64) -> Target {
    task::with_current(|t| match t.fds.get(fd as usize) {
        Some(Some(OpenFile::File { node, pos, append, .. })) => Target::File(*node, *pos, *append),
        Some(Some(OpenFile::PipeRead(id))) => Target::PipeRead(*id),
        Some(Some(OpenFile::PipeWrite(id))) => Target::PipeWrite(*id),
        Some(Some(OpenFile::Console)) => Target::Console,
        Some(Some(OpenFile::Dir { path, .. })) => Target::Dir(path.clone()),
        Some(Some(OpenFile::Mem { path, data, pos })) => Target::Mem(path.clone(), data.clone(), *pos),
        Some(Some(OpenFile::Socket { id, nonblock })) => Target::Socket(*id, *nonblock),
        Some(Some(OpenFile::Memfd { id, pos, .. })) => Target::Memfd(*id, *pos),
        Some(Some(OpenFile::Epoll(id))) => Target::Epoll(*id),
        Some(Some(OpenFile::Mailbox)) => Target::Mailbox,
        Some(Some(OpenFile::Special(id))) => Target::Special(*id),
        Some(Some(OpenFile::PtySlave { input, output, .. })) => Target::Pty(*input, *output),
        Some(Some(OpenFile::PtyMaster { input, output, .. })) => Target::PtyMaster(*input, *output),
        _ if fd < 3 && !t.kernel_thread => Target::Console,
        _ => Target::Bad,
    })
}

pub fn emit(fd: u64, data: &[u8]) {
    match target(fd) {
        Target::PipeWrite(id) => {
            if pipe::available(id) < 64 * 1024 {
                let copy = if pipe::is_output_terminal(id) { linux::tty::cook(data) } else { data.to_vec() };
                pipe::write(id, &copy, task::current_pid());
            }
        }
        _ => {
            let vt = task::current_vt();
            if let Some(mut console) = TEXT_CONSOLE.try_lock() {
                console.write_bytes_to(vt, data);
            }
        }
    }
}

pub fn close_all_files() {
    let files: Vec<OpenFile> = task::with_current(|t| t.fds.iter_mut().filter_map(|slot| slot.take()).collect());
    let mut written = false;
    for file in files {
        written |= file.release();
    }
    if written {
        fs::request_sync();
    }
}

fn sys_write(fd: u64, buf: u64, count: u64) -> i64 {
    match user_slice(buf, count) {
        Ok(data) => write_data(fd, data),
        Err(e) => e,
    }
}

pub fn pty_master_open(index: u32) -> bool {
    task::with_tasks(|tasks| tasks.values().any(|t| t.fds.iter().flatten().any(|f| matches!(f, OpenFile::PtyMaster { index: i, .. } if *i == index))))
}

fn write_target(fd: u64) -> Target {
    match target(fd) {
        Target::Pty(_, output) => Target::PipeWrite(output),
        Target::PtyMaster(input, _) => Target::PipeWrite(input),
        other => other,
    }
}

fn read_target(fd: u64) -> Target {
    match target(fd) {
        Target::Pty(input, _) => Target::PipeRead(input),
        Target::PtyMaster(_, output) => Target::PipeRead(output),
        other => other,
    }
}

fn write_data(fd: u64, data: &[u8]) -> i64 {
    let count = data.len();
    match write_target(fd) {
        Target::Console => {
            let vt = task::current_vt();
            if crate::task::display::owner_vt() == Some(vt) {
                if let Ok(text) = core::str::from_utf8(data) {
                    crate::serial_print!("{}", text);
                }
            }
            if linux::tty::output_cooked() {
                TEXT_CONSOLE.lock().write_bytes_to(vt, data);
            } else {
                TEXT_CONSOLE.lock().write_raw_to(vt, data);
            }
            count as i64
        }
        Target::PipeWrite(id) => {
            if fd_nonblock(fd) && pipe::available(id) >= 256 * 1024 {
                return EAGAIN;
            }
            let copy = if pipe::is_output_terminal(id) && linux::tty::output_cooked() { linux::tty::cook(data) } else { data.to_vec() };
            let written = pipe::write(id, &copy, task::current_pid());
            if written == EPIPE && linux_abi() {
                linux::signal::send(task::current_pid(), linux::signal::SIGPIPE);
            }
            if written == copy.len() as i64 { count as i64 } else { written.min(count as i64) }
        }
        Target::Socket(id, nonblock) => linux::socket::send_bytes(id, nonblock, data),
        Target::Memfd(id, pos) => {
            let mut copy = data.to_vec();
            let end = pos + copy.len();
            if ipc::shm_len(id).map(|l| (l as usize) < end).unwrap_or(false) {
                let r = ipc::shm_set_len(id, end as u64);
                if r < 0 {
                    return r;
                }
            }
            match ipc::shm_io(id, pos as u64, &mut copy, true) {
                Ok(n) => {
                    advance_pos(fd, n);
                    n as i64
                }
                Err(e) => e,
            }
        }
        Target::Special(id) => linux::special::write(fd, id, data),
        Target::PipeRead(_) | Target::Bad | Target::Dir(_) | Target::Mem(..) | Target::Epoll(_) | Target::Mailbox | Target::Pty(..) | Target::PtyMaster(..) => EBADF,
        Target::File(node, pos, append) => {
            let uid = euid();
            let mut chunk = Vec::with_capacity(count.min(WRITE_CHUNK));
            let mut done = 0usize;
            let mut end = pos;
            while done < count || count == 0 {
                let piece = (count - done).min(WRITE_CHUNK);
                chunk.clear();
                chunk.extend_from_slice(&data[done..done + piece]);
                let mut guard = fs::VFS.lock();
                let Some(vfs) = guard.as_mut() else {
                    return ENODEV;
                };
                let offset = if append { vfs.node_size(node) } else { pos + done };
                let outcome = vfs.write_node_at(node, offset, &chunk, uid);
                drop(guard);
                match outcome {
                    Ok(()) => {
                        done += piece;
                        end = offset + piece;
                    }
                    Err(e) if done == 0 => return fs_error(e),
                    Err(_) => break,
                }
                if count == 0 {
                    break;
                }
            }
            task::with_current(|t| {
                if let Some(Some(OpenFile::File { pos, written, .. })) = t.fds.get_mut(fd as usize) {
                    *pos = end;
                    *written = true;
                }
            });
            done as i64
        }
    }
}

fn key_code(key: Key) -> Option<i64> {
    Some(match key {
        Key::Char(ch) if ch.is_ascii() => ch as i64,
        Key::Char(_) => return None,
        Key::Ctrl(ch) => (ch as u8 & 0x1F) as i64,
        Key::Enter => 10,
        Key::Backspace => 8,
        Key::Tab => 9,
        Key::Up => -1,
        Key::Down => -2,
        Key::Left => -3,
        Key::Right => -4,
        Key::Home => -5,
        Key::End => -6,
        Key::Delete => -7,
        Key::PageUp => -8,
        Key::PageDown => -9,
        Key::Escape => 27,
        Key::AltTab => -20,
        Key::Super => -21,
        Key::AltF4 => -22,
        Key::SnapLeft => -23,
        Key::SnapRight => -24,
        Key::SnapUp => -25,
        Key::SnapDown => -26,
        Key::Insert => -53,
        Key::F(n) => -40 - n as i64,
    })
}

fn key_code_with_mods(key: Key) -> Option<i64> {
    let code = key_code(key)?;
    let mods = match key {
        Key::Char(_) => keyboard::last_modifiers() & keyboard::MOD_ALT,
        _ => keyboard::last_modifiers(),
    };
    Some(if mods == 0 { code } else { ((mods as i64) << 32) | (code as u32 as i64) })
}

fn pipe_key(id: u32, nonblock: bool) -> Option<i64> {
    let first = match pipe::read_byte(id, nonblock) {
        Some(b) => b,
        None => return if nonblock && !pipe::readable(id) { None } else { Some(4) },
    };
    Some(match first {
        0x1b => {
            if pipe::available(id) == 0 {
                sleep_ms(8);
            }
            if pipe::available(id) == 0 {
                return Some(27);
            }
            let second = pipe::read_byte(id, true).unwrap_or(0);
            if second != b'[' && second != b'O' {
                return Some(27);
            }
            let mut params = String::new();
            let final_byte = loop {
                match pipe::read_byte(id, true) {
                    Some(b) if b.is_ascii_digit() || b == b';' => params.push(b as char),
                    Some(b) => break b,
                    None => break 0,
                }
            };
            let first_param = params.split(';').next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
            let mods = params.split(';').nth(1).and_then(|p| p.parse::<i64>().ok()).map(|m| (m - 1).clamp(0, 7)).unwrap_or(0);
            let code = match final_byte {
                b'A' => -1,
                b'B' => -2,
                b'C' => -4,
                b'D' => -3,
                b'H' => -5,
                b'F' => -6,
                b'P' => -41,
                b'Q' => -42,
                b'R' => -43,
                b'S' => -44,
                b'Z' => 9,
                b'~' => match first_param {
                    1 | 7 => -5,
                    4 | 8 => -6,
                    2 => -53,
                    3 => -7,
                    5 => -8,
                    6 => -9,
                    11..=15 => -30 - first_param as i64,
                    17..=21 => -29 - first_param as i64,
                    23 | 24 => -28 - first_param as i64,
                    _ => return pipe_key(id, true).or(Some(27)),
                },
                _ => return pipe_key(id, true).or(Some(27)),
            };
            if mods == 0 { code } else { (mods << 32) | (code as u32 as i64) }
        }
        b'\r' | b'\n' => 10,
        0x7f | 0x08 => 8,
        other => other as i64,
    })
}

fn poll_key() -> Option<i64> {
    if let Target::PipeRead(id) = read_target(0) {
        return pipe_key(id, true);
    }
    loop {
        let key = keyboard::read_key()?;
        if let Some(code) = key_code_with_mods(key) {
            return Some(code);
        }
    }
}

const KEY_RESIZE: i64 = -30;

fn read_key_code() -> i64 {
    if let Target::PipeRead(id) = read_target(0) {
        let generation = pipe::terminal_generation(id);
        loop {
            if let Some(code) = pipe_key(id, true) {
                return code;
            }
            if generation.is_some() && pipe::terminal_generation(id) != generation {
                return KEY_RESIZE;
            }
            task::block(task::WAIT_PIPE, Some(task::TICK_HZ / 10), task::input_seq());
            task::check_killed();
        }
    }
    loop {
        let seq = task::input_seq();
        if let Some(code) = poll_key() {
            return code;
        }
        task::block(task::WAIT_INPUT, Some(task::TICK_HZ / 4), seq);
        task::check_killed();
        crate::vt::service_pending();
    }
}

fn packet_master(fd: u64) -> bool {
    let index = task::with_current(|t| match t.fds.get(fd as usize) {
        Some(Some(OpenFile::PtyMaster { index, .. })) => Some(*index),
        _ => None,
    });
    index.map(task::pty::packet).unwrap_or(false)
}

fn sys_read(fd: u64, buf: u64, count: u64) -> i64 {
    if count > 1 && packet_master(fd) {
        let n = sys_read_plain(fd, buf + 1, count - 1);
        if n > 0 {
            let r = copy_out(buf, 1, &[0u8]);
            return if r < 0 { r } else { n + 1 };
        }
        return n;
    }
    sys_read_plain(fd, buf, count)
}

fn sys_read_plain(fd: u64, buf: u64, count: u64) -> i64 {
    if let Err(e) = user_slice(buf, count) {
        return e;
    }
    if linux_abi() {
        if let Some(n) = linux::tty::read(fd, buf, count) {
            return n;
        }
    }
    match read_target(fd) {
        Target::Console => {
            let mut bytes = Vec::new();
            while (bytes.len() as u64) < count {
                let seq = task::input_seq();
                match keyboard::read_key() {
                    Some(Key::Enter) => {
                        bytes.push(b'\n');
                        break;
                    }
                    Some(Key::Char(ch)) if ch.is_ascii() => bytes.push(ch as u8),
                    Some(Key::Ctrl('d')) if bytes.is_empty() => break,
                    Some(_) => {}
                    None => {
                        task::block(task::WAIT_INPUT, Some(task::TICK_HZ / 4), seq);
                        task::check_killed();
                        crate::vt::service_pending();
                    }
                }
            }
            copy_out(buf, count, &bytes)
        }
        Target::PipeRead(id) => {
            let mut tmp = alloc::vec![0u8; count.min(64 * 1024) as usize];
            let n = pipe::read(id, &mut tmp, fd_nonblock(fd));
            if n > 0 {
                if pipe::is_input_terminal(id) {
                    tmp[..n as usize].iter_mut().filter(|b| **b == b'\r').for_each(|b| *b = b'\n');
                }
                copy_out(buf, count, &tmp[..n as usize]);
            }
            n
        }
        Target::Dir(_) => EISDIR,
        Target::Socket(id, nonblock) => linux::socket::recv_bytes(id, nonblock, buf, count),
        Target::Memfd(id, pos) => {
            let mut tmp = alloc::vec![0u8; count.min(64 << 20) as usize];
            match ipc::shm_io(id, pos as u64, &mut tmp, false) {
                Ok(n) => {
                    let r = copy_out(buf, n as u64, &tmp[..n]);
                    if r < 0 {
                        return r;
                    }
                    advance_pos(fd, n);
                    n as i64
                }
                Err(e) => e,
            }
        }
        Target::Special(id) => linux::special::read(fd, id, buf, count),
        Target::Epoll(_) | Target::Mailbox => EINVAL,
        Target::Mem(_, data, pos) => {
            let start = pos.min(data.len());
            let n = copy_out(buf, count, &data[start..]).min(count as i64).min((data.len() - start) as i64);
            if n > 0 {
                task::with_current(|t| {
                    if let Some(Some(OpenFile::Mem { pos, .. })) = t.fds.get_mut(fd as usize) {
                        *pos += n as usize;
                    }
                });
            }
            n
        }
        Target::File(node, pos, _) => {
            let out = match user_slice(buf, count.min(64 << 20)) {
                Ok(o) => o,
                Err(e) => return e,
            };
            let n = {
                let mut guard = fs::VFS.lock();
                let Some(vfs) = guard.as_mut() else {
                    return ENODEV;
                };
                match vfs.read_node_at(node, pos, out) {
                    Ok(n) => n,
                    Err(_) => return EISDIR,
                }
            };
            task::with_current(|t| {
                if let Some(Some(OpenFile::File { pos, .. })) = t.fds.get_mut(fd as usize) {
                    *pos += n;
                }
            });
            n as i64
        }
        _ => EBADF,
    }
}

fn advance_pos(fd: u64, n: usize) {
    task::with_current(|t| match t.fds.get_mut(fd as usize) {
        Some(Some(OpenFile::File { pos, .. })) | Some(Some(OpenFile::Mem { pos, .. })) | Some(Some(OpenFile::Memfd { pos, .. })) => *pos += n,
        _ => {}
    });
}

pub fn fd_flag(bits: &[u64; 4], fd: u64) -> bool {
    fd < 256 && bits[fd as usize / 64] & (1 << (fd % 64)) != 0
}

fn put_flag(bits: &mut [u64; 4], fd: u64, on: bool) {
    if fd < 256 {
        if on {
            bits[fd as usize / 64] |= 1 << (fd % 64);
        } else {
            bits[fd as usize / 64] &= !(1 << (fd % 64));
        }
    }
}

pub fn set_cloexec(fd: u64, on: bool) {
    task::with_current(|t| put_flag(&mut t.cloexec, fd, on));
}

pub fn set_fd_nonblock(fd: u64, on: bool) {
    task::with_current(|t| put_flag(&mut t.nonblock, fd, on));
}

pub fn fd_nonblock(fd: u64) -> bool {
    task::with_current(|t| fd_flag(&t.nonblock, fd))
}

fn with_open_flags(result: i64, flags: u64) -> i64 {
    if result >= 0 {
        set_cloexec(result as u64, flags & 0o2000000 != 0);
        set_fd_nonblock(result as u64, flags & 0o4000 != 0);
        set_fd_access(result as u64, flags & O_ACCMODE);
    }
    result
}

pub fn set_fd_access(fd: u64, mode: u64) {
    task::with_current(|t| {
        if let Some(Some(OpenFile::File { access, .. })) | Some(Some(OpenFile::Memfd { access, .. })) = t.fds.get_mut(fd as usize) {
            *access = mode.min(2) as u8;
        }
    });
}

pub fn fd_access(fd: u64) -> u64 {
    task::with_current(|t| match t.fds.get(fd as usize) {
        Some(Some(OpenFile::File { access, .. })) | Some(Some(OpenFile::Memfd { access, .. })) => *access as u64,
        _ => 2,
    })
}

fn install_fd(file: OpenFile) -> i64 {
    let leftover = task::with_current(|t| {
        if !t.fds.iter().skip(FIRST_USER_FD).any(|f| f.is_none()) && t.fds.len() < 256 {
            let grow = (t.fds.len() + 32).min(256);
            t.fds.resize_with(grow, || None);
        }
        match t.fds.iter().skip(FIRST_USER_FD).position(|f| f.is_none()) {
            Some(slot) => {
                t.fds[slot + FIRST_USER_FD] = Some(file);
                put_flag(&mut t.cloexec, (slot + FIRST_USER_FD) as u64, false);
                put_flag(&mut t.nonblock, (slot + FIRST_USER_FD) as u64, false);
                Ok((slot + FIRST_USER_FD) as i64)
            }
            None => Err(file),
        }
    });
    match leftover {
        Ok(fd) => fd,
        Err(file) => {
            file.release();
            EMFILE
        }
    }
}

fn sys_open(path_ptr: u64, flags: u64) -> i64 {
    match lookup_arg(path_ptr) {
        Ok(path) => open_path(&path, flags),
        Err(e) => e,
    }
}

fn open_device(path: &str) -> Option<i64> {
    if path == "/dev/ptmx" || path == "/dev/pts/ptmx" {
        let (index, input, output) = task::pty::create();
        return Some(install_fd(OpenFile::PtyMaster { index, input, output }));
    }
    if let Some(n) = path.strip_prefix("/dev/pts/") {
        let index: u32 = n.parse().ok()?;
        return Some(match task::pty::open_slave(index) {
            Some((input, output)) => install_fd(OpenFile::PtySlave { index, input, output }),
            None => ENOENT,
        });
    }
    if path == "/dev/tty" {
        let copy = task::with_current(|t| t.fds.first().and_then(|f| f.as_ref()).map(|f| f.duplicate()));
        return Some(match copy {
            Some(file) if is_tty(0) => install_fd(file),
            Some(file) => {
                file.release();
                -6
            }
            None if is_tty(0) => install_fd(OpenFile::Console),
            None => -6,
        });
    }
    None
}

fn open_path(path: &str, flags: u64) -> i64 {
    if let Some(result) = open_device(path) {
        return result;
    }
    if let Some(result) = linux::base::open_special(path, flags) {
        return result;
    }
    let path = String::from(path);
    let uid = euid();
    let node = {
        let mut guard = fs::VFS.lock();
        let Some(vfs) = guard.as_mut() else {
            return ENODEV;
        };
        let root = vfs.root_id();
        let id = match vfs.resolve(root, &path) {
            Some(id) => id,
            None if flags & O_CREAT != 0 => match vfs.create_file(root, &path, Vec::new(), uid) {
                Ok(id) => id,
                Err(e) => return fs_error(e),
            },
            None => return ENOENT,
        };
        if vfs.is_dir(id) {
            if flags & O_ACCMODE != 0 || (flags & O_DIRECTORY == 0 && task::with_current(|t| t.abi) != task::Abi::Linux) {
                return EISDIR;
            }
            if !vfs.can_read(id, uid) {
                return EACCES;
            }
            let entries = linux::base::vfs_dir_entries(vfs, id);
            drop(guard);
            let entries = linux::base::merge_proc_entries(&path, entries);
            return install_fd(OpenFile::Dir { path, entries: alloc::sync::Arc::new(entries), pos: 0 });
        }
        if flags & O_DIRECTORY != 0 {
            return ENOTDIR;
        }
        if flags & O_ACCMODE != O_WRONLY && !vfs.can_read(id, uid) {
            return EACCES;
        }
        if flags & O_ACCMODE != 0 && !vfs.can_write(id, uid) {
            return EACCES;
        }
        if flags & O_TRUNC != 0 && vfs.truncate_node(id, uid).is_err() {
            return EACCES;
        }
        id
    };
    install_fd(OpenFile::File { node, pos: 0, append: flags & O_APPEND != 0, written: flags & O_TRUNC != 0, access: (flags & O_ACCMODE).min(2) as u8 })
}

fn sys_close(fd: u64) -> i64 {
    let closed = task::with_current(|t| {
        put_flag(&mut t.cloexec, fd, false);
        put_flag(&mut t.nonblock, fd, false);
        t.fds.get_mut(fd as usize).and_then(|slot| slot.take())
    });
    match closed {
        Some(file) => {
            if file.release() {
                fs::request_sync();
            }
            0
        }
        None => EBADF,
    }
}

pub(crate) struct LinuxStat {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    size: u64,
    nlink: u64,
}

fn node_stat(vfs: &fs::Vfs, node: usize) -> LinuxStat {
    let stat = vfs.stat(node);
    if let Some(target) = vfs.peek_link(node) {
        return LinuxStat { dev: stat.dev as u64 + 1, ino: node as u64 + 1, mode: 0o120777, uid: stat.owner, size: target.len() as u64, nlink: 0 };
    }
    open_node_stat(vfs, node)
}

fn open_node_stat(vfs: &fs::Vfs, node: usize) -> LinuxStat {
    let stat = vfs.stat(node);
    let kind = match stat.kind {
        fs::KIND_DIR => 0o040000,
        fs::KIND_DEVICE => 0o020000,
        fs::KIND_FILE if stat.size == 0 && linux::socket::is_socket_path(&vfs.path_of(node)) => 0o140000,
        _ => 0o100000,
    };
    LinuxStat { dev: stat.dev as u64 + 1, ino: node as u64 + 1, mode: kind | stat.mode as u32, uid: stat.owner, size: stat.size, nlink: 0 }
}

fn links(st: &LinuxStat) -> u64 {
    if st.nlink > 0 {
        st.nlink
    } else if st.mode & 0o170000 == 0o040000 {
        2
    } else {
        1
    }
}

#[cfg(target_arch = "x86_64")]
fn write_stat(statbuf: u64, st: &LinuxStat) -> i64 {
    let buf = match user_slice(statbuf, 144) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let mtime = crate::drivers::rtc::boot_epoch();
    buf.fill(0);
    buf[0..8].copy_from_slice(&st.dev.to_le_bytes());
    buf[8..16].copy_from_slice(&st.ino.to_le_bytes());
    buf[16..24].copy_from_slice(&links(st).to_le_bytes());
    buf[24..28].copy_from_slice(&st.mode.to_le_bytes());
    buf[28..32].copy_from_slice(&st.uid.to_le_bytes());
    buf[32..36].copy_from_slice(&st.uid.to_le_bytes());
    buf[48..56].copy_from_slice(&st.size.to_le_bytes());
    buf[56..64].copy_from_slice(&4096u64.to_le_bytes());
    buf[64..72].copy_from_slice(&st.size.div_ceil(512).to_le_bytes());
    for field in [72usize, 88, 104] {
        buf[field..field + 8].copy_from_slice(&mtime.to_le_bytes());
    }
    0
}

#[cfg(not(target_arch = "x86_64"))]
fn write_stat(statbuf: u64, st: &LinuxStat) -> i64 {
    let buf = match user_slice(statbuf, 128) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let mtime = crate::drivers::rtc::boot_epoch();
    buf.fill(0);
    buf[0..8].copy_from_slice(&st.dev.to_le_bytes());
    buf[8..16].copy_from_slice(&st.ino.to_le_bytes());
    buf[16..20].copy_from_slice(&st.mode.to_le_bytes());
    buf[20..24].copy_from_slice(&(links(st) as u32).to_le_bytes());
    buf[24..28].copy_from_slice(&st.uid.to_le_bytes());
    buf[28..32].copy_from_slice(&st.uid.to_le_bytes());
    buf[48..56].copy_from_slice(&st.size.to_le_bytes());
    buf[56..60].copy_from_slice(&4096u32.to_le_bytes());
    buf[64..72].copy_from_slice(&st.size.div_ceil(512).to_le_bytes());
    for field in [72usize, 88, 104] {
        buf[field..field + 8].copy_from_slice(&mtime.to_le_bytes());
    }
    0
}

fn sys_fstat(fd: u64, statbuf: u64) -> i64 {
    match fd_stat(fd) {
        Ok(st) => write_stat(statbuf, &st),
        Err(e) => e,
    }
}

fn sys_statx(dirfd: u64, path_ptr: u64, flags: u64, statxbuf: u64) -> i64 {
    let st = if flags & linux::base::AT_EMPTY_PATH != 0 && matches!(user_cstr(path_ptr).as_deref(), Ok("")) {
        fd_stat(dirfd)
    } else {
        linux::base::stat_at(dirfd, path_ptr, flags & linux::base::AT_SYMLINK_NOFOLLOW == 0)
    };
    let st = match st {
        Ok(st) => st,
        Err(e) => return e,
    };
    let buf = match user_slice(statxbuf, 256) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let mtime = crate::drivers::rtc::boot_epoch();
    buf.fill(0);
    buf[0..4].copy_from_slice(&0x7FFu32.to_le_bytes());
    buf[4..8].copy_from_slice(&4096u32.to_le_bytes());
    buf[16..20].copy_from_slice(&1u32.to_le_bytes());
    buf[20..24].copy_from_slice(&st.uid.to_le_bytes());
    buf[24..28].copy_from_slice(&st.uid.to_le_bytes());
    buf[28..30].copy_from_slice(&(st.mode as u16).to_le_bytes());
    buf[32..40].copy_from_slice(&st.ino.to_le_bytes());
    buf[40..48].copy_from_slice(&st.size.to_le_bytes());
    buf[48..56].copy_from_slice(&st.size.div_ceil(512).to_le_bytes());
    for field in [64usize, 80, 96, 112] {
        buf[field..field + 8].copy_from_slice(&mtime.to_le_bytes());
    }
    buf[140..144].copy_from_slice(&(st.dev as u32).to_le_bytes());
    0
}

fn sys_close_range(first: u64, last: u64, flags: u64) -> i64 {
    const CLOSE_RANGE_CLOEXEC: u64 = 4;
    if first > last {
        return EINVAL;
    }
    let count = task::with_current(|t| t.fds.len()) as u64;
    let end = last.min(count.saturating_sub(1));
    for fd in first..=end {
        if flags & CLOSE_RANGE_CLOEXEC != 0 {
            task::with_current(|t| put_flag(&mut t.cloexec, fd, true));
        } else {
            sys_close(fd);
        }
    }
    0
}

fn fd_stat(fd: u64) -> Result<LinuxStat, i64> {
    Ok(match target(fd) {
        Target::File(node, _, _) => {
            let guard = fs::VFS.lock();
            match guard.as_ref() {
                Some(v) => open_node_stat(v, node),
                None => LinuxStat { dev: 1, ino: 1, mode: 0o100644, uid: 0, size: 0, nlink: 0 },
            }
        }
        Target::PipeRead(id) => LinuxStat { dev: 0, ino: id as u64 + 1, mode: 0o010600, uid: euid(), size: pipe::available(id) as u64, nlink: 0 },
        Target::PipeWrite(id) => LinuxStat { dev: 0, ino: id as u64 + 1, mode: 0o010600, uid: euid(), size: 0, nlink: 0 },
        Target::Pty(id, _) => LinuxStat { dev: 0x88, ino: id as u64, mode: 0o020620, uid: euid(), size: 0, nlink: 0 },
        Target::PtyMaster(id, output) => LinuxStat { dev: 0x05, ino: id as u64, mode: 0o020666, uid: euid(), size: pipe::available(output) as u64, nlink: 0 },
        Target::Console => {
            let guard = fs::VFS.lock();
            match guard.as_ref().and_then(|v| v.resolve(v.root_id(), "/dev/console").map(|id| node_stat(v, id))) {
                Some(st) => st,
                None => LinuxStat { dev: 0, ino: fd + 1, mode: 0o020620, uid: euid(), size: 0, nlink: 0 },
            }
        }
        Target::Dir(path) => linux::base::stat_path(&path, true)?,
        Target::Mem(path, data, _) => match linux::base::stat_path(&path, true) {
            Ok(mut st) => {
                st.size = data.len() as u64;
                st
            }
            Err(_) => LinuxStat { dev: 0x50, ino: 1, mode: 0o100444, uid: 0, size: data.len() as u64, nlink: 0 },
        },
        Target::Socket(id, _) => LinuxStat { dev: 0x51, ino: id as u64, mode: 0o140777, uid: euid(), size: 0, nlink: 0 },
        Target::Memfd(id, _) => LinuxStat { dev: 0x52, ino: id as u64, mode: 0o100777, uid: euid(), size: ipc::shm_len(id).unwrap_or(0), nlink: 0 },
        Target::Epoll(id) => LinuxStat { dev: 0x53, ino: id as u64, mode: 0o600, uid: euid(), size: 0, nlink: 0 },
        Target::Mailbox => LinuxStat { dev: 0x53, ino: 0, mode: 0o600, uid: euid(), size: 0, nlink: 0 },
        Target::Special(id) => LinuxStat { dev: 0x54, ino: id as u64, mode: 0o600, uid: euid(), size: 0, nlink: 0 },
        Target::Bad => return Err(EBADF),
    })
}

fn sys_lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    let (pos, len) = match target(fd) {
        Target::File(node, pos, _) => (pos, fs::VFS.lock().as_ref().map(|v| v.node_size(node)).unwrap_or(0) as i64),
        Target::Mem(_, data, pos) => (pos, data.len() as i64),
        Target::Memfd(id, pos) => (pos, ipc::shm_len(id).unwrap_or(0) as i64),
        Target::Dir(_) if whence == 0 => (0, 0),
        Target::Bad => return EBADF,
        _ => return -29,
    };
    let base = match whence {
        0 => 0,
        1 => pos as i64,
        2 => len,
        _ => return EINVAL,
    };
    let target_pos = base + offset;
    if target_pos < 0 {
        return EINVAL;
    }
    task::with_current(|t| match t.fds.get_mut(fd as usize) {
        Some(Some(OpenFile::File { pos, .. })) | Some(Some(OpenFile::Mem { pos, .. })) | Some(Some(OpenFile::Dir { pos, .. })) | Some(Some(OpenFile::Memfd { pos, .. })) => *pos = target_pos as usize,
        _ => {}
    });
    target_pos
}

fn sys_writev(fd: u64, iov: u64, count: u64) -> i64 {
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
        let n = sys_write(fd, base, len);
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
    }
    total
}

fn fs_error(e: &str) -> i64 {
    if e == "not a regular file" {
        return EISDIR;
    }
    match e {
        "no such file or directory" | "bad path" => ENOENT,
        "already exists" | "destination exists" => EEXIST,
        "parent is not a directory" | "not a directory" | "destination is not a directory" => ENOTDIR,
        "device or resource busy" => EBUSY,
        "cannot move a directory into itself" => EINVAL,
        fs::NO_SPACE => ENOSPC,
        _ => EACCES,
    }
}


fn sys_chdir(path_ptr: u64) -> i64 {
    let path = match lookup_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let ok = {
        let guard = fs::VFS.lock();
        guard.as_ref().and_then(|v| v.resolve(v.root_id(), &path).map(|id| v.is_dir(id)))
    };
    match ok {
        Some(true) => {
            task::with_current(|t| t.cwd = path);
            0
        }
        Some(false) => ENOTDIR,
        None => ENOENT,
    }
}

fn vfs_op(f: impl FnOnce(&mut fs::Vfs, u32) -> Result<(), &'static str>) -> i64 {
    let uid = euid();
    let result = fs::VFS.lock().as_mut().map(|v| f(v, uid));
    match result {
        Some(Ok(())) => {
            fs::request_sync();
            0
        }
        Some(Err(e)) => fs_error(e),
        None => ENODEV,
    }
}







fn sys_brk(addr: u64) -> i64 {
    task::with_current(|t| {
        if addr == 0 || addr < t.brk_start || addr >= paging::USER_MMAP_BASE {
            return t.brk as i64;
        }
        let Some(aspace) = t.aspace.as_mut() else {
            return t.brk as i64;
        };
        let old_top = (t.brk + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let new_top = (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        if new_top > old_top {
            if !aspace.map_zero(old_top, new_top - old_top) {
                return t.brk as i64;
            }
        } else if new_top < old_top {
            aspace.unmap_range(new_top, old_top - new_top);
            paging::flush_tlb();
        }
        t.brk = addr;
        addr as i64
    })
}

fn sys_mmap(len: u64) -> i64 {
    if len == 0 || len > 1 << 32 {
        return EINVAL;
    }
    let size = (len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    task::with_current(|t| {
        let reuse = t.mmap_free.iter().position(|(_, free)| *free >= size + PAGE_SIZE);
        let base = match reuse {
            Some(i) => t.mmap_free[i].0,
            None => t.mmap_next,
        };
        if reuse.is_none() && base + size >= paging::USER_SHM_BASE {
            return ENOMEM;
        }
        let Some(aspace) = t.aspace.as_mut() else {
            return ENOMEM;
        };
        if !aspace.map_zero(base, size) {
            aspace.unmap_range(base, size);
            return ENOMEM;
        }
        match reuse {
            Some(i) => {
                let (start, free) = t.mmap_free[i];
                let used = size + PAGE_SIZE;
                if free == used {
                    t.mmap_free.remove(i);
                } else {
                    t.mmap_free[i] = (start + used, free - used);
                }
            }
            None => t.mmap_next = base + size + PAGE_SIZE,
        }
        base as i64
    })
}

fn release_mmap_range(t: &mut task::Task, addr: u64, len: u64) {
    let start = addr & !(PAGE_SIZE - 1);
    let span = ((addr + len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)) - start + PAGE_SIZE;
    let mut begin = start;
    let mut end = start + span;
    let mut i = 0;
    while i < t.mmap_free.len() {
        let (s, l) = t.mmap_free[i];
        if s + l >= begin && s <= end {
            begin = begin.min(s);
            end = end.max(s + l);
            t.mmap_free.swap_remove(i);
            i = 0;
        } else {
            i += 1;
        }
    }
    if end >= t.mmap_next {
        t.mmap_next = begin.max(paging::USER_MMAP_BASE);
    } else {
        t.mmap_free.push((begin, end - begin));
    }
}

fn sys_munmap(addr: u64, len: u64) -> i64 {
    let shared = ipc::unmap_range(task::current_pid(), addr, len);
    if addr >= paging::USER_SHM_BASE && shared {
        return 0;
    }
    if addr < paging::USER_MMAP_BASE || addr + len > paging::USER_SHM_BASE {
        return EINVAL;
    }
    task::with_current(|t| {
        if let Some(aspace) = t.aspace.as_mut() {
            aspace.unmap_range(addr, len);
        }
        if addr >= paging::USER_MMAP_BASE && addr < t.mmap_next {
            release_mmap_range(t, addr, len);
        }
    });
    paging::flush_tlb();
    0
}

fn sys_uname(buf: u64) -> i64 {
    const FIELD: usize = 65;
    let out = match user_slice(buf, (FIELD * 6) as u64) {
        Ok(o) => o,
        Err(e) => return e,
    };
    out.fill(0);
    let hostname = fs::VFS
        .lock()
        .as_mut()
        .and_then(|v| v.read(0, "/etc/hostname").ok())
        .map(|b| String::from(String::from_utf8_lossy(&b).trim()))
        .unwrap_or_else(|| String::from("hamix"));
    let fields: [&[u8]; 6] = [b"HamixOS", hostname.as_bytes(), b"0.6.1", b"#1 HamixOS", crate::arch::MACHINE.as_bytes(), b"localdomain"];
    for (i, field) in fields.iter().enumerate() {
        let n = field.len().min(FIELD - 1);
        out[i * FIELD..i * FIELD + n].copy_from_slice(&field[..n]);
    }
    0
}

fn sys_clock_gettime(clock: u64, ts: u64) -> i64 {
    let out = match user_slice(ts, 16) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let ms = task::uptime_ms();
    let secs = if clock == 0 { crate::drivers::rtc::boot_epoch() + ms / 1000 } else { ms / 1000 };
    let nanos = (ms % 1000) * 1_000_000;
    out[0..8].copy_from_slice(&(secs as i64).to_le_bytes());
    out[8..16].copy_from_slice(&(nanos as i64).to_le_bytes());
    0
}



fn sleep_ms(ms: u64) {
    let deadline = task::ticks() + task::ms_to_ticks(ms);
    while task::ticks() < deadline {
        task::sleep_ticks(deadline - task::ticks());
        task::check_killed();
    }
}

fn sleep_interruptible(ms: u64) -> Result<(), u64> {
    let deadline = task::ticks() + task::ms_to_ticks(ms);
    let linux = linux_abi();
    while task::ticks() < deadline {
        if linux && linux::signal::pending() {
            return Err((deadline - task::ticks()) * 1000 / task::TICK_HZ);
        }
        task::sleep_ticks((deadline - task::ticks()).min(task::TICK_HZ / 10));
        task::check_killed();
    }
    Ok(())
}

fn pipe_fds_readable() -> bool {
    let ids: Vec<u32> = task::with_current(|t| {
        t.fds
            .iter()
            .filter_map(|f| match f {
                Some(OpenFile::PipeRead(id)) | Some(OpenFile::PtyMaster { output: id, .. }) => Some(*id),
                _ => None,
            })
            .collect()
    });
    ids.into_iter().any(pipe::readable)
}











pub fn cpu_stats_text() -> String {
    use crate::arch::smp;
    let mut text = String::new();
    for cpu in 0..smp::count() {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            cpu,
            smp::BUSY_TICKS[cpu].load(core::sync::atomic::Ordering::Relaxed) * 1000 / task::TICK_HZ,
            smp::TOTAL_TICKS[cpu].load(core::sync::atomic::Ordering::Relaxed) * 1000 / task::TICK_HZ,
            smp::apic_id_of(cpu),
            if smp::online(cpu) { 1 } else { 0 }
        ));
    }
    text
}













fn sys_pipe(out: u64, flags: u64) -> i64 {
    if let Err(e) = user_slice(out, 8) {
        return e;
    }
    let id = pipe::create();
    let read_fd = install_fd(OpenFile::PipeRead(id));
    if read_fd < 0 {
        pipe::release(id, false);
        return read_fd;
    }
    let write_fd = install_fd(OpenFile::PipeWrite(id));
    if write_fd < 0 {
        sys_close(read_fd as u64);
        return write_fd;
    }
    let mut raw = [0u8; 8];
    raw[0..4].copy_from_slice(&(read_fd as u32).to_le_bytes());
    raw[4..8].copy_from_slice(&(write_fd as u32).to_le_bytes());
    copy_out(out, 8, &raw);
    with_open_flags(read_fd, flags);
    with_open_flags(write_fd, flags);
    0
}



#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "swapgs",
        "mov gs:[8], rsp",
        "mov rsp, gs:[0]",
        "push {user_ds}",
        "push qword ptr gs:[8]",
        "push r11",
        "push {user_cs}",
        "push rcx",
        "swapgs",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rbp, rsp",
        "sub rsp, 512",
        "and rsp, -16",
        "fxsave64 [rsp]",
        "mov rdi, rbp",
        "mov rsi, rsp",
        "sti",
        "call {handler}",
        "cli",
        "fxrstor64 [rsp]",
        "mov rsp, rbp",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "iretq",
        user_ds = const crate::arch::x86_64::gdt::USER_DS as u64,
        user_cs = const crate::arch::x86_64::gdt::USER_CS as u64,
        handler = sym handle_syscall,
    );
}

pub fn notify_child_exit(parent: Pid, _child: Pid) {
    if parent != 0 && task::with_task(parent, |t| t.abi == task::Abi::Linux).unwrap_or(false) {
        linux::signal::send(parent, linux::signal::SIGCHLD);
    }
}

pub fn notify_resize(pipe_id: u32) {
    let targets: Vec<Pid> = task::with_tasks(|tasks| {
        tasks
            .values()
            .filter(|t| t.leader == t.pid && t.abi == task::Abi::Linux && t.state != task::State::Zombie)
            .filter(|t| t.fds.iter().flatten().any(|f| matches!(f, OpenFile::PipeRead(id) | OpenFile::PipeWrite(id) | OpenFile::PtySlave { input: id, .. } if *id == pipe_id)))
            .map(|t| t.pid)
            .collect()
    });
    for pid in targets {
        linux::signal::send(pid, linux::signal::SIGWINCH);
    }
}
