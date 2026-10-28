use alloc::collections::VecDeque;
use alloc::vec::Vec;
use hxproto::client::posix;
use hxproto::Event;
use wlclient::{Connection, Message, Reader, ShmBuffer};

const WINDOW_ID: u32 = 1;

const DISPLAY: u32 = 1;
const REGISTRY: u32 = 2;

const MOUSE_MOVE: u32 = 0;
const MOUSE_PRESS: u32 = 1;
const MOUSE_RELEASE: u32 = 2;
const MOUSE_WHEEL: u32 = 3;
const MOUSE_LEAVE: u32 = 4;

const SHIFT: u32 = 1;
const ALT: u32 = 2;
const CTRL: u32 = 4;

pub fn available() -> bool {
    wlclient::available()
}

#[derive(Default)]
struct Ids {
    compositor: u32,
    shm: u32,
    wm_base: u32,
    seat: u32,
    seat_version: u32,
    surface: u32,
    xdg_surface: u32,
    toplevel: u32,
    pointer: u32,
    keyboard: u32,
    sync: u32,
}

pub struct WlWindow {
    conn: Connection,
    ids: Ids,
    buffers: Vec<ShmBuffer>,
    current: usize,
    pub width: u32,
    pub height: u32,
    configured: bool,
    pending_ack: Option<u32>,
    pending_size: Option<(u32, u32)>,
    events: VecDeque<Event>,
    pointer: (i32, i32),
    buttons: u32,
    mods: u32,
    repeat: Option<(u32, u64)>,
    repeat_rate: u32,
    repeat_delay: u32,
    alive: bool,
    requests: u32,
}

fn now_ms() -> u64 {
    posix::now_ms()
}

impl WlWindow {
    pub fn open(title: &str, app_id: &str, width: u32, height: u32) -> Result<WlWindow, &'static str> {
        let mut conn = Connection::connect()?;
        if conn.new_id() != REGISTRY {
            return Err("unexpected Wayland object ids");
        }
        let mut w = WlWindow {
            conn,
            ids: Ids::default(),
            buffers: Vec::new(),
            current: 0,
            width,
            height,
            configured: false,
            pending_ack: None,
            pending_size: None,
            events: VecDeque::new(),
            pointer: (0, 0),
            buttons: 0,
            mods: 0,
            repeat: None,
            repeat_rate: 25,
            repeat_delay: 500,
            alive: true,
            requests: 1,
        };
        w.send(Message::new(DISPLAY, 1).uint(REGISTRY).finish());
        w.roundtrip()?;
        if w.ids.compositor == 0 || w.ids.shm == 0 || w.ids.wm_base == 0 {
            return Err("the compositor lacks wl_compositor, wl_shm or xdg_wm_base");
        }
        if w.ids.seat != 0 {
            w.roundtrip()?;
        }
        w.ids.surface = w.alloc_id();
        w.send(Message::new(w.ids.compositor, 0).uint(w.ids.surface).finish());
        w.ids.xdg_surface = w.alloc_id();
        w.send(Message::new(w.ids.wm_base, 2).uint(w.ids.xdg_surface).uint(w.ids.surface).finish());
        w.ids.toplevel = w.alloc_id();
        w.send(Message::new(w.ids.xdg_surface, 1).uint(w.ids.toplevel).finish());
        w.set_title(title);
        w.set_app_id(app_id);
        w.send(Message::new(w.ids.surface, 6).finish());
        let deadline = now_ms() + 5000;
        while !w.configured {
            if now_ms() > deadline {
                return Err("the compositor did not configure the window");
            }
            w.flush();
            w.wait(50);
            w.read();
            if !w.alive {
                return Err("the compositor closed the connection");
            }
        }
        if let Some((nw, nh)) = w.pending_size.take() {
            w.width = nw;
            w.height = nh;
        }
        w.events.clear();
        Ok(w)
    }

    fn alloc_id(&mut self) -> u32 {
        self.conn.new_id()
    }

    fn send(&mut self, bytes: Vec<u8>) {
        self.conn.send(bytes);
    }

    fn flush(&mut self) {
        self.conn.flush();
        if !self.conn.alive() {
            self.alive = false;
        }
    }

    fn wait(&self, timeout_ms: i64) -> bool {
        self.conn.wait(timeout_ms)
    }

    fn read(&mut self) {
        self.conn.read();
        if !self.conn.alive() {
            self.alive = false;
        }
        while let Some(e) = self.conn.next() {
            self.event(e.object, e.opcode, &e.body);
        }
    }

    fn roundtrip(&mut self) -> Result<(), &'static str> {
        let id = self.alloc_id();
        self.ids.sync = id;
        self.send(Message::new(DISPLAY, 0).uint(id).finish());
        let deadline = now_ms() + 5000;
        while self.ids.sync == id {
            if now_ms() > deadline {
                return Err("the compositor does not answer");
            }
            self.flush();
            self.wait(50);
            self.read();
            if !self.alive {
                return Err("the compositor closed the connection");
            }
        }
        Ok(())
    }

    fn bind(&mut self, name: u32, interface: &str, version: u32) -> u32 {
        let id = self.alloc_id();
        self.send(Message::new(REGISTRY, 0).uint(name).string(interface).uint(version).uint(id).finish());
        id
    }

    fn event(&mut self, object: u32, opcode: u16, body: &[u8]) {
        let mut r = Reader::new(body);
        if object == DISPLAY {
            if opcode == 0 {
                self.alive = false;
            }
            return;
        }
        if object == REGISTRY && opcode == 0 {
            let name = r.uint();
            let interface = r.string();
            let version = r.uint();
            match interface.as_str() {
                "wl_compositor" if self.ids.compositor == 0 => self.ids.compositor = self.bind(name, "wl_compositor", version.min(4)),
                "wl_shm" if self.ids.shm == 0 => self.ids.shm = self.bind(name, "wl_shm", 1),
                "xdg_wm_base" if self.ids.wm_base == 0 => self.ids.wm_base = self.bind(name, "xdg_wm_base", 1),
                "wl_seat" if self.ids.seat == 0 => {
                    self.ids.seat_version = version.min(5);
                    self.ids.seat = self.bind(name, "wl_seat", self.ids.seat_version);
                }
                _ => {}
            }
            return;
        }
        if object == self.ids.sync && opcode == 0 {
            self.ids.sync = 0;
            return;
        }
        if object == self.ids.wm_base && opcode == 0 {
            let serial = r.uint();
            let wm = self.ids.wm_base;
            self.send(Message::new(wm, 3).uint(serial).finish());
            return;
        }
        if object == self.ids.xdg_surface && opcode == 0 {
            self.pending_ack = Some(r.uint());
            self.configured = true;
            if let Some((w, h)) = self.pending_size {
                if (w, h) != (self.width, self.height) {
                    self.events.push_back(Event::Resize { window: WINDOW_ID, width: w, height: h });
                }
            }
            return;
        }
        if object == self.ids.toplevel {
            match opcode {
                0 => {
                    let w = r.int();
                    let h = r.int();
                    if w > 0 && h > 0 {
                        self.pending_size = Some((w as u32, h as u32));
                    }
                }
                1 => self.events.push_back(Event::Close { window: WINDOW_ID }),
                _ => {}
            }
            return;
        }
        if object == self.ids.seat && opcode == 0 {
            let caps = r.uint();
            if caps & 1 != 0 && self.ids.pointer == 0 {
                self.ids.pointer = self.alloc_id();
                let (seat, p) = (self.ids.seat, self.ids.pointer);
                self.send(Message::new(seat, 0).uint(p).finish());
            }
            if caps & 2 != 0 && self.ids.keyboard == 0 {
                self.ids.keyboard = self.alloc_id();
                let (seat, k) = (self.ids.seat, self.ids.keyboard);
                self.send(Message::new(seat, 1).uint(k).finish());
            }
            return;
        }
        if object == self.ids.pointer && self.ids.pointer != 0 {
            self.pointer_event(opcode, &mut r);
            return;
        }
        if object == self.ids.keyboard && self.ids.keyboard != 0 {
            self.keyboard_event(opcode, &mut r);
            return;
        }
        if let Some(b) = self.buffers.iter_mut().find(|b| b.id == object) {
            if opcode == 0 {
                b.busy = false;
            }
        }
    }

    fn mouse(&mut self, kind: u32, wheel: i32) {
        let (x, y) = self.pointer;
        self.events.push_back(Event::Mouse { window: WINDOW_ID, x, y, buttons: self.buttons, kind, wheel });
    }

    fn pointer_event(&mut self, opcode: u16, r: &mut Reader) {
        match opcode {
            0 => {
                r.uint();
                r.uint();
                self.pointer = (r.fixed(), r.fixed());
                self.mouse(MOUSE_MOVE, 0);
            }
            1 => {
                self.buttons = 0;
                self.events.push_back(Event::Mouse { window: WINDOW_ID, x: -1, y: -1, buttons: 0, kind: MOUSE_LEAVE, wheel: 0 });
            }
            2 => {
                r.uint();
                self.pointer = (r.fixed(), r.fixed());
                self.mouse(MOUSE_MOVE, 0);
            }
            3 => {
                r.uint();
                r.uint();
                let button = r.uint();
                let pressed = r.uint() == 1;
                let bit = match button {
                    0x110 => 1,
                    0x111 => 2,
                    0x112 => 4,
                    _ => 0,
                };
                if bit == 0 {
                    return;
                }
                if pressed {
                    self.buttons |= bit;
                    self.mouse(MOUSE_PRESS, bit as i32);
                } else {
                    self.buttons &= !bit;
                    self.mouse(MOUSE_RELEASE, bit as i32);
                }
            }
            4 => {
                r.uint();
                let axis = r.uint();
                let value = r.int();
                if axis == 0 && value != 0 {
                    self.mouse(MOUSE_WHEEL, value.signum());
                }
            }
            _ => {}
        }
    }

    fn keyboard_event(&mut self, opcode: u16, r: &mut Reader) {
        match opcode {
            1 => {
                r.uint();
                r.uint();
                r.array();
                self.events.push_back(Event::Focus { window: WINDOW_ID, focused: true });
            }
            2 => {
                self.repeat = None;
                self.events.push_back(Event::Focus { window: WINDOW_ID, focused: false });
            }
            3 => {
                r.uint();
                r.uint();
                let key = r.uint();
                let pressed = r.uint() == 1;
                if !pressed {
                    if self.repeat.map(|(k, _)| k == key).unwrap_or(false) {
                        self.repeat = None;
                    }
                    return;
                }
                if let Some(code) = translate(key, self.mods & SHIFT != 0) {
                    self.events.push_back(Event::Key { window: WINDOW_ID, code, mods: self.mods });
                    self.repeat = Some((key, now_ms() + self.repeat_delay as u64));
                }
            }
            4 => {
                r.uint();
                let depressed = r.uint();
                let latched = r.uint();
                let locked = r.uint();
                let all = depressed | latched;
                let mut mods = 0;
                if all & 1 != 0 || locked & 2 != 0 {
                    mods |= SHIFT;
                }
                if all & 4 != 0 {
                    mods |= CTRL;
                }
                if all & 8 != 0 {
                    mods |= ALT;
                }
                self.mods = mods;
            }
            5 => {
                let rate = r.int();
                let delay = r.int();
                self.repeat_rate = rate.max(0) as u32;
                self.repeat_delay = delay.max(100) as u32;
            }
            _ => {}
        }
    }

    fn repeat_due(&mut self) -> Option<u64> {
        let (key, at) = self.repeat?;
        if self.repeat_rate == 0 {
            return None;
        }
        let now = now_ms();
        if now >= at {
            if let Some(code) = translate(key, self.mods & SHIFT != 0) {
                self.events.push_back(Event::Key { window: WINDOW_ID, code, mods: self.mods });
            }
            self.repeat = Some((key, now + 1000 / self.repeat_rate as u64));
            return Some(0);
        }
        Some(at - now)
    }

    pub fn fd(&self) -> i32 {
        self.conn.fd()
    }

    pub fn push(&mut self, event: Event) {
        self.events.push_back(event);
    }

    pub fn next_request(&mut self) -> u32 {
        self.requests += 1;
        self.requests
    }

    pub fn alive(&self) -> bool {
        self.alive
    }

    pub fn next_event(&mut self, timeout_ms: i64) -> Option<Event> {
        if let Some(e) = self.events.pop_front() {
            return Some(e);
        }
        self.flush();
        self.read();
        if let Some(e) = self.events.pop_front() {
            return Some(e);
        }
        let mut timeout = timeout_ms;
        if let Some(due) = self.repeat_due() {
            if let Some(e) = self.events.pop_front() {
                return Some(e);
            }
            timeout = if timeout < 0 { due as i64 } else { timeout.min(due as i64) };
        }
        if timeout != 0 && self.wait(timeout) {
            self.read();
        }
        self.repeat_due();
        self.events.pop_front()
    }

    fn create_buffer(&mut self, width: u32, height: u32) -> Option<ShmBuffer> {
        let shm = self.ids.shm;
        ShmBuffer::create(&mut self.conn, shm, width, height, 1)
    }

    fn destroy_buffer(&mut self, b: ShmBuffer) {
        b.destroy(&mut self.conn);
    }

    fn choose_buffer(&mut self) -> usize {
        let (w, h) = (self.width, self.height);
        let stale: Vec<usize> = (0..self.buffers.len()).filter(|i| self.buffers[*i].width != w || self.buffers[*i].height != h).collect();
        for i in stale.into_iter().rev() {
            if !self.buffers[i].busy {
                let b = self.buffers.remove(i);
                self.destroy_buffer(b);
            }
        }
        if let Some(i) = self.buffers.iter().position(|b| !b.busy && b.width == w && b.height == h) {
            return i;
        }
        if self.buffers.iter().filter(|b| b.width == w && b.height == h).count() < 2 {
            if let Some(b) = self.create_buffer(w, h) {
                self.buffers.push(b);
                return self.buffers.len() - 1;
            }
        }
        let deadline = now_ms() + 100;
        while now_ms() < deadline {
            self.flush();
            self.wait(10);
            self.read();
            if let Some(i) = self.buffers.iter().position(|b| !b.busy && b.width == w && b.height == h) {
                return i;
            }
        }
        match self.create_buffer(w, h) {
            Some(b) => {
                self.buffers.push(b);
                self.buffers.len() - 1
            }
            None => 0,
        }
    }

    pub fn buffer(&mut self) -> &mut [u32] {
        self.current = self.choose_buffer();
        match self.buffers.get_mut(self.current) {
            Some(b) => b.pixels(),
            None => &mut [],
        }
    }

    pub fn present(&mut self) {
        let Some(b) = self.buffers.get_mut(self.current) else {
            return;
        };
        b.busy = true;
        let id = b.id;
        if let Some(serial) = self.pending_ack.take() {
            let xs = self.ids.xdg_surface;
            self.send(Message::new(xs, 4).uint(serial).finish());
        }
        let s = self.ids.surface;
        self.send(Message::new(s, 1).uint(id).int(0).int(0).finish());
        self.send(Message::new(s, 9).int(0).int(0).int(self.width as i32).int(self.height as i32).finish());
        self.send(Message::new(s, 6).finish());
        self.flush();
    }

    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let width = width.clamp(16, 8192);
        let height = height.clamp(16, 8192);
        if width == self.width && height == self.height {
            return false;
        }
        self.width = width;
        self.height = height;
        true
    }

    pub fn set_title(&mut self, title: &str) {
        let t = self.ids.toplevel;
        self.send(Message::new(t, 2).string(title).finish());
    }

    pub fn set_app_id(&mut self, app_id: &str) {
        let t = self.ids.toplevel;
        self.send(Message::new(t, 3).string(app_id).finish());
    }

    pub fn size_hints(&mut self, min: (u32, u32), max: (u32, u32)) {
        let t = self.ids.toplevel;
        self.send(Message::new(t, 8).int(min.0 as i32).int(min.1 as i32).finish());
        self.send(Message::new(t, 7).int(max.0 as i32).int(max.1 as i32).finish());
    }
}

impl Drop for WlWindow {
    fn drop(&mut self) {
        let buffers: Vec<ShmBuffer> = self.buffers.drain(..).collect();
        for b in buffers {
            self.destroy_buffer(b);
        }
        self.flush();
    }
}

fn translate(key: u32, shift: bool) -> Option<i32> {
    wlclient::translate_key(key, shift).filter(|c| *c != -21)
}
