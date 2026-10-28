use super::*;

pub(in crate::syscall) const SYS_HAMIX_TRACE: u64 = 9103;
pub(in crate::syscall) const SYS_HAMIX_PROF: u64 = 9104;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, _a3: u64, _a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_TRACE => sys_trace(a1, a2),
        SYS_HAMIX_PROF => {
            if let Err(e) = require_root() {
                return Some(e);
            }
            crate::prof::control(a1)
        }
        _ => return None,
    })
}
