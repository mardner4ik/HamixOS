use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use hxproto::{Event, Request, Text, Title, MESSAGE_MAX};
use hxstd::sys;

pub use hxproto::{
    ACTION_ACTIVATE, ACTION_CLOSE, ACTION_MINIMIZE, ACTION_TOGGLE_MAXIMIZE, ANCHOR_BOTTOM, ANCHOR_LEFT, ANCHOR_RIGHT, ANCHOR_TOP, LAYER_BACKGROUND, LAYER_BOTTOM, LAYER_OVERLAY, LAYER_TOP,
    TOPLEVEL_ACTIVE, TOPLEVEL_MAXIMIZED, TOPLEVEL_MINIMIZED,
};

fn send(server: i64, request: Request) -> bool {
    let mut buf = [0u8; MESSAGE_MAX];
    let len = request.encode(&mut buf);
    sys::msg_send(server, &buf[..len]) >= 0
}

fn wait_for<F: FnMut(&Event) -> bool>(server: i64, pending: &mut VecDeque<Event>, mut matches: F, timeout_ms: u64) -> Option<Event> {
    if let Some(i) = pending.iter().position(|e| matches(e)) {
        return pending.remove(i);
    }
    let deadline = sys::uptime_ms() + timeout_ms;
    let mut buf = [0u8; MESSAGE_MAX];
    while sys::uptime_ms() < deadline {
        let Some(len) = sys::msg_recv_from(server, &mut buf, 50) else {
            continue;
        };
        if let Some(event) = Event::decode(&buf[..len]) {
            if matches(&event) {
                return Some(event);
            }
            pending.push_back(event);
        }
    }
    None
}

pub struct Connection {
    pub server: i64,
    pub version: u32,
    pub features: u32,
    pub screen: (u32, u32, u32),
    pub workspaces: (u32, u32),
    pending: VecDeque<Event>,
}

impl Connection {
    pub fn open() -> Result<Connection, &'static str> {
        let server = crate::server().ok_or("hxserver is not running")?;
        if !send(server, Request::Hello { version: hxproto::VERSION }) {
            return Err("cannot reach hxserver");
        }
        let mut pending = VecDeque::new();
        let Some(Event::Welcome { version, features }) = wait_for(server, &mut pending, |e| matches!(e, Event::Welcome { .. }), 2000) else {
            return Err("hxserver does not speak hxproto v2");
        };
        let mut conn = Connection { server, version, features, screen: (0, 0, 100), workspaces: (0, 1), pending };
        if let Some(Event::Output { width, height, scale }) = wait_for(server, &mut conn.pending, |e| matches!(e, Event::Output { .. }), 500) {
            conn.screen = (width, height, scale);
        }
        if let Some(Event::Workspaces { current, count }) = wait_for(server, &mut conn.pending, |e| matches!(e, Event::Workspaces { .. }), 500) {
            conn.workspaces = (current, count);
        }
        Ok(conn)
    }

    pub fn has(&self, feature: u32) -> bool {
        self.features & feature != 0
    }

    pub fn next_event(&mut self, timeout_ms: u64) -> Option<Event> {
        if let Some(e) = self.pending.pop_front() {
            return Some(e);
        }
        wait_for(self.server, &mut self.pending, |_| true, timeout_ms)
    }

    pub fn take_pending(&mut self) -> Option<Event> {
        self.pending.pop_front()
    }

    pub fn wait<F: FnMut(&Event) -> bool>(&mut self, matches: F, timeout_ms: u64) -> Option<Event> {
        wait_for(self.server, &mut self.pending, matches, timeout_ms)
    }

    pub fn subscribe_toplevels(&self) -> bool {
        send(self.server, Request::SubscribeToplevels)
    }

    pub fn toplevel_action(&self, toplevel: u32, action: u32) -> bool {
        send(self.server, Request::ToplevelAction { toplevel, action })
    }

    pub fn switch_workspace(&self, index: u32) -> bool {
        send(self.server, Request::SwitchWorkspace { index })
    }

    pub fn move_to_workspace(&self, window: u32, index: u32) -> bool {
        send(self.server, Request::MoveToWorkspace { window, index })
    }

    pub fn set_selection(&self, mime: &str, data: &str) -> bool {
        send(self.server, Request::SetSelection { mime: Title::new(mime), data: Text::new(data) })
    }

    pub fn selection(&mut self, mime: &str) -> Option<(String, String)> {
        send(self.server, Request::GetSelection { mime: Title::new(mime) });
        match self.wait(|e| matches!(e, Event::Selection { .. }), 1000)? {
            Event::Selection { mime, data } if !mime.as_str().is_empty() => Some((mime.as_str().to_string(), data.as_str().to_string())),
            _ => None,
        }
    }

    pub fn start_drag(&self, window: u32, mime: &str, data: &str) -> bool {
        send(self.server, Request::StartDrag { window, mime: Title::new(mime), data: Text::new(data) })
    }
}

pub struct LayerSurface {
    pub id: u32,
    server: i64,
    shm: u32,
    pixels: *mut u32,
    capacity: usize,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

pub struct LayerSpec {
    pub layer: u32,
    pub anchor: u32,
    pub exclusive: i32,
    pub margins: [i32; 4],
    pub keyboard: bool,
    pub width: u32,
    pub height: u32,
}

impl LayerSurface {
    pub fn open(conn: &mut Connection, spec: &LayerSpec) -> Result<LayerSurface, &'static str> {
        let width = if spec.width == 0 { conn.screen.0.max(1) } else { spec.width };
        let height = if spec.height == 0 { conn.screen.1.max(1) } else { spec.height };
        let capacity = width as usize * height as usize * 4;
        let shm = sys::shm_create(capacity);
        if shm < 0 {
            return Err("out of memory for the layer buffer");
        }
        let shm = shm as u32;
        let (ptr, _) = sys::shm_map(shm).ok_or("cannot map the layer buffer")?;
        let request = Request::CreateLayer { shm, width, height, layer: spec.layer, anchor: spec.anchor, exclusive: spec.exclusive, margins: spec.margins, keyboard: spec.keyboard as u32 };
        if !send(conn.server, request) {
            return Err("hxserver refused the layer");
        }
        let id = match conn.wait(|e| matches!(e, Event::Created { .. } | Event::Rejected { .. }), 2000) {
            Some(Event::Created { window }) => window,
            _ => return Err("hxserver rejected the layer"),
        };
        let mut surface = LayerSurface { id, server: conn.server, shm, pixels: ptr as *mut u32, capacity, width, height, x: 0, y: 0 };
        if let Some(Event::LayerConfigured { x, y, width, height, .. }) = conn.wait(|e| matches!(e, Event::LayerConfigured { window, .. } if *window == id), 1000) {
            surface.x = x;
            surface.y = y;
            if (width, height) != (surface.width, surface.height) && width as usize * height as usize * 4 <= capacity {
                surface.width = width;
                surface.height = height;
                send(conn.server, Request::Attach { window: id, shm, width, height });
            }
        }
        Ok(surface)
    }

    pub fn pixels(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.pixels, (self.width * self.height) as usize) }
    }

    pub fn present(&self) -> bool {
        send(self.server, Request::Present { window: self.id, x: 0, y: 0, width: self.width, height: self.height })
    }

    pub fn present_area(&self, x: u32, y: u32, width: u32, height: u32) -> bool {
        send(self.server, Request::Present { window: self.id, x, y, width, height })
    }

    pub fn configure(&self, anchor: u32, exclusive: i32, margins: [i32; 4]) -> bool {
        send(self.server, Request::ConfigureLayer { window: self.id, anchor, exclusive, margins })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Drop for LayerSurface {
    fn drop(&mut self) {
        send(self.server, Request::Destroy { window: self.id });
        sys::shm_release(self.shm);
    }
}
