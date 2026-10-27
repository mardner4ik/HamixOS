use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::sys::{self, MOUSE_LEFT, MOUSE_RIGHT};
use hxproto::{Event, Request};
use vellum::Area;

use crate::anim::{Effect, Finish};
use crate::compositor::window::{edge_cursor, Body, Snap, SHADOW_SIZE, TITLE_H};
use crate::compositor::send_event;
use crate::{app_by_icon, apps, Grab, MenuAction, MenuItem, Nook, Popup, KEY_SNAP_DOWN, KEY_SNAP_LEFT, KEY_SNAP_UP};

impl Nook {
    pub fn focus(&mut self, id: u32) {
        if self.core.raise(id) {
            self.start_animation(id, Effect::Restore, None, Finish::None);
        }
        self.sync_focus();
    }

    pub fn sync_focus(&mut self) {
        if !self.core.sync_focus() {
            return;
        }
        let region = self.dock_region();
        self.core.add_damage(region);
    }

    fn new_window(&mut self, title: &str, icon: &str, w: i32, h: i32, body: Body) -> u32 {
        self.preload_window_icon(icon);
        self.core.new_window(title, icon, w, h, body)
    }

    pub fn preload_window_icon(&mut self, icon: &str) {
        if icon.is_empty() {
            return;
        }
        let name = format!("apps/{}-24", icon);
        self.ui.load_icon(&name);
        self.sized_icon(&name, 18);
    }

    pub fn dismiss_client_popups(&mut self) {
        let popups: Vec<u32> = self.core.windows.iter().filter(|w| w.is_popup() && w.parent != Some(0)).map(|w| w.id).collect();
        for id in popups {
            self.remove_window(id, true);
        }
    }

    pub fn add_internal(&mut self, title: &str, icon: &str, w: i32, h: i32, body: Body) {
        let id = self.new_window(title, icon, w, h, body);
        self.focus(id);
    }

    pub fn open_about(&mut self) {
        if let Some(id) = self.core.windows.iter().find(|w| matches!(w.body, Body::About)).map(|w| w.id) {
            self.focus(id);
            return;
        }
        self.add_internal("About HamixOS", "hamix", 460, TITLE_H + 320, Body::About);
    }

    pub fn remove_window(&mut self, id: u32, notify: bool) {
        if self.core.index_of(id).is_none() {
            return;
        }
        for child in self.core.children_of(id) {
            self.remove_window(child, notify);
        }
        if self.core.take_window(id, notify).is_none() {
            return;
        }
        match self.grab {
            Grab::Move { id: g, .. } | Grab::Resize { id: g, .. } | Grab::Client(g) if g == id => self.grab = Grab::None,
            _ => {}
        }
        if self.hover_client == Some(id) {
            self.hover_client = None;
        }
        self.switcher = None;
        let region = self.dock_region();
        self.core.add_damage(region);
        self.sync_focus();
    }

    pub fn close_window(&mut self, id: u32) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        if let Body::Client { pid, .. } = self.core.windows[index].body {
            send_event(pid, Event::Close { window: id });
        }
        self.start_animation(id, Effect::Close, None, Finish::None);
        self.remove_window(id, false);
    }

    pub fn minimize(&mut self, id: u32) {
        if self.animating(id) {
            return;
        }
        self.dismiss_client_popups();
        if self.start_animation(id, Effect::Minimize, None, Finish::Hide) {
            return;
        }
        if let Some(index) = self.core.index_of(id) {
            self.core.windows[index].minimized = true;
            let area = self.core.windows[index].with_shadow();
            self.core.add_damage(area);
            self.sync_focus();
        }
    }

    pub fn quarter(&self, corner: u8) -> Area {
        let work = self.core.work;
        let half_w = work.w / 2;
        let half_h = work.h / 2;
        match corner {
            0 => Area::new(work.x, work.y, half_w, half_h),
            1 => Area::new(work.x + half_w, work.y, work.w - half_w, half_h),
            2 => Area::new(work.x, work.y + half_h, half_w, work.h - half_h),
            _ => Area::new(work.x + half_w, work.y + half_h, work.w - half_w, work.h - half_h),
        }
    }

    pub fn keyboard_snap(&mut self, code: i32) {
        let Some(id) = self.core.focused() else {
            return;
        };
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        if !self.core.windows[index].resizable() {
            return;
        }
        let work = self.core.work;
        let current = self.core.windows[index].outer();
        let half = Area::new(work.x, work.y, work.w / 2, work.h);
        let other_half = Area::new(work.x + work.w / 2, work.y, work.w - work.w / 2, work.h);
        match code {
            KEY_SNAP_UP => {
                if self.core.windows[index].tiled.is_none() {
                    self.tile(id, Snap::Maximize, work);
                } else if current.w < work.w {
                    self.tile(id, Snap::Maximize, work);
                }
            }
            KEY_SNAP_DOWN => {
                if self.core.windows[index].tiled.is_some() {
                    self.toggle_maximize(id);
                } else {
                    self.minimize(id);
                }
            }
            KEY_SNAP_LEFT => {
                let target = if current == other_half { work } else { half };
                let snap = if target == work { Snap::Maximize } else { Snap::Left };
                self.tile(id, snap, target);
            }
            _ => {
                let target = if current == half { work } else { other_half };
                let snap = if target == work { Snap::Maximize } else { Snap::Right };
                self.tile(id, snap, target);
            }
        }
    }

    fn client_window_count(&self, sender: i64) -> usize {
        self.core.windows.iter().filter(|w| w.client_pid() == Some(sender)).count()
    }

    fn client_window_limit(&self, sender: i64) -> usize {
        if self.trusted_bridge(sender) { 512 } else { 64 }
    }

    pub fn trusted_bridge(&self, sender: i64) -> bool {
        sender > 0 && (sender == self.wayland || sender == sys::service_lookup("hxwayland"))
    }

    pub fn set_window_icon(&mut self, window: u32, icon: String) {
        let Some(index) = self.core.index_of(window) else {
            return;
        };
        if self.core.windows[index].icon == icon {
            return;
        }
        self.preload_window_icon(&icon);
        let before = self.dock_region();
        self.core.windows[index].icon = icon;
        let area = self.core.windows[index].title_bar();
        self.core.add_damage(area);
        self.core.add_damage(before);
        let region = self.dock_region();
        self.core.add_damage(region);
    }

    fn launched_ancestor(&self, pid: i64) -> Option<usize> {
        if pid <= 0 {
            return None;
        }
        let table = sys::proc_list();
        let mut current = pid;
        for _ in 0..3 {
            let parent = table.iter().find(|p| p.pid == current).map(|p| p.parent)?;
            if parent <= 1 {
                return None;
            }
            if let Some((_, app)) = self.launched.iter().find(|(p, _)| *p == parent) {
                let app = *app;
                let launcher_like = apps::get(app).roles.iter().any(|r| r == crate::ROLE_TERMINAL || r == crate::ROLE_FILES);
                return if launcher_like { None } else { Some(app) };
            }
            current = parent;
        }
        None
    }

    pub fn identify_window(&mut self, window: u32, pid: i64, app_id: &str) {
        let mut candidates: Vec<String> = app_id.split('\n').map(String::from).filter(|s| !s.trim().is_empty()).collect();
        if pid > 0 {
            if let Some(name) = sys::proc_list().into_iter().find(|p| p.pid == pid).map(|p| p.name) {
                let base = String::from(name.rsplit('/').next().unwrap_or(&name));
                if !base.is_empty() && !candidates.contains(&base) {
                    candidates.push(base);
                }
            }
        }
        let exact = if pid > 0 { self.launched.iter().find(|(p, _)| *p == pid).map(|(_, a)| *a) } else { None };
        let resolved = exact
            .filter(|a| apps::alive(*a))
            .or_else(|| apps::match_identity(&candidates))
            .or_else(|| self.launched_ancestor(pid).filter(|a| apps::alive(*a)));
        let icon = match resolved {
            Some(app) => apps::get(app).key.clone(),
            None if !candidates.is_empty() => apps::adhoc_key(&mut self.ui, &candidates),
            None => String::from(apps::GENERIC_LINUX_ICON),
        };
        self.set_window_icon(window, icon);
    }

    pub fn app_windows(&self, app: usize) -> Vec<u32> {
        self.core.windows.iter().filter(|w| w.icon == apps::get(app).key).map(|w| w.id).collect()
    }

    pub fn window_press(&mut self, x: i32, y: i32, button: u32) -> bool {
        if button == MOUSE_LEFT {
            if let Some((index, edges)) = self.core.resize_edge_at(x, y) {
                let id = self.core.windows[index].id;
                if self.core.focused() != Some(id) {
                    self.focus(id);
                }
                let index = self.core.index_of(id).unwrap();
                self.grab = Grab::Resize { id, edges, start: self.core.windows[index].outer(), px: x, py: y };
                self.core.set_cursor(edge_cursor(edges));
                return true;
            }
        }
        let Some(index) = self.core.window_at(x, y) else {
            return false;
        };
        let id = self.core.windows[index].id;
        if self.core.focused() != Some(id) {
            self.focus(id);
        }
        let index = self.core.index_of(id).unwrap();
        let (close, maximize, minimize, title_bar, content, resizable) = {
            let w = &self.core.windows[index];
            (w.close_button(), w.maximize_button(), w.minimize_button(), w.title_bar(), w.content(), w.resizable())
        };
        if title_bar.contains(x, y) {
            if button == MOUSE_LEFT {
                if close.contains(x, y) {
                    self.close_window(id);
                    return true;
                }
                if resizable && maximize.contains(x, y) {
                    self.toggle_maximize(id);
                    return true;
                }
                if minimize.contains(x, y) {
                    self.minimize(id);
                    return true;
                }
                let now = sys::uptime_ms();
                let double = self.last_click.1 == -2 && self.last_click.2 == id as i32 && now.saturating_sub(self.last_click.0) < 450;
                self.last_click = (now, -2, id as i32);
                if double && resizable {
                    self.toggle_maximize(id);
                    return true;
                }
                let w = &self.core.windows[index];
                self.grab = Grab::Move { id, dx: x - w.x, dy: y - w.y };
                self.core.set_cursor(3);
            } else if button == MOUSE_RIGHT {
                let maximized = self.core.windows[index].tiled.is_some();
                let mut items = Vec::new();
                if resizable {
                    items.push(MenuItem { label: String::from(if maximized { "Restore" } else { "Maximize" }), icon: if maximized { "ui/restore" } else { "ui/maximize" }, action: MenuAction::ToggleMaximize(id), danger: false });
                }
                items.push(MenuItem { label: String::from("Minimize"), icon: "ui/minimize", action: MenuAction::Minimize(id), danger: false });
                if let Some(app) = app_by_icon(&self.core.windows[index].icon) {
                    if self.pinned.contains(&app) {
                        items.push(MenuItem { label: String::from("Unpin from dock"), icon: "ui/unpin", action: MenuAction::UnpinApp(app), danger: false });
                    } else {
                        items.push(MenuItem { label: String::from("Pin to dock"), icon: "ui/pin", action: MenuAction::PinApp(app), danger: false });
                    }
                }
                items.push(MenuItem { label: String::from("Close"), icon: "ui/close", action: MenuAction::CloseWindow(id), danger: true });
                self.set_popup(Popup::Menu { x, y, items });
            }
            return true;
        }
        let area = self.core.windows[index].outer();
        match &mut self.core.windows[index].body {
            Body::Client { pid, .. } => {
                if content.contains(x, y) {
                    let pid = *pid;
                    self.grab = Grab::Client(id);
                    send_event(pid, Event::Mouse { window: id, x: x - content.x, y: y - content.y, buttons: self.core.buttons, kind: hxproto::MOUSE_PRESS, wheel: button as i32 });
                }
            }
            Body::Auth { .. } => {
                let ok = Area::new(content.right() - 112, content.bottom() - 52, 96, 36);
                let cancel = Area::new(content.right() - 216, content.bottom() - 52, 96, 36);
                if ok.contains(x, y) {
                    self.submit_auth(id);
                } else if cancel.contains(x, y) {
                    self.remove_window(id, false);
                }
                let _ = area;
            }
            Body::About => {}
        }
        true
    }

    pub fn drag_move(&mut self, id: u32, x: i32, y: i32, dx: i32, dy: i32) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        let mut dx = dx;
        if self.core.windows[index].tiled.is_some() {
            let Some(restore) = self.core.windows[index].restore else {
                return;
            };
            let current = self.core.windows[index].outer();
            if (x - (current.x + dx)).abs() < 6 && (y - (current.y + dy)).abs() < 6 {
                return;
            }
            dx = (dx as i64 * restore.w as i64 / current.w.max(1) as i64) as i32;
            self.core.windows[index].tiled = None;
            self.core.windows[index].restore = None;
            let area = Area::new(x - dx, y - dy, restore.w, restore.h);
            let full = self.core.windows[index].with_shadow().union(&area.expand(SHADOW_SIZE));
            self.core.set_geometry(index, area);
            self.core.add_damage(full);
            self.grab = Grab::Move { id, dx, dy };
            self.core.request_client_size(id, true);
        }
        let w = self.core.windows[index].w;
        let nx = (x - dx).clamp(-w + 80, self.core.screen.width - 80);
        let ny = (y - dy).clamp(self.core.work.y, self.core.screen.height - TITLE_H);
        let area = Area::new(nx, ny, w, self.core.windows[index].h);
        self.core.set_geometry(index, area);
        let preview = if self.core.windows[index].resizable() {
            let work = self.core.work;
            if y <= work.y + 1 {
                Some(work)
            } else if x <= 1 {
                Some(Area::new(work.x, work.y, work.w / 2, work.h))
            } else if x >= self.core.screen.width - 2 {
                Some(Area::new(work.x + work.w / 2, work.y, work.w - work.w / 2, work.h))
            } else {
                None
            }
        } else {
            None
        };
        if preview != self.snap_preview {
            if let Some(old) = self.snap_preview {
                self.core.add_damage(old.expand(2));
            }
            if let Some(new) = preview {
                self.core.add_damage(new.expand(2));
            }
            self.snap_preview = preview;
        }
    }

    pub fn finish_move(&mut self, id: u32) {
        let Some(preview) = self.snap_preview.take() else {
            return;
        };
        self.core.add_damage(preview.expand(2));
        let (x, _) = self.core.pointer;
        let snap = if preview.w == self.core.screen.width { Snap::Maximize } else if x <= 1 { Snap::Left } else { Snap::Right };
        self.tile(id, snap, preview);
    }

    pub fn tile(&mut self, id: u32, snap: Snap, area: Area) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        if self.core.windows[index].outer() != area {
            self.start_animation(id, Effect::Snap, Some(area), Finish::None);
        }
        if self.core.windows[index].tiled.is_none() {
            self.core.windows[index].restore = Some(self.core.windows[index].outer());
        }
        self.core.windows[index].tiled = Some(snap);
        let old = self.core.windows[index].with_shadow();
        self.core.set_geometry(index, area);
        self.core.add_damage(old);
        self.core.request_client_size(id, true);
    }

    pub fn toggle_maximize(&mut self, id: u32) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        if !self.core.windows[index].resizable() {
            return;
        }
        if self.core.windows[index].tiled.is_some() {
            let restore = self.core.windows[index].restore.take();
            self.core.windows[index].tiled = None;
            let target = restore.unwrap_or_else(|| {
                let o = self.core.windows[index].outer();
                Area::new(o.x + 40, o.y + 40, o.w * 2 / 3, o.h * 2 / 3)
            });
            let old = self.core.windows[index].outer();
            if old != target {
                self.start_animation(id, Effect::Snap, Some(target), Finish::None);
            }
            self.core.set_geometry(index, target);
            self.core.add_damage(old);
            self.core.request_client_size(id, true);
        } else {
            let area = self.core.work;
            self.tile(id, Snap::Maximize, area);
        }
    }

    pub fn drag_resize(&mut self, id: u32, edges: u8, start: Area, dx: i32, dy: i32) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        let area = self.core.resolve_resize(index, edges, start, dx, dy);
        self.core.set_geometry(index, area);
        self.core.request_client_size(id, false);
    }

    pub fn flush_resize(&mut self, now: u64, force: bool) {
        if let Grab::Resize { id, .. } = self.grab {
            if force || now.saturating_sub(self.core.resize_sent) >= 40 {
                self.core.request_client_size(id, true);
            }
        }
    }

    pub fn cycle_switcher(&mut self) {
        let count = self.core.windows.iter().filter(|w| !w.is_popup() && (!w.minimized || w.client_pid().is_some())).count();
        if count < 2 && self.switcher.is_none() {
            if let Some(w) = self.core.windows.iter().find(|w| w.minimized) {
                let id = w.id;
                self.focus(id);
            }
            return;
        }
        let next = match self.switcher {
            Some(i) => (i + 1) % count.max(1),
            None => 1 % count.max(1),
        };
        self.switcher = Some(next);
        self.switcher_until = sys::uptime_ms() + 900;
        let area = self.switcher_area();
        self.core.add_damage(area.expand(24));
    }

    pub fn switcher_order(&self) -> Vec<u32> {
        self.core.windows.iter().rev().filter(|w| !w.is_popup() && (!w.minimized || w.client_pid().is_some())).map(|w| w.id).collect()
    }

    pub fn switcher_area(&self) -> Area {
        let count = self.switcher_order().len().max(1) as i32;
        let w = (count * 96 + 24).min(self.core.screen.width - 40);
        Area::new((self.core.screen.width - w) / 2, self.core.screen.height / 2 - 70, w, 140)
    }

    pub fn tick_switcher(&mut self, now: u64) {
        if self.switcher.is_some() && now >= self.switcher_until {
            self.finish_switcher();
        }
    }

    pub fn finish_switcher(&mut self) {
        let Some(index) = self.switcher.take() else {
            return;
        };
        let area = self.switcher_area();
        self.core.add_damage(area.expand(24));
        if let Some(id) = self.switcher_order().get(index).copied() {
            self.focus(id);
        }
    }

    pub fn client_request(&mut self, sender: i64, request: Request) {
        match request {
            Request::CreateWindow { shm, width, height, title } => {
                if self.client_window_count(sender) >= self.client_window_limit(sender) {
                    send_event(sender, Event::Rejected { reason: 4 });
                    return;
                }
                let (w, h) = (width as i32, height as i32);
                let pixels = match self.core.adopt_buffer(shm, w, h, 16) {
                    Ok(p) => p,
                    Err(e) => {
                        send_event(sender, Event::Rejected { reason: e.reason() });
                        return;
                    }
                };
                let icon = self.launched.iter().find(|(p, _)| *p == sender).map(|(_, a)| apps::get(*a).key.clone()).unwrap_or_default();
                let work = self.core.work;
                let outer_w = w.min(self.core.screen.width);
                let outer_h = (h + TITLE_H).min(work.h);
                let id = self.new_window(title.as_str(), &icon, outer_w, outer_h, Body::Client { pid: sender, shm, pixels, width: w, height: h });
                if let Some(index) = self.core.index_of(id) {
                    self.core.windows[index].requested = (w, h);
                }
                send_event(sender, Event::Created { window: id });
                if outer_w != w || outer_h != h + TITLE_H {
                    self.core.request_client_size(id, true);
                }
                self.focus(id);
                self.defer_open(id);
            }
            Request::CreatePopup { shm, width, height, parent, x, y } => {
                if self.client_window_count(sender) >= self.client_window_limit(sender) {
                    send_event(sender, Event::Rejected { reason: 4 });
                    return;
                }
                let (w, h) = (width as i32, height as i32);
                let parent_ok = if parent == 0 { self.trusted_bridge(sender) } else { self.core.owned_by(parent, sender) };
                if !parent_ok {
                    send_event(sender, Event::Rejected { reason: 1 });
                    return;
                }
                let pixels = match self.core.adopt_buffer(shm, w, h, 1) {
                    Ok(p) => p,
                    Err(e) => {
                        send_event(sender, Event::Rejected { reason: e.reason() });
                        return;
                    }
                };
                let area = self.core.popup_area(parent, x, y, w, h);
                let id = self.core.new_popup(parent, area, Body::Client { pid: sender, shm, pixels, width: w, height: h });
                send_event(sender, Event::Created { window: id });
            }
            Request::MovePopup { window, x, y } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) && self.core.windows[index].is_popup() {
                        let parent = self.core.windows[index].parent.unwrap_or(0);
                        let (w, h) = (self.core.windows[index].w, self.core.windows[index].h);
                        let area = self.core.popup_area(parent, x, y, w, h);
                        self.core.set_geometry(index, area);
                    }
                }
            }
            Request::SetFrame { window, decorated } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) && !self.core.windows[index].is_popup() {
                        let frameless = decorated == 0;
                        if self.core.windows[index].frameless != frameless {
                            let old = self.core.windows[index].with_shadow();
                            let o = self.core.windows[index].outer();
                            self.core.windows[index].frameless = frameless;
                            let nh = if frameless { o.h - TITLE_H } else { o.h + TITLE_H };
                            self.core.set_geometry(index, Area::new(o.x, o.y, o.w, nh.max(1)));
                            self.core.add_damage(old);
                            let area = self.core.windows[index].with_shadow();
                            self.core.add_damage(area);
                            self.core.send_placements();
                        }
                    }
                }
            }
            Request::BeginMove { window } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) && !self.core.windows[index].is_popup() && self.core.buttons != 0 {
                        let (px, py) = self.core.pointer;
                        let w = &self.core.windows[index];
                        self.grab = Grab::Move { id: window, dx: px - w.x, dy: py - w.y };
                        self.core.set_cursor(3);
                    }
                }
            }
            Request::BeginResize { window, edges } => {
                if let Some(index) = self.core.index_of(window) {
                    let edges = (edges & 0xF) as u8;
                    if self.core.windows[index].client_pid() == Some(sender) && self.core.windows[index].resizable() && edges != 0 && self.core.buttons != 0 {
                        let (px, py) = self.core.pointer;
                        self.grab = Grab::Resize { id: window, edges, start: self.core.windows[index].outer(), px, py };
                        self.core.set_cursor(edge_cursor(edges));
                    }
                }
            }
            Request::Minimize { window } => {
                if self.core.owned_by(window, sender) {
                    self.minimize(window);
                }
            }
            Request::TrackPosition { window } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) {
                        self.core.windows[index].track = true;
                        self.core.windows[index].placed = (i32::MIN, i32::MIN);
                        self.core.send_placements();
                    }
                }
            }
            Request::Present { window, x, y, width, height } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) && !self.core.windows[index].minimized {
                        self.client_presented(window);
                        let Some(index) = self.core.index_of(window) else {
                            return;
                        };
                        let content = self.core.windows[index].content();
                        let area = Area::new(content.x + x as i32, content.y + y as i32, width as i32, height as i32).intersect(&content);
                        self.core.add_damage(area);
                    }
                }
            }
            Request::Attach { window, shm, width, height } => {
                let Some(index) = self.core.index_of(window) else {
                    return;
                };
                if self.core.windows[index].client_pid() != Some(sender) {
                    return;
                }
                let (w, h) = (width as i32, height as i32);
                let Body::Client { width: old_w, height: old_h, .. } = self.core.windows[index].body else {
                    return;
                };
                let resizing = matches!(self.grab, Grab::Resize { id, .. } if id == window);
                let settle = if (old_w, old_h) != (w, h) && !resizing && !self.animating(window) && !self.core.windows[index].minimized { self.capture(index) } else { None };
                if !self.core.swap_buffer(index, shm, w, h) {
                    return;
                }
                if let Some((area, pixels)) = settle {
                    self.start_settle(window, pixels, area);
                }
                if self.core.windows[index].is_popup() {
                    let old = self.core.windows[index].with_shadow();
                    self.core.windows[index].w = w;
                    self.core.windows[index].h = h;
                    self.core.add_damage(old);
                } else if !resizing && self.core.windows[index].tiled.is_none() && self.core.windows[index].requested != (w, h) && sys::uptime_ms().saturating_sub(self.core.windows[index].resized_at) > 400 {
                    let o = self.core.windows[index].outer();
                    let work = self.core.work;
                    let nw = w.clamp(160, self.core.screen.width);
                    let nh = (h + self.core.windows[index].bar_h()).min(work.h);
                    self.core.windows[index].requested = (w, h);
                    let x = o.x.clamp(0, (self.core.screen.width - nw).max(0));
                    let y = o.y.clamp(work.y, (self.core.screen.height - nh).max(work.y));
                    self.core.set_geometry(index, Area::new(x, y, nw, nh));
                }
                let area = self.core.windows[index].outer();
                self.core.add_damage(area);
            }
            Request::SizeHints { window, min_width, min_height, max_width, max_height } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) {
                        self.core.set_size_hints(index, min_width, min_height, max_width, max_height);
                    }
                }
            }
            Request::Destroy { window } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) {
                        self.remove_window(window, false);
                    }
                }
            }
            Request::SetTitle { window, title } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) {
                        self.core.set_title(index, title.as_str());
                    }
                }
            }
            Request::SetIcon { window, name } => {
                if self.core.owned_by(window, sender) && apps::safe_icon_name(name.as_str()) && !name.as_str().contains('/') {
                    let icon = String::from(name.as_str());
                    self.set_window_icon(window, icon);
                }
            }
            Request::SetAppId { window, pid, app_id } => {
                if self.core.owned_by(window, sender) {
                    self.identify_window(window, pid as i64, app_id.as_str());
                }
            }
            Request::SetMaximized { window, maximized } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) && self.core.windows[index].resizable() && (maximized != 0) != self.core.windows[index].tiled.is_some() {
                        self.toggle_maximize(window);
                    }
                }
            }
            Request::SetCursor { window, cursor } => {
                if let Some(index) = self.core.index_of(window) {
                    if self.core.windows[index].client_pid() == Some(sender) {
                        self.core.windows[index].cursor = cursor.min(7);
                        if self.hover_client == Some(window) {
                            self.core.set_cursor(cursor.min(7) as usize);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
