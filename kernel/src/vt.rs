use core::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use crate::drivers::video::text_mode::{self, TEXT_CONSOLE};
use crate::task::{self, Pid};

pub const VT_COUNT: usize = text_mode::VT_COUNT;

static SESSION_PID: [AtomicU32; VT_COUNT] = [const { AtomicU32::new(0) }; VT_COUNT];
static OWNER_PID: [AtomicU32; VT_COUNT] = [const { AtomicU32::new(0) }; VT_COUNT];
static PENDING_SWITCH: AtomicIsize = AtomicIsize::new(-1);
static PENDING_KILL: AtomicIsize = AtomicIsize::new(-1);

pub fn foreground() -> usize {
    text_mode::foreground()
}

pub fn request_switch(target: usize) {
    if target < VT_COUNT {
        PENDING_SWITCH.store(target as isize, Ordering::SeqCst);
        task::notify_input();
    }
}

pub fn request_kill() {
    PENDING_KILL.store(foreground() as isize, Ordering::SeqCst);
    task::notify_input();
}

extern "C" fn login_entry(vt: u64) -> ! {
    crate::login::run_login(vt as usize)
}

pub fn start_shell(vt: usize) {
    if vt >= VT_COUNT || SESSION_PID[vt].load(Ordering::SeqCst) != 0 {
        return;
    }
    let name = alloc::format!("login-tty{}", vt + 1);
    if let Some(pid) = task::spawn_kernel_thread(&name, vt, login_entry, vt as u64) {
        SESSION_PID[vt].store(pid, Ordering::SeqCst);
    }
}

pub fn has_pending() -> bool {
    PENDING_SWITCH.load(Ordering::Relaxed) >= 0 || PENDING_KILL.load(Ordering::Relaxed) >= 0
}

pub fn service_pending() {
    let target = PENDING_SWITCH.swap(-1, Ordering::SeqCst);
    if target >= 0 {
        let target = target as usize;
        if target != foreground() {
            let was_graphics = text_mode::graphics_owned();
            let graphics = crate::task::display::on_vt_switch(target);
            if was_graphics && !graphics {
                text_mode::fb_clear_full();
            }
            if let Some(mut console) = TEXT_CONSOLE.try_lock() {
                if graphics {
                    text_mode::set_graphics_owned(true);
                }
                console.switch_to(target);
            } else {
                PENDING_SWITCH.store(target as isize, Ordering::SeqCst);
                return;
            }
            if !graphics {
                crate::drivers::video::console_mouse::enable();
            } else {
                crate::drivers::video::console_mouse::disable();
            }
            start_shell(target);
        }
    }

    let kill_vt = PENDING_KILL.swap(-1, Ordering::SeqCst);
    if kill_vt >= 0 {
        let vt = kill_vt as usize;
        let owner = OWNER_PID[vt].load(Ordering::SeqCst);
        if owner != 0 && crate::task::display::owner() != Some(owner) {
            let parent = task::with_task(owner, |t| t.parent).unwrap_or(0);
            if parent != SESSION_PID[vt].load(Ordering::SeqCst) {
                let linux = task::with_task(owner, |t| (t.abi == task::Abi::Linux, crate::syscall::linux::tty::signals_enabled(&t.termios)));
                match linux {
                    Some((true, true)) => {
                        crate::syscall::linux::signal::send(owner, crate::syscall::linux::signal::SIGINT);
                    }
                    Some((true, false)) => {}
                    _ => {
                        task::kill(owner, 130);
                    }
                }
            }
        }
    }
}

pub fn input_allowed(pid: Pid, vt: usize) -> bool {
    if vt >= VT_COUNT || vt != foreground() {
        return false;
    }
    let owner = OWNER_PID[vt].load(Ordering::Relaxed);
    if owner != 0 {
        return pid == owner;
    }
    pid == SESSION_PID[vt].load(Ordering::Relaxed)
}

pub fn set_input_owner(vt: usize, pid: Pid) {
    if vt < VT_COUNT {
        OWNER_PID[vt].store(pid, Ordering::SeqCst);
    }
}

pub fn input_owner(vt: usize) -> Pid {
    OWNER_PID[vt.min(VT_COUNT - 1)].load(Ordering::SeqCst)
}

pub fn on_process_exit(pid: Pid) {
    let parent = task::with_task(pid, |t| t.parent).unwrap_or(0);
    for (vt, owner) in OWNER_PID.iter().enumerate() {
        let fallback = if parent != 0 && parent != SESSION_PID[vt].load(Ordering::SeqCst) && task::exists(parent) { parent } else { 0 };
        let _ = owner.compare_exchange(pid, fallback, Ordering::SeqCst, Ordering::SeqCst);
    }
}
