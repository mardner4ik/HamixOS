use core::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use crate::drivers::video::text_mode::{self, TEXT_CONSOLE};
use crate::task::{self, Pid};

pub const VT_COUNT: usize = text_mode::VT_COUNT;

static SESSION_PID: [AtomicU32; VT_COUNT] = [const { AtomicU32::new(0) }; VT_COUNT];
static OWNER_PID: [AtomicU32; VT_COUNT] = [const { AtomicU32::new(0) }; VT_COUNT];
static PENDING_SWITCH: AtomicIsize = AtomicIsize::new(-1);
static PENDING_KILL: AtomicIsize = AtomicIsize::new(-1);
static FG_PGRP: [AtomicU32; VT_COUNT] = [const { AtomicU32::new(0) }; VT_COUNT];

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
    if task::with_task(task::INIT_PID, |t| !t.kernel_thread && t.state != task::State::Zombie).unwrap_or(false) {
        let name = alloc::format!("tty{}", vt + 1);
        crate::net::netlink::uevent("change", &alloc::format!("/devices/virtual/tty/{}", name), &[("SUBSYSTEM", "tty"), ("DEVNAME", &name), ("HAMIX_LOGIN", "1")]);
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
                crate::drivers::video::drm::refresh();
            }
            start_shell(target);
        }
    }

    let kill_vt = PENDING_KILL.swap(-1, Ordering::SeqCst);
    if kill_vt >= 0 && interrupt_foreground_group(kill_vt as usize) {
        return;
    }
    if kill_vt >= 0 {
        let vt = kill_vt as usize;
        let owner = OWNER_PID[vt].load(Ordering::SeqCst);
        if owner != 0 && crate::task::display::owner() != Some(owner) {
            let parent = task::with_task(owner, |t| t.parent).unwrap_or(0);
            if parent != SESSION_PID[vt].load(Ordering::SeqCst) {
                let linux = task::with_task(owner, |t| (t.abi.posix(), crate::tty::signals_enabled(&t.termios)));
                match linux {
                    Some((true, true)) => {
                        crate::syscall::signal::send(owner, crate::syscall::signal::SIGINT);
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

pub fn set_foreground_group(vt: usize, pgid: Pid) {
    if vt < VT_COUNT {
        FG_PGRP[vt].store(pgid, Ordering::SeqCst);
        task::notify_input();
    }
}

pub fn foreground_group(vt: usize) -> Option<Pid> {
    if vt >= VT_COUNT {
        return None;
    }
    let group = FG_PGRP[vt].load(Ordering::SeqCst);
    if group == 0 {
        return None;
    }
    let alive = task::with_tasks(|tasks| tasks.values().any(|t| t.pgid == group && !t.kernel_thread && t.state != task::State::Zombie));
    alive.then_some(group)
}

pub fn leads_foreground_group(pid: Pid, vt: usize) -> bool {
    vt < VT_COUNT && shell_owns(vt) && in_foreground_group(pid, vt)
}

fn shell_owns(vt: usize) -> bool {
    let owner = OWNER_PID[vt].load(Ordering::SeqCst);
    if owner == 0 || owner == SESSION_PID[vt].load(Ordering::SeqCst) {
        return true;
    }
    task::with_task(owner, |t| t.abi.posix() || t.state == task::State::Zombie).unwrap_or(true)
}

fn in_foreground_group(pid: Pid, vt: usize) -> bool {
    let group = FG_PGRP[vt].load(Ordering::Relaxed);
    group != 0 && task::with_task(pid, |t| t.pgid == group && t.vt == vt).unwrap_or(false)
}

pub fn input_allowed(pid: Pid, vt: usize) -> bool {
    if vt >= VT_COUNT || vt != foreground() {
        return false;
    }
    if in_foreground_group(pid, vt) && shell_owns(vt) {
        return true;
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

pub fn attach_session(vt: usize, pid: Pid) -> bool {
    if vt >= VT_COUNT {
        return false;
    }
    let current = SESSION_PID[vt].load(Ordering::SeqCst);
    if current != 0 && current != pid && task::exists(current) {
        return false;
    }
    SESSION_PID[vt].store(pid, Ordering::SeqCst);
    OWNER_PID[vt].store(pid, Ordering::SeqCst);
    FG_PGRP[vt].store(0, Ordering::SeqCst);
    true
}

fn interrupt_foreground_group(vt: usize) -> bool {
    if !shell_owns(vt) || text_mode::graphics_owned() {
        return false;
    }
    let Some(group) = foreground_group(vt) else {
        return false;
    };
    let session = SESSION_PID[vt].load(Ordering::SeqCst);
    let members: alloc::vec::Vec<(Pid, bool, bool)> = task::with_tasks(|tasks| {
        tasks
            .values()
            .filter(|t| t.pgid == group && t.vt == vt && !t.kernel_thread && t.state != task::State::Zombie)
            .map(|t| (t.pid, t.abi.posix(), crate::tty::signals_enabled(&t.termios)))
            .collect()
    });
    if members.is_empty() || members.iter().any(|(pid, _, _)| *pid == session) || crate::task::display::owner().map(|o| members.iter().any(|m| m.0 == o)).unwrap_or(false) {
        return false;
    }
    for (pid, posix, isig) in members {
        match (posix, isig) {
            (true, true) => {
                crate::syscall::signal::send(pid, crate::syscall::signal::SIGINT);
            }
            (true, false) => {}
            _ => {
                task::kill(pid, 130);
            }
        }
    }
    true
}

pub fn on_process_exit(pid: Pid) {
    for session in SESSION_PID.iter() {
        let _ = session.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
    let parent = task::with_task(pid, |t| t.parent).unwrap_or(0);
    for (vt, owner) in OWNER_PID.iter().enumerate() {
        let fallback = if parent != 0 && parent != SESSION_PID[vt].load(Ordering::SeqCst) && task::exists(parent) { parent } else { 0 };
        let _ = owner.compare_exchange(pid, fallback, Ordering::SeqCst, Ordering::SeqCst);
    }
}

static KD_MODE: [core::sync::atomic::AtomicU8; VT_COUNT] = [const { core::sync::atomic::AtomicU8::new(0) }; VT_COUNT];
static KB_MODE: [core::sync::atomic::AtomicU8; VT_COUNT] = [const { core::sync::atomic::AtomicU8::new(3) }; VT_COUNT];
static VT_MODE: [core::sync::atomic::AtomicU64; VT_COUNT] = [const { core::sync::atomic::AtomicU64::new(0) }; VT_COUNT];

const VT_OPENQRY: u64 = 0x5600;
const VT_GETMODE: u64 = 0x5601;
const VT_SETMODE: u64 = 0x5602;
const VT_GETSTATE: u64 = 0x5603;
const VT_RELDISP: u64 = 0x5605;
const VT_ACTIVATE: u64 = 0x5606;
const VT_WAITACTIVE: u64 = 0x5607;
const VT_DISALLOCATE: u64 = 0x5608;
const KDGETLED: u64 = 0x4B31;
const KDSETLED: u64 = 0x4B32;
const KDGKBTYPE: u64 = 0x4B33;
const KDSETMODE: u64 = 0x4B3A;
const KDGETMODE: u64 = 0x4B3B;
const KDGKBMODE: u64 = 0x4B44;
const KDSKBMODE: u64 = 0x4B45;
const KB_101: u8 = 2;
const KD_GRAPHICS: u8 = 1;

pub fn mark_graphics(vt: usize) {
    if vt < VT_COUNT {
        KD_MODE[vt].store(KD_GRAPHICS, Ordering::Relaxed);
    }
}

pub fn mark_text(vt: usize) {
    if vt < VT_COUNT {
        KD_MODE[vt].store(0, Ordering::Relaxed);
    }
}

pub fn in_use(vt: usize) -> bool {
    vt < VT_COUNT && (SESSION_PID[vt].load(Ordering::Relaxed) != 0 || OWNER_PID[vt].load(Ordering::Relaxed) != 0 || KD_MODE[vt].load(Ordering::Relaxed) == KD_GRAPHICS)
}


fn put_int(arg: u64, value: i32) -> i64 {
    crate::usercopy::copy_out(arg, 4, &value.to_le_bytes()).min(0)
}

pub fn ioctl(vt: usize, request: u64, arg: u64) -> Option<i64> {
    let vt = vt.min(VT_COUNT - 1);
    Some(match request {
        VT_OPENQRY => match (0..VT_COUNT).find(|v| !in_use(*v) && *v != foreground()) {
            Some(free) => put_int(arg, free as i32 + 1),
            None => put_int(arg, -1),
        },
        VT_GETMODE => crate::usercopy::copy_out(arg, 8, &VT_MODE[vt].load(Ordering::Relaxed).to_le_bytes()).min(0),
        VT_SETMODE => match crate::usercopy::user_slice(arg, 8) {
            Ok(raw) => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(raw);
                VT_MODE[vt].store(u64::from_le_bytes(bytes), Ordering::Relaxed);
                0
            }
            Err(e) => e,
        },
        VT_GETSTATE => {
            let mut state: u16 = 1;
            for v in 0..VT_COUNT {
                if in_use(v) || v == foreground() {
                    state |= 1 << (v + 1);
                }
            }
            let mut raw = [0u8; 6];
            raw[0..2].copy_from_slice(&(foreground() as u16 + 1).to_le_bytes());
            raw[4..6].copy_from_slice(&state.to_le_bytes());
            crate::usercopy::copy_out(arg, 6, &raw).min(0)
        }
        VT_RELDISP | VT_DISALLOCATE | KDSETLED => 0,
        VT_ACTIVATE => {
            if arg == 0 || arg as usize > VT_COUNT {
                return Some(crate::errno::ENXIO);
            }
            request_switch(arg as usize - 1);
            service_pending();
            0
        }
        VT_WAITACTIVE => {
            if arg == 0 || arg as usize > VT_COUNT {
                return Some(crate::errno::ENXIO);
            }
            let wanted = arg as usize - 1;
            while foreground() != wanted {
                if crate::syscall::signal::pending() {
                    return Some(crate::errno::EINTR);
                }
                task::sleep_ticks((task::TICK_HZ / 50).max(1));
                service_pending();
            }
            0
        }
        KDGETLED => crate::usercopy::copy_out(arg, 1, &[0u8]).min(0),
        KDGKBTYPE => crate::usercopy::copy_out(arg, 1, &[KB_101]).min(0),
        KDGETMODE => put_int(arg, KD_MODE[vt].load(Ordering::Relaxed) as i32),
        KDSETMODE => {
            KD_MODE[vt].store((arg as u8).min(KD_GRAPHICS), Ordering::Relaxed);
            if arg as u8 == KD_GRAPHICS {
                task::with_current(|t| t.vt = vt);
            }
            0
        }
        KDGKBMODE => put_int(arg, KB_MODE[vt].load(Ordering::Relaxed) as i32),
        KDSKBMODE => {
            KB_MODE[vt].store(arg as u8, Ordering::Relaxed);
            0
        }
        _ => return None,
    })
}
