use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::*;
use crate::drivers::audio;
use crate::drivers::block;
use crate::fs;
use crate::task::{self, ipc, pipe, registry, Pid};

mod display;
mod gpu;
mod process;
mod messages;
mod commands;
mod files;
mod disk;
mod term;
mod power;
mod debug;
mod net;
mod sound;

pub(super) use display::*;
pub(super) use process::*;
pub(super) use files::*;
pub(super) use sound::*;

pub(super) const SPAWN_DETACH: u64 = 1;
pub(super) const SPAWN_FOREGROUND: u64 = 2;
pub(super) const SPAWN_ROOT: u64 = 4;
pub(super) const SPAWN_ENV: u64 = 8;
pub(super) const MAX_ENV_ENTRIES: usize = 1024;
pub(super) const EVENT_INPUT: u64 = 1;
pub(super) const EVENT_MESSAGE: u64 = 2;
pub(super) const EVENT_PIPE: u64 = 4;
pub(super) const EVENT_SOCKET: u64 = 8;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> Option<i64> {
    display::dispatch(number, a1, a2, a3, a4, a5, a6)
        .or_else(|| gpu::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| process::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| messages::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| commands::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| files::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| disk::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| term::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| power::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| debug::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| net::dispatch(number, a1, a2, a3, a4, a5, a6))
        .or_else(|| sound::dispatch(number, a1, a2, a3, a4, a5, a6))
}

pub(super) fn require_root_or_console() -> Result<(), i64> {
    let uid = euid();
    if uid == 0 {
        return Ok(());
    }
    let owner_uid = crate::task::display::owner().and_then(|pid| task::with_task(pid, |t| t.ruid));
    let vt = task::current_vt();
    if owner_uid == Some(uid) || crate::tty::vt::foreground() == vt { Ok(()) } else { Err(EPERM) }
}

pub(super) fn require_root() -> Result<(), i64> {
    if euid() == 0 { Ok(()) } else { Err(EPERM) }
}
