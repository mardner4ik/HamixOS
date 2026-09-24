use alloc::vec;

use super::{SA_ONSTACK, SA_RESTORER, SS_DISABLE, SS_ONSTACK};
use crate::syscall::{copy_out, user_slice};
use crate::task::switch::{InterruptFrame, FX_SIZE};
use crate::task::{self, SigAction};

const INFO_SIZE: u64 = 128;
const UC_STACK: usize = 16;
const UC_SIGMASK: usize = 40;
const MCONTEXT: usize = 176;
const FPREGS: usize = MCONTEXT + 32 * 8;
const UC_SIZE: usize = FPREGS + 528;
const FRAME_SIZE: u64 = INFO_SIZE + UC_SIZE as u64;

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u64(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap())
}

pub fn setup(frame: &mut InterruptFrame, fx: *mut u8, sig: u32, action: &SigAction, mask: u64, altstack: (u64, u64, u32)) -> bool {
    let restorer = if action.flags & SA_RESTORER != 0 && action.restorer != 0 { action.restorer } else { crate::task::elf::SIGRETURN_TRAMPOLINE };
    let current_sp = frame.x[2];
    let on_alt = altstack.2 & SS_DISABLE == 0 && current_sp > altstack.0 && current_sp <= altstack.0 + altstack.1;
    let top = if action.flags & SA_ONSTACK != 0 && altstack.2 & SS_DISABLE == 0 && !on_alt { altstack.0 + altstack.1 } else { current_sp };
    let sp = (top - FRAME_SIZE) & !15;
    let mut raw = vec![0u8; FRAME_SIZE as usize];
    raw[0..4].copy_from_slice(&sig.to_le_bytes());
    raw[16..20].copy_from_slice(&task::current_pid().to_le_bytes());
    let uc = &mut raw[INFO_SIZE as usize..];
    put_u64(uc, UC_STACK, altstack.0);
    uc[UC_STACK + 8..UC_STACK + 12].copy_from_slice(&(if altstack.2 & SS_DISABLE != 0 { SS_DISABLE } else if on_alt { SS_ONSTACK } else { 0 }).to_le_bytes());
    put_u64(uc, UC_STACK + 16, altstack.1);
    put_u64(uc, UC_SIGMASK, mask);
    put_u64(uc, MCONTEXT, frame.sepc);
    for i in 1..32 {
        put_u64(uc, MCONTEXT + i * 8, frame.x[i]);
    }
    let fp = unsafe { core::slice::from_raw_parts(fx, FX_SIZE) };
    uc[FPREGS..FPREGS + 260].copy_from_slice(&fp[0..260]);
    if copy_out(sp, FRAME_SIZE, &raw) < 0 {
        return false;
    }
    frame.x[10] = sig as u64;
    frame.x[11] = sp;
    frame.x[12] = sp + INFO_SIZE;
    frame.x[1] = restorer;
    frame.x[2] = sp;
    frame.sepc = action.handler;
    true
}

pub fn restore(frame: &mut InterruptFrame, fx: *mut u8) -> Option<u64> {
    let uc = user_slice(frame.x[2] + INFO_SIZE, UC_SIZE as u64).ok()?.to_vec();
    frame.sepc = get_u64(&uc, MCONTEXT);
    for i in 1..32 {
        frame.x[i] = get_u64(&uc, MCONTEXT + i * 8);
    }
    let fp = unsafe { core::slice::from_raw_parts_mut(fx, FX_SIZE) };
    fp[0..260].copy_from_slice(&uc[FPREGS..FPREGS + 260]);
    Some(get_u64(&uc, UC_SIGMASK))
}
