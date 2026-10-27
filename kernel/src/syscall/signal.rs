use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::syscall::{copy_out, user_slice, EFAULT, EINVAL, EPERM, ESRCH};
use crate::task::switch::InterruptFrame;
use crate::task::{self, Abi, Pid, SigAction};

#[cfg_attr(target_arch = "x86_64", path = "sigframe/x86_64.rs")]
#[cfg_attr(target_arch = "aarch64", path = "sigframe/aarch64.rs")]
#[cfg_attr(target_arch = "riscv64", path = "sigframe/riscv64.rs")]
mod sigframe;

pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGKILL: u32 = 9;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;
pub const SIGTTIN: u32 = 21;
pub const SIGTTOU: u32 = 22;
pub const SIGURG: u32 = 23;
pub const SIGWINCH: u32 = 28;
const NSIG: u32 = 64;

const SIG_DFL: u64 = 0;
const SIG_IGN: u64 = 1;
pub(super) const SA_ONSTACK: u64 = 0x0800_0000;
const SA_RESTART: u64 = 0x1000_0000;
const SA_NODEFER: u64 = 0x4000_0000;
const SA_RESETHAND: u64 = 0x8000_0000;
pub(super) const SA_RESTORER: u64 = 0x0400_0000;

pub(super) const SS_ONSTACK: u32 = 1;
pub(super) const SS_DISABLE: u32 = 2;

pub const EINTR: i64 = -4;

const UNBLOCKABLE: u64 = (1 << (SIGKILL - 1)) | (1 << (SIGSTOP - 1));

static PENDING_HINT: AtomicU64 = AtomicU64::new(0);

fn bit(sig: u32) -> u64 {
    1u64 << (sig - 1)
}

fn default_ignored(sig: u32) -> bool {
    matches!(sig, SIGCHLD | SIGCONT | SIGURG | SIGWINCH | SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU)
}

fn terminate(pid: Pid, sig: u32) {
    task::with_task(pid, |t| t.exit_signal = sig as u8);
    task::kill(pid, 128 + sig as i32);
}

pub fn send(pid: Pid, sig: u32) -> bool {
    if sig > NSIG {
        return false;
    }
    let info = task::with_task(pid, |t| (t.leader, t.kernel_thread, t.state == task::State::Zombie));
    let Some((leader, kernel_thread, zombie)) = info else {
        return false;
    };
    if kernel_thread || zombie {
        return false;
    }
    if sig == 0 {
        return true;
    }
    let (abi, action) = task::with_task(leader, |t| (t.abi, t.sig_actions[sig as usize])).unwrap_or((Abi::Native, SigAction::default()));
    if sig == SIGKILL {
        terminate(leader, sig);
        return true;
    }
    if sig == SIGCONT {
        return true;
    }
    if abi == Abi::Native || action.handler == SIG_DFL {
        if default_ignored(sig) {
            return true;
        }
        if abi == Abi::Native {
            terminate(leader, sig);
            return true;
        }
    }
    if action.handler == SIG_IGN {
        return true;
    }
    let members = task::group_of(leader);
    let chosen = task::with_tasks(|tasks| {
        let open = members.iter().copied().find(|m| tasks.get(m).map(|t| t.sig_mask & bit(sig) == 0).unwrap_or(false));
        open.or(Some(leader)).filter(|m| tasks.contains_key(m))
    });
    let Some(target) = chosen else {
        return false;
    };
    queue(target, sig);
    true
}

pub fn send_thread(tid: Pid, sig: u32) -> bool {
    if sig > NSIG {
        return false;
    }
    let Some(leader) = task::with_task(tid, |t| t.leader) else {
        return false;
    };
    if sig == 0 {
        return true;
    }
    if leader == tid {
        let blocked = task::with_task(tid, |t| t.sig_mask & bit(sig) != 0).unwrap_or(false);
        if !blocked || sig == SIGKILL {
            return send(tid, sig);
        }
    }
    let action = task::with_task(leader, |t| t.sig_actions[sig as usize]).unwrap_or_default();
    if action.handler == SIG_IGN || (action.handler == SIG_DFL && default_ignored(sig)) {
        return true;
    }
    if action.handler == SIG_DFL {
        terminate(leader, sig);
        return true;
    }
    queue(tid, sig);
    true
}

fn queue(tid: Pid, sig: u32) {
    task::with_task(tid, |t| t.sig_pending |= bit(sig));
    PENDING_HINT.fetch_add(1, Ordering::Relaxed);
    task::interrupt(tid);
}

fn check_posix_timers(now: u64) {
    let fired: Vec<(Pid, u32, Pid)> = task::with_current(|t| {
        let pid = t.pid;
        let mut fired = Vec::new();
        for timer in t.timers.iter_mut() {
            if timer.next != 0 && now >= timer.next {
                if timer.interval != 0 {
                    let missed = (now - timer.next) / timer.interval;
                    timer.overrun = missed as u32;
                    timer.next += (missed + 1) * timer.interval;
                } else {
                    timer.next = 0;
                }
                if timer.signo != 0 {
                    fired.push((pid, timer.signo, timer.thread));
                }
            }
        }
        fired
    });
    for (pid, sig, thread) in fired {
        if thread != 0 {
            send_thread(thread, sig);
        } else {
            send(pid, sig);
        }
    }
}

fn timer_value(raw: &[u8]) -> Result<u64, i64> {
    let secs = get_u64(raw, 0);
    let nanos = get_u64(raw, 8);
    if nanos >= 1_000_000_000 {
        return Err(EINVAL);
    }
    let ms = secs.saturating_mul(1000).saturating_add(nanos.div_ceil(1_000_000));
    Ok(if secs == 0 && nanos == 0 { 0 } else { task::ms_to_ticks(ms.max(1)) })
}

pub fn sys_timer_create(clock: u64, sevp: u64, out: u64) -> i64 {
    if clock > 11 {
        return EINVAL;
    }
    let (signo, thread) = if sevp == 0 {
        (SIGALRM, 0)
    } else {
        let raw = match user_slice(sevp, 20) {
            Ok(b) => b.to_vec(),
            Err(e) => return e,
        };
        let signo = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        let notify = u32::from_le_bytes(raw[12..16].try_into().unwrap());
        let tid = u32::from_le_bytes(raw[16..20].try_into().unwrap());
        match notify {
            1 => (0, 0),
            0 => (signo, 0),
            4 => (signo, tid),
            _ => return EINVAL,
        }
    };
    if signo > NSIG {
        return EINVAL;
    }
    let id = task::with_current(|t| {
        let id = (0..).find(|i| !t.timers.iter().any(|x| x.id == *i)).unwrap();
        t.timers.push(task::PosixTimer { id, signo, thread, next: 0, interval: 0, overrun: 0 });
        id
    });
    if copy_out(out, 4, &id.to_le_bytes()) < 0 {
        task::with_current(|t| t.timers.retain(|x| x.id != id));
        return EFAULT;
    }
    0
}

fn timer_setting(timer: &task::PosixTimer) -> [u8; 32] {
    let mut raw = [0u8; 32];
    let to_ts = |ticks: u64| {
        let ns = ticks * 1_000_000_000 / task::TICK_HZ;
        (ns / 1_000_000_000, ns % 1_000_000_000)
    };
    let (is, ins) = to_ts(timer.interval);
    let left = if timer.next == 0 { 0 } else { timer.next.saturating_sub(task::ticks()).max(1) };
    let (vs, vns) = to_ts(left);
    put_u64(&mut raw, 0, is);
    put_u64(&mut raw, 8, ins);
    put_u64(&mut raw, 16, vs);
    put_u64(&mut raw, 24, vns);
    raw
}

pub fn sys_timer_settime(id: u64, flags: u64, new: u64, old: u64) -> i64 {
    let raw = match user_slice(new, 32) {
        Ok(b) => b.to_vec(),
        Err(e) => return e,
    };
    let (interval, value) = match (timer_value(&raw[0..16]), timer_value(&raw[16..32])) {
        (Ok(i), Ok(v)) => (i, v),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let now = task::ticks();
    let next = if value == 0 {
        0
    } else if flags & 1 != 0 {
        let secs = get_u64(&raw, 16);
        let nanos = get_u64(&raw, 24);
        let target_ms = secs.saturating_mul(1000).saturating_add(nanos / 1_000_000);
        let now_ms = crate::drivers::rtc::boot_epoch() * 1000 + task::uptime_ms();
        now + task::ms_to_ticks(target_ms.saturating_sub(now_ms).max(1))
    } else {
        now + value
    };
    let previous = task::with_current(|t| {
        let timer = t.timers.iter_mut().find(|x| x.id == id as u32)?;
        let previous = timer_setting(timer);
        timer.next = next;
        timer.interval = interval;
        Some(previous)
    });
    let Some(previous) = previous else {
        return EINVAL;
    };
    if old != 0 && copy_out(old, 32, &previous) < 0 {
        return EFAULT;
    }
    0
}

pub fn sys_timer_gettime(id: u64, out: u64) -> i64 {
    match task::with_current(|t| t.timers.iter().find(|x| x.id == id as u32).map(timer_setting)) {
        Some(raw) => copy_out(out, 32, &raw).min(0),
        None => EINVAL,
    }
}

pub fn sys_timer_getoverrun(id: u64) -> i64 {
    task::with_current(|t| t.timers.iter().find(|x| x.id == id as u32).map(|x| x.overrun as i64)).unwrap_or(EINVAL)
}

pub fn sys_timer_delete(id: u64) -> i64 {
    let removed = task::with_current(|t| {
        let before = t.timers.len();
        t.timers.retain(|x| x.id != id as u32);
        before != t.timers.len()
    });
    if removed { 0 } else { EINVAL }
}

fn check_alarm() {
    let now = task::ticks();
    check_posix_timers(now);
    let fire = task::with_current(|t| {
        if t.alarm_at != 0 && now >= t.alarm_at {
            t.alarm_at = if t.alarm_interval != 0 { now + t.alarm_interval } else { 0 };
            Some(t.pid)
        } else {
            None
        }
    });
    if let Some(pid) = fire {
        send(pid, SIGALRM);
    }
}

pub fn pending() -> bool {
    if task::current_tid() == 0 {
        return false;
    }
    check_alarm();
    task::with_current_thread(|t| t.sig_pending & !(t.sig_mask & !UNBLOCKABLE) != 0)
}

pub fn with_wait_mask(set: u64, sigsetsize: u64, wait: impl FnOnce() -> i64) -> i64 {
    if set == 0 || task::current_tid() == 0 {
        return wait();
    }
    if sigsetsize != 8 {
        return EINVAL;
    }
    let mask = match user_slice(set, 8) {
        Ok(b) => get_u64(b, 0),
        Err(e) => return e,
    };
    let old = task::with_current_thread(|t| core::mem::replace(&mut t.sig_mask, mask & !UNBLOCKABLE));
    let result = wait();
    if pending() {
        task::with_current_thread(|t| t.saved_mask = Some(old));
    } else {
        task::with_current_thread(|t| t.sig_mask = old);
    }
    result
}

pub fn maybe_pending() -> bool {
    PENDING_HINT.load(Ordering::Relaxed) != 0 || task::with_current(|t| t.alarm_at != 0 || t.timers.iter().any(|x| x.next != 0))
}

fn restartable(nr: u64) -> bool {
    matches!(nr, 0 | 1 | 16 | 17 | 18 | 19 | 20 | 43 | 44 | 45 | 46 | 47 | 42 | 61 | 202 | 288 | 2 | 257)
}

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u64(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap())
}

pub fn deliver(frame: &mut InterruptFrame, fx: *mut u8, syscall: Option<(u64, i64)>) {
    if !frame.from_user() {
        return;
    }
    check_alarm();
    loop {
        let next = task::with_current_thread(|t| {
            let ready = t.sig_pending & !(t.sig_mask & !UNBLOCKABLE);
            if ready == 0 {
                return None;
            }
            let sig = ready.trailing_zeros() + 1;
            t.sig_pending &= !bit(sig);
            Some((sig, t.saved_mask.take().unwrap_or(t.sig_mask), t.altstack))
        });
        let Some((sig, mask, altstack)) = next else {
            if let Some((_, result)) = syscall {
                frame.set_ret(result as u64);
            }
            return;
        };
        let _ = PENDING_HINT.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some(v.saturating_sub(1)));
        let action = task::with_current(|t| {
            let action = t.sig_actions[sig as usize];
            if action.flags & SA_RESETHAND != 0 && action.handler > SIG_IGN {
                t.sig_actions[sig as usize] = SigAction::default();
            }
            action
        });
        if action.handler == SIG_IGN {
            continue;
        }
        if action.handler == SIG_DFL {
            if default_ignored(sig) {
                continue;
            }
            let pid = task::current_pid();
            task::with_task(pid, |t| t.exit_signal = sig as u8);
            task::exit_current(128 + sig as i32);
        }
        if let Some((nr, result)) = syscall {
            if result == EINTR && action.flags & SA_RESTART != 0 && restartable(nr) {
                frame.restart(nr);
            } else {
                frame.set_ret(result as u64);
            }
        }
        if !sigframe::setup(frame, fx, sig, &action, mask, altstack) {
            let pid = task::current_pid();
            task::with_task(pid, |t| t.exit_signal = 11);
            task::exit_current(128 + 11);
        }
        let mut new_mask = mask | action.mask;
        if action.flags & SA_NODEFER == 0 {
            new_mask |= bit(sig);
        }
        task::with_current_thread(|t| t.sig_mask = new_mask & !UNBLOCKABLE);
        return;
    }
}

pub fn sys_rt_sigreturn(frame: &mut InterruptFrame, fx: *mut u8) {
    match sigframe::restore(frame, fx) {
        Some(mask) => task::with_current_thread(|t| t.sig_mask = mask & !UNBLOCKABLE),
        None => task::exit_current(128 + 11),
    }
}

pub fn sys_rt_sigaction(sig: u64, act: u64, oldact: u64, sigsetsize: u64) -> i64 {
    if sigsetsize != 8 {
        return EINVAL;
    }
    if sig == 0 || sig > NSIG as u64 || ((sig as u32 == SIGKILL || sig as u32 == SIGSTOP) && act != 0) {
        return EINVAL;
    }
    let new = if act != 0 {
        match user_slice(act, 32) {
            Ok(b) => Some(SigAction { handler: get_u64(b, 0), flags: get_u64(b, 8), restorer: get_u64(b, 16), mask: get_u64(b, 24) }),
            Err(e) => return e,
        }
    } else {
        None
    };
    let old = task::with_current(|t| {
        let old = t.sig_actions[sig as usize];
        if let Some(action) = new {
            t.sig_actions[sig as usize] = action;
        }
        old
    });
    if oldact != 0 {
        let mut raw = [0u8; 32];
        put_u64(&mut raw, 0, old.handler);
        put_u64(&mut raw, 8, old.flags);
        put_u64(&mut raw, 16, old.restorer);
        put_u64(&mut raw, 24, old.mask);
        if copy_out(oldact, 32, &raw) < 0 {
            return EFAULT;
        }
    }
    if let Some(action) = new {
        if action.handler == SIG_IGN || (action.handler == SIG_DFL && default_ignored(sig as u32)) {
            let members = task::group_of(task::current_pid());
            for m in members {
                task::with_task(m, |t| t.sig_pending &= !bit(sig as u32));
            }
        }
    }
    0
}

pub fn sys_rt_sigprocmask(how: u64, set: u64, oldset: u64, sigsetsize: u64) -> i64 {
    if sigsetsize != 8 {
        return EINVAL;
    }
    let new = if set != 0 {
        match user_slice(set, 8) {
            Ok(b) => Some(get_u64(b, 0)),
            Err(e) => return e,
        }
    } else {
        None
    };
    let old = task::with_current_thread(|t| t.sig_mask);
    if let Some(value) = new {
        let mask = match how {
            0 => old | value,
            1 => old & !value,
            2 => value,
            _ => return EINVAL,
        };
        task::with_current_thread(|t| t.sig_mask = mask & !UNBLOCKABLE);
    }
    if oldset != 0 && copy_out(oldset, 8, &old.to_le_bytes()) < 0 {
        return EFAULT;
    }
    0
}

pub fn sys_rt_sigpending(set: u64, sigsetsize: u64) -> i64 {
    if sigsetsize != 8 {
        return EINVAL;
    }
    let pending = task::with_current_thread(|t| t.sig_pending & t.sig_mask);
    copy_out(set, 8, &pending.to_le_bytes()).min(0)
}

pub fn sys_sigaltstack(ss: u64, old: u64) -> i64 {
    let current = task::with_current_thread(|t| t.altstack);
    if old != 0 {
        let mut raw = [0u8; 24];
        put_u64(&mut raw, 0, current.0);
        raw[8..12].copy_from_slice(&current.2.to_le_bytes());
        put_u64(&mut raw, 16, current.1);
        if copy_out(old, 24, &raw) < 0 {
            return EFAULT;
        }
    }
    if ss != 0 {
        let raw = match user_slice(ss, 24) {
            Ok(b) => b.to_vec(),
            Err(e) => return e,
        };
        let sp = get_u64(&raw, 0);
        let flags = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        let size = get_u64(&raw, 16);
        if flags & !(SS_DISABLE | 0x8000_0000) != 0 {
            return EINVAL;
        }
        if flags & SS_DISABLE == 0 && size < 2048 {
            return -12;
        }
        task::with_current_thread(|t| t.altstack = if flags & SS_DISABLE != 0 { (0, 0, SS_DISABLE) } else { (sp, size, 0) });
    }
    0
}

pub fn sys_kill(pid: i64, sig: u64) -> i64 {
    if sig > NSIG as u64 {
        return EINVAL;
    }
    let me = task::current_pid();
    let targets: Vec<Pid> = if pid > 0 {
        alloc::vec![pid as Pid]
    } else if pid == 0 {
        let group = task::with_current(|t| t.pgid);
        task::with_tasks(|tasks| tasks.values().filter(|t| t.leader == t.pid && t.pgid == group && !t.kernel_thread && t.state != task::State::Zombie).map(|t| t.pid).collect())
    } else if pid == -1 {
        let uid = task::with_current(|t| t.uid);
        task::with_tasks(|tasks| tasks.values().filter(|t| t.leader == t.pid && t.pid != me && !t.kernel_thread && (uid == 0 || t.uid == uid) && t.state != task::State::Zombie).map(|t| t.pid).collect())
    } else {
        let group = (-pid) as Pid;
        task::with_tasks(|tasks| tasks.values().filter(|t| t.leader == t.pid && t.pgid == group && !t.kernel_thread && t.state != task::State::Zombie).map(|t| t.pid).collect())
    };
    if targets.is_empty() {
        return ESRCH;
    }
    let (uid, ruid) = task::with_current(|t| (t.uid, t.ruid));
    let mut delivered = false;
    let mut denied = false;
    for target in targets {
        if uid != 0 {
            match task::with_task(target, |t| (t.uid, t.ruid)) {
                Some((u, r)) if u == uid || r == ruid || u == ruid => {}
                Some(_) => {
                    denied = true;
                    continue;
                }
                None => continue,
            }
        }
        delivered |= send(target, sig as u32);
    }
    if delivered {
        0
    } else if denied {
        EPERM
    } else {
        ESRCH
    }
}

pub fn sys_tgkill(tgid: i64, tid: u64, sig: u64) -> i64 {
    if sig > NSIG as u64 {
        return EINVAL;
    }
    let leader = task::with_task(tid as Pid, |t| t.leader);
    match leader {
        Some(l) if tgid <= 0 || l == tgid as Pid => {
            if send_thread(tid as Pid, sig as u32) {
                0
            } else {
                ESRCH
            }
        }
        _ => ESRCH,
    }
}

pub fn sys_rt_sigsuspend(set: u64, sigsetsize: u64) -> i64 {
    if sigsetsize != 8 {
        return EINVAL;
    }
    let mask = match user_slice(set, 8) {
        Ok(b) => get_u64(b, 0),
        Err(e) => return e,
    };
    let old = task::with_current_thread(|t| core::mem::replace(&mut t.sig_mask, mask & !UNBLOCKABLE));
    while !pending() {
        task::block(task::WAIT_SIGNAL, Some(task::TICK_HZ / 10), task::input_seq());
        task::check_killed();
    }
    task::with_current_thread(|t| t.saved_mask = Some(old));
    EINTR
}

pub fn sys_pause() -> i64 {
    while !pending() {
        task::block(task::WAIT_SIGNAL, Some(task::TICK_HZ / 10), task::input_seq());
        task::check_killed();
    }
    EINTR
}

pub fn sys_rt_sigtimedwait(set: u64, info: u64, timeout: u64, sigsetsize: u64) -> i64 {
    if sigsetsize != 8 {
        return EINVAL;
    }
    let wanted = match user_slice(set, 8) {
        Ok(b) => get_u64(b, 0),
        Err(e) => return e,
    };
    let deadline = if timeout != 0 {
        match user_slice(timeout, 16) {
            Ok(b) => {
                let secs = get_u64(b, 0);
                let nanos = get_u64(b, 8);
                Some(task::ticks() + task::ms_to_ticks(secs.saturating_mul(1000).saturating_add(nanos / 1_000_000)))
            }
            Err(e) => return e,
        }
    } else {
        None
    };
    loop {
        check_alarm();
        let got = task::with_current_thread(|t| {
            let ready = t.sig_pending & wanted;
            if ready == 0 {
                return None;
            }
            let sig = ready.trailing_zeros() + 1;
            t.sig_pending &= !bit(sig);
            Some(sig)
        });
        if let Some(sig) = got {
            if info != 0 {
                let mut raw = [0u8; 128];
                raw[0..4].copy_from_slice(&sig.to_le_bytes());
                copy_out(info, 128, &raw);
            }
            return sig as i64;
        }
        if pending() {
            return EINTR;
        }
        if let Some(d) = deadline {
            if task::ticks() >= d {
                return -11;
            }
        }
        task::block(task::WAIT_SIGNAL, Some(task::TICK_HZ / 20), task::input_seq());
        task::check_killed();
    }
}

fn timeval_ticks(sec: u64, usec: u64) -> u64 {
    if sec == 0 && usec == 0 {
        0
    } else {
        task::ms_to_ticks(sec.saturating_mul(1000).saturating_add(usec.div_ceil(1000)))
    }
}

fn remaining(at: u64) -> (u64, u64) {
    if at == 0 {
        return (0, 0);
    }
    let left = at.saturating_sub(task::ticks()).max(1);
    let us = left * 1_000_000 / task::TICK_HZ;
    (us / 1_000_000, us % 1_000_000)
}

pub fn sys_alarm(seconds: u64) -> i64 {
    let at = if seconds == 0 { 0 } else { task::ticks() + seconds * task::TICK_HZ };
    let old = task::with_current(|t| {
        let old = t.alarm_at;
        t.alarm_at = at;
        t.alarm_interval = 0;
        old
    });
    let (secs, usecs) = remaining(old);
    (secs + if usecs >= 500_000 || (secs == 0 && usecs > 0) { 1 } else { 0 }) as i64
}

pub fn sys_getitimer(which: u64, value: u64) -> i64 {
    if which != 0 {
        let raw = [0u8; 32];
        return copy_out(value, 32, &raw).min(0);
    }
    let (at, interval) = task::with_current(|t| (t.alarm_at, t.alarm_interval));
    let (s, us) = remaining(at);
    let iv_us = interval * 1_000_000 / task::TICK_HZ;
    let mut raw = [0u8; 32];
    put_u64(&mut raw, 0, iv_us / 1_000_000);
    put_u64(&mut raw, 8, iv_us % 1_000_000);
    put_u64(&mut raw, 16, s);
    put_u64(&mut raw, 24, us);
    copy_out(value, 32, &raw).min(0)
}

pub fn sys_setitimer(which: u64, new: u64, old: u64) -> i64 {
    if old != 0 {
        let r = sys_getitimer(which, old);
        if r < 0 {
            return r;
        }
    }
    if which != 0 {
        return 0;
    }
    if new == 0 {
        return 0;
    }
    let raw = match user_slice(new, 32) {
        Ok(b) => b.to_vec(),
        Err(e) => return e,
    };
    let interval = timeval_ticks(get_u64(&raw, 0), get_u64(&raw, 8));
    let first = timeval_ticks(get_u64(&raw, 16), get_u64(&raw, 24));
    task::with_current(|t| {
        t.alarm_at = if first == 0 { 0 } else { task::ticks() + first };
        t.alarm_interval = interval;
    });
    0
}

pub fn wait_status(code: i32, signal: u8) -> u32 {
    if signal != 0 {
        signal as u32 & 0x7F
    } else {
        ((code as u32) & 0xFF) << 8
    }
}
