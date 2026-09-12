use core::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use spin::Mutex;

use crate::drivers::video::text_mode::{COLS, ROWS, TEXT_CONSOLE};

pub const VT_COUNT: usize = 6;

const VT_STACK_SIZE: usize = 64 * 1024;

#[repr(C, align(16))]
struct VtStacks([[u8; VT_STACK_SIZE]; VT_COUNT]);

static mut VT_STACKS: VtStacks = VtStacks([[0u8; VT_STACK_SIZE]; VT_COUNT]);

static mut VT_CORO_RSP: [u64; VT_COUNT] = [0; VT_COUNT];

const DEFAULT_ATTR: u8 = crate::drivers::video::text_mode::attr(
    crate::drivers::video::text_mode::LIGHT_GRAY,
    crate::drivers::video::text_mode::BLACK,
);
const BLANK_CELL: u16 = ((DEFAULT_ATTR as u16) << 8) | (b' ' as u16);

#[derive(Clone, Copy)]
struct Screen {
    cells: [u16; COLS * ROWS],
    col: usize,
    row: usize,
    attr: u8,
}

impl Screen {
    const fn blank() -> Self {
        Self {
            cells: [BLANK_CELL; COLS * ROWS],
            col: 0,
            row: 0,
            attr: DEFAULT_ATTR,
        }
    }
}

static VT_SCREEN: Mutex<[Screen; VT_COUNT]> = Mutex::new([Screen::blank(); VT_COUNT]);

static CURRENT_VT: AtomicUsize = AtomicUsize::new(0);
static PENDING_FOREGROUND: AtomicIsize = AtomicIsize::new(-1);
static PENDING_KILL: AtomicBool = AtomicBool::new(false);

struct Ring3Session {
    owner_vt: usize,
}

static RING3: Mutex<Option<Ring3Session>> = Mutex::new(None);

pub fn current() -> usize {
    CURRENT_VT.load(Ordering::SeqCst)
}

pub fn request_switch(target: usize) {
    if target < VT_COUNT {
        PENDING_FOREGROUND.store(target as isize, Ordering::SeqCst);
    }
}

pub fn service_pending_foreground_switch() {
    let p = PENDING_FOREGROUND.load(Ordering::SeqCst);
    if p < 0 {
        return;
    }
    let target = p as usize;
    if target == current() {
        let _ = PENDING_FOREGROUND.compare_exchange(p, -1, Ordering::SeqCst, Ordering::SeqCst);
        return;
    }
    if PENDING_FOREGROUND
        .compare_exchange(p, -1, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let prev = CURRENT_VT.swap(target, Ordering::SeqCst);
        swap_screen(prev, target);
        sched::ensure_started(target);
    }
}

fn swap_screen(prev: usize, target: usize) {
    let mut screens = VT_SCREEN.lock();
    let mut console = TEXT_CONSOLE.lock();
    let mut cells = [0u16; COLS * ROWS];
    let (col, row, attr) = console.snapshot(&mut cells);
    screens[prev] = Screen { cells, col, row, attr };
    let target_screen = screens[target];
    console.restore(&target_screen.cells, target_screen.col, target_screen.row, target_screen.attr);
}

pub fn try_claim_ring3(vt: usize) -> Result<(), usize> {
    let mut guard = RING3.lock();
    match &*guard {
        Some(sess) if sess.owner_vt != vt => Err(sess.owner_vt),
        _ => {
            PENDING_KILL.store(false, Ordering::SeqCst);
            *guard = Some(Ring3Session { owner_vt: vt });
            Ok(())
        }
    }
}

pub fn release_ring3(vt: usize) {
    let mut guard = RING3.lock();
    if matches!(&*guard, Some(sess) if sess.owner_vt == vt) {
        *guard = None;
    }
}

pub fn current_coro_slot_ptr() -> *mut u64 {
    coro_slot_ptr(current())
}

pub(crate) fn coro_slot_ptr(vt: usize) -> *mut u64 {
    unsafe { (&raw mut VT_CORO_RSP).cast::<u64>().add(vt) }
}

fn vt_stack_top(vt: usize) -> u64 {
    unsafe { (&raw const VT_STACKS).cast::<u8>().add(vt * VT_STACK_SIZE) as u64 + VT_STACK_SIZE as u64 }
}

pub fn request_kill() {
    PENDING_KILL.store(true, Ordering::SeqCst);
}

pub fn kill_pending() -> bool {
    PENDING_KILL.swap(false, Ordering::SeqCst)
}

pub fn ring3_owner_or_current() -> usize {
    RING3.lock().as_ref().map(|s| s.owner_vt).unwrap_or_else(current)
}

pub fn terminate_current_ring3() -> ! {
    terminate_current_ring3_with(130)
}

pub fn terminate_current_ring3_with(code: i32) -> ! {
    crate::syscall::release_framebuffer();
    let vt = ring3_owner_or_current();
    release_ring3(vt);
    unsafe { crate::task::usermode::resume_kernel(code, coro_slot_ptr(vt)) }
}

pub fn is_foreground_task() -> bool {
    sched::current_running() == current()
}

pub fn yield_to_next() {
    sched::yield_to_next();
}

mod sched {
    use super::*;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct TrapFrame {
        r15: u64,
        r14: u64,
        r13: u64,
        r12: u64,
        r11: u64,
        r10: u64,
        r9: u64,
        r8: u64,
        rdi: u64,
        rsi: u64,
        rbp: u64,
        rdx: u64,
        rcx: u64,
        rbx: u64,
        rax: u64,
        rip: u64,
        cs: u64,
        rflags: u64,
        rsp: u64,
        ss: u64,
    }

    impl TrapFrame {
        const fn empty() -> Self {
            Self {
                r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
                rdi: 0, rsi: 0, rbp: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
                rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
            }
        }
    }

    static mut TASK_FRAMES: [TrapFrame; VT_COUNT] = [TrapFrame::empty(); VT_COUNT];
    static mut TASK_STARTED: [bool; VT_COUNT] = {
        let mut a = [false; VT_COUNT];
        a[0] = true;
        a
    };
    static CURRENT_RUNNING: AtomicUsize = AtomicUsize::new(0);

    pub fn current_running() -> usize {
        CURRENT_RUNNING.load(Ordering::SeqCst)
    }

    extern "C" fn vt_entry(vt: u64) -> ! {
        crate::hsh::run_login(vt as usize)
    }

    pub fn ensure_started(vt: usize) {
        unsafe {
            if TASK_STARTED[vt] {
                return;
            }
            let top = super::vt_stack_top(vt) & !0xF;
            TASK_FRAMES[vt] = TrapFrame {
                r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
                rdi: vt as u64, rsi: 0, rbp: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
                rip: vt_entry as *const () as u64,
                cs: 0x08,
                rflags: 0x202,
                rsp: top - 8,
                ss: 0x10,
            };
            TASK_STARTED[vt] = true;
        }
    }

    fn next_task(from: usize) -> usize {
        let mut i = from;
        loop {
            i = (i + 1) % VT_COUNT;
            if i == from {
                return from;
            }
            if unsafe { TASK_STARTED[i] } {
                return i;
            }
        }
    }

    pub fn yield_to_next() {
        let cur = CURRENT_RUNNING.load(Ordering::SeqCst);
        let next = next_task(cur);
        if next == cur {
            return;
        }
        ensure_started(next);
        CURRENT_RUNNING.store(next, Ordering::SeqCst);
        crate::syscall::set_current_task(next);
        unsafe {
            cooperative_switch(&raw mut TASK_FRAMES[cur], &raw const TASK_FRAMES[next]);
        }
    }

    #[unsafe(naked)]
    unsafe extern "C" fn cooperative_switch(save_to: *mut TrapFrame, target: *const TrapFrame) {
        core::arch::naked_asm!(
            "mov rax, [rsp]",
            "lea rdx, [rsp + 8]",
            "mov [rdi + 120], rax",
            "mov qword ptr [rdi + 128], 0x08",
            "mov [rdi + 144], rdx",
            "mov qword ptr [rdi + 152], 0x10",
            "pushfq",
            "pop rax",
            "mov [rdi + 136], rax",
            "mov [rdi + 0], r15",
            "mov [rdi + 8], r14",
            "mov [rdi + 16], r13",
            "mov [rdi + 24], r12",
            "mov qword ptr [rdi + 32], 0",
            "mov qword ptr [rdi + 40], 0",
            "mov qword ptr [rdi + 48], 0",
            "mov qword ptr [rdi + 56], 0",
            "mov qword ptr [rdi + 64], 0",
            "mov qword ptr [rdi + 72], 0",
            "mov [rdi + 80], rbp",
            "mov qword ptr [rdi + 88], 0",
            "mov qword ptr [rdi + 96], 0",
            "mov [rdi + 104], rbx",
            "mov qword ptr [rdi + 112], 0",
            "mov rsp, rsi",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rbp",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            "pop rax",
            "iretq",
        );
    }

    #[unsafe(no_mangle)]
    extern "C" fn schedule_tick(frame: *mut TrapFrame) -> *mut TrapFrame {
        crate::arch::x86_64::outb(0x20, 0x20);
        crate::task::tick();

        let ring3 = unsafe { (*frame).cs & 3 == 3 };
        if !ring3 {
            return frame;
        }

        let cur = CURRENT_RUNNING.load(Ordering::SeqCst);
        unsafe { TASK_FRAMES[cur] = *frame };
        let next = next_task(cur);
        if next != cur {
            CURRENT_RUNNING.store(next, Ordering::SeqCst);
            crate::syscall::set_current_task(next);
        }
        unsafe { &raw mut TASK_FRAMES[next] }
    }

    #[unsafe(naked)]
    pub(super) unsafe extern "C" fn timer_handler_naked() {
        core::arch::naked_asm!(
            "push rax",
            "push rbx",
            "push rcx",
            "push rdx",
            "push rbp",
            "push rsi",
            "push rdi",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "mov rdi, rsp",
            "sub rsp, 528",
            "and rsp, -16",
            "fxsave64 [rsp]",
            "mov rbx, rsp",
            "call {sched}",
            "fxrstor64 [rbx]",
            "mov rsp, rax",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rbp",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            "pop rax",
            "iretq",
            sched = sym schedule_tick,
        );
    }
}

pub(crate) fn timer_handler_entry() -> u64 {
    sched::timer_handler_naked as *const () as u64
}
