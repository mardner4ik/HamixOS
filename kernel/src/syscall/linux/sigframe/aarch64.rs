use alloc::vec;

use super::{SA_ONSTACK, SA_RESTORER, SS_DISABLE, SS_ONSTACK};
use crate::syscall::{copy_out, user_slice};
use crate::task::switch::{InterruptFrame, FX_SIZE};
use crate::task::{self, SigAction};

const INFO_SIZE: u64 = 128;
const UC_STACK: usize = 16;
const UC_SIGMASK: usize = 40;
const MCONTEXT: usize = 176;
const REGS: usize = MCONTEXT + 8;
const SP: usize = REGS + 31 * 8;
const PC: usize = SP + 8;
const PSTATE: usize = PC + 8;
const RESERVED: usize = PSTATE + 16;
const UC_SIZE: usize = RESERVED + 4096;
const FRAME_SIZE: u64 = INFO_SIZE + UC_SIZE as u64 + 16;
const FPSIMD_MAGIC: u32 = 0x4650_8001;
const FPSIMD_SIZE: usize = 528;

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u64(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap())
}

pub fn setup(frame: &mut InterruptFrame, fx: *mut u8, sig: u32, action: &SigAction, mask: u64, altstack: (u64, u64, u32)) -> bool {
    let restorer = if action.flags & SA_RESTORER != 0 && action.restorer != 0 { action.restorer } else { crate::task::elf::SIGRETURN_TRAMPOLINE };
    let on_alt = altstack.2 & SS_DISABLE == 0 && frame.sp > altstack.0 && frame.sp <= altstack.0 + altstack.1;
    let top = if action.flags & SA_ONSTACK != 0 && altstack.2 & SS_DISABLE == 0 && !on_alt { altstack.0 + altstack.1 } else { frame.sp };
    let sp = (top - FRAME_SIZE) & !15;
    let mut raw = vec![0u8; FRAME_SIZE as usize];
    raw[0..4].copy_from_slice(&sig.to_le_bytes());
    raw[16..20].copy_from_slice(&task::current_pid().to_le_bytes());
    let uc = &mut raw[INFO_SIZE as usize..INFO_SIZE as usize + UC_SIZE];
    put_u64(uc, UC_STACK, altstack.0);
    uc[UC_STACK + 8..UC_STACK + 12].copy_from_slice(&(if altstack.2 & SS_DISABLE != 0 { SS_DISABLE } else if on_alt { SS_ONSTACK } else { 0 }).to_le_bytes());
    put_u64(uc, UC_STACK + 16, altstack.1);
    put_u64(uc, UC_SIGMASK, mask);
    put_u64(uc, MCONTEXT, frame.far);
    for i in 0..31 {
        put_u64(uc, REGS + i * 8, frame.x[i]);
    }
    put_u64(uc, SP, frame.sp);
    put_u64(uc, PC, frame.elr);
    put_u64(uc, PSTATE, frame.spsr);
    uc[RESERVED..RESERVED + 4].copy_from_slice(&FPSIMD_MAGIC.to_le_bytes());
    uc[RESERVED + 4..RESERVED + 8].copy_from_slice(&(FPSIMD_SIZE as u32).to_le_bytes());
    let fp = unsafe { core::slice::from_raw_parts(fx, FX_SIZE) };
    uc[RESERVED + 8..RESERVED + 12].copy_from_slice(&fp[512..516]);
    uc[RESERVED + 12..RESERVED + 16].copy_from_slice(&fp[520..524]);
    uc[RESERVED + 16..RESERVED + 16 + 512].copy_from_slice(&fp[0..512]);
    if copy_out(sp, FRAME_SIZE, &raw) < 0 {
        return false;
    }
    frame.x[0] = sig as u64;
    frame.x[1] = sp;
    frame.x[2] = sp + INFO_SIZE;
    frame.x[29] = sp + FRAME_SIZE - 16;
    frame.x[30] = restorer;
    frame.sp = sp;
    frame.elr = action.handler;
    true
}

pub fn restore(frame: &mut InterruptFrame, fx: *mut u8) -> Option<u64> {
    let uc = user_slice(frame.sp + INFO_SIZE, UC_SIZE as u64).ok()?.to_vec();
    for i in 0..31 {
        frame.x[i] = get_u64(&uc, REGS + i * 8);
    }
    frame.sp = get_u64(&uc, SP);
    frame.elr = get_u64(&uc, PC);
    frame.spsr = get_u64(&uc, PSTATE) & 0xF000_0000;
    if u32::from_le_bytes(uc[RESERVED..RESERVED + 4].try_into().unwrap()) == FPSIMD_MAGIC {
        let fp = unsafe { core::slice::from_raw_parts_mut(fx, FX_SIZE) };
        fp[0..512].copy_from_slice(&uc[RESERVED + 16..RESERVED + 16 + 512]);
        fp[512..520].fill(0);
        fp[512..516].copy_from_slice(&uc[RESERVED + 8..RESERVED + 12]);
        fp[520..528].fill(0);
        fp[520..524].copy_from_slice(&uc[RESERVED + 12..RESERVED + 16]);
    }
    Some(get_u64(&uc, UC_SIGMASK))
}
