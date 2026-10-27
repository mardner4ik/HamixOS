#![no_std]
#![no_main]

extern crate alloc;

mod layout;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{audio, display, entry, env, eprintln, fs, net, sys, users};
use hxclient::ui::{self, theme, FieldAction, Hits, Style, TextField, Ui};
use hxclient::{Event, Window, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_WHEEL};
use layout::{row, segmented, value_row, Flow, CONTROL_H, GAP, PAD, ROW_H};
use vellum::gfx::ellipsize;
use vellum::{Area, Image, Painter};

const INITIAL_W: i32 = 900;
const INITIAL_H: i32 = 640;
const SIDEBAR: i32 = 216;

const PAGE_APPEARANCE: usize = 0;
const PAGE_ANIMATIONS: usize = 1;
const PAGE_DISPLAY: usize = 2;
const PAGE_NETWORK: usize = 3;
const PAGE_SOUND: usize = 4;
const PAGE_SESSION: usize = 5;
const PAGE_ACCOUNT: usize = 6;
const PAGE_SYSTEM: usize = 7;
const PAGE_ABOUT: usize = 8;

struct PageInfo {
    title: &'static str,
    icon: &'static str,
    subtitle: &'static str,
    group: &'static str,
}

const PAGES: [PageInfo; 9] = [
    PageInfo { title: "Appearance", icon: "ui/wallpaper", subtitle: "Wallpaper and desktop", group: "Desktop" },
    PageInfo { title: "Animations", icon: "ui/apps", subtitle: "How windows open, close and move", group: "Desktop" },
    PageInfo { title: "Display", icon: "ui/display", subtitle: "Resolution and refresh rate", group: "Hardware" },
    PageInfo { title: "Network", icon: "ui/network", subtitle: "Ethernet, Wi-Fi and addresses", group: "Hardware" },
    PageInfo { title: "Sound", icon: "ui/sound", subtitle: "Output device and volume", group: "Hardware" },
    PageInfo { title: "Session", icon: "ui/session", subtitle: "What starts after login", group: "System" },
    PageInfo { title: "Account", icon: "ui/user", subtitle: "Your user and password", group: "System" },
    PageInfo { title: "System status", icon: "ui/system", subtitle: "What started at boot", group: "System" },
    PageInfo { title: "About", icon: "ui/info", subtitle: "This computer", group: "System" },
];

const FIELD_CURRENT: usize = 0;
const FIELD_NEW: usize = 1;
const FIELD_REPEAT: usize = 2;
const FIELD_ADMIN: usize = 3;
const FIELD_WIFI: usize = 4;
const FIELD_ADDRESS: usize = 5;
const FIELD_GATEWAY: usize = 6;
const FIELD_DNS: usize = 7;

struct AnimEffect {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    choices: &'static [(&'static str, &'static str)],
    default: &'static str,
}

const LAMP: [(&str, &str); 2] = [("magic-lamp", "Magic Lamp"), ("disabled", "Disabled")];
const MORPH: [(&str, &str); 2] = [("morph", "Morph"), ("disabled", "Disabled")];
const APPEAR: [(&str, &str); 2] = [("appear", "Appear"), ("disabled", "Disabled")];

const ANIMATIONS: [AnimEffect; 5] = [
    AnimEffect { key: "anim_open", label: "Opening a window", description: "The window fades in and grows slightly into place", choices: &APPEAR, default: "appear" },
    AnimEffect { key: "anim_close", label: "Closing a window", description: "The window fades out and shrinks slightly", choices: &APPEAR, default: "appear" },
    AnimEffect { key: "anim_minimize", label: "Minimizing", description: "The window flows into its dock icon", choices: &LAMP, default: "magic-lamp" },
    AnimEffect { key: "anim_restore", label: "Restoring from the dock", description: "The window flows back out of its dock icon", choices: &LAMP, default: "magic-lamp" },
    AnimEffect { key: "anim_snap", label: "Resizing, maximizing and snapping", description: "The window glides to its new size and the new content fades in", choices: &MORPH, default: "morph" },
];

const SPEEDS: [(&str, u32); 3] = [("Slow", 60), ("Normal", 100), ("Fast", 170)];

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Page(usize),
    Wallpaper(usize),
    Theme(bool),
    Autohide(bool),
    DockSize(i32),
    DockLimit(u32),
    Animation(usize, usize),
    AnimationSpeed(u32),
    Autostart,
    Field(usize),
    ApplySession,
    ChangePassword,
    Resolution(u32, u32),
    Refresh(u32),
    ApplyDisplay,
    KeepDisplay,
    RevertDisplay,
    Mirror(u32, bool),
    NetMode(u64),
    WifiRow(usize),
    WifiConnect,
    WifiScan,
    WifiForget(usize),
    WifiAutojoin(usize),
    Dhcp(bool),
    ApplyNetwork,
    VolumeSlider,
    Mute,
    TestSound,
}

struct Wallpaper {
    path: String,
    thumb: Option<Image>,
    title: String,
    credit: String,
}

fn wallpaper_credits() -> Vec<(String, String, String, String)> {
    fs::read_to_string("/usr/share/wallpapers/CREDITS")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            if f.len() >= 4 { Some((String::from(f[0]), String::from(f[1]), String::from(f[2]), String::from(f[3]))) } else { None }
        })
        .collect()
}

struct App {
    window: Window,
    ui: Ui,
    page: usize,
    wallpapers: Vec<Wallpaper>,
    current_wallpaper: String,
    autohide: bool,
    dock_size: i32,
    dock_limit: u32,
    animations: Vec<String>,
    anim_speed: u32,
    autostart: bool,
    autostart_saved: bool,
    fields: Vec<TextField>,
    focus: Option<usize>,
    message: (String, u32),
    hits: Hits<Hit>,
    hover: Option<Hit>,
    display: display::Info,
    chosen: (u32, u32, u32),
    net: net::Status,
    networks: Vec<net::WifiNetwork>,
    saved_networks: Vec<net::SavedNetwork>,
    wifi_selected: Option<usize>,
    dhcp: bool,
    last_refresh: u64,
    scroll: i32,
    content_h: i32,
    view_h: i32,
    slider: Area,
}

fn login_conf() -> Vec<(String, String)> {
    fs::read_to_string("/etc/hamix/login.conf")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (String::from(k.trim()), String::from(v.trim()))))
        .collect()
}

fn config_value(key: &str) -> Option<String> {
    ui::read_nook_config().into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

impl App {
    fn size(&self) -> (i32, i32) {
        (self.window.width() as i32, self.window.height() as i32)
    }

    fn load_wallpapers(&mut self) {
        let mut paths: Vec<String> = Vec::new();
        for dir in [String::from("/usr/share/wallpapers"), format!("{}/Pictures", ui::home_dir())] {
            for e in sys::read_dir(&dir).unwrap_or_default() {
                if !e.is_dir && e.name.to_lowercase().ends_with(".png") {
                    paths.push(format!("{}/{}", dir, e.name));
                }
            }
        }
        let credits = wallpaper_credits();
        self.wallpapers = paths
            .into_iter()
            .map(|path| {
                let file = String::from(path.rsplit('/').next().unwrap_or(""));
                let known = credits.iter().find(|c| c.0 == file && path.starts_with("/usr/share/wallpapers/"));
                let title = known.map(|c| c.1.clone()).unwrap_or_else(|| String::from(file.trim_end_matches(".png").trim_end_matches(".PNG")));
                let credit = known.map(|c| if c.2 == "HamixOS project" { String::from("HamixOS") } else { format!("{} · {}", c.2, c.3) }).unwrap_or_else(|| String::from("Your picture"));
                Wallpaper { path, thumb: None, title, credit }
            })
            .collect();
    }

    fn load_thumbnail(&mut self) -> bool {
        for w in self.wallpapers.iter_mut() {
            if w.thumb.is_none() {
                let (dir, file) = w.path.rsplit_once('/').unwrap_or(("", ""));
                let ready = fs::read(&format!("{}/thumbs/{}", dir, file)).and_then(|b| Image::from_png(&b)).filter(|i| i.w == 172 && i.h == 104);
                if let Some(img) = ready {
                    w.thumb = Some(img);
                    return true;
                }
                let image = fs::read(&w.path).and_then(|b| Image::from_png(&b)).map(|img| {
                    let (tw, th) = (172, 104);
                    let scale_w = img.w as i64 * th as i64;
                    let scale_h = img.h as i64 * tw as i64;
                    let (cw, ch) = if scale_w > scale_h { ((img.h as i64 * tw as i64 / th as i64) as i32, img.h) } else { (img.w, (img.w as i64 * th as i64 / tw as i64) as i32) };
                    let ox = (img.w - cw) / 2;
                    let oy = (img.h - ch) / 2;
                    let mut crop = Image { w: cw, h: ch, px: Vec::with_capacity((cw * ch) as usize) };
                    for y in 0..ch {
                        let start = ((oy + y) * img.w + ox) as usize;
                        crop.px.extend_from_slice(&img.px[start..start + cw as usize]);
                    }
                    crop.scaled(tw, th)
                });
                w.thumb = Some(image.unwrap_or_else(|| Image::solid(172, 104, 0x333333)));
                return true;
            }
        }
        false
    }

    fn refresh_display(&mut self) {
        let was_pending = self.display.revert_in.is_some();
        self.display = display::info();
        if was_pending && self.display.revert_in.is_none() {
            let c = self.display.current;
            self.chosen = (c.width, c.height, c.refresh);
            if self.message.1 == theme::warn() {
                self.message.0.clear();
            }
        }
        let c = self.display.current;
        if self.chosen.0 == 0 {
            self.chosen = (c.width, c.height, c.refresh);
        }
    }

    fn refresh_network(&mut self, scan: bool) {
        self.net = net::status();
        if self.net.has_kind("wifi") {
            self.networks = net::wifi_scan(scan);
            self.saved_networks = net::wifi_known();
        }
        self.last_refresh = sys::uptime_ms();
    }

    fn elevated(&mut self, command: &str) -> bool {
        let password = self.fields[FIELD_ADMIN].text.clone();
        match ui::elevated_spawn(if password.is_empty() { None } else { Some(&password) }, "/usr/bin/hsh", &["-c", command], None) {
            Ok(pid) => {
                let code = sys::waitpid(pid, false);
                if code == 0 {
                    self.fields[FIELD_ADMIN].set("");
                    true
                } else {
                    self.message = (format!("The change failed (code {})", code), theme::danger());
                    false
                }
            }
            Err(e) => {
                self.message = (String::from(e), theme::danger());
                self.focus = Some(FIELD_ADMIN);
                false
            }
        }
    }

    fn apply_session(&mut self) {
        let command = format!("session autostart {}", if self.autostart { "on" } else { "off" });
        if self.elevated(&command) {
            self.autostart_saved = self.autostart;
            self.message = (String::from(if self.autostart { "Nook will start automatically after login" } else { "The text shell will be used after login" }), theme::success());
        }
    }

    fn change_password(&mut self) {
        let (current, new, confirm) = (self.fields[FIELD_CURRENT].text.clone(), self.fields[FIELD_NEW].text.clone(), self.fields[FIELD_REPEAT].text.clone());
        if new.is_empty() || new != confirm {
            self.message = (String::from("The new passwords are empty or do not match"), theme::danger());
            return;
        }
        let r = if sys::geteuid() == 0 {
            match users::set_password("", &ui::current_user(), &new) {
                Ok(()) => 0,
                Err(_) => -13,
            }
        } else {
            sys::set_own_password(&current, &new)
        };
        if r < 0 {
            self.message = (String::from("The current password is wrong"), theme::danger());
        } else {
            self.message = (String::from("Password changed"), theme::success());
            for f in self.fields[..3].iter_mut() {
                f.set("");
            }
        }
    }

    fn apply_display(&mut self) {
        let (w, h, r) = self.chosen;
        let result = display::set_mode(w, h, r, true);
        if result < 0 {
            self.message = (format!("{}x{} at {} Hz is not supported: {}", w, h, r, sys::error_name(result)), theme::danger());
            return;
        }
        hxclient::display_changed();
        self.refresh_display();
        self.message.0.clear();
    }

    fn connect_wifi(&mut self) {
        let Some(index) = self.wifi_selected else {
            return;
        };
        let Some(network) = self.networks.get(index).cloned() else {
            return;
        };
        let password = self.fields[FIELD_WIFI].text.clone();
        let r = net::wifi_connect(&network.ssid, &password);
        self.message = if r < 0 { (format!("Cannot connect: {}", sys::error_name(r)), theme::danger()) } else { (format!("Connecting to {}…", network.ssid), theme::success()) };
        self.fields[FIELD_WIFI].set("");
        self.saved_networks = net::wifi_known();
    }

    fn apply_network(&mut self) {
        let command = if self.dhcp {
            String::from("netconf dhcp")
        } else {
            let address = String::from(self.fields[FIELD_ADDRESS].text.trim());
            if !address.contains('.') {
                self.message = (String::from("Enter the address as 192.168.1.20/24"), theme::danger());
                return;
            }
            format!("netconf static {} {} {}", address, self.fields[FIELD_GATEWAY].text.trim(), self.fields[FIELD_DNS].text.trim())
        };
        if self.elevated(&command) {
            self.message = (String::from(if self.dhcp { "Using DHCP" } else { "Static address applied" }), theme::success());
            self.refresh_network(false);
        }
    }

    fn animation_choice(&self, index: usize) -> &str {
        self.animations.get(index).map(|s| s.as_str()).unwrap_or(ANIMATIONS[index].default)
    }

    fn admin_block(&mut self, p: &mut Painter, flow: &mut Flow, action: Hit, label: &str) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        let needs_password = sys::geteuid() != 0;
        let card = flow.block(p, if needs_password { 118 } else { 74 });
        if needs_password {
            p.text(&ui.small, card.x + PAD, card.y + 16, &format!("Administrator password for {}", ui::current_user()), theme::faint());
            let field = Area::new(card.x + PAD, card.y + 36, (card.w / 2).clamp(180, 320), CONTROL_H);
            ui::text_field(p, ui, field, &mut self.fields[FIELD_ADMIN], self.focus == Some(FIELD_ADMIN), "Password");
            self.hits.add(field, Hit::Field(FIELD_ADMIN));
        }
        let button = Area::new(card.right() - PAD - 130, card.bottom() - PAD - CONTROL_H, 130, CONTROL_H);
        ui::button(p, ui, button, label, Style::Primary, hover == Some(action), true);
        self.hits.add(button, action);
    }

    fn draw(&mut self) {
        let (w, h) = self.size();
        let buffer = unsafe { &mut *(self.window.buffer() as *mut [u32]) };
        let mut p = Painter::new(buffer, w, h);
        self.hits.clear();
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        p.fill(Area::new(0, 0, w, h), theme::bg());
        let compact = w < 760;
        let sidebar = if compact { 60 } else { SIDEBAR };
        p.fill(Area::new(0, 0, sidebar, h), theme::sidebar());
        p.fill(Area::new(sidebar, 0, 1, h), theme::border());
        if !compact {
            p.text(&ui.title, PAD + 2, 20, "Settings", theme::text());
        }
        let mut y = if compact { 18 } else { 62 };
        let mut group = "";
        for (i, page) in PAGES.iter().enumerate() {
            if page.group != group && !compact {
                group = page.group;
                p.text(&ui.small, PAD + 2, y + 4, group, theme::faint());
                y += 22;
            }
            let item = if compact { Area::new(8, y, sidebar - 16, 40) } else { Area::new(10, y, sidebar - 20, 36) };
            ui::list_row(&mut p, item, self.page == i, hover == Some(Hit::Page(i)));
            let tint = if self.page == i { theme::on_selection() } else { theme::dim() };
            if compact {
                ui::symbol(&mut p, ui, page.icon, item.x + (item.w - 16) / 2, item.y + (item.h - 16) / 2, tint);
            } else {
                ui::symbol(&mut p, ui, page.icon, item.x + 12, item.y + 10, tint);
                ui::text_in(&mut p, &ui.font, item.x + 40, item, page.title, if self.page == i { theme::on_selection() } else { theme::text() });
            }
            self.hits.add(item, Hit::Page(i));
            y = item.bottom() + if compact { 6 } else { 4 };
        }

        let gutter = if compact { 16 } else { 28 };
        let header = Area::new(sidebar + gutter, 22, w - sidebar - gutter * 2, 52);
        p.text(&ui.title, header.x, header.y, PAGES[self.page].title, theme::text());
        p.text(&ui.small, header.x, header.y + 30, PAGES[self.page].subtitle, theme::faint());
        let footer = if self.message.0.is_empty() { 0 } else { 52 };
        let view = Area::new(header.x, header.bottom() + 8, header.w, h - header.bottom() - 16 - footer);
        self.view_h = view.h;
        let saved_clip = p.clip;
        p.set_clip(Area::new(view.x - 6, view.y, view.w + 12, view.h).intersect(&saved_clip));
        let mut flow = Flow::new(view, self.scroll);
        match self.page {
            PAGE_APPEARANCE => self.draw_appearance(&mut p, &mut flow),
            PAGE_ANIMATIONS => self.draw_animations(&mut p, &mut flow),
            PAGE_DISPLAY => self.draw_display(&mut p, &mut flow),
            PAGE_NETWORK => self.draw_network(&mut p, &mut flow),
            PAGE_SOUND => self.draw_sound(&mut p, &mut flow),
            PAGE_SESSION => self.draw_session(&mut p, &mut flow),
            PAGE_ACCOUNT => self.draw_account(&mut p, &mut flow),
            PAGE_SYSTEM => self.draw_system(&mut p, &mut flow),
            _ => self.draw_about(&mut p, &mut flow),
        }
        self.content_h = flow.height();
        p.clip = saved_clip;
        let max_scroll = (self.content_h - view.h).max(0);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        if max_scroll > 0 {
            let track = Area::new(view.right() + 6, view.y, 4, view.h);
            p.rounded(track, 2, theme::surface_2(), 255);
            let thumb_h = (view.h * view.h / self.content_h.max(1)).max(30);
            let offset = (view.h - thumb_h) * self.scroll / max_scroll;
            p.rounded(Area::new(track.x, track.y + offset, track.w, thumb_h), 2, theme::border(), 255);
        }
        if !self.message.0.is_empty() {
            let area = Area::new(header.x, h - 50, header.w.min(ui.font.measure(&self.message.0) + 40), 36);
            p.rounded(area, 8, theme::surface_2(), 255);
            p.fill(Area::new(area.x, area.y + 8, 3, area.h - 16), self.message.1);
            ui::text_in(&mut p, &ui.font, area.x + 16, area, &self.message.0, theme::text());
        }
        drop(p);
        self.window.present();
    }

    fn draw_appearance(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        flow.heading(p, ui, "Style");
        let tiles = flow.free(132);
        for (i, (light, label)) in [(false, "Dark"), (true, "Light")].into_iter().enumerate() {
            let area = Area::new(tiles.x + i as i32 * 190, tiles.y, 176, 108);
            let pal = if light { &theme::LIGHT } else { &theme::DARK };
            p.shadow(area, 10, 12, 120, 3);
            p.rounded(area, 10, pal.bg, 255);
            let win = Area::new(area.x + 16, area.y + 18, area.w - 32, area.h - 30);
            p.rounded(win, 7, pal.surface, 255);
            p.rounded_border(win, 7, pal.border, 255);
            p.fill(Area::new(win.x + 1, win.y + 18, win.w - 2, 1), pal.title_line);
            p.rounded(Area::new(win.x + 10, win.y + 7, 34, 5), 2, pal.text, 200);
            p.circle((win.right() - 12) as f32, (win.y + 9) as f32, 3.5, pal.danger, 255);
            p.rounded(Area::new(win.x + 10, win.y + 28, win.w - 60, 6), 3, pal.dim, 160);
            p.rounded(Area::new(win.x + 10, win.y + 40, win.w - 90, 6), 3, pal.faint, 160);
            p.rounded(Area::new(win.right() - 46, win.bottom() - 20, 36, 12), 4, pal.accent, 255);
            let selected = theme::is_light() == light;
            if selected {
                p.rounded_border(area, 10, theme::accent(), 255);
                p.rounded_border(area.inset(1), 9, theme::accent(), 255);
            } else if hover == Some(Hit::Theme(light)) {
                p.rounded_border(area, 10, theme::faint(), 200);
            }
            p.text(&ui.font, area.x + 2, area.bottom() + 8, label, if selected { theme::text() } else { theme::dim() });
            self.hits.add(area, Hit::Theme(light));
        }
        flow.note(p, ui, "Changes every Nook window, title bar and panel right away.");
        flow.gap(GAP);
        flow.heading(p, ui, "Wallpaper");
        let columns = ((flow.width + 14) / 190).max(1);
        let rows = (self.wallpapers.len() as i32 + columns - 1) / columns;
        let grid = flow.free(rows.max(1) * 168);
        for (i, wp) in self.wallpapers.iter().enumerate() {
            let area = Area::new(grid.x + (i as i32 % columns) * 190, grid.y + (i as i32 / columns) * 168, 176, 108);
            let selected = wp.path == self.current_wallpaper;
            match &wp.thumb {
                Some(img) => {
                    p.shadow(area, 10, 12, 150, 3);
                    p.image(img, area.x + 2, area.y + 2, 255);
                }
                None => p.rounded(area.inset(2), 8, theme::surface_2(), 255),
            }
            if selected {
                p.rounded_border(area, 10, theme::accent(), 255);
                p.rounded_border(area.inset(1), 9, theme::accent(), 255);
            } else if hover == Some(Hit::Wallpaper(i)) {
                p.rounded_border(area, 10, theme::faint(), 200);
            }
            p.text(&ui.font, area.x + 2, area.bottom() + 8, &ellipsize(&ui.font, &wp.title, 176), if selected { theme::text() } else { theme::dim() });
            p.text(&ui.small, area.x + 2, area.bottom() + 28, &ellipsize(&ui.small, &wp.credit, 176), theme::faint());
            self.hits.add(area, Hit::Wallpaper(i));
        }
        flow.note(p, ui, "Photographs from Wikimedia Commons, credits in /usr/share/wallpapers/CREDITS.");
        flow.note(p, ui, "Add your own PNG pictures to ~/Pictures to see them here.");
        flow.gap(GAP);
        flow.heading(p, ui, "Dock");
        let card = flow.card(p, 3);
        let control = row(p, ui, card, 0, 3, "Hide the dock automatically", "The dock slides away when a window covers it");
        for (area, value) in segmented(p, ui, control, &[("On", true), ("Off", false)], self.autohide, hover_autohide(hover)) {
            self.hits.add(area, Hit::Autohide(value));
        }
        let control = row(p, ui, card, 1, 3, "Icon size", "How big the dock icons are");
        let hovered = match hover {
            Some(Hit::DockSize(v)) => Some(v),
            _ => None,
        };
        let nearest = DOCK_SIZES.iter().min_by_key(|(_, v)| (v - self.dock_size).abs()).map(|(_, v)| *v).unwrap_or(48);
        for (area, value) in segmented(p, ui, control, &DOCK_SIZES, nearest, hovered) {
            self.hits.add(area, Hit::DockSize(value));
        }
        let control = row(p, ui, card, 2, 3, "Icon limit", "Extra icons move into the ••• menu");
        let hovered = match hover {
            Some(Hit::DockLimit(v)) => Some(v),
            _ => None,
        };
        let nearest = DOCK_LIMITS.iter().min_by_key(|(_, v)| (*v as i32 - self.dock_limit as i32).abs()).map(|(_, v)| *v).unwrap_or(12);
        for (area, value) in segmented(p, ui, control, &DOCK_LIMITS, nearest, hovered) {
            self.hits.add(area, Hit::DockLimit(value));
        }
    }

    fn draw_animations(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        flow.heading(p, ui, "Window effects");
        let card = flow.card(p, ANIMATIONS.len() as i32);
        for (i, effect) in ANIMATIONS.iter().enumerate() {
            let control = row(p, ui, card, i as i32, ANIMATIONS.len() as i32, effect.label, effect.description);
            let current = self.animation_choice(i);
            let options: Vec<(&str, usize)> = effect.choices.iter().enumerate().map(|(k, (_, label))| (*label, k)).collect();
            let selected = effect.choices.iter().position(|(name, _)| *name == current).unwrap_or(0);
            let hovered = match hover {
                Some(Hit::Animation(e, k)) if e == i => Some(k),
                _ => None,
            };
            for (area, k) in segmented(p, ui, control, &options, selected, hovered) {
                self.hits.add(area, Hit::Animation(i, k));
            }
        }
        flow.heading(p, ui, "Speed");
        let card = flow.card(p, 1);
        let control = row(p, ui, card, 0, 1, "Animation speed", "How long each effect takes");
        let options: Vec<(&str, u32)> = SPEEDS.iter().map(|(label, value)| (*label, *value)).collect();
        let hovered = match hover {
            Some(Hit::AnimationSpeed(v)) => Some(v),
            _ => None,
        };
        for (area, value) in segmented(p, ui, control, &options, self.anim_speed, hovered) {
            self.hits.add(area, Hit::AnimationSpeed(value));
        }
        flow.note(p, ui, "Every effect can be turned off on its own. Turning all of them off makes windows appear instantly.");
    }

    fn draw_display(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        let info = self.display.clone();
        flow.heading(p, ui, "Current mode");
        let card = flow.card(p, 3);
        value_row(p, ui, card, 0, 3, "Resolution", &format!("{}×{} at {} Hz", info.current.width, info.current.height, info.current.refresh));
        value_row(p, ui, card, 1, 3, "Driver", &info.backend);
        value_row(p, ui, card, 2, 3, "Output", &info.output);
        if !info.connectors.is_empty() {
            flow.heading(p, ui, "Outputs");
            let count = info.connectors.len() as i32;
            let card = flow.card(p, count);
            for (i, c) in info.connectors.iter().enumerate() {
                let label = format!("{}{}", c.name, if c.primary { "  (primary)" } else { "" });
                let detail = if !c.connected {
                    String::from("Disconnected")
                } else {
                    let monitor = if c.monitor.is_empty() { String::new() } else { format!("{} {} · ", c.vendor, c.monitor) };
                    let edid = if c.edid_len > 0 { format!(" · {} modes from EDID", c.modes.len()) } else { String::from(" · no EDID") };
                    format!("{}{}×{} at {} Hz{}", monitor, c.native.width, c.native.height, c.native.refresh, edid)
                };
                value_row(p, ui, card, i as i32, count, &label, &detail);
                if c.connected && !c.primary && info.can_set {
                    let slot = Area::new(card.right() - PAD - 96, card.y + i as i32 * ROW_H + (ROW_H - CONTROL_H) / 2, 96, CONTROL_H);
                    let hit = Hit::Mirror(c.id, true);
                    ui::button(p, ui, slot, "Mirror", Style::Secondary, hover == Some(hit), true);
                    self.hits.add(slot, hit);
                }
            }
        }
        if let Some(left) = info.revert_in {
            let banner = flow.block(p, 76);
            p.rounded_border(banner, 10, theme::warn(), 160);
            p.text(&ui.medium, banner.x + PAD, banner.y + 16, "Keep this display setting?", theme::text());
            p.text(&ui.small, banner.x + PAD, banner.y + 42, &format!("The previous mode comes back in {} seconds.", left), theme::dim());
            let keep = Area::new(banner.right() - PAD - 108, banner.y + 20, 108, CONTROL_H);
            let revert = Area::new(keep.x - 116, banner.y + 20, 108, CONTROL_H);
            ui::button(p, ui, keep, "Keep", Style::Primary, hover == Some(Hit::KeepDisplay), true);
            ui::button(p, ui, revert, "Revert", Style::Secondary, hover == Some(Hit::RevertDisplay), true);
            self.hits.add(keep, Hit::KeepDisplay);
            self.hits.add(revert, Hit::RevertDisplay);
        }
        if !info.can_set {
            flow.note(p, ui, "Changing the resolution is not supported for this graphics adapter or output.");
            flow.note(p, ui, "Supported: Intel gen4 (GMA X3100/X4500) built-in panels, Intel gen6-gen9 through the intel-display module, virtio-gpu, and Bochs VBE (QEMU).");
            return;
        }
        let mut sizes: Vec<(u32, u32, bool)> = Vec::new();
        for m in info.modes.iter() {
            if !sizes.iter().any(|s| s.0 == m.width && s.1 == m.height) {
                sizes.push((m.width, m.height, m.width == info.native.width && m.height == info.native.height));
            }
        }
        sizes.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
        flow.heading(p, ui, "Resolution");
        let columns = ((flow.width + 10) / 130).max(1);
        let cell_w = (flow.width + 10) / columns - 10;
        let rows = (sizes.len() as i32 + columns - 1) / columns;
        let grid = flow.free(rows.max(1) * 44);
        for (i, (sw, sh, native)) in sizes.iter().enumerate() {
            let cell = Area::new(grid.x + (i as i32 % columns) * (cell_w + 10), grid.y + (i as i32 / columns) * 44, cell_w, 38);
            let active = self.chosen.0 == *sw && self.chosen.1 == *sh;
            p.rounded(cell, 8, if active { theme::accent() } else if hover == Some(Hit::Resolution(*sw, *sh)) { theme::hover() } else { theme::surface() }, 255);
            let label = format!("{}×{}{}", sw, sh, if *native { "  ★" } else { "" });
            ui::centered_text(p, &ui.font, cell, &label, if active { theme::accent_text() } else { theme::text() });
            self.hits.add(cell, Hit::Resolution(*sw, *sh));
        }
        let mut rates: Vec<u32> = info.modes.iter().filter(|m| m.width == self.chosen.0 && m.height == self.chosen.1).map(|m| m.refresh).collect();
        rates.sort_by(|a, b| b.cmp(a));
        rates.dedup();
        flow.heading(p, ui, "Refresh rate");
        let strip = flow.free(CONTROL_H);
        for (i, rate) in rates.iter().enumerate() {
            let chip = Area::new(strip.x + i as i32 * 96, strip.y, 86, CONTROL_H);
            let active = self.chosen.2 == *rate;
            p.rounded(chip, 16, if active { theme::accent() } else if hover == Some(Hit::Refresh(*rate)) { theme::hover() } else { theme::surface() }, 255);
            ui::centered_text(p, &ui.font, chip, &format!("{} Hz", rate), if active { theme::accent_text() } else { theme::text() });
            self.hits.add(chip, Hit::Refresh(*rate));
        }
        let changed = (self.chosen.0, self.chosen.1, self.chosen.2) != (info.current.width, info.current.height, info.current.refresh);
        let strip = flow.free(CONTROL_H);
        let apply = Area::new(strip.right() - 130, strip.y, 130, CONTROL_H);
        ui::button(p, ui, apply, "Apply", Style::Primary, hover == Some(Hit::ApplyDisplay), changed && info.revert_in.is_none());
        if changed && info.revert_in.is_none() {
            self.hits.add(apply, Hit::ApplyDisplay);
        }
        flow.note(p, ui, "On a laptop panel a lower resolution is scaled up by the graphics chip, so the panel keeps running at its own mode. ★ marks the native one.");
    }

    fn draw_network(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        let status = self.net.clone();
        if !status.available {
            flow.note(p, ui, "The kernel has no network stack.");
            return;
        }
        flow.heading(p, ui, "Connection");
        let card = flow.card(p, 1);
        let control = row(p, ui, card, 0, 1, "Network", "Which adapter the system uses");
        let options = [("Ethernet", net::MODE_ETHERNET), ("Wi-Fi", net::MODE_WIFI), ("Off", net::MODE_OFF)];
        let selected = match status.mode.as_str() {
            "ethernet" => net::MODE_ETHERNET,
            "wifi" => net::MODE_WIFI,
            _ => net::MODE_OFF,
        };
        let hovered = match hover {
            Some(Hit::NetMode(m)) => Some(m),
            _ => None,
        };
        let wide = Area::new(control.x - 60, control.y, control.w + 60, control.h);
        for (area, value) in segmented(p, ui, wide, &options, selected, hovered) {
            self.hits.add(area, Hit::NetMode(value));
        }

        for iface in status.interfaces.iter() {
            flow.heading(p, ui, &format!("{} · {}", iface.name, iface.driver));
            let card = flow.card(p, 5);
            let state = if iface.link && !iface.address.is_empty() {
                format!("connected · {}", iface.address)
            } else if iface.link {
                format!("{} · no address yet", iface.state)
            } else {
                iface.state.clone()
            };
            value_row(p, ui, card, 0, 5, "Status", &state);
            value_row(p, ui, card, 1, 5, "MAC", &iface.mac);
            value_row(p, ui, card, 2, 5, "Gateway", if iface.gateway.is_empty() { "—" } else { &iface.gateway });
            value_row(p, ui, card, 3, 5, "DNS", if iface.dns.is_empty() { "—" } else { &iface.dns });
            value_row(p, ui, card, 4, 5, "Packets", &format!("{} received, {} sent", iface.rx_packets, iface.tx_packets));
            let dot = Area::new(card.right() - PAD - 10, card.y + ROW_H / 2 - 5, 10, 10);
            p.rounded(dot, 5, if iface.link && !iface.address.is_empty() { theme::success() } else if iface.link { theme::warn() } else { theme::faint() }, 255);
        }
        if status.interfaces.is_empty() {
            flow.note(p, ui, "No network adapters were found.");
        }

        if status.mode == "wifi" && status.has_kind("wifi") {
            flow.heading(p, ui, "Available networks");
            let strip = flow.free(0);
            let scan = Area::new(strip.right() - 100, strip.y - 30, 100, 28);
            ui::button(p, ui, scan, "Scan", Style::Secondary, hover == Some(Hit::WifiScan), true);
            self.hits.add(scan, Hit::WifiScan);
            if self.networks.is_empty() {
                flow.note(p, ui, "No networks found yet.");
            }
            for (i, network) in self.networks.iter().enumerate() {
                let selected = self.wifi_selected == Some(i);
                let needs_password = selected && !network.connected && network.secured();
                let card = flow.block(p, ROW_H + if needs_password { 52 } else { 0 });
                let area = Area::new(card.x, card.y, card.w, ROW_H);
                if selected {
                    p.rounded_border(card, 10, theme::accent(), 200);
                }
                let bars = match network.signal {
                    0..=25 => "ui/wifi-0",
                    26..=50 => "ui/wifi-1",
                    51..=75 => "ui/wifi-2",
                    _ => "ui/wifi-3",
                };
                ui::symbol(p, ui, bars, area.x + PAD, area.y + 22, if network.connected { theme::success() } else { theme::text() });
                p.text(&ui.medium, area.x + PAD + 28, area.y + 12, &ellipsize(&ui.medium, &network.ssid, area.w - 200), theme::text());
                let detail = if network.connected {
                    String::from("Connected · click to disconnect")
                } else {
                    format!("{} · channel {} · {}%", network.security.to_uppercase(), network.channel, network.signal)
                };
                p.text(&ui.small, area.x + PAD + 28, area.y + 34, &detail, if network.connected { theme::success() } else { theme::faint() });
                if self.saved_networks.iter().any(|s| s.ssid == network.ssid) {
                    ui::symbol(p, ui, "ui/check", area.right() - PAD - 20, area.y + 22, theme::dim());
                }
                self.hits.add(area, Hit::WifiRow(i));
                if needs_password {
                    let field = Area::new(card.x + PAD + 28, area.bottom() + 4, (card.w / 2).clamp(180, 300), CONTROL_H);
                    ui::text_field(p, ui, field, &mut self.fields[FIELD_WIFI], self.focus == Some(FIELD_WIFI), "Wi-Fi password");
                    self.hits.add(field, Hit::Field(FIELD_WIFI));
                    let connect = Area::new(field.right() + 12, field.y, 110, CONTROL_H);
                    ui::button(p, ui, connect, "Connect", Style::Primary, hover == Some(Hit::WifiConnect), true);
                    self.hits.add(connect, Hit::WifiConnect);
                }
            }

            flow.heading(p, ui, "Saved networks");
            if self.saved_networks.is_empty() {
                flow.note(p, ui, "Networks you connect to are saved here and joined again automatically.");
            } else {
                let count = self.saved_networks.len() as i32;
                let card = flow.card(p, count);
                let entries: Vec<(String, bool)> = self.saved_networks.iter().map(|s| (s.ssid.clone(), s.automatic)).collect();
                for (i, (ssid, automatic)) in entries.iter().enumerate() {
                    let control = row(p, ui, card, i as i32, count, ssid, if *automatic { "Joined automatically when in range" } else { "Saved, joined only by hand" });
                    let forget = Area::new(control.right() - 90, control.y, 90, control.h);
                    let toggle = Area::new(forget.x - 60, control.y + 3, 48, 26);
                    ui::switch(p, toggle, *automatic, hover == Some(Hit::WifiAutojoin(i)));
                    ui::button(p, ui, forget, "Forget", Style::Secondary, hover == Some(Hit::WifiForget(i)), true);
                    self.hits.add(toggle, Hit::WifiAutojoin(i));
                    self.hits.add(forget, Hit::WifiForget(i));
                }
                flow.note(p, ui, "Passwords are kept in /etc/hamix/networks.conf, readable only by root.");
            }
        }

        if status.mode != "off" {
            flow.gap(GAP);
            flow.heading(p, ui, "IP configuration");
            let card = flow.card(p, 1);
            let control = row(p, ui, card, 0, 1, "Addressing", "Automatic uses the router's DHCP server");
            let hovered = match hover {
                Some(Hit::Dhcp(v)) => Some(v),
                _ => None,
            };
            for (area, value) in segmented(p, ui, control, &[("Automatic", true), ("Manual", false)], self.dhcp, hovered) {
                self.hits.add(area, Hit::Dhcp(value));
            }
            if !self.dhcp {
                let card = flow.card(p, 3);
                for (i, (index, label, placeholder)) in [(FIELD_ADDRESS, "Address", "192.168.1.20/24"), (FIELD_GATEWAY, "Gateway", "192.168.1.1"), (FIELD_DNS, "DNS", "1.1.1.1")].iter().enumerate() {
                    let control = row(p, ui, card, i as i32, 3, label, "");
                    ui::text_field(p, ui, control, &mut self.fields[*index], self.focus == Some(*index), placeholder);
                    self.hits.add(control, Hit::Field(*index));
                }
            }
            self.admin_block(p, flow, Hit::ApplyNetwork, "Apply");
        }
    }

    fn draw_sound(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        let info = audio::info();
        let volume = audio::volume();
        flow.heading(p, ui, "Device");
        let card = flow.card(p, 4);
        value_row(p, ui, card, 0, 4, "Device", if info.available { &info.device } else { "No sound device was found" });
        let driver_line = if info.available { format!("{} · {} Hz", info.driver, info.rate) } else { String::from("—") };
        value_row(p, ui, card, 1, 4, "Driver", &driver_line);
        value_row(p, ui, card, 2, 4, "Outputs", if info.outputs.is_empty() { "—" } else { &info.outputs });
        let state = if !info.available {
            String::from("unavailable")
        } else if info.running {
            format!("playing, {} stream{}", info.streams, if info.streams == 1 { "" } else { "s" })
        } else {
            String::from("idle")
        };
        value_row(p, ui, card, 3, 4, "State", &state);

        let Some(v) = volume else {
            self.slider = Area::default();
            return;
        };
        flow.heading(p, ui, "Output volume");
        let card = flow.card(p, 1);
        let control = row(p, ui, card, 0, 1, "Volume", "Laptop volume keys change this everywhere");
        let mute = Area::new(control.x - 46, control.y, 40, control.h);
        ui::icon_button(
            p,
            ui,
            mute,
            if v.muted || v.level == 0 { "ui/volume-mute" } else if v.level < 34 { "ui/volume-1" } else if v.level < 67 { "ui/volume-2" } else { "ui/volume-3" },
            hover == Some(Hit::Mute),
            v.muted,
        );
        self.hits.add(mute, Hit::Mute);
        let label = if v.muted { String::from("Muted") } else { format!("{}%", v.level) };
        let label_w = 52;
        let slider = Area::new(control.x, control.y, control.w - label_w, control.h);
        self.slider = slider;
        let track = Area::new(slider.x, slider.y + slider.h / 2 - 3, slider.w, 6);
        p.rounded(track, 3, theme::surface_2(), 255);
        let filled = (track.w as i64 * v.level as i64 / 100) as i32;
        let color = if v.muted { theme::faint() } else { theme::accent() };
        if filled > 0 {
            p.rounded(Area::new(track.x, track.y, filled.max(6), 6), 3, color, 255);
        }
        p.circle((track.x + filled) as f32, track.y as f32 + 3.0, if hover == Some(Hit::VolumeSlider) { 10.0 } else { 9.0 }, 0xffffff, 255);
        self.hits.add(slider.expand(6), Hit::VolumeSlider);
        p.text(&ui.medium, slider.right() + 10, slider.y + (slider.h - ui.medium.height()) / 2, &label, theme::text());
        let strip = flow.free(CONTROL_H);
        let test = Area::new(strip.x, strip.y, 160, CONTROL_H);
        ui::button(p, ui, test, "Play test sound", Style::Secondary, hover == Some(Hit::TestSound), info.available);
        self.hits.add(test, Hit::TestSound);
        flow.note(p, ui, "Supported hardware: Intel High Definition Audio and AC'97.");
    }

    fn play_test_sound(&mut self) {
        let rate = 48000u32;
        let Ok(stream) = audio::Stream::open(rate, 2) else {
            self.message = (String::from("The sound device is busy or missing"), theme::warn());
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
        self.message = (String::from("Playing a test sound"), theme::success());
    }

    fn draw_session(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        flow.heading(p, ui, "After login");
        let card = flow.card(p, 1);
        let control = row(p, ui, card, 0, 1, "Start Nook automatically", "When off you get the text shell and can type startx");
        let toggle = Area::new(control.right() - 50, control.y + 3, 50, 28);
        ui::switch(p, toggle, self.autostart, hover == Some(Hit::Autostart));
        self.hits.add(toggle, Hit::Autostart);
        let conf = login_conf();
        let get = |k: &str| conf.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
        flow.heading(p, ui, "Login shell");
        let card = flow.card(p, 2);
        value_row(p, ui, card, 0, 2, "Shell", &format!("{} {}", get("shell"), get("shell_args")));
        value_row(p, ui, card, 1, 2, "Configured in", "/etc/hamix/login.conf");
        if self.autostart != self.autostart_saved {
            self.admin_block(p, flow, Hit::ApplySession, "Apply");
        }
    }

    fn draw_account(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        let user = ui::current_user();
        flow.heading(p, ui, "Account");
        let card = flow.block(p, 84);
        p.circle(card.x as f32 + 46.0, card.y as f32 + 42.0, 22.0, theme::accent(), 255);
        let initial: String = user.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        ui::centered_text(p, &ui.title, Area::new(card.x + 24, card.y + 22, 44, 40), &initial, theme::accent_text());
        p.text(&ui.medium, card.x + 84, card.y + 24, &user, theme::text());
        let role = if users::is_sudoer(&user) || sys::geteuid() == 0 { "Administrator" } else { "Standard account" };
        p.text(&ui.small, card.x + 84, card.y + 48, &format!("{} · home {}", role, ui::home_dir()), theme::dim());
        flow.heading(p, ui, "Change password");
        let card = flow.card(p, 3);
        for (i, label) in ["Current password", "New password", "Repeat new password"].iter().enumerate() {
            let control = row(p, ui, card, i as i32, 3, label, "");
            ui::text_field(p, ui, control, &mut self.fields[i], self.focus == Some(i), "");
            self.hits.add(control, Hit::Field(i));
        }
        let strip = flow.free(CONTROL_H);
        let button = Area::new(strip.right() - 180, strip.y, 180, CONTROL_H);
        ui::button(p, ui, button, "Change password", Style::Primary, hover == Some(Hit::ChangePassword), true);
        self.hits.add(button, Hit::ChangePassword);
    }

    fn draw_system(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let text = fs::read_to_string("/proc/hxinit").unwrap_or_default();
        let units: Vec<Vec<String>> = text.lines().map(|l| l.split('\t').map(String::from).collect()).filter(|f: &Vec<String>| f.len() >= 5).collect();
        let failed = units.iter().filter(|u| u[1] == "fail").count();
        let warned = units.iter().filter(|u| u[1] == "warn").count();
        flow.heading(p, ui, &format!("{} units · {} warning{} · {} failed", units.len(), warned, if warned == 1 { "" } else { "s" }, failed));
        let row_h = 34;
        let card = flow.block(p, units.len() as i32 * row_h + 16);
        for (i, unit) in units.iter().enumerate() {
            let line = Area::new(card.x + 8, card.y + 8 + i as i32 * row_h, card.w - 16, row_h - 2);
            let (label, color) = match unit[1].as_str() {
                "ok" => ("OK", theme::success()),
                "warn" => ("WARN", theme::warn()),
                "fail" => ("FAILED", theme::danger()),
                _ => ("SKIP", theme::faint()),
            };
            let badge = Area::new(line.x + 6, line.y + 5, 62, line.h - 10);
            p.rounded(badge, 6, color, 50);
            ui::centered_text(p, &ui.small, badge, label, color);
            ui::text_in(p, &ui.medium, line.x + 80, line, &unit[3], theme::text());
            let ms = format!("{} ms", unit[2]);
            let mw = ui.small.measure(&ms);
            p.text(&ui.small, line.right() - mw - 8, line.y + 11, &ms, theme::faint());
            ui::text_in(p, &ui.font, line.x + 280, Area::new(line.x, line.y, line.w - mw - 300, line.h), &unit[4], theme::dim());
        }
    }

    fn draw_about(&mut self, p: &mut Painter, flow: &mut Flow) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let banner = flow.free(104);
        let wordmark = if theme::is_light() { "about-logo-light" } else { "about-logo-dark" };
        let mut text_x = banner.x;
        if let Some(logo) = ui.icon(wordmark) {
            p.image(logo, banner.x, banner.y, 255);
            text_x = banner.x + logo.w + 24;
        }
        p.text(&ui.title, text_x, banner.y + 30, "HamixOS 0.6.1", theme::text());
        p.text(&ui.font, text_x, banner.y + 60, "Rust kernel · hext · Nook · hsh · smoltcp", theme::dim());
        let info = sys::sysinfo();
        let cpu = fs::read_to_string("/proc/cpuinfo")
            .and_then(|t| t.lines().find(|l| l.starts_with("model name")).map(|l| String::from(l.split(':').nth(1).unwrap_or("").trim())))
            .unwrap_or_default();
        let cores = sys::cpu_stats().len();
        let gpu = fs::read_to_string("/proc/gpu").and_then(|t| t.lines().next().map(|l| String::from(l.split('\t').nth(1).unwrap_or("")))).unwrap_or_default();
        let root = fs::read_to_string("/proc/mounts").and_then(|t| t.lines().next().map(|l| String::from(l.split('\t').next().unwrap_or("")))).unwrap_or_default();
        let disks: Vec<String> = sys::disk_listing()
            .lines()
            .filter(|l| l.starts_with("disk"))
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                format!("{} {} {}", f.get(1).unwrap_or(&""), ui::human_size(f.get(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) * 512), f.get(4).unwrap_or(&""))
            })
            .collect();
        let rows = [
            ("Processor", format!("{} ({} cores)", cpu, cores)),
            ("Graphics", gpu),
            ("Memory", format!("{} total, {} used", ui::human_size(info.mem_total), ui::human_size(info.mem_used()))),
            ("Root filesystem", if root == "live" { String::from("live image in RAM") } else { root }),
            ("Disks", if disks.is_empty() { String::from("none detected") } else { disks.join(", ") }),
            ("Uptime", format!("{} minutes", info.uptime_ms / 60000)),
        ];
        flow.heading(p, ui, "This computer");
        let card = flow.card(p, rows.len() as i32);
        for (i, (k, v)) in rows.iter().enumerate() {
            value_row(p, ui, card, i as i32, rows.len() as i32, k, v);
        }
    }

    fn press(&mut self, x: i32, y: i32) {
        self.focus = None;
        match self.hits.at(x, y) {
            Some(Hit::Page(i)) => {
                self.page = i;
                self.scroll = 0;
                self.message.0.clear();
                match i {
                    PAGE_DISPLAY => self.refresh_display(),
                    PAGE_NETWORK => self.refresh_network(false),
                    _ => {}
                }
            }
            Some(Hit::Wallpaper(i)) => {
                let path = self.wallpapers[i].path.clone();
                if ui::write_nook_config("wallpaper", &path) {
                    self.current_wallpaper = path;
                    hxclient::reload_desktop();
                    self.message = (String::from("Wallpaper changed"), theme::success());
                } else {
                    self.message = (String::from("Cannot save the setting"), theme::danger());
                }
            }
            Some(Hit::Theme(light)) => {
                theme::set_light(light);
                if ui::write_nook_config("theme", if light { "light" } else { "dark" }) {
                    hxclient::reload_desktop();
                    self.message = (String::from(if light { "Light style on" } else { "Dark style on" }), theme::success());
                } else {
                    self.message = (String::from("Cannot save the setting"), theme::danger());
                }
            }
            Some(Hit::Autohide(value)) => {
                self.autohide = value;
                ui::write_nook_config("dock_autohide", if value { "yes" } else { "no" });
                hxclient::reload_desktop();
            }
            Some(Hit::DockSize(value)) => {
                self.dock_size = value;
                ui::write_nook_config("dock_size", &format!("{}", value));
                hxclient::reload_desktop();
            }
            Some(Hit::DockLimit(value)) => {
                self.dock_limit = value;
                ui::write_nook_config("dock_limit", &format!("{}", value));
                hxclient::reload_desktop();
            }
            Some(Hit::Animation(effect, choice)) => {
                let name = ANIMATIONS[effect].choices[choice].0;
                self.animations[effect] = String::from(name);
                ui::write_nook_config(ANIMATIONS[effect].key, name);
                hxclient::reload_desktop();
                self.message = (format!("{}: {}", ANIMATIONS[effect].label, ANIMATIONS[effect].choices[choice].1), theme::success());
            }
            Some(Hit::AnimationSpeed(value)) => {
                self.anim_speed = value;
                ui::write_nook_config("anim_speed", &format!("{}", value));
                hxclient::reload_desktop();
            }
            Some(Hit::Autostart) => {
                self.autostart = !self.autostart;
                if self.autostart != self.autostart_saved && sys::geteuid() != 0 {
                    self.focus = Some(FIELD_ADMIN);
                }
            }
            Some(Hit::Field(i)) => self.focus = Some(i),
            Some(Hit::ApplySession) => self.apply_session(),
            Some(Hit::ChangePassword) => self.change_password(),
            Some(Hit::Resolution(w, h)) => {
                let rates: Vec<u32> = self.display.modes.iter().filter(|m| m.width == w && m.height == h).map(|m| m.refresh).collect();
                let refresh = if rates.contains(&self.chosen.2) { self.chosen.2 } else { rates.iter().copied().max().unwrap_or(60) };
                self.chosen = (w, h, refresh);
            }
            Some(Hit::Refresh(r)) => self.chosen.2 = r,
            Some(Hit::ApplyDisplay) => self.apply_display(),
            Some(Hit::Mirror(id, on)) => {
                let (w, h) = if on { (self.display.current.width, self.display.current.height) } else { (0, 0) };
                display::set_output(id, w, h);
                self.refresh_display();
            }
            Some(Hit::KeepDisplay) => {
                display::confirm();
                self.refresh_display();
                self.message = (String::from("Display setting saved"), theme::success());
            }
            Some(Hit::RevertDisplay) => {
                display::revert();
                hxclient::display_changed();
                self.chosen = (0, 0, 0);
                self.refresh_display();
                self.message = (String::from("The previous display mode was restored"), theme::dim());
            }
            Some(Hit::NetMode(mode)) => {
                let r = net::set_mode(mode);
                if r < 0 {
                    self.message = (format!("Cannot switch: {}", sys::error_name(r)), theme::danger());
                } else {
                    self.message.0.clear();
                }
                self.wifi_selected = None;
                self.refresh_network(mode == net::MODE_WIFI);
            }
            Some(Hit::WifiRow(i)) => {
                if self.networks.get(i).map(|n| n.connected).unwrap_or(false) {
                    net::wifi_disconnect();
                    self.refresh_network(false);
                    return;
                }
                self.wifi_selected = if self.wifi_selected == Some(i) { None } else { Some(i) };
                if self.networks.get(i).map(|n| n.secured()).unwrap_or(false) {
                    self.focus = Some(FIELD_WIFI);
                }
            }
            Some(Hit::WifiConnect) => self.connect_wifi(),
            Some(Hit::WifiScan) => self.refresh_network(true),
            Some(Hit::WifiForget(i)) => {
                if let Some(entry) = self.saved_networks.get(i).cloned() {
                    let r = net::wifi_forget(&entry.ssid);
                    self.message = if r < 0 {
                        (format!("Cannot forget {}: {}", entry.ssid, sys::error_name(r)), theme::danger())
                    } else {
                        (format!("{} was forgotten", entry.ssid), theme::success())
                    };
                    self.saved_networks = net::wifi_known();
                }
            }
            Some(Hit::WifiAutojoin(i)) => {
                if let Some(entry) = self.saved_networks.get(i).cloned() {
                    let r = net::wifi_autojoin(&entry.ssid, !entry.automatic);
                    if r < 0 {
                        self.message = (format!("Cannot change {}: {}", entry.ssid, sys::error_name(r)), theme::danger());
                    }
                    self.saved_networks = net::wifi_known();
                }
            }
            Some(Hit::Dhcp(on)) => self.dhcp = on,
            Some(Hit::ApplyNetwork) => self.apply_network(),
            Some(Hit::Mute) => {
                if let Some(v) = audio::toggle_mute() {
                    ui::write_nook_config("audio_muted", if v.muted { "yes" } else { "no" });
                }
            }
            Some(Hit::VolumeSlider) => {
                if self.slider.w > 0 {
                    let level = ((x - self.slider.x) as i64 * 100 / self.slider.w.max(1) as i64).clamp(0, 100) as u32;
                    audio::set_volume(level);
                    if level > 0 {
                        audio::set_muted(false);
                    }
                    ui::write_nook_config("audio_volume", &format!("{}", level));
                }
            }
            Some(Hit::TestSound) => self.play_test_sound(),
            None => {}
        }
    }

    fn key(&mut self, code: i32) -> bool {
        let Some(i) = self.focus else {
            return false;
        };
        match self.fields[i].key(code) {
            FieldAction::Submit => match i {
                FIELD_ADMIN if self.page == PAGE_SESSION => self.apply_session(),
                FIELD_ADMIN => self.apply_network(),
                FIELD_REPEAT => self.change_password(),
                FIELD_WIFI => self.connect_wifi(),
                FIELD_CURRENT | FIELD_NEW => self.focus = Some(i + 1),
                FIELD_ADDRESS | FIELD_GATEWAY => self.focus = Some(i + 1),
                FIELD_DNS => self.apply_network(),
                _ => {}
            },
            FieldAction::Next => {
                self.focus = Some(match i {
                    FIELD_CURRENT | FIELD_NEW => i + 1,
                    FIELD_ADDRESS | FIELD_GATEWAY => i + 1,
                    _ => i,
                })
            }
            _ => {}
        }
        true
    }
}

const DOCK_SIZES: [(&str, i32); 4] = [("Small", 36), ("Medium", 48), ("Large", 64), ("Huge", 80)];
const DOCK_LIMITS: [(&str, u32); 5] = [("6", 6), ("8", 8), ("12", 12), ("16", 16), ("24", 24)];

fn hover_autohide(hover: Option<Hit>) -> Option<bool> {
    match hover {
        Some(Hit::Autohide(v)) => Some(v),
        _ => None,
    }
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&[
        "ui/wallpaper", "ui/session", "ui/user", "ui/info", "logo", "about-logo-dark", "about-logo-light", "ui/display", "ui/network", "ui/system", "ui/apps", "ui/check", "ui/wifi-0", "ui/wifi-1", "ui/wifi-2", "ui/wifi-3", "ui/ethernet", "ui/lock", "ui/sound",
        "ui/volume-mute", "ui/volume-1", "ui/volume-2", "ui/volume-3",
    ]);
    let window = match Window::open("Settings", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxsettings: {}", e);
            return 1;
        }
    };
    let autostart = login_conf().iter().any(|(k, v)| k == "autostart_desktop" && matches!(v.as_str(), "yes" | "true" | "on" | "1"));
    let current_wallpaper = config_value("wallpaper").unwrap_or_else(|| String::from("/usr/share/wallpapers/dusk.png"));
    let autohide = config_value("dock_autohide").map(|v| v != "no").unwrap_or(true);
    let dock_size = config_value("dock_size").and_then(|v| v.trim().parse().ok()).unwrap_or(48);
    let dock_limit = config_value("dock_limit").and_then(|v| v.trim().parse().ok()).unwrap_or(12);
    let animations: Vec<String> = ANIMATIONS
        .iter()
        .map(|effect| {
            config_value(effect.key)
                .filter(|v| effect.choices.iter().any(|(name, _)| *name == v.trim()))
                .map(|v| String::from(v.trim()))
                .unwrap_or_else(|| String::from(effect.default))
        })
        .collect();
    let anim_speed = config_value("anim_speed").and_then(|v| v.trim().parse().ok()).unwrap_or(100u32);
    let page = match env::args().get(1).map(|s| s.as_str()) {
        Some("animations") => PAGE_ANIMATIONS,
        Some("display") => PAGE_DISPLAY,
        Some("network") => PAGE_NETWORK,
        Some("sound") => PAGE_SOUND,
        Some("session") => PAGE_SESSION,
        Some("account") => PAGE_ACCOUNT,
        Some("system") => PAGE_SYSTEM,
        Some("about") => PAGE_ABOUT,
        _ => PAGE_APPEARANCE,
    };
    let mut fields: Vec<TextField> = (0..5).map(|_| TextField::secret()).collect();
    fields.extend((0..3).map(|_| TextField::default()));
    let mut app = App {
        window,
        ui,
        page,
        wallpapers: Vec::new(),
        current_wallpaper,
        autohide,
        dock_size,
        dock_limit,
        animations,
        anim_speed,
        autostart,
        autostart_saved: autostart,
        fields,
        focus: None,
        message: (String::new(), theme::dim()),
        hits: Hits::new(),
        hover: None,
        display: display::info(),
        chosen: (0, 0, 0),
        net: net::status(),
        networks: Vec::new(),
        saved_networks: Vec::new(),
        wifi_selected: None,
        dhcp: true,
        last_refresh: 0,
        scroll: 0,
        content_h: 0,
        view_h: 0,
        slider: Area::default(),
    };
    app.window.set_icon("settings");
    app.window.set_min_size(640, 460);
    app.refresh_display();
    app.refresh_network(false);
    app.load_wallpapers();
    app.draw();
    loop {
        let pending_thumbs = app.wallpapers.iter().any(|w| w.thumb.is_none());
        let live = matches!(app.page, PAGE_DISPLAY | PAGE_NETWORK | PAGE_SYSTEM | PAGE_SOUND);
        let timeout = if pending_thumbs { 0 } else if live { 1000 } else { -1 };
        let event = app.window.wait_event(timeout);
        let mut redraw = false;
        match event {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Resize { .. }) | Some(Event::Theme { .. }) => redraw = true,
            Some(Event::Mouse { x, y, kind, wheel, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    let hover = if kind == MOUSE_LEAVE { None } else { app.hits.at(x, y) };
                    if hover != app.hover {
                        app.hover = hover;
                        redraw = true;
                    }
                }
                MOUSE_WHEEL => {
                    let max_scroll = (app.content_h - app.view_h).max(0);
                    let next = (app.scroll + wheel * 48).clamp(0, max_scroll);
                    if next != app.scroll {
                        app.scroll = next;
                        redraw = true;
                    }
                }
                MOUSE_PRESS => {
                    app.press(x, y);
                    redraw = true;
                }
                _ => {}
            },
            Some(Event::Key { code, .. }) => redraw = app.key(code),
            None => {
                if pending_thumbs {
                    redraw = app.load_thumbnail();
                } else if live {
                    match app.page {
                        PAGE_DISPLAY => app.refresh_display(),
                        PAGE_NETWORK if sys::uptime_ms().saturating_sub(app.last_refresh) >= 2000 => app.refresh_network(false),
                        _ => {}
                    }
                    redraw = true;
                }
            }
            _ => {}
        }
        if redraw {
            app.draw();
        }
    }
}

entry!(main);
