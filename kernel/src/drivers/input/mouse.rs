use spin::Mutex;

use crate::arch::io::{inb, outb};
use crate::arch::without_interrupts;

const PS2_DATA: u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_COMMAND: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 1;
const STATUS_INPUT_FULL: u8 = 2;

const CMD_ENABLE_AUX: u8 = 0xA8;
const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_WRITE_AUX: u8 = 0xD4;

pub const AUX_SET_DEFAULTS: u8 = 0xF6;
const AUX_ENABLE_REPORTING: u8 = 0xF4;
const AUX_SET_SAMPLE_RATE: u8 = 0xF3;
const AUX_GET_DEVICE_ID: u8 = 0xF2;
pub const AUX_ACK: u8 = 0xFA;

pub const BUTTON_LEFT: u8 = 1;
pub const BUTTON_RIGHT: u8 = 2;
pub const BUTTON_MIDDLE: u8 = 4;

#[derive(Clone, Copy, Default)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    pub wheel: i32,
    pub presses: [u8; 3],
}

#[derive(Clone, Copy, Default)]
pub struct MouseEvent {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    pub pressed: u8,
    pub released: u8,
    pub wheel: i32,
}

struct Packet {
    bytes: [u8; 4],
    len: usize,
    size: usize,
}

static PACKET: Mutex<Packet> = Mutex::new(Packet { bytes: [0; 4], len: 0, size: 3 });
static STATE: Mutex<MouseState> = Mutex::new(MouseState { x: 0, y: 0, buttons: 0, wheel: 0, presses: [0; 3] });
static BOUNDS: Mutex<(i32, i32)> = Mutex::new((640, 200));
static PRESENT: Mutex<bool> = Mutex::new(false);

const EVENT_QUEUE_SIZE: usize = 64;

struct EventQueue {
    buf: [MouseEvent; EVENT_QUEUE_SIZE],
    head: usize,
    tail: usize,
}

impl EventQueue {
    const fn new() -> Self {
        Self {
            buf: [MouseEvent { x: 0, y: 0, buttons: 0, pressed: 0, released: 0, wheel: 0 };
                EVENT_QUEUE_SIZE],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, ev: MouseEvent) {
        let next = (self.tail + 1) % EVENT_QUEUE_SIZE;
        if next != self.head {
            self.buf[self.tail] = ev;
            self.tail = next;
        }
    }

    fn pop(&mut self) -> Option<MouseEvent> {
        if self.head == self.tail {
            return None;
        }
        let ev = self.buf[self.head];
        self.head = (self.head + 1) % EVENT_QUEUE_SIZE;
        Some(ev)
    }
}

static EVENTS: Mutex<EventQueue> = Mutex::new(EventQueue::new());

fn wait_input_clear() -> bool {
    for _ in 0..200_000 {
        if inb(PS2_STATUS) & STATUS_INPUT_FULL == 0 {
            return true;
        }
    }
    false
}

fn wait_output_full() -> bool {
    for _ in 0..200_000 {
        if inb(PS2_STATUS) & STATUS_OUTPUT_FULL != 0 {
            return true;
        }
    }
    false
}

fn command(value: u8) {
    wait_input_clear();
    outb(PS2_COMMAND, value);
}

fn write_data(value: u8) {
    wait_input_clear();
    outb(PS2_DATA, value);
}

pub fn read_data() -> u8 {
    if !wait_output_full() {
        return 0;
    }
    inb(PS2_DATA)
}

pub fn aux_command(value: u8) -> u8 {
    command(CMD_WRITE_AUX);
    write_data(value);
    read_data()
}

fn set_sample_rate(rate: u8) {
    aux_command(AUX_SET_SAMPLE_RATE);
    aux_command(rate);
}

pub fn init() {
    let (w, h) = match *crate::memory::FRAMEBUFFER.lock() {
        Some(fb) => (fb.width as i32, fb.height as i32),
        None => (
            (crate::drivers::video::text_mode::COLS * 8) as i32,
            (crate::drivers::video::text_mode::ROWS * 8) as i32,
        ),
    };
    *BOUNDS.lock() = (w, h);
    {
        let mut state = STATE.lock();
        state.x = w / 2;
        state.y = h / 2;
    }

    command(CMD_ENABLE_AUX);

    command(CMD_READ_CONFIG);
    let mut config = read_data();
    config |= 0x02;
    config &= !0x20;
    command(CMD_WRITE_CONFIG);
    write_data(config);

    if aux_command(AUX_SET_DEFAULTS) != AUX_ACK {
        crate::drivers::klog::log("mouse: no PS/2 pointing device responded");
        return;
    }

    if let Some(description) = super::touchpad::detect() {
        aux_command(AUX_ENABLE_REPORTING);
        *PRESENT.lock() = true;
        #[cfg(target_arch = "x86_64")]
        {
            *crate::arch::x86_64::idt::MOUSE_HANDLER.lock() = Some(super::touchpad::on_byte);
        }
        crate::drivers::klog::log(&alloc::format!("mouse: {}", description));
        return;
    }

    set_sample_rate(200);
    set_sample_rate(100);
    set_sample_rate(80);
    aux_command(AUX_GET_DEVICE_ID);
    let id = read_data();

    let size = if id == 3 || id == 4 { 4 } else { 3 };
    PACKET.lock().size = size;

    aux_command(AUX_ENABLE_REPORTING);
    *PRESENT.lock() = true;
    #[cfg(target_arch = "x86_64")]
    {
        *crate::arch::x86_64::idt::MOUSE_HANDLER.lock() = Some(on_byte);
    }

    crate::drivers::klog::log(&alloc::format!(
        "mouse: PS/2 pointing device id {} ({}-byte packets{})",
        id,
        size,
        if size == 4 { ", scroll wheel" } else { "" }
    ));
}

pub fn present() -> bool {
    without_interrupts(|| *PRESENT.lock())
}

pub fn set_bounds(w: i32, h: i32) {
    *BOUNDS.lock() = (w.max(1), h.max(1));
    let mut state = STATE.lock();
    state.x = state.x.clamp(0, w - 1);
    state.y = state.y.clamp(0, h - 1);
}

pub fn state() -> MouseState {
    without_interrupts(|| *STATE.lock())
}

pub fn take_state() -> MouseState {
    without_interrupts(|| {
        let mut state = STATE.lock();
        let snapshot = *state;
        state.wheel = 0;
        state.presses = [0; 3];
        snapshot
    })
}

pub fn next_event() -> Option<MouseEvent> {
    without_interrupts(|| EVENTS.lock().pop())
}

pub fn drain_events() {
    without_interrupts(|| {
        let mut q = EVENTS.lock();
        while q.pop().is_some() {}
    });
}

pub fn mark_present() {
    *PRESENT.lock() = true;
}

pub fn inject_absolute(x: i32, y: i32, buttons: u8, wheel: i32) {
    let (cx, cy) = {
        let state = STATE.lock();
        (state.x, state.y)
    };
    inject(x - cx, y - cy, buttons, wheel);
}

pub fn bounds() -> (i32, i32) {
    *BOUNDS.lock()
}

pub fn inject(dx: i32, dy: i32, buttons: u8, wheel: i32) {
    *PRESENT.lock() = true;
    let (max_x, max_y) = *BOUNDS.lock();
    let (x, y, pressed, released) = {
        let mut state = STATE.lock();
        let previous = state.buttons;
        state.x = (state.x + dx).clamp(0, max_x - 1);
        state.y = (state.y + dy).clamp(0, max_y - 1);
        for bit in 0..3 {
            if buttons & !previous & (1 << bit) != 0 {
                state.presses[bit] = state.presses[bit].saturating_add(1);
            }
        }
        state.buttons = buttons;
        state.wheel += wheel;
        (state.x, state.y, buttons & !previous, previous & !buttons)
    };
    let event = MouseEvent { x, y, buttons, pressed, released, wheel };
    EVENTS.lock().push(event);
    crate::task::notify_input();
    crate::drivers::video::console_mouse::on_event(event);
}

fn on_byte(byte: u8) {
    let mut packet = PACKET.lock();

    if packet.len == 0 && byte & 0x08 == 0 {
        return;
    }

    let position = packet.len;
    packet.bytes[position] = byte;
    packet.len += 1;
    if packet.len < packet.size {
        return;
    }

    let bytes = packet.bytes;
    packet.len = 0;
    let size = packet.size;
    drop(packet);

    let flags = bytes[0];
    if flags & 0xC0 != 0 {
        return;
    }

    let mut dx = bytes[1] as i32;
    let mut dy = bytes[2] as i32;
    if flags & 0x10 != 0 {
        dx -= 256;
    }
    if flags & 0x20 != 0 {
        dy -= 256;
    }

    let wheel = if size == 4 {
        let z = (bytes[3] & 0x0F) as i8;
        if z & 0x08 != 0 { (z | 0xF0u8 as i8) as i32 } else { z as i32 }
    } else {
        0
    };

    let buttons = flags & 0x07;
    let (max_x, max_y) = *BOUNDS.lock();

    let (x, y, pressed, released) = {
        let mut state = STATE.lock();
        let previous = state.buttons;
        state.x = (state.x + dx).clamp(0, max_x - 1);
        state.y = (state.y - dy).clamp(0, max_y - 1);
        for bit in 0..3 {
            if buttons & !previous & (1 << bit) != 0 {
                state.presses[bit] = state.presses[bit].saturating_add(1);
            }
        }
        state.buttons = buttons;
        state.wheel += wheel;
        (state.x, state.y, buttons & !previous, previous & !buttons)
    };

    EVENTS.lock().push(MouseEvent { x, y, buttons, pressed, released, wheel });
    crate::task::notify_input();

    crate::drivers::video::console_mouse::on_event(MouseEvent {
        x,
        y,
        buttons,
        pressed,
        released,
        wheel,
    });
}
