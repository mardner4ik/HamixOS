use core::fmt;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

core::arch::global_asm!(
    r#"
    .arch armv8-a+fp+simd
    .section .text
    .balign 0x800
    .global hamix_vectors
hamix_vectors:
    .irp kind, 0, 1, 2, 3, 4, 5, 6, 7
    .balign 0x80
    sub sp, sp, #320
    stp x0, x1, [sp, #0]
    mov x0, #\kind
    b hamix_trap_kernel
    .endr
    .irp kind, 8, 9, 10, 11, 12, 13, 14, 15
    .balign 0x80
    sub sp, sp, #320
    stp x0, x1, [sp, #0]
    mov x0, #\kind
    b hamix_trap_user
    .endr

hamix_save_regs:
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    mrs x2, elr_el1
    mrs x3, spsr_el1
    stp x2, x3, [sp, #248]
    mrs x2, esr_el1
    mrs x3, far_el1
    stp x2, x3, [sp, #264]
    mrs x2, sp_el0
    stp x2, x0, [sp, #280]
    ldr x2, [sp, #0]
    str x2, [sp, #296]
    ret

hamix_trap_kernel:
    str x30, [sp, #240]
    bl hamix_save_regs
    mov x1, x0
    mov x0, sp
    bl aarch64_trap
    b hamix_restore_regs

hamix_trap_user:
    str x30, [sp, #240]
    bl hamix_save_regs
    mov x2, sp
    sub sp, sp, #528
    stp q0, q1, [sp, #0]
    stp q2, q3, [sp, #32]
    stp q4, q5, [sp, #64]
    stp q6, q7, [sp, #96]
    stp q8, q9, [sp, #128]
    stp q10, q11, [sp, #160]
    stp q12, q13, [sp, #192]
    stp q14, q15, [sp, #224]
    stp q16, q17, [sp, #256]
    stp q18, q19, [sp, #288]
    stp q20, q21, [sp, #320]
    stp q22, q23, [sp, #352]
    stp q24, q25, [sp, #384]
    stp q26, q27, [sp, #416]
    stp q28, q29, [sp, #448]
    stp q30, q31, [sp, #480]
    mrs x3, fpsr
    mrs x4, fpcr
    add x5, sp, #512
    stp x3, x4, [x5]
    mov x1, sp
    mov x0, x2
    bl aarch64_user_trap

    .global hamix_exit_to_user
hamix_exit_to_user:
    ldp q0, q1, [sp, #0]
    ldp q2, q3, [sp, #32]
    ldp q4, q5, [sp, #64]
    ldp q6, q7, [sp, #96]
    ldp q8, q9, [sp, #128]
    ldp q10, q11, [sp, #160]
    ldp q12, q13, [sp, #192]
    ldp q14, q15, [sp, #224]
    ldp q16, q17, [sp, #256]
    ldp q18, q19, [sp, #288]
    ldp q20, q21, [sp, #320]
    ldp q22, q23, [sp, #352]
    ldp q24, q25, [sp, #384]
    ldp q26, q27, [sp, #416]
    ldp q28, q29, [sp, #448]
    ldp q30, q31, [sp, #480]
    add x5, sp, #512
    ldp x3, x4, [x5]
    msr fpsr, x3
    msr fpcr, x4
    add sp, sp, #528

hamix_restore_regs:
    ldp x2, x3, [sp, #248]
    msr elr_el1, x2
    msr spsr_el1, x3
    ldr x2, [sp, #280]
    msr sp_el0, x2
    ldp x0, x1, [sp, #0]
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]
    ldr x30, [sp, #240]
    add sp, sp, #320
    eret

    .global hamix_probe_read
hamix_probe_read:
    mov x1, x0
    .global hamix_probe_insn
hamix_probe_insn:
    ldr x0, [x1]
    ret
    "#
);

unsafe extern "C" {
    static hamix_vectors: u8;
    static hamix_probe_insn: u8;
    fn hamix_probe_read(addr: usize) -> u64;
}

pub use super::context::InterruptFrame as TrapFrame;

const EC_UNKNOWN: u64 = 0x00;
const EC_SVC64: u64 = 0x15;
const EC_INSN_ABORT_LOWER: u64 = 0x20;
const EC_INSN_ABORT: u64 = 0x21;
const EC_DATA_ABORT_LOWER: u64 = 0x24;
const EC_DATA_ABORT: u64 = 0x25;
const EC_BRK64: u64 = 0x3C;

static BREAKPOINTS: AtomicUsize = AtomicUsize::new(0);
static PROBE_FAULT: AtomicU64 = AtomicU64::new(u64::MAX);

fn class_name(ec: u64) -> &'static str {
    match ec {
        EC_UNKNOWN => "undefined instruction",
        EC_SVC64 => "svc",
        EC_INSN_ABORT_LOWER | EC_INSN_ABORT => "instruction abort",
        EC_DATA_ABORT_LOWER | EC_DATA_ABORT => "data abort",
        0x22 => "pc alignment fault",
        0x26 => "sp alignment fault",
        EC_BRK64 => "brk",
        _ => "exception",
    }
}

impl fmt::Display for TrapFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ec = (self.esr >> 26) & 0x3F;
        writeln!(f, "{} (ec {:#x}, iss {:#x}) at elr {:#018x}, far {:#018x}, spsr {:#x}", class_name(ec), ec, self.esr & 0x1FF_FFFF, self.elr, self.far, self.spsr)?;
        for row in 0..8 {
            for col in 0..4 {
                let n = row * 4 + col;
                if n < 31 {
                    write!(f, "x{:<2} {:#018x}  ", n, self.x[n])?;
                } else {
                    write!(f, "sp0 {:#018x}", self.sp)?;
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

pub fn init() {
    unsafe {
        core::arch::asm!("msr vbar_el1, {}", "isb", in(reg) (&raw const hamix_vectors) as u64, options(nostack));
    }
}

pub fn breakpoint() {
    unsafe { core::arch::asm!("brk #0x48", options(nomem, nostack)) };
}

pub fn breakpoints() -> usize {
    BREAKPOINTS.load(Ordering::Relaxed)
}

pub fn probe_read(addr: usize) -> Result<u64, u64> {
    PROBE_FAULT.store(u64::MAX, Ordering::SeqCst);
    let value = unsafe { hamix_probe_read(addr) };
    match PROBE_FAULT.swap(u64::MAX, Ordering::SeqCst) {
        u64::MAX => Ok(value),
        far => Err(far),
    }
}

static TICK_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn note_tick() {
    TICK_PENDING.store(true, Ordering::Relaxed);
}

fn take_tick() -> bool {
    TICK_PENDING.swap(false, Ordering::Relaxed)
}

fn synchronous(frame: &mut TrapFrame) {
    let ec = (frame.esr >> 26) & 0x3F;
    match ec {
        EC_BRK64 => {
            BREAKPOINTS.fetch_add(1, Ordering::Relaxed);
            frame.elr += 4;
        }
        EC_DATA_ABORT | EC_DATA_ABORT_LOWER if frame.elr == (&raw const hamix_probe_insn) as u64 => {
            PROBE_FAULT.store(frame.far, Ordering::SeqCst);
            frame.x[0] = 0;
            frame.elr += 4;
        }
        EC_DATA_ABORT if translation_fault(frame.esr) && crate::task::fault::kernel_touch(frame.far, frame.spsr & (1 << 7) == 0) => {}
        EC_SVC64 => {
            frame.x[0] = (-38i64) as u64;
        }
        _ => panic!("unhandled {}", frame),
    }
}

fn translation_fault(esr: u64) -> bool {
    matches!(esr & 0x3F, 0x04..=0x07 | 0x0C..=0x0F)
}

#[unsafe(no_mangle)]
extern "C" fn aarch64_trap(frame: &mut TrapFrame, kind: u64) {
    match kind % 4 {
        0 => synchronous(frame),
        1 => {
            super::gic::handle();
            if take_tick() {
                crate::task::on_tick(0, true, frame, core::ptr::null_mut());
            }
        }
        2 => panic!("unexpected FIQ\n{}", frame),
        _ => panic!("SError\n{}", frame),
    }
}

#[unsafe(no_mangle)]
extern "C" fn aarch64_user_trap(frame: &mut TrapFrame, fx: *mut u8) {
    match frame.kind % 4 {
        0 => {
            let ec = (frame.esr >> 26) & 0x3F;
            match ec {
                EC_SVC64 => {
                    frame.orig_x0 = frame.x[0];
                    crate::arch::enable_interrupts();
                    crate::syscall::handle_syscall(frame, fx);
                    crate::arch::disable_interrupts();
                }
                EC_DATA_ABORT_LOWER | EC_INSN_ABORT_LOWER => {
                    let handled = translation_fault(frame.esr) && crate::task::fault::user_fault(frame.far);
                    if !handled {
                        crate::serial_println!("user fault: {}", frame);
                        crate::task::fault::kill_faulting_process(class_name(ec));
                    }
                }
                _ => {
                    crate::serial_println!("user exception: {}", frame);
                    crate::task::fault::kill_faulting_process(class_name(ec));
                }
            }
        }
        1 => {
            super::gic::handle();
            if take_tick() {
                crate::task::on_tick(0, true, frame, fx);
            }
        }
        _ => {
            crate::serial_println!("user exception: {}", frame);
            crate::task::fault::kill_faulting_process("serror");
        }
    }
    crate::syscall::deliver_from_trap(frame, fx);
}
