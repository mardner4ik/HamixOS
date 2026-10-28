use alloc::string::String;
use alloc::vec::Vec;
use hxproto::{Event, Title};
use hxstd::sys;
use vellum::Area;
use wlclient::{now_ms, Connection, Message, ShmBuffer};

use crate::shell::{Core, Kind};
use crate::TOP_H;

const DISPLAY: u32 = wlclient::DISPLAY;
const LAYER_BOTTOM: u32 = 1;
const LAYER_TOP: u32 = 2;
const LAYER_OVERLAY: u32 = 3;
const ANCHOR_TOP: u32 = 1;
const ANCHOR_BOTTOM: u32 = 2;
const ANCHOR_LEFT: u32 = 4;
const ANCHOR_RIGHT: u32 = 8;
const ALL: u32 = ANCHOR_TOP | ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT;
const STATE_MAXIMIZED: u32 = 0;
const STATE_MINIMIZED: u32 = 1;
const STATE_ACTIVATED: u32 = 2;
const SHIFT: u32 = 1;
const ALT: u32 = 2;
const CTRL: u32 = 4;

struct Layer {
    kind: Kind,
    surface: u32,
    role: u32,
    buffer: Option<ShmBuffer>,
    anchor: u32,
    margins: [i32; 4],
    want: (i32, i32),
    size: (i32, i32),
    origin: (i32, i32),
    serial: Option<u32>,
    configured: bool,
}

struct Toplevel {
    handle: u32,
    title: String,
    app_id: String,
    state: u32,
}

#[derive(Default)]
struct Globals {
    registry: u32,
    compositor: u32,
    shm: u32,
    layer_shell: u32,
    seat: u32,
    pointer: u32,
    keyboard: u32,
    manager: u32,
    output: u32,
    cursor_shape: u32,
    cursor_device: u32,
    hamix: u32,
}

pub struct Platform {
    conn: Connection,
    g: Globals,
    screen: (i32, i32),
    layers: Vec<Layer>,
    toplevels: Vec<Toplevel>,
    queue: Vec<Event>,
    pointer_surface: u32,
    pointer: (i32, i32),
    buttons: u32,
    enter_serial: u32,
    keyboard_surface: u32,
    mods: u32,
    repeat: Option<(u32, u64)>,
    repeat_rate: u32,
    repeat_delay: u32,
    protocol_error: bool,
}

fn origin_for(anchor: u32, margins: [i32; 4], size: (i32, i32), screen: (i32, i32)) -> (i32, i32) {
    let [top, right, bottom, left] = margins;
    let x = match (anchor & ANCHOR_LEFT != 0, anchor & ANCHOR_RIGHT != 0) {
        (true, _) => left,
        (false, true) => screen.0 - size.0 - right,
        _ => (screen.0 - size.0) / 2,
    };
    let y = match (anchor & ANCHOR_TOP != 0, anchor & ANCHOR_BOTTOM != 0) {
        (true, _) => top,
        (false, true) => screen.1 - size.1 - bottom,
        _ => (screen.1 - size.1) / 2,
    };
    (x, y)
}

fn cursor_shape(cursor: u32) -> u32 {
    match cursor {
        hxproto::CURSOR_TEXT => 9,
        hxproto::CURSOR_HAND => 4,
        hxproto::CURSOR_MOVE => 13,
        hxproto::CURSOR_RESIZE_NS => 27,
        hxproto::CURSOR_RESIZE_EW => 26,
        hxproto::CURSOR_RESIZE_NWSE => 29,
        hxproto::CURSOR_RESIZE_NESW => 28,
        _ => 1,
    }
}

impl Platform {
    pub fn open() -> Result<Platform, &'static str> {
        let mut conn = Connection::connect()?;
        let registry = conn.get_registry();
        let mut p = Platform {
            conn,
            g: Globals { registry, ..Globals::default() },
            screen: (0, 0),
            layers: Vec::new(),
            toplevels: Vec::new(),
            queue: Vec::new(),
            pointer_surface: 0,
            pointer: (0, 0),
            buttons: 0,
            enter_serial: 0,
            keyboard_surface: 0,
            mods: 0,
            repeat: None,
            repeat_rate: 25,
            repeat_delay: 500,
            protocol_error: false,
        };
        p.roundtrip()?;
        if p.g.compositor == 0 || p.g.shm == 0 || p.g.layer_shell == 0 {
            return Err("the compositor lacks wl_compositor, wl_shm or zwlr_layer_shell_v1");
        }
        p.roundtrip()?;
        if p.screen == (0, 0) {
            p.screen = (1024, 768);
        }
        Ok(p)
    }

    fn roundtrip(&mut self) -> Result<(), &'static str> {
        let callback = self.conn.sync();
        let deadline = now_ms() + 5000;
        loop {
            if self.protocol_error {
                return Err("the compositor reported a Wayland protocol error");
            }
            self.conn.flush();
            while let Some(e) = self.conn.next() {
                if e.object == callback && e.opcode == 0 {
                    return Ok(());
                }
                self.handle(e);
            }
            if !self.conn.alive() {
                return Err("the compositor closed the connection");
            }
            if now_ms() > deadline {
                return Err("the compositor does not answer");
            }
            if self.conn.wait(50) {
                self.conn.read();
            }
        }
    }

    fn handle(&mut self, e: wlclient::Event) {
        let mut r = e.reader();
        if e.object == DISPLAY {
            if let Some(error) = e.display_error() {
                hxstd::println!("nook-shell: Wayland error on object {} ({}): {}", error.object, error.code, error.message);
                self.protocol_error = true;
            }
            return;
        }
        if e.object == self.g.registry && e.opcode == 0 {
            let Some(global) = e.registry_global() else {
                return;
            };
            let registry = self.g.registry;
            let c = &mut self.conn;
            match global.interface.as_str() {
                "wl_compositor" if self.g.compositor == 0 => self.g.compositor = global.bind(c, registry, 4).unwrap_or(0),
                "wl_shm" if self.g.shm == 0 => self.g.shm = global.bind(c, registry, 1).unwrap_or(0),
                "zwlr_layer_shell_v1" if self.g.layer_shell == 0 => self.g.layer_shell = global.bind(c, registry, 4).unwrap_or(0),
                "wl_seat" if self.g.seat == 0 => self.g.seat = global.bind(c, registry, 5).unwrap_or(0),
                "zwlr_foreign_toplevel_manager_v1" if self.g.manager == 0 => self.g.manager = global.bind(c, registry, 3).unwrap_or(0),
                "wl_output" if self.g.output == 0 => self.g.output = global.bind(c, registry, 2).unwrap_or(0),
                "wp_cursor_shape_manager_v1" if self.g.cursor_shape == 0 => self.g.cursor_shape = global.bind(c, registry, 1).unwrap_or(0),
                "hamix_shell_v1" if self.g.hamix == 0 => self.g.hamix = global.bind(c, registry, 1).unwrap_or(0),
                _ => {}
            }
            return;
        }
        if e.object == self.g.output && e.opcode == 1 {
            let flags = r.uint();
            let (w, h) = (r.int(), r.int());
            if flags & 1 != 0 && w > 0 && h > 0 && (w, h) != self.screen {
                let first = self.screen == (0, 0);
                self.screen = (w, h);
                if !first {
                    self.queue.push(Event::Screen { width: w as u32, height: h as u32 });
                }
            }
            return;
        }
        if e.object == self.g.hamix && self.g.hamix != 0 {
            if e.opcode == 0 {
                let toplevel = r.uint();
                let (x, y, width, height) = (r.int(), r.int(), r.int(), r.int());
                self.queue.push(Event::ToplevelGeometry { toplevel, x, y, width, height });
            }
            return;
        }
        if e.object == self.g.seat && e.opcode == 0 {
            let caps = r.uint();
            if caps & 1 != 0 && self.g.pointer == 0 {
                self.g.pointer = self.conn.new_id();
                let (seat, ptr) = (self.g.seat, self.g.pointer);
                self.conn.send(Message::new(seat, 0).uint(ptr).finish());
                if self.g.cursor_shape != 0 {
                    self.g.cursor_device = self.conn.new_id();
                    let (m, d) = (self.g.cursor_shape, self.g.cursor_device);
                    self.conn.send(Message::new(m, 1).uint(d).uint(ptr).finish());
                }
            }
            if caps & 2 != 0 && self.g.keyboard == 0 {
                self.g.keyboard = self.conn.new_id();
                let (seat, k) = (self.g.seat, self.g.keyboard);
                self.conn.send(Message::new(seat, 1).uint(k).finish());
            }
            return;
        }
        if e.object == self.g.pointer && self.g.pointer != 0 {
            self.pointer_event(e.opcode, &mut r);
            return;
        }
        if e.object == self.g.keyboard && self.g.keyboard != 0 {
            self.keyboard_event(e.opcode, &mut r);
            return;
        }
        if e.object == self.g.manager && self.g.manager != 0 {
            if e.opcode == 0 {
                let handle = r.uint();
                self.toplevels.push(Toplevel { handle, title: String::new(), app_id: String::new(), state: 0 });
            }
            return;
        }
        if let Some(i) = self.toplevels.iter().position(|t| t.handle == e.object) {
            match e.opcode {
                0 => self.toplevels[i].title = r.string(),
                1 => self.toplevels[i].app_id = r.string(),
                4 => {
                    let states = r.array();
                    let mut state = 0;
                    for chunk in states.chunks_exact(4) {
                        match u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) {
                            STATE_MAXIMIZED => state |= hxproto::TOPLEVEL_MAXIMIZED,
                            STATE_MINIMIZED => state |= hxproto::TOPLEVEL_MINIMIZED,
                            STATE_ACTIVATED => state |= hxproto::TOPLEVEL_ACTIVE,
                            _ => {}
                        }
                    }
                    self.toplevels[i].state = state;
                }
                5 => {
                    let t = &self.toplevels[i];
                    self.queue.push(Event::ToplevelState { toplevel: t.handle, state: t.state, workspace: 0, title: Title::new(&t.title), app_id: Title::new(&t.app_id) });
                }
                6 => {
                    let t = self.toplevels.remove(i);
                    self.conn.send(Message::new(t.handle, 7).finish());
                    self.queue.push(Event::ToplevelClosed { toplevel: t.handle });
                }
                _ => {}
            }
            return;
        }
        if let Some(i) = self.layers.iter().position(|l| l.role == e.object) {
            match e.opcode {
                0 => {
                    let serial = r.uint();
                    let (w, h) = (r.uint() as i32, r.uint() as i32);
                    let screen = self.screen;
                    let l = &mut self.layers[i];
                    let w = if w > 0 { w } else { l.want.0.max(1) };
                    let h = if h > 0 { h } else { l.want.1.max(1) };
                    if l.kind == Kind::Desktop && (w, h) != screen && screen != (0, 0) {
                        self.screen = (w, h);
                        self.queue.push(Event::Screen { width: w as u32, height: h as u32 });
                    }
                    let l = &mut self.layers[i];
                    let resized = l.configured && (w, h) != l.size;
                    l.size = (w, h);
                    l.serial = Some(serial);
                    l.configured = true;
                    l.origin = origin_for(l.anchor, l.margins, l.size, self.screen);
                    if resized {
                        let (surface, origin) = (l.surface, l.origin);
                        self.queue.push(Event::LayerConfigured { window: surface, x: origin.0, y: origin.1, width: w as u32, height: h as u32 });
                    }
                }
                1 => {
                    let l = self.layers.remove(i);
                    self.drop_layer(l);
                }
                _ => {}
            }
            return;
        }
        for l in self.layers.iter_mut() {
            if let Some(b) = l.buffer.as_mut().filter(|b| e.buffer_release(b.id)) {
                b.busy = false;
            }
        }
    }

    fn mouse(&mut self, kind: u32, wheel: i32) {
        let (x, y) = self.pointer;
        self.queue.push(Event::Mouse { window: self.pointer_surface, x, y, buttons: self.buttons, kind, wheel });
    }

    fn pointer_event(&mut self, opcode: u16, r: &mut wlclient::Reader) {
        match opcode {
            0 => {
                self.enter_serial = r.uint();
                self.pointer_surface = r.uint();
                self.pointer = (r.fixed(), r.fixed());
                self.mouse(hxproto::MOUSE_MOVE, 0);
            }
            1 => {
                let surface = self.pointer_surface;
                self.buttons = 0;
                self.queue.push(Event::Mouse { window: surface, x: -1, y: -1, buttons: 0, kind: hxproto::MOUSE_LEAVE, wheel: 0 });
            }
            2 => {
                r.uint();
                self.pointer = (r.fixed(), r.fixed());
                self.mouse(hxproto::MOUSE_MOVE, 0);
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
                    self.mouse(hxproto::MOUSE_PRESS, bit as i32);
                } else {
                    self.buttons &= !bit;
                    self.mouse(hxproto::MOUSE_RELEASE, bit as i32);
                }
            }
            4 => {
                r.uint();
                let axis = r.uint();
                let value = r.int();
                if axis == 0 && value != 0 {
                    self.mouse(hxproto::MOUSE_WHEEL, value.signum());
                }
            }
            _ => {}
        }
    }

    fn keyboard_event(&mut self, opcode: u16, r: &mut wlclient::Reader) {
        match opcode {
            0 => {
                r.uint();
                r.uint();
                if let Some(fd) = self.conn.take_fd() {
                    wlclient::close_fd(fd);
                }
            }
            1 => {
                r.uint();
                self.keyboard_surface = r.uint();
            }
            2 => {
                self.repeat = None;
                self.keyboard_surface = 0;
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
                if let Some(code) = wlclient::translate_key(key, self.mods & SHIFT != 0) {
                    self.queue.push(Event::Key { window: self.keyboard_surface, code, mods: self.mods });
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
                self.repeat_rate = r.int().max(0) as u32;
                self.repeat_delay = r.int().max(100) as u32;
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
            if let Some(code) = wlclient::translate_key(key, self.mods & SHIFT != 0) {
                self.queue.push(Event::Key { window: self.keyboard_surface, code, mods: self.mods });
            }
            self.repeat = Some((key, now + 1000 / self.repeat_rate as u64));
            return Some(0);
        }
        Some(at - now)
    }

    pub fn screen(&self) -> (i32, i32) {
        self.screen
    }

    pub fn workspace(&self) -> u32 {
        0
    }

    pub fn alive(&self) -> bool {
        self.conn.alive() && !self.protocol_error
    }

    fn native(&self, request: hxproto::Request) -> bool {
        let Some(server) = hxclient::server() else {
            return false;
        };
        let mut buf = [0u8; hxproto::MESSAGE_MAX];
        let len = request.encode(&mut buf);
        sys::msg_send(server, &buf[..len]) >= 0
    }

    pub fn toast(&mut self, title: &str, body: &str, icon: &str) {
        self.native(hxproto::Request::Toast { title: Title::new(title), body: Title::new(body), icon: Title::new(icon) });
    }

    pub fn shell_action(&mut self, action: u32) {
        if self.native(hxproto::Request::ShellAction { action }) {
            return;
        }
        let none: &[&str] = &[];
        match action {
            hxproto::SHELL_LOGOUT => {
                let parent = sys::proc_list().into_iter().find(|p| p.pid == sys::getpid()).map(|p| p.parent).unwrap_or(0);
                if parent > 1 {
                    sys::send_signal(parent, sys::SIGTERM);
                }
            }
            hxproto::SHELL_REBOOT => {
                sys::spawn("/usr/bin/reboot", none, sys::SPAWN_DETACH);
            }
            hxproto::SHELL_SHUTDOWN => {
                sys::spawn("/usr/bin/poweroff", none, sys::SPAWN_DETACH);
            }
            _ => {
                sys::spawn("/usr/bin/hxsettings", &["about"], sys::SPAWN_DETACH);
            }
        }
    }

    pub fn launched(&mut self, _pid: i64, _key: &str) {}

    pub fn minimize_target(&mut self, toplevel: u32, area: Area) {
        let Some(dock) = self.layers.iter().find(|l| l.kind == Kind::Dock) else {
            return;
        };
        if !self.toplevels.iter().any(|t| t.handle == toplevel) {
            return;
        }
        let (surface, origin) = (dock.surface, dock.origin);
        self.conn.send(Message::new(toplevel, 6).uint(surface).int(area.x - origin.0).int(area.y - origin.1).int(area.w).int(area.h).finish());
    }

    pub fn set_cursor(&mut self, _window: u32, cursor: u32) {
        if self.g.cursor_device == 0 {
            return;
        }
        let (d, serial) = (self.g.cursor_device, self.enter_serial);
        self.conn.send(Message::new(d, 1).uint(serial).uint(cursor_shape(cursor)).finish());
        self.conn.flush();
    }

    pub fn toplevel_action(&mut self, toplevel: u32, action: u32) {
        let Some(t) = self.toplevels.iter().find(|t| t.handle == toplevel) else {
            return;
        };
        let (state, seat) = (t.state, self.g.seat);
        match action {
            hxproto::ACTION_ACTIVATE => {
                if state & hxproto::TOPLEVEL_MINIMIZED != 0 {
                    self.conn.send(Message::new(toplevel, 3).finish());
                }
                self.conn.send(Message::new(toplevel, 4).uint(seat).finish());
            }
            hxproto::ACTION_MINIMIZE => self.conn.send(Message::new(toplevel, 2).finish()),
            hxproto::ACTION_CLOSE => self.conn.send(Message::new(toplevel, 5).finish()),
            hxproto::ACTION_TOGGLE_MAXIMIZE => {
                let op = if state & hxproto::TOPLEVEL_MAXIMIZED != 0 { 1 } else { 0 };
                self.conn.send(Message::new(toplevel, op).finish());
            }
            _ => {}
        }
        self.conn.flush();
    }

    pub fn screen_changed(&mut self, width: u32, height: u32) {
        self.screen = (width as i32, height as i32);
    }

    pub fn pump(&mut self, timeout: u64, out: &mut Vec<Event>) {
        let mut wait = timeout as i64;
        if let Some(due) = self.repeat_due() {
            wait = wait.min(due as i64);
        }
        if let Some(e) = self.conn.dispatch_pending(if self.queue.is_empty() { wait } else { 0 }) {
            self.handle(e);
        }
        while let Some(e) = self.conn.next() {
            self.handle(e);
        }
        self.conn.read();
        while let Some(e) = self.conn.next() {
            self.handle(e);
        }
        self.repeat_due();
        self.conn.flush();
        out.append(&mut self.queue);
    }

    fn create_layer(&mut self, kind: Kind, layer: u32, anchor: u32, exclusive: i32, margins: [i32; 4], keyboard: u32, want: (i32, i32)) -> Option<usize> {
        let surface = self.conn.new_id();
        let compositor = self.g.compositor;
        self.conn.send(Message::new(compositor, 0).uint(surface).finish());
        let role = self.conn.new_id();
        let shell = self.g.layer_shell;
        self.conn.send(Message::new(shell, 0).uint(role).uint(surface).uint(0).uint(layer).string("nook").finish());
        self.conn.send(Message::new(role, 0).uint(want.0.max(0) as u32).uint(want.1.max(0) as u32).finish());
        self.conn.send(Message::new(role, 1).uint(anchor).finish());
        self.conn.send(Message::new(role, 2).int(exclusive).finish());
        self.conn.send(Message::new(role, 3).int(margins[0]).int(margins[1]).int(margins[2]).int(margins[3]).finish());
        self.conn.send(Message::new(role, 4).uint(keyboard).finish());
        self.conn.send(Message::new(surface, 6).finish());
        self.layers.push(Layer { kind, surface, role, buffer: None, anchor, margins, want, size: want, origin: (0, 0), serial: None, configured: false });
        let deadline = now_ms() + 3000;
        loop {
            let index = self.layers.iter().position(|l| l.surface == surface)?;
            if self.layers[index].configured {
                break;
            }
            if now_ms() > deadline || !self.conn.alive() {
                let l = self.layers.remove(index);
                self.drop_layer(l);
                return None;
            }
            self.conn.flush();
            if self.conn.wait(50) {
                self.conn.read();
            }
            while let Some(e) = self.conn.next() {
                self.handle(e);
            }
        }
        let index = self.layers.iter().position(|l| l.surface == surface)?;
        self.ack(index);
        Some(index)
    }

    fn ack(&mut self, index: usize) {
        let (role, w, h) = (self.layers[index].role, self.layers[index].size.0, self.layers[index].size.1);
        if let Some(serial) = self.layers[index].serial.take() {
            self.conn.send(Message::new(role, 6).uint(serial).finish());
        }
        let fits = self.layers[index].buffer.as_ref().map(|b| (b.width as i32, b.height as i32) == (w, h)).unwrap_or(false);
        if !fits {
            if let Some(old) = self.layers[index].buffer.take() {
                old.destroy(&mut self.conn);
            }
            let shm = self.g.shm;
            self.layers[index].buffer = ShmBuffer::create(&mut self.conn, shm, w.max(1) as u32, h.max(1) as u32, 0);
            if let Some(b) = self.layers[index].buffer.as_mut() {
                b.pixels().fill(0);
            }
        }
    }

    fn drop_layer(&mut self, l: Layer) {
        self.conn.send(Message::new(l.role, 7).finish());
        self.conn.send(Message::new(l.surface, 0).finish());
        if let Some(b) = l.buffer {
            b.destroy(&mut self.conn);
        }
        self.conn.flush();
    }

    fn remove(&mut self, kind: Kind) -> Option<Area> {
        let index = self.layers.iter().position(|l| l.kind == kind)?;
        let l = self.layers.remove(index);
        let area = Area::new(l.origin.0, l.origin.1, l.size.0, l.size.1);
        self.drop_layer(l);
        Some(area)
    }

    fn area(&self, kind: Kind) -> Option<Area> {
        self.layers.iter().find(|l| l.kind == kind).map(|l| Area::new(l.origin.0, l.origin.1, l.size.0, l.size.1))
    }

    pub fn rebuild(&mut self, core: &mut Core, dock_height: i32, exclusive: i32) {
        for kind in [Kind::Desktop, Kind::Bar, Kind::Dock, Kind::Popup, Kind::Osd] {
            self.remove(kind);
        }
        self.create_layer(Kind::Desktop, LAYER_BOTTOM, ALL, -1, [0; 4], 0, (0, 0));
        if self.screen != (core.screen.width, core.screen.height) {
            core.screen.width = self.screen.0;
            core.screen.height = self.screen.1;
        }
        self.create_layer(Kind::Bar, LAYER_TOP, ANCHOR_TOP | ANCHOR_LEFT | ANCHOR_RIGHT, TOP_H, [0; 4], 0, (0, TOP_H));
        self.create_layer(Kind::Dock, LAYER_TOP, ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT, exclusive, [0; 4], 0, (0, dock_height.max(1)));
        core.damage_all();
    }

    pub fn ensure_dock(&mut self, core: &mut Core, height: i32, exclusive: i32) {
        if self.area(Kind::Dock).map(|a| a.h == height).unwrap_or(false) {
            return;
        }
        self.remove(Kind::Dock);
        if let Some(i) = self.create_layer(Kind::Dock, LAYER_TOP, ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT, exclusive, [0; 4], 0, (0, height.max(1))) {
            let l = &self.layers[i];
            core.add_damage(Area::new(l.origin.0, l.origin.1, l.size.0, l.size.1));
        }
    }

    pub fn configure_dock(&mut self, exclusive: i32) {
        if let Some(l) = self.layers.iter().find(|l| l.kind == Kind::Dock) {
            let (role, surface) = (l.role, l.surface);
            self.conn.send(Message::new(role, 2).int(exclusive).finish());
            self.conn.send(Message::new(surface, 6).finish());
            self.conn.flush();
        }
    }

    pub fn set_popup(&mut self, core: &mut Core, wanted: Option<Area>) {
        match (wanted, self.area(Kind::Popup)) {
            (None, Some(_)) => {
                self.remove(Kind::Popup);
                core.damage_all();
            }
            (Some(area), None) => {
                self.create_layer(Kind::Popup, LAYER_OVERLAY, ALL, -1, [0; 4], 1, (0, 0));
                core.add_damage(area);
            }
            _ => {}
        }
    }

    pub fn set_osd(&mut self, core: &mut Core, wanted: Option<Area>) {
        let current = self.area(Kind::Osd);
        if wanted == current {
            return;
        }
        if let Some(old) = self.remove(Kind::Osd) {
            core.add_damage(old);
        }
        if let Some(area) = wanted.filter(|a| !a.is_empty()) {
            self.create_layer(Kind::Osd, LAYER_OVERLAY, ANCHOR_TOP | ANCHOR_LEFT, 0, [area.y, 0, 0, area.x], 0, (area.w, area.h));
            core.add_damage(area);
        }
    }

    pub fn origin_of(&self, window: u32) -> Option<(i32, i32)> {
        self.layers.iter().find(|l| l.surface == window).map(|l| l.origin)
    }

    pub fn is_desktop(&self, window: u32) -> bool {
        self.layers.iter().any(|l| l.surface == window && l.kind == Kind::Desktop)
    }

    pub fn configured(&mut self, window: u32, area: Area) {
        if let Some(i) = self.layers.iter().position(|l| l.surface == window) {
            self.layers[i].origin = (area.x, area.y);
            self.layers[i].size = (area.w, area.h);
            self.ack(i);
        }
    }

    pub fn target(&mut self, kind: Kind) -> Option<(*mut u32, Area)> {
        let l = self.layers.iter_mut().find(|l| l.kind == kind)?;
        let area = Area::new(l.origin.0, l.origin.1, l.size.0, l.size.1);
        let b = l.buffer.as_mut()?;
        if (b.width as i32, b.height as i32) != l.size {
            return None;
        }
        Some((b.as_mut_ptr(), area))
    }

    pub fn present(&mut self, kind: Kind, rect: Area) {
        let Some(l) = self.layers.iter_mut().find(|l| l.kind == kind) else {
            return;
        };
        let Some(b) = l.buffer.as_mut() else {
            return;
        };
        b.busy = true;
        let (surface, buffer, origin) = (l.surface, b.id, l.origin);
        self.conn.send(Message::new(surface, 1).uint(buffer).int(0).int(0).finish());
        self.conn.send(Message::new(surface, 9).int(rect.x - origin.0).int(rect.y - origin.1).int(rect.w).int(rect.h).finish());
        self.conn.send(Message::new(surface, 6).finish());
        self.conn.flush();
    }

    pub fn input_region(&mut self, kind: Kind, areas: &[Area]) {
        let Some(l) = self.layers.iter().find(|l| l.kind == kind) else {
            return;
        };
        let (surface, origin) = (l.surface, l.origin);
        let region = self.conn.new_id();
        let compositor = self.g.compositor;
        self.conn.send(Message::new(compositor, 1).uint(region).finish());
        for a in areas {
            self.conn.send(Message::new(region, 1).int(a.x - origin.0).int(a.y - origin.1).int(a.w).int(a.h).finish());
        }
        self.conn.send(Message::new(surface, 5).uint(region).finish());
        self.conn.send(Message::new(region, 0).finish());
    }
}
