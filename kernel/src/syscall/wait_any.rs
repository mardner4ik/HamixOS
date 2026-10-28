use alloc::vec::Vec;

use super::{copy_out, user_slice, EINVAL};
use crate::task::{self, ipc, Pid};

const ENTRY: usize = 24;
const MAX_ENTRIES: u64 = 256;
const KIND_FD: u32 = 1;
const KIND_MAILBOX: u32 = 2;
const KIND_CHILD: u32 = 3;
const KIND_TIMER: u32 = 4;

fn child_exited(pid: u64) -> bool {
    let me = task::current_pid();
    task::with_tasks(|tasks| tasks.values().any(|t| t.parent == me && t.pid == t.leader && t.state == task::State::Zombie && (pid == 0 || t.pid == pid as Pid)))
}

fn now_ns() -> u64 {
    task::uptime_ms().saturating_mul(1_000_000)
}

pub fn sys_wait_any(set: u64, count: u64, timeout_ns: u64) -> i64 {
    if count == 0 || count > MAX_ENTRIES {
        return EINVAL;
    }
    let len = count * ENTRY as u64;
    let mut entries: Vec<u8> = match user_slice(set, len) {
        Ok(b) => b.to_vec(),
        Err(e) => return e,
    };
    let deadline = if timeout_ns == u64::MAX { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ns.div_ceil(1_000_000))) };
    loop {
        let mut ready = 0i64;
        let mut timer_deadline: Option<u64> = None;
        for chunk in entries.chunks_exact_mut(ENTRY) {
            let kind = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
            let flags = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
            let id = u64::from_le_bytes(chunk[8..16].try_into().unwrap());
            let state: u64 = match kind {
                KIND_FD => crate::tty::readiness(id, (flags as u16).max(1)) as u64,
                KIND_MAILBOX => ipc::has_message() as u64,
                KIND_CHILD => child_exited(id) as u64,
                KIND_TIMER => {
                    if now_ns() >= id {
                        1
                    } else {
                        let ticks = task::ticks() + task::ms_to_ticks((id - now_ns()).div_ceil(1_000_000));
                        timer_deadline = Some(timer_deadline.map_or(ticks, |t: u64| t.min(ticks)));
                        0
                    }
                }
                _ => return EINVAL,
            };
            chunk[16..24].copy_from_slice(&state.to_le_bytes());
            if state != 0 {
                ready += 1;
            }
        }
        let expired = deadline.map(|d| task::ticks() >= d).unwrap_or(false);
        if ready > 0 || timeout_ns == 0 || expired {
            let r = copy_out(set, len, &entries);
            return if r < 0 { r } else { ready };
        }
        if super::signal::pending() {
            return super::signal::EINTR;
        }
        let now = task::ticks();
        let mut until = deadline.unwrap_or(now + task::TICK_HZ / 10);
        if let Some(t) = timer_deadline {
            until = until.min(t);
        }
        let slice = until.saturating_sub(now).clamp(1, task::TICK_HZ / 10);
        task::block(task::WAIT_INPUT | task::WAIT_PIPE | task::WAIT_MSG | task::WAIT_CHILD, Some(slice), task::input_seq());
        task::check_killed();
        crate::tty::vt::service_pending();
    }
}
