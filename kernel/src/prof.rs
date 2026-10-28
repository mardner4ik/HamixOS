use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

const SLOTS: usize = 16384;
const USER_BIT: u32 = 1 << 31;

static ENABLED: AtomicBool = AtomicBool::new(false);
static IPS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static META: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static NEXT: AtomicUsize = AtomicUsize::new(0);
static STARTED_MS: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    if crate::memory::cmdline().split_whitespace().any(|w| w == "hxprof") {
        start();
    }
}

pub fn start() {
    STARTED_MS.store(crate::task::uptime_ms(), Ordering::Relaxed);
    ENABLED.store(true, Ordering::Release);
}

pub fn stop() {
    ENABLED.store(false, Ordering::Release);
}

pub fn reset() {
    NEXT.store(0, Ordering::Relaxed);
    for i in 0..SLOTS {
        IPS[i].store(0, Ordering::Relaxed);
        META[i].store(0, Ordering::Relaxed);
    }
    STARTED_MS.store(crate::task::uptime_ms(), Ordering::Relaxed);
}

pub fn control(op: u64) -> i64 {
    match op {
        0 => stop(),
        1 => start(),
        2 => reset(),
        _ => return -22,
    }
    ENABLED.load(Ordering::Relaxed) as i64
}

#[inline]
pub fn sample(ip: u64, pid: u32, user: bool) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let i = NEXT.fetch_add(1, Ordering::Relaxed) % SLOTS;
    IPS[i].store(ip, Ordering::Relaxed);
    META[i].store((pid & !USER_BIT) | if user { USER_BIT } else { 0 }, Ordering::Relaxed);
}

pub fn report() -> String {
    let taken = NEXT.load(Ordering::Relaxed).min(SLOTS);
    let mut kernel: BTreeMap<u64, u64> = BTreeMap::new();
    let mut user: BTreeMap<u32, u64> = BTreeMap::new();
    let mut user_ips: BTreeMap<(u32, u64), u64> = BTreeMap::new();
    let mut idle = 0u64;
    for i in 0..taken {
        let ip = IPS[i].load(Ordering::Relaxed);
        let meta = META[i].load(Ordering::Relaxed);
        let pid = meta & !USER_BIT;
        if meta & USER_BIT != 0 {
            *user.entry(pid).or_default() += 1;
            *user_ips.entry((pid, ip)).or_default() += 1;
        } else if pid == 0 {
            idle += 1;
        } else {
            *kernel.entry(ip).or_default() += 1;
        }
    }
    let mut out = format!(
        "hxprof {} samples over {} ms ({}), idle {}, kernel {}, user {}\nbkl_contended {}\nbkl_long_waits {}\n",
        taken,
        crate::task::uptime_ms().saturating_sub(STARTED_MS.load(Ordering::Relaxed)),
        if ENABLED.load(Ordering::Relaxed) { "running" } else { "stopped" },
        idle,
        kernel.values().sum::<u64>(),
        user.values().sum::<u64>(),
        crate::task::bkl::contention(),
        crate::task::bkl::long_waits()
    );
    let mut top: Vec<(u64, u64)> = kernel.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    out.push_str("kernel ip, samples\n");
    for (ip, n) in top.iter().take(40) {
        out.push_str(&format!("k {:#x} {}\n", ip, n));
    }
    let mut procs: Vec<(u32, u64)> = user.into_iter().collect();
    procs.sort_by(|a, b| b.1.cmp(&a.1));
    out.push_str("user pid, samples\n");
    for (pid, n) in procs.iter().take(20) {
        out.push_str(&format!("u {} {}\n", pid, n));
    }
    let mut hot: Vec<((u32, u64), u64)> = user_ips.into_iter().collect();
    hot.sort_by(|a, b| b.1.cmp(&a.1));
    for ((tid, ip), n) in hot.iter().take(20) {
        let pid = &crate::task::with_task(*tid, |t| t.leader).filter(|l| *l != 0).unwrap_or(*tid);
        let origin = crate::task::with_task(*pid, |t| t.aspace.as_ref().and_then(|a| a.file_origin(*ip))).flatten();
        let place = origin
            .and_then(|(node, offset)| crate::fs::VFS.try_lock().and_then(|g| g.as_ref().map(|v| format!("{}+{:#x}", v.path_of(node), offset))))
            .unwrap_or_default();
        let code = crate::task::with_task(*pid, |t| {
            let a = t.aspace.as_ref()?;
            let phys = a.translate(*ip & !0xfff)?;
            let at = (phys + (*ip & 0xfff)) as *const u8;
            let len = (0x1000 - (*ip & 0xfff)).min(12) as usize;
            Some((0..len).map(|i| format!("{:02x}", unsafe { *at.add(i) })).collect::<Vec<_>>().join(""))
        })
        .flatten()
        .unwrap_or_default();
        out.push_str(&format!("uip {} {:#x} {} {} {}\n", tid, ip, n, place, code));
    }
    out
}
