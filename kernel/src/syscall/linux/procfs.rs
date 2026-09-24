use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::syscall::LinuxStat;
use crate::arch::paging;
use crate::fs;
use crate::task::{self, DirEntry, OpenFile, Pid, State};

pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
const DT_FIFO: u8 = 1;

const PID_FILES: [&str; 6] = ["stat", "statm", "status", "cmdline", "comm", "maps"];
const PID_LINKS: [&str; 2] = ["exe", "cwd"];
const INO_BASE: u64 = 0x4000_0000;

pub enum Node {
    Dir(Vec<DirEntry>),
    File(Vec<u8>),
    Link(String),
}

pub struct Lookup {
    pub node: Node,
    pub stat: LinuxStat,
}

enum FdKind {
    File(usize),
    Pipe(u32),
    Console,
    Path(String),
}

struct Snapshot {
    pid: Pid,
    parent: Pid,
    name: String,
    state: State,
    uid: u32,
    ruid: u32,
    args: Vec<String>,
    exe: String,
    cwd: String,
    kernel_thread: bool,
    cpu_ticks: u64,
    started: u64,
    brk: (u64, u64),
    fds: Vec<(usize, FdKind)>,
    rss: u64,
    mmap: (u64, u64),
    cpu: usize,
}

fn snapshot(pid: Pid) -> Option<Snapshot> {
    task::with_task(pid, |t| {
        let mut fds = Vec::new();
        for (i, slot) in t.fds.iter().enumerate() {
            let kind = match slot {
                Some(OpenFile::File { node, .. }) => FdKind::File(*node),
                Some(OpenFile::PipeRead(id)) | Some(OpenFile::PipeWrite(id)) => FdKind::Pipe(*id),
                Some(OpenFile::Console) => FdKind::Console,
                Some(OpenFile::Dir { path, .. }) | Some(OpenFile::Mem { path, .. }) => FdKind::Path(path.clone()),
                Some(OpenFile::Socket { id, .. }) => FdKind::Path(format!("socket:[{}]", id)),
                Some(OpenFile::Memfd { name, .. }) => FdKind::Path(format!("/memfd:{} (deleted)", name)),
                Some(OpenFile::Epoll(_)) => FdKind::Path(String::from("anon_inode:[eventpoll]")),
                Some(OpenFile::Mailbox) => FdKind::Path(String::from("anon_inode:[hamix-mailbox]")),
                Some(OpenFile::Special(id)) => FdKind::Path(String::from(super::special::describe(*id))),
                Some(OpenFile::PtyMaster { .. }) => FdKind::Path(String::from("/dev/ptmx")),
                Some(OpenFile::PtySlave { index, .. }) => FdKind::Path(alloc::format!("/dev/pts/{}", index)),
                None if i < 3 && !t.kernel_thread => FdKind::Console,
                None => continue,
            };
            fds.push((i, kind));
        }
        Snapshot {
            pid: t.pid,
            parent: t.parent,
            name: t.name.clone(),
            state: t.state,
            uid: t.uid,
            ruid: t.ruid,
            args: t.args.clone(),
            exe: t.exe.clone(),
            cwd: t.cwd.clone(),
            kernel_thread: t.kernel_thread,
            cpu_ticks: t.cpu_ticks,
            started: t.started,
            brk: (t.brk_start, t.brk),
            fds,
            rss: t.aspace.as_ref().map(|a| paging::resident_pages(a.root)).unwrap_or(0),
            mmap: (paging::USER_MMAP_BASE, t.mmap_next),
            cpu: t.last_cpu,
        }
    })
    .filter(|s| s.pid != 0)
}

fn to_clock_ticks(ticks: u64) -> u64 {
    ticks * 100 / task::TICK_HZ
}

fn state_letter(state: State) -> char {
    match state {
        State::Runnable => 'R',
        State::Blocked => 'S',
        State::Zombie => 'Z',
    }
}

fn fd_target(kind: &FdKind) -> String {
    match kind {
        FdKind::File(node) => fs::VFS.lock().as_ref().map(|v| v.path_of(*node)).unwrap_or_default(),
        FdKind::Pipe(id) => format!("pipe:[{}]", id),
        FdKind::Console => String::from("/dev/console"),
        FdKind::Path(path) => path.clone(),
    }
}

fn file_content(s: &Snapshot, name: &str) -> Vec<u8> {
    let comm = if s.name.len() > 15 { &s.name[..15] } else { s.name.as_str() };
    let rss = s.rss;
    let vsize = rss * paging::PAGE_SIZE;
    let text = match name {
        "stat" => format!(
            "{} ({}) {} {} {} {} 0 -1 {} 0 0 0 0 {} 0 0 0 20 0 {} 0 {} {} {} 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 {} 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
            s.pid,
            comm,
            state_letter(s.state),
            s.parent,
            s.pid,
            s.pid,
            if s.kernel_thread { 0x0020_0040u32 } else { 0x0040_0000 },
            to_clock_ticks(s.cpu_ticks),
            task::group_of(s.pid).len().max(1),
            to_clock_ticks(s.started),
            vsize,
            rss,
            s.cpu
        ),
        "statm" => format!("{} {} 0 0 0 {} 0\n", rss, rss, s.brk.1.saturating_sub(s.brk.0) / paging::PAGE_SIZE),
        "status" => format!(
            "Name:\t{}\nState:\t{} ({})\nTgid:\t{}\nPid:\t{}\nPPid:\t{}\nUid:\t{}\t{}\t{}\t{}\nGid:\t{}\t{}\t{}\t{}\nVmSize:\t{} kB\nVmRSS:\t{} kB\nThreads:\t{}\n",
            comm,
            state_letter(s.state),
            match s.state {
                State::Runnable => "running",
                State::Blocked => "sleeping",
                State::Zombie => "zombie",
            },
            s.pid,
            s.pid,
            s.parent,
            s.ruid,
            s.uid,
            s.uid,
            s.uid,
            s.ruid,
            s.uid,
            s.uid,
            s.uid,
            vsize / 1024,
            vsize / 1024,
            task::group_of(s.pid).len().max(1)
        ),
        "cmdline" => {
            let mut out = Vec::new();
            for arg in &s.args {
                out.extend_from_slice(arg.as_bytes());
                out.push(0);
            }
            return out;
        }
        "comm" => format!("{}\n", comm),
        "maps" if !s.kernel_thread => {
            let mut out = String::new();
            if s.brk.1 > s.brk.0 {
                out.push_str(&format!("{:x}-{:x} rw-p 00000000 00:00 0 [heap]\n", s.brk.0, (s.brk.1 + 0xFFF) & !0xFFF));
            }
            if s.mmap.1 > s.mmap.0 {
                out.push_str(&format!("{:x}-{:x} rw-p 00000000 00:00 0\n", s.mmap.0, s.mmap.1));
            }
            out.push_str(&format!("{:x}-{:x} rw-p 00000000 00:00 0 [stack]\n", paging::USER_STACK_TOP - 0x10_0000, paging::USER_STACK_TOP));
            out
        }
        _ => String::new(),
    };
    text.into_bytes()
}

fn ino(pid: Pid, slot: u64) -> u64 {
    INO_BASE + ((pid as u64) << 12) + slot
}

fn stat(ino: u64, mode: u32, uid: u32, size: u64) -> LinuxStat {
    LinuxStat { dev: 0x50, ino, mode, uid, size, nlink: 0 }
}

fn split(path: &str) -> Option<(Pid, Vec<&str>)> {
    let rest = path.strip_prefix("/proc/")?;
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let first = parts.next()?;
    let pid = if first == "self" { task::current_pid() } else { first.parse::<Pid>().ok()? };
    Some((pid, parts.collect()))
}

fn global_text(name: &str) -> Option<String> {
    use crate::arch::smp;
    use core::sync::atomic::Ordering;
    let hz = |ticks: u64| ticks * 100 / task::TICK_HZ;
    Some(match name {
        "stat" => {
            let mut busy_all = 0u64;
            let mut idle_all = 0u64;
            let mut lines = String::new();
            for cpu in 0..smp::count() {
                let busy = hz(smp::BUSY_TICKS[cpu].load(Ordering::Relaxed));
                let total = hz(smp::TOTAL_TICKS[cpu].load(Ordering::Relaxed));
                let idle = total.saturating_sub(busy);
                busy_all += busy;
                idle_all += idle;
                lines.push_str(&format!("cpu{} {} 0 0 {} 0 0 0 0 0 0\n", cpu, busy, idle));
            }
            let list = task::list();
            let running = list.iter().filter(|t| t.state == State::Runnable).count();
            format!(
                "cpu  {} 0 0 {} 0 0 0 0 0 0\n{}intr 0\nctxt 0\nbtime {}\nprocesses {}\nprocs_running {}\nprocs_blocked 0\nsoftirq 0\n",
                busy_all,
                idle_all,
                lines,
                crate::drivers::rtc::boot_epoch(),
                list.len(),
                running.max(1)
            )
        }
        "meminfo" => {
            let (free, total) = crate::memory::frame::memory_info();
            let cached = fs::file_cache_bytes();
            let kb = |b: usize| b / 1024;
            format!(
                "MemTotal:       {:>8} kB\nMemFree:        {:>8} kB\nMemAvailable:   {:>8} kB\nBuffers:               0 kB\nCached:         {:>8} kB\nSwapCached:            0 kB\nActive:         {:>8} kB\nInactive:              0 kB\nSwapTotal:             0 kB\nSwapFree:              0 kB\nShmem:                 0 kB\nSReclaimable:          0 kB\n",
                kb(total),
                kb(free),
                kb(free + cached),
                kb(cached),
                kb(total - free)
            )
        }
        "uptime" => {
            let ms = task::uptime_ms();
            format!("{}.{:02} 0.00\n", ms / 1000, (ms % 1000) / 10)
        }
        "loadavg" => {
            let list = task::list();
            let running = list.iter().filter(|t| t.state == State::Runnable && !t.kernel_thread).count();
            let last = list.iter().map(|t| t.pid).max().unwrap_or(0);
            format!("0.00 0.00 0.00 {}/{} {}\n", running.max(1), list.len(), last)
        }
        _ => return None,
    })
}

pub fn lookup(path: &str) -> Option<Lookup> {
    if let Some(name) = path.strip_prefix("/proc/") {
        if !name.contains('/') {
            if let Some(text) = global_text(name) {
                let size = text.len() as u64;
                return Some(Lookup { node: Node::File(text.into_bytes()), stat: stat(INO_BASE - 1 - name.len() as u64, 0o100444, 0, size) });
            }
        }
    }
    let (pid, mut parts) = split(path)?;
    if parts.first() == Some(&"task") && parts.len() >= 2 {
        if parts[1].parse::<Pid>().ok() != Some(pid) {
            return None;
        }
        parts.drain(..2);
    }
    let s = snapshot(pid)?;
    let uid = s.uid;
    match parts.as_slice() {
        [] => {
            let mut entries: Vec<DirEntry> = PID_FILES.iter().enumerate().map(|(i, n)| DirEntry { name: n.to_string(), ino: ino(pid, 1 + i as u64), kind: DT_REG }).collect();
            entries.extend(PID_LINKS.iter().enumerate().map(|(i, n)| DirEntry { name: n.to_string(), ino: ino(pid, 16 + i as u64), kind: DT_LNK }));
            entries.push(DirEntry { name: String::from("fd"), ino: ino(pid, 32), kind: DT_DIR });
            entries.push(DirEntry { name: String::from("task"), ino: ino(pid, 33), kind: DT_DIR });
            Some(Lookup { node: Node::Dir(entries), stat: stat(ino(pid, 0), 0o040555, uid, 0) })
        }
        [name] if PID_FILES.contains(name) => {
            let slot = PID_FILES.iter().position(|n| n == name).unwrap() as u64;
            Some(Lookup { node: Node::File(file_content(&s, name)), stat: stat(ino(pid, 1 + slot), 0o100444, uid, 0) })
        }
        ["exe"] if !s.kernel_thread && !s.exe.is_empty() => Some(Lookup { node: Node::Link(s.exe.clone()), stat: stat(ino(pid, 16), 0o120777, uid, s.exe.len() as u64) }),
        ["cwd"] => Some(Lookup { node: Node::Link(s.cwd.clone()), stat: stat(ino(pid, 17), 0o120777, uid, s.cwd.len() as u64) }),
        ["task"] => {
            let threads = task::group_of(pid);
            let entries = threads.iter().map(|t| DirEntry { name: t.to_string(), ino: ino(*t, 0), kind: DT_DIR }).collect::<Vec<_>>();
            let entries = if entries.is_empty() { alloc::vec![DirEntry { name: pid.to_string(), ino: ino(pid, 0), kind: DT_DIR }] } else { entries };
            let mut st = stat(ino(pid, 33), 0o040555, uid, 0);
            st.nlink = 2 + entries.len() as u64;
            Some(Lookup { node: Node::Dir(entries), stat: st })
        }
        ["fd"] => {
            let entries = s.fds.iter().map(|(fd, _)| DirEntry { name: fd.to_string(), ino: ino(pid, 64 + *fd as u64), kind: DT_LNK }).collect();
            Some(Lookup { node: Node::Dir(entries), stat: stat(ino(pid, 32), 0o040500, uid, 0) })
        }
        ["fd", fd] => {
            let fd: usize = fd.parse().ok()?;
            let (_, kind) = s.fds.iter().find(|(n, _)| *n == fd)?;
            let target = fd_target(kind);
            Some(Lookup { node: Node::Link(target.clone()), stat: stat(ino(pid, 64 + fd as u64), 0o120700, uid, target.len() as u64) })
        }
        _ => None,
    }
}

pub fn pid_entries() -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = task::list()
        .into_iter()
        .filter(|t| t.state != State::Zombie)
        .map(|t| DirEntry { name: t.pid.to_string(), ino: ino(t.pid, 0), kind: DT_DIR })
        .collect();
    entries.push(DirEntry { name: String::from("self"), ino: ino(task::current_pid(), 0), kind: DT_LNK });
    entries
}

pub fn dirent_kind(mode: u32) -> u8 {
    match mode & 0o170000 {
        0o040000 => DT_DIR,
        0o020000 => DT_CHR,
        0o010000 => DT_FIFO,
        0o120000 => DT_LNK,
        _ => DT_REG,
    }
}
