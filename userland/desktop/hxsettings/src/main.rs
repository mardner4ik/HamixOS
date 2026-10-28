#![no_std]
#![no_main]

extern crate alloc;

mod pages;
mod system;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::permissions;
use hxstd::{audio, display, env, net, sys, users};
use hxtoolkit::{App, Context, Element, Pixmap, Settings, Tone};

use system::Failure;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Network,
    Display,
    Sound,
    Appearance,
    Desktop,
    Apps,
    Users,
    System,
}

pub const PAGES: [(Page, &str, &str, &str); 8] = [
    (Page::Network, "network", "Network", "ui/network"),
    (Page::Display, "display", "Display", "ui/display"),
    (Page::Sound, "sound", "Sound", "ui/sound"),
    (Page::Appearance, "appearance", "Appearance", "ui/wallpaper"),
    (Page::Desktop, "desktop", "Desktop & Dock", "ui/maximize"),
    (Page::Apps, "apps", "Apps", "ui/apps"),
    (Page::Users, "users", "Users", "ui/user"),
    (Page::System, "system", "System", "ui/system"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    None,
    App(usize),
    About,
    Startup,
    Boot,
}

pub struct Effect {
    pub key: &'static str,
    pub label: &'static str,
    pub on: &'static str,
}

pub const EFFECTS: [Effect; 5] = [
    Effect { key: "anim_open", label: "Opening windows", on: "appear" },
    Effect { key: "anim_close", label: "Closing windows", on: "appear" },
    Effect { key: "anim_minimize", label: "Minimizing into the dock", on: "magic-lamp" },
    Effect { key: "anim_restore", label: "Restoring from the dock", on: "magic-lamp" },
    Effect { key: "anim_snap", label: "Resizing and snapping", on: "morph" },
];

pub const SPEEDS: [(&str, u32); 3] = [("Slow", 60), ("Normal", 100), ("Fast", 170)];
pub const DOCK_SIZES: [(&str, i32); 4] = [("Small", 36), ("Medium", 48), ("Large", 64), ("Huge", 80)];
pub const DOCK_LIMITS: [u32; 5] = [6, 8, 12, 16, 24];

#[derive(Clone)]
pub enum Msg {
    Page(usize),
    Open(Sub),
    Back,
    Refresh,
    Thumb,
    Style(usize),
    Wallpaper(usize),
    Autohide(bool),
    DockSize(usize),
    DockLimit(usize),
    Effect(usize, bool),
    Speed(usize),
    Resolution(usize),
    Rate(usize),
    ApplyDisplay,
    KeepDisplay,
    RevertDisplay,
    Mirror(u32),
    NetMode(usize),
    WifiPick(usize),
    WifiPassword(String),
    WifiConnect,
    WifiDisconnect,
    WifiScan,
    WifiForget(String),
    WifiAutojoin(String, bool),
    Dhcp(usize),
    Address(String),
    Gateway(String),
    Dns(String),
    ApplyNetwork,
    Volume(f32),
    Mute(bool),
    TestSound,
    PickApp(usize),
    Permit(String, bool),
    ResetPermissions,
    Current(String),
    New(String),
    Repeat(String),
    ChangePassword,
    Autostart(bool),
    Linux(bool),
    ApplyStartup,
    AdminPassword(String),
    Unlock,
    CancelUnlock,
}

pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub requested: Vec<String>,
    pub required: Vec<String>,
    pub granted: Vec<String>,
    pub declared: bool,
}

struct Pending {
    args: Vec<String>,
    done: String,
}

pub struct Prefs {
    pub page: Page,
    pub sub: Sub,
    pub light: bool,
    pub styles: [Pixmap; 2],
    pub wallpapers: Vec<system::Wallpaper>,
    pub wallpaper: String,
    pub autohide: bool,
    pub dock_size: i32,
    pub dock_limit: u32,
    pub effects: Vec<bool>,
    pub speed: u32,
    pub display: display::Info,
    pub chosen: (u32, u32, u32),
    pub net: net::Status,
    pub networks: Vec<net::WifiNetwork>,
    pub saved_networks: Vec<net::SavedNetwork>,
    pub wifi_selected: Option<usize>,
    pub wifi_password: String,
    pub dhcp: bool,
    pub address: String,
    pub gateway: String,
    pub dns: String,
    pub last_net: u64,
    pub audio: audio::Info,
    pub volume: Option<audio::Volume>,
    pub apps: Vec<AppEntry>,
    pub current: String,
    pub new: String,
    pub repeat: String,
    pub autostart: bool,
    pub autostart_saved: bool,
    pub linux: bool,
    pub linux_saved: bool,
    pub linux_available: bool,
    pub admin: String,
    pub unlock: Option<String>,
    pub status: (String, Tone),
    pending: Option<Pending>,
}

fn load_apps() -> Vec<AppEntry> {
    let mut apps: Vec<AppEntry> = permissions::applications()
        .into_iter()
        .filter_map(|id| {
            let declaration = permissions::declaration(&id)?;
            let declared = declaration.requested.is_some();
            if declaration.hidden && !declared {
                return None;
            }
            Some(AppEntry {
                name: if declaration.name.is_empty() { id.clone() } else { declaration.name.clone() },
                icon: system::app_icon(&declaration.icon),
                granted: permissions::granted(&id),
                requested: permissions::requested(&id),
                required: declaration.required,
                declared,
                id,
            })
        })
        .collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

impl Prefs {
    fn new() -> Prefs {
        let value = system::nook_value;
        let effects = EFFECTS.iter().map(|e| value(e.key).map(|v| v != "disabled").unwrap_or(true)).collect();
        let wanted = env::args().get(1).map(|a| match a.as_str() {
            "wallpaper" | "theme" => "appearance",
            "dock" | "animations" | "effects" => "desktop",
            "account" | "user" => "users",
            "about" | "session" | "startup" => "system",
            "permissions" => "apps",
            other => other,
        });
        let page = wanted.and_then(|a| PAGES.iter().find(|p| p.1 == a)).map(|p| p.0).unwrap_or(Page::Appearance);
        let autostart = system::autostart();
        let linux = system::linux_state() != "off";
        let mut prefs = Prefs {
            page,
            sub: Sub::None,
            light: value("theme").as_deref() == Some("light"),
            styles: [system::style_preview(false), system::style_preview(true)],
            wallpapers: system::wallpapers(),
            wallpaper: value("wallpaper").unwrap_or_else(|| String::from("/usr/share/wallpapers/dusk.png")),
            autohide: value("dock_autohide").map(|v| v != "no").unwrap_or(true),
            dock_size: value("dock_size").and_then(|v| v.parse().ok()).unwrap_or(48),
            dock_limit: value("dock_limit").and_then(|v| v.parse().ok()).unwrap_or(12),
            effects,
            speed: value("anim_speed").and_then(|v| v.parse().ok()).unwrap_or(100),
            display: display::info(),
            chosen: (0, 0, 0),
            net: net::status(),
            networks: Vec::new(),
            saved_networks: Vec::new(),
            wifi_selected: None,
            wifi_password: String::new(),
            dhcp: true,
            address: String::new(),
            gateway: String::new(),
            dns: String::new(),
            last_net: 0,
            audio: audio::info(),
            volume: audio::volume(),
            apps: load_apps(),
            current: String::new(),
            new: String::new(),
            repeat: String::new(),
            autostart,
            autostart_saved: autostart,
            linux,
            linux_saved: linux,
            linux_available: hxstd::fs::exists("/linux/bin/busybox"),
            admin: String::new(),
            unlock: None,
            status: (String::new(), Tone::Dim),
            pending: None,
        };
        prefs.refresh_display();
        prefs.refresh_network(false);
        prefs
    }

    fn ok(&mut self, text: impl Into<String>) {
        self.status = (text.into(), Tone::Success);
    }

    fn fail(&mut self, text: impl Into<String>) {
        self.status = (text.into(), Tone::Danger);
    }

    fn refresh_display(&mut self) {
        let was_pending = self.display.revert_in.is_some();
        self.display = display::info();
        let c = self.display.current;
        if self.chosen.0 == 0 || (was_pending && self.display.revert_in.is_none()) {
            self.chosen = (c.width, c.height, c.refresh);
        }
    }

    fn refresh_network(&mut self, scan: bool) {
        self.net = net::status();
        if self.net.has_kind("wifi") {
            self.networks = net::wifi_scan(scan);
            self.saved_networks = net::wifi_known();
        }
        self.last_net = sys::uptime_ms();
    }

    fn refresh_apps(&mut self) {
        let selected = match self.sub {
            Sub::App(i) => self.apps.get(i).map(|a| a.id.clone()),
            _ => None,
        };
        self.apps = load_apps();
        if let Some(id) = selected {
            self.sub = self.apps.iter().position(|a| a.id == id).map(Sub::App).unwrap_or(Sub::None);
        }
    }

    pub fn sizes(&self) -> Vec<(u32, u32)> {
        let mut sizes: Vec<(u32, u32)> = Vec::new();
        for m in self.display.modes.iter() {
            if !sizes.contains(&(m.width, m.height)) {
                sizes.push((m.width, m.height));
            }
        }
        sizes.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
        sizes
    }

    pub fn rates(&self) -> Vec<u32> {
        let mut rates: Vec<u32> = self.display.modes.iter().filter(|m| m.width == self.chosen.0 && m.height == self.chosen.1).map(|m| m.refresh).collect();
        rates.sort_by(|a, b| b.cmp(a));
        rates.dedup();
        rates
    }

    fn privileged(&mut self, args: &[&str], done: &str) {
        let password = self.admin.clone();
        match system::sysconf(args, &password) {
            Ok(()) => {
                self.admin.clear();
                self.unlock = None;
                self.pending = None;
                self.ok(done);
                self.after_privileged();
            }
            Err(Failure::NeedPassword) => {
                self.pending = Some(Pending { args: args.iter().map(|a| String::from(*a)).collect(), done: String::from(done) });
                self.unlock = Some(String::new());
                self.status.0.clear();
            }
            Err(Failure::Failed(e)) => {
                self.admin.clear();
                if self.pending.is_some() || !system::privileged_without_password() {
                    self.pending = Some(Pending { args: args.iter().map(|a| String::from(*a)).collect(), done: String::from(done) });
                    self.unlock = Some(e.clone());
                    self.status.0.clear();
                } else {
                    self.fail(e);
                }
            }
        }
    }

    fn after_privileged(&mut self) {
        self.autostart = system::autostart();
        self.autostart_saved = self.autostart;
        self.linux = system::linux_state() != "off";
        self.linux_saved = self.linux;
        self.refresh_apps();
        self.refresh_network(false);
    }

    fn unlock(&mut self) {
        let Some(pending) = self.pending.take() else {
            self.unlock = None;
            return;
        };
        let args: Vec<&str> = pending.args.iter().map(|a| a.as_str()).collect();
        let done = pending.done.clone();
        self.privileged(&args, &done);
    }

    fn change_password(&mut self) {
        if self.new.is_empty() || self.new != self.repeat {
            self.fail("The new passwords are empty or do not match");
            return;
        }
        let r = if sys::geteuid() == 0 {
            users::set_password("", &system::user(), &self.new).map(|_| 0).unwrap_or(-13)
        } else {
            sys::set_own_password(&self.current, &self.new)
        };
        if r < 0 {
            self.fail("The current password is wrong");
        } else {
            self.current.clear();
            self.new.clear();
            self.repeat.clear();
            self.ok("Password changed");
        }
    }

    fn apply_network(&mut self) {
        if self.dhcp {
            self.privileged(&["netconf", "dhcp"], "The address is now assigned automatically");
            return;
        }
        if !self.address.contains('.') {
            self.fail("Enter the address as 192.168.1.20/24");
            return;
        }
        let (address, gateway, dns) = (String::from(self.address.trim()), String::from(self.gateway.trim()), String::from(self.dns.trim()));
        self.privileged(&["netconf", "static", &address, &gateway, &dns], "The manual address was applied");
    }

    fn apply_startup(&mut self) {
        if self.autostart != self.autostart_saved {
            let value = if self.autostart { "on" } else { "off" };
            self.privileged(&["session", "autostart", value], if self.autostart { "Nook starts right after login" } else { "The text console is used after login" });
            if self.unlock.is_some() {
                return;
            }
        }
        if self.linux != self.linux_saved {
            let value = if self.linux { "on" } else { "off" };
            self.privileged(&["linux", value], if self.linux { "Linux programs can run" } else { "Linux programs are turned off" });
        }
    }

    fn play_test_sound(&mut self) {
        let rate = 48000u32;
        let Ok(stream) = audio::Stream::open(rate, 2) else {
            self.status = (String::from("The sound device is busy, missing, or Settings may not use Sound"), Tone::Warning);
            return;
        };
        let mut samples = Vec::with_capacity(rate as usize);
        for freq in [660.0f32, 880.0f32] {
            let frames = rate as usize / 4;
            let mut phase = 0.0f32;
            for i in 0..frames {
                let t = i as f32 / frames as f32;
                let envelope = if t < 0.05 { t / 0.05 } else { (1.0 - t).max(0.0) };
                phase += freq / rate as f32;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                let tri = if phase < 0.5 { phase * 4.0 - 1.0 } else { 3.0 - phase * 4.0 };
                let v = (tri * 9000.0 * envelope) as i16;
                samples.push(v);
                samples.push(v);
            }
        }
        let _ = stream.write_all(&samples);
        stream.close_after_playing();
    }

    fn enter(&mut self, page: Page) {
        self.status.0.clear();
        match page {
            Page::Display => self.refresh_display(),
            Page::Network => self.refresh_network(false),
            Page::Sound => {
                self.audio = audio::info();
                self.volume = audio::volume();
            }
            Page::Apps => self.refresh_apps(),
            _ => {}
        }
    }
}

impl App for Prefs {
    type Message = Msg;

    fn title(&self) -> String {
        String::from("Settings")
    }

    fn view(&self) -> Element<Msg> {
        pages::view(self)
    }

    fn update(&mut self, message: Msg, _cx: &mut Context<Msg>) {
        match message {
            Msg::Page(i) => {
                let page = PAGES[i.min(PAGES.len() - 1)].0;
                if page != self.page || self.sub != Sub::None {
                    self.page = page;
                    self.sub = Sub::None;
                    self.enter(page);
                }
            }
            Msg::Open(sub) => {
                self.sub = sub;
                self.status.0.clear();
            }
            Msg::Back => {
                self.sub = Sub::None;
                self.status.0.clear();
            }
            Msg::Refresh => match self.page {
                Page::Display => self.refresh_display(),
                Page::Network if sys::uptime_ms().saturating_sub(self.last_net) >= 2000 => self.refresh_network(false),
                Page::Sound => {
                    self.audio = audio::info();
                    self.volume = audio::volume();
                }
                _ => {}
            },
            Msg::Thumb => {
                if let Some(w) = self.wallpapers.iter_mut().find(|w| w.thumb.is_none()) {
                    w.thumb = Some(system::thumbnail(&w.path));
                }
            }
            Msg::Style(i) => {
                self.light = i == 1;
                if !system::set_desktop("theme", if self.light { "light" } else { "dark" }) {
                    self.fail("Cannot save the setting");
                }
            }
            Msg::Wallpaper(i) => {
                if let Some(path) = self.wallpapers.get(i).map(|w| w.path.clone()) {
                    if system::set_desktop("wallpaper", &path) {
                        self.wallpaper = path;
                    } else {
                        self.fail("Cannot save the setting");
                    }
                }
            }
            Msg::Autohide(on) => {
                self.autohide = on;
                system::set_desktop("dock_autohide", if on { "yes" } else { "no" });
            }
            Msg::DockSize(i) => {
                self.dock_size = DOCK_SIZES[i].1;
                system::set_desktop("dock_size", &format!("{}", self.dock_size));
            }
            Msg::DockLimit(i) => {
                self.dock_limit = DOCK_LIMITS[i];
                system::set_desktop("dock_limit", &format!("{}", self.dock_limit));
            }
            Msg::Effect(e, on) => {
                self.effects[e] = on;
                system::set_desktop(EFFECTS[e].key, if on { EFFECTS[e].on } else { "disabled" });
            }
            Msg::Speed(i) => {
                self.speed = SPEEDS[i].1;
                system::set_desktop("anim_speed", &format!("{}", self.speed));
            }
            Msg::Resolution(i) => {
                if let Some((w, h)) = self.sizes().get(i).copied() {
                    let rates: Vec<u32> = self.display.modes.iter().filter(|m| m.width == w && m.height == h).map(|m| m.refresh).collect();
                    let refresh = if rates.contains(&self.chosen.2) { self.chosen.2 } else { rates.iter().copied().max().unwrap_or(60) };
                    self.chosen = (w, h, refresh);
                }
            }
            Msg::Rate(i) => {
                if let Some(r) = self.rates().get(i).copied() {
                    self.chosen.2 = r;
                }
            }
            Msg::ApplyDisplay => {
                let (w, h, r) = self.chosen;
                let result = display::set_mode(w, h, r, true);
                if result < 0 {
                    self.fail(format!("{}×{} at {} Hz is not supported: {}", w, h, r, sys::error_name(result)));
                } else {
                    system::reload_desktop();
                    self.status.0.clear();
                }
                self.refresh_display();
            }
            Msg::KeepDisplay => {
                display::confirm();
                self.refresh_display();
                self.ok("The new display mode is kept");
            }
            Msg::RevertDisplay => {
                display::revert();
                system::reload_desktop();
                self.chosen = (0, 0, 0);
                self.refresh_display();
            }
            Msg::Mirror(id) => {
                display::set_output(id, self.display.current.width, self.display.current.height);
                system::reload_desktop();
                self.refresh_display();
            }
            Msg::NetMode(i) => {
                let mode = [net::MODE_ETHERNET, net::MODE_WIFI, net::MODE_OFF][i.min(2)];
                let r = net::set_mode(mode);
                if r < 0 {
                    self.fail(format!("Cannot switch the network: {}", sys::error_name(r)));
                }
                self.wifi_selected = None;
                self.refresh_network(mode == net::MODE_WIFI);
            }
            Msg::WifiPick(i) => {
                self.wifi_selected = Some(i);
                self.wifi_password.clear();
            }
            Msg::WifiPassword(v) => self.wifi_password = v,
            Msg::WifiConnect => {
                if let Some(network) = self.wifi_selected.and_then(|i| self.networks.get(i)).cloned() {
                    let r = net::wifi_connect(&network.ssid, &self.wifi_password);
                    if r < 0 {
                        self.fail(format!("Cannot connect: {}", sys::error_name(r)));
                    } else {
                        self.ok(format!("Connecting to {}…", network.ssid));
                    }
                    self.wifi_password.clear();
                    self.saved_networks = net::wifi_known();
                }
            }
            Msg::WifiDisconnect => {
                net::wifi_disconnect();
                self.refresh_network(false);
            }
            Msg::WifiScan => self.refresh_network(true),
            Msg::WifiForget(ssid) => {
                let r = net::wifi_forget(&ssid);
                if r < 0 {
                    self.fail(format!("Cannot forget {}: {}", ssid, sys::error_name(r)));
                }
                self.saved_networks = net::wifi_known();
            }
            Msg::WifiAutojoin(ssid, on) => {
                let r = net::wifi_autojoin(&ssid, on);
                if r < 0 {
                    self.fail(format!("Cannot change {}: {}", ssid, sys::error_name(r)));
                }
                self.saved_networks = net::wifi_known();
            }
            Msg::Dhcp(i) => self.dhcp = i == 0,
            Msg::Address(v) => self.address = v,
            Msg::Gateway(v) => self.gateway = v,
            Msg::Dns(v) => self.dns = v,
            Msg::ApplyNetwork => self.apply_network(),
            Msg::Volume(level) => {
                let level = (level as u32).min(100);
                self.volume = audio::set_volume(level);
                if level > 0 {
                    self.volume = audio::set_muted(false).or(self.volume.take());
                }
                system::set_nook_value("audio_volume", &format!("{}", level));
            }
            Msg::Mute(muted) => {
                self.volume = audio::set_muted(muted);
                system::set_nook_value("audio_muted", if muted { "yes" } else { "no" });
            }
            Msg::TestSound => self.play_test_sound(),
            Msg::PickApp(i) => {
                self.sub = Sub::App(i);
                self.status.0.clear();
            }
            Msg::Permit(group, allow) => {
                if let Sub::App(i) = self.sub {
                    if let Some(app) = self.apps.get(i) {
                        let id = app.id.clone();
                        let label = permissions::find(&group).map(|p| p.label).unwrap_or("");
                        let done = format!("{}: {} {} · takes effect the next time it starts", app.name, label, if allow { "allowed" } else { "turned off" });
                        self.privileged(&["permit", &id, &group, if allow { "yes" } else { "no" }], &done);
                    }
                }
            }
            Msg::ResetPermissions => {
                if let Sub::App(i) = self.sub {
                    if let Some(app) = self.apps.get(i) {
                        let (id, done) = (app.id.clone(), format!("{} uses its default permissions again", app.name));
                        self.privileged(&["reset", &id], &done);
                    }
                }
            }
            Msg::Current(v) => self.current = v,
            Msg::New(v) => self.new = v,
            Msg::Repeat(v) => self.repeat = v,
            Msg::ChangePassword => self.change_password(),
            Msg::Autostart(on) => self.autostart = on,
            Msg::Linux(on) => self.linux = on,
            Msg::ApplyStartup => self.apply_startup(),
            Msg::AdminPassword(v) => self.admin = v,
            Msg::Unlock => self.unlock(),
            Msg::CancelUnlock => {
                self.unlock = None;
                self.pending = None;
                self.admin.clear();
                self.autostart = self.autostart_saved;
                self.linux = self.linux_saved;
            }
        }
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        if self.page == Page::Appearance && self.wallpapers.iter().any(|w| w.thumb.is_none()) {
            return Some((10, Msg::Thumb));
        }
        matches!(self.page, Page::Display | Page::Network | Page::Sound).then_some((1000, Msg::Refresh))
    }
}

pub fn hostname() -> String {
    hxstd::fs::read_to_string("/etc/hostname").map(|h| String::from(h.trim())).filter(|h| !h.is_empty()).unwrap_or_else(|| String::from("hamix"))
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("settings").icon("settings").size(940, 660).min_size(760, 520), Prefs::new())
}

hxstd::entry!(run);
