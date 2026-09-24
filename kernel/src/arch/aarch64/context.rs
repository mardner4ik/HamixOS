pub const FX_SIZE: usize = 528;
pub const FRAME_SIZE: usize = 320;
const SWITCH_SIZE: u64 = 112;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct InterruptFrame {
    pub x: [u64; 31],
    pub elr: u64,
    pub spsr: u64,
    pub esr: u64,
    pub far: u64,
    pub sp: u64,
    pub kind: u64,
    pub orig_x0: u64,
    pub _pad: [u64; 2],
}

const _: () = assert!(core::mem::size_of::<InterruptFrame>() == FRAME_SIZE);

impl InterruptFrame {
    #[inline]
    pub fn from_user(&self) -> bool {
        self.spsr & 0xF == 0
    }

    #[inline]
    pub fn syscall_number(&self) -> u64 {
        self.x[8]
    }

    #[inline]
    pub fn args(&self) -> [u64; 6] {
        [self.x[0], self.x[1], self.x[2], self.x[3], self.x[4], self.x[5]]
    }

    #[inline]
    pub fn set_arg(&mut self, index: usize, value: u64) {
        self.x[index.min(5)] = value;
    }

    #[inline]
    pub fn ret(&self) -> u64 {
        self.x[0]
    }

    #[inline]
    pub fn set_ret(&mut self, value: u64) {
        self.x[0] = value;
    }

    #[inline]
    pub fn pc(&self) -> u64 {
        self.elr
    }

    #[inline]
    pub fn set_pc(&mut self, value: u64) {
        self.elr = value;
    }

    #[inline]
    pub fn sp(&self) -> u64 {
        self.sp
    }

    #[inline]
    pub fn set_sp(&mut self, value: u64) {
        self.sp = value;
    }

    pub fn restart(&mut self, _nr: u64) {
        self.x[0] = self.orig_x0;
        self.elr -= 4;
    }

    pub fn fresh_user(&self, pc: u64, sp: u64) -> InterruptFrame {
        InterruptFrame::user(pc, sp)
    }

    pub fn user(pc: u64, sp: u64) -> InterruptFrame {
        InterruptFrame { x: [0; 31], elr: pc, spsr: 0, esr: 0, far: 0, sp, kind: 0, orig_x0: 0, _pad: [0; 2] }
    }

    pub fn set_child_tls(&mut self, _value: u64) -> bool {
        false
    }
}

pub fn reset_fx(fx: &mut [u8]) {
    fx.fill(0);
}

core::arch::global_asm!(
    r#"
    .arch armv8-a+fp+simd
    .section .text
    .global hamix_switch_context
hamix_switch_context:
    sub sp, sp, #112
    stp x19, x20, [sp, #0]
    stp x21, x22, [sp, #16]
    stp x23, x24, [sp, #32]
    stp x25, x26, [sp, #48]
    stp x27, x28, [sp, #64]
    stp x29, x30, [sp, #80]
    mrs x9, daif
    str x9, [sp, #96]
    mov x9, sp
    str x9, [x0]
    mov sp, x1
    ldp x19, x20, [sp, #0]
    ldp x21, x22, [sp, #16]
    ldp x23, x24, [sp, #32]
    ldp x25, x26, [sp, #48]
    ldp x27, x28, [sp, #64]
    ldp x29, x30, [sp, #80]
    ldr x9, [sp, #96]
    msr daif, x9
    add sp, sp, #112
    ret

    .global hamix_kernel_thread_trampoline
hamix_kernel_thread_trampoline:
    bl task_entry_kernel
    msr daifclr, #2
    mov x0, x19
    blr x20
    brk #1

    .global hamix_return_trampoline
hamix_return_trampoline:
    msr daifset, #2
    bl task_entry_user
    b hamix_exit_to_user
    "#
);

unsafe extern "C" {
    fn hamix_switch_context(save: *mut u64, next: u64);
    fn hamix_kernel_thread_trampoline();
    fn hamix_return_trampoline();
}

pub unsafe fn switch_context(save: *mut u64, next: u64) {
    unsafe { hamix_switch_context(save, next) }
}

fn switch_frame(at: u64, x19: u64, x20: u64, lr: u64) {
    let words: [u64; 14] = [x19, x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr, 0x3C0, 0];
    unsafe {
        for (i, value) in words.iter().enumerate() {
            *((at + i as u64 * 8) as *mut u64) = *value;
        }
    }
}

pub fn prepare_kernel_stack(top: u64, entry: u64, arg: u64) -> u64 {
    let sp = (top - SWITCH_SIZE) & !0xF;
    switch_frame(sp, arg, entry, hamix_kernel_thread_trampoline as *const () as u64);
    sp
}

pub fn prepare_return_stack(top: u64, frame: &InterruptFrame, fx: &[u8]) -> u64 {
    let frame_addr = (top - FRAME_SIZE as u64) & !0xF;
    let fx_addr = frame_addr - FX_SIZE as u64;
    unsafe {
        core::ptr::copy_nonoverlapping(frame as *const InterruptFrame as *const u8, frame_addr as *mut u8, FRAME_SIZE);
        core::ptr::copy_nonoverlapping(fx.as_ptr(), fx_addr as *mut u8, FX_SIZE.min(fx.len()));
    }
    let sp = fx_addr - SWITCH_SIZE;
    switch_frame(sp, 0, 0, hamix_return_trampoline as *const () as u64);
    sp
}

pub fn prepare_user_stack(top: u64, pc: u64, sp: u64) -> u64 {
    let fx = [0u8; FX_SIZE];
    prepare_return_stack(top, &InterruptFrame::user(pc, sp), &fx)
}

pub fn set_kernel_stack(_top: u64) {}

pub fn load_tls(value: u64) {
    unsafe { core::arch::asm!("msr tpidr_el0, {}", in(reg) value, options(nomem, nostack)) };
}

pub fn save_tls() -> Option<u64> {
    let value: u64;
    unsafe { core::arch::asm!("mrs {}, tpidr_el0", out(reg) value, options(nomem, nostack)) };
    Some(value)
}
