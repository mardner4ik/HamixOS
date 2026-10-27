pub mod damage;
pub mod screen;
pub mod window;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::display::gpu;
use hamix_std::{fs, sys};
use hxproto::{Event, MESSAGE_MAX};
use vellum::{Area, Image, Painter};

pub use damage::Damage;
pub use screen::Screen;
pub use window::{Body, Cursor, Snap, Window, EDGE_BOTTOM, EDGE_LEFT, EDGE_RIGHT, EDGE_TOP, TITLE_H};

pub fn send_event(pid: i64, event: Event) {
    let mut buf = [0u8; MESSAGE_MAX];
    let len = event.encode(&mut buf);
    sys::msg_send(pid, &buf[..len]);
}

pub const CURSOR_NAMES: [&str; 8] = ["arrow", "text", "hand", "move", "resize-ns", "resize-ew", "resize-nwse", "resize-nesw"];

pub enum Rejected {
    BadSize,
    NoBuffer,
    ShortBuffer,
}

impl Rejected {
    pub fn reason(&self) -> u32 {
        match self {
            Rejected::BadSize => 1,
            Rejected::NoBuffer => 2,
            Rejected::ShortBuffer => 3,
        }
    }
}

pub struct Compositor {
    pub screen: Screen,
    pub damage: Damage,
    pub windows: Vec<Window>,
    pub cursors: Vec<Cursor>,
    pub cursor_kind: usize,
    pub hw_cursor: bool,
    pub pointer: (i32, i32),
    pub buttons: u32,
    pub work: Area,
    pub next_id: u32,
    pub cascade: i32,
    pub focused_sent: Option<u32>,
    pub resize_sent: u64,
}

impl Compositor {
    pub fn new(screen: Screen, pointer: (i32, i32), buttons: u32, work: Area) -> Compositor {
        Compositor {
            screen,
            damage: Damage::default(),
            windows: Vec::new(),
            cursors: Vec::new(),
            cursor_kind: 0,
            hw_cursor: false,
            pointer,
            buttons,
            work,
            next_id: 1,
            cascade: 0,
            focused_sent: None,
            resize_sent: 0,
        }
    }

    pub fn width(&self) -> i32 {
        self.screen.width
    }

    pub fn height(&self) -> i32 {
        self.screen.height
    }

    pub fn bounds(&self) -> Area {
        self.screen.bounds()
    }

    pub fn clip(&self) -> Area {
        self.screen.clip
    }

    pub fn set_clip(&mut self, area: Area) {
        self.screen.clip = area;
    }

    pub fn frame(&mut self) -> &mut Vec<u32> {
        &mut self.screen.frame
    }

    pub fn painter(&mut self) -> Painter<'_> {
        self.screen.painter()
    }

    pub fn painter_clipped(&mut self) -> Painter<'_> {
        self.screen.painter_clipped()
    }

    pub fn add_damage(&mut self, area: Area) {
        let bounds = self.screen.bounds();
        self.damage.add(area, bounds);
    }

    pub fn damage_all(&mut self) {
        let bounds = self.screen.bounds();
        self.damage.all(bounds);
    }

    pub fn remap(&mut self) -> Option<bool> {
        let resized = self.screen.remap()?;
        self.damage_all();
        Some(resized)
    }

    pub fn load_cursors(&mut self) {
        let hotspots = fs::read_to_string("/usr/share/nook/cursors/hotspots").unwrap_or_default();
        self.cursors.clear();
        for name in CURSOR_NAMES {
            let hot = hotspots
                .lines()
                .find(|l| l.split_whitespace().next() == Some(name))
                .map(|l| {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    (f.get(1).and_then(|v| v.parse().ok()).unwrap_or(0), f.get(2).and_then(|v| v.parse().ok()).unwrap_or(0))
                })
                .unwrap_or((0, 0));
            let image = fs::read(&alloc::format!("/usr/share/nook/cursors/{}.png", name)).and_then(|b| Image::from_png(&b)).unwrap_or_else(|| Image::solid(10, 16, 0));
            self.cursors.push(Cursor { image, hot });
        }
        self.hw_cursor = gpu::caps() & gpu::CAP_CURSOR != 0
            && self.cursors.iter().all(|c| c.image.w as u32 <= gpu::CURSOR_MAX && c.image.h as u32 <= gpu::CURSOR_MAX)
            && self.upload_cursor();
        if self.hw_cursor {
            gpu::cursor_move(self.pointer.0, self.pointer.1);
        }
    }

    fn upload_cursor(&self) -> bool {
        let Some(cursor) = self.cursors.get(self.cursor_kind.min(self.cursors.len().saturating_sub(1))) else {
            return false;
        };
        gpu::cursor_set(&cursor.image.px, cursor.image.w as u32, cursor.image.h as u32, cursor.hot.0.max(0) as u32, cursor.hot.1.max(0) as u32) == 0
    }

    pub fn move_pointer(&mut self, x: i32, y: i32) {
        if self.hw_cursor {
            self.pointer = (x, y);
            gpu::cursor_move(x, y);
            return;
        }
        let old = self.cursor_area();
        self.pointer = (x, y);
        self.add_damage(old);
        let new = self.cursor_area();
        self.add_damage(new);
    }

    pub fn release_cursor(&mut self) {
        if self.hw_cursor {
            gpu::cursor_hide();
            self.hw_cursor = false;
        }
    }

    pub fn cursor_area(&self) -> Area {
        let c = &self.cursors[self.cursor_kind.min(self.cursors.len() - 1)];
        Area::new(self.pointer.0 - c.hot.0, self.pointer.1 - c.hot.1, c.image.w, c.image.h)
    }

    pub fn set_cursor(&mut self, kind: usize) {
        let kind = kind.min(self.cursors.len() - 1);
        if kind == self.cursor_kind {
            return;
        }
        if self.hw_cursor {
            self.cursor_kind = kind;
            self.upload_cursor();
            return;
        }
        let old = self.cursor_area();
        self.cursor_kind = kind;
        self.add_damage(old);
        let new = self.cursor_area();
        self.add_damage(new);
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    pub fn focused(&self) -> Option<u32> {
        self.windows.iter().rev().find(|w| !w.minimized && !w.is_popup()).map(|w| w.id)
    }

    pub fn window_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.windows.len()).rev().find(|&i| !self.windows[i].minimized && self.windows[i].outer().contains(x, y))
    }

    pub fn children_of(&self, id: u32) -> Vec<u32> {
        self.windows.iter().filter(|w| w.parent == Some(id)).map(|w| w.id).collect()
    }

    pub fn client_pid(&self, id: u32) -> Option<i64> {
        self.index_of(id).and_then(|i| self.windows[i].client_pid())
    }

    pub fn owned_by(&self, id: u32, sender: i64) -> bool {
        self.client_pid(id) == Some(sender)
    }

    pub fn raise(&mut self, id: u32) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let mut window = self.windows.remove(index);
        let restored = window.minimized;
        window.minimized = false;
        let area = window.with_shadow();
        self.windows.push(window);
        self.add_damage(area);
        restored
    }

    pub fn sync_focus(&mut self) -> bool {
        let now = self.focused();
        if now == self.focused_sent {
            return false;
        }
        for (id, focused) in [(self.focused_sent, false), (now, true)] {
            if let Some(id) = id {
                if let Some(index) = self.index_of(id) {
                    let area = self.windows[index].with_shadow();
                    self.add_damage(area);
                    if let Some(pid) = self.windows[index].client_pid() {
                        send_event(pid, Event::Focus { window: id, focused });
                    }
                }
            }
        }
        self.focused_sent = now;
        true
    }

    pub fn place(&mut self, w: i32, h: i32) -> (i32, i32) {
        let avail = self.work;
        let slack = (avail.h - h).max(0);
        let offset = if slack > 0 { self.cascade % (slack + 1) } else { 0 };
        let x = (self.width() - w) / 2 + self.cascade;
        let y = avail.y + if slack > 0 { (slack / 3).min(48) + offset.min(slack / 2) } else { 0 };
        self.cascade = (self.cascade + 28) % 140;
        (x.clamp(0, (self.width() - w).max(0)), y.clamp(avail.y, (self.height() - h).max(avail.y)))
    }

    pub fn new_window(&mut self, title: &str, icon: &str, w: i32, h: i32, body: Body) -> u32 {
        let (x, y) = self.place(w, h);
        let id = self.next_id;
        self.next_id += 1;
        let fixed = !matches!(body, Body::Client { .. });
        self.windows.push(Window {
            id,
            x,
            y,
            w,
            h,
            title: String::from(title),
            icon: String::from(icon),
            body,
            minimized: false,
            cursor: 0,
            min_w: if fixed { w } else { 160 },
            min_h: if fixed { h - TITLE_H } else { 80 },
            max_w: if fixed { w } else { 0 },
            max_h: if fixed { h - TITLE_H } else { 0 },
            restore: None,
            tiled: None,
            requested: (w, h - TITLE_H),
            resized_at: 0,
            parent: None,
            track: false,
            placed: (i32::MIN, i32::MIN),
            frameless: false,
        });
        id
    }

    pub fn new_popup(&mut self, parent: u32, area: Area, body: Body) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let (w, h) = (area.w, area.h);
        self.windows.push(Window {
            id,
            x: area.x,
            y: area.y,
            w,
            h,
            title: String::new(),
            icon: String::new(),
            body,
            minimized: false,
            cursor: 0,
            min_w: w,
            min_h: h,
            max_w: w,
            max_h: h,
            restore: None,
            tiled: None,
            requested: (w, h),
            resized_at: 0,
            parent: Some(parent),
            track: false,
            placed: (i32::MIN, i32::MIN),
            frameless: false,
        });
        self.add_damage(self.windows[self.windows.len() - 1].with_shadow());
        id
    }

    pub fn take_window(&mut self, id: u32, notify: bool) -> Option<Window> {
        let index = self.index_of(id)?;
        let window = self.windows.remove(index);
        if let Body::Client { pid, shm, .. } = window.body {
            if notify {
                send_event(pid, Event::Close { window: id });
            }
            sys::shm_release(shm);
        }
        self.add_damage(window.with_shadow());
        Some(window)
    }

    pub fn set_geometry(&mut self, index: usize, area: Area) {
        let old = self.windows[index].with_shadow();
        let w = &mut self.windows[index];
        if w.x == area.x && w.y == area.y && w.w == area.w && w.h == area.h {
            return;
        }
        w.x = area.x;
        w.y = area.y;
        w.w = area.w;
        w.h = area.h;
        let new = self.windows[index].with_shadow();
        if old.overlaps(&new) {
            self.add_damage(old.union(&new));
        } else {
            self.add_damage(old);
            self.add_damage(new);
        }
    }

    pub fn popup_area(&self, parent: u32, x: i32, y: i32, w: i32, h: i32) -> Area {
        let (ox, oy) = match self.index_of(parent) {
            Some(i) if parent != 0 => {
                let c = self.windows[i].content();
                (c.x, c.y)
            }
            _ => (0, 0),
        };
        let mut px = ox + x;
        let mut py = oy + y;
        if px + w > self.width() {
            px = self.width() - w;
        }
        if py + h > self.height() {
            py = self.height() - h;
        }
        Area::new(px.max(0), py.max(0), w, h)
    }

    pub fn send_placements(&mut self) {
        for w in self.windows.iter_mut() {
            if !w.track {
                continue;
            }
            let content = w.content();
            if (content.x, content.y) == w.placed {
                continue;
            }
            w.placed = (content.x, content.y);
            if let Body::Client { pid, .. } = w.body {
                send_event(pid, Event::Placed { window: w.id, x: content.x, y: content.y });
            }
        }
    }

    pub fn request_client_size(&mut self, id: u32, force: bool) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let content = self.windows[index].content();
        let size = (content.w, content.h);
        if self.windows[index].requested == size {
            return;
        }
        let now = sys::uptime_ms();
        if !force && now.saturating_sub(self.resize_sent) < 40 {
            return;
        }
        self.resize_sent = now;
        self.windows[index].requested = size;
        self.windows[index].resized_at = now;
        if let Some(pid) = self.windows[index].client_pid() {
            send_event(pid, Event::Resize { window: id, width: size.0 as u32, height: size.1 as u32 });
        }
    }

    pub fn adopt_buffer(&self, shm: u32, w: i32, h: i32, min: i32) -> Result<*const u32, Rejected> {
        if !(min..=8192).contains(&w) || !(min..=8192).contains(&h) {
            return Err(Rejected::BadSize);
        }
        let Some((ptr, len)) = sys::shm_map(shm) else {
            return Err(Rejected::NoBuffer);
        };
        if len < (w as usize * h as usize * 4) {
            sys::shm_release(shm);
            return Err(Rejected::ShortBuffer);
        }
        Ok(ptr as *const u32)
    }

    pub fn swap_buffer(&mut self, index: usize, shm: u32, w: i32, h: i32) -> bool {
        let Body::Client { shm: old_shm, .. } = self.windows[index].body else {
            return false;
        };
        if shm == old_shm {
            if let Body::Client { width: bw, height: bh, .. } = &mut self.windows[index].body {
                *bw = w;
                *bh = h;
            }
            return true;
        }
        let Some((ptr, len)) = sys::shm_map(shm) else {
            return false;
        };
        if len < w as usize * h as usize * 4 {
            sys::shm_release(shm);
            return false;
        }
        if let Body::Client { shm: s, pixels, width: bw, height: bh, .. } = &mut self.windows[index].body {
            *s = shm;
            *pixels = ptr as *const u32;
            *bw = w;
            *bh = h;
        }
        sys::shm_release(old_shm);
        true
    }

    pub fn set_title(&mut self, index: usize, title: &str) {
        self.windows[index].title = title.to_string();
        let area = self.windows[index].title_bar();
        self.add_damage(area);
    }

    pub fn set_size_hints(&mut self, index: usize, min_w: u32, min_h: u32, max_w: u32, max_h: u32) {
        let w = &mut self.windows[index];
        w.min_w = (min_w as i32).clamp(16, 4096);
        w.min_h = (min_h as i32).clamp(16, 4096);
        w.max_w = max_w as i32;
        w.max_h = max_h as i32;
        let area = w.title_bar();
        self.add_damage(area);
    }

    pub fn resize_edge_at(&self, x: i32, y: i32) -> Option<(usize, u8)> {
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized {
                continue;
            }
            let outer = w.outer();
            if !outer.expand(6).contains(x, y) {
                continue;
            }
            if outer.inset(3).contains(x, y) || !w.resizable() || w.tiled.is_some() {
                return None;
            }
            let mut edges = 0u8;
            let corner = 16;
            if x < outer.x + 3 {
                edges |= EDGE_LEFT;
            }
            if x >= outer.right() - 3 {
                edges |= EDGE_RIGHT;
            }
            if y < outer.y + 3 {
                edges |= EDGE_TOP;
            }
            if y >= outer.bottom() - 3 {
                edges |= EDGE_BOTTOM;
            }
            if edges & (EDGE_LEFT | EDGE_RIGHT) != 0 {
                if y < outer.y + corner {
                    edges |= EDGE_TOP;
                } else if y >= outer.bottom() - corner {
                    edges |= EDGE_BOTTOM;
                }
            }
            if edges & (EDGE_TOP | EDGE_BOTTOM) != 0 {
                if x < outer.x + corner {
                    edges |= EDGE_LEFT;
                } else if x >= outer.right() - corner {
                    edges |= EDGE_RIGHT;
                }
            }
            return if edges != 0 { Some((i, edges)) } else { None };
        }
        None
    }

    pub fn resolve_resize(&self, index: usize, edges: u8, start: Area, dx: i32, dy: i32) -> Area {
        let (min_w, min_h) = self.windows[index].min_outer();
        let (max_w, max_h) = self.windows[index].max_outer();
        let (mut x, mut y, mut w, mut h) = (start.x, start.y, start.w, start.h);
        if edges & EDGE_RIGHT != 0 {
            w = (start.w + dx).clamp(min_w, max_w);
        }
        if edges & EDGE_LEFT != 0 {
            w = (start.w - dx).clamp(min_w, max_w);
            x = start.right() - w;
        }
        if edges & EDGE_BOTTOM != 0 {
            h = (start.h + dy).clamp(min_h, max_h);
        }
        if edges & EDGE_TOP != 0 {
            let limit = start.bottom() - self.work.y;
            h = (start.h - dy).clamp(min_h, max_h.min(limit.max(min_h)));
            y = start.bottom() - h;
        }
        Area::new(x, y, w, h)
    }

    pub fn fit_to_screen(&mut self) {
        let work = self.work;
        let (sw, sh) = (self.width(), self.height());
        let ids: Vec<u32> = self.windows.iter().map(|w| w.id).collect();
        for id in ids {
            let index = self.index_of(id).unwrap();
            let window = &self.windows[index];
            let target = if window.tiled.is_some() {
                match window.tiled {
                    Some(Snap::Left) => Area::new(0, work.y, sw / 2, work.h),
                    Some(Snap::Right) => Area::new(sw / 2, work.y, sw - sw / 2, work.h),
                    _ => work,
                }
            } else {
                let w = window.w.min(sw);
                let h = window.h.min(work.h);
                Area::new(window.x.clamp(0, (sw - w).max(0)), window.y.clamp(work.y, (sh - h).max(work.y)), w, h)
            };
            self.windows[index].x = target.x;
            self.windows[index].y = target.y;
            self.windows[index].w = target.w;
            self.windows[index].h = target.h;
            self.request_client_size(id, true);
        }
    }
}
