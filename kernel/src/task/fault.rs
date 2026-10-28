pub fn user_fault(addr: u64) -> bool {
    if !crate::arch::paging::is_user_range(addr, 1) || crate::task::current_pid() == 0 || !crate::task::is_user_process() {
        return false;
    }
    crate::arch::enable_interrupts();
    let had_lock = crate::task::bkl::held();
    if !had_lock {
        crate::task::bkl::acquire();
    }
    let handled = crate::syscall::base::lazy_fault(addr);
    if !handled {
        let page = addr & !0xfff;
        let lazy = crate::task::with_current(|t| t.aspace.as_ref().map(|a| a.lazy_at(page).is_some()).unwrap_or(false));
        let (free, total) = crate::memory::frame::memory_info();
        crate::debug_println!("fault: {:#x} {} ({} of {} KiB free)", addr, if lazy { "is reserved but could not be filled" } else { "is not mapped" }, free / 1024, total / 1024);
    }
    if !had_lock {
        crate::task::bkl::release();
    }
    crate::arch::disable_interrupts();
    handled
}

pub fn resolve_cow_locked(addr: u64, interruptible: bool) -> bool {
    let root = crate::arch::paging::pte::read_root();
    if !interruptible || crate::task::bkl::held() || crate::task::current_pid() == 0 {
        return crate::memory::aspace::resolve_cow(root, addr);
    }
    crate::arch::enable_interrupts();
    crate::task::bkl::acquire();
    let resolved = crate::memory::aspace::resolve_cow(root, addr);
    crate::task::bkl::release();
    crate::arch::disable_interrupts();
    resolved
}

pub fn kernel_touch(addr: u64, interruptible: bool) -> bool {
    if !interruptible {
        return false;
    }
    if user_fault(addr) {
        return true;
    }
    if crate::arch::paging::is_user_range(addr, 1) && crate::task::current_pid() != 0 && crate::task::is_user_process() {
        kill_faulting_process("bad user pointer");
    }
    false
}

pub fn kill_faulting_process(name: &str) -> ! {
    crate::task::bkl::acquire_from_trap();
    let pid = crate::task::current_pid();
    let message = alloc::format!("\n\x1b[91msegmentation fault: pid {} ({})\x1b[0m\n", pid, name);
    crate::syscall::emit(2, message.as_bytes());
    crate::task::with_task(pid, |t| t.exit_signal = 11);
    crate::task::kill(pid, 139);
    crate::task::exit_current(139);
}

pub fn describe(addr: u64) -> Option<alloc::string::String> {
    let (node, offset) = crate::task::with_current(|t| t.aspace.as_ref().and_then(|a| a.file_origin(addr)))?;
    let path = crate::fs::VFS.try_lock().and_then(|g| g.as_ref().map(|v| v.path_of(node)))?;
    Some(alloc::format!("{:#x} = {}+{:#x}", addr, path, offset))
}
