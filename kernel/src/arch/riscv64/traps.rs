use core::fmt;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

core::arch::global_asm!(
    r#"
    .section .text
    .option push
    .option arch, +d
    .balign 4
    .global hamix_trap_entry
hamix_trap_entry:
    csrrw sp, sscratch, sp
    bnez sp, hamix_trap_from_user
    csrrw sp, sscratch, sp
    addi sp, sp, -320
    .irp n, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    sd x\n, \n*8(sp)
    .endr
    addi t0, sp, 320
    sd t0, 16(sp)
    csrr t0, sepc
    sd t0, 256(sp)
    csrr t0, sstatus
    sd t0, 264(sp)
    csrr t0, scause
    sd t0, 272(sp)
    csrr t0, stval
    sd t0, 280(sp)
    sd zero, 296(sp)
    mv a0, sp
    call riscv_trap
    ld t0, 256(sp)
    csrw sepc, t0
    ld t0, 264(sp)
    csrw sstatus, t0
    .irp n, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    ld x\n, \n*8(sp)
    .endr
    addi sp, sp, 320
    sret

hamix_trap_from_user:
    addi sp, sp, -320
    .irp n, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    sd x\n, \n*8(sp)
    .endr
    csrr t0, sscratch
    sd t0, 16(sp)
    csrw sscratch, zero
    csrr t0, sepc
    sd t0, 256(sp)
    csrr t0, sstatus
    sd t0, 264(sp)
    csrr t0, scause
    sd t0, 272(sp)
    csrr t0, stval
    sd t0, 280(sp)
    sd a0, 288(sp)
    li t0, 1
    sd t0, 296(sp)
    mv a0, sp
    addi sp, sp, -272
    .irp n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    fsd f\n, \n*8(sp)
    .endr
    frcsr t0
    sd t0, 256(sp)
    mv a1, sp
    call riscv_user_trap

    .global hamix_exit_to_user
hamix_exit_to_user:
    .irp n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    fld f\n, \n*8(sp)
    .endr
    ld t0, 256(sp)
    fscsr t0
    addi sp, sp, 272
    ld t0, 256(sp)
    csrw sepc, t0
    ld t0, 264(sp)
    csrw sstatus, t0
    addi t0, sp, 320
    csrw sscratch, t0
    .irp n, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    ld x\n, \n*8(sp)
    .endr
    ld sp, 16(sp)
    sret
    .option pop

    .option push
    .option norvc
    .global hamix_probe_read
hamix_probe_read:
    mv a1, a0
    .global hamix_probe_insn
hamix_probe_insn:
    ld a0, 0(a1)
    ret
    .option pop
    "#
);

unsafe extern "C" {
    static hamix_trap_entry: u8;
    static hamix_probe_insn: u8;
    fn hamix_probe_read(addr: usize) -> u64;
}

pub use super::context::InterruptFrame as TrapFrame;

const INTERRUPT: u64 = 1 << 63;
const IRQ_SOFTWARE: u64 = 1;
const IRQ_TIMER: u64 = 5;
const IRQ_EXTERNAL: u64 = 9;
const BREAKPOINT: u64 = 3;
const LOAD_ACCESS: u64 = 5;
const ECALL_USER: u64 = 8;
const LOAD_PAGE_FAULT: u64 = 13;
const INSTRUCTION_PAGE_FAULT: u64 = 12;
const STORE_PAGE_FAULT: u64 = 15;

static BREAKPOINTS: AtomicUsize = AtomicUsize::new(0);
static PROBE_FAULT: AtomicU64 = AtomicU64::new(u64::MAX);

const NAMES: [&str; 16] = [
    "instruction misaligned",
    "instruction access fault",
    "illegal instruction",
    "breakpoint",
    "load misaligned",
    "load access fault",
    "store misaligned",
    "store access fault",
    "ecall from U-mode",
    "ecall from S-mode",
    "reserved",
    "ecall from M-mode",
    "instruction page fault",
    "load page fault",
    "reserved",
    "store page fault",
];

impl fmt::Display for TrapFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.scause & !INTERRUPT;
        let name = NAMES.get(code as usize).copied().unwrap_or("exception");
        writeln!(f, "{} (scause {:#x}) at sepc {:#018x}, stval {:#018x}, sstatus {:#x}", name, self.scause, self.sepc, self.stval, self.sstatus)?;
        for row in 0..8 {
            for col in 0..4 {
                let n = row * 4 + col;
                write!(f, "x{:<2} {:#018x}  ", n, self.x[n])?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

pub fn init() {
    unsafe {
        core::arch::asm!("csrw stvec, {}", in(reg) (&raw const hamix_trap_entry) as usize, options(nostack));
        core::arch::asm!("csrw sscratch, zero", options(nostack));
    }
}

pub fn breakpoint() {
    unsafe {
        core::arch::asm!(".option push", ".option norvc", "ebreak", ".option pop", options(nomem, nostack));
    }
}

pub fn breakpoints() -> usize {
    BREAKPOINTS.load(Ordering::Relaxed)
}

pub fn probe_read(addr: usize) -> Result<u64, u64> {
    PROBE_FAULT.store(u64::MAX, Ordering::SeqCst);
    let value = unsafe { hamix_probe_read(addr) };
    match PROBE_FAULT.swap(u64::MAX, Ordering::SeqCst) {
        u64::MAX => Ok(value),
        stval => Err(stval),
    }
}

fn instruction_len(sepc: u64) -> u64 {
    let low = unsafe { core::ptr::read_volatile(sepc as *const u16) };
    if low & 0b11 == 0b11 { 4 } else { 2 }
}

fn exception(frame: &mut TrapFrame) {
    let code = frame.scause;
    match code {
        BREAKPOINT => {
            BREAKPOINTS.fetch_add(1, Ordering::Relaxed);
            frame.sepc += instruction_len(frame.sepc);
        }
        LOAD_ACCESS | LOAD_PAGE_FAULT if frame.sepc == (&raw const hamix_probe_insn) as u64 => {
            PROBE_FAULT.store(frame.stval, Ordering::SeqCst);
            frame.x[10] = 0;
            frame.sepc += 4;
        }
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT if crate::task::fault::kernel_touch(frame.stval, frame.sstatus & (1 << 5) != 0) => {}
        _ => panic!("unhandled {}", frame),
    }
}

static TICK_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn note_tick() {
    TICK_PENDING.store(true, Ordering::Relaxed);
}

fn interrupt(frame: &mut TrapFrame, fx: *mut u8) {
    match frame.scause & !INTERRUPT {
        IRQ_TIMER => super::timer::on_interrupt(),
        IRQ_EXTERNAL => super::plic::handle(),
        IRQ_SOFTWARE => unsafe { core::arch::asm!("csrc sip, {}", in(reg) 2usize, options(nostack)) },
        other => panic!("unexpected interrupt {}\n{}", other, frame),
    }
    if TICK_PENDING.swap(false, Ordering::Relaxed) {
        crate::task::on_tick(0, true, frame, fx);
    }
}

#[unsafe(no_mangle)]
extern "C" fn riscv_user_trap(frame: &mut TrapFrame, fx: *mut u8) {
    if frame.scause & INTERRUPT != 0 {
        interrupt(frame, fx);
    } else {
        match frame.scause {
            ECALL_USER => {
                frame.sepc += 4;
                crate::arch::enable_interrupts();
                crate::syscall::handle_syscall(frame, fx);
                crate::arch::disable_interrupts();
            }
            INSTRUCTION_PAGE_FAULT | LOAD_PAGE_FAULT | STORE_PAGE_FAULT if crate::task::fault::user_fault(frame.stval) => {}
            _ => {
                crate::serial_println!("user exception: {}", frame);
                let code = frame.scause as usize;
                crate::task::fault::kill_faulting_process(NAMES.get(code).copied().unwrap_or("exception"));
            }
        }
    }
    crate::syscall::deliver_from_trap(frame, fx);
}

#[unsafe(no_mangle)]
extern "C" fn riscv_trap(frame: &mut TrapFrame) {
    if frame.scause & INTERRUPT == 0 {
        exception(frame);
        return;
    }
    interrupt(frame, core::ptr::null_mut());
}
