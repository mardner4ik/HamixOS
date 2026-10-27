use alloc::collections::BTreeMap;
use spin::Mutex;

use super::pipe;
use crate::arch::without_interrupts;

struct Pty {
    input: u32,
    output: u32,
    reserved: bool,
    packet: bool,
}

static PTYS: Mutex<BTreeMap<u32, Pty>> = Mutex::new(BTreeMap::new());

fn with<R>(f: impl FnOnce(&mut BTreeMap<u32, Pty>) -> R) -> R {
    without_interrupts(|| f(&mut PTYS.lock()))
}

pub fn create() -> (u32, u32, u32) {
    let input = pipe::create();
    let output = pipe::create();
    pipe::mark_terminal(input, 80, 24, false);
    pipe::mark_terminal(output, 80, 24, true);
    let index = with(|t| {
        let index = (0..).find(|i| !t.contains_key(i)).unwrap();
        t.insert(index, Pty { input, output, reserved: true, packet: false });
        index
    });
    (index, input, output)
}

pub fn open_slave(index: u32) -> Option<(u32, u32)> {
    let (input, output, reserved) = with(|t| {
        let pty = t.get_mut(&index)?;
        let reserved = core::mem::replace(&mut pty.reserved, false);
        Some((pty.input, pty.output, reserved))
    })?;
    if !reserved {
        pipe::retain(input, true);
        pipe::retain(output, false);
    }
    Some((input, output))
}

pub fn set_packet(index: u32, on: bool) -> bool {
    with(|t| match t.get_mut(&index) {
        Some(pty) => {
            pty.packet = on;
            true
        }
        None => false,
    })
}

pub fn packet(index: u32) -> bool {
    with(|t| t.get(&index).map(|p| p.packet).unwrap_or(false))
}

pub fn master_closed(index: u32) {
    let released = with(|t| t.remove(&index));
    if let Some(pty) = released {
        if pty.reserved {
            pipe::release(pty.input, true);
            pipe::release(pty.output, false);
        }
    }
}
