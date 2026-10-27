use hamix_kpi as kpi;

use crate::regs::{Gen, Mmio};

const AUX_CTL_BASE: u32 = 0x64010;
const AUX_DATA_BASE: u32 = 0x64014;
const PORT_STRIDE: u32 = 0x100;

const SEND_BUSY: u32 = 1 << 31;
const DONE: u32 = 1 << 30;
const TIME_OUT_ERROR: u32 = 1 << 28;
const RECEIVE_ERROR: u32 = 1 << 25;
const TIME_OUT_1600US: u32 = 3 << 26;
const SIZE_SHIFT: u32 = 20;
const SIZE_MASK: u32 = 0x1F << SIZE_SHIFT;
const PRECHARGE: u32 = 5 << 16;

const I2C_WRITE: u8 = 0x0;
const I2C_READ: u8 = 0x1;
const I2C_MOT: u8 = 0x4;
const REPLY_ACK: u8 = 0x0;
const EDID_ADDRESS: u32 = 0x50;

fn control(mmio: &Mmio, chip: Gen, port: u32, size: usize) -> u32 {
    let base = SEND_BUSY | DONE | TIME_OUT_ERROR | RECEIVE_ERROR | ((size as u32) << SIZE_SHIFT);
    if matches!(chip, Gen::Gen9) {
        base | TIME_OUT_1600US | (31 << 5) | 31
    } else {
        let existing = mmio.read(AUX_CTL_BASE + port * PORT_STRIDE) & 0x7FF;
        let divider = if existing == 0 { 225 } else { existing };
        base | TIME_OUT_1600US | PRECHARGE | divider
    }
}

fn transfer(mmio: &Mmio, chip: Gen, port: u32, message: &[u8], reply: &mut [u8]) -> Option<usize> {
    if message.is_empty() || message.len() > 20 {
        return None;
    }
    let ctl = AUX_CTL_BASE + port * PORT_STRIDE;
    let data = AUX_DATA_BASE + port * PORT_STRIDE;
    for _ in 0..3 {
        let mut waited = 0;
        while mmio.read(ctl) & SEND_BUSY != 0 {
            if waited > 10_000 {
                return None;
            }
            unsafe { kpi::hamix_udelay(10) };
            waited += 10;
        }
        for word in 0..5u32 {
            let mut value = 0u32;
            for byte in 0..4usize {
                let index = word as usize * 4 + byte;
                if index < message.len() {
                    value |= (message[index] as u32) << (24 - byte * 8);
                }
            }
            mmio.write(data + word * 4, value);
        }
        mmio.write(ctl, control(mmio, chip, port, message.len()));
        let mut status = 0u32;
        let mut waited = 0;
        while waited < 20_000 {
            status = mmio.read(ctl);
            if status & SEND_BUSY == 0 {
                break;
            }
            unsafe { kpi::hamix_udelay(20) };
            waited += 20;
        }
        mmio.write(ctl, status | DONE | TIME_OUT_ERROR | RECEIVE_ERROR);
        if status & SEND_BUSY != 0 {
            return None;
        }
        if status & (TIME_OUT_ERROR | RECEIVE_ERROR) != 0 {
            unsafe { kpi::hamix_udelay(500) };
            continue;
        }
        let size = (((status & SIZE_MASK) >> SIZE_SHIFT) as usize).min(20);
        for i in 0..size.min(reply.len()) {
            let word = mmio.read(data + (i / 4) as u32 * 4);
            reply[i] = (word >> (24 - (i % 4) * 8)) as u8;
        }
        return Some(size);
    }
    None
}

fn i2c(mmio: &Mmio, chip: Gen, port: u32, command: u8, payload: &[u8], reply: &mut [u8], read_len: usize) -> bool {
    let mut message = [0u8; 20];
    message[0] = command << 4;
    message[1] = 0;
    message[2] = EDID_ADDRESS as u8;
    let mut len = 3;
    if !payload.is_empty() || read_len > 0 {
        message[3] = (if read_len > 0 { read_len } else { payload.len() }).saturating_sub(1) as u8;
        len = 4;
    }
    for byte in payload {
        if len < message.len() {
            message[len] = *byte;
            len += 1;
        }
    }
    for _ in 0..8 {
        let mut buffer = [0u8; 20];
        let Some(size) = transfer(mmio, chip, port, &message[..len], &mut buffer) else {
            return false;
        };
        if size == 0 {
            continue;
        }
        let native = (buffer[0] >> 4) & 0x3;
        let i2c_reply = (buffer[0] >> 6) & 0x3;
        if native != REPLY_ACK || i2c_reply != 0 {
            unsafe { kpi::hamix_udelay(400) };
            continue;
        }
        let got = size - 1;
        for i in 0..got.min(reply.len()) {
            reply[i] = buffer[1 + i];
        }
        return read_len == 0 || got == read_len;
    }
    false
}

pub fn read_edid(mmio: &Mmio, chip: Gen, port: u32, out: &mut [u8; 256]) -> usize {
    if !i2c(mmio, chip, port, I2C_WRITE | I2C_MOT, &[0], &mut [], 0) {
        return 0;
    }
    let mut total = 0usize;
    let mut limit = 128usize;
    while total < limit {
        let mut chunk = [0u8; 16];
        if !i2c(mmio, chip, port, I2C_READ | I2C_MOT, &[], &mut chunk, 16) {
            break;
        }
        out[total..total + 16].copy_from_slice(&chunk);
        total += 16;
        if total == 128 && out[126] > 0 {
            limit = 256;
        }
    }
    i2c(mmio, chip, port, I2C_READ, &[], &mut [], 0);
    if total >= 128 && out[0] == 0x00 && out[1] == 0xFF && out[7] == 0x00 { total } else { 0 }
}
