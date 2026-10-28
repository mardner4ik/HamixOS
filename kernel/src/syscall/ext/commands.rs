use super::*;

pub(in crate::syscall) const SYS_HAMIX_CMD_REGISTER: u64 = 9040;
pub(in crate::syscall) const SYS_HAMIX_CMD_UNREGISTER: u64 = 9041;
pub(in crate::syscall) const SYS_HAMIX_CMD_LIST: u64 = 9042;
pub(in crate::syscall) const SYS_HAMIX_CMD_RUN: u64 = 9043;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_CMD_REGISTER => sys_cmd_register(a1, a2, a3, a4),
        SYS_HAMIX_CMD_UNREGISTER => match user_string(a1, a2) {
            Ok(name) => match registry::unregister(&name, euid()) {
                Ok(()) => 0,
                Err(_) => EACCES,
            },
            Err(e) => e,
        },
        SYS_HAMIX_CMD_LIST => copy_out(a1, a2, registry::render().as_bytes()),
        SYS_HAMIX_CMD_RUN => sys_cmd_run(a1, a2, a3, a4),
        _ => return None,
    })
}

pub(in crate::syscall) fn sys_cmd_register(name_ptr: u64, name_len: u64, target_ptr: u64, packed: u64) -> i64 {
    let name = match user_string(name_ptr, name_len) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let parts = match user_args(target_ptr, packed & 0xFFFF_FFFF) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some((path, args)) = parts.split_first() else {
        return EINVAL;
    };
    let (_, _, uid, cwd) = current_identity();
    let path = fs::absolute(&cwd, path);
    match registry::register(&name, &path, args.to_vec(), uid) {
        Ok(()) => 0,
        Err("command belongs to another user") => EACCES,
        Err("target does not exist") => ENOENT,
        Err(_) => EINVAL,
    }
}

pub(in crate::syscall) fn sys_cmd_run(name_ptr: u64, name_len: u64, args_ptr: u64, packed: u64) -> i64 {
    let name = match user_string(name_ptr, name_len) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let extra = match user_args(args_ptr, packed & 0xFFFF_FFFF) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let Some(command) = registry::resolve(&name) else {
        return ENOENT;
    };
    let mut args = command.args.clone();
    args.extend(extra);
    spawn_from_user(&command.path, &args, None, (packed >> 32) & !SPAWN_ROOT, 0)
}
