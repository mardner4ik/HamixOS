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
static SHIFT_STATE: Mutex<bool> = Mutex::new(false);
static CAPS_STATE: Mutex<bool> = Mutex::new(false);

fn on_scancode(scancode: u8) {
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
    loop {
        let sc = KEY_QUEUE.lock().pop()?;

        if sc == 0xE0 {
            let sc2 = loop {
                if let Some(b) = KEY_QUEUE.lock().pop() {
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

        return match scancode_to_char(make, shift, caps) {
            Some('\n') => Some(Key::Enter),
            Some('\x08') => Some(Key::Backspace),
            Some('\t') => Some(Key::Tab),
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
        crate::arch::x86_64::hlt();
    }
}
