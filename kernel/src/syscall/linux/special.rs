use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::syscall::{copy_out, install_fd, target, user_slice, Target, EAGAIN, EBADF, EINVAL};
use crate::arch::without_interrupts;
use crate::task::{self, OpenFile};

const CLOEXEC: u64 = 0o2000000;
const NONBLOCK: u64 = 0o4000;
const EFD_SEMAPHORE: u64 = 1;
const TFD_TIMER_ABSTIME: u64 = 1;

#[derive(Clone, Copy)]
pub enum Kind {
    Timer { realtime: bool, next: u64, interval: u64, expirations: u64 },
    Event { counter: u64, semaphore: bool },
    Signal { mask: u64 },
    Inotify { next_wd: u32 },
}

struct Object {
    kind: Kind,
    refs: u32,
}

static OBJECTS: Mutex<BTreeMap<u32, Object>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

fn with<R>(f: impl FnOnce(&mut BTreeMap<u32, Object>) -> R) -> R {
    without_interrupts(|| f(&mut OBJECTS.lock()))
}

pub fn retain(id: u32) {
    with(|t| {
        if let Some(o) = t.get_mut(&id) {
            o.refs += 1;
        }
    });
}

pub fn release(id: u32) {
    with(|t| {
        let dead = match t.get_mut(&id) {
            Some(o) => {
                o.refs = o.refs.saturating_sub(1);
                o.refs == 0
            }
            None => false,
        };
        if dead {
            t.remove(&id);
        }
    });
}

pub fn describe(id: u32) -> &'static str {
    match with(|t| t.get(&id).map(|o| o.kind)) {
        Some(Kind::Timer { .. }) => "anon_inode:[timerfd]",
        Some(Kind::Event { .. }) => "anon_inode:[eventfd]",
        Some(Kind::Signal { .. }) => "anon_inode:[signalfd]",
        Some(Kind::Inotify { .. }) => "anon_inode:inotify",
        None => "anon_inode:[unknown]",
    }
}

fn create(kind: Kind, flags: u64) -> i64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    with(|t| t.insert(id, Object { kind, refs: 1 }));
    let fd = install_fd(OpenFile::Special(id));
    if fd >= 0 {
        crate::syscall::set_cloexec(fd as u64, flags & CLOEXEC != 0);
        crate::syscall::set_fd_nonblock(fd as u64, flags & NONBLOCK != 0);
    }
    fd
}

fn update_timer(kind: &mut Kind) {
    if let Kind::Timer { next, interval, expirations, .. } = kind {
        let now = task::ticks();
        if *next != 0 && now >= *next {
            if *interval != 0 {
                let missed = (now - *next) / *interval + 1;
                *expirations += missed;
                *next += missed * *interval;
            } else {
                *expirations += 1;
                *next = 0;
            }
        }
    }
}

pub fn next_deadline() -> Option<u64> {
    with(|t| t.values().filter_map(|o| if let Kind::Timer { next, .. } = o.kind { if next != 0 { Some(next) } else { None } } else { None }).min())
}

fn signal_bits() -> u64 {
    task::with_current_thread(|t| t.sig_pending)
}

pub fn readable(id: u32) -> bool {
    let pending = signal_bits();
    with(|t| {
        let Some(o) = t.get_mut(&id) else {
            return false;
        };
        update_timer(&mut o.kind);
        match o.kind {
            Kind::Timer { expirations, .. } => expirations > 0,
            Kind::Event { counter, .. } => counter > 0,
            Kind::Signal { mask } => pending & mask != 0,
            Kind::Inotify { .. } => false,
        }
    })
}

pub fn writable(id: u32) -> bool {
    with(|t| matches!(t.get(&id).map(|o| o.kind), Some(Kind::Event { counter, .. }) if counter < u64::MAX - 1))
}

fn wait(fd: u64, id: u32) -> Result<(), i64> {
    loop {
        if readable(id) {
            return Ok(());
        }
        if crate::syscall::fd_nonblock(fd) {
            return Err(EAGAIN);
        }
        if super::signal::pending() {
            return Err(super::signal::EINTR);
        }
        let now = task::ticks();
        let slice = next_deadline().map(|d| d.saturating_sub(now)).unwrap_or(task::TICK_HZ / 10).clamp(1, task::TICK_HZ / 10);
        super::tty::sleep_for_events(slice);
    }
}

pub fn read(fd: u64, id: u32, buf: u64, count: u64) -> i64 {
    let kind = with(|t| t.get(&id).map(|o| o.kind));
    match kind {
        Some(Kind::Timer { .. }) | Some(Kind::Event { .. }) => {
            if count < 8 {
                return EINVAL;
            }
            if let Err(e) = wait(fd, id) {
                return e;
            }
            let value = with(|t| {
                let o = t.get_mut(&id)?;
                update_timer(&mut o.kind);
                match &mut o.kind {
                    Kind::Timer { expirations, .. } => Some(core::mem::replace(expirations, 0)),
                    Kind::Event { counter, semaphore } => {
                        if *semaphore {
                            *counter -= 1;
                            Some(1)
                        } else {
                            Some(core::mem::replace(counter, 0))
                        }
                    }
                    _ => None,
                }
            });
            match value {
                Some(v) if v > 0 => {
                    crate::task::wake_all(task::WAIT_PIPE);
                    copy_out(buf, 8, &v.to_le_bytes()).min(8).max(-14)
                }
                _ => EAGAIN,
            }
        }
        Some(Kind::Signal { mask }) => {
            if count < 128 {
                return EINVAL;
            }
            if let Err(e) = wait(fd, id) {
                return e;
            }
            let mut done = 0u64;
            while done + 128 <= count {
                let sig = task::with_current_thread(|t| {
                    let ready = t.sig_pending & mask;
                    if ready == 0 {
                        return None;
                    }
                    let sig = ready.trailing_zeros() + 1;
                    t.sig_pending &= !(1u64 << (sig - 1));
                    Some(sig)
                });
                let Some(sig) = sig else {
                    break;
                };
                let mut raw = [0u8; 128];
                raw[0..4].copy_from_slice(&sig.to_le_bytes());
                if copy_out(buf + done, 128, &raw) < 0 {
                    return -14;
                }
                done += 128;
            }
            if done == 0 { EAGAIN } else { done as i64 }
        }
        Some(Kind::Inotify { .. }) => match wait(fd, id) {
            Ok(()) => EAGAIN,
            Err(e) => e,
        },
        None => EBADF,
    }
}

pub fn write(fd: u64, id: u32, data: &[u8]) -> i64 {
    if data.len() < 8 {
        return EINVAL;
    }
    let value = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if value == u64::MAX {
        return EINVAL;
    }
    loop {
        let result = with(|t| match t.get_mut(&id).map(|o| &mut o.kind) {
            Some(Kind::Event { counter, .. }) => {
                if u64::MAX - 1 - *counter >= value {
                    *counter += value;
                    Some(true)
                } else {
                    Some(false)
                }
            }
            _ => None,
        });
        match result {
            Some(true) => {
                crate::task::wake_all(task::WAIT_PIPE);
                task::notify_input();
                return 8;
            }
            Some(false) => {
                if crate::syscall::fd_nonblock(fd) {
                    return EAGAIN;
                }
                super::tty::sleep_for_events(task::TICK_HZ / 20);
            }
            None => return EINVAL,
        }
    }
}

pub fn sys_inotify_init1(flags: u64) -> i64 {
    if flags & !(CLOEXEC | NONBLOCK) != 0 {
        return EINVAL;
    }
    create(Kind::Inotify { next_wd: 1 }, flags)
}

pub fn sys_inotify_add_watch(fd: u64, _path: u64, _mask: u64) -> i64 {
    let id = match target(fd) {
        Target::Special(id) => id,
        Target::Bad => return EBADF,
        _ => return EINVAL,
    };
    with(|t| match t.get_mut(&id).map(|o| &mut o.kind) {
        Some(Kind::Inotify { next_wd }) => {
            let wd = *next_wd as i64;
            *next_wd = next_wd.wrapping_add(1).max(1);
            wd
        }
        _ => EINVAL,
    })
}

pub fn sys_inotify_rm_watch(fd: u64, _wd: u64) -> i64 {
    match target(fd) {
        Target::Special(id) if matches!(with(|t| t.get(&id).map(|o| o.kind)), Some(Kind::Inotify { .. })) => 0,
        Target::Bad => EBADF,
        _ => EINVAL,
    }
}

pub fn sys_eventfd2(initval: u64, flags: u64) -> i64 {
    if flags & !(CLOEXEC | NONBLOCK | EFD_SEMAPHORE) != 0 {
        return EINVAL;
    }
    create(Kind::Event { counter: initval as u32 as u64, semaphore: flags & EFD_SEMAPHORE != 0 }, flags)
}

pub fn sys_timerfd_create(clock: u64, flags: u64) -> i64 {
    if flags & !(CLOEXEC | NONBLOCK) != 0 || !matches!(clock, 0 | 1 | 7 | 8 | 9) {
        return EINVAL;
    }
    create(Kind::Timer { realtime: clock == 0 || clock == 8, next: 0, interval: 0, expirations: 0 }, flags)
}

fn timer_id(fd: u64) -> Result<u32, i64> {
    match target(fd) {
        Target::Special(id) if matches!(with(|t| t.get(&id).map(|o| o.kind)), Some(Kind::Timer { .. })) => Ok(id),
        Target::Bad => Err(EBADF),
        _ => Err(EINVAL),
    }
}

fn timespec_ms(raw: &[u8]) -> Result<u64, i64> {
    let secs = i64::from_le_bytes(raw[0..8].try_into().unwrap());
    let nanos = i64::from_le_bytes(raw[8..16].try_into().unwrap());
    if secs < 0 || !(0..1_000_000_000).contains(&nanos) {
        return Err(EINVAL);
    }
    Ok((secs as u64).saturating_mul(1000).saturating_add((nanos as u64).div_ceil(1_000_000)))
}

fn current_setting(id: u32) -> [u8; 32] {
    let (next, interval) = with(|t| {
        let Some(o) = t.get_mut(&id) else {
            return (0, 0);
        };
        update_timer(&mut o.kind);
        if let Kind::Timer { next, interval, .. } = o.kind { (next, interval) } else { (0, 0) }
    });
    let mut raw = [0u8; 32];
    let put = |raw: &mut [u8; 32], at: usize, ticks: u64| {
        let ms = ticks * 1000 / task::TICK_HZ;
        raw[at..at + 8].copy_from_slice(&(ms / 1000).to_le_bytes());
        raw[at + 8..at + 16].copy_from_slice(&((ms % 1000) * 1_000_000).to_le_bytes());
    };
    put(&mut raw, 0, interval);
    put(&mut raw, 16, if next == 0 { 0 } else { next.saturating_sub(task::ticks()).max(1) });
    raw
}

pub fn sys_timerfd_settime(fd: u64, flags: u64, new: u64, old: u64) -> i64 {
    let id = match timer_id(fd) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let raw = match user_slice(new, 32) {
        Ok(b) => b.to_vec(),
        Err(e) => return e,
    };
    let (interval_ms, value_ms) = match (timespec_ms(&raw[0..16]), timespec_ms(&raw[16..32])) {
        (Ok(i), Ok(v)) => (i, v),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    if old != 0 {
        let current = current_setting(id);
        if copy_out(old, 32, &current) < 0 {
            return -14;
        }
    }
    let realtime = matches!(with(|t| t.get(&id).map(|o| o.kind)), Some(Kind::Timer { realtime: true, .. }));
    let now = task::ticks();
    let next = if value_ms == 0 && raw[16..32].iter().all(|b| *b == 0) {
        0
    } else if flags & TFD_TIMER_ABSTIME != 0 {
        let now_ms = if realtime { crate::drivers::rtc::boot_epoch() * 1000 + task::uptime_ms() } else { task::uptime_ms() };
        now + task::ms_to_ticks(value_ms.saturating_sub(now_ms).max(1))
    } else {
        now + task::ms_to_ticks(value_ms.max(1))
    };
    let interval = if interval_ms == 0 { 0 } else { task::ms_to_ticks(interval_ms) };
    with(|t| {
        if let Some(o) = t.get_mut(&id) {
            if let Kind::Timer { next: n, interval: i, expirations, .. } = &mut o.kind {
                *n = next;
                *i = interval;
                *expirations = 0;
            }
        }
    });
    task::notify_input();
    0
}

pub fn sys_timerfd_gettime(fd: u64, out: u64) -> i64 {
    let id = match timer_id(fd) {
        Ok(id) => id,
        Err(e) => return e,
    };
    copy_out(out, 32, &current_setting(id)).min(0)
}

pub fn sys_signalfd4(fd: u64, mask_ptr: u64, size: u64, flags: u64) -> i64 {
    if size != 8 || flags & !(CLOEXEC | NONBLOCK) != 0 {
        return EINVAL;
    }
    let mask = match user_slice(mask_ptr, 8) {
        Ok(b) => u64::from_le_bytes((&*b).try_into().unwrap()),
        Err(e) => return e,
    } & !((1 << 8) | (1 << 18));
    if fd as i64 != -1 {
        return match target(fd) {
            Target::Special(id) => {
                let ok = with(|t| match t.get_mut(&id).map(|o| &mut o.kind) {
                    Some(Kind::Signal { mask: m }) => {
                        *m = mask;
                        true
                    }
                    _ => false,
                });
                if ok { fd as i64 } else { EINVAL }
            }
            Target::Bad => EBADF,
            _ => EINVAL,
        };
    }
    create(Kind::Signal { mask }, flags)
}
