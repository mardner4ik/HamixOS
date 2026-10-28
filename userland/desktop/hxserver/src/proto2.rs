use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hxproto::{Event, Request, Text, Title};
use hxstd::sys;
use vellum::Area;

use crate::compositor::send_event;
use crate::compositor::window::Body;
use crate::Nook;

pub const WORKSPACES: u32 = 4;
const FIRST_LAYER_ID: u32 = 0x4000_0000;

pub struct LayerSurface {
    pub id: u32,
    pub pid: i64,
    pub shm: u32,
    pub pixels: *const u32,
    pub buf_w: i32,
    pub buf_h: i32,
    pub layer: u32,
    pub anchor: u32,
    pub exclusive: i32,
    pub margins: [i32; 4],
    pub keyboard: bool,
    pub area: Area,
    pub cursor: u32,
    pub tiles: Vec<u8>,
}

const TILE: i32 = 32;
const TILE_EMPTY: u8 = 0;
const TILE_MIXED: u8 = 1;
const TILE_OPAQUE: u8 = 2;

impl LayerSurface {
    fn tile_cols(&self) -> i32 {
        (self.buf_w + TILE - 1) / TILE
    }

    pub fn classify(&mut self, rect: Area) {
        let cols = self.tile_cols();
        let rows = (self.buf_h + TILE - 1) / TILE;
        if self.tiles.len() != (cols * rows) as usize {
            self.tiles = alloc::vec![TILE_MIXED; (cols * rows).max(0) as usize];
        }
        if self.pixels.is_null() {
            return;
        }
        let rect = rect.intersect(&Area::new(0, 0, self.buf_w, self.buf_h));
        if rect.is_empty() {
            return;
        }
        let src = unsafe { core::slice::from_raw_parts(self.pixels, (self.buf_w * self.buf_h) as usize) };
        for ty in rect.y / TILE..=(rect.bottom() - 1) / TILE {
            for tx in rect.x / TILE..=(rect.right() - 1) / TILE {
                let (x0, y0) = (tx * TILE, ty * TILE);
                let (x1, y1) = ((x0 + TILE).min(self.buf_w), (y0 + TILE).min(self.buf_h));
                let (mut any, mut all) = (false, true);
                for y in y0..y1 {
                    for p in &src[(y * self.buf_w + x0) as usize..(y * self.buf_w + x1) as usize] {
                        let a = p >> 24;
                        any |= a != 0;
                        all &= a == 255;
                    }
                    if any && !all {
                        break;
                    }
                }
                self.tiles[(ty * cols + tx) as usize] = if !any { TILE_EMPTY } else if all { TILE_OPAQUE } else { TILE_MIXED };
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Toplevel {
    id: u32,
    state: u32,
    workspace: u32,
    title: String,
    app_id: String,
    area: Area,
}

pub struct State {
    pub layers: Vec<LayerSurface>,
    next_layer: u32,
    subscribers: Vec<i64>,
    snapshot: Vec<Toplevel>,
    selection: Option<(String, String)>,
    pub drag: Option<(String, String)>,
    pub grab_layer: Option<u32>,
    pub hover_layer: Option<u32>,
    pub keyboard_layer: Option<u32>,
}

impl State {
    pub fn new() -> State {
        State {
            layers: Vec::new(),
            next_layer: FIRST_LAYER_ID,
            subscribers: Vec::new(),
            snapshot: Vec::new(),
            selection: None,
            drag: None,
            grab_layer: None,
            hover_layer: None,
            keyboard_layer: None,
        }
    }
}

fn blit(frame: &mut [u32], width: i32, clip: Area, l: &LayerSurface) {
    let (area, bw, bh) = (l.area, l.buf_w, l.buf_h);
    let src = unsafe { core::slice::from_raw_parts(l.pixels, (bw * bh) as usize) };
    let visible = clip.intersect(&area).intersect(&Area::new(area.x, area.y, bw, bh));
    if visible.is_empty() {
        return;
    }
    let cols = l.tile_cols();
    let tiled = l.tiles.len() as i32 == cols * ((bh + TILE - 1) / TILE);
    let first_ty = (visible.y - area.y) / TILE;
    let last_ty = (visible.bottom() - 1 - area.y) / TILE;
    let first_tx = (visible.x - area.x) / TILE;
    let last_tx = (visible.right() - 1 - area.x) / TILE;
    for ty in first_ty..=last_ty {
        for tx in first_tx..=last_tx {
            let kind = if tiled { l.tiles[(ty * cols + tx) as usize] } else { TILE_MIXED };
            if kind == TILE_EMPTY {
                continue;
            }
            let tile = Area::new(area.x + tx * TILE, area.y + ty * TILE, TILE, TILE).intersect(&visible);
            for y in tile.y..tile.bottom() {
                let srow = ((y - area.y) * bw + tile.x - area.x) as usize;
                let drow = (y * width + tile.x) as usize;
                let s = &src[srow..srow + tile.w as usize];
                let d = &mut frame[drow..drow + tile.w as usize];
                if kind == TILE_OPAQUE {
                    for (o, p) in d.iter_mut().zip(s.iter()) {
                        *o = p & 0x00FF_FFFF;
                    }
                    continue;
                }
                for (o, &p) in d.iter_mut().zip(s.iter()) {
                    let a = p >> 24;
                    if a == 255 {
                        *o = p & 0x00FF_FFFF;
                    } else if a != 0 {
                        let inv = 255 - a;
                        let mix = |shift: u32| ((((p >> shift) & 0xFF) + (((*o >> shift) & 0xFF) * inv) / 255).min(255)) << shift;
                        *o = mix(16) | mix(8) | mix(0);
                    }
                }
            }
        }
    }
}

fn layer_opaque_at(l: &LayerSurface, x: i32, y: i32) -> bool {
    let (lx, ly) = (x - l.area.x, y - l.area.y);
    if l.pixels.is_null() || lx < 0 || ly < 0 || lx >= l.buf_w || ly >= l.buf_h {
        return false;
    }
    unsafe { *l.pixels.add((ly * l.buf_w + lx) as usize) >> 24 != 0 }
}

impl Nook {
    fn layer_geometry(&self, l: &LayerSurface) -> Area {
        let (sw, sh) = (self.core.screen.width, self.core.screen.height);
        let [top, right, bottom, left] = l.margins;
        let horizontal = l.anchor & hxproto::ANCHOR_LEFT != 0 && l.anchor & hxproto::ANCHOR_RIGHT != 0;
        let vertical = l.anchor & hxproto::ANCHOR_TOP != 0 && l.anchor & hxproto::ANCHOR_BOTTOM != 0;
        let w = if l.buf_w == 0 || horizontal { (sw - left - right).max(1) } else { l.buf_w };
        let h = if l.buf_h == 0 || vertical { (sh - top - bottom).max(1) } else { l.buf_h };
        let x = match (l.anchor & hxproto::ANCHOR_LEFT != 0, l.anchor & hxproto::ANCHOR_RIGHT != 0) {
            (true, _) => left,
            (false, true) => sw - w - right,
            _ => (sw - w) / 2,
        };
        let y = match (l.anchor & hxproto::ANCHOR_TOP != 0, l.anchor & hxproto::ANCHOR_BOTTOM != 0) {
            (true, _) => top,
            (false, true) => sh - h - bottom,
            _ => (sh - h) / 2,
        };
        Area::new(x, y, w, h)
    }

    pub fn relayout_layers(&mut self) {
        for i in 0..self.v2.layers.len() {
            let area = self.layer_geometry(&self.v2.layers[i]);
            let old = self.v2.layers[i].area;
            if old != area {
                self.core.add_damage(old);
                self.core.add_damage(area);
                self.v2.layers[i].area = area;
                let l = &self.v2.layers[i];
                send_event(l.pid, Event::LayerConfigured { window: l.id, x: area.x, y: area.y, width: area.w as u32, height: area.h as u32 });
            }
        }
        self.update_work_area();
    }

    pub fn apply_exclusive_zones(&mut self) {
        let mut work = self.core.work;
        for l in self.v2.layers.iter().filter(|l| l.exclusive > 0) {
            let a = l.anchor;
            let e = l.exclusive;
            let only = |edge: u32, across: u32| a & edge != 0 && a & (hxproto::ANCHOR_TOP | hxproto::ANCHOR_BOTTOM | hxproto::ANCHOR_LEFT | hxproto::ANCHOR_RIGHT) & !(edge | across) == 0;
            if only(hxproto::ANCHOR_TOP, hxproto::ANCHOR_LEFT | hxproto::ANCHOR_RIGHT) {
                let top = (l.area.y + e).max(work.y);
                work.h -= top - work.y;
                work.y = top;
            } else if only(hxproto::ANCHOR_BOTTOM, hxproto::ANCHOR_LEFT | hxproto::ANCHOR_RIGHT) {
                work.h = work.h.min(l.area.bottom() - e - work.y);
            } else if only(hxproto::ANCHOR_LEFT, hxproto::ANCHOR_TOP | hxproto::ANCHOR_BOTTOM) {
                let left = (l.area.x + e).max(work.x);
                work.w -= left - work.x;
                work.x = left;
            } else if only(hxproto::ANCHOR_RIGHT, hxproto::ANCHOR_TOP | hxproto::ANCHOR_BOTTOM) {
                work.w = work.w.min(l.area.right() - e - work.x);
            }
        }
        work.w = work.w.max(160);
        work.h = work.h.max(120);
        self.core.work = work;
    }

    pub fn draw_layers(&mut self, low: bool) {
        let clip = self.core.screen.clip;
        let width = self.core.screen.width;
        for l in self.v2.layers.iter() {
            let wanted = if low { l.layer <= hxproto::LAYER_BOTTOM } else { l.layer == hxproto::LAYER_TOP };
            if !wanted || l.pixels.is_null() {
                continue;
            }
            blit(&mut self.core.screen.frame, width, clip, l);
        }
    }

    pub fn draw_overlay_layers(&mut self) {
        let clip = self.core.screen.clip;
        let width = self.core.screen.width;
        for l in self.v2.layers.iter().filter(|l| l.layer == hxproto::LAYER_OVERLAY && !l.pixels.is_null()) {
            blit(&mut self.core.screen.frame, width, clip, l);
        }
    }

    pub fn layer_at(&self, x: i32, y: i32, above_windows: bool) -> Option<usize> {
        (0..self.v2.layers.len()).rev().find(|&i| {
            let l = &self.v2.layers[i];
            (l.layer >= hxproto::LAYER_TOP) == above_windows && l.area.contains(x, y) && (!above_windows || layer_opaque_at(l, x, y))
        })
    }

    fn grab_overlay(&self) -> Option<usize> {
        (0..self.v2.layers.len()).rev().find(|&i| {
            let l = &self.v2.layers[i];
            l.layer == hxproto::LAYER_OVERLAY && l.keyboard && !l.pixels.is_null()
        })
    }

    pub fn overlay_grab_press(&mut self, x: i32, y: i32, button: u32) -> bool {
        let Some(index) = self.grab_overlay() else {
            return false;
        };
        let id = self.v2.layers[index].id;
        self.v2.grab_layer = Some(id);
        self.v2.keyboard_layer = Some(id);
        self.layer_mouse(index, x, y, hxproto::MOUSE_PRESS, button as i32);
        true
    }

    fn layer_index(&self, id: u32) -> Option<usize> {
        self.v2.layers.iter().position(|l| l.id == id)
    }

    pub fn layer_mouse(&mut self, index: usize, x: i32, y: i32, kind: u32, wheel: i32) {
        let l = &self.v2.layers[index];
        send_event(l.pid, Event::Mouse { window: l.id, x: x - l.area.x, y: y - l.area.y, buttons: self.core.buttons, kind, wheel });
    }

    pub fn layer_press(&mut self, x: i32, y: i32, button: u32, above_windows: bool) -> bool {
        let Some(index) = self.layer_at(x, y, above_windows) else {
            return false;
        };
        self.v2.grab_layer = Some(self.v2.layers[index].id);
        if self.v2.layers[index].keyboard {
            self.v2.keyboard_layer = Some(self.v2.layers[index].id);
        }
        self.layer_mouse(index, x, y, hxproto::MOUSE_PRESS, button as i32);
        true
    }

    pub fn layer_release(&mut self, x: i32, y: i32, button: u32) -> bool {
        let Some(id) = self.v2.grab_layer else {
            return false;
        };
        if self.core.buttons == 0 {
            self.v2.grab_layer = None;
        }
        if let Some(index) = self.layer_index(id) {
            self.layer_mouse(index, x, y, hxproto::MOUSE_RELEASE, button as i32);
        }
        true
    }

    pub fn layer_motion(&mut self, x: i32, y: i32) -> bool {
        let target = match self.v2.grab_layer {
            Some(id) => self.layer_index(id),
            None => self.grab_overlay().or_else(|| self.layer_at(x, y, true)).or_else(|| if self.core.window_at(x, y).is_none() { self.layer_at(x, y, false) } else { None }),
        };
        let target_id = target.map(|i| self.v2.layers[i].id);
        if target_id != self.v2.hover_layer {
            if let Some(old) = self.v2.hover_layer.and_then(|id| self.layer_index(id)) {
                let l = &self.v2.layers[old];
                send_event(l.pid, Event::Mouse { window: l.id, x: -1, y: -1, buttons: 0, kind: hxproto::MOUSE_LEAVE, wheel: 0 });
            }
            self.v2.hover_layer = target_id;
        }
        match target {
            Some(index) => {
                let cursor = self.v2.layers[index].cursor as usize;
                self.layer_mouse(index, x, y, hxproto::MOUSE_MOVE, 0);
                self.core.set_cursor(cursor);
                true
            }
            None => false,
        }
    }

    pub fn layer_wheel(&mut self, x: i32, y: i32, wheel: i32) -> bool {
        let hit = self.grab_overlay().or_else(|| self.layer_at(x, y, true)).or_else(|| if self.core.window_at(x, y).is_none() { self.layer_at(x, y, false) } else { None });
        match hit {
            Some(index) => {
                self.layer_mouse(index, x, y, hxproto::MOUSE_WHEEL, wheel);
                true
            }
            None => false,
        }
    }

    pub fn layer_key(&mut self, code: i32) -> bool {
        let Some(index) = self.grab_overlay().or_else(|| self.v2.keyboard_layer.and_then(|id| self.layer_index(id))) else {
            return false;
        };
        let l = &self.v2.layers[index];
        send_event(l.pid, Event::Key { window: l.id, code, mods: self.key_mods });
        true
    }

    fn remove_layer(&mut self, index: usize) {
        let l = self.v2.layers.remove(index);
        sys::shm_release(l.shm);
        self.core.add_damage(l.area);
        for slot in [&mut self.v2.grab_layer, &mut self.v2.hover_layer, &mut self.v2.keyboard_layer] {
            if *slot == Some(l.id) {
                *slot = None;
            }
        }
        self.update_work_area();
    }

    pub fn reap_layers(&mut self) {
        while let Some(index) = self.v2.layers.iter().position(|l| !sys::proc_alive(l.pid)) {
            self.remove_layer(index);
        }
        self.v2.subscribers.retain(|pid| sys::proc_alive(*pid));
    }

    fn welcome(&mut self, sender: i64) {
        let features = hxproto::FEATURE_LAYERS | hxproto::FEATURE_TOPLEVELS | hxproto::FEATURE_WORKSPACES | hxproto::FEATURE_SELECTION | hxproto::FEATURE_DND | hxproto::FEATURE_OUTPUT;
        send_event(sender, Event::Welcome { version: hxproto::VERSION, features });
        send_event(sender, Event::Output { width: self.core.screen.width as u32, height: self.core.screen.height as u32, scale: 100 });
        send_event(sender, Event::Workspaces { current: self.core.workspace, count: WORKSPACES });
    }

    pub fn switch_workspace(&mut self, index: u32) {
        if index >= WORKSPACES || index == self.core.workspace {
            return;
        }
        self.dismiss_client_popups();
        self.core.workspace = index;
        self.core.damage_all();
        self.sync_focus();
        for pid in self.v2.subscribers.clone() {
            send_event(pid, Event::Workspaces { current: index, count: WORKSPACES });
        }
    }

    fn toplevel_action(&mut self, id: u32, action: u32) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        if self.core.windows[index].is_popup() {
            return;
        }
        match action {
            hxproto::ACTION_ACTIVATE => {
                let ws = self.core.windows[index].workspace;
                self.switch_workspace(ws);
                self.focus(id);
            }
            hxproto::ACTION_MINIMIZE => self.minimize(id),
            hxproto::ACTION_CLOSE => self.close_window(id),
            hxproto::ACTION_TOGGLE_MAXIMIZE => {
                if self.core.windows[index].resizable() {
                    self.toggle_maximize(id);
                }
            }
            _ => {}
        }
    }

    fn toplevels(&self) -> Vec<Toplevel> {
        let focused = self.core.focused();
        self.core
            .windows
            .iter()
            .filter(|w| !w.is_popup())
            .map(|w| {
                let mut state = 0;
                let hiding = self.hiding(w.id);
                if Some(w.id) == focused && !hiding {
                    state |= hxproto::TOPLEVEL_ACTIVE;
                }
                if w.minimized || hiding {
                    state |= hxproto::TOPLEVEL_MINIMIZED;
                }
                if w.tiled == Some(crate::compositor::window::Snap::Maximize) {
                    state |= hxproto::TOPLEVEL_MAXIMIZED;
                }
                Toplevel { id: w.id, state, workspace: w.workspace, title: w.title.clone(), app_id: w.icon.clone(), area: w.outer() }
            })
            .collect()
    }

    pub fn sync_toplevels(&mut self) {
        if self.v2.subscribers.is_empty() {
            return;
        }
        let now = self.toplevels();
        let subscribers = self.v2.subscribers.clone();
        for t in now.iter() {
            let old = self.v2.snapshot.iter().find(|o| o.id == t.id);
            if !old.map(|o| o.state == t.state && o.workspace == t.workspace && o.title == t.title && o.app_id == t.app_id).unwrap_or(false) {
                for pid in subscribers.iter() {
                    send_event(*pid, Event::ToplevelState { toplevel: t.id, state: t.state, workspace: t.workspace, title: Title::new(&t.title), app_id: Title::new(&t.app_id) });
                }
            }
            if old.map(|o| o.area != t.area).unwrap_or(true) {
                for pid in subscribers.iter() {
                    send_event(*pid, Event::ToplevelGeometry { toplevel: t.id, x: t.area.x, y: t.area.y, width: t.area.w, height: t.area.h });
                }
            }
        }
        for old in self.v2.snapshot.iter() {
            if !now.iter().any(|t| t.id == old.id) {
                for pid in subscribers.iter() {
                    send_event(*pid, Event::ToplevelClosed { toplevel: old.id });
                }
            }
        }
        self.v2.snapshot = now;
    }

    pub fn finish_drag(&mut self, x: i32, y: i32) {
        let Some((mime, data)) = self.v2.drag.take() else {
            return;
        };
        let Some(index) = self.core.window_at(x, y) else {
            return;
        };
        let content = self.core.windows[index].content();
        if let (Body::Client { pid, .. }, true) = (&self.core.windows[index].body, content.contains(x, y)) {
            send_event(*pid, Event::Drop { window: self.core.windows[index].id, x: x - content.x, y: y - content.y, mime: Title::new(&mime), data: Text::new(&data) });
        }
    }

    pub fn v2_request(&mut self, sender: i64, request: &Request) -> bool {
        match *request {
            Request::Hello { .. } => self.welcome(sender),
            Request::CreateLayer { shm, width, height, layer, anchor, exclusive, margins, keyboard } => {
                if layer > hxproto::LAYER_OVERLAY || self.v2.layers.iter().filter(|l| l.pid == sender).count() >= 8 {
                    send_event(sender, Event::Rejected { reason: 4 });
                    return true;
                }
                let (w, h) = (width as i32, height as i32);
                let pixels = match self.core.adopt_buffer(shm, w, h, 1) {
                    Ok(p) => p,
                    Err(e) => {
                        send_event(sender, Event::Rejected { reason: e.reason() });
                        return true;
                    }
                };
                let id = self.v2.next_layer;
                self.v2.next_layer += 1;
                let mut surface = LayerSurface { id, pid: sender, shm, pixels, buf_w: w, buf_h: h, layer, anchor, exclusive, margins, keyboard: keyboard != 0, area: Area::new(0, 0, 0, 0), cursor: 0, tiles: Vec::new() };
                surface.classify(Area::new(0, 0, w, h));
                surface.area = self.layer_geometry(&surface);
                let area = surface.area;
                let grabs = surface.keyboard && layer == hxproto::LAYER_OVERLAY;
                self.v2.layers.push(surface);
                self.v2.layers.sort_by_key(|l| l.layer);
                if grabs {
                    self.v2.keyboard_layer = Some(id);
                    self.v2.grab_layer = None;
                }
                send_event(sender, Event::Created { window: id });
                send_event(sender, Event::LayerConfigured { window: id, x: area.x, y: area.y, width: area.w as u32, height: area.h as u32 });
                self.core.add_damage(area);
                self.update_work_area();
            }
            Request::ConfigureLayer { window, anchor, exclusive, margins } => {
                if let Some(index) = self.layer_index(window).filter(|i| self.v2.layers[*i].pid == sender) {
                    let l = &mut self.v2.layers[index];
                    l.anchor = anchor;
                    l.exclusive = exclusive;
                    l.margins = margins;
                    self.relayout_layers();
                }
            }
            Request::Attach { window, shm, width, height } if window >= FIRST_LAYER_ID => {
                if let Some(index) = self.layer_index(window).filter(|i| self.v2.layers[*i].pid == sender) {
                    let (w, h) = (width as i32, height as i32);
                    if let Ok(pixels) = self.core.adopt_buffer(shm, w, h, 1) {
                        let l = &mut self.v2.layers[index];
                        if l.shm != shm {
                            sys::shm_release(l.shm);
                        }
                        l.shm = shm;
                        l.pixels = pixels;
                        l.buf_w = w;
                        l.buf_h = h;
                        l.tiles.clear();
                        l.classify(Area::new(0, 0, w, h));
                        let area = l.area;
                        self.core.add_damage(area);
                        self.relayout_layers();
                    }
                }
            }
            Request::Present { window, x, y, width, height } if window >= FIRST_LAYER_ID => {
                if let Some(index) = self.layer_index(window).filter(|i| self.v2.layers[*i].pid == sender) {
                    self.v2.layers[index].classify(Area::new(x as i32, y as i32, width as i32, height as i32));
                    let a = self.v2.layers[index].area;
                    let damage = Area::new(a.x + x as i32, a.y + y as i32, width as i32, height as i32).intersect(&a);
                    self.core.add_damage(damage);
                }
            }
            Request::Destroy { window } if window >= FIRST_LAYER_ID => {
                if let Some(index) = self.layer_index(window).filter(|i| self.v2.layers[*i].pid == sender) {
                    self.remove_layer(index);
                }
            }
            Request::SetCursor { window, cursor } if window >= FIRST_LAYER_ID => {
                if let Some(index) = self.layer_index(window).filter(|i| self.v2.layers[*i].pid == sender) {
                    self.v2.layers[index].cursor = cursor.min(7);
                }
            }
            Request::SubscribeToplevels => {
                if !self.v2.subscribers.contains(&sender) {
                    self.v2.subscribers.push(sender);
                }
                for t in self.toplevels() {
                    send_event(sender, Event::ToplevelState { toplevel: t.id, state: t.state, workspace: t.workspace, title: Title::new(&t.title), app_id: Title::new(&t.app_id) });
                    send_event(sender, Event::ToplevelGeometry { toplevel: t.id, x: t.area.x, y: t.area.y, width: t.area.w, height: t.area.h });
                }
                send_event(sender, Event::Workspaces { current: self.core.workspace, count: WORKSPACES });
            }
            Request::ToplevelAction { toplevel, action } => self.toplevel_action(toplevel, action),
            Request::SetSelection { mime, data } => {
                self.v2.selection = Some((mime.as_str().to_string(), data.as_str().to_string()));
            }
            Request::GetSelection { mime } => {
                let wanted = mime.as_str();
                let reply = match &self.v2.selection {
                    Some((m, d)) if wanted.is_empty() || wanted == m => (m.clone(), d.clone()),
                    _ => (String::new(), String::new()),
                };
                send_event(sender, Event::Selection { mime: Title::new(&reply.0), data: Text::new(&reply.1) });
            }
            Request::SwitchWorkspace { index } => self.switch_workspace(index),
            Request::MoveToWorkspace { window, index } => {
                if index < WORKSPACES && (self.core.owned_by(window, sender) || self.v2.subscribers.contains(&sender)) {
                    if let Some(i) = self.core.index_of(window) {
                        let area = self.core.windows[i].with_shadow();
                        self.core.windows[i].workspace = index;
                        self.core.add_damage(area);
                        self.sync_focus();
                    }
                }
            }
            Request::StartDrag { window, mime, data } => {
                if self.core.owned_by(window, sender) && self.core.buttons != 0 {
                    self.v2.drag = Some((mime.as_str().to_string(), data.as_str().to_string()));
                }
            }
            _ => return false,
        }
        true
    }
}
