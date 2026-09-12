use crate::arch::x86_64::gdt::{USER_CS, USER_DS};
use crate::arch::x86_64::paging;

const USER_STACK_SIZE: usize = 4096;
static mut USER_STACK: [u8; USER_STACK_SIZE] = [0u8; USER_STACK_SIZE];

#[unsafe(naked)]
unsafe extern "C" fn ring3_stub() {
    core::arch::naked_asm!(
        "lea rsi, [rip + 2f]",
        "mov rdi, 1",
        "mov rdx, 17",
        "mov rax, 1",
        "syscall",
        "mov rdi, 0",
        "mov rax, 60",
        "syscall",
        "3:",
        "jmp 3b",
        "2:",
        ".ascii \"hello from ring3\\n\"",
    );
}

/// Drops into ring 3 at `entry`/`user_stack_top` and, unlike
/// `enter_user_mode`, actually comes back: when the user process calls
/// exit()/exit_group(), the kernel's syscall handler calls `resume_kernel`
/// instead of `sysretq`, which restores the rsp and callee-saved registers
/// captured here and `ret`s -- so to the caller this looks like an ordinary
/// function call that returns the process's exit code once it's done.
///
/// `save_slot` is a pointer to *this VT's own* parking spot (see
/// `vt::current_coro_slot_ptr`), not a single fixed global -- each virtual
/// terminal that ever execs a ring-3 binary gets its own slot, so VT1's
/// in-flight `hed`/`hxserver` call and VT2's don't stomp on each other's
/// saved rsp when Ctrl+Alt+F<n> switches which one is actually running.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn enter_user_mode_and_return(
    entry: u64,
    user_stack_top: u64,
    save_slot: *mut u64,
) -> i32 {
    core::arch::naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdx], rsp",
        "mov rax, {user_ds}",
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "push {user_ds}",
        "push rsi",
        "push 0x202",
        "push {user_cs}",
        "push rdi",
        "iretq",
        user_ds = const USER_DS as u64,
        user_cs = const USER_CS as u64,
    );
}

/// Called from the SYS_EXIT/SYS_EXIT_GROUP syscall handler (still running
/// on the syscall's own kernel stack, right after the `swapgs` that
/// `syscall_entry` did on the way in). Undoes that swapgs, then throws away
/// the syscall stack entirely and long-jumps back to whatever called
/// `enter_user_mode_and_return`, handing back `exit_code` as if that call
/// had simply returned. `restore_slot` must be the *same* VT slot pointer
/// that was passed to the matching `enter_user_mode_and_return` call (the
/// exiting process's own VT, i.e. `vt::current_coro_slot_ptr()`).
#[unsafe(naked)]
pub unsafe extern "C" fn resume_kernel(exit_code: i32, restore_slot: *mut u64) -> ! {
    core::arch::naked_asm!(
        "swapgs",
        "mov eax, edi",
        "mov rsp, [rsi]",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "ret",
    );
}

pub(crate) unsafe fn enter_user_mode(entry: u64, user_stack_top: u64) -> ! {
    unsafe {
        core::arch::asm!(
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "push {ss}",
            "push {stack}",
            "push 0x202",
            "push {cs}",
            "push {entry}",
            "iretq",
            in("ax") USER_DS,
            ss = const USER_DS as u64,
            cs = const USER_CS as u64,
            stack = in(reg) user_stack_top,
            entry = in(reg) entry,
            options(noreturn),
        );
    }
}

/// Runs the ring-3 smoke test. Does not return: the stub spins in an
/// infinite loop in ring 3 after its exit syscall, since there is no
/// scheduler yet to hand control back to. Only call this from a place
/// that's fine losing control of the machine (e.g. an explicit shell
/// command the user invoked on purpose).
pub fn run_smoke_test() -> ! {
    let entry = ring3_stub as *const () as u64;
    let stack_top = unsafe { (&raw const USER_STACK) as u64 + USER_STACK_SIZE as u64 };

    paging::allow_user_access(entry, 1);
    paging::allow_user_access(stack_top - USER_STACK_SIZE as u64, USER_STACK_SIZE as u64);

    crate::serial_println!(
        "ring3: entering user mode at {:#x}, stack {:#x}",
        entry,
        stack_top
    );

    unsafe { enter_user_mode(entry, stack_top) }
}
