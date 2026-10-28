use alloc::string::String;

use crate::linux::loader as linux_loader;
use crate::task::{self, Abi};

pub fn is_linux() -> bool {
    task::with_current(|t| t.abi) == Abi::Linux
}

pub fn guest_path(path: &str) -> String {
    guest_path_with(path, true)
}

pub fn guest_path_with(path: &str, follow_last: bool) -> String {
    match task::with_current(|t| t.abi) {
        Abi::Linux => linux_loader::follow_linux(path, follow_last),
        abi if abi.posix() => linux_loader::follow(path, follow_last),
        _ => String::from(path),
    }
}

pub fn view_of(native: &str) -> String {
    if is_linux() && native.starts_with('/') { linux_loader::to_linux(native) } else { String::from(native) }
}

pub fn native_of(path: &str) -> String {
    if is_linux() { linux_loader::to_native(path) } else { String::from(path) }
}

pub fn cwd_view(cwd: String) -> String {
    if is_linux() { linux_loader::to_linux(&cwd) } else { cwd }
}
