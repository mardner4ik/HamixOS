use crate::arch::x86_64::gdt::{USER_CS, USER_DS};

#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(save: *mut u64, next: u64) {
    core::arch::naked_asm!(
        "pushfq",
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",
        "mov rsp, rsi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "popfq",
        "ret",
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn kernel_thread_trampoline() {
    core::arch::naked_asm!(
        "and rsp, -16",
        "call {entry}",
        "mov rdi, rbx",
        "sti",
        "call r12",
        "ud2",
        entry = sym crate::task::task_entry_kernel,
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn user_trampoline() {
    core::arch::naked_asm!(
        "cli",
        "and rsp, -16",
        "call {entry}",
        "fninit",
        "push 0x1F80",
        "ldmxcsr [rsp]",
        "add rsp, 8",
        "mov ax, {user_ds}",
        "mov ds, ax",
        "mov es, ax",
        "push {user_ds}",
        "push r12",
        "push 0x202",
        "push {user_cs}",
        "push rbx",
        "xor eax, eax",
        "xor ebx, ebx",
        "xor ecx, ecx",
        "xor edx, edx",
        "xor esi, esi",
        "xor edi, edi",
        "xor ebp, ebp",
        "xor r8d, r8d",
        "xor r9d, r9d",
        "xor r10d, r10d",
        "xor r11d, r11d",
        "xor r12d, r12d",
        "xor r13d, r13d",
        "xor r14d, r14d",
        "xor r15d, r15d",
        "iretq",
        user_ds = const USER_DS as u64,
        user_cs = const USER_CS as u64,
        entry = sym crate::task::task_entry_user,
    );
}

#[repr(C)]
pub struct InterruptFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[unsafe(naked)]
pub unsafe extern "C" fn timer_entry() {
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
        "mov rbp, rsp",
        "mov rdi, rsp",
        "sub rsp, 512",
        "and rsp, -16",
        "fxsave64 [rsp]",
        "mov rsi, rsp",
        "call {handler}",
        "fxrstor64 [rsp]",
        "mov rsp, rbp",
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
        handler = sym crate::task::timer_interrupt,
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn lapic_timer_entry() {
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
        "mov rbp, rsp",
        "mov rdi, rsp",
        "sub rsp, 512",
        "and rsp, -16",
        "fxsave64 [rsp]",
        "mov rsi, rsp",
        "call {handler}",
        "fxrstor64 [rsp]",
        "mov rsp, rbp",
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
        handler = sym crate::task::lapic_timer_interrupt,
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn return_trampoline() {
    core::arch::naked_asm!(
        "cli",
        "call {entry}",
        "fxrstor64 [rsp]",
        "add rsp, 512",
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
        entry = sym crate::task::task_entry_user,
    );
}

pub const FX_SIZE: usize = 512;

impl InterruptFrame {
    #[inline]
    pub fn from_user(&self) -> bool {
        self.cs & 3 == 3
    }

    #[inline]
    pub fn syscall_number(&self) -> u64 {
        self.rax
    }

    #[inline]
    pub fn args(&self) -> [u64; 6] {
        [self.rdi, self.rsi, self.rdx, self.r10, self.r8, self.r9]
    }

    #[inline]
    pub fn set_arg(&mut self, index: usize, value: u64) {
        match index {
            0 => self.rdi = value,
            1 => self.rsi = value,
            2 => self.rdx = value,
            3 => self.r10 = value,
            4 => self.r8 = value,
            _ => self.r9 = value,
        }
    }

    #[inline]
    pub fn ret(&self) -> u64 {
        self.rax
    }

    #[inline]
    pub fn set_ret(&mut self, value: u64) {
        self.rax = value;
    }

    #[inline]
    pub fn pc(&self) -> u64 {
        self.rip
    }

    #[inline]
    pub fn set_pc(&mut self, value: u64) {
        self.rip = value;
    }

    #[inline]
    pub fn sp(&self) -> u64 {
        self.rsp
    }

    #[inline]
    pub fn set_sp(&mut self, value: u64) {
        self.rsp = value;
    }

    pub fn fresh_user(&self, pc: u64, sp: u64) -> InterruptFrame {
        InterruptFrame {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rdi: 0,
            rsi: 0,
            rbp: 0,
            rdx: 0,
            rcx: 0,
            rbx: 0,
            rax: 0,
            rip: pc,
            cs: self.cs,
            rflags: 0x202,
            rsp: sp,
            ss: self.ss,
        }
    }

    pub fn set_child_tls(&mut self, _value: u64) -> bool {
        false
    }

    pub fn restart(&mut self, nr: u64) {
        self.rip -= 2;
        self.rax = nr;
    }
}

pub fn reset_fx(fx: &mut [u8]) {
    fx.fill(0);
    fx[0..2].copy_from_slice(&0x037Fu16.to_le_bytes());
    fx[24..28].copy_from_slice(&0x1F80u32.to_le_bytes());
}

fn build_stack(top: u64, trampoline: u64, rbx: u64, r12: u64) -> u64 {
    let frame: [u64; 8] = [0, 0, 0, r12, rbx, 0, 0x2, trampoline];
    let rsp = top - 8 - frame.len() as u64 * 8;
    unsafe {
        for (i, value) in frame.iter().enumerate() {
            *((rsp + i as u64 * 8) as *mut u64) = *value;
        }
    }
    rsp
}

pub fn prepare_kernel_stack(top: u64, entry: u64, arg: u64) -> u64 {
    build_stack(top, kernel_thread_trampoline as *const () as u64, arg, entry)
}

pub fn prepare_user_stack(top: u64, pc: u64, sp: u64) -> u64 {
    build_stack(top, user_trampoline as *const () as u64, pc, sp)
}

pub fn prepare_return_stack(top: u64, frame: &InterruptFrame, fx: &[u8]) -> u64 {
    let frame_size = core::mem::size_of::<InterruptFrame>() as u64;
    let frame_addr = (top - frame_size) & !0xF;
    let fx_addr = frame_addr - FX_SIZE as u64;
    unsafe {
        core::ptr::copy_nonoverlapping(frame as *const InterruptFrame as *const u8, frame_addr as *mut u8, frame_size as usize);
        core::ptr::copy_nonoverlapping(fx.as_ptr(), fx_addr as *mut u8, FX_SIZE);
    }
    let words: [u64; 8] = [0, 0, 0, 0, 0, 0, 0x2, return_trampoline as *const () as u64];
    let rsp = fx_addr - words.len() as u64 * 8;
    unsafe {
        for (i, value) in words.iter().enumerate() {
            *((rsp + i as u64 * 8) as *mut u64) = *value;
        }
    }
    rsp
}

pub fn set_kernel_stack(top: u64) {
    super::gdt::set_kernel_stack(top);
    super::smp::set_syscall_stack(top);
}

const IA32_FS_BASE: u32 = 0xC000_0100;

pub fn load_tls(value: u64) {
    super::write_msr(IA32_FS_BASE, value);
}

pub fn save_tls() -> Option<u64> {
    None
}
