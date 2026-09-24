use alloc::vec::Vec;

use super::{SA_ONSTACK, SA_RESTORER, SS_DISABLE, SS_ONSTACK};
use crate::syscall::{copy_out, user_slice};
use crate::task::switch::InterruptFrame;
use crate::task::{self, SigAction};

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u64(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap())
}

const UC_OFFSET: u64 = 8;
const MCONTEXT: usize = 40;
const SIGMASK: usize = MCONTEXT + 256;
const UC_SIZE: usize = SIGMASK + 8;
const INFO_OFFSET: u64 = UC_OFFSET + UC_SIZE as u64;
const FRAME_SIZE: u64 = INFO_OFFSET + 128;

fn save_context(frame: &InterruptFrame, uc: &mut [u8], fpstate: u64, mask: u64, altstack: (u64, u64, u32), on_alt: bool) {
    put_u64(uc, 16, altstack.0);
    uc[24..28].copy_from_slice(&(if altstack.2 & SS_DISABLE != 0 { SS_DISABLE } else if on_alt { SS_ONSTACK } else { 0 }).to_le_bytes());
    put_u64(uc, 32, altstack.1);
    let regs = [
        frame.r8, frame.r9, frame.r10, frame.r11, frame.r12, frame.r13, frame.r14, frame.r15, frame.rdi, frame.rsi, frame.rbp, frame.rbx, frame.rdx, frame.rax, frame.rcx, frame.rsp,
        frame.rip, frame.rflags,
    ];
    for (i, v) in regs.iter().enumerate() {
        put_u64(uc, MCONTEXT + i * 8, *v);
    }
    put_u64(uc, MCONTEXT + 18 * 8, (frame.cs & 0xFFFF) | ((frame.ss & 0xFFFF) << 48));
    put_u64(uc, MCONTEXT + 21 * 8, mask);
    put_u64(uc, MCONTEXT + 23 * 8, fpstate);
    put_u64(uc, SIGMASK, mask);
}

pub fn setup(frame: &mut InterruptFrame, fx: *mut u8, sig: u32, action: &SigAction, mask: u64, altstack: (u64, u64, u32)) -> bool {
    if action.flags & SA_RESTORER == 0 || action.restorer == 0 {
        return false;
    }
    let on_alt = altstack.2 & SS_DISABLE == 0 && frame.rsp > altstack.0 && frame.rsp <= altstack.0 + altstack.1;
    let mut sp = if action.flags & SA_ONSTACK != 0 && altstack.2 & SS_DISABLE == 0 && !on_alt { altstack.0 + altstack.1 } else { frame.rsp - 128 };
    sp = (sp - 512) & !63;
    let fpstate = sp;
    let fx_bytes = unsafe { core::slice::from_raw_parts(fx, 512) };
    if copy_out(fpstate, 512, fx_bytes) < 0 {
        return false;
    }
    sp = ((sp - FRAME_SIZE) & !15) - 8;
    let mut raw = alloc::vec![0u8; FRAME_SIZE as usize];
    put_u64(&mut raw, 0, action.restorer);
    save_context(frame, &mut raw[UC_OFFSET as usize..UC_OFFSET as usize + UC_SIZE], fpstate, mask, altstack, on_alt);
    let info = &mut raw[INFO_OFFSET as usize..];
    info[0..4].copy_from_slice(&sig.to_le_bytes());
    info[16..20].copy_from_slice(&(task::current_pid()).to_le_bytes());
    if copy_out(sp, FRAME_SIZE, &raw) < 0 {
        return false;
    }
    frame.rip = action.handler;
    frame.rsp = sp;
    frame.rdi = sig as u64;
    frame.rsi = sp + INFO_OFFSET;
    frame.rdx = sp + UC_OFFSET;
    frame.rax = 0;
    frame.rflags &= !(0x400 | 0x100 | 0x40000);
    true
}

pub fn restore(frame: &mut InterruptFrame, fx: *mut u8) -> Option<u64> {
    let base = frame.rsp - 8;
    let uc_addr = base + UC_OFFSET;
    let raw: Vec<u8> = user_slice(uc_addr, UC_SIZE as u64).ok()?.to_vec();
    let reg = |i: usize| get_u64(&raw, MCONTEXT + i * 8);
    frame.r8 = reg(0);
    frame.r9 = reg(1);
    frame.r10 = reg(2);
    frame.r11 = reg(3);
    frame.r12 = reg(4);
    frame.r13 = reg(5);
    frame.r14 = reg(6);
    frame.r15 = reg(7);
    frame.rdi = reg(8);
    frame.rsi = reg(9);
    frame.rbp = reg(10);
    frame.rbx = reg(11);
    frame.rdx = reg(12);
    frame.rax = reg(13);
    frame.rcx = reg(14);
    frame.rsp = reg(15);
    frame.rip = reg(16);
    let user_flags = 0x0CD5 | 0x40000 | 0x200000;
    frame.rflags = (frame.rflags & !user_flags) | (reg(17) & user_flags) | 0x202;
    let fpstate = reg(23);
    if fpstate != 0 {
        if let Ok(src) = user_slice(fpstate, 512) {
            let mut copy = [0u8; 512];
            copy.copy_from_slice(src);
            let mxcsr = u32::from_le_bytes(copy[24..28].try_into().unwrap()) & 0xFFBF;
            copy[24..28].copy_from_slice(&mxcsr.to_le_bytes());
            unsafe { core::ptr::copy_nonoverlapping(copy.as_ptr(), fx, 512) };
        }
    }
    Some(get_u64(&raw, SIGMASK))
}

