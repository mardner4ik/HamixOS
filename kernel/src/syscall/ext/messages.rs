use super::*;

pub(in crate::syscall) const SYS_HAMIX_WAIT_EVENT: u64 = 9014;
pub(in crate::syscall) const SYS_HAMIX_MSG_SEND: u64 = 9020;
pub(in crate::syscall) const SYS_HAMIX_MSG_RECV: u64 = 9021;
pub(in crate::syscall) const SYS_HAMIX_SERVICE_REGISTER: u64 = 9022;
pub(in crate::syscall) const SYS_HAMIX_SERVICE_LOOKUP: u64 = 9023;
pub(in crate::syscall) const SYS_HAMIX_MAILBOX_FD: u64 = 9024;
pub(in crate::syscall) const SYS_HAMIX_SERVICE_ACTIVATOR: u64 = 9029;
pub(in crate::syscall) const SYS_HAMIX_SHM_CREATE: u64 = 9030;
pub(in crate::syscall) const SYS_HAMIX_SHM_MAP: u64 = 9031;
pub(in crate::syscall) const SYS_HAMIX_SHM_RELEASE: u64 = 9032;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_MAILBOX_FD => crate::fs::epoll::sys_mailbox_fd(),
        SYS_HAMIX_WAIT_EVENT => sys_wait_event(a1, a2 as i64),
        SYS_HAMIX_MSG_SEND => match user_slice(a2, a3) {
            Ok(data) => ipc::send(task::current_pid(), a1 as Pid, data),
            Err(e) => e,
        },
        SYS_HAMIX_MSG_RECV => sys_msg_recv(a1, a2, a3, a4 as i64, a5 as Pid),
        SYS_HAMIX_SERVICE_REGISTER => match user_string(a1, a2) {
            Ok(name) => ipc::register_service(&name, task::current_pid()),
            Err(e) => e,
        },
        SYS_HAMIX_SERVICE_LOOKUP => match user_string(a1, a2) {
            Ok(name) => ipc::lookup_service_activating(&name),
            Err(e) => e,
        },
        SYS_HAMIX_SERVICE_ACTIVATOR => {
            if let Err(e) = require_root() {
                return Some(e);
            }
            match user_string(a1, a2) {
                Ok(name) => ipc::register_activator(&name, task::current_pid()),
                Err(e) => e,
            }
        }
        SYS_HAMIX_SHM_CREATE => ipc::shm_create(task::current_pid(), a1),
        SYS_HAMIX_SHM_MAP => match ipc::shm_map(task::current_pid(), a1 as u32) {
            Ok((addr, len)) => {
                if a2 != 0 {
                    if let Ok(out) = user_slice_mut(a2, 8) {
                        out.copy_from_slice(&len.to_le_bytes());
                    }
                }
                addr as i64
            }
            Err(e) => e,
        },
        SYS_HAMIX_SHM_RELEASE => ipc::shm_release(task::current_pid(), a1 as u32),
        _ => return None,
    })
}

pub(in crate::syscall) fn sys_wait_event(mask: u64, timeout_ms: i64) -> i64 {
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    let stdin_pipe = match read_target(0) {
        Target::PipeRead(id) => Some(id),
        _ => None,
    };
    loop {
        let seq = task::input_seq();
        let (pid, vt, _, _) = current_identity();
        let mut ready = 0u64;
        if mask & EVENT_MESSAGE != 0 && ipc::has_message() {
            ready |= EVENT_MESSAGE;
        }
        if mask & EVENT_PIPE != 0 && pipe_fds_readable() {
            ready |= EVENT_PIPE;
        }
        if mask & EVENT_SOCKET != 0 && socket_fds_pending() {
            ready |= EVENT_SOCKET;
        }
        if mask & EVENT_INPUT != 0 {
            if let Some(id) = stdin_pipe {
                if pipe::available(id) > 0 {
                    ready |= EVENT_INPUT;
                }
            } else if crate::tty::vt::input_allowed(pid, vt) {
                let fresh = task::with_current(|t| {
                    let changed = t.seen_input != seq;
                    t.seen_input = seq;
                    changed
                });
                if fresh || keyboard::has_pending() {
                    ready |= EVENT_INPUT;
                }
            }
        }
        if ready != 0 {
            return ready as i64;
        }
        if timeout_ms == 0 {
            return 0;
        }
        let remaining = match deadline {
            Some(d) => {
                let now = task::ticks();
                if now >= d {
                    return 0;
                }
                Some(d - now)
            }
            None => Some(task::TICK_HZ),
        };
        let mut flags = 0;
        if mask & EVENT_INPUT != 0 {
            flags |= if stdin_pipe.is_some() { task::WAIT_PIPE } else { task::WAIT_INPUT };
        }
        if mask & EVENT_MESSAGE != 0 {
            flags |= task::WAIT_MSG;
        }
        if mask & (EVENT_PIPE | EVENT_SOCKET) != 0 {
            flags |= task::WAIT_PIPE;
        }
        if posix_abi() && crate::syscall::signal::pending() {
            return EINTR;
        }
        task::block(flags, remaining.map(|r| r.min(task::TICK_HZ / 10)), seq);
        task::check_killed();
        crate::tty::vt::service_pending();
    }
}

pub(in crate::syscall) fn sys_msg_recv(buf: u64, len: u64, sender_ptr: u64, timeout_ms: i64, from: Pid) -> i64 {
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    loop {
        let taken = if from == 0 { ipc::take_message() } else { ipc::take_message_from(from) };
        if let Some(message) = taken {
            if sender_ptr != 0 {
                if let Ok(out) = user_slice_mut(sender_ptr, 4) {
                    out.copy_from_slice(&message.sender.to_le_bytes());
                }
            }
            return copy_out(buf, len, &message.data);
        }
        if timeout_ms == 0 {
            return EAGAIN;
        }
        let remaining = match deadline {
            Some(d) => {
                let now = task::ticks();
                if now >= d {
                    return EAGAIN;
                }
                Some(d - now)
            }
            None => Some(task::TICK_HZ),
        };
        if posix_abi() && crate::syscall::signal::pending() {
            return EINTR;
        }
        task::block(task::WAIT_MSG, remaining.map(|r| r.min(task::TICK_HZ / 10)), task::input_seq());
        task::check_killed();
    }
}
