use alloc::collections::VecDeque;
use crate::{Event, Request, Text, Title};

use super::connection::Connection;
use super::posix::{self, SharedMemory};

pub struct Surface {
    pub id: u32,
    shm: SharedMemory,
    pub width: u32,
    pub height: u32,
}

impl Surface {
    pub fn buffer(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.shm.addr as *mut u32, self.width as usize * self.height as usize) }
    }
}

pub struct Window {
    pub conn: Connection,
    pub id: u32,
    shm: SharedMemory,
    pub width: u32,
    pub height: u32,
    pub pending: VecDeque<Event>,
    cursor: u32,
    next_request: u32,
}

fn round_up(v: u32, step: u32) -> u32 {
    v.div_ceil(step) * step
}

impl Window {
    pub fn open(title: &str, width: u32, height: u32) -> Result<Window, &'static str> {
        let conn = Connection::open().ok_or("hxserver is not running")?;
        let shm = SharedMemory::create(width as usize * height as usize * 4).ok_or("cannot allocate the window buffer")?;
        if !conn.send(Request::CreateWindow { shm: shm.id, width, height, title: Title::new(title) }) {
            return Err("hxserver refused the connection");
        }
        let mut pending = VecDeque::new();
        let deadline = posix::now_ms() + 5000;
        while posix::now_ms() < deadline {
            conn.wait(50);
            while let Some(event) = conn.poll_event() {
                match event {
                    Event::Created { window } => {
                        return Ok(Window { conn, id: window, shm, width, height, pending, cursor: 0, next_request: 1 });
                    }
                    Event::Rejected { .. } => return Err("hxserver rejected the window"),
                    other => pending.push_back(other),
                }
            }
            if !conn.server_alive() {
                return Err("hxserver exited");
            }
        }
        Err("hxserver did not answer")
    }

    pub fn buffer(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.shm.addr as *mut u32, self.width as usize * self.height as usize) }
    }

    pub fn present(&self) {
        self.conn.send(Request::Present { window: self.id, x: 0, y: 0, width: self.width, height: self.height });
    }

    pub fn present_area(&self, x: u32, y: u32, width: u32, height: u32) {
        self.conn.send(Request::Present { window: self.id, x, y, width, height });
    }

    pub fn set_title(&self, title: &str) {
        self.conn.send(Request::SetTitle { window: self.id, title: Title::new(title) });
    }

    pub fn set_icon(&self, name: &str) {
        self.conn.send(Request::SetIcon { window: self.id, name: Title::new(name) });
    }

    pub fn set_app_id(&self, app_id: &str) {
        self.conn.send(Request::SetAppId { window: self.id, pid: posix::pid() as u32, app_id: Title::new(app_id) });
    }

    pub fn set_cursor(&mut self, cursor: u32) {
        if cursor != self.cursor {
            self.cursor = cursor;
            self.conn.send(Request::SetCursor { window: self.id, cursor });
        }
    }

    pub fn size_hints(&self, min: (u32, u32), max: (u32, u32)) {
        self.conn.send(Request::SizeHints { window: self.id, min_width: min.0, min_height: min.1, max_width: max.0, max_height: max.1 });
    }

    pub fn notify(&self, title: &str, body: &str) {
        self.conn.send(Request::Notify { title: Title::new(title), body: Title::new(body) });
    }

    pub fn set_selection(&self, text: &str) {
        self.conn.send(Request::SetSelection { mime: Title::new("text/plain"), data: Text::new(text) });
    }

    pub fn request_selection(&self) {
        self.conn.send(Request::GetSelection { mime: Title::new("text/plain") });
    }

    pub fn choose_file(&mut self, mode: u32, title: &str, filters: &str, start: &str) -> u32 {
        let request = self.next_request;
        self.next_request += 1;
        self.conn.send(Request::ChooseFile { request, mode, title: Title::new(title), filters: Text::new(filters), start: Text::new(start) });
        request
    }

    pub fn set_wallpaper(&mut self, path: &str) -> u32 {
        let request = self.next_request;
        self.next_request += 1;
        self.conn.send(Request::SetWallpaper { request, path: Text::new(path) });
        request
    }

    pub fn create_popup(&mut self, x: i32, y: i32, width: u32, height: u32) -> Option<Surface> {
        let shm = SharedMemory::create(width as usize * height as usize * 4)?;
        if !self.conn.send(Request::CreatePopup { shm: shm.id, width, height, parent: self.id, x, y }) {
            return None;
        }
        let deadline = posix::now_ms() + 2000;
        while posix::now_ms() < deadline {
            self.conn.wait(20);
            while let Some(event) = self.conn.poll_event() {
                match event {
                    Event::Created { window } => return Some(Surface { id: window, shm, width, height }),
                    Event::Rejected { .. } => return None,
                    other => self.pending.push_back(other),
                }
            }
        }
        None
    }

    pub fn present_surface(&self, surface: &Surface) {
        self.conn.send(Request::Present { window: surface.id, x: 0, y: 0, width: surface.width, height: surface.height });
    }

    pub fn destroy_surface(&self, surface: Surface) {
        self.conn.send(Request::Destroy { window: surface.id });
    }

    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let width = width.clamp(16, 8192);
        let height = height.clamp(16, 8192);
        if width == self.width && height == self.height {
            return false;
        }
        let needed = width as usize * height as usize * 4;
        if needed > self.shm.len {
            let capacity = round_up(width, 256) as usize * round_up(height, 256) as usize * 4;
            let Some(shm) = SharedMemory::create(capacity) else {
                return false;
            };
            self.conn.send(Request::Attach { window: self.id, shm: shm.id, width, height });
            self.shm = shm;
        } else {
            self.conn.send(Request::Attach { window: self.id, shm: self.shm.id, width, height });
        }
        self.width = width;
        self.height = height;
        true
    }

    pub fn next_event(&mut self, timeout_ms: i64) -> Option<Event> {
        if let Some(e) = self.pending.pop_front() {
            return Some(e);
        }
        if let Some(e) = self.conn.poll_event() {
            return Some(e);
        }
        if timeout_ms != 0 && self.conn.wait(timeout_ms) {
            return self.conn.poll_event();
        }
        None
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.conn.send(Request::Destroy { window: self.id });
    }
}
