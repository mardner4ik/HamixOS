#![no_std]
#![no_main]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../hxserver/src/apps.rs"]
mod apps;
#[allow(dead_code)]
#[path = "../../hxserver/src/config.rs"]
mod config;
mod desktop;
mod dock;
mod launcher;
mod panel;
mod render;
mod shell;
mod sound;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxclient::ui::{TextField, Ui};
use hxproto::Event;
use hxstd::net;
use hxstd::sys::{self, MOUSE_LEFT};
use hxstd::{entry, fs, println, users};
use vellum::{Area, Image};

use crate::render::Platform;
use crate::shell::Core;
use crate::sound::SoundState;

pub const TOP_H: i32 = 32;
pub const DOCK_MARGIN: i32 = 10;
pub const ICON_CELL_W: i32 = 96;
pub const ICON_CELL_H: i32 = 104;

pub const ROLE_FILES: &str = "files";
pub const ROLE_TERMINAL: &str = "terminal";
pub const ROLE_SETTINGS: &str = "settings";
pub const ROLE_INSTALLER: &str = "installer";
const APP_WATCH_MS: u64 = 3000;
const KEY_SUPER: i32 = -21;
const PERM_EXEC: &str = "/usr/libexec/hamix/hxperm-exec";

pub fn app_by_icon(icon: &str) -> Option<usize> {
    apps::by_key(icon)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopItem {
    App(usize),
    Home,
}

impl DesktopItem {
    pub fn key(&self) -> &'static str {
        match self {
            DesktopItem::App(i) => apps::get(*i).key.as_str(),
            DesktopItem::Home => "home",
        }
    }

    pub fn from_key(key: &str) -> Option<DesktopItem> {
        if key == "home" {
            return Some(DesktopItem::Home);
        }
        app_by_icon(key).map(DesktopItem::App)
    }
}

pub struct DesktopIcon {
    pub item: DesktopItem,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Logout,
    Reboot,
    Shutdown,
}

#[derive(Clone, PartialEq)]
pub enum MenuAction {
    Launch(usize),
    LaunchWith(usize, String),
    PinApp(usize),
    UnpinApp(usize),
    CloseApp(usize),
    CloseWindow(u32),
    ToggleMaximize(u32),
    Minimize(u32),
    AddToDesktop(usize),
    RemoveFromDesktop(usize),
    OpenDesktopItem(usize),
    ArrangeIcons,
    ToggleAutohide,
    About,
    LaunchRole(&'static str, Option<String>),
}

#[derive(Clone)]
pub struct MenuItem {
    pub label: String,
    pub icon: &'static str,
    pub action: MenuAction,
    pub danger: bool,
}

#[derive(Clone)]
pub enum Popup {
    None,
    Launcher,
    Power,
    Network,
    Calendar,
    Sound,
    DockOverflow,
    Menu { x: i32, y: i32, items: Vec<MenuItem> },
}

impl Popup {
    pub fn is_none(&self) -> bool {
        matches!(self, Popup::None)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    None,
    DockDrag { index: usize, start_x: i32, moved: bool },
    LauncherScroll { offset: i32 },
    IconDrag { index: usize, dx: i32, dy: i32, start_x: i32, start_y: i32, moved: bool },
    VolumeDrag,
}

pub struct NetState {
    pub status: net::Status,
    pub networks: Vec<net::WifiNetwork>,
    pub last_poll: u64,
    pub last_scan: u64,
    pub selected: Option<String>,
    pub password: TextField,
    pub message: String,
    pub last_connected: Option<bool>,
}

pub struct Nook {
    pub platform: Platform,
    pub core: Core,
    pub ui: Ui,
    pub sized_icons: BTreeMap<(String, i32), Image>,
    pub grab: Grab,
    pub popup: Popup,
    pub popup_hover: Option<usize>,
    pub search: TextField,
    pub desktop: Vec<DesktopIcon>,
    pub desktop_removed: Vec<String>,
    pub desktop_added: Vec<String>,
    pub desktop_hover: Option<usize>,
    pub desktop_selected: Option<usize>,
    pub pinned: Vec<usize>,
    pub dock_hover: Option<usize>,
    pub dock_offset: i32,
    pub dock_size: i32,
    pub dock_limit: usize,
    pub popup_dock: bool,
    pub autohide: bool,
    pub running: bool,
    pub logout: bool,
    pub last_minute: i64,
    pub user: String,
    pub live: bool,
    pub last_click: (u64, i32, i32),
    pub launcher_scroll: usize,
    pub launcher_focus: Option<usize>,
    pub apps_fingerprint: u64,
    pub last_app_check: u64,
    pub hot_buttons: Vec<Area>,
    pub known_commands: Vec<String>,
    pub net: NetState,
    pub calendar_month: (i64, u32),
    pub sound: SoundState,
    pub targets: Vec<(u32, Area)>,
    pub cursor: (u32, u32),
    pub dock_input: Option<Area>,
}

pub fn weekday(days: i64) -> &'static str {
    ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days.rem_euclid(7)) as usize]
}

pub fn civil(epoch: i64) -> (i64, u32, u32, u32, u32) {
    let days = epoch.div_euclid(86400);
    let secs = epoch.rem_euclid(86400) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d, secs / 3600, secs / 60 % 60)
}

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

impl Nook {
    pub fn ui(&self) -> &'static Ui {
        unsafe { &*(&self.ui as *const Ui) }
    }

    pub fn sized_icon(&mut self, name: &str, size: i32) -> Option<&Image> {
        let key = (String::from(name), size);
        if !self.sized_icons.contains_key(&key) {
            self.ui.load_icon(name);
            let image = self.ui.icon(name)?;
            let scaled = if image.w == size && image.h == size { Image { w: image.w, h: image.h, px: image.px.clone() } } else { image.scaled(size, size) };
            self.sized_icons.insert(key.clone(), scaled);
        }
        self.sized_icons.get(&key)
    }

    pub fn toast(&mut self, title: &str, body: &str, icon: &str) {
        self.platform.toast(title, body, icon);
    }

    pub fn launch(&mut self, app: usize, args: &[&str]) {
        if !apps::alive(app) {
            return;
        }
        let entry = apps::get(app);
        let mut argv: Vec<String> = entry.args.clone();
        argv.extend(args.iter().map(|a| String::from(*a)));
        let (program, mut argv) = if entry.terminal {
            let mut command = apps::shell_quote(&entry.program);
            for arg in &argv {
                command.push(' ');
                command.push_str(&apps::shell_quote(arg));
            }
            (String::from("/usr/bin/hxterm"), alloc::vec![command])
        } else {
            (entry.program.clone(), argv)
        };
        let program = if fs::exists(PERM_EXEC) {
            argv.insert(0, program);
            argv.insert(0, entry.id.clone());
            String::from(PERM_EXEC)
        } else {
            program
        };
        let pid = if entry.linux && !entry.terminal {
            let env: Vec<String> = hxstd::env::vars().iter().filter(|(k, _)| k != "GTK_THEME").map(|(k, v)| format!("{}={}", k, v)).collect();
            sys::spawn_io_env(&program, &argv, Some(&env), sys::SPAWN_DETACH, None, None, None)
        } else {
            sys::spawn(&program, &argv, sys::SPAWN_DETACH)
        };
        let key = entry.key.clone();
        let icon = format!("apps/{}", key);
        let (name, linux, terminal) = (entry.name.clone(), entry.linux, entry.terminal);
        if pid < 0 {
            self.toast(&name, "could not be started", &icon);
            return;
        }
        if !terminal {
            self.platform.launched(pid, &key);
        }
        if linux && !terminal && sys::service_lookup("hxwayland") <= 0 {
            sys::spawn("/usr/libexec/hamix/hxwayland", &[] as &[&str], sys::SPAWN_DETACH);
        }
    }

    pub fn launch_role(&mut self, role: &str, args: &[&str]) {
        match apps::by_role(role) {
            Some(app) => self.launch(app, args),
            None => self.toast("No application for this task", role, "apps/hello"),
        }
    }

    pub fn refresh_apps(&mut self) {
        self.apps_fingerprint = apps::fingerprint();
        self.last_app_check = sys::uptime_ms();
        let before: Vec<usize> = (0..apps::count()).filter(|i| apps::alive(*i)).collect();
        if !apps::scan(&mut self.ui, &mut self.sized_icons) {
            return;
        }
        self.pinned.retain(|a| apps::alive(*a));
        let removed: Vec<usize> = self
            .desktop
            .iter()
            .filter_map(|d| match d.item {
                DesktopItem::App(a) if !apps::alive(a) => Some(a),
                _ => None,
            })
            .collect();
        self.desktop.retain(|d| !matches!(d.item, DesktopItem::App(a) if removed.contains(&a)));
        if !before.is_empty() {
            let fresh: Vec<usize> = (0..apps::count()).filter(|i| apps::alive(*i) && !before.contains(i)).collect();
            match fresh.len() {
                0 => {}
                1 => {
                    let app = apps::get(fresh[0]);
                    let icon = format!("apps/{}", app.key);
                    let body = format!("{} is ready in Applications", app.name);
                    self.toast("New application", &body, &icon);
                }
                n => self.toast("New applications", &format!("{} programs were added to Applications", n), "ui/apps"),
            }
        }
        self.launcher_focus = None;
        self.core.damage_all();
    }

    pub fn watch_apps(&mut self, now: u64) {
        if now.saturating_sub(self.last_app_check) < APP_WATCH_MS {
            return;
        }
        self.last_app_check = now;
        self.known_commands = sys::cmd_list().into_iter().map(|c| c.name).collect();
        if apps::fingerprint() != self.apps_fingerprint {
            self.refresh_apps();
        }
    }

    pub fn start_power(&mut self, action: PowerAction) {
        match action {
            PowerAction::Logout => self.running = false,
            PowerAction::Reboot => self.platform.shell_action(hxproto::SHELL_REBOOT),
            PowerAction::Shutdown => self.platform.shell_action(hxproto::SHELL_SHUTDOWN),
        }
    }

    pub fn open_about(&mut self) {
        self.platform.shell_action(hxproto::SHELL_ABOUT);
    }

    pub fn focus(&mut self, id: u32) {
        self.platform.toplevel_action(id, hxproto::ACTION_ACTIVATE);
    }

    pub fn minimize(&mut self, id: u32) {
        self.platform.toplevel_action(id, hxproto::ACTION_MINIMIZE);
    }

    pub fn close_window(&mut self, id: u32) {
        self.platform.toplevel_action(id, hxproto::ACTION_CLOSE);
    }

    pub fn toggle_maximize(&mut self, id: u32) {
        self.platform.toplevel_action(id, hxproto::ACTION_TOGGLE_MAXIMIZE);
    }

    pub fn app_windows(&self, app: usize) -> Vec<u32> {
        self.core.windows.iter().filter(|w| w.icon == apps::get(app).key).map(|w| w.id).collect()
    }

    pub fn dock_anchored(&self) -> bool {
        false
    }

    pub fn update_work_area(&mut self) {
        self.platform.configure_dock(self.dock_exclusive());
        self.core.add_damage(Area::new(0, 0, self.core.screen.width, self.core.screen.height));
    }

    pub fn dock_exclusive(&self) -> i32 {
        if self.autohide { 0 } else { self.dock_h() + 2 * DOCK_MARGIN }
    }

    pub fn reload_config(&mut self) {
        let themed = hxclient::ui::theme::load();
        if themed {
            self.sized_icons.clear();
        }
        let cfg = config::load();
        let dock = (cfg.autohide, cfg.dock_size, cfg.dock_limit);
        if dock == (self.autohide, self.dock_size, self.dock_limit) && !themed {
            return;
        }
        let region = self.dock_region();
        self.core.add_damage(region);
        self.autohide = cfg.autohide;
        self.dock_size = cfg.dock_size;
        self.dock_limit = cfg.dock_limit;
        self.platform.configure_dock(self.dock_exclusive());
        if themed {
            self.core.damage_all();
        }
        let region = self.dock_region();
        self.core.add_damage(region);
    }

    pub fn rebuild_surfaces(&mut self) {
        let region = self.core.screen.height - self.dock_region().y;
        let exclusive = self.dock_exclusive();
        self.platform.rebuild(&mut self.core, region, exclusive);
        self.sync_popup_surface();
        self.core.damage_all();
    }

    pub fn sync_popup_surface(&mut self) {
        let screen = Area::new(0, 0, self.core.screen.width, self.core.screen.height);
        let wanted = self.popup_area().map(|a| a.expand(40).intersect(&screen));
        self.platform.set_popup(&mut self.core, wanted);
        let osd = if self.sound.osd_shown { Some(self.volume_osd_area().expand(32).intersect(&screen)) } else { None };
        self.platform.set_osd(&mut self.core, osd);
    }

    pub fn shell_press(&mut self, x: i32, y: i32, button: u32, desktop: bool) {
        if !self.popup.is_none() {
            let was = self.popup_area();
            if self.popup_press(x, y, button) {
                return;
            }
            if was.map(|a| a.contains(x, y)).unwrap_or(false) {
                return;
            }
        }
        if y < TOP_H {
            self.top_bar_press(x, button);
            return;
        }
        if self.dock_visible() && self.dock_area().contains(x, y) {
            self.dock_press(x, y, button);
            return;
        }
        if desktop {
            self.desktop_press(x, y, button);
        }
    }

    pub fn shell_release(&mut self) {
        match self.grab {
            Grab::DockDrag { index, moved, .. } => {
                self.grab = Grab::None;
                if moved {
                    self.save_pinned();
                } else {
                    self.activate_dock(index);
                }
                let region = self.dock_region();
                self.core.add_damage(region);
            }
            Grab::LauncherScroll { .. } => {
                self.grab = Grab::None;
            }
            Grab::IconDrag { index, moved, .. } => {
                self.grab = Grab::None;
                if moved {
                    self.snap_icon(index);
                    self.save_desktop();
                }
            }
            Grab::VolumeDrag => {
                self.grab = Grab::None;
                if let Some(area) = self.popup_area() {
                    self.core.add_damage(area.expand(32));
                }
            }
            Grab::None => {}
        }
    }

    pub fn shell_motion(&mut self, x: i32, y: i32, desktop: bool) {
        match self.grab {
            Grab::DockDrag { index, start_x, moved } => {
                self.drag_dock(index, start_x, moved, x);
                self.core.pointer = (x, y);
                return;
            }
            Grab::LauncherScroll { offset } => {
                self.drag_launcher_scroll(y, offset);
                self.core.pointer = (x, y);
                return;
            }
            Grab::IconDrag { index, dx, dy, start_x, start_y, moved } => {
                self.drag_icon(index, dx, dy, start_x, start_y, moved, x, y);
                self.core.pointer = (x, y);
                return;
            }
            Grab::VolumeDrag => {
                self.set_volume_from_x(x);
                self.core.pointer = (x, y);
                return;
            }
            Grab::None => {}
        }
        if !self.popup.is_none() {
            let hover = self.popup_hover_index(x, y);
            if hover != self.popup_hover {
                self.popup_hover = hover;
                if let Some(area) = self.popup_area() {
                    self.core.add_damage(area);
                }
            }
        }
        let (px, py) = self.core.pointer;
        let hot = self.hot_buttons.clone();
        for area in hot {
            if area.contains(px, py) != area.contains(x, y) {
                self.core.add_damage(area.expand(2));
            }
        }
        self.core.pointer = (x, y);
        let dock = self.dock_hit(x, y);
        if dock != self.dock_hover {
            self.dock_hover = dock;
            let region = self.dock_region();
            self.core.add_damage(region);
        }
        let icon = if desktop && self.popup.is_none() && y >= TOP_H { self.desktop_icon_at(x, y) } else { None };
        if icon != self.desktop_hover {
            for i in [self.desktop_hover, icon].into_iter().flatten() {
                if i < self.desktop.len() {
                    let a = self.desktop_area(i);
                    self.core.add_damage(a);
                }
            }
            self.desktop_hover = icon;
        }
    }

    fn cursor_at(&self, x: i32, y: i32) -> u32 {
        let field = match self.popup {
            Popup::Launcher => {
                let a = self.launcher_area();
                Some(Area::new(a.x + 10, a.y + 10, a.w - 20, 34))
            }
            Popup::Network => self.network_password_field().map(|(field, _)| field),
            _ => None,
        };
        if field.map(|f| f.contains(x, y)).unwrap_or(false) { hxproto::CURSOR_TEXT } else { hxproto::CURSOR_ARROW }
    }

    fn update_cursor(&mut self, window: u32, x: i32, y: i32) {
        let cursor = self.cursor_at(x, y);
        if self.cursor != (window, cursor) {
            self.cursor = (window, cursor);
            self.platform.set_cursor(window, cursor);
        }
    }

    pub fn shell_leave(&mut self) {
        let (px, py) = self.core.pointer;
        let hot = self.hot_buttons.clone();
        for area in hot {
            if area.contains(px, py) {
                self.core.add_damage(area.expand(2));
            }
        }
        if self.dock_hover.take().is_some() {
            let region = self.dock_region();
            self.core.add_damage(region);
        }
        if let Some(i) = self.desktop_hover.take() {
            if i < self.desktop.len() {
                let a = self.desktop_area(i);
                self.core.add_damage(a);
            }
        }
        if !matches!(self.grab, Grab::None) {
            return;
        }
        self.core.pointer = (-1, -1);
    }

    pub fn shell_wheel(&mut self, x: i32, y: i32, wheel: i32) {
        if matches!(self.popup, Popup::Launcher) && self.launcher_area().contains(x, y) {
            self.launcher_wheel(wheel);
            return;
        }
        if (y < TOP_H && self.volume_button().contains(x, y)) || (matches!(self.popup, Popup::Sound) && self.sound_area().contains(x, y)) {
            self.volume_wheel(wheel);
        }
    }

    pub fn shell_key(&mut self, code: i32) {
        if code == KEY_SUPER {
            if matches!(self.popup, Popup::Launcher) {
                self.set_popup(Popup::None);
            } else {
                self.set_popup(Popup::Launcher);
            }
            return;
        }
        if self.popup_key(code) {
            return;
        }
        if code == 32 || code == 10 {
            self.set_popup(Popup::Launcher);
        }
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Mouse { window, x, y, kind, wheel, .. } => {
                let Some(origin) = self.platform.origin_of(window) else {
                    return;
                };
                let desktop = self.platform.is_desktop(window);
                if kind == hxproto::MOUSE_LEAVE {
                    self.shell_leave();
                    return;
                }
                let (ax, ay) = (x + origin.0, y + origin.1);
                match kind {
                    hxproto::MOUSE_MOVE => {
                        self.shell_motion(ax, ay, desktop);
                        self.update_cursor(window, ax, ay);
                    }
                    hxproto::MOUSE_PRESS => {
                        self.core.pointer = (ax, ay);
                        let button = if wheel > 0 { wheel as u32 } else { MOUSE_LEFT };
                        self.shell_press(ax, ay, button, desktop);
                    }
                    hxproto::MOUSE_RELEASE => {
                        self.core.pointer = (ax, ay);
                        self.shell_release();
                    }
                    hxproto::MOUSE_WHEEL => self.shell_wheel(ax, ay, wheel),
                    _ => {}
                }
            }
            Event::Key { code, mods, .. } => {
                let _ = mods;
                self.shell_key(code);
            }
            Event::ToplevelState { toplevel, state, workspace, title, app_id } => {
                self.core.update_window(toplevel, state, workspace, title.as_str(), app_id.as_str());
                for (_, icon) in [(0, format!("apps/{}", app_id.as_str()))] {
                    self.ui.load_icon(&icon);
                }
                let region = self.dock_region();
                self.core.add_damage(region);
            }
            Event::ToplevelGeometry { toplevel, x, y, width, height } => {
                self.core.move_window(toplevel, Area::new(x, y, width, height));
            }
            Event::ToplevelClosed { toplevel } => {
                self.core.windows.retain(|w| w.id != toplevel);
                self.targets.retain(|(id, _)| *id != toplevel);
                let region = self.dock_region();
                self.core.add_damage(region);
            }
            Event::Workspaces { current, .. } => {
                self.core.workspace = current;
                let region = self.dock_region();
                self.core.add_damage(region);
            }
            Event::LayerConfigured { window, x, y, width, height } => {
                self.platform.configured(window, Area::new(x, y, width as i32, height as i32));
                self.core.damage_all();
            }
            Event::Screen { width, height } | Event::Output { width, height, .. } => {
                if (width as i32, height as i32) != (self.core.screen.width, self.core.screen.height) {
                    self.core.screen.width = width as i32;
                    self.core.screen.height = height as i32;
                    self.platform.screen_changed(width, height);
                    self.clamp_desktop_icons();
                    self.rebuild_surfaces();
                }
            }
            Event::Theme { .. } => {
                hxclient::ui::theme::load();
                self.sized_icons.clear();
                self.core.damage_all();
            }
            _ => {}
        }
    }

    fn pump(&mut self, timeout: u64) {
        let mut events = Vec::new();
        self.platform.pump(timeout, &mut events);
        for event in events {
            self.on_event(event);
        }
    }

    fn sync_targets(&mut self) {
        let mut targets = Vec::new();
        let items = self.dock_visible_items();
        let more = items.iter().position(|i| *i == dock::DockItem::More);
        for w in self.core.windows.iter() {
            let app = app_by_icon(&w.icon);
            let index = items.iter().position(|item| match item {
                dock::DockItem::Window(id) => *id == w.id,
                dock::DockItem::App(a) => Some(*a) == app,
                dock::DockItem::More => false,
            });
            let slot = index.or(more).unwrap_or(0);
            targets.push((w.id, self.dock_slot_area(slot, true)));
        }
        for (id, area) in targets.iter() {
            if !self.targets.iter().any(|(i, a)| i == id && a == area) {
                self.platform.minimize_target(*id, *area);
            }
        }
        self.targets = targets;
    }

    fn render(&mut self) {
        if self.core.damage.is_empty() {
            return;
        }
        let region = self.core.screen.height - self.dock_region().y;
        let exclusive = self.dock_exclusive();
        self.platform.ensure_dock(&mut self.core, region, exclusive);
        self.sync_popup_surface();
        let damage = core::mem::take(&mut self.core.damage);
        for kind in shell::KINDS {
            let Some((pixels, area)) = self.platform.target(kind) else {
                continue;
            };
            let mut dirty: Option<Area> = None;
            for d in damage.iter() {
                let rect = d.intersect(&area);
                if rect.is_empty() {
                    continue;
                }
                self.core.begin(pixels, area, rect, kind == shell::Kind::Dock && self.autohide);
                match kind {
                    shell::Kind::Desktop => self.draw_desktop_icons(),
                    shell::Kind::Bar => self.draw_top_bar(),
                    shell::Kind::Dock => self.draw_dock(),
                    shell::Kind::Popup => self.draw_popup(),
                    shell::Kind::Osd => self.draw_volume_osd(),
                }
                dirty = Some(match dirty {
                    Some(d) => d.union(&rect),
                    None => rect,
                });
            }
            self.core.end();
            if let Some(rect) = dirty {
                self.platform.present(kind, rect);
            }
        }
        self.sync_targets();
        let input = if self.dock_visible() { self.dock_area() } else { Area::new(0, self.core.screen.height - 2, self.core.screen.width, 2) };
        if self.dock_input != Some(input) {
            self.dock_input = Some(input);
            self.platform.input_region(shell::Kind::Dock, &[input]);
        }
    }
}

fn main() -> i32 {
    let platform = match Platform::open() {
        Ok(p) => p,
        Err(e) => {
            println!("nook-shell: {}", e);
            return 1;
        }
    };
    let (width, height) = platform.screen();
    let live = fs::read_to_string("/proc/mounts").and_then(|t| t.lines().next().map(|l| l.starts_with("live"))).unwrap_or(false);
    let cfg = config::load();
    let mut ui = Ui::load();
    let mut scan_cache = BTreeMap::new();
    apps::scan(&mut ui, &mut scan_cache);
    let mut pinned: Vec<usize> = match &cfg.dock {
        Some(names) => names.iter().filter_map(|n| app_by_icon(n)).collect(),
        None => apps::default_dock(),
    };
    pinned.dedup();
    let (year, month, _, _, _) = civil(sys::realtime().sec);
    let workspace = platform.workspace();

    let mut nook = Nook {
        platform,
        core: Core::new(width, height, workspace),
        ui,
        sized_icons: scan_cache,
        grab: Grab::None,
        popup: Popup::None,
        popup_hover: None,
        search: TextField::default(),
        desktop: Vec::new(),
        desktop_removed: cfg.desktop_removed.clone(),
        desktop_added: cfg.desktop_added.clone(),
        desktop_hover: None,
        desktop_selected: None,
        pinned,
        dock_hover: None,
        dock_offset: 0,
        dock_size: cfg.dock_size,
        dock_limit: cfg.dock_limit,
        popup_dock: false,
        autohide: cfg.autohide,
        running: true,
        logout: false,
        last_minute: -1,
        user: users::name_of(sys::getuid() as u32),
        live,
        last_click: (0, -1, -1),
        launcher_scroll: 0,
        launcher_focus: None,
        apps_fingerprint: apps::fingerprint(),
        last_app_check: sys::uptime_ms(),
        hot_buttons: Vec::new(),
        known_commands: sys::cmd_list().into_iter().map(|c| c.name).collect(),
        net: NetState {
            status: net::Status::default(),
            networks: Vec::new(),
            last_poll: 0,
            last_scan: 0,
            selected: None,
            password: TextField::secret(),
            message: String::new(),
            last_connected: None,
        },
        calendar_month: (year, month),
        sound: SoundState::new(),
        targets: Vec::new(),
        cursor: (0, 0),
        dock_input: None,
    };
    nook.build_desktop(&cfg);
    let mut icons: Vec<String> = alloc::vec![String::from("logo"), String::from("logo-20"), String::from("apps/folder-home"), String::from("apps/hello")];
    for i in 0..apps::count() {
        icons.push(format!("apps/{}", apps::get(i).key));
        icons.push(format!("apps/{}-24", apps::get(i).key));
    }
    for name in ["power", "user", "minimize", "maximize", "restore", "close", "search", "logout", "reboot", "run", "bell", "pin", "unpin", "wifi-0", "wifi-1", "wifi-2", "wifi-3", "wifi-off", "ethernet", "network-off", "lock", "check", "chevron-left", "chevron-right", "display", "trash", "open", "info", "plus", "folder", "wallpaper", "apps", "network", "volume-off", "volume-mute", "volume-1", "volume-2", "volume-3"] {
        icons.push(format!("ui/{}", name));
    }
    for name in &icons {
        nook.ui.load_icon(name);
    }
    for i in 0..apps::count() {
        let key = apps::get(i).key.clone();
        nook.sized_icon(&format!("apps/{}-24", key), 18);
    }
    nook.poll_network(true);
    nook.init_sound();
    nook.rebuild_surfaces();
    nook.render();
    println!("nook-shell: running on {} ({}x{})", nook.platform.name(), nook.core.screen.width, nook.core.screen.height);
    sys::catch_hangup();

    while nook.running {
        let animating = nook.dock_animating() || !matches!(nook.grab, Grab::None);
        let timeout = if animating { 16 } else if nook.sound.osd_shown || nook.sound.save_at != 0 { 100 } else { 500 };
        nook.pump(timeout);
        if !nook.platform.alive() {
            return 0;
        }
        let now = sys::uptime_ms();
        if sys::take_hangup() {
            nook.reload_config();
        }
        let minute = sys::realtime().sec / 60;
        if minute != nook.last_minute {
            nook.last_minute = minute;
            let bar = nook.top_bar();
            nook.core.add_damage(bar);
        }
        nook.watch_apps(now);
        nook.poll_network(false);
        nook.poll_sound(now);
        nook.update_dock_visibility();
        nook.render();
    }
    nook.platform.shell_action(hxproto::SHELL_LOGOUT);
    0
}

entry!(main);
