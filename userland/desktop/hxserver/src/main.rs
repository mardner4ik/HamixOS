#![no_std]
#![no_main]

extern crate alloc;

mod proto2;
mod anim;
#[allow(dead_code)]
mod apps;
mod chrome;
#[allow(dead_code)]
mod config;
mod compositor;
#[cfg(feature = "wayland")]
mod frontend;
mod render;
mod wm;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::sys::{self, MouseState, MOUSE_LEFT, MOUSE_RIGHT};
use hxstd::{entry, fs, println};
use hxclient::ui::{TextField, Ui};
use hxproto::Event;
use vellum::{Area, Image, Painter};

use crate::compositor::window::{edge_cursor, Body, TITLE_H};
use crate::compositor::{send_event, Compositor, Screen};

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
pub enum PowerAction {
    Logout,
    Reboot,
    Shutdown,
}

fn write_toolkit_settings() {
    let Some(home) = hxstd::env::var("HOME") else {
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
}

pub struct Toast {
    pub title: String,
    pub body: String,
    pub icon: String,
    pub until: u64,
}

pub struct Picker {
    pub token: u32,
    pub client: i64,
    pub request: u32,
    pub pid: i64,
    pub output: Option<u64>,
    pub chosen: Vec<u8>,
}

pub struct Nook {
    pub core: Compositor,
    pub key_mods: u32,
    pub input: Option<hxstd::input::Input>,
    pub uevents: Option<hxstd::uevent::Monitor>,
    pub wayland: i64,
    pub background: Vec<u32>,
    pub ui: Ui,
    pub sized_icons: BTreeMap<(String, i32), Image>,
    pub grab: Grab,
    pub snap_preview: Option<Area>,
    pub hover_client: Option<u32>,
    pub menu: Option<chrome::WindowMenu>,
    pub toasts: Vec<Toast>,
    pub known_commands: Vec<String>,
    pub running: bool,
    pub live: bool,
    pub last_click: (u64, i32, i32),
    pub launched: Vec<(i64, usize)>,
    pub closing: Vec<(u32, u64)>,
    pub apps_fingerprint: u64,
    pub last_app_check: u64,
    pub hot_buttons: Vec<(u32, Area)>,
    pub wallpaper: String,
    pub shell: Shell,
    pub minimize_targets: Vec<(u32, Area)>,
    pub switcher: Option<usize>,
    pub switcher_until: u64,
    pub pickers: Vec<Picker>,
    pub next_token: u32,
    pub anim: anim::Prefs,
    pub animations: Vec<anim::Animation>,
    pub pending_open: Vec<(u32, u64)>,
    pub last_compose: u64,
    pub v2: proto2::State,
}

pub struct Shell {
    pub pid: i64,
    pub started: u64,
    pub restarts: u32,
}

pub const SHELL_PROGRAM: &str = "/usr/bin/nook-shell";

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
        self.wallpaper = self.wallpaper_key();
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
        let Some(image) = fs::read(&path).and_then(|b| Image::decode_sized(&b, sw, sh)) else {
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
        Area::new(self.core.screen.width - 380, self.core.work.y, 380, 4 * 84 + 24)
    }

    pub fn toast_area(&self, index: usize) -> Area {
        Area::new(self.core.screen.width - 356, self.core.work.y + 12 + index as i32 * 84, 344, 72)
    }

    pub fn broadcast_theme(&mut self) {
        write_toolkit_settings();
        let light = hxclient::ui::theme::is_light();
        let mut pids: Vec<i64> = self.core.windows.iter().filter_map(|w| w.client_pid()).collect();
        pids.extend(self.pickers.iter().map(|p| p.pid));
        if self.shell.pid > 0 {
            pids.push(self.shell.pid);
        }
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            send_event(pid, Event::Theme { light });
        }
        self.core.damage_all();
    }

    pub fn refresh_apps(&mut self) {
        self.apps_fingerprint = apps::fingerprint();
        self.last_app_check = sys::uptime_ms();
        if !apps::scan(&mut self.ui, &mut self.sized_icons) {
            return;
        }
        for window in 0..self.core.windows.len() {
            let icon = self.core.windows[window].icon.clone();
            if !icon.is_empty() {
                self.preload_window_icon(&icon);
            }
        }
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
        let program = if action == PowerAction::Reboot { "/usr/bin/reboot" } else { "/usr/bin/poweroff" };
        if sys::access(program, sys::X_OK) {
            let pid = sys::spawn(program, &[] as &[&str], 0);
            if pid > 0 && sys::waitpid(pid, false) == 0 {
                return;
            }
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
        match hxclient::ui::elevated_spawn(Some(&password), "/usr/bin/bash", &["-c", command], None) {
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

    pub fn reload_config(&mut self) {
        if self.shell.pid > 0 && sys::proc_alive(self.shell.pid) {
            sys::send_signal(self.shell.pid, sys::SIGHUP);
        }
        if self.core.screen.stale() {
            self.map_framebuffer();
        }
        if hxclient::ui::theme::load() {
            self.broadcast_theme();
        }
        let wallpaper = self.wallpaper_key();
        if wallpaper != self.wallpaper {
            self.load_wallpaper();
            self.core.damage_all();
        }
        self.anim = anim::Prefs::load();
    }

    fn wallpaper_key(&self) -> String {
        let path = config::load().wallpaper.unwrap_or_else(|| String::from(DEFAULT_WALLPAPER));
        let size = sys::stat(&path).map(|s| s.size).unwrap_or(0);
        format!("{} {} {}x{}", path, size, self.core.screen.width, self.core.screen.height)
    }

    fn ask_wallpaper(&mut self, client: i64, request: u32, path: &str) {
        let decoded = fs::read(path).and_then(|b| Image::decode_sized(&b, 960, 540));
        let Some(image) = decoded else {
            send_event(client, Event::WallpaperResult { request, accepted: false });
            return;
        };
        let (w, h) = (480, 270);
        let scale = |a: i32, b: i32, c: i32| (a as i64 * b as i64 / c.max(1) as i64) as i32;
        let (pw, ph) = if image.w as i64 * h as i64 > image.h as i64 * w as i64 { (w, scale(image.h, w, image.w).max(1)) } else { (scale(image.w, h, image.h).max(1), h) };
        let preview = image.scaled(pw, ph);
        let window = self.core.windows.iter().find(|w| w.client_pid() == Some(client));
        let app = window
            .and_then(|w| app_by_icon(&w.icon).map(|a| apps::get(a).name.clone()).or_else(|| Some(w.title.clone())))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| String::from("An application"));
        let over = self.core.windows.iter().rev().find(|w| !w.is_popup() && w.client_pid() == Some(client)).map(|w| w.outer());
        self.add_dialog(over, "Change wallpaper", "settings", 520, TITLE_H + 410, Body::Wallpaper { client, request, path: String::from(path), app, preview });
    }

    pub fn answer_wallpaper(&mut self, id: u32, accepted: bool) {
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        let Body::Wallpaper { client, request, path, .. } = &mut self.core.windows[index].body else {
            return;
        };
        let (pid, request, path) = (*client, *request, path.clone());
        *client = 0;
        if accepted {
            hxclient::ui::write_nook_config("wallpaper", &path);
            self.load_wallpaper();
            self.core.damage_all();
        }
        send_event(pid, Event::WallpaperResult { request, accepted });
        self.remove_window(id, false);
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
        let pipe = sys::pipe().ok();
        let pid = sys::spawn_io(&program, &args, sys::SPAWN_DETACH, None, pipe.map(|p| p.1), None);
        if let Some((_, write_end)) = pipe {
            sys::close(write_end);
        }
        if pid < 0 {
            if let Some((read_end, _)) = pipe {
                sys::close(read_end);
            }
            send_event(client, Event::FileChosen { request, status: hxproto::PICK_FAILED, path: hxproto::Text::new("") });
            return;
        }
        if let Some((read_end, _)) = pipe {
            sys::set_nonblocking(read_end);
        }
        if let Some(files) = files {
            self.launched.push((pid, files));
        }
        self.pickers.push(Picker { token, client, request, pid, output: pipe.map(|p| p.0), chosen: Vec::new() });
    }

    pub fn reap_pickers(&mut self) {
        let mut index = 0;
        while index < self.pickers.len() {
            let (client, pid) = (self.pickers[index].client, self.pickers[index].pid);
            if let Some(fd) = self.pickers[index].output {
                let mut buf = [0u8; 1024];
                loop {
                    let n = sys::read(fd, &mut buf);
                    if n <= 0 {
                        break;
                    }
                    self.pickers[index].chosen.extend_from_slice(&buf[..n as usize]);
                }
            }
            if !sys::proc_alive(pid) {
                let picker = self.pickers.remove(index);
                if let Some(fd) = picker.output {
                    sys::close(fd);
                }
                let text = String::from_utf8_lossy(&picker.chosen);
                let path = text.lines().next().unwrap_or("").trim_end();
                let status = if path.starts_with('/') { hxproto::PICK_OK } else { hxproto::PICK_CANCELLED };
                send_event(picker.client, Event::FileChosen { request: picker.request, status, path: hxproto::Text::new(path) });
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
            let visible = self.core.index_of(id).map(|i| !self.core.windows[i].is_popup() && !self.core.windows[i].minimized).unwrap_or(false);
            if visible {
                self.start_animation(id, anim::Effect::Close, None, anim::Finish::None);
            }
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
        let before = self.core.work;
        self.core.work = Area::new(0, 0, w, h);
        self.apply_exclusive_zones();
        if self.core.work != before && !self.toasts.is_empty() {
            self.core.damage_all();
        }
    }

    pub fn is_shell(&self, pid: i64) -> bool {
        pid > 0 && pid == self.shell.pid
    }

    pub fn start_shell(&mut self) {
        let program = hxstd::env::var("NOOK_SHELL").unwrap_or(SHELL_PROGRAM);
        if program == "none" || !sys::access(program, sys::X_OK) {
            return;
        }
        self.shell.pid = sys::spawn(program, &[] as &[&str], sys::SPAWN_DETACH);
        self.shell.started = sys::uptime_ms();
    }

    pub fn watch_shell(&mut self, now: u64) {
        if self.shell.pid <= 0 || sys::proc_alive(self.shell.pid) {
            return;
        }
        self.shell.pid = -1;
        if now.saturating_sub(self.shell.started) > 30_000 {
            self.shell.restarts = 0;
        }
        if self.shell.restarts >= 3 {
            self.toast("The desktop shell stopped", "Panel and dock are unavailable until the next login", "ui/bell");
            return;
        }
        self.shell.restarts += 1;
        self.start_shell();
    }

    pub fn shell_key(&mut self, code: i32) -> bool {
        let me = sys::getpid();
        let own = self.v2.layers.iter().find(|l| self.shell.pid > 0 && l.pid == self.shell.pid);
        let wayland = || self.v2.layers.iter().find(|l| l.pid == me && l.layer == hxproto::LAYER_BOTTOM && l.anchor == 15);
        let Some(layer) = own.or_else(wayland) else {
            return false;
        };
        send_event(layer.pid, Event::Key { window: layer.id, code, mods: self.key_mods });
        true
    }

    fn map_framebuffer(&mut self) -> bool {
        let Some(resized) = self.core.remap() else {
            return false;
        };
        if resized {
            let (w, h) = (self.core.screen.width, self.core.screen.height);
            if let Some(input) = self.input.as_mut() {
                input.set_bounds(w, h);
            }
            self.background = alloc::vec![0; (w * h) as usize];
            self.update_work_area();
            self.relayout_layers();
            self.load_wallpaper();
            self.core.fit_to_screen();
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
        if self.shell.pid > 0 {
            pids.push(self.shell.pid);
        }
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            send_event(pid, Event::Screen { width, height });
        }
    }

    fn read_mouse(&mut self, state: &mut MouseState) {
        match self.input.as_mut() {
            Some(input) => {
                input.pump();
                input.mouse(state);
            }
            None => {
                sys::mouse(state);
            }
        }
    }

    fn next_key(&mut self) -> Option<i64> {
        match self.input.as_mut() {
            Some(input) => {
                input.pump();
                input.next_key()
            }
            None => sys::poll_key_code(),
        }
    }

    fn handle_mouse(&mut self) {
        let mut current = MouseState::default();
        self.read_mouse(&mut current);
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
        if self.menu.is_some() {
            self.menu_press(x, y);
            return;
        }
        if self.overlay_grab_press(x, y, button) {
            return;
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
            if hit != owner {
                let id = self.core.windows[index].id;
                let content = self.core.windows[index].content();
                if let Some(pid) = owner {
                    self.grab = Grab::Client(id);
                    send_event(pid, Event::Mouse { window: id, x: x - content.x, y: y - content.y, buttons: self.core.buttons, kind: hxproto::MOUSE_PRESS, wheel: button as i32 });
                }
                return;
            }
        }
        self.v2.keyboard_layer = None;
        if self.layer_press(x, y, button, true) {
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
        if self.window_press(x, y, button) {
            return;
        }
        self.layer_press(x, y, button, false);
    }

    pub fn on_release(&mut self, x: i32, y: i32, button: u32) {
        if self.v2.drag.is_some() {
            self.finish_drag(x, y);
        }
        if self.layer_release(x, y, button) {
            return;
        }
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
            Grab::Client(id) => {
                if self.core.buttons == 0 {
                    self.grab = Grab::None;
                }
                if let Some(index) = self.core.index_of(id) {
                    if let Body::Client { pid, .. } | Body::Wayland { pid, .. } = self.core.windows[index].body {
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
            _ => {}
        }
        if self.menu.is_some() {
            self.menu_motion(x, y);
            self.core.set_cursor(0);
            return;
        }

        if !matches!(self.grab, Grab::Client(_)) && self.layer_motion(x, y) {
            return;
        }
        let target = if let Grab::Client(id) = self.grab {
            Some(id)
        } else {
            self.core.window_at(x, y).map(|i| self.core.windows[i].id).filter(|id| {
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
        } else if let Some((_, edges)) = self.core.resize_edge_at(x, y) {
            cursor = edge_cursor(edges);
        } else if let Some(index) = self.core.window_at(x, y) {
            if matches!(self.core.windows[index].body, Body::Auth { .. }) {
                let content = self.core.windows[index].content();
                if Area::new(content.x + 20, content.y + 70, content.w - 40, 38).contains(x, y) {
                    cursor = hxproto::CURSOR_TEXT as usize;
                }
            }
        }
        self.core.set_cursor(cursor);
        self.hover_title_buttons(x, y);
    }

    fn hover_title_buttons(&mut self, x: i32, y: i32) {
        let (px, py) = self.core.pointer;
        let hot: Vec<Area> = self.hot_buttons.iter().map(|(_, a)| *a).collect();
        for area in hot {
            if area.contains(px, py) != area.contains(x, y) {
                self.core.add_damage(area.expand(2));
            }
        }
    }

    pub fn shell_request(&mut self, request: &hxproto::Request) -> bool {
        match *request {
            hxproto::Request::ShellAction { action } => match action {
                hxproto::SHELL_LOGOUT => self.start_power(PowerAction::Logout),
                hxproto::SHELL_REBOOT => self.start_power(PowerAction::Reboot),
                hxproto::SHELL_SHUTDOWN => self.start_power(PowerAction::Shutdown),
                hxproto::SHELL_ABOUT => self.open_about(),
                _ => {}
            },
            hxproto::Request::Toast { title, body, icon } => {
                let icon = String::from(icon.as_str());
                self.toast(title.as_str(), body.as_str(), &icon);
            }
            hxproto::Request::ToplevelRectangle { toplevel, x, y, width, height } => {
                self.minimize_targets.retain(|(id, _)| *id != toplevel);
                if width > 0 && height > 0 {
                    self.minimize_targets.push((toplevel, Area::new(x, y, width, height)));
                }
            }
            hxproto::Request::SetAppId { window: 0, pid, app_id } => {
                if let Some(app) = app_by_icon(app_id.as_str()) {
                    self.launched.retain(|(p, _)| *p != pid as i64);
                    self.launched.push((pid as i64, app));
                }
            }
            _ => return false,
        }
        true
    }

    pub fn on_wheel(&mut self, wheel: i32) {
        let (x, y) = self.core.pointer;
        if self.menu.is_some() {
            return;
        }
        if self.layer_wheel(x, y, wheel) {
            return;
        }
        if let Some(index) = self.core.window_at(x, y) {
            let window = &self.core.windows[index];
            if let Body::Client { pid, .. } | Body::Wayland { pid, .. } = window.body {
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
                self.shell_key(code);
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
        if self.menu.is_some() {
            if code == 27 {
                self.close_menu();
            }
            return;
        }
        let ctrl_alt = sys::MOD_CTRL as u32 | sys::MOD_ALT as u32;
        if self.key_mods & ctrl_alt == ctrl_alt && (code == -3 || code == -4) {
            let current = self.core.workspace;
            let next = if code == -3 { current.checked_sub(1) } else { Some(current + 1) };
            if let Some(next) = next {
                self.switch_workspace(next);
            }
            return;
        }
        if self.layer_key(code) {
            return;
        }
        match self.core.focused().and_then(|id| self.core.index_of(id)) {
            Some(index) => {
                let id = self.core.windows[index].id;
                let area = self.core.windows[index].outer();
                match &mut self.core.windows[index].body {
                    Body::Client { pid, .. } | Body::Wayland { pid, .. } => send_event(*pid, Event::Key { window: id, code, mods: self.key_mods }),
                    Body::About => {
                        if code == 27 || code == 10 {
                            self.remove_window(id, false);
                        }
                    }
                    Body::Wallpaper { .. } => match code {
                        27 => self.answer_wallpaper(id, false),
                        10 => self.answer_wallpaper(id, true),
                        _ => {}
                    },
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
                    self.shell_key(code);
                }
            }
        }
    }

    pub fn on_message(&mut self, sender: i64, bytes: &[u8]) {
        let Some(request) = hxproto::Request::decode(bytes) else {
            return;
        };
        self.handle_request(sender, request);
    }

    pub fn handle_request(&mut self, sender: i64, request: hxproto::Request) {
        if let hxproto::Request::SetWallpaper { request: id, path } = request {
            self.ask_wallpaper(sender, id, path.as_str());
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
                    if let Some(fd) = picker.output {
                        sys::close(fd);
                    }
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
        if self.is_shell(sender) && self.shell_request(&request) {
            return;
        }
        if self.v2_request(sender, &request) {
            return;
        }
        self.client_request(sender, request);
    }
}

fn main() -> i32 {
    let mut taken = Screen::take();
    let deadline = sys::uptime_ms() + 5000;
    while taken.is_none() && sys::uptime_ms() < deadline {
        sys::sleep_ms(100);
        taken = Screen::take();
    }
    let Some(mut screen) = taken else {
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
    let mut input = if screen.drm.is_some() { hxstd::input::Input::open(width, height) } else { None };
    println!(
        "hxserver: {}x{} via {}, input via {}",
        width,
        height,
        if screen.drm.is_some() { "/dev/dri/card0" } else { "the framebuffer" },
        if input.is_some() { "/dev/input/event*" } else { "the console" }
    );
    let mut state = MouseState::default();
    match input.as_mut() {
        Some(i) => i.mouse(&mut state),
        None => {
            sys::mouse(&mut state);
        }
    }

    let live = fs::read_to_string("/proc/mounts").and_then(|t| t.lines().next().map(|l| l.starts_with("live"))).unwrap_or(false);
    let mut ui = Ui::load();
    let mut scan_cache = BTreeMap::new();
    apps::scan(&mut ui, &mut scan_cache);

    let mut nook = Nook {
        core: Compositor::new(screen, (state.x.clamp(0, width - 1), state.y.clamp(0, height - 1)), state.buttons & 7, Area::new(0, 0, width, height)),
        key_mods: 0,
        input,
        uevents: hxstd::uevent::Monitor::open(),
        wayland: -1,
        background: alloc::vec![0; (width * height) as usize],
        ui,
        sized_icons: scan_cache,
        grab: Grab::None,
        snap_preview: None,
        hover_client: None,
        menu: None,
        toasts: Vec::new(),
        known_commands: Vec::new(),
        running: true,
        live,
        last_click: (0, -1, -1),
        launched: Vec::new(),
        closing: Vec::new(),
        hot_buttons: Vec::new(),
        wallpaper: String::new(),
        shell: Shell { pid: -1, started: 0, restarts: 0 },
        minimize_targets: Vec::new(),
        switcher: None,
        switcher_until: 0,
        pickers: Vec::new(),
        next_token: 1,
        anim: anim::Prefs::load(),
        animations: Vec::new(),
        pending_open: Vec::new(),
        last_compose: 0,
        v2: proto2::State::new(),
        apps_fingerprint: apps::fingerprint(),
        last_app_check: sys::uptime_ms(),
    };
    nook.update_work_area();
    let mut icons: Vec<String> = alloc::vec![String::from("logo"), String::from("about-logo-dark"), String::from("about-logo-light"), String::from("apps/hello")];
    for i in 0..apps::count() {
        icons.push(format!("apps/{}", apps::get(i).key));
        icons.push(format!("apps/{}-24", apps::get(i).key));
    }
    for name in ["minimize", "maximize", "restore", "close", "bell", "info"] {
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
    let me = sys::getpid();
    #[cfg(feature = "wayland")]
    let mut bridge = if hxstd::env::var("NOOK_WAYLAND").as_deref() == Some("external") {
        None
    } else {
        let link = frontend::wayland::Embedded { nook: &mut nook as *mut Nook, me, mailbox: sys::mailbox_fd().max(0) as u64 };
        match frontend::wayland::protocol::Bridge::start(link) {
            Ok(b) => {
                sys::service_register("hxwayland");
                Some(b)
            }
            Err(e) => {
                println!("hxserver: no built-in Wayland: {}", e);
                None
            }
        }
    };
    #[cfg(feature = "wayland")]
    let external = bridge.is_none();
    #[cfg(not(feature = "wayland"))]
    let external = true;
    if external {
        nook.wayland = sys::spawn("/usr/libexec/hamix/hxwayland", &[] as &[&str], sys::SPAWN_DETACH);
    }
    nook.start_shell();
    nook.core.damage_all();
    nook.compose();
    if live {
        nook.toast("Welcome to HamixOS", "Double-click Install HamixOS to put it on a disk", "logo");
    }

    sys::catch_hangup();
    let mut message = [0u8; 4096];
    let mut last_housekeeping = sys::uptime_ms();
    let mut last_sweep = last_housekeeping;

    while nook.running {
        let dragging = matches!(nook.grab, Grab::Move { .. } | Grab::Resize { .. });
        let animating = dragging || nook.switcher.is_some() || nook.animations_active();
        #[allow(unused_mut)]
        let mut timeout: u64 = if dragging && !nook.core.damage.is_empty() { 4 } else if animating { 16 } else if nook.toasts.is_empty() { 500 } else { 100 };
        let mut entries = alloc::vec![sys::WaitEntry::mailbox()];
        #[cfg(feature = "wayland")]
        if let Some(b) = bridge.as_mut() {
            for (fd, pending) in b.fds() {
                if fd >= 0 {
                    entries.push(sys::WaitEntry::fd(fd as u64));
                }
                if pending {
                    timeout = timeout.min(16);
                }
            }
            if let Some(t) = b.timeout_ms() {
                timeout = timeout.min(t);
            }
        }
        if let Some(input) = nook.input.as_ref() {
            entries.extend(input.fds().into_iter().map(sys::WaitEntry::fd));
        }
        if let Some(monitor) = nook.uevents.as_ref() {
            entries.push(sys::WaitEntry::fd(monitor.fd()));
        }
        if nook.input.is_none() {
            entries.push(sys::WaitEntry::fd(0));
        }
        sys::wait_any(&mut entries, Some(timeout as u64));
        let now = sys::uptime_ms();
        if sys::take_hangup() {
            nook.reload_config();
        }
        if !nook.pickers.is_empty() {
            nook.reap_pickers();
        }

        if entries[0].ready != 0 {
            while let Some((sender, len)) = sys::msg_recv(&mut message, 0) {
                if sender == me {
                    #[cfg(feature = "wayland")]
                    if let (Some(b), Some(event)) = (bridge.as_mut(), hxproto::Event::decode(&message[..len])) {
                        b.deliver(event);
                    }
                    continue;
                }
                let bytes = message[..len].to_vec();
                nook.on_message(sender, &bytes);
            }
        }
        let mut hotplug = false;
        if let Some(monitor) = nook.uevents.as_mut() {
            while let Some(event) = monitor.next() {
                hotplug |= event.subsystem() == "drm" && event.get("HOTPLUG") == Some("1");
            }
        }
        if hotplug {
            nook.map_framebuffer();
        }

        nook.handle_mouse();
        #[cfg(feature = "wayland")]
        if let Some(b) = bridge.as_mut() {
            b.service(0);
        }
        while let Some(raw) = nook.next_key() {
            nook.key_mods = sys::key_modifiers(raw) as u32;
            nook.on_key(sys::key_base(raw) as i32);
        }

        if now.saturating_sub(last_housekeeping) >= 1000 {
            last_housekeeping = now;
            if !animating {
                hxstd::heap::release_cached();
            }
            nook.reap_clients();
            nook.reap_layers();
            nook.watch_shell(now);
            if now.saturating_sub(last_sweep) >= 10_000 {
                last_sweep = now;
                sys::shm_sweep();
            }
            nook.watch_registry(false);
            nook.watch_apps(now);
            if nook.core.screen.stale() {
                nook.map_framebuffer();
            }
        }
        nook.flush_resize(now, false);
        nook.tick_animations(now);
        nook.tick_switcher(now);
        let before = nook.toasts.len();
        nook.toasts.retain(|t| t.until > now);
        if nook.toasts.len() != before {
            let area = nook.toast_region();
            nook.core.add_damage(area);
        }
        nook.sync_toplevels();
        let paced = dragging && now.saturating_sub(nook.last_compose) < FRAME_MS;
        if !nook.core.damage.is_empty() && !paced {
            nook.compose();
        }
    }

    let ids: Vec<u32> = nook.core.windows.iter().map(|w| w.id).collect();
    for id in ids {
        nook.remove_window(id, true);
    }
    if nook.shell.pid > 0 {
        sys::send_signal(nook.shell.pid, sys::SIGTERM);
    }
    #[cfg(feature = "wayland")]
    if let Some(b) = bridge.as_mut() {
        b.shutdown();
    }
    nook.core.release_cursor();
    sys::release_framebuffer();
    0
}

entry!(main);
