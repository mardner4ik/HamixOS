use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{fs, sys};
use vellum::gfx::ellipsize;
use vellum::{Area, Font, Image, Painter};

pub mod theme {
    use core::sync::atomic::{AtomicBool, Ordering};

    pub struct Palette {
        pub bg: u32,
        pub sidebar: u32,
        pub surface: u32,
        pub surface_2: u32,
        pub hover: u32,
        pub pressed: u32,
        pub border: u32,
        pub text: u32,
        pub dim: u32,
        pub faint: u32,
        pub accent: u32,
        pub accent_hover: u32,
        pub accent_text: u32,
        pub selection: u32,
        pub on_selection: u32,
        pub danger: u32,
        pub success: u32,
        pub warn: u32,
        pub title_bar: u32,
        pub title_bar_inactive: u32,
        pub title_line: u32,
        pub gutter: u32,
        pub current_line: u32,
        pub popup: u32,
        pub shadow_alpha: u32,
        pub edge: u32,
        pub edge_alpha: u32,
        pub panel: u32,
        pub panel_alpha: u32,
        pub panel_text: u32,
        pub panel_dim: u32,
        pub overlay: u32,
        pub dock: u32,
        pub dock_alpha: u32,
        pub dock_line: u32,
        pub dock_dot: u32,
        pub osd: u32,
    }

    pub const DARK: Palette = Palette {
        bg: 0x1b1e25,
        sidebar: 0x171a20,
        surface: 0x23272f,
        surface_2: 0x2a2f39,
        hover: 0x313743,
        pressed: 0x3a4150,
        border: 0x363c48,
        text: 0xe8eaf0,
        dim: 0xa0a7b4,
        faint: 0x6c7383,
        accent: 0x5b8cff,
        accent_hover: 0x7aa2ff,
        accent_text: 0xffffff,
        selection: 0x2c4677,
        on_selection: 0xffffff,
        danger: 0xe5534b,
        success: 0x3fb950,
        warn: 0xe3a33c,
        title_bar: 0x23272f,
        title_bar_inactive: 0x1f2229,
        title_line: 0x15171c,
        gutter: 0x181b21,
        current_line: 0x20242c,
        popup: 0x2a2f39,
        shadow_alpha: 200,
        edge: 0xffffff,
        edge_alpha: 22,
        panel: 0x0c0e12,
        panel_alpha: 200,
        panel_text: 0xffffff,
        panel_dim: 0x9aa1ad,
        overlay: 0xffffff,
        dock: 0x12141a,
        dock_alpha: 218,
        dock_line: 0x4a505c,
        dock_dot: 0xdfe3ea,
        osd: 0x1c1f26,
    };

    pub const LIGHT: Palette = Palette {
        bg: 0xf6f7f9,
        sidebar: 0xeceef2,
        surface: 0xffffff,
        surface_2: 0xf0f2f5,
        hover: 0xe3e7ed,
        pressed: 0xd5dbe4,
        border: 0xd6dae1,
        text: 0x1d2129,
        dim: 0x565e6d,
        faint: 0x8b92a1,
        accent: 0x2f6fed,
        accent_hover: 0x4a82f0,
        accent_text: 0xffffff,
        selection: 0xcfe0ff,
        on_selection: 0x0f2446,
        danger: 0xd93f37,
        success: 0x23913a,
        warn: 0xbd760b,
        title_bar: 0xffffff,
        title_bar_inactive: 0xf0f1f4,
        title_line: 0xdcdfe5,
        gutter: 0xf0f1f4,
        current_line: 0xeef3fc,
        popup: 0xffffff,
        shadow_alpha: 120,
        edge: 0x000000,
        edge_alpha: 30,
        panel: 0xf5f6f8,
        panel_alpha: 218,
        panel_text: 0x1d2129,
        panel_dim: 0x656c7a,
        overlay: 0x000000,
        dock: 0xf7f8fa,
        dock_alpha: 212,
        dock_line: 0xc6cbd4,
        dock_dot: 0x3d4350,
        osd: 0xffffff,
    };

    static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

    pub fn palette() -> &'static Palette {
        if LIGHT_MODE.load(Ordering::Relaxed) { &LIGHT } else { &DARK }
    }

    pub fn is_light() -> bool {
        LIGHT_MODE.load(Ordering::Relaxed)
    }

    pub fn set_light(light: bool) -> bool {
        LIGHT_MODE.swap(light, Ordering::Relaxed) != light
    }

    pub fn name() -> &'static str {
        if is_light() { "light" } else { "dark" }
    }

    pub fn load() -> bool {
        let light = super::read_nook_config().iter().any(|(k, v)| k == "theme" && v == "light");
        set_light(light)
    }

    pub fn bg() -> u32 {
        palette().bg
    }
    pub fn sidebar() -> u32 {
        palette().sidebar
    }
    pub fn surface() -> u32 {
        palette().surface
    }
    pub fn surface_2() -> u32 {
        palette().surface_2
    }
    pub fn hover() -> u32 {
        palette().hover
    }
    pub fn pressed() -> u32 {
        palette().pressed
    }
    pub fn border() -> u32 {
        palette().border
    }
    pub fn text() -> u32 {
        palette().text
    }
    pub fn dim() -> u32 {
        palette().dim
    }
    pub fn faint() -> u32 {
        palette().faint
    }
    pub fn accent() -> u32 {
        palette().accent
    }
    pub fn accent_hover() -> u32 {
        palette().accent_hover
    }
    pub fn accent_text() -> u32 {
        palette().accent_text
    }
    pub fn selection() -> u32 {
        palette().selection
    }
    pub fn on_selection() -> u32 {
        palette().on_selection
    }
    pub fn danger() -> u32 {
        palette().danger
    }
    pub fn success() -> u32 {
        palette().success
    }
    pub fn warn() -> u32 {
        palette().warn
    }
    pub fn gutter() -> u32 {
        palette().gutter
    }
    pub fn current_line() -> u32 {
        palette().current_line
    }
    pub fn popup() -> u32 {
        palette().popup
    }

    pub const RADIUS: i32 = 7;
}

pub const ASSETS: &str = "/usr/share/nook";

pub struct Ui {
    pub font: Font,
    pub medium: Font,
    pub small: Font,
    pub title: Font,
    pub big: Font,
    pub mono: Font,
    icons: BTreeMap<String, Option<Image>>,
}

fn load_font(name: &str) -> Font {
    let path = format!("{}/fonts/{}.hfnt", ASSETS, name);
    match fs::read(&path).and_then(|b| Font::parse(&b)) {
        Some(font) => font,
        None => {
            hamix_std::eprintln!("nook: missing font {}", path);
            sys::exit(1)
        }
    }
}

impl Ui {
    pub fn load() -> Ui {
        theme::load();
        Ui {
            font: load_font("sans-13"),
            medium: load_font("sans-medium-13"),
            small: load_font("sans-11"),
            title: load_font("sans-medium-18"),
            big: load_font("sans-light-30"),
            mono: load_font("mono-14"),
            icons: BTreeMap::new(),
        }
    }

    pub fn load_icon(&mut self, name: &str) -> bool {
        if !self.icons.contains_key(name) {
            let path = if name.starts_with('/') { String::from(name) } else { format!("{}/icons/{}.png", ASSETS, name) };
            let image = fs::read(&path).and_then(|b| Image::from_png(&b));
            self.icons.insert(String::from(name), image);
        }
        self.icons.get(name).map(|i| i.is_some()).unwrap_or(false)
    }

    pub fn insert_icon(&mut self, name: &str, image: Image) {
        self.icons.insert(String::from(name), Some(image));
    }

    pub fn icon(&self, name: &str) -> Option<&Image> {
        self.icons.get(name).and_then(|i| i.as_ref())
    }

    pub fn preload(&mut self, names: &[&str]) {
        for name in names {
            self.load_icon(name);
        }
    }
}

pub struct Hits<T: Copy + PartialEq> {
    list: Vec<(Area, T)>,
}

impl<T: Copy + PartialEq> Hits<T> {
    pub fn new() -> Self {
        Hits { list: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn add(&mut self, area: Area, value: T) {
        self.list.push((area, value));
    }

    pub fn at(&self, x: i32, y: i32) -> Option<T> {
        self.list.iter().rev().find(|(a, _)| a.contains(x, y)).map(|(_, v)| *v)
    }

    pub fn area_of(&self, value: T) -> Option<Area> {
        self.list.iter().rev().find(|(_, v)| *v == value).map(|(a, _)| *a)
    }
}

pub fn open_with_default(path: &str) -> bool {
    let lower = path.to_lowercase();
    let is_dir = sys::stat(path).map(|s| s.is_dir()).unwrap_or(false);
    let program = if is_dir {
        "/usr/bin/hxfiles"
    } else if lower.ends_with(".png") {
        "/usr/bin/hxview"
    } else if is_video_name(&lower) {
        "/usr/bin/hxvideo"
    } else if lower.ends_with(".wav") {
        return sys::spawn("/usr/bin/hxsound", &["play", path], sys::SPAWN_DETACH) > 0;
    } else if lower.ends_with(".sh") {
        return sys::spawn("/usr/bin/hxterm", &["hsh", path], sys::SPAWN_DETACH) > 0;
    } else {
        let head = fs::read_prefix(path, 4).unwrap_or_default();
        if head.starts_with(b"\x7fELF") {
            return sys::spawn(path, &[] as &[&str], sys::SPAWN_DETACH) > 0;
        }
        "/usr/bin/hxnotes"
    };
    sys::spawn(program, &[path], sys::SPAWN_DETACH) > 0
}

pub const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "m4v", "mov", "3gp"];

pub fn is_video_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    VIDEO_EXTENSIONS.iter().any(|e| lower.ends_with(&format!(".{}", e)))
}

pub fn file_kind(name: &str, is_dir: bool, mode: u32, kind: char) -> &'static str {
    let lower = name.to_lowercase();
    if is_dir {
        "folder"
    } else if kind == 'c' || kind == 'p' {
        "device"
    } else if lower.ends_with(".png") {
        "image"
    } else if is_video_name(&lower) {
        "video"
    } else if lower.ends_with(".sh") {
        "script"
    } else if mode & 0o111 != 0 {
        "exec"
    } else if lower.ends_with(".txt") || lower.ends_with(".conf") || lower.ends_with(".md") || lower.ends_with(".rs") || !lower.contains('.') {
        "text"
    } else {
        "file"
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Primary,
    Secondary,
    Flat,
    Danger,
}

pub fn centered_text(p: &mut Painter, font: &Font, area: Area, text: &str, color: u32) {
    let shown = ellipsize(font, text, area.w - 4);
    let w = font.measure(&shown);
    p.text(font, area.x + (area.w - w) / 2, area.y + (area.h - font.height()) / 2, &shown, color);
}

pub fn text_in(p: &mut Painter, font: &Font, x: i32, area: Area, text: &str, color: u32) {
    let shown = ellipsize(font, text, area.right() - x - 2);
    p.text(font, x, area.y + (area.h - font.height()) / 2, &shown, color);
}

pub fn button(p: &mut Painter, ui: &Ui, area: Area, label: &str, style: Style, hover: bool, enabled: bool) {
    let (fill, text, border) = match (style, enabled) {
        (_, false) => (theme::surface(), theme::faint(), Some(theme::border())),
        (Style::Primary, _) => (if hover { theme::accent_hover() } else { theme::accent() }, theme::accent_text(), None),
        (Style::Danger, _) => (if hover { 0xf06b63 } else { theme::danger() }, 0xffffff, None),
        (Style::Secondary, _) => (if hover { theme::hover() } else { theme::surface_2() }, theme::text(), Some(theme::border())),
        (Style::Flat, _) => (if hover { theme::hover() } else { theme::bg() }, theme::text(), None),
    };
    if style != Style::Flat || hover {
        p.rounded(area, theme::RADIUS, fill, 255);
    }
    if let Some(b) = border {
        p.rounded_border(area, theme::RADIUS, b, 255);
    }
    centered_text(p, &ui.medium, area, label, text);
}

pub fn icon_button(p: &mut Painter, ui: &Ui, area: Area, icon: &str, hover: bool, active: bool) {
    if hover || active {
        p.rounded(area, theme::RADIUS, if active { theme::selection() } else { theme::hover() }, 255);
    }
    if let Some(img) = ui.icon(icon) {
        p.image_tinted(img, area.x + (area.w - img.w) / 2, area.y + (area.h - img.h) / 2, if active { theme::on_selection() } else { theme::text() }, 255);
    }
}

pub fn symbol(p: &mut Painter, ui: &Ui, name: &str, x: i32, y: i32, color: u32) {
    if let Some(img) = ui.icon(name) {
        p.image_tinted(img, x, y, color, 255);
    }
}

pub fn card(p: &mut Painter, area: Area) {
    p.rounded(area, 10, theme::surface(), 255);
    p.rounded_border(area, 10, theme::border(), 160);
}

pub fn separator(p: &mut Painter, x: i32, y: i32, w: i32) {
    p.fill(Area::new(x, y, w, 1), theme::border());
}

pub fn progress(p: &mut Painter, area: Area, per_mille: u32, color: u32) {
    p.rounded(area, area.h / 2, theme::surface_2(), 255);
    let filled = (area.w as i64 * per_mille.min(1000) as i64 / 1000) as i32;
    if filled > 0 {
        p.rounded(Area::new(area.x, area.y, filled.max(area.h), area.h), area.h / 2, color, 255);
    }
}

pub fn switch(p: &mut Painter, area: Area, on: bool, hover: bool) {
    let track = if on { if hover { theme::accent_hover() } else { theme::accent() } } else if hover { theme::pressed() } else { theme::hover() };
    p.rounded(area, area.h / 2, track, 255);
    let knob = area.h - 6;
    let x = if on { area.right() - knob - 3 } else { area.x + 3 };
    p.circle(x as f32 + knob as f32 / 2.0, area.y as f32 + area.h as f32 / 2.0, knob as f32 / 2.0, 0xffffff, 255);
}

pub fn checkbox(p: &mut Painter, ui: &Ui, x: i32, y: i32, checked: bool, label: &str, hover: bool) -> Area {
    let box_area = Area::new(x, y, 18, 18);
    if checked {
        p.rounded(box_area, 5, if hover { theme::accent_hover() } else { theme::accent() }, 255);
        p.line(x as f32 + 4.5, y as f32 + 9.5, x as f32 + 7.8, y as f32 + 12.8, 2.0, 0xffffff, 255);
        p.line(x as f32 + 7.8, y as f32 + 12.8, x as f32 + 13.5, y as f32 + 5.5, 2.0, 0xffffff, 255);
    } else {
        p.rounded(box_area, 5, if hover { theme::hover() } else { theme::surface_2() }, 255);
        p.rounded_border(box_area, 5, theme::faint(), 255);
    }
    let w = p.text(&ui.font, x + 28, y + (18 - ui.font.height()) / 2, label, theme::text());
    Area::new(x, y, 28 + w, 18)
}

pub fn list_row(p: &mut Painter, area: Area, selected: bool, hover: bool) {
    if selected {
        p.rounded(area, 6, theme::selection(), 255);
    } else if hover {
        p.rounded(area, 6, theme::hover(), 255);
    }
}

pub fn scrollbar(p: &mut Painter, track: Area, offset: i32, content: i32, visible: i32) {
    if content <= visible || content <= 0 {
        return;
    }
    let h = (track.h as i64 * visible as i64 / content as i64).max(24) as i32;
    let y = track.y + ((track.h - h) as i64 * offset as i64 / (content - visible).max(1) as i64) as i32;
    p.rounded(Area::new(track.x, y, track.w, h), track.w / 2, theme::faint(), 150);
}

#[derive(Clone, Default)]
pub struct TextField {
    pub text: String,
    pub cursor: usize,
    pub secret: bool,
    pub scroll: i32,
}

pub enum FieldAction {
    None,
    Changed,
    Submit,
    Next,
    Moved,
}

impl TextField {
    pub fn new(text: &str) -> TextField {
        TextField { text: String::from(text), cursor: text.chars().count(), secret: false, scroll: 0 }
    }

    pub fn secret() -> TextField {
        TextField { secret: true, ..TextField::default() }
    }

    pub fn set(&mut self, text: &str) {
        self.text = String::from(text);
        self.cursor = self.text.chars().count();
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    pub fn shown(&self) -> String {
        if self.secret { self.text.chars().map(|_| '•').collect() } else { self.text.clone() }
    }

    pub fn key(&mut self, code: i32) -> FieldAction {
        let len = self.text.chars().count();
        match code {
            10 => FieldAction::Submit,
            9 => FieldAction::Next,
            8 => {
                if self.cursor > 0 {
                    let start = self.byte_index(self.cursor - 1);
                    let end = self.byte_index(self.cursor);
                    self.text.replace_range(start..end, "");
                    self.cursor -= 1;
                    FieldAction::Changed
                } else {
                    FieldAction::None
                }
            }
            -7 => {
                if self.cursor < len {
                    let start = self.byte_index(self.cursor);
                    let end = self.byte_index(self.cursor + 1);
                    self.text.replace_range(start..end, "");
                    FieldAction::Changed
                } else {
                    FieldAction::None
                }
            }
            -3 => {
                self.cursor = self.cursor.saturating_sub(1);
                FieldAction::Moved
            }
            -4 => {
                self.cursor = (self.cursor + 1).min(len);
                FieldAction::Moved
            }
            -5 | 1 => {
                self.cursor = 0;
                FieldAction::Moved
            }
            -6 | 5 => {
                self.cursor = len;
                FieldAction::Moved
            }
            21 => {
                self.text.clear();
                self.cursor = 0;
                FieldAction::Changed
            }
            c if (32..127).contains(&c) => {
                let at = self.byte_index(self.cursor);
                self.text.insert(at, c as u8 as char);
                self.cursor += 1;
                FieldAction::Changed
            }
            _ => FieldAction::None,
        }
    }

    pub fn click(&mut self, font: &Font, local_x: i32) {
        let shown = self.shown();
        let mut x = -self.scroll;
        let mut index = 0;
        for ch in shown.chars() {
            let w = font.advance(ch);
            if x + w / 2 > local_x {
                break;
            }
            x += w;
            index += 1;
        }
        self.cursor = index;
    }
}

pub fn text_field(p: &mut Painter, ui: &Ui, area: Area, field: &mut TextField, focused: bool, placeholder: &str) {
    p.rounded(area, theme::RADIUS, theme::bg(), 255);
    p.rounded_border(area, theme::RADIUS, if focused { theme::accent() } else { theme::border() }, 255);
    if focused {
        p.rounded_border(area.expand(1), theme::RADIUS + 1, theme::accent(), 70);
    }
    let inner = area.inset(10);
    let font = &ui.font;
    let shown = field.shown();
    let prefix: String = shown.chars().take(field.cursor).collect();
    let cursor_x = font.measure(&prefix);
    if cursor_x - field.scroll > inner.w - 2 {
        field.scroll = cursor_x - inner.w + 2;
    }
    if cursor_x - field.scroll < 0 {
        field.scroll = cursor_x;
    }
    let saved = p.clip;
    p.set_clip(Area::new(area.x + 8, area.y, area.w - 16, area.h).intersect(&saved));
    let ty = area.y + (area.h - font.height()) / 2;
    if shown.is_empty() && !placeholder.is_empty() {
        p.text(font, inner.x, ty, placeholder, theme::faint());
    } else {
        p.text(font, inner.x - field.scroll, ty, &shown, theme::text());
    }
    if focused {
        p.fill(Area::new(inner.x - field.scroll + cursor_x, ty - 1, 1, font.height() + 2), theme::accent_hover());
    }
    p.clip = saved;
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 4 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else if value >= 100.0 {
        format!("{} {}", value as u64, UNITS[unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
    }
}

pub fn current_user() -> String {
    hamix_std::users::name_of(sys::getuid() as u32)
}

pub fn elevated_spawn(password: Option<&str>, program: &str, args: &[&str], stdout: Option<u64>) -> Result<i64, &'static str> {
    if sys::geteuid() != 0 {
        let Some(password) = password else {
            return Err("administrator password required");
        };
        match sys::auth(&current_user(), password) {
            0 => {}
            1 => return Err("this account is not allowed to administer the system"),
            _ => return Err("wrong password"),
        }
    }
    let flags = if sys::geteuid() != 0 { sys::SPAWN_ROOT } else { 0 };
    let pid = sys::spawn_io(program, args, flags, None, stdout, stdout);
    if pid < 0 { Err("cannot start the helper") } else { Ok(pid) }
}

pub fn read_nook_config() -> Vec<(String, String)> {
    let path = format!("{}/.config/nook.conf", home_dir());
    fs::read_to_string(&path)
        .map(|t| t.lines().filter_map(|l| l.split_once('=').map(|(k, v)| (String::from(k.trim()), String::from(v.trim())))).collect())
        .unwrap_or_default()
}

pub fn write_nook_config(key: &str, value: &str) -> bool {
    let home = home_dir();
    let dir = format!("{}/.config", home);
    sys::mkdir(&dir);
    let mut entries = read_nook_config();
    entries.retain(|(k, _)| k != key);
    entries.push((String::from(key), String::from(value)));
    let mut text = String::new();
    for (k, v) in entries {
        text.push_str(&format!("{}={}\n", k, v));
    }
    fs::write(&format!("{}/nook.conf", dir), text.as_bytes())
}

pub fn home_dir() -> String {
    hamix_std::users::find_uid("", sys::getuid() as u32).map(|u| u.home).unwrap_or_else(|| String::from("/tmp"))
}

#[derive(Clone)]
pub struct MenuEntry {
    pub label: String,
    pub shortcut: String,
    pub command: u32,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub separator: bool,
}

impl MenuEntry {
    pub fn item(label: &str, shortcut: &str, command: u32) -> MenuEntry {
        MenuEntry { label: String::from(label), shortcut: String::from(shortcut), command, enabled: true, checked: None, separator: false }
    }

    pub fn separator() -> MenuEntry {
        MenuEntry { label: String::new(), shortcut: String::new(), command: 0, enabled: false, checked: None, separator: true }
    }

    pub fn enabled(mut self, enabled: bool) -> MenuEntry {
        self.enabled = enabled;
        self
    }

    pub fn checked(mut self, checked: bool) -> MenuEntry {
        self.checked = Some(checked);
        self
    }
}

#[derive(Clone)]
pub struct Menu {
    pub title: String,
    pub entries: Vec<MenuEntry>,
}

impl Menu {
    pub fn new(title: &str, entries: Vec<MenuEntry>) -> Menu {
        Menu { title: String::from(title), entries }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuResult {
    Ignored,
    Consumed,
    Command(u32),
}

pub struct MenuBar {
    pub menus: Vec<Menu>,
    open: Option<usize>,
    hot_title: Option<usize>,
    hot_entry: Option<usize>,
    titles: Vec<Area>,
    rows: Vec<(Area, usize)>,
    dropdown: Option<Area>,
    width: i32,
}

const MENU_ROW: i32 = 28;
const KEY_F10: i32 = -50;
const MENU_SEPARATOR: i32 = 9;

impl MenuBar {
    pub const HEIGHT: i32 = 30;

    pub fn new(menus: Vec<Menu>) -> MenuBar {
        MenuBar { menus, open: None, hot_title: None, hot_entry: None, titles: Vec::new(), rows: Vec::new(), dropdown: None, width: 0 }
    }

    pub fn set_menus(&mut self, menus: Vec<Menu>) {
        if self.open.map(|i| i >= menus.len()).unwrap_or(false) {
            self.open = None;
        }
        self.menus = menus;
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) -> bool {
        let was = self.open.is_some();
        self.open = None;
        self.hot_entry = None;
        was
    }

    pub fn open_menu(&mut self, index: usize) {
        if index < self.menus.len() {
            self.open = Some(index);
            self.hot_entry = None;
        }
    }

    pub fn bar_area(&self) -> Area {
        Area::new(0, 0, self.width, Self::HEIGHT)
    }

    pub fn dropdown_area(&self) -> Option<Area> {
        if self.open.is_some() { self.dropdown } else { None }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.bar_area().contains(x, y) || self.dropdown_area().map(|a| a.contains(x, y)).unwrap_or(false)
    }

    fn layout_titles(&mut self, ui: &Ui) {
        self.titles.clear();
        let mut x = 6;
        for menu in self.menus.iter() {
            let w = ui.font.measure(&menu.title) + 20;
            self.titles.push(Area::new(x, 3, w, Self::HEIGHT - 6));
            x += w + 2;
        }
    }

    fn layout_dropdown(&mut self, ui: &Ui, max_w: i32, max_h: i32) {
        self.rows.clear();
        self.dropdown = None;
        let Some(open) = self.open else {
            return;
        };
        let Some(menu) = self.menus.get(open) else {
            return;
        };
        let label_w = menu.entries.iter().map(|e| ui.font.measure(&e.label)).max().unwrap_or(0);
        let shortcut_w = menu.entries.iter().map(|e| if e.shortcut.is_empty() { 0 } else { ui.small.measure(&e.shortcut) + 28 }).max().unwrap_or(0);
        let w = (label_w + shortcut_w + 64).clamp(180, max_w.max(180));
        let h = menu.entries.iter().map(|e| if e.separator { MENU_SEPARATOR } else { MENU_ROW }).sum::<i32>() + 12;
        let title = self.titles.get(open).copied().unwrap_or(Area::new(6, 3, 60, Self::HEIGHT - 6));
        let x = title.x.min(max_w - w - 4).max(2);
        let area = Area::new(x, Self::HEIGHT - 1, w, h.min(max_h - Self::HEIGHT - 2));
        let mut y = area.y + 6;
        for (i, entry) in menu.entries.iter().enumerate() {
            let rh = if entry.separator { MENU_SEPARATOR } else { MENU_ROW };
            self.rows.push((Area::new(area.x + 6, y, area.w - 12, rh), i));
            y += rh;
        }
        self.dropdown = Some(area);
    }

    pub fn draw(&mut self, p: &mut Painter, ui: &Ui, width: i32) {
        self.width = width;
        self.layout_titles(ui);
        let bar = Area::new(0, 0, width, Self::HEIGHT);
        p.fill(bar, theme::surface());
        p.fill(Area::new(0, Self::HEIGHT - 1, width, 1), theme::border());
        for (i, menu) in self.menus.iter().enumerate() {
            let area = self.titles[i];
            let active = self.open == Some(i);
            if active || self.hot_title == Some(i) {
                p.rounded(area, 6, if active { theme::selection() } else { theme::hover() }, 255);
            }
            centered_text(p, &ui.font, area, &menu.title, if active { theme::on_selection() } else { theme::text() });
        }
    }

    pub fn draw_dropdown(&mut self, p: &mut Painter, ui: &Ui, max_w: i32, max_h: i32) {
        self.layout_dropdown(ui, max_w, max_h);
        let (Some(area), Some(open)) = (self.dropdown, self.open) else {
            return;
        };
        let Some(menu) = self.menus.get(open) else {
            return;
        };
        let saved = p.clip;
        p.shadow(area, 8, 14, theme::palette().shadow_alpha, 4);
        p.rounded(area, 8, theme::popup(), 255);
        p.rounded_border(area, 8, theme::border(), 255);
        p.set_clip(area.intersect(&saved));
        for (row, index) in self.rows.iter() {
            let entry = &menu.entries[*index];
            if entry.separator {
                p.fill(Area::new(row.x + 6, row.y + MENU_SEPARATOR / 2, row.w - 12, 1), theme::border());
                continue;
            }
            let hot = self.hot_entry == Some(*index) && entry.enabled;
            if hot {
                p.rounded(*row, 6, theme::accent(), 255);
            }
            let color = if !entry.enabled { theme::faint() } else if hot { theme::accent_text() } else { theme::text() };
            if entry.checked == Some(true) {
                let (cx, cy) = (row.x as f32 + 12.0, row.y as f32 + row.h as f32 / 2.0);
                p.line(cx - 4.0, cy, cx - 1.0, cy + 3.5, 1.8, color, 255);
                p.line(cx - 1.0, cy + 3.5, cx + 5.0, cy - 4.0, 1.8, color, 255);
            }
            text_in(p, &ui.font, row.x + 28, *row, &entry.label, color);
            if !entry.shortcut.is_empty() {
                let sw = ui.small.measure(&entry.shortcut);
                p.text(&ui.small, row.right() - sw - 10, row.y + (row.h - ui.small.height()) / 2, &entry.shortcut, if hot { theme::accent_text() } else { theme::faint() });
            }
        }
        p.clip = saved;
    }

    fn title_at(&self, x: i32, y: i32) -> Option<usize> {
        self.titles.iter().position(|a| a.contains(x, y))
    }

    fn entry_at(&self, x: i32, y: i32) -> Option<usize> {
        self.rows.iter().find(|(a, _)| a.contains(x, y)).map(|(_, i)| *i)
    }

    pub fn motion(&mut self, x: i32, y: i32) -> bool {
        let title = self.title_at(x, y);
        let entry = if self.open.is_some() { self.entry_at(x, y) } else { None };
        let mut changed = title != self.hot_title || entry != self.hot_entry;
        self.hot_title = title;
        self.hot_entry = entry;
        if let (Some(open), Some(t)) = (self.open, title) {
            if open != t {
                self.open = Some(t);
                self.hot_entry = None;
                changed = true;
            }
        }
        changed
    }

    pub fn leave(&mut self) -> bool {
        let changed = self.hot_title.is_some() || self.hot_entry.is_some();
        self.hot_title = None;
        self.hot_entry = None;
        changed
    }

    fn activate(&mut self, index: usize) -> MenuResult {
        let Some(open) = self.open else {
            return MenuResult::Consumed;
        };
        let Some(entry) = self.menus.get(open).and_then(|m| m.entries.get(index)) else {
            return MenuResult::Consumed;
        };
        if entry.separator || !entry.enabled {
            return MenuResult::Consumed;
        }
        let command = entry.command;
        self.close();
        MenuResult::Command(command)
    }

    pub fn press(&mut self, x: i32, y: i32) -> MenuResult {
        if let Some(t) = self.title_at(x, y) {
            if self.open == Some(t) {
                self.close();
            } else {
                self.open_menu(t);
            }
            return MenuResult::Consumed;
        }
        if self.open.is_none() {
            return if self.bar_area().contains(x, y) { MenuResult::Consumed } else { MenuResult::Ignored };
        }
        if let Some(index) = self.entry_at(x, y) {
            return self.activate(index);
        }
        if self.dropdown_area().map(|a| a.contains(x, y)).unwrap_or(false) {
            return MenuResult::Consumed;
        }
        self.close();
        MenuResult::Consumed
    }

    fn step_entry(&mut self, forward: bool) {
        let Some(menu) = self.open.and_then(|o| self.menus.get(o)) else {
            return;
        };
        let count = menu.entries.len();
        if count == 0 {
            return;
        }
        let mut index = self.hot_entry.unwrap_or(if forward { count - 1 } else { 0 });
        for _ in 0..count {
            index = if forward { (index + 1) % count } else { (index + count - 1) % count };
            let e = &menu.entries[index];
            if !e.separator && e.enabled {
                self.hot_entry = Some(index);
                return;
            }
        }
    }

    pub fn key(&mut self, code: i32) -> MenuResult {
        let Some(open) = self.open else {
            if code == KEY_F10 && !self.menus.is_empty() {
                self.open_menu(0);
                self.step_entry(true);
                return MenuResult::Consumed;
            }
            return MenuResult::Ignored;
        };
        match code {
            27 => {
                self.close();
            }
            -1 => self.step_entry(false),
            -2 => self.step_entry(true),
            -3 => {
                let n = self.menus.len();
                self.open_menu((open + n - 1) % n);
            }
            -4 => {
                let n = self.menus.len();
                self.open_menu((open + 1) % n);
            }
            10 | 32 => {
                if let Some(index) = self.hot_entry {
                    return self.activate(index);
                }
            }
            _ => {}
        }
        MenuResult::Consumed
    }
}
