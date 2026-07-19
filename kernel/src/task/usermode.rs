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
