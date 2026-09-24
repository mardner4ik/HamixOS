#![no_std]

extern crate alloc;

pub mod ui;

use alloc::collections::VecDeque;
use hamix_std::sys;
pub use hamix_std::sys::Key;
pub use hxproto::{PICK_CANCELLED, PICK_FAILED, PICK_FOLDER, PICK_OK, PICK_OPEN_FILE, PICK_SAVE_FILE};
pub use hxproto::{Event, CURSOR_ARROW, CURSOR_HAND, CURSOR_MOVE, CURSOR_RESIZE_EW, CURSOR_RESIZE_NESW, CURSOR_RESIZE_NS, CURSOR_RESIZE_NWSE, CURSOR_TEXT, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_RELEASE, MOUSE_WHEEL};
use hxproto::{Request, Text, Title, MESSAGE_MAX};
pub use vellum::{Area, Font, Image, Painter};
use vellum::{Canvas, Color, Rect};

pub struct Window {
    server: i64,
    id: u32,
    shm: u32,
    capacity: usize,
    width: u32,
    height: u32,
    pixels: *mut u32,
    pending: VecDeque<Event>,
    closed: bool,
    cursor: u32,
}

fn send(server: i64, request: Request) -> bool {
    let mut buf = [0u8; MESSAGE_MAX];
    let len = request.encode(&mut buf);
    sys::msg_send(server, &buf[..len]) >= 0
}

pub fn server() -> Option<i64> {
    for _ in 0..50 {
        let pid = sys::service_lookup(hxproto::SERVICE);
        if pid > 0 {
            return Some(pid);
        }
        sys::sleep_ms(20);
    }
    None
}

pub fn notify(title: &str, body: &str) -> bool {
    match server() {
        Some(pid) => send(pid, Request::Notify { title: Title::new(title), body: Title::new(body) }),
        None => false,
    }
}

pub fn reload_desktop() -> bool {
    match server() {
        Some(pid) => send(pid, Request::Reload),
        None => false,
    }
}

pub fn display_changed() -> bool {
    match server() {
        Some(pid) => send(pid, Request::DisplayChanged),
        None => false,
    }
}

static NEXT_REQUEST: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

static WINDOW_WIDTH: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
static WINDOW_HEIGHT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

pub fn window_width() -> i32 {
    WINDOW_WIDTH.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn window_height() -> i32 {
    WINDOW_HEIGHT.load(core::sync::atomic::Ordering::Relaxed)
}

fn record_size(width: u32, height: u32) {
    WINDOW_WIDTH.store(width as i32, core::sync::atomic::Ordering::Relaxed);
    WINDOW_HEIGHT.store(height as i32, core::sync::atomic::Ordering::Relaxed);
}

fn round_up(value: u32, step: u32) -> u32 {
    value.div_ceil(step) * step
}

impl Window {
    pub fn open(title: &str, width: u32, height: u32) -> Result<Window, &'static str> {
        let server = server().ok_or("hxserver is not running (start it with startx)")?;
        let bytes = width as usize * height as usize * 4;
        let shm = sys::shm_create(bytes);
        if shm < 0 {
            return Err("out of memory for the window buffer");
        }
        let shm = shm as u32;
        let (ptr, _) = sys::shm_map(shm).ok_or("cannot map the window buffer")?;
        if !send(server, Request::CreateWindow { shm, width, height, title: Title::new(title) }) {
            return Err("hxserver refused the connection");
        }
        let mut pending = VecDeque::new();
        let mut buf = [0u8; MESSAGE_MAX];
        for _ in 0..100 {
            match sys::msg_recv(&mut buf, 50) {
                Some((sender, len)) if sender == server => match Event::decode(&buf[..len]) {
                    Some(Event::Created { window }) => {
                        record_size(width, height);
                        return Ok(Window { server, id: window, shm, capacity: bytes, width, height, pixels: ptr as *mut u32, pending, closed: false, cursor: CURSOR_ARROW });
                    }
                    Some(Event::Rejected { .. }) => return Err("hxserver rejected the window"),
                    Some(other) => pending.push_back(other),
                    None => {}
                },
                Some(_) => {}
                None => {
                    if !sys::proc_alive(server) {
                        return Err("hxserver exited");
                    }
                }
            }
        }
        Err("hxserver did not answer")
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn buffer(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.pixels, self.width as usize * self.height as usize) }
    }

    pub fn painter(&mut self) -> Painter<'_> {
        let (w, h) = (self.width as i32, self.height as i32);
        Painter::new(self.buffer(), w, h)
    }

    pub fn present(&mut self) {
        self.present_rect(0, 0, self.width, self.height);
    }

    pub fn present_area(&mut self, area: Area) {
        let a = area.intersect(&Area::new(0, 0, self.width as i32, self.height as i32));
        if !a.is_empty() {
            self.present_rect(a.x as u32, a.y as u32, a.w as u32, a.h as u32);
        }
    }

    pub fn present_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if !self.closed {
            send(self.server, Request::Present { window: self.id, x, y, width, height });
        }
    }

    pub fn set_size_hints(&mut self, min_width: u32, min_height: u32, max_width: u32, max_height: u32) {
        send(self.server, Request::SizeHints { window: self.id, min_width, min_height, max_width, max_height });
    }

    pub fn set_min_size(&mut self, min_width: u32, min_height: u32) {
        self.set_size_hints(min_width, min_height, 0, 0);
    }

    pub fn set_fixed_size(&mut self) {
        let (w, h) = (self.width, self.height);
        self.set_size_hints(w, h, w, h);
    }

    fn apply_resize(&mut self, width: u32, height: u32) -> bool {
        let width = width.clamp(16, 8192);
        let height = height.clamp(16, 8192);
        if width == self.width && height == self.height {
            return false;
        }
        let needed = width as usize * height as usize * 4;
        record_size(width, height);
        if needed <= self.capacity {
            self.width = width;
            self.height = height;
            send(self.server, Request::Attach { window: self.id, shm: self.shm, width, height });
            return true;
        }
        let capacity = round_up(width, 256) as usize * round_up(height, 256) as usize * 4;
        let shm = sys::shm_create(capacity);
        if shm < 0 {
            return false;
        }
        let shm = shm as u32;
        let Some((ptr, _)) = sys::shm_map(shm) else {
            sys::shm_release(shm);
            return false;
        };
        let old = self.shm;
        self.shm = shm;
        self.capacity = capacity;
        self.pixels = ptr as *mut u32;
        self.width = width;
        self.height = height;
        send(self.server, Request::Attach { window: self.id, shm, width, height });
        sys::shm_release(old);
        true
    }

    pub fn request_file(&mut self, mode: u32, title: &str, filters: &str, start: &str) -> u32 {
        let request = NEXT_REQUEST.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        send(self.server, Request::ChooseFile { request, mode, title: Title::new(title), filters: Text::new(filters), start: Text::new(start) });
        request
    }

    pub fn choose(&mut self, mode: u32, title: &str, filters: &str, start: &str) -> Option<alloc::string::String> {
        let request = self.request_file(mode, title, filters, start);
        let mut buf = [0u8; MESSAGE_MAX];
        loop {
            match sys::msg_recv(&mut buf, 1000) {
                Some((sender, len)) if sender == self.server => match Event::decode(&buf[..len]) {
                    Some(Event::FileChosen { request: r, status, path }) if r == request => {
                        return if status == PICK_OK { Some(alloc::string::String::from(path.as_str())) } else { None };
                    }
                    Some(Event::Close { window }) => {
                        self.closed = true;
                        self.pending.push_back(Event::Close { window });
                        return None;
                    }
                    Some(other) => self.pending.push_back(other),
                    None => {}
                },
                Some(_) => {}
                None => {
                    if !sys::proc_alive(self.server) {
                        self.closed = true;
                        return None;
                    }
                }
            }
        }
    }

    pub fn open_file_dialog(&mut self, title: &str, filters: &str, start: &str) -> Option<alloc::string::String> {
        self.choose(PICK_OPEN_FILE, title, filters, start)
    }

    pub fn choose_folder_dialog(&mut self, title: &str, start: &str) -> Option<alloc::string::String> {
        self.choose(PICK_FOLDER, title, "", start)
    }

    pub fn save_file_dialog(&mut self, title: &str, filters: &str, suggested: &str) -> Option<alloc::string::String> {
        self.choose(PICK_SAVE_FILE, title, filters, suggested)
    }

    pub fn set_maximized(&mut self, maximized: bool) {
        send(self.server, Request::SetMaximized { window: self.id, maximized: maximized as u32 });
    }

    pub fn set_title(&mut self, title: &str) {
        send(self.server, Request::SetTitle { window: self.id, title: Title::new(title) });
    }

    pub fn set_icon(&mut self, name: &str) {
        send(self.server, Request::SetIcon { window: self.id, name: Title::new(name) });
    }

    pub fn set_cursor(&mut self, cursor: u32) {
        if cursor != self.cursor {
            self.cursor = cursor;
            send(self.server, Request::SetCursor { window: self.id, cursor });
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.wait_event(0)
    }

    fn deliver(&mut self, event: Event) -> Event {
        if let Event::Theme { light } = event {
            ui::theme::set_light(light);
            return event;
        }
        if let Event::Resize { window, width, height } = event {
            self.apply_resize(width, height);
            return Event::Resize { window, width: self.width, height: self.height };
        }
        event
    }

    pub fn wait_event(&mut self, timeout_ms: i64) -> Option<Event> {
        if let Some(event) = self.pending.pop_front() {
            return Some(self.deliver(event));
        }
        if self.closed {
            return None;
        }
        if timeout_ms < 0 || timeout_ms >= 500 {
            hamix_std::heap::release_cached();
        }
        let mut buf = [0u8; MESSAGE_MAX];
        let deadline = sys::uptime_ms() + timeout_ms.max(0) as u64;
        loop {
            let now = sys::uptime_ms();
            let slice = if timeout_ms < 0 { 1000 } else { deadline.saturating_sub(now).min(1000) as i64 };
            match sys::msg_recv(&mut buf, slice) {
                Some((sender, len)) if sender == self.server => {
                    if let Some(event) = Event::decode(&buf[..len]) {
                        if let Event::Close { .. } = event {
                            self.closed = true;
                        }
                        if let Event::Mouse { kind: MOUSE_MOVE, .. } = event {
                            return Some(self.coalesce(event));
                        }
                        if let Event::Resize { .. } = event {
                            let latest = self.coalesce(event);
                            return Some(self.deliver(latest));
                        }
                        return Some(self.deliver(event));
                    }
                }
                Some(_) => {}
                None => {
                    if !sys::proc_alive(self.server) {
                        self.closed = true;
                        return Some(Event::Close { window: self.id });
                    }
                    if timeout_ms >= 0 && sys::uptime_ms() >= deadline {
                        return None;
                    }
                }
            }
        }
    }

    fn coalesce(&mut self, mut latest: Event) -> Event {
        let mut buf = [0u8; MESSAGE_MAX];
        while let Some((sender, len)) = sys::msg_recv(&mut buf, 0) {
            if sender != self.server {
                continue;
            }
            match Event::decode(&buf[..len]) {
                Some(event @ Event::Mouse { kind: MOUSE_MOVE, .. }) if matches!(latest, Event::Mouse { .. }) => latest = event,
                Some(event @ Event::Resize { .. }) if matches!(latest, Event::Resize { .. }) => latest = event,
                Some(other) => {
                    if let Event::Close { .. } = other {
                        self.closed = true;
                    }
                    self.pending.push_back(other);
                    break;
                }
                None => {}
            }
        }
        latest
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if !self.closed {
            send(self.server, Request::Destroy { window: self.id });
        }
        sys::shm_release(self.shm);
    }
}

impl Canvas for Window {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            let w = self.width as usize;
            self.buffer()[y as usize * w + x as usize] = color.as_u32();
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let clamped = rect.clamp_to(self.width, self.height);
        let w = self.width as usize;
        let value = color.as_u32();
        let buffer = self.buffer();
        for y in clamped.y..clamped.y + clamped.height {
            let start = y as usize * w + clamped.x as usize;
            buffer[start..start + clamped.width as usize].fill(value);
        }
    }
}
