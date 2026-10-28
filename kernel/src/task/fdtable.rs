use alloc::string::String;
use alloc::vec::Vec;

use crate::errno::EMFILE;
use crate::fs;
use crate::task::{self, OpenFile};

pub(crate) const FIRST_USER_FD: usize = 3;

pub(crate) enum Target {
    Console,
    File(usize, usize, bool),
    PipeRead(u32),
    PipeWrite(u32),
    Dir(String),
    Mem(String, alloc::sync::Arc<Vec<u8>>, usize),
    Socket(u32, bool),
    Memfd(u32, usize),
    Epoll(u32),
    Mailbox,
    Special(u32),
    Pty(u32, u32),
    PtyMaster(u32, u32),
    Bad,
}

pub(crate) fn target(fd: u64) -> Target {
    task::with_current(|t| match t.fds.get(fd as usize) {
        Some(Some(OpenFile::File { node, pos, append, .. })) => Target::File(*node, pos.get(), *append),
        Some(Some(OpenFile::PipeRead(id))) => Target::PipeRead(*id),
        Some(Some(OpenFile::PipeWrite(id))) => Target::PipeWrite(*id),
        Some(Some(OpenFile::Console)) => Target::Console,
        Some(Some(OpenFile::Dir { path, .. })) => Target::Dir(path.clone()),
        Some(Some(OpenFile::Mem { path, data, pos })) => Target::Mem(path.clone(), data.clone(), pos.get()),
        Some(Some(OpenFile::Socket { id, nonblock })) => Target::Socket(*id, *nonblock),
        Some(Some(OpenFile::Memfd { id, pos, .. })) => Target::Memfd(*id, pos.get()),
        Some(Some(OpenFile::Epoll(id))) => Target::Epoll(*id),
        Some(Some(OpenFile::Mailbox)) => Target::Mailbox,
        Some(Some(OpenFile::Special(id))) => Target::Special(*id),
        Some(Some(OpenFile::PtySlave { input, output, .. })) => Target::Pty(*input, *output),
        Some(Some(OpenFile::PtyMaster { input, output, .. })) => Target::PtyMaster(*input, *output),
        _ if fd < 3 && !t.kernel_thread && !t.abi.posix() => Target::Console,
        _ => Target::Bad,
    })
}

pub(crate) fn put_flag(bits: &mut [u64; 4], fd: u64, on: bool) {
    if fd < 256 {
        if on {
            bits[fd as usize / 64] |= 1 << (fd % 64);
        } else {
            bits[fd as usize / 64] &= !(1 << (fd % 64));
        }
    }
}

pub(crate) fn install_fd(file: OpenFile) -> i64 {
    let leftover = task::with_current(|t| {
        let first = if t.abi.posix() { 0 } else { FIRST_USER_FD };
        if !t.fds.iter().skip(first).any(|f| f.is_none()) && t.fds.len() < 256 {
            let grow = (t.fds.len() + 32).min(256);
            t.fds.resize_with(grow, || None);
        }
        match t.fds.iter().skip(first).position(|f| f.is_none()) {
            Some(slot) => {
                t.fds[slot + first] = Some(file);
                put_flag(&mut t.cloexec, (slot + first) as u64, false);
                put_flag(&mut t.nonblock, (slot + first) as u64, false);
                Ok((slot + first) as i64)
            }
            None => Err(file),
        }
    });
    match leftover {
        Ok(fd) => fd,
        Err(file) => {
            file.release();
            EMFILE
        }
    }
}

pub(crate) fn euid() -> u32 {
    task::with_current(|t| t.uid)
}

pub(crate) fn absolute(path: &str) -> String {
    if path.starts_with('/') {
        return fs::absolute("", path);
    }
    let (cwd, abi) = task::with_current(|t| (t.cwd.clone(), t.abi));
    let cwd = if abi == task::Abi::Linux { crate::linux::loader::to_linux(&cwd) } else { cwd };
    fs::absolute(&cwd, path)
}
