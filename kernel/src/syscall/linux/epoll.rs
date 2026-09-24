use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::syscall::{copy_out, install_fd, target, user_slice, Target, EBADF, EINVAL, ENOENT};
use super::tty;
use crate::arch::without_interrupts;
use crate::task::{self, OpenFile};

const EPOLL_CTL_ADD: u64 = 1;
const EPOLL_CTL_DEL: u64 = 2;
const EPOLL_CTL_MOD: u64 = 3;
const EPOLLONESHOT: u32 = 1 << 30;
const EPOLL_CLOEXEC: u64 = 0o2000000;
#[cfg(target_arch = "x86_64")]
const EVENT_SIZE: usize = 12;
#[cfg(not(target_arch = "x86_64"))]
const EVENT_SIZE: usize = 16;
const DATA_AT: usize = EVENT_SIZE - 8;
const MAX_EVENTS: u64 = 4096;
const EEXIST: i64 = -17;
const ELOOP: i64 = -40;

#[derive(Clone, Copy)]
struct Interest {
    events: u32,
    data: u64,
    armed: bool,
}

struct Instance {
    refs: u32,
    items: BTreeMap<i32, Interest>,
}

static INSTANCES: Mutex<BTreeMap<u32, Instance>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

fn with<R>(f: impl FnOnce(&mut BTreeMap<u32, Instance>) -> R) -> R {
    without_interrupts(|| f(&mut INSTANCES.lock()))
}

pub fn retain(id: u32) {
    with(|m| {
        if let Some(i) = m.get_mut(&id) {
            i.refs += 1;
        }
    });
}

pub fn release(id: u32) {
    with(|m| {
        if let Some(i) = m.get_mut(&id) {
            i.refs = i.refs.saturating_sub(1);
            if i.refs == 0 {
                m.remove(&id);
            }
        }
    });
}

pub fn sys_epoll_create(flags: u64) -> i64 {
    if flags & !EPOLL_CLOEXEC != 0 {
        return EINVAL;
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    with(|m| m.insert(id, Instance { refs: 1, items: BTreeMap::new() }));
    install_fd(OpenFile::Epoll(id))
}

fn instance_of(fd: u64) -> Result<u32, i64> {
    match target(fd) {
        Target::Epoll(id) => Ok(id),
        Target::Bad => Err(EBADF),
        _ => Err(EINVAL),
    }
}

pub fn sys_epoll_ctl(epfd: u64, op: u64, fd: u64, event: u64) -> i64 {
    let id = match instance_of(epfd) {
        Ok(id) => id,
        Err(e) => return e,
    };
    if fd == epfd {
        return EINVAL;
    }
    match target(fd) {
        Target::Bad => return EBADF,
        Target::Epoll(_) => return ELOOP,
        _ => {}
    }
    let interest = if op == EPOLL_CTL_DEL {
        None
    } else {
        match user_slice(event, EVENT_SIZE as u64) {
            Ok(raw) => Some(Interest {
                events: u32::from_le_bytes(raw[0..4].try_into().unwrap()),
                data: u64::from_le_bytes(raw[DATA_AT..DATA_AT + 8].try_into().unwrap()),
                armed: true,
            }),
            Err(e) => return e,
        }
    };
    with(|m| {
        let Some(instance) = m.get_mut(&id) else {
            return EBADF;
        };
        let key = fd as i32;
        match (op, interest) {
            (EPOLL_CTL_ADD, Some(i)) => {
                if instance.items.contains_key(&key) {
                    return EEXIST;
                }
                instance.items.insert(key, i);
                0
            }
            (EPOLL_CTL_MOD, Some(i)) => match instance.items.get_mut(&key) {
                Some(slot) => {
                    *slot = i;
                    0
                }
                None => ENOENT,
            },
            (EPOLL_CTL_DEL, _) => {
                if instance.items.remove(&key).is_some() {
                    0
                } else {
                    ENOENT
                }
            }
            _ => EINVAL,
        }
    })
}

fn collect(id: u32, max: usize) -> Vec<(u32, u64)> {
    let items: Vec<(i32, Interest)> = with(|m| m.get(&id).map(|i| i.items.iter().map(|(k, v)| (*k, *v)).collect()).unwrap_or_default());
    let mut ready = Vec::new();
    let mut stale = Vec::new();
    let mut fired = Vec::new();
    for (fd, interest) in items {
        if ready.len() >= max {
            break;
        }
        if !interest.armed {
            continue;
        }
        if matches!(target(fd as u64), Target::Bad) {
            stale.push(fd);
            continue;
        }
        let revents = tty::readiness(fd as u64, (interest.events & 0xFFFF) as u16) as u32 | tty::rdhup(fd as u64, interest.events);
        if revents != 0 {
            ready.push((revents, interest.data));
            if interest.events & EPOLLONESHOT != 0 {
                fired.push(fd);
            }
        }
    }
    if !stale.is_empty() || !fired.is_empty() {
        with(|m| {
            if let Some(instance) = m.get_mut(&id) {
                for fd in stale {
                    instance.items.remove(&fd);
                }
                for fd in fired {
                    if let Some(i) = instance.items.get_mut(&fd) {
                        i.armed = false;
                    }
                }
            }
        });
    }
    ready
}

pub fn ready_count(id: u32) -> usize {
    let items: Vec<(i32, Interest)> = with(|m| m.get(&id).map(|i| i.items.iter().map(|(k, v)| (*k, *v)).collect()).unwrap_or_default());
    items
        .into_iter()
        .filter(|(fd, i)| i.armed && !matches!(target(*fd as u64), Target::Bad | Target::Epoll(_)) && tty::readiness(*fd as u64, (i.events & 0xFFFF) as u16) != 0)
        .count()
}

pub fn sys_epoll_wait(epfd: u64, events: u64, max: u64, timeout_ms: i64) -> i64 {
    let id = match instance_of(epfd) {
        Ok(id) => id,
        Err(e) => return e,
    };
    if max == 0 || max > MAX_EVENTS {
        return EINVAL;
    }
    if let Err(e) = user_slice(events, max * EVENT_SIZE as u64) {
        return e;
    }
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    loop {
        let ready = collect(id, max as usize);
        if !ready.is_empty() {
            let mut raw = Vec::with_capacity(ready.len() * EVENT_SIZE);
            for (ev, data) in &ready {
                raw.extend_from_slice(&ev.to_le_bytes());
                raw.resize(raw.len() + DATA_AT - 4, 0);
                raw.extend_from_slice(&data.to_le_bytes());
            }
            let r = copy_out(events, raw.len() as u64, &raw);
            return if r < 0 { r } else { ready.len() as i64 };
        }
        let now = task::ticks();
        if timeout_ms == 0 || deadline.map(|d| now >= d).unwrap_or(false) {
            return 0;
        }
        if super::signal::pending() {
            return super::signal::EINTR;
        }
        let slice = deadline.map(|d| d - now).unwrap_or(task::TICK_HZ / 10).clamp(1, task::TICK_HZ / 10);
        tty::sleep_for_events(slice);
    }
}

pub fn sys_mailbox_fd() -> i64 {
    install_fd(OpenFile::Mailbox)
}
