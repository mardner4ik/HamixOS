pub const FX_SIZE: usize = 272;
pub const FRAME_SIZE: usize = 320;
const SWITCH_SIZE: u64 = 112;

const SSTATUS_SIE: u64 = 1 << 1;
const SSTATUS_SPIE: u64 = 1 << 5;
const SSTATUS_SPP: u64 = 1 << 8;
const SSTATUS_FS: u64 = 3 << 13;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct InterruptFrame {
    pub x: [u64; 32],
    pub sepc: u64,
    pub sstatus: u64,
    pub scause: u64,
    pub stval: u64,
    pub orig_a0: u64,
    pub kind: u64,
    pub _pad: [u64; 2],
}

const _: () = assert!(core::mem::size_of::<InterruptFrame>() == FRAME_SIZE);

fn read_sstatus() -> u64 {
    let value: u64;
    unsafe { core::arch::asm!("csrr {}, sstatus", out(reg) value, options(nomem, nostack)) };
    value
}

impl InterruptFrame {
    #[inline]
    pub fn from_user(&self) -> bool {
        self.kind == 1
    }

    #[inline]
    pub fn syscall_number(&self) -> u64 {
        self.x[17]
    }

    #[inline]
    pub fn args(&self) -> [u64; 6] {
        [self.x[10], self.x[11], self.x[12], self.x[13], self.x[14], self.x[15]]
    }

    #[inline]
    pub fn set_arg(&mut self, index: usize, value: u64) {
        self.x[10 + index.min(5)] = value;
    }

    #[inline]
    pub fn ret(&self) -> u64 {
        self.x[10]
    }

    #[inline]
    pub fn set_ret(&mut self, value: u64) {
        self.x[10] = value;
    }

    #[inline]
    pub fn pc(&self) -> u64 {
        self.sepc
    }

    #[inline]
    pub fn set_pc(&mut self, value: u64) {
        self.sepc = value;
    }

    #[inline]
    pub fn sp(&self) -> u64 {
        self.x[2]
    }

    #[inline]
    pub fn set_sp(&mut self, value: u64) {
        self.x[2] = value;
    }

    pub fn restart(&mut self, _nr: u64) {
        self.x[10] = self.orig_a0;
        self.sepc -= 4;
    }

    pub fn fresh_user(&self, pc: u64, sp: u64) -> InterruptFrame {
        InterruptFrame::user(pc, sp)
    }

    pub fn user(pc: u64, sp: u64) -> InterruptFrame {
        let sstatus = (read_sstatus() & !(SSTATUS_SPP | SSTATUS_SIE)) | SSTATUS_SPIE | SSTATUS_FS;
        let mut x = [0u64; 32];
        x[2] = sp;
        InterruptFrame { x, sepc: pc, sstatus, scause: 0, stval: 0, orig_a0: 0, kind: 1, _pad: [0; 2] }
    }

    pub fn set_child_tls(&mut self, value: u64) -> bool {
        self.x[4] = value;
        true
    }
}

pub fn reset_fx(fx: &mut [u8]) {
    fx.fill(0);
}

core::arch::global_asm!(
    r#"
    .section .text
    .balign 4
    .global hamix_switch_context
hamix_switch_context:
    addi sp, sp, -112
    sd ra, 0(sp)
    sd s0, 8(sp)
    sd s1, 16(sp)
    sd s2, 24(sp)
    sd s3, 32(sp)
    sd s4, 40(sp)
    sd s5, 48(sp)
    sd s6, 56(sp)
    sd s7, 64(sp)
    sd s8, 72(sp)
    sd s9, 80(sp)
    sd s10, 88(sp)
    sd s11, 96(sp)
    csrr t0, sstatus
    sd t0, 104(sp)
    sd sp, 0(a0)
    mv sp, a1
    ld ra, 0(sp)
    ld s0, 8(sp)
    ld s1, 16(sp)
    ld s2, 24(sp)
    ld s3, 32(sp)
    ld s4, 40(sp)
    ld s5, 48(sp)
    ld s6, 56(sp)
    ld s7, 64(sp)
    ld s8, 72(sp)
    ld s9, 80(sp)
    ld s10, 88(sp)
    ld s11, 96(sp)
    ld t0, 104(sp)
    andi t0, t0, 2
    csrci sstatus, 2
    csrs sstatus, t0
    addi sp, sp, 112
    ret

    .global hamix_kernel_thread_trampoline
hamix_kernel_thread_trampoline:
    call task_entry_kernel
    csrsi sstatus, 2
    mv a0, s0
    jalr s1
    unimp

    .global hamix_return_trampoline
hamix_return_trampoline:
    csrci sstatus, 2
    call task_entry_user
    j hamix_exit_to_user
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

fn switch_frame(at: u64, s0: u64, s1: u64, ra: u64) {
    let mut words = [0u64; 14];
    words[0] = ra;
    words[1] = s0;
    words[2] = s1;
    words[13] = 0;
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

pub fn load_tls(_value: u64) {}

pub fn save_tls() -> Option<u64> {
    None
}
