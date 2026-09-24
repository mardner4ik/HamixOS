pub fn user_fault(addr: u64) -> bool {
    if !crate::arch::paging::is_user_range(addr, 1) || crate::task::current_pid() == 0 || !crate::task::is_user_process() {
        return false;
    }
    crate::arch::enable_interrupts();
    let had_lock = crate::task::bkl::held();
    if !had_lock {
        crate::task::bkl::acquire();
    }
    let handled = crate::syscall::linux::base::lazy_fault(addr);
    if !had_lock {
        crate::task::bkl::release();
    }
    crate::arch::disable_interrupts();
    handled
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
