use super::*;

pub(in crate::syscall) const SYS_HAMIX_POWER: u64 = 9110;

pub(super) fn dispatch(number: u64, a1: u64, _a2: u64, _a3: u64, _a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_POWER => sys_power(a1),
        _ => return None,
    })
}

pub(in crate::syscall) fn sys_power(mode: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    power_off_mode(mode)
}
