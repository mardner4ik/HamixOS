use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::arch::context;
use crate::arch::paging::{self, AddressSpace};
use crate::arch::smp::{self, cpu_id, MAX_CPUS};
use crate::arch::without_interrupts;

pub mod bkl;
pub mod display;
pub mod elf;
pub mod fault;
pub mod ipc;
pub mod linux_loader;
pub mod pipe;
pub mod pty;
pub mod registry;
pub mod switch;

pub type Pid = u32;

pub const TICK_HZ: u64 = 1000;
const KSTACK_SIZE: usize = 64 * 1024;

pub const LEVELS: usize = 4;
const SLICE: [u64; LEVELS] = [6, 12, 24, 48];
const BOOST_INTERVAL: u64 = TICK_HZ;

pub const WAIT_INPUT: u32 = 1;
pub const WAIT_MSG: u32 = 2;
pub const WAIT_CHILD: u32 = 4;
pub const WAIT_TIMER: u32 = 8;
pub const WAIT_PIPE: u32 = 16;
pub const WAIT_SIGNAL: u32 = 32;
pub const WAIT_FUTEX: u32 = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Abi {
    Native,
    Linux,
}

impl Abi {
    pub fn name(self) -> &'static str {
        match self {
            Abi::Native => "native",
            Abi::Linux => "linux",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Runnable,
    Blocked,
    Zombie,
}

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: u64,
    pub kind: u8,
}

pub enum OpenFile {
    File { node: usize, pos: usize, append: bool, written: bool, access: u8 },
    PipeRead(u32),
    PipeWrite(u32),
    Console,
    Dir { path: String, entries: Arc<Vec<DirEntry>>, pos: usize },
    Mem { path: String, data: Arc<Vec<u8>>, pos: usize },
    Socket { id: u32, nonblock: bool },
    Memfd { id: u32, pos: usize, name: String, access: u8 },
    Epoll(u32),
    Mailbox,
    Special(u32),
    PtyMaster { index: u32, input: u32, output: u32 },
    PtySlave { index: u32, input: u32, output: u32 },
}

impl OpenFile {
    pub fn duplicate(&self) -> OpenFile {
        match self {
            OpenFile::File { node, pos, append, access, .. } => OpenFile::File { node: *node, pos: *pos, append: *append, written: false, access: *access },
            OpenFile::PipeRead(id) => {
                pipe::retain(*id, true);
                OpenFile::PipeRead(*id)
            }
            OpenFile::PipeWrite(id) => {
                pipe::retain(*id, false);
                OpenFile::PipeWrite(*id)
            }
            OpenFile::Console => OpenFile::Console,
            OpenFile::Dir { path, entries, pos } => OpenFile::Dir { path: path.clone(), entries: entries.clone(), pos: *pos },
            OpenFile::Mem { path, data, pos } => OpenFile::Mem { path: path.clone(), data: data.clone(), pos: *pos },
            OpenFile::Socket { id, nonblock } => {
                if crate::net::inet::is_inet(*id) {
                    crate::net::inet::retain(*id);
                } else {
                    crate::net::unix::retain(*id);
                }
                OpenFile::Socket { id: *id, nonblock: *nonblock }
            }
            OpenFile::Memfd { id, pos, name, access } => {
                ipc::retain(*id);
                OpenFile::Memfd { id: *id, pos: *pos, name: name.clone(), access: *access }
            }
            OpenFile::Epoll(id) => {
                crate::syscall::linux::epoll::retain(*id);
                OpenFile::Epoll(*id)
            }
            OpenFile::Mailbox => OpenFile::Mailbox,
            OpenFile::Special(id) => {
                crate::syscall::linux::special::retain(*id);
                OpenFile::Special(*id)
            }
            OpenFile::PtyMaster { index, input, output } => {
                pipe::retain(*input, false);
                pipe::retain(*output, true);
                OpenFile::PtyMaster { index: *index, input: *input, output: *output }
            }
            OpenFile::PtySlave { index, input, output } => {
                pipe::retain(*input, true);
                pipe::retain(*output, false);
                OpenFile::PtySlave { index: *index, input: *input, output: *output }
            }
        }
    }

    pub fn release(self) -> bool {
        match self {
            OpenFile::File { written, .. } => written,
            OpenFile::PipeRead(id) => {
                pipe::release(id, true);
                false
            }
            OpenFile::PipeWrite(id) => {
                pipe::release(id, false);
                false
            }
            OpenFile::Console | OpenFile::Dir { .. } | OpenFile::Mem { .. } | OpenFile::Mailbox => false,
            OpenFile::Special(id) => {
                crate::syscall::linux::special::release(id);
                false
            }
            OpenFile::PtyMaster { index, input, output } => {
                pipe::release(input, false);
                pipe::release(output, true);
                if !crate::syscall::pty_master_open(index) {
                    pty::master_closed(index);
                }
                false
            }
            OpenFile::PtySlave { input, output, .. } => {
                pipe::release(input, true);
                pipe::release(output, false);
                false
            }
            OpenFile::Socket { id, .. } if crate::net::inet::is_inet(id) => {
                crate::net::inet::release(id);
                false
            }
            OpenFile::Socket { id, .. } => {
                crate::net::unix::release(id);
                false
            }
            OpenFile::Memfd { id, .. } => {
                ipc::release(id);
                false
            }
            OpenFile::Epoll(id) => {
                crate::syscall::linux::epoll::release(id);
                false
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct PosixTimer {
    pub id: u32,
    pub signo: u32,
    pub thread: Pid,
    pub next: u64,
    pub interval: u64,
    pub overrun: u32,
}

#[derive(Clone, Copy, Default)]
pub struct SigAction {
    pub handler: u64,
    pub flags: u64,
    pub restorer: u64,
    pub mask: u64,
}

pub struct Message {
    pub sender: Pid,
    pub data: Vec<u8>,
}

pub struct Task {
    pub pid: Pid,
    pub parent: Pid,
    pub name: String,
    pub vt: usize,
    pub uid: u32,
    pub ruid: u32,
    pub root_ticket_until: u64,
    pub cwd: String,
    pub state: State,
    pub exit_code: i32,
    pub kernel_thread: bool,
    pub abi: Abi,
    pub exe: String,
    pub env: Vec<String>,
    pub args: Vec<String>,
    pub umask: u32,
    pub started: u64,
    pub termios: [u8; crate::syscall::linux::tty::TERMIOS_LEN],
    pub tty_pending: VecDeque<u8>,
    pub tty_line: VecDeque<u8>,
    pub fs_base: u64,
    pub leader: Pid,
    pub clear_child_tid: u64,
    pub frame: u64,
    pub frame_fx: u64,
    pub sig_actions: [SigAction; 65],
    pub sig_mask: u64,
    pub sig_pending: u64,
    pub altstack: (u64, u64, u32),
    pub alarm_at: u64,
    pub alarm_interval: u64,
    pub exit_signal: u8,
    pub pgid: Pid,
    pub saved_mask: Option<u64>,
    pub futex: u64,
    pub comm: Option<String>,
    pub cloexec: [u64; 4],
    pub nonblock: [u64; 4],
    pub timers: Vec<PosixTimer>,
    pub vfork_parent: Pid,
    kstack: *mut u8,
    rsp: u64,
    pub aspace: Option<AddressSpace>,
    wait: u32,
    wake_at: u64,
    input_seq: u64,
    wake_pending: bool,
    pub killed: Option<i32>,
    pub mailbox: VecDeque<Message>,
    pub brk_start: u64,
    pub brk: u64,
    pub mmap_next: u64,
    pub mmap_free: Vec<(u64, u64)>,
    pub shm_next: u64,
    pub shm: Vec<(u32, u64, u64)>,
    pub fds: Vec<Option<OpenFile>>,
    pub cpu_ticks: u64,
    pub seen_input: u64,
    on_cpu: Option<usize>,
    pub last_cpu: usize,
    pub pinned: Option<usize>,
    level: u8,
    queued: bool,
}

unsafe impl Send for Task {}

impl Task {
    fn kstack_top(&self) -> u64 {
        if self.kstack.is_null() {
            return 0;
        }
        (self.kstack as u64 + KSTACK_SIZE as u64) & !0xF
    }

    pub fn priority(&self) -> u8 {
        self.level
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if let Some(mut aspace) = self.aspace.take() {
            aspace.destroy();
        }
        if !self.kstack.is_null() {
            unsafe { alloc::alloc::dealloc(self.kstack, Layout::from_size_align_unchecked(KSTACK_SIZE, 16)) };
        }
    }
}

struct Sched {
    tasks: BTreeMap<Pid, alloc::boxed::Box<Task>>,
    ready: [VecDeque<Pid>; LEVELS],
    mask: u32,
    scanned_gen: u64,
    boosted_at: u64,
}

impl Sched {
    const fn new() -> Sched {
        Sched { tasks: BTreeMap::new(), ready: [const { VecDeque::new() }; LEVELS], mask: 0, scanned_gen: 0, boosted_at: 0 }
    }

    fn enqueue(&mut self, pid: Pid) {
        if pid == 0 {
            return;
        }
        let Some(task) = self.tasks.get_mut(&pid) else {
            return;
        };
        if task.queued || task.on_cpu.is_some() || task.state != State::Runnable {
            return;
        }
        task.queued = true;
        let level = task.level as usize;
        self.ready[level].push_back(pid);
        self.mask |= 1 << level;
        SCHED_GEN.fetch_add(1, Ordering::Relaxed);
    }

    fn make_runnable(&mut self, pid: Pid) {
        if let Some(task) = self.tasks.get_mut(&pid) {
            if task.state == State::Zombie {
                return;
            }
            task.state = State::Runnable;
            task.wait = 0;
            task.wake_pending = false;
        }
        self.enqueue(pid);
    }

    fn rescan_blocked(&mut self) {
        let now = ticks();
        let seq = input_seq();
        let mut woken: Vec<Pid> = Vec::new();
        let mut next_timer = u64::MAX;
        for (&pid, task) in self.tasks.iter() {
            if task.state != State::Blocked {
                continue;
            }
            let ready = task.wake_pending
                || task.killed.is_some()
                || (task.wait & WAIT_TIMER != 0 && now >= task.wake_at)
                || (task.wait & WAIT_INPUT != 0 && seq != task.input_seq);
            if ready {
                woken.push(pid);
            } else if task.wait & WAIT_TIMER != 0 {
                next_timer = next_timer.min(task.wake_at);
            }
        }
        for pid in woken {
            self.make_runnable(pid);
        }
        NEXT_TIMER.store(next_timer, Ordering::Relaxed);
    }

    fn boost(&mut self) {
        let now = ticks();
        if now.saturating_sub(self.boosted_at) < BOOST_INTERVAL {
            return;
        }
        self.boosted_at = now;
        for level in 1..LEVELS {
            while let Some(pid) = self.ready[level].pop_front() {
                if let Some(task) = self.tasks.get_mut(&pid) {
                    task.level = 0;
                }
                self.ready[0].push_back(pid);
            }
            self.mask &= !(1 << level);
        }
        if !self.ready[0].is_empty() {
            self.mask |= 1;
        }
    }

    fn pick(&mut self, cpu: usize) -> Pid {
        if self.scanned_gen != SCHED_GEN.load(Ordering::Relaxed) || ticks() >= NEXT_TIMER.load(Ordering::Relaxed) {
            self.rescan_blocked();
            self.scanned_gen = SCHED_GEN.load(Ordering::Relaxed);
        }
        self.boost();
        let mut deferred: Vec<Pid> = Vec::new();
        let mut chosen = 0;
        'outer: for level in 0..LEVELS {
            if self.mask & (1 << level) == 0 {
                continue;
            }
            while let Some(pid) = self.ready[level].pop_front() {
                let Some(task) = self.tasks.get_mut(&pid) else {
                    continue;
                };
                if task.state != State::Runnable || task.on_cpu.is_some() {
                    task.queued = false;
                    continue;
                }
                if task.pinned.map(|c| c != cpu).unwrap_or(false) {
                    deferred.push(pid);
                    continue;
                }
                task.queued = false;
                chosen = pid;
                break 'outer;
            }
            self.mask &= !(1 << level);
        }
        for pid in deferred {
            if let Some(task) = self.tasks.get_mut(&pid) {
                let level = task.level as usize;
                self.ready[level].push_front(pid);
                self.mask |= 1 << level;
            }
        }
        chosen
    }
}

static SCHED: Mutex<Sched> = Mutex::new(Sched::new());
static CURRENT_PID: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];
static CURRENT_VT: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static PREVIOUS_PID: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(0) }; MAX_CPUS];
static IDLE_RSP: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static SLICE_START: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static SLICE_LEN: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(SLICE[0]) }; MAX_CPUS];
static ACCOUNTED: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static NEED_RESCHED: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
static NEXT_PID: AtomicU32 = AtomicU32::new(1);
static SCHED_GEN: AtomicU64 = AtomicU64::new(1);
static NEXT_TIMER: AtomicU64 = AtomicU64::new(0);
static TICKS: AtomicU64 = AtomicU64::new(0);
static INPUT_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn notify_runnable() {
    SCHED_GEN.fetch_add(1, Ordering::Relaxed);
    smp::kick_idle(cpu_id());
}

pub fn init() {
    crate::arch::start_tick(TICK_HZ);

    let mut idle = new_task("idle", 0, 0, 0, "/", true);
    idle.pid = 0;
    idle.leader = 0;
    idle.fds = Vec::new();
    idle.mmap_next = 0;
    idle.shm_next = 0;
    without_interrupts(|| SCHED.lock().tasks.insert(0, alloc::boxed::Box::new(idle)));
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn uptime_ticks() -> u64 {
    ticks()
}

pub fn uptime_ms() -> u64 {
    ticks() * 1000 / TICK_HZ
}

pub fn ms_to_ticks(ms: u64) -> u64 {
    (ms * TICK_HZ).div_ceil(1000).max(1)
}

pub fn input_seq() -> u64 {
    INPUT_SEQ.load(Ordering::Relaxed)
}

pub fn notify_input() {
    INPUT_SEQ.fetch_add(1, Ordering::Relaxed);
    SCHED_GEN.fetch_add(1, Ordering::Relaxed);
}

pub fn current_tid() -> Pid {
    CURRENT_PID[cpu_id()].load(Ordering::Relaxed)
}

pub fn current_pid() -> Pid {
    let tid = current_tid();
    if tid == 0 {
        return 0;
    }
    with_tasks(|tasks| tasks.get(&tid).map(|t| t.leader).unwrap_or(tid))
}

pub fn current_vt() -> usize {
    CURRENT_VT[cpu_id()].load(Ordering::Relaxed)
}

pub fn cpu_is_idle(cpu: usize) -> bool {
    CURRENT_PID[cpu].load(Ordering::Relaxed) == 0
}

pub fn with_tasks<R>(f: impl FnOnce(&mut BTreeMap<Pid, alloc::boxed::Box<Task>>) -> R) -> R {
    without_interrupts(|| f(&mut SCHED.lock().tasks))
}

pub fn open_file_nodes() -> alloc::collections::BTreeSet<usize> {
    with_tasks(|tasks| {
        let mut nodes = alloc::collections::BTreeSet::new();
        for task in tasks.values() {
            for file in task.fds.iter().flatten() {
                if let OpenFile::File { node, .. } = file {
                    nodes.insert(*node);
                }
            }
            if let Some(aspace) = task.aspace.as_ref() {
                for region in aspace.lazy.iter() {
                    nodes.insert(region.node);
                }
            }
        }
        nodes
    })
}

pub fn with_current<R>(f: impl FnOnce(&mut Task) -> R) -> R {
    let tid = current_tid();
    with_tasks(|tasks| {
        let leader = tasks.get(&tid).map(|t| t.leader).unwrap_or(tid);
        let key = if leader == tid || !tasks.contains_key(&leader) { tid } else { leader };
        f(tasks.get_mut(&key).expect("current task missing"))
    })
}

pub fn with_current_thread<R>(f: impl FnOnce(&mut Task) -> R) -> R {
    let tid = current_tid();
    with_tasks(|tasks| f(tasks.get_mut(&tid).expect("current task missing")))
}

pub fn group_of(pid: Pid) -> Vec<Pid> {
    with_tasks(|tasks| tasks.values().filter(|t| t.leader == pid && t.state != State::Zombie).map(|t| t.pid).collect())
}

pub fn with_task<R>(pid: Pid, f: impl FnOnce(&mut Task) -> R) -> Option<R> {
    with_tasks(|tasks| tasks.get_mut(&pid).map(|t| f(t)))
}

pub fn is_user_process() -> bool {
    with_current(|t| !t.kernel_thread)
}

fn alloc_kstack() -> *mut u8 {
    unsafe { alloc::alloc::alloc(Layout::from_size_align_unchecked(KSTACK_SIZE, 16)) }
}

fn stack_top(kstack: *mut u8) -> u64 {
    (kstack as u64 + KSTACK_SIZE as u64) & !0xF
}

fn new_task(name: &str, parent: Pid, vt: usize, uid: u32, cwd: &str, kernel_thread: bool) -> Task {
    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let mut task = new_task_with_pid(pid, name, parent, vt, uid, cwd, kernel_thread);
    task.leader = pid;
    task.pgid = pid;
    task
}

fn new_task_with_pid(pid: Pid, name: &str, parent: Pid, vt: usize, uid: u32, cwd: &str, kernel_thread: bool) -> Task {
    Task {
        pid,
        parent,
        name: String::from(name),
        vt,
        uid,
        ruid: uid,
        root_ticket_until: 0,
        cwd: String::from(cwd),
        state: State::Runnable,
        exit_code: 0,
        kernel_thread,
        abi: Abi::Native,
        exe: String::new(),
        env: Vec::new(),
        args: Vec::new(),
        umask: 0o022,
        started: ticks(),
        termios: crate::syscall::linux::tty::DEFAULT_TERMIOS,
        tty_pending: VecDeque::new(),
        tty_line: VecDeque::new(),
        fs_base: 0,
        leader: 0,
        clear_child_tid: 0,
        frame: 0,
        frame_fx: 0,
        sig_actions: [SigAction::default(); 65],
        sig_mask: 0,
        sig_pending: 0,
        altstack: (0, 0, 2),
        alarm_at: 0,
        alarm_interval: 0,
        exit_signal: 0,
        pgid: 0,
        saved_mask: None,
        futex: 0,
        comm: None,
        cloexec: [0; 4],
        nonblock: [0; 4],
        timers: Vec::new(),
        vfork_parent: 0,
        kstack: core::ptr::null_mut(),
        rsp: 0,
        aspace: None,
        wait: 0,
        wake_at: 0,
        input_seq: 0,
        wake_pending: false,
        killed: None,
        mailbox: VecDeque::new(),
        brk_start: 0,
        brk: 0,
        mmap_next: paging::USER_MMAP_BASE,
        mmap_free: Vec::new(),
        shm_next: paging::USER_SHM_BASE,
        shm: Vec::new(),
        fds: (0..32).map(|_| None).collect(),
        cpu_ticks: 0,
        seen_input: 0,
        on_cpu: None,
        last_cpu: 0,
        pinned: None,
        level: 0,
        queued: false,
    }
}

fn admit(task: Task) -> Pid {
    let pid = task.pid;
    without_interrupts(|| {
        let mut sched = SCHED.lock();
        sched.tasks.insert(pid, alloc::boxed::Box::new(task));
        sched.enqueue(pid);
    });
    notify_runnable();
    pid
}

pub fn spawn_kernel_thread(name: &str, vt: usize, entry: extern "C" fn(u64) -> !, arg: u64) -> Option<Pid> {
    spawn_kernel_thread_on(name, vt, entry, arg, None)
}

pub fn spawn_kernel_thread_on(name: &str, vt: usize, entry: extern "C" fn(u64) -> !, arg: u64, cpu: Option<usize>) -> Option<Pid> {
    let kstack = alloc_kstack();
    if kstack.is_null() {
        return None;
    }
    let mut task = new_task(name, 0, vt, 0, "/", true);
    task.pinned = cpu;
    task.kstack = kstack;
    task.rsp = context::prepare_kernel_stack(stack_top(kstack), entry as *const () as u64, arg);
    Some(admit(task))
}

pub struct UserImage {
    pub aspace: AddressSpace,
    pub entry: u64,
    pub stack: u64,
    pub brk: u64,
    pub abi: Abi,
    pub exe: String,
    pub env: Vec<String>,
    pub args: Vec<String>,
}

pub fn clone_task(thread: bool, aspace: Option<AddressSpace>, frame: &switch::InterruptFrame, fx: &[u8], configure: impl FnOnce(&Task, &Task, &mut Task)) -> Option<Pid> {
    let kstack = alloc_kstack();
    if kstack.is_null() {
        if let Some(mut a) = aspace {
            a.destroy();
        }
        return None;
    }
    let tid = current_tid();
    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let mut child = new_task_with_pid(pid, "", 0, 0, 0, "/", false);
    let leader_pid = with_tasks(|tasks| {
        let current = tasks.get(&tid)?;
        let leader_pid = current.leader;
        let leader = tasks.get(&leader_pid).unwrap_or(current);
        configure(leader, current, &mut child);
        Some(leader_pid)
    });
    let Some(leader_pid) = leader_pid else {
        unsafe { alloc::alloc::dealloc(kstack, Layout::from_size_align_unchecked(KSTACK_SIZE, 16)) };
        if let Some(mut a) = aspace {
            a.destroy();
        }
        return None;
    };
    child.pid = pid;
    child.leader = if thread { leader_pid } else { pid };
    if thread {
        let cpu = cpu_id();
        child.pinned = Some(cpu);
        with_tasks(|tasks| {
            for t in tasks.values_mut() {
                if t.leader == leader_pid {
                    t.pinned = Some(cpu);
                }
            }
        });
    }
    child.kstack = kstack;
    child.rsp = context::prepare_return_stack(stack_top(kstack), frame, fx);
    child.aspace = aspace;
    child.started = ticks();
    Some(admit(child))
}

pub fn peek_next_pid() -> Pid {
    NEXT_PID.load(Ordering::Relaxed)
}

pub fn replace_aspace(new: AddressSpace) -> Option<AddressSpace> {
    let root = new.root;
    let old = with_current(|t| t.aspace.replace(new));
    without_interrupts(|| paging::load_root(root));
    old
}

pub fn spawn_user(name: &str, image: UserImage, parent: Pid, vt: usize, uid: u32, ruid: u32, cwd: &str) -> Option<Pid> {
    let kstack = alloc_kstack();
    if kstack.is_null() {
        let mut aspace = image.aspace;
        aspace.destroy();
        return None;
    }
    let mut task = new_task(name, parent, vt, uid, cwd, false);
    task.ruid = ruid;
    let inherited: Vec<Option<OpenFile>> = with_tasks(|tasks| match tasks.get(&parent) {
        Some(p) if !p.kernel_thread => (0..3).map(|i| p.fds.get(i).and_then(|f| f.as_ref()).map(|f| f.duplicate())).collect(),
        _ => Vec::new(),
    });
    for (i, file) in inherited.into_iter().enumerate() {
        task.fds[i] = file;
    }
    for slot in task.fds.iter_mut().take(3) {
        if slot.is_none() {
            *slot = Some(OpenFile::Console);
        }
    }
    task.kstack = kstack;
    task.rsp = context::prepare_user_stack(stack_top(kstack), image.entry, image.stack);
    task.aspace = Some(image.aspace);
    task.brk_start = image.brk;
    task.brk = image.brk;
    task.abi = image.abi;
    task.exe = image.exe;
    task.env = image.env;
    task.args = image.args;
    Some(admit(task))
}

pub fn finish_switch() {
    let cpu = cpu_id();
    let previous = PREVIOUS_PID[cpu].swap(0, Ordering::Relaxed);
    if previous != 0 {
        let mut sched = SCHED.lock();
        let still_ready = match sched.tasks.get_mut(&previous) {
            Some(task) if task.on_cpu == Some(cpu) => {
                task.on_cpu = None;
                task.state == State::Runnable
            }
            _ => false,
        };
        if still_ready {
            sched.enqueue(previous);
            drop(sched);
            smp::kick_idle(cpu);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn task_entry_kernel() {
    finish_switch();
    bkl::acquire();
}

#[unsafe(no_mangle)]
pub extern "C" fn task_entry_user() {
    finish_switch();
}

fn flush_accounting(sched: &mut Sched, cpu: usize, pid: Pid) {
    let used = ACCOUNTED[cpu].swap(0, Ordering::Relaxed);
    if used == 0 || pid == 0 {
        return;
    }
    if let Some(task) = sched.tasks.get_mut(&pid) {
        task.cpu_ticks += used;
    }
}

pub fn schedule() {
    let was_enabled = crate::arch::interrupts_enabled();
    crate::arch::disable_interrupts();
    let cpu = cpu_id();
    NEED_RESCHED[cpu].store(false, Ordering::Relaxed);

    let switch = {
        let mut sched = SCHED.lock();
        let current = CURRENT_PID[cpu].load(Ordering::Relaxed);
        flush_accounting(&mut sched, cpu, current);
        if current != 0 {
            let slice = SLICE_LEN[cpu].load(Ordering::Relaxed);
            let used = ticks().saturating_sub(SLICE_START[cpu].load(Ordering::Relaxed));
            if let Some(task) = sched.tasks.get_mut(&current) {
                if task.state == State::Runnable && used >= slice {
                    task.level = (task.level + 1).min(LEVELS as u8 - 1);
                } else if task.state == State::Blocked && used < slice {
                    task.level = task.level.saturating_sub(1);
                }
            }
        }
        let next = sched.pick(cpu);
        if next == current {
            if next != 0 {
                SLICE_START[cpu].store(ticks(), Ordering::Relaxed);
            }
            None
        } else {
            let (next_rsp, next_top, next_cr3, next_vt, next_slice, next_fs) = {
                let next_task = sched.tasks.get_mut(&next).unwrap();
                if next != 0 {
                    next_task.on_cpu = Some(cpu);
                    next_task.last_cpu = cpu;
                }
                let rsp = if next == 0 { IDLE_RSP[cpu].load(Ordering::Relaxed) } else { next_task.rsp };
                let fs = if next_task.kernel_thread { None } else { Some(next_task.fs_base) };
                let own_cr3 = next_task.aspace.as_ref().map(|a| a.root);
                let leader = next_task.leader;
                let result = (rsp, next_task.kstack_top(), own_cr3, next_task.vt, SLICE[next_task.level as usize], fs);
                let cr3 = result.2.or_else(|| if leader != next { sched.tasks.get(&leader).and_then(|l| l.aspace.as_ref().map(|a| a.root)) } else { None }).unwrap_or_else(paging::kernel_root);
                (result.0, result.1, cr3, result.3, result.4, result.5)
            };
            let save = if current == 0 {
                IDLE_RSP[cpu].as_ptr()
            } else {
                let outgoing = sched.tasks.get_mut(&current).unwrap();
                if !outgoing.kernel_thread {
                    if let Some(tls) = context::save_tls() {
                        outgoing.fs_base = tls;
                    }
                }
                &mut outgoing.rsp as *mut u64
            };
            PREVIOUS_PID[cpu].store(current, Ordering::Relaxed);
            CURRENT_PID[cpu].store(next, Ordering::Relaxed);
            CURRENT_VT[cpu].store(next_vt, Ordering::Relaxed);
            SLICE_START[cpu].store(ticks(), Ordering::Relaxed);
            SLICE_LEN[cpu].store(next_slice, Ordering::Relaxed);
            if next_top != 0 {
                context::set_kernel_stack(next_top);
            }
            if let Some(fs) = next_fs {
                context::load_tls(fs);
            }
            Some((save, next_rsp, next_cr3))
        }
    };

    if let Some((save, next_rsp, next_cr3)) = switch {
        let had_lock = bkl::held();
        if had_lock {
            bkl::release();
        }
        if paging::read_root() != next_cr3 {
            paging::load_root(next_cr3);
        }
        unsafe { context::switch_context(save, next_rsp) };
        finish_switch();
        if had_lock {
            bkl::acquire();
        }
    }

    if was_enabled {
        crate::arch::enable_interrupts();
    }
}

pub fn set_fs_base(value: u64) {
    context::load_tls(value);
}

pub fn yield_now() {
    schedule();
}

pub fn block(flags: u32, timeout_ticks: Option<u64>, seq: u64) {
    without_interrupts(|| {
        let mut sched = SCHED.lock();
        let pid = current_tid();
        if let Some(task) = sched.tasks.get_mut(&pid) {
            task.state = State::Blocked;
            task.wait = flags;
            task.input_seq = seq;
            task.wake_pending = task.sig_pending & !task.sig_mask != 0;
            if let Some(t) = timeout_ticks {
                task.wait |= WAIT_TIMER;
                task.wake_at = ticks() + t;
                NEXT_TIMER.fetch_min(task.wake_at, Ordering::Relaxed);
            }
        }
    });
    schedule();
}

pub fn sleep_ticks(t: u64) {
    block(0, Some(t), input_seq());
}

pub fn wake(pid: Pid, flag: u32) {
    let woke = without_interrupts(|| {
        let mut sched = SCHED.lock();
        let targets: Vec<Pid> = sched
            .tasks
            .values_mut()
            .filter(|t| (t.pid == pid || t.leader == pid) && t.state == State::Blocked && t.wait & flag != 0)
            .map(|t| {
                t.wake_pending = true;
                t.pid
            })
            .collect();
        for tid in targets.iter() {
            sched.make_runnable(*tid);
        }
        !targets.is_empty()
    });
    if woke {
        notify_runnable();
    }
}

pub fn interrupt(tid: Pid) {
    let woke = without_interrupts(|| {
        let mut sched = SCHED.lock();
        let ready = match sched.tasks.get_mut(&tid) {
            Some(task) if task.state == State::Blocked => {
                task.wake_pending = true;
                true
            }
            _ => false,
        };
        if ready {
            sched.make_runnable(tid);
        }
        ready
    });
    if woke {
        for flag in NEED_RESCHED.iter() {
            flag.store(true, Ordering::Relaxed);
        }
        notify_runnable();
    }
}

pub fn wake_futex(leader: Pid, addr: u64, count: usize) -> usize {
    let woken = without_interrupts(|| {
        let mut sched = SCHED.lock();
        let targets: Vec<Pid> = sched
            .tasks
            .values_mut()
            .filter(|t| t.leader == leader && t.futex == addr && t.state != State::Zombie)
            .take(count)
            .map(|t| {
                t.futex = 0;
                t.wake_pending = true;
                t.pid
            })
            .collect();
        for tid in targets.iter() {
            if sched.tasks.get(tid).map(|t| t.state == State::Blocked).unwrap_or(false) {
                sched.make_runnable(*tid);
            }
        }
        targets.len()
    });
    if woken > 0 {
        notify_runnable();
    }
    woken
}

pub fn wake_all(flag: u32) {
    let woke = without_interrupts(|| {
        let mut sched = SCHED.lock();
        let pids: Vec<Pid> = sched.tasks.iter().filter(|(_, t)| t.state == State::Blocked && t.wait & flag != 0).map(|(pid, _)| *pid).collect();
        let any = !pids.is_empty();
        for pid in pids {
            sched.make_runnable(pid);
        }
        any
    });
    if woke {
        notify_runnable();
    }
}

pub fn kill(pid: Pid, code: i32) -> bool {
    let found = without_interrupts(|| {
        let mut sched = SCHED.lock();
        let leader = match sched.tasks.get(&pid) {
            Some(task) if !task.kernel_thread && task.state != State::Zombie => task.leader,
            _ => return false,
        };
        let members: Vec<Pid> = sched.tasks.values().filter(|t| (t.leader == leader || t.pid == pid) && t.state != State::Zombie).map(|t| t.pid).collect();
        for member in members {
            if let Some(task) = sched.tasks.get_mut(&member) {
                if task.killed.is_none() {
                    task.killed = Some(code);
                }
            }
            sched.make_runnable(member);
        }
        true
    });
    if found {
        for flag in NEED_RESCHED.iter() {
            flag.store(true, Ordering::Relaxed);
        }
        notify_runnable();
    }
    found
}

pub fn check_killed() {
    let pid = current_tid();
    if pid == 0 {
        return;
    }
    let killed = with_tasks(|tasks| tasks.get(&pid).and_then(|t| if t.kernel_thread { None } else { t.killed }));
    if let Some(code) = killed {
        exit_current(code);
    }
}

pub fn release_vfork() {
    let parent = with_current(|t| core::mem::replace(&mut t.vfork_parent, 0));
    if parent != 0 {
        wake(parent, WAIT_CHILD);
    }
}

pub fn exit_thread() -> ! {
    let tid = current_tid();
    crate::arch::disable_interrupts();
    paging::load_root(paging::kernel_root());
    {
        let mut sched = SCHED.lock();
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.state = State::Zombie;
            task.queued = false;
            task.parent = 0;
        }
    }
    loop {
        schedule();
    }
}

pub fn stop_other_threads(pid: Pid) {
    loop {
        let others: Vec<Pid> = with_tasks(|tasks| {
            tasks
                .values_mut()
                .filter(|t| t.leader == pid && t.pid != pid && (t.state != State::Zombie || t.on_cpu.is_some()))
                .map(|t| {
                    if t.killed.is_none() {
                        t.killed = Some(0);
                    }
                    t.pid
                })
                .collect()
        });
        if others.is_empty() {
            return;
        }
        without_interrupts(|| {
            let mut sched = SCHED.lock();
            for tid in others.iter() {
                sched.make_runnable(*tid);
            }
        });
        for flag in NEED_RESCHED.iter() {
            flag.store(true, Ordering::Relaxed);
        }
        notify_runnable();
        sleep_ticks(1);
    }
}

pub fn exit_current(code: i32) -> ! {
    let tid = current_tid();
    let pid = current_pid();
    if tid != pid {
        exit_thread();
    }
    stop_other_threads(pid);
    release_vfork();
    display::release(pid);
    ipc::cleanup(pid);
    crate::net::cleanup(pid);
    crate::drivers::audio::cleanup(pid);
    crate::vt::on_process_exit(pid);
    crate::syscall::close_all_files();

    let aspace = with_tasks(|tasks| {
        let mut orphans = Vec::new();
        for (&child_pid, task) in tasks.iter_mut() {
            if task.parent == pid {
                task.parent = 0;
                orphans.push(child_pid);
            }
        }
        let task = tasks.get_mut(&pid).unwrap();
        task.exit_code = code;
        task.aspace.take()
    });

    crate::arch::disable_interrupts();
    paging::load_root(paging::kernel_root());
    if let Some(mut aspace) = aspace {
        aspace.destroy();
    }

    let parent = {
        let mut sched = SCHED.lock();
        let task = sched.tasks.get_mut(&pid).unwrap();
        task.state = State::Zombie;
        task.queued = false;
        task.parent
    };
    crate::debug_println!("task: pid {} exited with code {}", pid, code);
    wake(parent, WAIT_CHILD);
    crate::syscall::notify_child_exit(parent, pid);
    loop {
        schedule();
    }
}

pub enum WaitResult {
    Exited(i32),
    Running,
    NoChild,
}

pub fn try_reap(parent: Pid, child: Pid) -> WaitResult {
    with_tasks(|tasks| {
        let (state, running) = match tasks.get(&child) {
            Some(task) if task.parent == parent => (task.state, task.on_cpu.is_some()),
            _ => return WaitResult::NoChild,
        };
        if state == State::Zombie && !running {
            let task = tasks.remove(&child).unwrap();
            WaitResult::Exited(task.exit_code)
        } else {
            WaitResult::Running
        }
    })
}

pub enum ReapResult {
    Exited(Pid, i32, u8),
    Running,
    NoChild,
}

pub fn reap_matching(parent: Pid, matches: impl Fn(&Task) -> bool, consume: bool) -> ReapResult {
    with_tasks(|tasks| {
        let mut any = false;
        let mut found = None;
        for (pid, task) in tasks.iter() {
            if task.parent != parent || task.leader != task.pid || task.kernel_thread || !matches(task) {
                continue;
            }
            any = true;
            if task.state == State::Zombie && task.on_cpu.is_none() {
                found = Some(*pid);
                break;
            }
        }
        match found {
            Some(pid) => {
                let (code, signal) = {
                    let t = tasks.get(&pid).unwrap();
                    (t.exit_code, t.exit_signal)
                };
                if consume {
                    tasks.remove(&pid);
                }
                ReapResult::Exited(pid, code, signal)
            }
            None if any => ReapResult::Running,
            None => ReapResult::NoChild,
        }
    })
}

pub fn wait_child(child: Pid) -> Option<i32> {
    let parent = current_pid();
    loop {
        match try_reap(parent, child) {
            WaitResult::Exited(code) => return Some(code),
            WaitResult::NoChild => return None,
            WaitResult::Running => {
                crate::vt::service_pending();
                check_killed();
                block(WAIT_CHILD, Some(TICK_HZ / 10), input_seq());
            }
        }
    }
}

pub fn detach(pid: Pid) {
    with_task(pid, |t| t.parent = 0);
}

fn reap_orphans() {
    let current = current_tid();
    with_tasks(|tasks| {
        let dead: Vec<Pid> = tasks
            .iter()
            .filter(|(pid, t)| t.state == State::Zombie && t.parent == 0 && **pid != current && t.on_cpu.is_none())
            .map(|(pid, _)| *pid)
            .collect();
        for pid in dead {
            tasks.remove(&pid);
        }
    });
}

fn idle_maintenance(cpu: usize) {
    if bkl::try_acquire() {
        if cpu == 0 {
            crate::vt::service_pending();
            static LAST_TRIM: AtomicU64 = AtomicU64::new(0);
            let now = ticks();
            if now.saturating_sub(LAST_TRIM.load(Ordering::Relaxed)) >= TICK_HZ {
                LAST_TRIM.store(now, Ordering::Relaxed);
                crate::memory::heap_trim();
            }
        }
        reap_orphans();
        bkl::release();
    }
}

const IDLE_SCAN_TICKS: u64 = TICK_HZ / 50;

pub fn idle_loop() -> ! {
    let cpu = cpu_id();
    let mut seen = 0u64;
    let mut last_scan = 0u64;
    loop {
        let generation = SCHED_GEN.load(Ordering::Relaxed);
        let now = ticks();
        if generation != seen || now >= NEXT_TIMER.load(Ordering::Relaxed) || now.saturating_sub(last_scan) >= IDLE_SCAN_TICKS {
            seen = generation;
            last_scan = now;
            idle_maintenance(cpu);
            schedule();
        }
        crate::arch::idle_wait();
    }
}

pub fn ap_idle_loop(cpu: usize) -> ! {
    let _ = cpu;
    idle_loop()
}

pub struct TaskInfo {
    pub cpu: Option<usize>,
    pub pid: Pid,
    pub parent: Pid,
    pub name: String,
    pub state: State,
    pub vt: usize,
    pub kernel_thread: bool,
    pub memory: u64,
    pub cpu_ticks: u64,
    pub level: u8,
    pub abi: Abi,
}

pub fn list() -> Vec<TaskInfo> {
    with_tasks(|tasks| {
        tasks
            .values()
            .filter(|t| t.pid != 0 && t.leader == t.pid)
            .map(|t| TaskInfo {
                cpu: t.on_cpu,
                pid: t.pid,
                parent: t.parent,
                name: t.name.clone(),
                state: t.state,
                vt: t.vt,
                kernel_thread: t.kernel_thread,
                memory: t.aspace.as_ref().map(|a| paging::resident_pages(a.root) * paging::PAGE_SIZE).unwrap_or(0) + if t.kernel_thread { 0 } else { KSTACK_SIZE as u64 },
                cpu_ticks: t.cpu_ticks,
                level: t.level,
                abi: t.abi,
            })
            .collect()
    })
}

pub fn exists(pid: Pid) -> bool {
    with_tasks(|tasks| tasks.get(&pid).map(|t| t.state != State::Zombie).unwrap_or(false))
}

fn account_tick(cpu: usize) {
    let pid = CURRENT_PID[cpu].load(Ordering::Relaxed);
    smp::record_tick(cpu, pid != 0);
    crate::memory::vmalloc::tick(cpu);
    if pid != 0 {
        ACCOUNTED[cpu].fetch_add(1, Ordering::Relaxed);
    }
}

fn slice_expired(cpu: usize) -> bool {
    ticks().saturating_sub(SLICE_START[cpu].load(Ordering::Relaxed)) >= SLICE_LEN[cpu].load(Ordering::Relaxed)
}

fn preempt_user(cpu: usize) {
    let due = slice_expired(cpu) || NEED_RESCHED[cpu].load(Ordering::Relaxed) || ticks() >= NEXT_TIMER.load(Ordering::Relaxed);
    if !due && !crate::vt::has_pending() {
        return;
    }
    if !bkl::try_acquire() {
        NEED_RESCHED[cpu].store(true, Ordering::Relaxed);
        return;
    }
    let pid = CURRENT_PID[cpu].load(Ordering::Relaxed);
    let killed = with_tasks(|tasks| tasks.get(&pid).and_then(|t| t.killed));
    if let Some(code) = killed {
        exit_current(code);
    }
    crate::vt::service_pending();
    if due {
        schedule();
    }
    bkl::release();
}

pub fn on_tick(cpu: usize, primary: bool, frame: &mut switch::InterruptFrame, fx: *mut u8) {
    if primary {
        let now = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        if now % (TICK_HZ / 50) == 0 {
            crate::drivers::usb::poll();
        }
        crate::drivers::virtio::input::poll();
    }
    account_tick(cpu);
    if frame.from_user() {
        preempt_user(cpu);
        crate::syscall::deliver_from_trap(frame, fx);
    }
}

#[cfg(target_arch = "x86_64")]
pub extern "C" fn timer_interrupt(frame: *mut switch::InterruptFrame, fx: *mut u8) {
    crate::arch::x86_64::outb(0x20, 0x20);
    on_tick(0, true, unsafe { &mut *frame }, fx);
}

#[cfg(target_arch = "x86_64")]
pub extern "C" fn lapic_timer_interrupt(frame: *mut switch::InterruptFrame, fx: *mut u8) {
    crate::arch::x86_64::lapic::eoi();
    let cpu = cpu_id();
    on_tick(cpu, false, unsafe { &mut *frame }, fx);
}
