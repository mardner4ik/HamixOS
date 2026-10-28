use super::*;

pub(in crate::syscall) const SYS_HAMIX_PIPE: u64 = 9090;
pub(in crate::syscall) const SYS_HAMIX_ISATTY: u64 = 9093;
pub(in crate::syscall) const SYS_HAMIX_TERMSIZE: u64 = 9094;
pub(in crate::syscall) const SYS_HAMIX_SET_TERMINAL: u64 = 9095;
pub(in crate::syscall) const SYS_HAMIX_TERMGEN: u64 = 9097;
pub(in crate::syscall) const SYS_HAMIX_SET_FOREGROUND: u64 = 9096;
pub(in crate::syscall) const SYS_HAMIX_SESSION_ATTACH: u64 = 9098;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_PIPE => sys_pipe(a1, 0),
        SYS_HAMIX_ISATTY => is_tty(a1) as i64,
        SYS_HAMIX_TERMSIZE => match term_size(a1) {
            Some((cols, rows)) => ((cols as i64) << 16) | rows as i64,
            None => ENOTTY,
        },
        SYS_HAMIX_SET_TERMINAL => match target(a1) {
            Target::PipeRead(id) | Target::PipeWrite(id) => {
                if pipe::mark_terminal(id, a2 as u16, a3 as u16, matches!(target(a1), Target::PipeRead(_))) {
                    notify_resize(id);
                }
                0
            }
            Target::PtyMaster(input, output) | Target::Pty(input, output) => {
                pipe::set_terminal(output, a2 as u16, a3 as u16);
                if pipe::set_terminal(input, a2 as u16, a3 as u16) {
                    notify_resize(input);
                }
                0
            }
            _ => ENOTTY,
        },
        SYS_HAMIX_TERMGEN => match target(a1) {
            Target::Console => 0,
            Target::PipeRead(id) | Target::PipeWrite(id) | Target::Pty(id, _) => match pipe::terminal_generation(id) {
                Some(generation) => generation as i64,
                None => ENOTTY,
            },
            _ => ENOTTY,
        },
        SYS_HAMIX_SET_FOREGROUND => sys_set_foreground(a1),
        SYS_HAMIX_SESSION_ATTACH => {
            if let Err(e) = require_root() {
                return Some(e);
            }
            let vt = a1.wrapping_sub(1) as usize;
            if vt >= crate::tty::vt::VT_COUNT {
                return Some(EINVAL);
            }
            let pid = task::current_pid();
            if !crate::tty::vt::attach_session(vt, pid) {
                return Some(EBUSY);
            }
            task::move_current_to_vt(vt);
            0
        }
        _ => return None,
    })
}

pub(in crate::syscall) fn sys_set_foreground(target_pid: u64) -> i64 {
    let (pid, vt, _, _) = current_identity();
    let target_pid = if target_pid == 0 { pid } else { target_pid as Pid };
    match target(0) {
        Target::Console => {
            let owner = crate::tty::vt::input_owner(vt);
            let owner_parent = task::with_task(owner, |t| t.parent).unwrap_or(0);
            if owner == pid || owner == 0 || owner_parent == pid || !task::exists(owner) || crate::tty::vt::leads_foreground_group(pid, vt) {
                crate::tty::vt::set_input_owner(vt, target_pid);
                0
            } else {
                EPERM
            }
        }
        Target::PipeRead(id) | Target::Pty(id, _) => {
            pipe::set_foreground(id, if target_pid == pid { 0 } else { target_pid });
            0
        }
        _ => ENOTTY,
    }
}
