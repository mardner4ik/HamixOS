use super::*;

pub(in crate::syscall) const SYS_HAMIX_SPAWN: u64 = 9010;
pub(in crate::syscall) const SYS_HAMIX_WAITPID: u64 = 9012;
pub(in crate::syscall) const SYS_HAMIX_SLEEP: u64 = 9013;
pub(in crate::syscall) const SYS_HAMIX_KILL: u64 = 9015;
pub(in crate::syscall) const SYS_HAMIX_PROC_LIST: u64 = 9016;
pub(in crate::syscall) const SYS_HAMIX_SYSINFO: u64 = 9017;
pub(in crate::syscall) const SYS_HAMIX_PROC_ALIVE: u64 = 9018;
pub(in crate::syscall) const SYS_HAMIX_FUTEX_WAIT: u64 = 9025;
pub(in crate::syscall) const SYS_HAMIX_FUTEX_WAKE: u64 = 9026;
pub(in crate::syscall) const SYS_HAMIX_WAIT_ANY: u64 = 9027;
pub(in crate::syscall) const SYS_HAMIX_ABI_INFO: u64 = 9028;
pub(in crate::syscall) const SYS_HAMIX_REALTIME: u64 = 9060;
pub(in crate::syscall) const SYS_HAMIX_LINUX_CONTROL: u64 = 9111;
pub(in crate::syscall) const SYS_HAMIX_CPU_STATS: u64 = 9120;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_SPAWN => sys_spawn(a1, a2, a3, a4, a5, a6),
        SYS_HAMIX_WAITPID => sys_waitpid(a1, a2),
        SYS_HAMIX_SLEEP => match sleep_interruptible(a1) {
            Ok(()) => 0,
            Err(_) => EINTR,
        },
        SYS_HAMIX_KILL => sys_kill(a1),
        SYS_HAMIX_PROC_LIST => sys_proc_list(a1, a2),
        SYS_HAMIX_SYSINFO => sys_sysinfo(a1, a2),
        SYS_HAMIX_PROC_ALIVE => task::exists(a1 as Pid) as i64,
        SYS_HAMIX_FUTEX_WAIT => crate::syscall::base::futex_wait(a1, a2 as u32, a3),
        SYS_HAMIX_FUTEX_WAKE => crate::syscall::base::futex_wake(a1, a2),
        SYS_HAMIX_WAIT_ANY => super::wait_any::sys_wait_any(a1, a2, a3),
        SYS_HAMIX_ABI_INFO => {
            let abi = task::with_current(|t| t.abi);
            if a1 != 0 {
                let r = copy_out(a1, a2, crate::linux::info_text(abi).as_bytes());
                if r < 0 {
                    return Some(r);
                }
            }
            crate::linux::HAMIX_ABI_VERSION as i64
        }
        SYS_HAMIX_LINUX_CONTROL => match a1 {
            0 => crate::linux::linux_enabled() as i64,
            1 | 2 => {
                if let Err(e) = require_root() {
                    return Some(e);
                }
                if a1 == 1 {
                    crate::linux::set_linux(true) as i64
                } else {
                    crate::linux::set_linux(false) as i64
                }
            }
            _ => EINVAL,
        },
        SYS_HAMIX_REALTIME => crate::drivers::rtc::now() as i64,
        SYS_HAMIX_CPU_STATS => copy_out(a1, a2, cpu_stats_text().as_bytes()),
        _ => return None,
    })
}

pub(in crate::syscall) fn spawn_env(flags: u64, header: u64) -> Result<Option<Vec<String>>, i64> {
    if flags & SPAWN_ENV == 0 {
        return Ok(None);
    }
    let raw = user_slice(header, 16)?;
    let ptr = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let len = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let env: Vec<String> = user_args(ptr, len)?.into_iter().filter(|e| e.find('=').map(|i| i > 0).unwrap_or(false)).collect();
    if env.len() > MAX_ENV_ENTRIES {
        return Err(EINVAL);
    }
    Ok(Some(env))
}

pub(in crate::syscall) fn sys_spawn(path_ptr: u64, path_len: u64, args_ptr: u64, packed: u64, stdio: u64, env_header: u64) -> i64 {
    let args_len = packed & 0xFFFF_FFFF;
    let flags = packed >> 32;
    let path = match user_string(path_ptr, path_len) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let args = match user_args(args_ptr, args_len) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let env = match spawn_env(flags, env_header) {
        Ok(env) => env,
        Err(e) => return e,
    };
    spawn_from_user(&path, &args, env, flags, stdio)
}

pub(in crate::syscall) fn resolve_program(cwd: &str, name: &str) -> Option<String> {
    let guard = fs::VFS.lock();
    let vfs = guard.as_ref()?;
    let root = vfs.root_id();
    let candidates: Vec<String> = if name.contains('/') {
        alloc::vec![fs::absolute(cwd, name)]
    } else {
        alloc::vec![format!("/usr/bin/{}", name), format!("/bin/{}", name), format!("/sbin/{}", name)]
    };
    candidates.into_iter().find(|p| vfs.resolve(root, p).map(|id| !vfs.is_dir(id)).unwrap_or(false))
}

pub(in crate::syscall) fn has_root_ticket() -> bool {
    task::with_current(|t| t.uid == 0)
}

pub(in crate::syscall) fn spawn_from_user(program: &str, args: &[String], env: Option<Vec<String>>, flags: u64, stdio: u64) -> i64 {
    let (pid, vt, uid, cwd) = current_identity();
    let Some(path) = resolve_program(&cwd, program) else {
        return ENOENT;
    };
    let (child_uid, child_ruid) = if flags & SPAWN_ROOT != 0 {
        if !has_root_ticket() {
            return EPERM;
        }
        (0, 0)
    } else {
        (uid, task::with_current(|t| t.ruid))
    };
    let env = match env {
        Some(env) => env,
        None => {
            let inherited = task::with_current(|t| t.env.clone());
            if inherited.is_empty() { task::elf::default_env(child_uid) } else { inherited }
        }
    };
    let child = match task::elf::spawn(&path, args, &env, pid, vt, child_uid, child_ruid, &cwd) {
        Ok(child) => child,
        Err("out of memory") => return ENOMEM,
        Err("permission denied") => return EACCES,
        Err("no such file") | Err("no filesystem mounted") | Err("interpreter not found") => return ENOENT,
        Err("is a directory") => return EISDIR,
        Err(e) => {
            crate::debug_println!("spawn: {}: {}", path, e);
            return ENOEXEC;
        }
    };
    for slot in 0..4usize {
        let wanted = (stdio >> (slot * 16)) & 0xFFFF;
        if wanted == 0 {
            continue;
        }
        let fd = (wanted - 1) as usize;
        let file = task::with_current(|t| t.fds.get(fd).and_then(|f| f.as_ref()).map(|f| f.duplicate()));
        let old = task::with_task(child, |t| core::mem::replace(&mut t.fds[slot], file)).flatten();
        if let Some(old) = old {
            old.release();
        }
    }
    if flags & SPAWN_DETACH != 0 {
        task::detach(child);
    }
    if flags & SPAWN_FOREGROUND != 0 {
        let group = task::with_current(|t| t.pgid);
        if group != 0 {
            task::with_task(child, |t| t.pgid = group);
        }
        match target(0) {
            Target::Console => {
                if crate::tty::vt::input_owner(vt) == pid || crate::tty::vt::leads_foreground_group(pid, vt) {
                    crate::tty::vt::set_input_owner(vt, child);
                }
            }
            Target::PipeRead(id) | Target::Pty(id, _) => pipe::set_foreground(id, child),
            _ => {}
        }
    }
    child as i64
}

pub(in crate::syscall) fn sys_waitpid(child: u64, flags: u64) -> i64 {
    let parent = task::current_pid();
    loop {
        match task::try_reap(parent, child as Pid) {
            task::WaitResult::Exited(code) => return (code as u32) as i64,
            task::WaitResult::NoChild => return ECHILD,
            task::WaitResult::Running => {
                if flags & 1 != 0 {
                    return EAGAIN;
                }
                task::block(task::WAIT_CHILD, Some(task::TICK_HZ / 5), task::input_seq());
                task::check_killed();
            }
        }
    }
}

pub(in crate::syscall) fn sys_kill(target_pid: u64) -> i64 {
    let (uid, ruid) = task::with_current(|t| (t.uid, t.ruid));
    if uid != 0 {
        let target_uid = task::with_task(target_pid as Pid, |t| (t.uid, t.ruid));
        match target_uid {
            Some((u, r)) if u == uid || r == ruid => {}
            Some(_) => return EACCES,
            None => return ESRCH,
        }
    }
    if task::kill(target_pid as Pid, 143) { 0 } else { ESRCH }
}

pub(in crate::syscall) fn sys_proc_list(buf: u64, len: u64) -> i64 {
    let mut text = String::new();
    let owners: Vec<(Pid, u32)> = task::with_tasks(|tasks| tasks.values().map(|t| (t.pid, t.uid)).collect());
    for info in task::list() {
        let state = match info.state {
            task::State::Runnable => "R",
            task::State::Blocked => "S",
            task::State::Zombie => "Z",
        };
        let uid = owners.iter().find(|(p, _)| *p == info.pid).map(|(_, u)| *u).unwrap_or(0);
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            info.pid,
            info.parent,
            state,
            info.vt + 1,
            if info.kernel_thread { format!("[{}]", info.name) } else { info.name.clone() },
            info.memory / 1024,
            info.cpu_ticks * 1000 / task::TICK_HZ,
            uid,
            info.cpu.map(|c| c as i64).unwrap_or(-1),
            if info.kernel_thread { "kernel" } else { info.abi.name() }
        ));
    }
    copy_out(buf, len, text.as_bytes())
}

pub(in crate::syscall) fn sys_sysinfo(buf: u64, len: u64) -> i64 {
    let extended = len == 64;
    let out = match user_slice_mut(buf, if extended { 64 } else { 56 }) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (free, total) = crate::memory::frame::memory_info();
    let (heap_free, heap_total) = crate::memory::heap_stats();
    let values: [u64; 7] = [
        total as u64,
        free as u64,
        heap_total as u64,
        heap_free as u64,
        task::uptime_ms(),
        task::list().len() as u64,
        crate::drivers::rtc::boot_epoch() + task::uptime_ms() / 1000,
    ];
    for (i, v) in values.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
    }
    if extended {
        out[56..64].copy_from_slice(&(fs::file_cache_bytes() as u64).to_le_bytes());
    }
    0
}
