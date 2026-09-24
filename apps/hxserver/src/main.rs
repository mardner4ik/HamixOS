#![no_std]
#![no_main]

extern crate alloc;

mod anim;
mod apps;
mod config;
mod compositor;
mod desktop;
mod dock;
mod launcher;
mod panel;
mod render;
mod sound;
mod wm;

use sound::SoundState;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::net;
use hamix_std::sys::{self, MouseState, MOUSE_LEFT, MOUSE_RIGHT};
use hamix_std::{entry, fs, println, users};
use hxclient::ui::{TextField, Ui};
use hxproto::Event;
use vellum::{Area, Image, Painter};

use crate::compositor::window::{edge_cursor, Body, TITLE_H};
use crate::compositor::{send_event, Compositor, Screen};

pub const TOP_H: i32 = 32;
pub const DOCK_MARGIN: i32 = 10;
pub const ICON_CELL_W: i32 = 96;
pub const ICON_CELL_H: i32 = 92;
pub const FRAME_MS: u64 = 12;
pub const DEFAULT_WALLPAPER: &str = "/usr/share/wallpapers/dusk.png";

pub const ROLE_FILES: &str = "files";
pub const ROLE_TERMINAL: &str = "terminal";
pub const ROLE_SETTINGS: &str = "settings";
pub const ROLE_INSTALLER: &str = "installer";
const APP_WATCH_MS: u64 = 3000;
pub const KEY_ALT_TAB: i32 = -20;
pub const KEY_SUPER: i32 = -21;
pub const KEY_ALT_F4: i32 = -22;
pub const KEY_SNAP_LEFT: i32 = -23;
pub const KEY_SNAP_RIGHT: i32 = -24;
pub const KEY_SNAP_UP: i32 = -25;
pub const KEY_SNAP_DOWN: i32 = -26;

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

fn write_toolkit_settings() {
    let Some(home) = hamix_std::env::var("HOME") else {
        return;
    };
    let dark = !hxclient::ui::theme::is_light();
    let config = format!("{}/.config", home);
    sys::mkdir(&config);
    let gtk3 = format!(
        "[Settings]\ngtk-theme-name=Nook\ngtk-application-prefer-dark-theme={}\ngtk-font-name=Noto Sans 10\ngtk-icon-theme-name=Nook Icons\ngtk-cursor-theme-name=Adwaita\ngtk-enable-animations=false\ngtk-decoration-layout=:\ngtk-menu-images=true\ngtk-button-images=false\n",
        dark as u32
    );
    for dir in ["gtk-3.0", "gtk-4.0"] {
        let path = format!("{}/{}", config, dir);
        sys::mkdir(&path);
        let file = format!("{}/settings.ini", path);
        if fs::read(&file).as_deref() != Some(gtk3.as_bytes()) {
            fs::write(&file, gtk3.as_bytes());
        }
    }
    let dillo_dir = format!("{}/.dillo", home);
    let dillo = format!("{}/dillorc", dillo_dir);
    if sys::stat(&dillo).is_err() {
        sys::mkdir(&dillo_dir);
        fs::write(&dillo, b"theme=gtk+\nui_main_bg_color=#f6f7f9\nui_text_bg_color=#ffffff\nui_fg_color=#1d2129\nui_selection_color=#2f6fed\nui_button_highlight_color=#e3e7ed\nui_tab_active_bg_color=#ffffff\nui_tab_bg_color=#eceef2\nui_tab_active_fg_color=#1d2129\nui_tab_fg_color=#565e6d\nfont_sans_serif=Noto Sans\n");
    }
    let gtk2 = format!("include \"/usr/share/themes/Nook/gtk-2.0/{}\"\ngtk-theme-name = \"Nook\"\ngtk-icon-theme-name = \"Nook Icons\"\n", if dark { "gtkrc-dark" } else { "gtkrc" });
    let file = format!("{}/.gtkrc-2.0", home);
    if fs::read(&file).as_deref() != Some(gtk2.as_bytes()) {
        fs::write(&file, gtk2.as_bytes());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    None,
    Move { id: u32, dx: i32, dy: i32 },
    Resize { id: u32, edges: u8, start: Area, px: i32, py: i32 },
    Client(u32),
    DockDrag { index: usize, start_x: i32, moved: bool },
    LauncherScroll { offset: i32 },
    IconDrag { index: usize, dx: i32, dy: i32, start_x: i32, start_y: i32, moved: bool },
    VolumeDrag,
}

pub struct Toast {
    pub title: String,
    pub body: String,
    pub icon: String,
    pub until: u64,
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

pub struct Picker {
    pub token: u32,
    pub client: i64,
    pub request: u32,
    pub pid: i64,
}

pub struct Nook {
    pub core: Compositor,
    pub key_mods: u32,
    pub wayland: i64,
    pub background: Vec<u32>,
    pub ui: Ui,
    pub sized_icons: BTreeMap<(String, i32), Image>,
    pub grab: Grab,
    pub snap_preview: Option<Area>,
    pub hover_client: Option<u32>,
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
    pub toasts: Vec<Toast>,
    pub known_commands: Vec<String>,
    pub running: bool,
    pub last_minute: i64,
    pub user: String,
    pub live: bool,
    pub last_click: (u64, i32, i32),
    pub launched: Vec<(i64, usize)>,
    pub launcher_scroll: usize,
    pub launcher_focus: Option<usize>,
    pub apps_fingerprint: u64,
    pub last_app_check: u64,
    pub hot_buttons: Vec<Area>,
    pub net: NetState,
    pub calendar_month: (i64, u32),
    pub switcher: Option<usize>,
    pub switcher_until: u64,
    pub pickers: Vec<Picker>,
    pub next_token: u32,
    pub sound: SoundState,
    pub anim: anim::Prefs,
    pub animations: Vec<anim::Animation>,
    pub pending_open: Vec<(u32, u64)>,
    pub last_compose: u64,
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

    pub fn load_wallpaper(&mut self) {
        let path = config::load().wallpaper.unwrap_or_else(|| String::from(DEFAULT_WALLPAPER));
        let (sw, sh) = (self.core.screen.width, self.core.screen.height);
        let mut hash: u32 = 2166136261;
        for b in path.bytes() {
            hash = (hash ^ b as u32).wrapping_mul(16777619);
        }
        let size = sys::stat(&path).map(|s| s.size).unwrap_or(0);
        let cache = format!("/var/cache/nook/wall-{:08x}-{}-{}x{}.raw", hash, size, sw, sh);
        if let Some(bytes) = fs::read(&cache) {
            if bytes.len() == self.background.len() * 4 {
                for (dst, chunk) in self.background.iter_mut().zip(bytes.chunks_exact(4)) {
                    *dst = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                }
                return;
            }
        }
        {
            let mut p = Painter::new(&mut self.background, sw, sh);
            p.gradient_v(Area::new(0, 0, sw, sh), 0x1a2238, 0x0b0d14, 255);
        }
        let Some(image) = fs::read(&path).and_then(|b| Image::from_png(&b)) else {
            return;
        };
        let (iw, ih) = (image.w as i64, image.h as i64);
        let (crop_w, crop_h) = if iw * sh as i64 > ih * sw as i64 { (ih * sw as i64 / sh as i64, ih) } else { (iw, iw * sh as i64 / sw as i64) };
        let ox = ((iw - crop_w) / 2) as i32;
        let oy = ((ih - crop_h) / 2) as i32;
        let mut cropped = Image { w: crop_w as i32, h: crop_h as i32, px: Vec::with_capacity((crop_w * crop_h) as usize) };
        for y in 0..crop_h as i32 {
            let start = ((oy + y) * image.w + ox) as usize;
            cropped.px.extend_from_slice(&image.px[start..start + crop_w as usize]);
        }
        drop(image);
        let scaled = cropped.scaled(sw, sh);
        for (dst, src) in self.background.iter_mut().zip(scaled.px.iter()) {
            *dst = *src & 0xFFFFFF;
        }
        let _ = sys::mkdir("/var/cache");
        let _ = sys::mkdir("/var/cache/nook");
        sys::chmod("/var/cache/nook", 0o777);
        let mut raw = Vec::with_capacity(self.background.len() * 4);
        for p in &self.background {
            raw.extend_from_slice(&p.to_le_bytes());
        }
        fs::write(&cache, &raw);
    }

    pub fn toast(&mut self, title: &str, body: &str, icon: &str) {
        let now = sys::uptime_ms();
        let icon = if icon.is_empty() { String::from("ui/bell") } else { String::from(icon) };
        self.ui.load_icon(&icon);
        self.toasts.push(Toast { title: String::from(title), body: String::from(body), icon, until: now + 5000 });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        let area = self.toast_region();
        self.core.add_damage(area);
    }

    pub fn toast_region(&self) -> Area {
        Area::new(self.core.screen.width - 380, TOP_H, 380, 4 * 84 + 24)
    }

    pub fn toast_area(&self, index: usize) -> Area {
        Area::new(self.core.screen.width - 356, TOP_H + 12 + index as i32 * 84, 344, 72)
    }

    pub fn launch(&mut self, app: usize, args: &[&str]) {
        if !apps::alive(app) {
            return;
        }
        let entry = apps::get(app);
        let mut argv: Vec<String> = entry.args.clone();
        argv.extend(args.iter().map(|a| String::from(*a)));
        let pid = if entry.terminal {
            let mut command = apps::shell_quote(&entry.program);
            for arg in &argv {
                command.push(' ');
                command.push_str(&apps::shell_quote(arg));
            }
            sys::spawn("/usr/bin/hxterm", &[command], sys::SPAWN_DETACH)
        } else if entry.linux {
            let env: Vec<String> = hamix_std::env::vars().iter().filter(|(k, _)| k != "GTK_THEME").map(|(k, v)| format!("{}={}", k, v)).collect();
            sys::spawn_io_env(&entry.program, &argv, Some(&env), sys::SPAWN_DETACH, None, None, None)
        } else {
            sys::spawn(&entry.program, &argv, sys::SPAWN_DETACH)
        };
        let icon = format!("apps/{}", entry.key);
        if pid < 0 {
            self.toast(&entry.name, "could not be started", &icon);
            return;
        }
        if !entry.terminal {
            self.launched.push((pid, app));
        }
        if entry.linux && !entry.terminal && !sys::proc_alive(self.wayland) {
            self.wayland = sys::spawn("/usr/bin/hxwayland", &[] as &[&str], sys::SPAWN_DETACH);
        }
    }

    pub fn broadcast_theme(&mut self) {
        write_toolkit_settings();
        let light = hxclient::ui::theme::is_light();
        let mut pids: Vec<i64> = self.core.windows.iter().filter_map(|w| w.client_pid()).collect();
        pids.extend(self.pickers.iter().map(|p| p.pid));
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            send_event(pid, Event::Theme { light });
        }
        self.core.damage_all();
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
        let removed: Vec<usize> = self.desktop.iter().filter_map(|d| match d.item {
            DesktopItem::App(a) if !apps::alive(a) => Some(a),
            _ => None,
        }).collect();
        self.desktop.retain(|d| !matches!(d.item, DesktopItem::App(a) if removed.contains(&a)));
        if !before.is_empty() {
            let fresh: Vec<usize> = (0..apps::count()).filter(|i| apps::alive(*i) && !before.contains(i)).collect();
            match fresh.len() {
                0 => {}
                1 => {
                    let app = apps::get(fresh[0]);
                    let icon = format!("apps/{}", app.key);
                    self.toast("New application", &format!("{} is ready in Applications", app.name), &icon);
                }
                n => self.toast("New applications", &format!("{} programs were added to Applications", n), "ui/apps"),
            }
        }
        for window in 0..self.core.windows.len() {
            let icon = self.core.windows[window].icon.clone();
            if !icon.is_empty() {
                self.preload_window_icon(&icon);
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
        if apps::fingerprint() != self.apps_fingerprint {
            self.refresh_apps();
        }
    }

    pub fn start_power(&mut self, action: PowerAction) {
        if action == PowerAction::Logout {
            self.running = false;
            return;
        }
        if sys::geteuid() == 0 {
            sys::power(if action == PowerAction::Reboot { sys::POWER_REBOOT } else { sys::POWER_OFF });
            return;
        }
        let title = if action == PowerAction::Reboot { "Restart" } else { "Shut down" };
        self.add_internal(title, "settings", 400, TITLE_H + 190, Body::Auth { action, field: TextField::secret(), error: String::new() });
    }

    pub fn submit_auth(&mut self, id: u32) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        let (action, password) = match &self.core.windows[index].body {
            Body::Auth { action, field, .. } => (*action, field.text.clone()),
            _ => return,
        };
        let command = if action == PowerAction::Reboot { "reboot" } else { "poweroff" };
        match hxclient::ui::elevated_spawn(Some(&password), "/usr/bin/hsh", &["-c", command], None) {
            Ok(_) => {
                sys::sleep_ms(200);
            }
            Err(e) => {
                if let Body::Auth { error, field, .. } = &mut self.core.windows[index].body {
                    *error = String::from(e);
                    field.set("");
                }
                let area = self.core.windows[index].outer();
                self.core.add_damage(area);
            }
        }
    }

    pub fn open_picker(&mut self, client: i64, request: u32, mode: u32, title: &str, filters: &str, start: &str) {
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1).max(1);
        let mode_name = match mode {
            hxproto::PICK_FOLDER => "folder",
            hxproto::PICK_SAVE_FILE => "save",
            _ => "open",
        };
        let args = [
            format!("--pick={}", mode_name),
            format!("--token={}", token),
            format!("--title={}", title),
            format!("--filter={}", filters),
            format!("--start={}", start),
        ];
        let files = apps::by_role(ROLE_FILES);
        let program = files.map(|f| apps::get(f).program.clone()).unwrap_or_else(|| String::from("/usr/bin/hxfiles"));
        let pid = sys::spawn(&program, &args, sys::SPAWN_DETACH);
        if pid < 0 {
            send_event(client, Event::FileChosen { request, status: hxproto::PICK_FAILED, path: hxproto::Text::new("") });
            return;
        }
        if let Some(files) = files {
            self.launched.push((pid, files));
        }
        self.pickers.push(Picker { token, client, request, pid });
    }

    fn reap_pickers(&mut self) {
        let mut index = 0;
        while index < self.pickers.len() {
            let (client, pid) = (self.pickers[index].client, self.pickers[index].pid);
            if !sys::proc_alive(pid) {
                let picker = self.pickers.remove(index);
                send_event(picker.client, Event::FileChosen { request: picker.request, status: hxproto::PICK_CANCELLED, path: hxproto::Text::new("") });
                continue;
            }
            if !sys::proc_alive(client) {
                sys::kill(pid);
                self.pickers.remove(index);
                continue;
            }
            index += 1;
        }
    }

    pub fn reap_clients(&mut self) {
        self.reap_pickers();
        let dead: Vec<u32> = self.core.windows.iter().filter(|w| w.client_pid().map(|pid| !sys::proc_alive(pid)).unwrap_or(false)).map(|w| w.id).collect();
        for id in dead {
            self.remove_window(id, false);
        }
        self.launched.retain(|(pid, _)| sys::proc_alive(*pid));
    }

    pub fn watch_registry(&mut self, first: bool) {
        let names: Vec<String> = sys::cmd_list().into_iter().map(|c| c.name).collect();
        if !first {
            let fresh: Vec<String> = names.iter().filter(|n| !self.known_commands.contains(n)).cloned().collect();
            for name in fresh {
                self.toast("New command", &format!("'{}' was added to the registry", name), "apps/commands");
            }
        }
        self.known_commands = names;
    }

    pub fn update_work_area(&mut self) {
        let (w, h) = (self.core.screen.width, self.core.screen.height);
        let reserve = if self.autohide { 0 } else { self.dock_h() + 2 * DOCK_MARGIN };
        self.core.work = Area::new(0, TOP_H, w, (h - TOP_H - reserve).max(120));
    }

    fn map_framebuffer(&mut self) -> bool {
        let Some(resized) = self.core.remap() else {
            return false;
        };
        if resized {
            let (w, h) = (self.core.screen.width, self.core.screen.height);
            self.background = alloc::vec![0; (w * h) as usize];
            self.update_work_area();
            self.load_wallpaper();
            self.core.fit_to_screen();
            self.clamp_desktop_icons();
            self.broadcast_screen();
        }
        true
    }

    fn broadcast_screen(&mut self) {
        let (width, height) = (self.core.screen.width as u32, self.core.screen.height as u32);
        let mut pids: Vec<i64> = self.core.windows.iter().filter_map(|w| w.client_pid()).collect();
        pids.extend(self.pickers.iter().map(|p| p.pid));
        let wayland = sys::service_lookup("hxwayland");
        if wayland > 0 {
            pids.push(wayland);
        }
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            send_event(pid, Event::Screen { width, height });
        }
    }

    fn handle_mouse(&mut self) {
        let mut current = MouseState::default();
        sys::mouse(&mut current);
        let (nx, ny) = (current.x.clamp(0, self.core.screen.width - 1), current.y.clamp(0, self.core.screen.height - 1));
        let counts = [(current.buttons >> 8) & 15, (current.buttons >> 12) & 15, (current.buttons >> 16) & 15];
        let now_buttons = current.buttons & 7;
        if (nx, ny) != self.core.pointer {
            self.on_motion(nx, ny);
            self.core.move_pointer(nx, ny);
        }
        for (index, bit) in [MOUSE_LEFT, MOUSE_RIGHT, sys::MOUSE_MIDDLE].into_iter().enumerate() {
            let was_down = self.core.buttons & bit != 0;
            let is_down = now_buttons & bit != 0;
            let mut presses = counts[index];
            if presses == 0 && !was_down && is_down {
                presses = 1;
            }
            if was_down && presses > 0 {
                self.core.buttons &= !bit;
                self.on_release(nx, ny, bit);
            }
            for i in 0..presses {
                self.core.buttons |= bit;
                self.on_press(nx, ny, bit);
                if i + 1 < presses || !is_down {
                    self.core.buttons &= !bit;
                    self.on_release(nx, ny, bit);
                }
            }
            if presses == 0 && was_down && !is_down {
                self.core.buttons &= !bit;
                self.on_release(nx, ny, bit);
            }
        }
        self.core.buttons = now_buttons;
        if current.wheel != 0 {
            self.on_wheel(current.wheel);
        }
    }

    pub fn on_press(&mut self, x: i32, y: i32, button: u32) {
        if self.switcher.is_some() {
            self.finish_switcher();
        }
        if self.core.windows.iter().any(|w| w.is_popup() && w.parent != Some(0)) {
            let inside = self.core.window_at(x, y).map(|i| self.core.windows[i].is_popup()).unwrap_or(false);
            if !inside {
                self.dismiss_client_popups();
                return;
            }
        }
        if let Some(index) = self.core.windows.iter().rposition(|w| w.parent == Some(0)) {
            let owner = self.core.windows[index].client_pid();
            let hit = self.core.window_at(x, y).and_then(|i| self.core.windows[i].client_pid());
            if hit != owner || y < TOP_H {
                let id = self.core.windows[index].id;
                let content = self.core.windows[index].content();
                if let Some(pid) = owner {
                    self.grab = Grab::Client(id);
                    send_event(pid, Event::Mouse { window: id, x: x - content.x, y: y - content.y, buttons: self.core.buttons, kind: hxproto::MOUSE_PRESS, wheel: button as i32 });
                }
                return;
            }
        }
        if !self.popup.is_none() {
            if self.popup_press(x, y, button) {
                return;
            }
        }
        if y < TOP_H {
            self.top_bar_press(x, button);
            return;
        }
        for i in 0..self.toasts.len() {
            if self.toast_area(i).contains(x, y) {
                self.toasts.remove(i);
                let area = self.toast_region();
                self.core.add_damage(area);
                return;
            }
        }
        if self.dock_visible() && self.dock_area().contains(x, y) {
            self.dock_press(x, y, button);
            return;
        }
        if self.window_press(x, y, button) {
            return;
        }
        self.desktop_press(x, y, button);
    }

    pub fn on_release(&mut self, x: i32, y: i32, button: u32) {
        match self.grab {
            Grab::Move { id, .. } => {
                self.grab = Grab::None;
                self.finish_move(id);
                self.core.set_cursor(0);
            }
            Grab::Resize { id, .. } => {
                self.grab = Grab::None;
                self.core.request_client_size(id, true);
                self.core.set_cursor(0);
            }
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
            Grab::Client(id) => {
                if self.core.buttons == 0 {
                    self.grab = Grab::None;
                }
                if let Some(index) = self.core.index_of(id) {
                    if let Body::Client { pid, .. } = self.core.windows[index].body {
                        let content = self.core.windows[index].content();
                        send_event(pid, Event::Mouse { window: id, x: x - content.x, y: y - content.y, buttons: self.core.buttons, kind: hxproto::MOUSE_RELEASE, wheel: button as i32 });
                    }
                }
            }
            Grab::None => {}
        }
    }

    pub fn on_motion(&mut self, x: i32, y: i32) {
        match self.grab {
            Grab::Move { id, dx, dy } => {
                self.drag_move(id, x, y, dx, dy);
                return;
            }
            Grab::Resize { id, edges, start, px, py } => {
                self.drag_resize(id, edges, start, x - px, y - py);
                return;
            }
            Grab::DockDrag { index, start_x, moved } => {
                self.drag_dock(index, start_x, moved, x);
                return;
            }
            Grab::LauncherScroll { offset } => {
                self.drag_launcher_scroll(y, offset);
                return;
            }
            Grab::IconDrag { index, dx, dy, start_x, start_y, moved } => {
                self.drag_icon(index, dx, dy, start_x, start_y, moved, x, y);
                return;
            }
            Grab::VolumeDrag => {
                self.set_volume_from_x(x);
                return;
            }
            _ => {}
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

        let target = if let Grab::Client(id) = self.grab {
            Some(id)
        } else {
            let over_dock = self.dock_visible() && self.dock_area().contains(x, y);
            self.core.window_at(x, y).filter(|_| !over_dock).map(|i| self.core.windows[i].id).filter(|id| {
                let index = self.core.index_of(*id).unwrap();
                self.core.windows[index].content().contains(x, y) && self.core.windows[index].client_pid().is_some()
            })
        };
        if target != self.hover_client {
            if let Some(old) = self.hover_client {
                if let Some(index) = self.core.index_of(old) {
                    if let Some(pid) = self.core.windows[index].client_pid() {
                        send_event(pid, Event::Mouse { window: old, x: -1, y: -1, buttons: 0, kind: hxproto::MOUSE_LEAVE, wheel: 0 });
                    }
                }
            }
            self.hover_client = target;
        }
        let mut cursor = 0usize;
        if let Some(id) = target {
            if let Some(index) = self.core.index_of(id) {
                let content = self.core.windows[index].content();
                cursor = self.core.windows[index].cursor as usize;
                if let Some(pid) = self.core.windows[index].client_pid() {
                    send_event(pid, Event::Mouse { window: id, x: x - content.x, y: y - content.y, buttons: self.core.buttons, kind: hxproto::MOUSE_MOVE, wheel: 0 });
                }
            }
        } else if self.popup.is_none() {
            if let Some((_, edges)) = self.core.resize_edge_at(x, y) {
                cursor = edge_cursor(edges);
            }
        }
        self.core.set_cursor(cursor);

        let (px, py) = self.core.pointer;
        let hot = self.hot_buttons.clone();
        for area in hot {
            if area.contains(px, py) != area.contains(x, y) {
                self.core.add_damage(area.expand(2));
            }
        }

        let dock = self.dock_hit(x, y);
        if dock != self.dock_hover {
            self.dock_hover = dock;
            let region = self.dock_region();
            self.core.add_damage(region);
        }
        let over_window = self.core.window_at(x, y).is_some();
        let icon = if !over_window && self.popup.is_none() && y >= TOP_H { self.desktop_icon_at(x, y) } else { None };
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

    pub fn on_wheel(&mut self, wheel: i32) {
        let (x, y) = self.core.pointer;
        if matches!(self.popup, Popup::Launcher) && self.launcher_area().contains(x, y) {
            self.launcher_wheel(wheel);
            return;
        }
        if (y < TOP_H && self.volume_button().contains(x, y)) || (matches!(self.popup, Popup::Sound) && self.sound_area().contains(x, y)) {
            self.volume_wheel(wheel);
            return;
        }
        if self.dock_visible() && self.dock_area().contains(x, y) {
            return;
        }
        if let Some(index) = self.core.window_at(x, y) {
            let window = &self.core.windows[index];
            if let Body::Client { pid, .. } = window.body {
                let content = window.content();
                send_event(pid, Event::Mouse { window: window.id, x: x - content.x, y: y - content.y, buttons: self.core.buttons, kind: hxproto::MOUSE_WHEEL, wheel });
            }
        }
    }

    pub fn on_key(&mut self, code: i32) {
        match code {
            KEY_ALT_TAB => {
                self.cycle_switcher();
                return;
            }
            KEY_SUPER => {
                if matches!(self.popup, Popup::Launcher) {
                    self.set_popup(Popup::None);
                } else {
                    self.set_popup(Popup::Launcher);
                }
                return;
            }
            KEY_ALT_F4 => {
                if let Some(id) = self.core.focused() {
                    self.close_window(id);
                }
                return;
            }
            KEY_SNAP_LEFT | KEY_SNAP_RIGHT | KEY_SNAP_UP | KEY_SNAP_DOWN => {
                self.keyboard_snap(code);
                return;
            }
            _ => {}
        }
        if self.switcher.is_some() {
            self.finish_switcher();
        }
        if self.popup_key(code) {
            return;
        }
        match self.core.focused().and_then(|id| self.core.index_of(id)) {
            Some(index) => {
                let id = self.core.windows[index].id;
                let area = self.core.windows[index].outer();
                match &mut self.core.windows[index].body {
                    Body::Client { pid, .. } => send_event(*pid, Event::Key { window: id, code, mods: self.key_mods }),
                    Body::About => {
                        if code == 27 || code == 10 {
                            self.remove_window(id, false);
                        }
                    }
                    Body::Auth { field, .. } => match code {
                        27 => self.remove_window(id, false),
                        10 => self.submit_auth(id),
                        _ => {
                            field.key(code);
                            self.core.add_damage(area);
                        }
                    },
                }
            }
            None => {
                if code == 32 || code == 10 {
                    self.set_popup(Popup::Launcher);
                }
            }
        }
    }

    pub fn on_message(&mut self, sender: i64, bytes: &[u8]) {
        let Some(request) = hxproto::Request::decode(bytes) else {
            return;
        };
        if let hxproto::Request::DisplayChanged = request {
            self.map_framebuffer();
            return;
        }
        if let hxproto::Request::Reload = request {
            if hxclient::ui::theme::load() {
                self.broadcast_theme();
            }
            self.load_wallpaper();
            let cfg = config::load();
            self.autohide = cfg.autohide;
            let before = self.dock_region();
            self.dock_size = cfg.dock_size;
            self.dock_limit = cfg.dock_limit;
            self.core.add_damage(before);
            self.update_work_area();
            self.anim = anim::Prefs::load();
            self.core.damage_all();
            return;
        }
        match request {
            hxproto::Request::ChooseFile { request: id, mode, title, filters, start } => {
                self.open_picker(sender, id, mode, title.as_str(), filters.as_str(), start.as_str());
                return;
            }
            hxproto::Request::PickerResult { token, status, path } => {
                if let Some(index) = self.pickers.iter().position(|p| p.token == token && p.pid == sender) {
                    let picker = self.pickers.remove(index);
                    send_event(picker.client, Event::FileChosen { request: picker.request, status, path });
                }
                return;
            }
            _ => {}
        }
        if let hxproto::Request::Notify { title, body } = request {
            let icon = self.core.windows.iter().find(|w| w.client_pid() == Some(sender)).map(|w| format!("apps/{}", w.icon)).filter(|i| i.len() > 5).unwrap_or_default();
            self.toast(title.as_str(), body.as_str(), &icon);
            return;
        }
        self.client_request(sender, request);
    }
}

fn main() -> i32 {
    let Some(mut screen) = Screen::take() else {
        println!("hxserver: cannot take the framebuffer");
        return 1;
    };
    screen.configure_flip(config::load().pageflip);
    if sys::service_register(hxproto::SERVICE) < 0 {
        println!("hxserver: another display server is already running");
        sys::release_framebuffer();
        return 1;
    }
    let (width, height) = (screen.width, screen.height);
    let mut state = MouseState::default();
    sys::mouse(&mut state);

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
    let now = sys::realtime().sec;
    let (year, month, _, _, _) = civil(now);

    let mut nook = Nook {
        core: Compositor::new(screen, (state.x.clamp(0, width - 1), state.y.clamp(0, height - 1)), state.buttons & 7, Area::new(0, TOP_H, width, height - TOP_H)),
        key_mods: 0,
        wayland: -1,
        background: alloc::vec![0; (width * height) as usize],
        ui,
        sized_icons: scan_cache,
        grab: Grab::None,
        snap_preview: None,
        hover_client: None,
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
        toasts: Vec::new(),
        known_commands: Vec::new(),
        running: true,
        last_minute: -1,
        user: users::name_of(sys::getuid() as u32),
        live,
        last_click: (0, -1, -1),
        launched: Vec::new(),
        hot_buttons: Vec::new(),
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
        switcher: None,
        switcher_until: 0,
        pickers: Vec::new(),
        next_token: 1,
        sound: SoundState::new(),
        anim: anim::Prefs::load(),
        animations: Vec::new(),
        pending_open: Vec::new(),
        last_compose: 0,
        launcher_scroll: 0,
        launcher_focus: None,
        apps_fingerprint: apps::fingerprint(),
        last_app_check: sys::uptime_ms(),
    };
    nook.update_work_area();
    nook.build_desktop(&cfg);
    let mut icons: Vec<String> = alloc::vec![String::from("logo"), String::from("logo-20"), String::from("about-logo-dark"), String::from("about-logo-light"), String::from("apps/folder-home"), String::from("apps/hello")];
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
    nook.core.load_cursors();
    write_toolkit_settings();
    nook.load_wallpaper();
    nook.watch_registry(true);
    nook.poll_network(true);
    nook.init_sound();
    nook.wayland = sys::spawn("/usr/bin/hxwayland", &[] as &[&str], sys::SPAWN_DETACH);
    nook.core.damage_all();
    nook.compose();
    if live {
        nook.toast("Welcome to HamixOS", "Double-click Install HamixOS to put it on a disk", "logo");
    }

    let mut message = [0u8; 4096];
    let mut last_housekeeping = sys::uptime_ms();

    while nook.running {
        let dragging = matches!(nook.grab, Grab::Move { .. } | Grab::Resize { .. });
        let animating = nook.dock_animating() || dragging || nook.switcher.is_some() || nook.animations_active();
        let timeout = if dragging && !nook.core.damage.is_empty() { 4 } else if animating { 16 } else if nook.toasts.is_empty() && !nook.sound.osd_shown && nook.sound.save_at == 0 { 500 } else { 100 };
        let ready = sys::wait_event(sys::EVENT_INPUT | sys::EVENT_MESSAGE, timeout);
        let now = sys::uptime_ms();

        if ready & sys::EVENT_MESSAGE != 0 {
            while let Some((sender, len)) = sys::msg_recv(&mut message, 0) {
                let bytes = message[..len].to_vec();
                nook.on_message(sender, &bytes);
            }
        }

        nook.handle_mouse();
        while let Some(raw) = sys::poll_key_code() {
            nook.key_mods = sys::key_modifiers(raw) as u32;
            nook.on_key(sys::key_base(raw) as i32);
        }

        let minute = sys::realtime().sec / 60;
        if minute != nook.last_minute {
            nook.last_minute = minute;
            let bar = nook.top_bar();
            nook.core.add_damage(bar);
        }
        if now.saturating_sub(last_housekeeping) >= 1000 {
            last_housekeeping = now;
            if !animating {
                hamix_std::heap::release_cached();
            }
            nook.reap_clients();
            nook.watch_registry(false);
            nook.watch_apps(now);
            if nook.core.screen.stale() {
                nook.map_framebuffer();
            }
        }
        nook.poll_network(false);
        nook.poll_sound(now);
        nook.flush_resize(now, false);
        nook.tick_animations(now);
        nook.tick_switcher(now);
        nook.update_dock_visibility();
        let before = nook.toasts.len();
        nook.toasts.retain(|t| t.until > now);
        if nook.toasts.len() != before {
            let area = nook.toast_region();
            nook.core.add_damage(area);
        }
        let paced = dragging && now.saturating_sub(nook.last_compose) < FRAME_MS;
        if !nook.core.damage.is_empty() && !paced {
            nook.compose();
        }
    }

    let ids: Vec<u32> = nook.core.windows.iter().map(|w| w.id).collect();
    for id in ids {
        nook.remove_window(id, true);
    }
    nook.core.release_cursor();
    sys::release_framebuffer();
    0
}

entry!(main);
