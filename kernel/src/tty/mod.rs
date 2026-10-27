use alloc::vec::Vec;

use crate::syscall::{copy_out, target, user_slice, Target, EINVAL};
use crate::drivers::input::keyboard::{self, Key};
use crate::drivers::video::text_mode::TEXT_CONSOLE;
use crate::task::{self, pipe};

pub const TERMIOS_LEN: usize = 36;

const ICRNL: u32 = 0o400;
const OPOST: u32 = 0o1;
const ONLCR: u32 = 0o4;
const ISIG: u32 = 0o1;
const ICANON: u32 = 0o2;
const ECHO: u32 = 0o10;
const CC: usize = 17;
const VERASE: usize = CC + 2;
const VKILL: usize = CC + 3;
const VEOF: usize = CC + 4;
const VTIME: usize = CC + 5;
const VMIN: usize = CC + 6;
const MAX_LINE: usize = 4096;

const POLLIN: u16 = 1;
const POLLOUT: u16 = 4;
const POLLERR: u16 = 8;
const POLLHUP: u16 = 0x10;
const POLLNVAL: u16 = 0x20;

const fn put(mut raw: [u8; TERMIOS_LEN], at: usize, value: u32) -> [u8; TERMIOS_LEN] {
    let bytes = value.to_le_bytes();
    raw[at] = bytes[0];
    raw[at + 1] = bytes[1];
    raw[at + 2] = bytes[2];
    raw[at + 3] = bytes[3];
    raw
}

const fn build_default() -> [u8; TERMIOS_LEN] {
    let mut raw = [0u8; TERMIOS_LEN];
    raw = put(raw, 0, 0o2400);
    raw = put(raw, 4, 0o5);
    raw = put(raw, 8, 0o277);
    raw = put(raw, 12, 0o105073);
    let cc: [u8; 17] = [3, 0x1C, 0x7F, 0x15, 4, 0, 1, 0, 0x11, 0x13, 0x1A, 0, 0x12, 0x0F, 0x17, 0x16, 0];
    let mut i = 0;
    while i < cc.len() {
        raw[CC + i] = cc[i];
        i += 1;
    }
    raw
}

pub const DEFAULT_TERMIOS: [u8; TERMIOS_LEN] = build_default();

pub fn output_cooked() -> bool {
    task::with_current(|t| t.abi != task::Abi::Linux || field(&t.termios, 4) & (OPOST | ONLCR) == OPOST | ONLCR)
}

pub fn cook(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    for &b in data {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

pub fn signals_enabled(termios: &[u8; TERMIOS_LEN]) -> bool {
    field(termios, 12) & ISIG != 0
}

#[derive(Clone, Copy)]
enum Source {
    Console,
    Pipe(u32),
}

fn source(fd: u64) -> Option<Source> {
    match target(fd) {
        Target::Console => Some(Source::Console),
        Target::PipeRead(id) | Target::Pty(id, _) if pipe::terminal(id).is_some() => Some(Source::Pipe(id)),
        _ => None,
    }
}

fn field(raw: &[u8; TERMIOS_LEN], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}

pub fn get(arg: u64) -> i64 {
    let raw = task::with_current(|t| t.termios);
    copy_out(arg, TERMIOS_LEN as u64, &raw).min(0)
}

pub fn set(arg: u64, flush: bool) -> i64 {
    let raw: [u8; TERMIOS_LEN] = match user_slice(arg, TERMIOS_LEN as u64) {
        Ok(b) => (&*b).try_into().unwrap(),
        Err(e) => return e,
    };
    task::with_current(|t| {
        t.termios = raw;
        if flush {
            t.tty_pending.clear();
            t.tty_line.clear();
        }
    });
    0
}

fn key_bytes(key: Key, modes: &hxvt::Modes, out: &mut Vec<u8>) {
    let mods = keyboard::last_modifiers();
    let (vt_key, mods) = match key {
        Key::Char(c) => (hxvt::Key::Char(c), mods & keyboard::MOD_ALT),
        Key::Ctrl(c) => (hxvt::Key::Char(c), mods | keyboard::MOD_CTRL),
        Key::Enter => (hxvt::Key::Enter, mods & keyboard::MOD_ALT),
        Key::Backspace => (hxvt::Key::Backspace, mods),
        Key::Tab => (hxvt::Key::Tab, mods),
        Key::Escape => (hxvt::Key::Escape, 0),
        Key::Up | Key::SnapUp => (hxvt::Key::Up, mods),
        Key::Down | Key::SnapDown => (hxvt::Key::Down, mods),
        Key::Right | Key::SnapRight => (hxvt::Key::Right, mods),
        Key::Left | Key::SnapLeft => (hxvt::Key::Left, mods),
        Key::Home => (hxvt::Key::Home, mods),
        Key::End => (hxvt::Key::End, mods),
        Key::Delete => (hxvt::Key::Delete, mods),
        Key::Insert => (hxvt::Key::Insert, mods),
        Key::PageUp => (hxvt::Key::PageUp, mods),
        Key::PageDown => (hxvt::Key::PageDown, mods),
        Key::F(n) => (hxvt::Key::F(n), mods),
        Key::AltF4 => (hxvt::Key::F(4), keyboard::MOD_ALT),
        Key::AltTab => (hxvt::Key::Tab, keyboard::MOD_ALT),
        Key::Super => return,
    };
    out.extend_from_slice(&hxvt::encode_key(vt_key, mods, modes));
}

fn fill_pending(src: Source) -> bool {
    match src {
        Source::Console => {
            let vt = task::current_vt();
            let (mut bytes, modes) = {
                let mut console = TEXT_CONSOLE.lock();
                let injected = if crate::vt::input_allowed(task::current_pid(), vt) { console.take_input(vt) } else { Vec::new() };
                (injected, console.modes(vt))
            };
            let isig = task::with_current(|t| signals_enabled(&t.termios));
            while let Some(key) = keyboard::read_key() {
                if isig && matches!(key, Key::Ctrl('c')) {
                    continue;
                }
                key_bytes(key, &modes, &mut bytes);
            }
            if bytes.is_empty() {
                return false;
            }
            task::with_current(|t| t.tty_pending.extend(bytes));
            true
        }
        Source::Pipe(id) => {
            let mut buf = [0u8; 256];
            if pipe::available(id) == 0 {
                return false;
            }
            let n = pipe::read(id, &mut buf, true);
            if n <= 0 {
                return false;
            }
            let isig = task::with_current(|t| signals_enabled(&t.termios));
            let mut kept = Vec::with_capacity(n as usize);
            for &b in &buf[..n as usize] {
                let sig = match b {
                    0x03 if isig => Some(super::signal::SIGINT),
                    0x1c if isig => Some(super::signal::SIGQUIT),
                    0x1a if isig => Some(super::signal::SIGTSTP),
                    _ => None,
                };
                match sig {
                    Some(sig) => {
                        super::signal::send(task::current_pid(), sig);
                    }
                    None => kept.push(b),
                }
            }
            task::with_current(|t| t.tty_pending.extend(kept));
            true
        }
    }
}

fn pending_len() -> usize {
    task::with_current(|t| t.tty_pending.len())
}

fn at_eof(src: Source) -> bool {
    match src {
        Source::Console => false,
        Source::Pipe(id) => pipe::readable(id) && pipe::available(id) == 0,
    }
}

fn wait_input(src: Source, deadline: Option<u64>) -> bool {
    loop {
        if pending_len() > 0 || fill_pending(src) {
            return true;
        }
        if at_eof(src) || super::signal::pending() {
            return false;
        }
        let now = task::ticks();
        let slice = match deadline {
            Some(d) if now >= d => return false,
            Some(d) => (d - now).min(task::TICK_HZ / 10),
            None => task::TICK_HZ / 10,
        };
        let flag = match src {
            Source::Console => task::WAIT_INPUT,
            Source::Pipe(_) => task::WAIT_PIPE,
        };
        task::block(flag, Some(slice.max(1)), task::input_seq());
        task::check_killed();
        crate::vt::service_pending();
    }
}

fn take_byte() -> Option<u8> {
    task::with_current(|t| t.tty_pending.pop_front())
}

fn echo(src: Source, bytes: &[u8]) {
    let cooked = output_cooked();
    let mut visible = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\n' if cooked => visible.extend_from_slice(b"\r\n"),
            0x1b => visible.extend_from_slice(b"^["),
            b if b < 0x20 && b != b'\n' && b != b'\t' && b != 0x08 && b != b'\r' => {
                visible.push(b'^');
                visible.push(b + 0x40);
            }
            b => visible.push(b),
        }
    }
    match src {
        Source::Console => {
            let vt = task::current_vt();
            TEXT_CONSOLE.lock().write_raw_to(vt, &visible);
        }
        Source::Pipe(_) => {
            for fd in [1u64, 2] {
                if let Target::PipeWrite(id) | Target::Pty(_, id) = target(fd) {
                    if pipe::terminal(id).is_some() {
                        pipe::write(id, &visible, task::current_pid());
                        return;
                    }
                }
            }
        }
    }
}

fn deliver(buf: u64, count: u64, data: &[u8]) -> i64 {
    let n = data.len().min(count as usize);
    let r = copy_out(buf, n as u64, &data[..n]);
    if r < 0 {
        return r;
    }
    if n < data.len() {
        task::with_current(|t| {
            for &b in data[n..].iter().rev() {
                t.tty_line.push_front(b);
            }
        });
    }
    n as i64
}

fn read_canonical(src: Source, buf: u64, count: u64, termios: &[u8; TERMIOS_LEN]) -> i64 {
    let icrnl = field(termios, 0) & ICRNL != 0;
    let echoing = field(termios, 12) & ECHO != 0;
    let mut line: Vec<u8> = Vec::new();
    loop {
        if !wait_input(src, None) {
            if super::signal::pending() {
                task::with_current(|t| {
                    for &b in line.iter().rev() {
                        t.tty_pending.push_front(b);
                    }
                });
                return super::signal::EINTR;
            }
            break;
        }
        let Some(mut b) = take_byte() else {
            continue;
        };
        if b == b'\r' && icrnl {
            b = b'\n';
        }
        if b == termios[VERASE] || b == 0x08 {
            if line.pop().is_some() && echoing {
                echo(src, b"\x08 \x08");
            }
            continue;
        }
        if b == termios[VKILL] {
            if echoing {
                for _ in 0..line.len() {
                    echo(src, b"\x08 \x08");
                }
            }
            line.clear();
            continue;
        }
        if b == termios[VEOF] {
            if line.is_empty() {
                return 0;
            }
            break;
        }
        if b == 0x1B {
            while let Some(next) = task::with_current(|t| t.tty_pending.front().copied()) {
                if !(next == b'[' || next == b'O' || next.is_ascii_digit() || next == b';') {
                    if next.is_ascii_alphabetic() || next == b'~' {
                        take_byte();
                    }
                    break;
                }
                take_byte();
            }
            continue;
        }
        line.push(b);
        if echoing {
            echo(src, &[b]);
        }
        if b == b'\n' || line.len() >= MAX_LINE {
            break;
        }
    }
    deliver(buf, count, &line)
}

fn read_raw(src: Source, buf: u64, count: u64, termios: &[u8; TERMIOS_LEN]) -> i64 {
    let icrnl = field(termios, 0) & ICRNL != 0;
    let echoing = field(termios, 12) & ECHO != 0;
    let vmin = termios[VMIN];
    let vtime = termios[VTIME] as u64;
    let ready = match (vmin, vtime) {
        (0, 0) => pending_len() > 0 || fill_pending(src),
        (0, t) => wait_input(src, Some(task::ticks() + task::ms_to_ticks(t * 100))),
        _ => wait_input(src, None),
    };
    if !ready {
        return if super::signal::pending() { super::signal::EINTR } else { 0 };
    }
    fill_pending(src);
    let data: Vec<u8> = task::with_current(|t| {
        let n = t.tty_pending.len().min(count as usize);
        t.tty_pending.drain(..n).map(|b| if b == b'\r' && icrnl { b'\n' } else { b }).collect()
    });
    if echoing {
        echo(src, &data);
    }
    deliver(buf, count, &data)
}

pub fn read(fd: u64, buf: u64, count: u64) -> Option<i64> {
    let src = source(fd)?;
    if let Err(e) = user_slice(buf, count) {
        return Some(e);
    }
    if count == 0 {
        return Some(0);
    }
    let cooked: Vec<u8> = task::with_current(|t| {
        let n = t.tty_line.len().min(count as usize);
        t.tty_line.drain(..n).collect()
    });
    if !cooked.is_empty() {
        return Some(copy_out(buf, cooked.len() as u64, &cooked));
    }
    let termios = task::with_current(|t| t.termios);
    if crate::syscall::fd_nonblock(fd) && pending_len() == 0 && !fill_pending(src) {
        return Some(crate::syscall::EAGAIN);
    }
    Some(if field(&termios, 12) & ICANON != 0 { read_canonical(src, buf, count, &termios) } else { read_raw(src, buf, count, &termios) })
}

fn buffered() -> bool {
    task::with_current(|t| !t.tty_pending.is_empty() || !t.tty_line.is_empty())
}

pub fn rdhup(fd: u64, events: u32) -> u32 {
    const EPOLLRDHUP: u32 = 0x2000;
    if events & EPOLLRDHUP == 0 {
        return 0;
    }
    match target(fd) {
        Target::Socket(id, _) if crate::net::inet::is_inet(id) => {
            let r = crate::net::inet::readiness(id);
            if r.hangup || (r.readable && r.pending == 0 && !crate::net::inet::is_listening(id)) { EPOLLRDHUP } else { 0 }
        }
        Target::Socket(id, _) if crate::net::unix::readiness(id).readable && crate::net::unix::readiness(id).pending == 0 && !crate::net::unix::is_listening(id) => EPOLLRDHUP,
        _ => 0,
    }
}

pub fn sleep_for_events(slice: u64) {
    let slice = match super::special::next_deadline() {
        Some(d) => slice.min(d.saturating_sub(task::ticks()).max(1)),
        None => slice,
    };
    task::block(task::WAIT_INPUT | task::WAIT_PIPE | task::WAIT_MSG, Some(slice.max(1)), task::input_seq());
    task::check_killed();
    crate::vt::service_pending();
}

pub fn readiness(fd: u64, events: u16) -> u16 {
    let revents = match target(fd) {
        Target::Bad => return POLLNVAL,
        Target::File(..) | Target::Mem(..) | Target::Dir(_) | Target::Memfd(..) => POLLIN | POLLOUT,
        Target::Socket(id, _) if crate::net::inet::is_inet(id) => {
            let r = crate::net::inet::readiness(id);
            (if r.readable { POLLIN } else { 0 }) | (if r.writable { POLLOUT } else { 0 }) | (if r.hangup { POLLHUP } else { 0 }) | (if r.error { POLLERR } else { 0 })
        }
        Target::Socket(id, _) => {
            let r = crate::net::unix::readiness(id);
            (if r.readable { POLLIN } else { 0 }) | (if r.writable { POLLOUT } else { 0 }) | (if r.hangup { POLLHUP } else { 0 })
        }
        Target::Epoll(id) => {
            if super::epoll::ready_count(id) > 0 {
                POLLIN
            } else {
                0
            }
        }
        Target::Special(id) => (if super::special::readable(id) { POLLIN } else { 0 }) | (if super::special::writable(id) { POLLOUT } else { 0 }),
        Target::Mailbox => {
            if crate::task::ipc::has_message() {
                POLLIN
            } else {
                0
            }
        }
        Target::Console => {
            if events & POLLIN != 0 {
                fill_pending(Source::Console);
            }
            let input = buffered();
            POLLOUT | if input { POLLIN } else { 0 }
        }
        Target::Pty(input, _) => {
            let input_ready = pipe::available(input) > 0 || buffered();
            POLLOUT | if input_ready { POLLIN } else if pipe::readable(input) { POLLHUP } else { 0 }
        }
        Target::PtyMaster(_, output) => {
            POLLOUT | if pipe::available(output) > 0 { POLLIN } else if pipe::readable(output) { POLLHUP } else { 0 }
        }
        Target::PipeRead(id) => {
            let terminal = pipe::terminal(id).is_some();
            if pipe::available(id) > 0 || (terminal && buffered()) {
                POLLIN
            } else if pipe::readable(id) {
                POLLHUP
            } else {
                0
            }
        }
        Target::PipeWrite(id) => {
            if pipe::terminal(id).is_some() || pipe::available(id) < 64 * 1024 {
                POLLOUT
            } else {
                0
            }
        }
    };
    revents & (events | POLLERR | POLLHUP | POLLNVAL)
}

pub fn poll(fds: u64, nfds: u64, timeout_ms: i64) -> i64 {
    if nfds > 1024 {
        return EINVAL;
    }
    let len = nfds * 8;
    let entries: Vec<u8> = match user_slice(fds, len) {
        Ok(b) => b.to_vec(),
        Err(e) => return e,
    };
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    loop {
        let mut out = entries.clone();
        let mut ready = 0i64;
        for chunk in out.chunks_exact_mut(8) {
            let fd = i32::from_le_bytes(chunk[0..4].try_into().unwrap());
            let events = u16::from_le_bytes(chunk[4..6].try_into().unwrap());
            let revents = if fd < 0 { 0 } else { readiness(fd as u64, events) };
            chunk[6..8].copy_from_slice(&revents.to_le_bytes());
            if revents != 0 {
                ready += 1;
            }
        }
        if ready > 0 || timeout_ms == 0 || deadline.map(|d| task::ticks() >= d).unwrap_or(false) {
            let r = copy_out(fds, len, &out);
            return if r < 0 { r } else { ready };
        }
        if super::signal::pending() {
            return super::signal::EINTR;
        }
        let now = task::ticks();
        let slice = deadline.map(|d| d.saturating_sub(now)).unwrap_or(task::TICK_HZ / 10).clamp(1, task::TICK_HZ / 10);
        sleep_for_events(slice);
    }
}

pub fn ppoll(fds: u64, nfds: u64, timeout: u64) -> i64 {
    let ms = if timeout == 0 {
        -1
    } else {
        match user_slice(timeout, 16) {
            Ok(b) => {
                let secs = i64::from_le_bytes(b[0..8].try_into().unwrap());
                let nanos = i64::from_le_bytes(b[8..16].try_into().unwrap());
                if secs < 0 || !(0..1_000_000_000).contains(&nanos) {
                    return EINVAL;
                }
                secs.saturating_mul(1000).saturating_add((nanos + 999_999) / 1_000_000)
            }
            Err(e) => return e,
        }
    };
    poll(fds, nfds, ms)
}

fn read_set(ptr: u64, words: usize) -> Result<Option<Vec<u64>>, i64> {
    if ptr == 0 {
        return Ok(None);
    }
    let raw = user_slice(ptr, words as u64 * 8)?;
    Ok(Some(raw.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()))
}

fn write_set(ptr: u64, set: &[u64]) -> i64 {
    if ptr == 0 {
        return 0;
    }
    let raw: Vec<u8> = set.iter().flat_map(|w| w.to_le_bytes()).collect();
    copy_out(ptr, raw.len() as u64, &raw).min(0)
}

pub fn select(nfds: u64, readfds: u64, writefds: u64, exceptfds: u64, timeout_ms: i64) -> i64 {
    if nfds > 1024 {
        return EINVAL;
    }
    let words = (nfds as usize).div_ceil(64).max(1);
    let (reads, writes, excepts) = match (read_set(readfds, words), read_set(writefds, words), read_set(exceptfds, words)) {
        (Ok(r), Ok(w), Ok(e)) => (r, w, e),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return e,
    };
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    let has = |set: &Option<Vec<u64>>, fd: usize| set.as_ref().map(|s| s[fd / 64] & (1 << (fd % 64)) != 0).unwrap_or(false);
    loop {
        let mut out_r = alloc::vec![0u64; words];
        let mut out_w = alloc::vec![0u64; words];
        let out_e = alloc::vec![0u64; words];
        let mut count = 0i64;
        for fd in 0..nfds as usize {
            let want_r = has(&reads, fd);
            let want_w = has(&writes, fd);
            if !want_r && !want_w && !has(&excepts, fd) {
                continue;
            }
            let events = (if want_r { POLLIN } else { 0 }) | (if want_w { POLLOUT } else { 0 });
            let revents = readiness(fd as u64, events);
            if revents & POLLNVAL != 0 {
                return crate::syscall::EBADF;
            }
            if want_r && revents & (POLLIN | POLLHUP | POLLERR) != 0 {
                out_r[fd / 64] |= 1 << (fd % 64);
                count += 1;
            }
            if want_w && revents & (POLLOUT | POLLERR) != 0 {
                out_w[fd / 64] |= 1 << (fd % 64);
                count += 1;
            }
        }
        if count > 0 || timeout_ms == 0 || deadline.map(|d| task::ticks() >= d).unwrap_or(false) {
            for (ptr, set) in [(readfds, &out_r), (writefds, &out_w), (exceptfds, &out_e)] {
                let r = write_set(ptr, set);
                if r < 0 {
                    return r;
                }
            }
            return count;
        }
        if super::signal::pending() {
            return super::signal::EINTR;
        }
        let now = task::ticks();
        let slice = deadline.map(|d| d.saturating_sub(now)).unwrap_or(task::TICK_HZ / 10).clamp(1, task::TICK_HZ / 10);
        sleep_for_events(slice);
    }
}

pub fn sys_select(nfds: u64, readfds: u64, writefds: u64, exceptfds: u64, timeout: u64) -> i64 {
    let ms = if timeout == 0 {
        -1
    } else {
        match user_slice(timeout, 16) {
            Ok(b) => {
                let secs = i64::from_le_bytes(b[0..8].try_into().unwrap());
                let usecs = i64::from_le_bytes(b[8..16].try_into().unwrap());
                if secs < 0 || usecs < 0 {
                    return EINVAL;
                }
                secs.saturating_mul(1000).saturating_add((usecs + 999) / 1000)
            }
            Err(e) => return e,
        }
    };
    select(nfds, readfds, writefds, exceptfds, ms)
}

pub fn sys_pselect6(nfds: u64, readfds: u64, writefds: u64, exceptfds: u64, timeout: u64) -> i64 {
    let ms = if timeout == 0 {
        -1
    } else {
        match user_slice(timeout, 16) {
            Ok(b) => {
                let secs = i64::from_le_bytes(b[0..8].try_into().unwrap());
                let nanos = i64::from_le_bytes(b[8..16].try_into().unwrap());
                if secs < 0 || !(0..1_000_000_000).contains(&nanos) {
                    return EINVAL;
                }
                secs.saturating_mul(1000).saturating_add((nanos + 999_999) / 1_000_000)
            }
            Err(e) => return e,
        }
    };
    select(nfds, readfds, writefds, exceptfds, ms)
}
