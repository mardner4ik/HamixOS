pub mod loader;
pub mod paths;
#[cfg(not(target_arch = "x86_64"))]
pub mod syscalls;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::task::{self, Abi, Pid};

pub const HAMIX_ABI_LEGACY: u32 = 1;
pub const HAMIX_ABI_MIN: u32 = 0x0100;
pub const HAMIX_ABI_VERSION: u32 = 0x0100;

const OFF: u8 = 0;
const IDLE: u8 = 1;
const ACTIVE: u8 = 2;

static LINUX: AtomicU8 = AtomicU8::new(IDLE);

pub trait Personality: Sync {
    fn name(&self) -> &'static str;
    fn dispatch(&self, number: u64, args: [u64; 6]) -> Option<i64>;
    fn admit(&self, program: &str) -> Result<(), &'static str>;
}

pub struct Hamix;
pub struct Linux;

static HAMIX: Hamix = Hamix;
static LINUX_PERSONALITY: Linux = Linux;

pub fn of(abi: Abi) -> &'static dyn Personality {
    match abi {
        Abi::Linux => &LINUX_PERSONALITY,
        Abi::Native | Abi::Hamix => &HAMIX,
    }
}

impl Personality for Hamix {
    fn name(&self) -> &'static str {
        "hamix"
    }

    fn dispatch(&self, number: u64, args: [u64; 6]) -> Option<i64> {
        let [a1, a2, a3, a4, a5, a6] = args;
        if number >= crate::syscall::NATIVE_BASE {
            return crate::syscall::native_dispatch(number, a1, a2, a3, a4, a5, a6);
        }
        if !crate::syscall::core_table::allowed(number) {
            return None;
        }
        crate::syscall::core_dispatch(number, a1, a2, a3, a4, a5, a6)
    }

    fn admit(&self, _program: &str) -> Result<(), &'static str> {
        Ok(())
    }
}

impl Personality for Linux {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn dispatch(&self, number: u64, args: [u64; 6]) -> Option<i64> {
        let [a1, a2, a3, a4, a5, a6] = args;
        if number >= crate::syscall::NATIVE_BASE && number < 0xFFFF_0000 {
            return crate::syscall::native_dispatch(number, a1, a2, a3, a4, a5, a6);
        }
        crate::syscall::core_dispatch(number, a1, a2, a3, a4, a5, a6)
    }

    fn admit(&self, program: &str) -> Result<(), &'static str> {
        match LINUX.load(Ordering::Acquire) {
            OFF => Err("the Linux compatibility layer is off"),
            IDLE => {
                if LINUX.compare_exchange(IDLE, ACTIVE, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    crate::drivers::klog::log(&format!("linux: personality activated by the first Linux program ({})", program));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn config_disabled() -> bool {
    let text = {
        let mut guard = crate::fs::VFS.lock();
        guard.as_mut().and_then(|v| v.read(0, "/etc/hamix/linux.conf").ok())
    };
    let Some(text) = text else {
        return false;
    };
    String::from_utf8_lossy(&text).lines().any(|l| {
        let l = l.trim();
        matches!(l.split_once('=').map(|(k, v)| (k.trim(), v.trim().trim_matches('"'))), Some(("enabled", "no" | "0" | "off" | "false")))
    })
}

pub fn init() -> (bool, &'static str) {
    let cmdline = crate::memory::cmdline_value("linux");
    let off = match cmdline.as_deref() {
        Some("0" | "off" | "no") => Some("linux=0 on the kernel command line"),
        Some(_) => None,
        None if config_disabled() => Some("enabled=no in /etc/hamix/linux.conf"),
        None => None,
    };
    match off {
        Some(reason) => {
            LINUX.store(OFF, Ordering::Release);
            crate::fs::set_linux_nodes(false);
            (false, reason)
        }
        None => {
            LINUX.store(IDLE, Ordering::Release);
            crate::fs::set_linux_nodes(true);
            (true, "loads on the first Linux program")
        }
    }
}

pub fn linux_enabled() -> bool {
    LINUX.load(Ordering::Acquire) != OFF
}

pub fn linux_state() -> &'static str {
    match LINUX.load(Ordering::Acquire) {
        OFF => "off",
        IDLE => "on",
        _ => "active",
    }
}

pub fn linux_processes() -> Vec<Pid> {
    task::with_tasks(|tasks| tasks.values().filter(|t| t.leader == t.pid && t.abi == Abi::Linux && t.state != task::State::Zombie).map(|t| t.pid).collect())
}

pub fn set_linux(on: bool) -> usize {
    if on {
        let _ = LINUX.compare_exchange(OFF, IDLE, Ordering::AcqRel, Ordering::Acquire);
        crate::fs::set_linux_nodes(true);
        crate::drivers::klog::log("linux: compatibility layer switched on");
        return 0;
    }
    LINUX.store(OFF, Ordering::Release);
    crate::fs::set_linux_nodes(false);
    let victims = linux_processes();
    for pid in victims.iter() {
        task::kill(*pid, 143);
    }
    crate::drivers::klog::log(&format!("linux: compatibility layer switched off, {} Linux process(es) stopped", victims.len()));
    victims.len()
}

pub fn info_text(abi: Abi) -> String {
    format!(
        "version {}.{}\npersonality {}\nlinux {}\nlinux_processes {}\n",
        HAMIX_ABI_VERSION >> 8,
        HAMIX_ABI_VERSION & 0xFF,
        of(abi).name(),
        linux_state(),
        linux_processes().len()
    )
}
