use alloc::collections::{BTreeMap, VecDeque};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use super::{Pid, WAIT_PIPE};
use crate::arch::without_interrupts;

const CAPACITY: usize = 256 * 1024;

pub use crate::errno::EPIPE;
pub use crate::errno::EAGAIN;
pub use crate::errno::EINTR;

struct Pipe {
    buffer: VecDeque<u8>,
    readers: u32,
    writers: u32,
    foreground: Pid,
    group: Pid,
    tty: bool,
    cols: u16,
    rows: u16,
    size_gen: u32,
    output: bool,
}

static PIPES: Mutex<BTreeMap<u32, Pipe>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

fn with_pipes<R>(f: impl FnOnce(&mut BTreeMap<u32, Pipe>) -> R) -> R {
    without_interrupts(|| f(&mut PIPES.lock()))
}

pub fn create() -> u32 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    with_pipes(|pipes| {
        pipes.insert(id, Pipe { buffer: VecDeque::new(), readers: 1, writers: 1, foreground: 0, group: 0, tty: false, cols: 80, rows: 25, size_gen: 0, output: false });
    });
    id
}

pub fn retain(id: u32, reader: bool) {
    with_pipes(|pipes| {
        if let Some(pipe) = pipes.get_mut(&id) {
            if reader {
                pipe.readers += 1;
            } else {
                pipe.writers += 1;
            }
        }
    });
}

pub fn release(id: u32, reader: bool) {
    with_pipes(|pipes| {
        if let Some(pipe) = pipes.get_mut(&id) {
            if reader {
                pipe.readers = pipe.readers.saturating_sub(1);
            } else {
                pipe.writers = pipe.writers.saturating_sub(1);
            }
            if pipe.readers == 0 && pipe.writers == 0 {
                pipes.remove(&id);
            }
        }
    });
    super::wake_all(WAIT_PIPE);
}

pub fn readable(id: u32) -> bool {
    with_pipes(|pipes| pipes.get(&id).map(|p| !p.buffer.is_empty() || p.writers == 0).unwrap_or(true))
}

pub fn available(id: u32) -> usize {
    with_pipes(|pipes| pipes.get(&id).map(|p| p.buffer.len()).unwrap_or(0))
}

pub fn read(id: u32, out: &mut [u8], nonblock: bool) -> i64 {
    if out.is_empty() {
        return 0;
    }
    loop {
        let result = with_pipes(|pipes| {
            let pipe = pipes.get_mut(&id)?;
            if !pipe.buffer.is_empty() {
                let n = out.len().min(pipe.buffer.len());
                for slot in out.iter_mut().take(n) {
                    *slot = pipe.buffer.pop_front().unwrap();
                }
                return Some(n as i64);
            }
            if pipe.writers == 0 {
                return Some(0);
            }
            None
        });
        match result {
            Some(n) => {
                super::wake_all(WAIT_PIPE);
                return n;
            }
            None if nonblock => return EAGAIN,
            None => {
                if !with_pipes(|pipes| pipes.contains_key(&id)) {
                    return 0;
                }
                if crate::syscall::signal::pending() {
                    return EINTR;
                }
                super::block(WAIT_PIPE, Some(super::TICK_HZ / 5), super::input_seq());
                super::check_killed();
            }
        }
    }
}

pub fn read_byte(id: u32, nonblock: bool) -> Option<u8> {
    let mut byte = [0u8; 1];
    if read(id, &mut byte, nonblock) == 1 { Some(byte[0]) } else { None }
}

pub fn set_foreground_group(id: u32, group: Pid) {
    with_pipes(|pipes| {
        if let Some(pipe) = pipes.get_mut(&id) {
            pipe.group = group;
        }
    });
}

pub fn foreground_group(id: u32) -> Option<Pid> {
    let group = with_pipes(|pipes| pipes.get(&id).map(|p| p.group).unwrap_or(0));
    (group != 0 && group_members(group).next().is_some()).then_some(group)
}

fn group_members(group: Pid) -> alloc::vec::IntoIter<(Pid, bool, bool)> {
    super::with_tasks(|tasks| {
        tasks
            .values()
            .filter(|t| t.pgid == group && !t.kernel_thread && t.state != super::State::Zombie)
            .map(|t| (t.pid, t.abi.posix(), crate::tty::signals_enabled(&t.termios)))
            .collect::<alloc::vec::Vec<_>>()
    })
    .into_iter()
}

fn interrupt_group(id: u32, writer: Pid, sig: u32) -> bool {
    let Some(group) = foreground_group(id) else {
        return false;
    };
    let members: alloc::vec::Vec<(Pid, bool, bool)> = group_members(group).collect();
    if members.iter().any(|(pid, _, _)| *pid == writer) {
        return false;
    }
    for (pid, posix, isig) in members {
        match (posix, isig) {
            (true, true) => {
                crate::syscall::signal::send(pid, sig);
            }
            (true, false) => {}
            _ => {
                if sig == 2 {
                    super::kill(pid, 130);
                }
            }
        }
    }
    true
}

fn interrupt_policy(id: u32, writer: Pid) -> Option<(Pid, bool)> {
    if foreground_group(id).is_some() {
        return Some((0, true));
    }
    let foreground = with_pipes(|pipes| pipes.get(&id).filter(|p| p.tty && p.foreground != 0 && p.foreground != writer).map(|p| p.foreground))?;
    let linux = super::with_task(foreground, |t| (t.abi.posix(), crate::tty::signals_enabled(&t.termios)));
    match linux {
        Some((true, false)) => None,
        Some((true, true)) => Some((foreground, true)),
        _ => Some((foreground, false)),
    }
}

pub fn write(id: u32, data: &[u8], writer: Pid) -> i64 {
    let policy = if data.iter().any(|b| matches!(b, 0x03 | 0x1a | 0x1c)) { interrupt_policy(id, writer) } else { None };
    let mut done = 0usize;
    while done < data.len() {
        let step = with_pipes(|pipes| {
            let Some(pipe) = pipes.get_mut(&id) else {
                return Err(EPIPE);
            };
            if pipe.readers == 0 {
                return Err(EPIPE);
            }
            let room = CAPACITY.saturating_sub(pipe.buffer.len());
            let mut taken = 0usize;
            let mut interrupt = None;
            for &b in data[done..].iter().take(room) {
                taken += 1;
                if let Some((pid, linux)) = policy {
                    let signal = match b {
                        0x03 => Some(2u32),
                        0x1c if linux => Some(3),
                        0x1a if linux => Some(20),
                        _ => None,
                    };
                    if let Some(sig) = signal {
                        interrupt = Some((pid, linux, sig));
                        continue;
                    }
                }
                pipe.buffer.push_back(b);
            }
            Ok((taken, interrupt))
        });
        match step {
            Err(e) => return if done > 0 { done as i64 } else { e },
            Ok((taken, interrupt)) => {
                if let Some((pid, linux, sig)) = interrupt {
                    if pid == 0 {
                        if !interrupt_group(id, writer, sig) {
                            let _ = with_pipes(|pipes| pipes.get_mut(&id).map(|p| p.buffer.push_back(match sig { 2 => 0x03, 3 => 0x1c, _ => 0x1a })));
                        }
                    } else if linux && super::exists(pid) {
                        crate::syscall::signal::send(pid, sig);
                    } else if super::exists(pid) {
                        super::kill(pid, 130);
                    } else {
                        let _ = with_pipes(|pipes| pipes.get_mut(&id).map(|p| p.buffer.push_back(0x03)));
                    }
                }
                done += taken;
                super::wake_all(WAIT_PIPE);
                if taken == 0 {
                    if crate::syscall::signal::pending() {
                        return if done > 0 { done as i64 } else { EINTR };
                    }
                    super::block(WAIT_PIPE, Some(super::TICK_HZ / 5), super::input_seq());
                    super::check_killed();
                }
            }
        }
    }
    done as i64
}

pub fn mark_terminal(id: u32, cols: u16, rows: u16, output: bool) -> bool {
    with_pipes(|pipes| {
        if let Some(pipe) = pipes.get_mut(&id) {
            pipe.output = output;
        }
    });
    set_terminal(id, cols, rows)
}

pub fn is_output_terminal(id: u32) -> bool {
    with_pipes(|pipes| pipes.get(&id).map(|p| p.tty && p.output).unwrap_or(false))
}

pub fn is_input_terminal(id: u32) -> bool {
    with_pipes(|pipes| pipes.get(&id).map(|p| p.tty && !p.output).unwrap_or(false))
}

pub fn set_terminal(id: u32, cols: u16, rows: u16) -> bool {
    let changed = with_pipes(|pipes| {
        let Some(pipe) = pipes.get_mut(&id) else {
            return false;
        };
        let (cols, rows) = (cols.max(10), rows.max(4));
        let changed = !pipe.tty || pipe.cols != cols || pipe.rows != rows;
        pipe.tty = true;
        pipe.cols = cols;
        pipe.rows = rows;
        if changed {
            pipe.size_gen = pipe.size_gen.wrapping_add(1);
        }
        changed
    });
    if changed {
        super::wake_all(WAIT_PIPE);
        super::notify_input();
    }
    changed
}

pub fn terminal(id: u32) -> Option<(u16, u16)> {
    with_pipes(|pipes| pipes.get(&id).filter(|p| p.tty).map(|p| (p.cols, p.rows)))
}

pub fn terminal_generation(id: u32) -> Option<u32> {
    with_pipes(|pipes| pipes.get(&id).filter(|p| p.tty).map(|p| p.size_gen))
}

pub fn set_foreground(id: u32, pid: Pid) {
    with_pipes(|pipes| {
        if let Some(pipe) = pipes.get_mut(&id) {
            pipe.foreground = pid;
        }
    });
}
