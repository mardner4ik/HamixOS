use hxproto::client::{Surface, Window};
use hxproto::{Event, Text, Title, PICK_CANCELLED};

use super::wayland::WlWindow;

pub enum Transport {
    Hx(Window),
    Wl(WlWindow),
}

impl Transport {
    pub fn open_hxserver(title: &str, width: u32, height: u32) -> Result<Transport, &'static str> {
        Window::open(title, width, height).map(Transport::Hx)
    }

    pub fn open_wayland(title: &str, app_id: &str, width: u32, height: u32) -> Result<Transport, &'static str> {
        WlWindow::open(title, app_id, width, height).map(Transport::Wl)
    }

    pub fn width(&self) -> u32 {
        match self {
            Transport::Hx(w) => w.width,
            Transport::Wl(w) => w.width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Transport::Hx(w) => w.height,
            Transport::Wl(w) => w.height,
        }
    }

    pub fn buffer(&mut self) -> &mut [u32] {
        match self {
            Transport::Hx(w) => w.buffer(),
            Transport::Wl(w) => w.buffer(),
        }
    }

    pub fn update(&mut self, frame: &[u32], width: usize, height: usize) {
        match self {
            Transport::Hx(w) => {
                let shared = w.buffer();
                if shared.len() < width * height || frame.len() < width * height {
                    let n = shared.len().min(frame.len());
                    shared[..n].copy_from_slice(&frame[..n]);
                    w.present();
                    return;
                }
                let (mut left, mut right, mut top, mut bottom) = (width, 0usize, height, 0usize);
                for y in 0..height {
                    let row = y * width;
                    let (old, new) = (&mut shared[row..row + width], &frame[row..row + width]);
                    if old == new {
                        continue;
                    }
                    let first = old.iter().zip(new.iter()).position(|(a, b)| a != b).unwrap_or(0);
                    let last = width - old.iter().rev().zip(new.iter().rev()).position(|(a, b)| a != b).unwrap_or(0);
                    old[first..last].copy_from_slice(&new[first..last]);
                    left = left.min(first);
                    right = right.max(last);
                    top = top.min(y);
                    bottom = y + 1;
                }
                if right > left && bottom > top {
                    w.present_area(left as u32, top as u32, (right - left) as u32, (bottom - top) as u32);
                }
            }
            Transport::Wl(w) => {
                let shared = w.buffer();
                let n = shared.len().min(frame.len());
                shared[..n].copy_from_slice(&frame[..n]);
                w.present();
            }
        }
    }

    pub fn next_event(&mut self, timeout_ms: i64) -> Option<Event> {
        match self {
            Transport::Hx(w) => w.next_event(timeout_ms),
            Transport::Wl(w) => w.next_event(timeout_ms),
        }
    }

    pub fn fd(&self) -> i32 {
        match self {
            Transport::Hx(w) => w.conn.fd(),
            Transport::Wl(w) => w.fd(),
        }
    }

    pub fn alive(&self) -> bool {
        match self {
            Transport::Hx(w) => w.conn.server_alive(),
            Transport::Wl(w) => w.alive(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        match self {
            Transport::Hx(w) => w.resize(width, height),
            Transport::Wl(w) => w.resize(width, height),
        }
    }

    pub fn set_title(&mut self, title: &str) {
        match self {
            Transport::Hx(w) => w.set_title(title),
            Transport::Wl(w) => w.set_title(title),
        }
    }

    pub fn set_app_id(&mut self, app_id: &str) {
        match self {
            Transport::Hx(w) => w.set_app_id(app_id),
            Transport::Wl(w) => w.set_app_id(app_id),
        }
    }

    pub fn set_icon(&mut self, name: &str) {
        if let Transport::Hx(w) = self {
            w.set_icon(name);
        }
    }

    pub fn size_hints(&mut self, min: (u32, u32), max: (u32, u32)) {
        match self {
            Transport::Hx(w) => w.size_hints(min, max),
            Transport::Wl(w) => w.size_hints(min, max),
        }
    }

    pub fn set_cursor(&mut self, cursor: u32) {
        if let Transport::Hx(w) = self {
            w.set_cursor(cursor);
        }
    }

    pub fn notify(&mut self, title: &str, body: &str) {
        if let Transport::Hx(w) = self {
            w.notify(title, body);
        }
    }

    pub fn set_selection(&mut self, text: &str) {
        if let Transport::Hx(w) = self {
            w.set_selection(text);
        }
    }

    pub fn request_selection(&mut self) {
        match self {
            Transport::Hx(w) => w.request_selection(),
            Transport::Wl(w) => w.push(Event::Selection { mime: Title::new("text/plain"), data: Text::new("") }),
        }
    }

    pub fn choose_file(&mut self, mode: u32, title: &str, filters: &str, start: &str) -> u32 {
        match self {
            Transport::Hx(w) => w.choose_file(mode, title, filters, start),
            Transport::Wl(w) => {
                let request = w.next_request();
                w.push(Event::FileChosen { request, status: PICK_CANCELLED, path: Text::new("") });
                request
            }
        }
    }

    pub fn set_wallpaper(&mut self, path: &str) -> u32 {
        match self {
            Transport::Hx(w) => w.set_wallpaper(path),
            Transport::Wl(w) => {
                let request = w.next_request();
                w.push(Event::WallpaperResult { request, accepted: false });
                request
            }
        }
    }

    pub fn create_popup(&mut self, x: i32, y: i32, width: u32, height: u32) -> Option<Surface> {
        match self {
            Transport::Hx(w) => w.create_popup(x, y, width, height),
            Transport::Wl(_) => None,
        }
    }

    pub fn present_surface(&mut self, surface: &Surface) {
        if let Transport::Hx(w) = self {
            w.present_surface(surface);
        }
    }

    pub fn destroy_surface(&mut self, surface: Surface) {
        if let Transport::Hx(w) = self {
            w.destroy_surface(surface);
        }
    }
}
