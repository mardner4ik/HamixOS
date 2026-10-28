use alloc::vec::Vec;
use hxclient::v2::{Connection, LayerSpec, LayerSurface, ANCHOR_BOTTOM, ANCHOR_LEFT, ANCHOR_RIGHT, ANCHOR_TOP, LAYER_BOTTOM, LAYER_OVERLAY, LAYER_TOP};
use hxproto::{Event, Request, Title};
use hxstd::sys;
use vellum::Area;

use crate::shell::{Core, Kind};
use crate::TOP_H;

pub struct Platform {
    conn: Connection,
    surfaces: Surfaces,
}

impl Platform {
    pub fn open() -> Result<Platform, &'static str> {
        let conn = Connection::open()?;
        if !conn.has(hxproto::FEATURE_LAYERS) {
            return Err("the display server has no layer surfaces");
        }
        conn.subscribe_toplevels();
        Ok(Platform { conn, surfaces: Surfaces::new() })
    }

    pub fn screen(&self) -> (i32, i32) {
        (self.conn.screen.0 as i32, self.conn.screen.1 as i32)
    }

    pub fn workspace(&self) -> u32 {
        self.conn.workspaces.0
    }

    pub fn alive(&self) -> bool {
        sys::proc_alive(self.conn.server)
    }

    fn send(&self, request: Request) {
        let mut buf = [0u8; hxproto::MESSAGE_MAX];
        let len = request.encode(&mut buf);
        sys::msg_send(self.conn.server, &buf[..len]);
    }

    pub fn toast(&mut self, title: &str, body: &str, icon: &str) {
        self.send(Request::Toast { title: Title::new(title), body: Title::new(body), icon: Title::new(icon) });
    }

    pub fn shell_action(&mut self, action: u32) {
        self.send(Request::ShellAction { action });
    }

    pub fn launched(&mut self, pid: i64, key: &str) {
        self.send(Request::SetAppId { window: 0, pid: pid as u32, app_id: Title::new(key) });
    }

    pub fn minimize_target(&mut self, toplevel: u32, area: Area) {
        self.send(Request::ToplevelRectangle { toplevel, x: area.x, y: area.y, width: area.w, height: area.h });
    }

    pub fn set_cursor(&mut self, window: u32, cursor: u32) {
        self.send(Request::SetCursor { window, cursor });
    }

    pub fn toplevel_action(&mut self, toplevel: u32, action: u32) {
        self.conn.toplevel_action(toplevel, action);
    }

    pub fn screen_changed(&mut self, width: u32, height: u32) {
        self.conn.screen = (width, height, 100);
    }

    pub fn pump(&mut self, timeout: u64, out: &mut Vec<Event>) {
        let mut entries = [sys::WaitEntry::mailbox()];
        sys::wait_any(&mut entries, Some(timeout));
        while let Some(event) = self.conn.take_pending() {
            out.push(event);
        }
        let mut buf = [0u8; hxproto::MESSAGE_MAX];
        while let Some((sender, len)) = sys::msg_recv(&mut buf, 0) {
            if sender != self.conn.server {
                continue;
            }
            if let Some(event) = Event::decode(&buf[..len]) {
                out.push(event);
            }
            while let Some(event) = self.conn.take_pending() {
                out.push(event);
            }
        }
    }

    pub fn rebuild(&mut self, core: &mut Core, dock_height: i32, exclusive: i32) {
        self.surfaces.rebuild(&mut self.conn, core, dock_height, exclusive);
    }

    pub fn ensure_dock(&mut self, core: &mut Core, height: i32, exclusive: i32) {
        self.surfaces.ensure_dock(&mut self.conn, core, height, exclusive);
    }

    pub fn configure_dock(&mut self, exclusive: i32) {
        self.surfaces.configure_dock(exclusive);
    }

    pub fn set_popup(&mut self, core: &mut Core, wanted: Option<Area>) {
        self.surfaces.set_popup(&mut self.conn, core, wanted);
    }

    pub fn set_osd(&mut self, core: &mut Core, wanted: Option<Area>) {
        self.surfaces.set_osd(&mut self.conn, core, wanted);
    }

    pub fn origin_of(&self, window: u32) -> Option<(i32, i32)> {
        self.surfaces.origin_of(window)
    }

    pub fn is_desktop(&self, window: u32) -> bool {
        self.surfaces.is_desktop(window)
    }

    pub fn configured(&mut self, window: u32, area: Area) {
        self.surfaces.configured(window, area);
    }

    pub fn target(&mut self, kind: Kind) -> Option<(*mut u32, Area)> {
        self.surfaces.target(kind)
    }

    pub fn present(&mut self, kind: Kind, rect: Area) {
        self.surfaces.present(kind, rect);
    }

    pub fn input_region(&mut self, _kind: Kind, _areas: &[Area]) {}
}

pub struct Surfaces {
    desktop: Option<LayerSurface>,
    bar: Option<LayerSurface>,
    dock: Option<LayerSurface>,
    popup: Option<LayerSurface>,
    osd: Option<LayerSurface>,
}

fn open(conn: &mut Connection, layer: u32, anchor: u32, exclusive: i32, margins: [i32; 4], keyboard: bool, width: u32, height: u32) -> Option<LayerSurface> {
    LayerSurface::open(conn, &LayerSpec { layer, anchor, exclusive, margins, keyboard, width, height }).ok()
}

fn area_of(l: &LayerSurface) -> Area {
    Area::new(l.x, l.y, l.width as i32, l.height as i32)
}

impl Surfaces {
    pub fn new() -> Surfaces {
        Surfaces { desktop: None, bar: None, dock: None, popup: None, osd: None }
    }

    fn get(&self, kind: Kind) -> Option<&LayerSurface> {
        match kind {
            Kind::Desktop => self.desktop.as_ref(),
            Kind::Bar => self.bar.as_ref(),
            Kind::Dock => self.dock.as_ref(),
            Kind::Popup => self.popup.as_ref(),
            Kind::Osd => self.osd.as_ref(),
        }
    }

    fn all(&self) -> [Option<&LayerSurface>; 5] {
        [self.desktop.as_ref(), self.bar.as_ref(), self.dock.as_ref(), self.popup.as_ref(), self.osd.as_ref()]
    }

    pub fn rebuild(&mut self, conn: &mut Connection, core: &mut Core, dock_height: i32, exclusive: i32) {
        self.desktop = None;
        self.bar = None;
        self.dock = None;
        self.popup = None;
        self.osd = None;
        let all = ANCHOR_TOP | ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT;
        self.desktop = open(conn, LAYER_BOTTOM, all, 0, [0; 4], false, 0, 0);
        self.bar = open(conn, LAYER_TOP, ANCHOR_TOP | ANCHOR_LEFT | ANCHOR_RIGHT, TOP_H, [0; 4], false, 0, TOP_H as u32);
        self.dock = open(conn, LAYER_TOP, ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT, exclusive, [0; 4], false, 0, dock_height.max(1) as u32);
        core.damage_all();
    }

    pub fn dock_height(&self) -> i32 {
        self.dock.as_ref().map(|d| d.height as i32).unwrap_or(0)
    }

    pub fn ensure_dock(&mut self, conn: &mut Connection, core: &mut Core, height: i32, exclusive: i32) {
        if self.dock.is_some() && self.dock_height() == height {
            return;
        }
        self.dock = None;
        self.dock = open(conn, LAYER_TOP, ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT, exclusive, [0; 4], false, 0, height.max(1) as u32);
        if let Some(d) = self.dock.as_ref() {
            core.add_damage(area_of(d));
        }
    }

    pub fn configure_dock(&self, exclusive: i32) {
        if let Some(d) = self.dock.as_ref() {
            d.configure(ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT, exclusive, [0; 4]);
        }
    }

    fn floating(slot: &mut Option<LayerSurface>, conn: &mut Connection, core: &mut Core, wanted: Option<Area>, keyboard: bool) {
        let current = slot.as_ref().map(area_of);
        match wanted {
            None => {
                if let Some(area) = current {
                    *slot = None;
                    core.add_damage(area);
                }
            }
            Some(area) if Some(area) == current => {}
            Some(area) => {
                *slot = None;
                if area.is_empty() {
                    return;
                }
                *slot = open(conn, LAYER_OVERLAY, ANCHOR_TOP | ANCHOR_LEFT, 0, [area.y, 0, 0, area.x], keyboard, area.w as u32, area.h as u32);
                core.add_damage(area);
            }
        }
    }

    pub fn set_popup(&mut self, conn: &mut Connection, core: &mut Core, wanted: Option<Area>) {
        Self::floating(&mut self.popup, conn, core, wanted, true);
    }

    pub fn set_osd(&mut self, conn: &mut Connection, core: &mut Core, wanted: Option<Area>) {
        Self::floating(&mut self.osd, conn, core, wanted, false);
    }

    pub fn origin_of(&self, window: u32) -> Option<(i32, i32)> {
        self.all().into_iter().flatten().find(|l| l.id == window).map(|l| (l.x, l.y))
    }

    pub fn is_desktop(&self, window: u32) -> bool {
        self.desktop.as_ref().map(|d| d.id == window).unwrap_or(false)
    }

    pub fn configured(&mut self, window: u32, area: Area) {
        for slot in [&mut self.desktop, &mut self.bar, &mut self.dock, &mut self.popup, &mut self.osd] {
            if let Some(l) = slot.as_mut().filter(|l| l.id == window) {
                l.x = area.x;
                l.y = area.y;
            }
        }
    }

    pub fn target(&mut self, kind: Kind) -> Option<(*mut u32, Area)> {
        let slot = match kind {
            Kind::Desktop => self.desktop.as_mut(),
            Kind::Bar => self.bar.as_mut(),
            Kind::Dock => self.dock.as_mut(),
            Kind::Popup => self.popup.as_mut(),
            Kind::Osd => self.osd.as_mut(),
        }?;
        let area = area_of(slot);
        Some((slot.pixels().as_mut_ptr(), area))
    }

    pub fn present(&self, kind: Kind, rect: Area) {
        if let Some(l) = self.get(kind) {
            l.present_area((rect.x - l.x) as u32, (rect.y - l.y) as u32, rect.w as u32, rect.h as u32);
        }
    }
}
