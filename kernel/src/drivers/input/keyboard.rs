use spin::Mutex;
use crate::arch::x86_64::idt::KEYBOARD_HANDLER;

const QUEUE_SIZE: usize = 256;

struct KeyQueue {
    buf: [u8; QUEUE_SIZE],
    head: usize,
    tail: usize,
}

impl KeyQueue {
    const fn new() -> Self {
        Self {
            buf: [0u8; QUEUE_SIZE],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        let next = (self.tail + 1) % QUEUE_SIZE;
        if next != self.head {
            self.buf[self.tail] = byte;
            self.tail = next;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let byte = self.buf[self.head];
        self.head = (self.head + 1) % QUEUE_SIZE;
        Some(byte)
    }
}

static KEY_QUEUE: Mutex<KeyQueue> = Mutex::new(KeyQueue::new());
static PASTE_QUEUE: Mutex<alloc::collections::VecDeque<char>> =
    Mutex::new(alloc::collections::VecDeque::new());

pub fn push_text(text: &str) {
    let mut queue = PASTE_QUEUE.lock();
    for ch in text.chars() {
        queue.push_back(ch);
    }
}
static SHIFT_STATE: Mutex<bool> = Mutex::new(false);
static CAPS_STATE: Mutex<bool> = Mutex::new(false);
static CTRL_STATE: Mutex<bool> = Mutex::new(false);

// Independent, minimal modifier tracking used *only* to recognize the
// Ctrl+Alt+F1..F6 workspace-switch hotkey at the moment the F-key scancode
// arrives (deliberately separate from SHIFT_STATE/CTRL_STATE above, which
// are consumed lazily by read_key() -- duplicating two booleans here is
// simpler and safer than restructuring that existing, working pipeline).
static HOTKEY_CTRL: Mutex<bool> = Mutex::new(false);
static HOTKEY_ALT: Mutex<bool> = Mutex::new(false);

// Scancode Set 1 make codes for F1..F6, and for the 'C' key (Ctrl+C).
const SC_F1: u8 = 0x3B;
const SC_F6: u8 = 0x40;
const SC_C: u8 = 0x2E;

fn on_scancode(scancode: u8) {
    match scancode {
        0x1D => *HOTKEY_CTRL.lock() = true,
        0x9D => *HOTKEY_CTRL.lock() = false,
        0x38 => *HOTKEY_ALT.lock() = true,
        0xB8 => *HOTKEY_ALT.lock() = false,
        _ => {}
    }
    if (SC_F1..=SC_F6).contains(&scancode) && *HOTKEY_CTRL.lock() && *HOTKEY_ALT.lock() {
        crate::vt::request_switch((scancode - SC_F1) as usize);
        return;
    }
    if scancode == SC_C && *HOTKEY_CTRL.lock() {
        crate::vt::request_kill();
    }
    KEY_QUEUE.lock().push(scancode);
}

pub fn init() {
    *KEYBOARD_HANDLER.lock() = Some(on_scancode);
}

fn scancode_to_char(sc: u8, shift: bool, caps: bool) -> Option<char> {
    let lower = [
        '\0', '\x1B', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\x08',
        '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n', '\0',
        'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`', '\0', '\\',
        'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', '\0', '*', '\0', ' ',
    ];
    let upper = [
        '\0', '\x1B', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '\x08',
        '\t', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}', '\n', '\0',
        'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L', ':', '"', '~', '\0', '|',
        'Z', 'X', 'C', 'V', 'B', 'N', 'M', '<', '>', '?', '\0', '*', '\0', ' ',
    ];

    let idx = sc as usize;
    if idx >= lower.len() {
        return None;
    }

    let use_upper = shift ^ caps;
    let ch = if use_upper { upper[idx] } else { lower[idx] };
    if ch == '\0' {
        None
    } else {
        Some(ch)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Backspace,
    Enter,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
}

fn extended_to_key(sc: u8) -> Option<Key> {
    match sc {
        0x48 => Some(Key::Up),
        0x50 => Some(Key::Down),
        0x4B => Some(Key::Left),
        0x4D => Some(Key::Right),
        0x47 => Some(Key::Home),
        0x4F => Some(Key::End),
        0x53 => Some(Key::Delete),
        _ => None,
    }
}

pub fn read_key() -> Option<Key> {
    if !crate::vt::is_foreground_task() {
        return None;
    }
    if let Some(ch) = crate::arch::x86_64::without_interrupts(|| PASTE_QUEUE.lock().pop_front()) {
        return Some(match ch {
            '\n' | '\r' => Key::Enter,
            '\t' => Key::Tab,
            other => Key::Char(other),
        });
    }
    loop {
        // read_key() now runs both at ring0 (interrupts always on) and
        // inside syscalls (interrupts on for the duration, see
        // syscall::syscall_entry's sti/cli). KEY_QUEUE is also written
        // from on_scancode, which runs *inside* the keyboard IRQ handler.
        // Without this guard, a keyboard interrupt landing exactly while
        // this pop() holds the lock would spin forever trying to take a
        // lock the interrupted code can't release until the handler
        // returns -- a classic single-core spinlock-in-IRQ deadlock, and
        // with a timer tick re-entering this loop at high frequency while
        // blocked waiting for a key, it was hitting basically every time.
        let sc = crate::arch::x86_64::without_interrupts(|| KEY_QUEUE.lock().pop())?;

        if sc == 0xE0 {
            let sc2 = loop {
                if let Some(b) = crate::arch::x86_64::without_interrupts(|| KEY_QUEUE.lock().pop()) {
                    break b;
                }
                crate::arch::x86_64::hlt();
            };
            let is_break = sc2 & 0x80 != 0;
            let make = sc2 & 0x7F;
            if is_break {
                continue;
            }
            match extended_to_key(make) {
                Some(key) => return Some(key),
                None => continue,
            }
        }

        let is_break = sc & 0x80 != 0;
        let make = sc & 0x7F;

        match make {
            0x2A | 0x36 => {
                *SHIFT_STATE.lock() = !is_break;
                continue;
            }
            0x1D => {
                *CTRL_STATE.lock() = !is_break;
                continue;
            }
            0x3A if !is_break => {
                let mut caps = CAPS_STATE.lock();
                *caps = !*caps;
                continue;
            }
            _ => {}
        }

        if is_break {
            continue;
        }

        let shift = *SHIFT_STATE.lock();
        let caps = *CAPS_STATE.lock();
        let ctrl = *CTRL_STATE.lock();

        return match scancode_to_char(make, shift, caps) {
            Some('\n') => Some(Key::Enter),
            Some('\x08') => Some(Key::Backspace),
            Some('\t') => Some(Key::Tab),
            Some(ch) if ctrl && ch.is_ascii_alphabetic() => Some(Key::Ctrl(ch.to_ascii_lowercase())),
            Some(ch) => Some(Key::Char(ch)),
            None => continue,
        };
    }
}

pub fn read_key_blocking() -> Key {
    loop {
        if let Some(key) = read_key() {
            return key;
        }
        crate::vt::service_pending_foreground_switch();
        crate::vt::yield_to_next();
        crate::arch::x86_64::hlt();
    }
}

pub fn read_key_blocking_ring3() -> Key {
    loop {
        if crate::vt::kill_pending() {
            crate::vt::terminate_current_ring3();
        }
        if let Some(key) = read_key() {
            return key;
        }
        crate::vt::service_pending_foreground_switch();
        crate::vt::yield_to_next();
        crate::arch::x86_64::hlt();
    }
}
