use super::*;

pub(in crate::syscall) const SYS_HAMIX_TRUNCATE: u64 = 9003;
pub(in crate::syscall) const SYS_HAMIX_SYNC: u64 = 9050;
pub(in crate::syscall) const SYS_HAMIX_READDIR: u64 = 9051;
pub(in crate::syscall) const SYS_HAMIX_STAT: u64 = 9052;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_TRUNCATE => sys_truncate(a1),
        SYS_HAMIX_SYNC => {
            fs::sync();
            0
        }
        SYS_HAMIX_READDIR => sys_readdir(a1, a2, a3, a4),
        SYS_HAMIX_STAT => sys_stat(a1, a2),
        _ => return None,
    })
}

pub(in crate::syscall) fn sys_truncate(path_ptr: u64) -> i64 {
    let path = match path_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let uid = euid();
    let mut guard = fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return ENODEV;
    };
    let root = vfs.root_id();
    match vfs.resolve(root, &path) {
        Some(id) => match vfs.truncate_node(id, uid) {
            Ok(()) => 0,
            Err(_) => EACCES,
        },
        None => match vfs.create_file(root, &path, Vec::new(), uid) {
            Ok(_) => 0,
            Err(e) => fs_error(e),
        },
    }
}

pub(in crate::syscall) fn sys_mkdir(path_ptr: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => vfs_op(|v, uid| if v.exists(0, &path) { Err("already exists") } else { v.mkdir(0, &path, uid).map(|_| ()) }),
        Err(e) => e,
    }
}

pub(in crate::syscall) fn sys_unlink(path_ptr: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => {
            crate::net::socket::unlink_hook(&path);
            vfs_op(|v, uid| v.remove(0, &path, uid))
        }
        Err(e) => e,
    }
}

pub(in crate::syscall) fn sys_rename(from_ptr: u64, to_ptr: u64) -> i64 {
    match (path_arg(from_ptr), path_arg(to_ptr)) {
        (Ok(from), Ok(to)) => vfs_op(|v, uid| v.rename(0, &from, &to, uid)),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

pub(in crate::syscall) fn sys_chmod(path_ptr: u64, mode: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => vfs_op(|v, uid| v.chmod(0, &path, uid, mode as u16)),
        Err(e) => e,
    }
}

pub(in crate::syscall) fn sys_chown(path_ptr: u64, owner: u64, group: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => {
            let cred = task::current_cred();
            let owner = (owner as u32 != u32::MAX).then_some(owner as u32);
            let group = (group as u32 != u32::MAX).then_some(group as u32);
            vfs_op(|v, _| v.chown(0, &path, cred, owner, group))
        }
        Err(e) => e,
    }
}

pub(in crate::syscall) fn sys_stat(path_ptr: u64, buf: u64) -> i64 {
    let path = match path_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let stat = {
        let guard = fs::VFS.lock();
        let Some(vfs) = guard.as_ref() else {
            return ENODEV;
        };
        match vfs.resolve(0, &path) {
            Some(id) => vfs.stat(id),
            None => return ENOENT,
        }
    };
    let mut raw = [0u8; 24];
    raw[0..4].copy_from_slice(&stat.kind.to_le_bytes());
    raw[4..8].copy_from_slice(&(stat.mode as u32).to_le_bytes());
    raw[8..12].copy_from_slice(&stat.owner.to_le_bytes());
    raw[12..16].copy_from_slice(&(stat.dev as u32).to_le_bytes());
    raw[16..24].copy_from_slice(&stat.size.to_le_bytes());
    let r = copy_out(buf, 24, &raw);
    if r < 0 { r } else { 0 }
}

pub(in crate::syscall) fn sys_readdir(path_ptr: u64, path_len: u64, buf: u64, len: u64) -> i64 {
    let path = match user_string(path_ptr, path_len) {
        Ok(p) => absolute(&p),
        Err(e) => return e,
    };
    let mut text = String::new();
    {
        let guard = fs::VFS.lock();
        let Some(vfs) = guard.as_ref() else {
            return ENODEV;
        };
        let entries = match vfs.list_detailed(vfs.root_id(), &path) {
            Ok(entries) => entries,
            Err("not a directory") => return ENOTDIR,
            Err(_) => return ENOENT,
        };
        for (name, id) in entries {
            let stat = vfs.stat(id);
            let kind = match stat.kind {
                fs::KIND_DIR => 'd',
                fs::KIND_DEVICE => 'c',
                fs::KIND_PROC => 'p',
                _ => 'f',
            };
            text.push_str(&format!("{}\t{}\t{}\t{}\t{}\n", name, kind, stat.size, stat.mode, stat.owner));
        }
    }
    copy_out(buf, len, text.as_bytes())
}
