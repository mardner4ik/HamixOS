use spin::Mutex;

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
static SUPER_STATE: Mutex<bool> = Mutex::new(false);
static SUPER_USED: Mutex<bool> = Mutex::new(false);
static ALT_STATE: Mutex<bool> = Mutex::new(false);

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

static EXTENDED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn on_scancode(scancode: u8) {
    let extended = EXTENDED.swap(scancode == 0xE0, core::sync::atomic::Ordering::Relaxed);
    if extended && scancode & 0x80 == 0 && crate::drivers::audio::hotkey(scancode) {
        KEY_QUEUE.lock().push(scancode);
        crate::task::notify_input();
        return;
    }
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
    crate::task::notify_input();
}

pub fn inject_scancode(scancode: u8) {
    on_scancode(scancode);
}

pub fn has_pending() -> bool {
    crate::arch::without_interrupts(|| {
        let queue = KEY_QUEUE.lock();
        queue.head != queue.tail || !PASTE_QUEUE.lock().is_empty()
    })
}

pub fn init() {
    #[cfg(target_arch = "x86_64")]
    {
        *crate::arch::x86_64::idt::KEYBOARD_HANDLER.lock() = Some(on_scancode);
    }
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

    let letter = lower[idx].is_ascii_alphabetic();
    let use_upper = if letter { shift ^ caps } else { shift };
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
    PageUp,
    PageDown,
    Escape,
    AltTab,
    Super,
    AltF4,
    SnapLeft,
    SnapRight,
    SnapUp,
    SnapDown,
    Insert,
    F(u8),
}

pub const MOD_SHIFT: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_CTRL: u8 = 4;

static LAST_MODS: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub fn last_modifiers() -> u8 {
    LAST_MODS.load(core::sync::atomic::Ordering::Relaxed)
}

fn current_modifiers() -> u8 {
    let mut mods = 0;
    if *SHIFT_STATE.lock() {
        mods |= MOD_SHIFT;
    }
    if *ALT_STATE.lock() {
        mods |= MOD_ALT;
    }
    if *CTRL_STATE.lock() {
        mods |= MOD_CTRL;
    }
    mods
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
        0x49 => Some(Key::PageUp),
        0x51 => Some(Key::PageDown),
        0x52 => Some(Key::Insert),
        _ => None,
    }
}

pub fn read_key() -> Option<Key> {
    if !crate::vt::input_allowed(crate::task::current_pid(), crate::task::current_vt()) {
        return None;
    }
    let key = decode_key();
    if key.is_some() {
        LAST_MODS.store(current_modifiers(), core::sync::atomic::Ordering::Relaxed);
    }
    key
}

fn decode_key() -> Option<Key> {
    if let Some(ch) = crate::arch::without_interrupts(|| PASTE_QUEUE.lock().pop_front()) {
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
        let sc = crate::arch::without_interrupts(|| KEY_QUEUE.lock().pop())?;

        if sc == 0xE0 {
            let sc2 = loop {
                if let Some(b) = crate::arch::without_interrupts(|| KEY_QUEUE.lock().pop()) {
                    break b;
                }
                crate::arch::hlt();
            };
            let is_break = sc2 & 0x80 != 0;
            let make = sc2 & 0x7F;
            if make == 0x38 {
                *ALT_STATE.lock() = !is_break;
                continue;
            }
            if make == 0x1D {
                *CTRL_STATE.lock() = !is_break;
                continue;
            }
            if make == 0x5B || make == 0x5C {
                if is_break {
                    *SUPER_STATE.lock() = false;
                    let used = core::mem::replace(&mut *SUPER_USED.lock(), false);
                    if used {
                        continue;
                    }
                    return Some(Key::Super);
                }
                *SUPER_STATE.lock() = true;
                *SUPER_USED.lock() = false;
                continue;
            }
            if is_break {
                continue;
            }
            match extended_to_key(make) {
                Some(key) => {
                    if *SUPER_STATE.lock() {
                        *SUPER_USED.lock() = true;
                        match key {
                            Key::Left => return Some(Key::SnapLeft),
                            Key::Right => return Some(Key::SnapRight),
                            Key::Up => return Some(Key::SnapUp),
                            Key::Down => return Some(Key::SnapDown),
                            _ => {}
                        }
                    }
                    return Some(key);
                }
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
            0x38 => {
                *ALT_STATE.lock() = !is_break;
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
        let alt = *ALT_STATE.lock();
        if alt && make == 0x0F {
            return Some(Key::AltTab);
        }
        if alt && make == 0x3E {
            return Some(Key::AltF4);
        }
        match make {
            0x3B..=0x44 => return Some(Key::F(make - 0x3A)),
            0x57 => return Some(Key::F(11)),
            0x58 => return Some(Key::F(12)),
            _ => {}
        }

        if *SUPER_STATE.lock() {
            *SUPER_USED.lock() = true;
        }
        return match scancode_to_char(make, shift, caps) {
            Some('\n') => Some(Key::Enter),
            Some('\x08') => Some(Key::Backspace),
            Some('\t') => Some(Key::Tab),
            Some('\x1B') => Some(Key::Escape),
            Some(ch) if ctrl && ch.is_ascii_alphabetic() => Some(Key::Ctrl(ch.to_ascii_lowercase())),
            Some(ch) => Some(Key::Char(ch)),
            None => continue,
        };
    }
}

pub fn read_key_blocking() -> Key {
    loop {
        let seq = crate::task::input_seq();
        crate::vt::service_pending();
        if let Some(key) = read_key() {
            return key;
        }
        crate::task::block(crate::task::WAIT_INPUT, Some(crate::task::TICK_HZ / 4), seq);
    }
}
