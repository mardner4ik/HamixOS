use alloc::string::String;
use alloc::vec::Vec;

use crate::syscall::{copy_out, user_cstr, user_slice, EAGAIN, ECHILD, EFAULT, EINVAL, ENOENT, ENOEXEC, ENOMEM};
use crate::arch::paging;
use crate::task::switch::{InterruptFrame, FX_SIZE};
use crate::task::{self, elf, Abi, OpenFile, Pid, ReapResult, SigAction};

const CLONE_VM: u64 = 0x100;
const CLONE_SIGHAND: u64 = 0x800;
const CLONE_VFORK: u64 = 0x4000;
const CLONE_PARENT: u64 = 0x8000;
const CLONE_THREAD: u64 = 0x10000;
const CLONE_SETTLS: u64 = 0x80000;
const CLONE_PARENT_SETTID: u64 = 0x100000;
const CLONE_CHILD_CLEARTID: u64 = 0x200000;
const CLONE_CHILD_SETTID: u64 = 0x1000000;

const WNOHANG: u64 = 1;
const WNOWAIT: u64 = 0x0100_0000;

fn current_frame() -> Option<(InterruptFrame, [u8; FX_SIZE])> {
    let (frame, fx) = task::with_current_thread(|t| (t.frame, t.frame_fx));
    if frame == 0 || fx == 0 {
        return None;
    }
    let frame = unsafe { core::ptr::read(frame as *const InterruptFrame) };
    let mut copy = [0u8; FX_SIZE];
    unsafe { core::ptr::copy_nonoverlapping(fx as *const u8, copy.as_mut_ptr(), FX_SIZE) };
    Some((frame, copy))
}

fn write_tid(addr: u64, tid: Pid) {
    if addr != 0 {
        copy_out(addr, 4, &tid.to_le_bytes());
    }
}

fn write_tid_in(aspace: &paging::AddressSpace, addr: u64, tid: Pid) {
    if addr != 0 && aspace.is_mapped(addr, 4) {
        aspace.write_bytes(addr, &tid.to_le_bytes());
    }
}

pub fn sys_clone(flags: u64, stack: u64, parent_tid: u64, child_tid: u64, tls: u64) -> i64 {
    let Some((mut frame, fx)) = current_frame() else {
        return EINVAL;
    };
    frame.set_ret(0);
    if stack != 0 {
        frame.set_sp(stack);
    }
    let thread = flags & CLONE_THREAD != 0;
    if thread && flags & (CLONE_VM | CLONE_SIGHAND) != (CLONE_VM | CLONE_SIGHAND) {
        return EINVAL;
    }
    let fs_base = if flags & CLONE_SETTLS != 0 && !frame.set_child_tls(tls) { Some(tls) } else { crate::arch::context::save_tls() };
    if thread {
        let tid = task::clone_task(true, None, &frame, &fx, |leader, current, child| {
            child.name = leader.name.clone();
            child.parent = 0;
            child.vt = leader.vt;
            child.uid = leader.uid;
            child.ruid = leader.ruid;
            child.cwd = leader.cwd.clone();
            child.abi = leader.abi;
            child.exe = leader.exe.clone();
            child.sig_mask = current.sig_mask;
            child.fs_base = fs_base.unwrap_or(current.fs_base);
            child.clear_child_tid = if flags & CLONE_CHILD_CLEARTID != 0 { child_tid } else { 0 };
            child.pgid = leader.pgid;
            child.termios = leader.termios;
            child.fds = Vec::new();
        });
        let Some(tid) = tid else {
            return EAGAIN;
        };
        if flags & CLONE_PARENT_SETTID != 0 {
            write_tid(parent_tid, tid);
        }
        if flags & CLONE_CHILD_SETTID != 0 {
            write_tid(child_tid, tid);
        }
        return tid as i64;
    }
    let Some(aspace) = task::with_current(|t| t.aspace.as_ref().and_then(|a| a.duplicate())) else {
        return ENOMEM;
    };
    if flags & CLONE_CHILD_SETTID != 0 || flags & CLONE_PARENT_SETTID != 0 {
        let next = task::peek_next_pid();
        if flags & CLONE_CHILD_SETTID != 0 {
            write_tid_in(&aspace, child_tid, next);
        }
        if flags & CLONE_PARENT_SETTID != 0 && flags & CLONE_VM == 0 {
            write_tid_in(&aspace, parent_tid, next);
        }
    }
    let vfork = flags & CLONE_VFORK != 0;
    let shm_ids: Vec<u32> = task::with_current(|t| t.shm.iter().map(|(id, _, _)| *id).collect());
    let pid = task::clone_task(false, Some(aspace), &frame, &fx, |leader, current, child| {
        child.name = leader.name.clone();
        child.parent = if flags & CLONE_PARENT != 0 { leader.parent } else { leader.pid };
        child.vt = leader.vt;
        child.uid = leader.uid;
        child.ruid = leader.ruid;
        child.root_ticket_until = leader.root_ticket_until;
        child.cwd = leader.cwd.clone();
        child.abi = leader.abi;
        child.exe = leader.exe.clone();
        child.env = leader.env.clone();
        child.args = leader.args.clone();
        child.umask = leader.umask;
        child.termios = leader.termios;
        child.fs_base = fs_base.unwrap_or(current.fs_base);
        child.sig_actions = leader.sig_actions;
        child.sig_mask = current.sig_mask;
        child.altstack = current.altstack;
        child.pgid = leader.pgid;
        child.brk_start = leader.brk_start;
        child.brk = leader.brk;
        child.mmap_next = leader.mmap_next;
        child.mmap_free = leader.mmap_free.clone();
        child.shm_next = leader.shm_next;
        child.shm = leader.shm.clone();
        child.cloexec = leader.cloexec;
        child.nonblock = leader.nonblock;
        child.fds = leader.fds.iter().map(|f| f.as_ref().map(|f| f.duplicate())).collect();
        child.clear_child_tid = if flags & CLONE_CHILD_CLEARTID != 0 { child_tid } else { 0 };
        child.vfork_parent = if vfork { current.pid } else { 0 };
        child.comm = leader.comm.clone();
    });
    let Some(pid) = pid else {
        return ENOMEM;
    };
    for id in shm_ids {
        task::ipc::retain(id);
    }
    if flags & CLONE_PARENT_SETTID != 0 {
        write_tid(parent_tid, pid);
    }
    if vfork {
        loop {
            let waiting = task::with_task(pid, |t| t.vfork_parent != 0 && t.state != task::State::Zombie).unwrap_or(false);
            if !waiting {
                break;
            }
            task::block(task::WAIT_CHILD, Some(task::TICK_HZ / 20), task::input_seq());
            task::check_killed();
        }
    }
    pid as i64
}

pub fn sys_fork() -> i64 {
    sys_clone(17, 0, 0, 0, 0)
}

pub fn sys_vfork() -> i64 {
    sys_clone(CLONE_VM | CLONE_VFORK | 17, 0, 0, 0, 0)
}

fn read_string_array(ptr: u64) -> Result<Vec<String>, i64> {
    let mut out = Vec::new();
    if ptr == 0 {
        return Ok(out);
    }
    let mut total = 0usize;
    for i in 0..4096u64 {
        let raw = user_slice(ptr + i * 8, 8)?;
        let item = u64::from_le_bytes((&*raw).try_into().unwrap());
        if item == 0 {
            return Ok(out);
        }
        let s = user_cstr(item)?;
        total += s.len() + 1;
        if total > 1 << 20 {
            return Err(-7);
        }
        out.push(s);
    }
    Err(-7)
}

pub fn sys_execve(path_ptr: u64, argv_ptr: u64, envp_ptr: u64) -> i64 {
    let raw_path = match user_cstr(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if raw_path.is_empty() {
        return ENOENT;
    }
    let path = if crate::syscall::linux_abi() { super::base::guest_path(&crate::syscall::absolute(&raw_path)) } else { crate::syscall::absolute(&raw_path) };
    let path = self_exe(&path).unwrap_or(path);
    let mut argv = match read_string_array(argv_ptr) {
        Ok(a) => a,
        Err(e) => return e,
    };
    if argv.is_empty() {
        argv.push(raw_path.clone());
    }
    let envp = match read_string_array(envp_ptr) {
        Ok(e) => e,
        Err(e) => return e,
    };
    exec_image(&path, argv, envp)
}

fn self_exe(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/proc/")?.strip_suffix("/exe")?;
    let pid = if rest == "self" || rest == "thread-self" {
        task::current_pid()
    } else {
        rest.parse::<Pid>().ok()?
    };
    task::with_task(pid, |t| t.exe.clone()).filter(|e| !e.is_empty())
}

fn exec_image(path: &str, argv: Vec<String>, envp: Vec<String>) -> i64 {
    let exists = {
        let guard = crate::fs::VFS.lock();
        guard.as_ref().map(|v| v.resolve(v.root_id(), path).is_some()).unwrap_or(false)
    };
    let (uid, ruid) = task::with_current(|t| (t.uid, t.ruid));
    let (image, name) = match elf::load_program(path, &argv, &envp, uid, ruid, 0) {
        Ok(r) => r,
        Err(reason) => {
            crate::debug_println!("execve: {}: {}", path, reason);
            return match reason {
                "no such file" if !exists => ENOENT,
                "permission denied" => -13,
                "is a directory" => -13,
                _ => ENOEXEC,
            };
        }
    };
    let pid = task::current_pid();
    task::stop_other_threads(pid);
    let doomed: Vec<OpenFile> = task::with_current(|t| {
        let mut closed = Vec::new();
        for fd in 0..t.fds.len().min(256) {
            if t.cloexec[fd / 64] & (1 << (fd % 64)) != 0 {
                if let Some(file) = t.fds[fd].take() {
                    closed.push(file);
                }
            }
        }
        t.cloexec = [0; 4];
        closed
    });
    for file in doomed {
        file.release();
    }
    task::ipc::unmap_range(pid, paging::USER_BASE, paging::USER_END - paging::USER_BASE);
    let crate::task::UserImage { aspace, entry, stack, brk, abi, exe, env, args } = image;
    if let Some(mut old) = task::replace_aspace(aspace) {
        old.destroy();
    }
    task::with_current(|t| {
        t.name = name.clone();
        t.abi = abi;
        t.exe = exe;
        t.env = env;
        t.args = args;
        t.brk_start = brk;
        t.brk = brk;
        t.mmap_next = paging::USER_MMAP_BASE;
        t.mmap_free.clear();
        t.shm_next = paging::USER_SHM_BASE;
        t.shm.clear();
        t.comm = None;
        t.timers.clear();
        for action in t.sig_actions.iter_mut() {
            if action.handler > 1 {
                *action = SigAction::default();
            }
        }
    });
    task::with_current_thread(|t| {
        t.fs_base = 0;
        t.clear_child_tid = 0;
        t.altstack = (0, 0, 2);
        t.sig_pending &= !(1 << 27);
    });
    task::set_fs_base(0);
    task::release_vfork();
    let (frame_ptr, fx_ptr) = task::with_current_thread(|t| (t.frame, t.frame_fx));
    if frame_ptr != 0 {
        let frame = unsafe { &mut *(frame_ptr as *mut InterruptFrame) };
        *frame = frame.fresh_user(entry, stack);
    }
    if fx_ptr != 0 {
        let fx = unsafe { core::slice::from_raw_parts_mut(fx_ptr as *mut u8, FX_SIZE) };
        crate::arch::context::reset_fx(fx);
    }
    crate::debug_println!("execve: pid {} -> {} ({})", pid, name, if abi == Abi::Linux { "linux" } else { "native" });
    0
}

fn wait_filter(pid: i64, me: Pid) -> impl Fn(&task::Task) -> bool {
    let group = task::with_task(me, |t| t.pgid).unwrap_or(me);
    move |t: &task::Task| {
        if pid > 0 {
            t.pid == pid as Pid
        } else if pid == 0 {
            t.pgid == group
        } else if pid < -1 {
            t.pgid == (-pid) as Pid
        } else {
            true
        }
    }
}

pub fn sys_wait4(pid: u64, status: u64, options: u64, rusage: u64) -> i64 {
    let pid = pid as i64;
    let me = task::current_pid();
    loop {
        match task::reap_matching(me, wait_filter(pid, me), options & WNOWAIT == 0) {
            ReapResult::Exited(child, code, signal) => {
                if status != 0 {
                    let word = super::signal::wait_status(code, signal);
                    if copy_out(status, 4, &word.to_le_bytes()) < 0 {
                        return EFAULT;
                    }
                }
                if rusage != 0 {
                    copy_out(rusage, 144, &[0u8; 144]);
                }
                return child as i64;
            }
            ReapResult::NoChild => return ECHILD,
            ReapResult::Running => {
                if options & WNOHANG != 0 {
                    return 0;
                }
                if super::signal::pending() {
                    return super::signal::EINTR;
                }
                task::block(task::WAIT_CHILD, Some(task::TICK_HZ / 10), task::input_seq());
                task::check_killed();
            }
        }
    }
}

pub fn sys_waitid(idtype: u64, id: u64, info: u64, options: u64) -> i64 {
    let pid: i64 = match idtype {
        0 => -1,
        1 => id as i64,
        2 => -(id as i64),
        _ => return EINVAL,
    };
    let me = task::current_pid();
    loop {
        match task::reap_matching(me, wait_filter(if pid == 0 { -1 } else { pid }, me), options & WNOWAIT == 0) {
            ReapResult::Exited(child, code, signal) => {
                if info != 0 {
                    let mut raw = [0u8; 128];
                    raw[0..4].copy_from_slice(&17u32.to_le_bytes());
                    raw[8..12].copy_from_slice(&(if signal != 0 { 2u32 } else { 1u32 }).to_le_bytes());
                    raw[16..20].copy_from_slice(&child.to_le_bytes());
                    let value = if signal != 0 { signal as i32 } else { code & 0xFF };
                    raw[24..28].copy_from_slice(&value.to_le_bytes());
                    if copy_out(info, 128, &raw) < 0 {
                        return EFAULT;
                    }
                }
                return 0;
            }
            ReapResult::NoChild => return ECHILD,
            ReapResult::Running => {
                if options & WNOHANG != 0 {
                    if info != 0 {
                        copy_out(info, 128, &[0u8; 128]);
                    }
                    return 0;
                }
                if super::signal::pending() {
                    return super::signal::EINTR;
                }
                task::block(task::WAIT_CHILD, Some(task::TICK_HZ / 10), task::input_seq());
                task::check_killed();
            }
        }
    }
}

pub fn sys_exit(code: u64) -> ! {
    let tid = task::current_tid();
    let pid = task::current_pid();
    if tid != pid {
        let clear = task::with_current_thread(|t| core::mem::replace(&mut t.clear_child_tid, 0));
        if clear != 0 && copy_out(clear, 4, &0u32.to_le_bytes()) >= 0 {
            task::wake_futex(pid, clear, 1);
        }
        task::bkl::release();
        task::exit_thread();
    }
    task::exit_current(code as i32 & 0xFF)
}

pub fn sys_exit_group(code: u64) -> ! {
    task::exit_current(code as i32 & 0xFF)
}

pub fn sys_set_tid_address(addr: u64) -> i64 {
    task::with_current_thread(|t| t.clear_child_tid = addr);
    task::current_tid() as i64
}
