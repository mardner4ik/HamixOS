use alloc::collections::BTreeMap;
use alloc::string::String;
use vellum::gfx::ellipsize;
use vellum::{Area, Font, Image, Painter};

use super::posix;
use crate::style::{ButtonKind, TextSize, Tone};

pub const ASSETS: &str = "/usr/share/nook";
pub const RADIUS: i32 = 7;

#[derive(Clone, Copy)]
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
    pub popup: u32,
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
    popup: 0x2a2f39,
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
    popup: 0xffffff,
};

pub fn configured_dark() -> bool {
    let Some(home) = posix::env("HOME") else {
        return false;
    };
    posix::read_text(&alloc::format!("{}/.config/nook.conf", home))
        .map(|t| t.lines().filter_map(|l| l.split_once('=')).any(|(k, v)| k.trim() == "theme" && v.trim() == "dark"))
        .unwrap_or(false)
}

#[allow(dead_code)]
pub fn configured_light() -> bool {
    !configured_dark()
}

pub struct Fonts {
    pub body: Font,
    pub strong: Font,
    pub small: Font,
    pub title: Font,
    pub display: Font,
    pub mono: Font,
    pub mono_bold: Option<Font>,
}

fn load_font(name: &str) -> Option<Font> {
    posix::read_file(&alloc::format!("{}/fonts/{}.hfnt", ASSETS, name)).and_then(|b| Font::parse(&b))
}

impl Fonts {
    pub fn load() -> Option<Fonts> {
        Some(Fonts {
            body: load_font("sans-13")?,
            strong: load_font("sans-medium-13")?,
            small: load_font("sans-11")?,
            title: load_font("sans-medium-18")?,
            display: load_font("sans-light-30")?,
            mono: load_font("mono-14")?,
            mono_bold: load_font("mono-bold-14"),
        })
    }

    pub fn get(&self, size: TextSize) -> &Font {
        match size {
            TextSize::Body => &self.body,
            TextSize::Small => &self.small,
            TextSize::Strong => &self.strong,
            TextSize::Title => &self.title,
            TextSize::Display => &self.display,
            TextSize::Mono => &self.mono,
            TextSize::Heading1 => &self.display,
            TextSize::Heading2 => &self.title,
            TextSize::Heading3 => &self.strong,
        }
    }

    pub fn get_by_name(&self, name: &str) -> Option<&Font> {
        match name {
            "mono" | "monospace" => Some(&self.mono),
            "mono-bold" | "mono-bold-14" => self.mono_bold.as_ref().or(Some(&self.mono)),
            "body" | "sans" => Some(&self.body),
            "strong" | "bold" | "sans-medium" => Some(&self.strong),
            "small" => Some(&self.small),
            "title" | "heading2" => Some(&self.title),
            "display" | "heading1" => Some(&self.display),
            _ => None,
        }
    }
}

pub struct Theme {
    pub p: Palette,
    pub fonts: Fonts,
    images: BTreeMap<String, Option<Image>>,
}

impl Theme {
    pub fn load() -> Option<Theme> {
        Some(Theme { p: if configured_dark() { DARK } else { LIGHT }, fonts: Fonts::load()?, images: BTreeMap::new() })
    }

    pub fn set_light(&mut self, light: bool) {
        self.p = if light { LIGHT } else { DARK };
    }

    pub fn is_light(&self) -> bool {
        self.p.bg == LIGHT.bg
    }

    pub fn icon_path(name: &str) -> String {
        if name.starts_with('/') { String::from(name) } else { alloc::format!("{}/icons/{}.png", ASSETS, name) }
    }

    pub fn preload(&mut self, key: &str) {
        if !self.images.contains_key(key) {
            let image = posix::read_file(key).and_then(|b| Image::from_png(&b));
            self.images.insert(String::from(key), image);
        }
    }

    pub fn image(&self, key: &str) -> Option<&Image> {
        self.images.get(key).and_then(|i| i.as_ref())
    }

    pub fn tone(&self, tone: Tone, inverted: bool) -> u32 {
        match tone {
            Tone::Normal if inverted => self.p.on_selection,
            Tone::Dim if inverted => self.p.on_selection,
            Tone::Normal => self.p.text,
            Tone::Dim => self.p.dim,
            Tone::Faint => self.p.faint,
            Tone::Accent => self.p.accent,
            Tone::Danger => self.p.danger,
            Tone::Success => self.p.success,
            Tone::Warning => self.p.warn,
        }
    }

    pub fn centered_text(&self, p: &mut Painter, font: &Font, area: Area, text: &str, color: u32) {
        let shown = ellipsize(font, text, area.w - 4);
        let w = font.measure(&shown);
        p.text(font, area.x + (area.w - w) / 2, area.y + (area.h - font.height()) / 2, &shown, color);
    }

    pub fn button(&self, p: &mut Painter, area: Area, label: &str, icon: Option<&str>, kind: ButtonKind, size: TextSize, hover: bool, pressed: bool, enabled: bool, text_color: Option<u32>) {
        let c = &self.p;
        let (fill, text, border) = match (kind, enabled) {
            (_, false) => (c.surface, c.faint, Some(c.border)),
            (ButtonKind::Primary, _) => (if hover { c.accent_hover } else { c.accent }, c.accent_text, None),
            (ButtonKind::Danger, _) => (if hover { 0xf06b63 } else { c.danger }, 0xffffff, None),
            (ButtonKind::Secondary, _) => (if pressed { c.pressed } else if hover { c.hover } else { c.surface_2 }, text_color.unwrap_or(c.text), Some(c.border)),
            (ButtonKind::Flat, _) => (if pressed { c.pressed } else if hover { c.hover } else { c.bg }, text_color.unwrap_or(c.text), None),
            (ButtonKind::Link, _) => (if pressed { c.pressed } else if hover { c.hover } else { 0 }, text_color.unwrap_or(if hover { c.accent_hover } else { c.accent }), None),
        };
        if (kind != ButtonKind::Flat && kind != ButtonKind::Link) || hover || pressed {
            if fill != 0 {
                p.rounded(area, RADIUS, fill, 255);
            }
        }
        if let Some(b) = border {
            p.rounded_border(area, RADIUS, b, 255);
        }
        let image = icon.and_then(|i| self.image(&Theme::icon_path(i)));
        match (image, label.is_empty()) {
            (Some(img), true) => p.image_tinted(img, area.x + (area.w - img.w) / 2, area.y + (area.h - img.h) / 2, text, 255),
            (Some(img), false) => {
                let font = self.fonts.get(size);
                let tw = font.measure(label);
                let total = img.w + 8 + tw;
                let x = area.x + (area.w - total).max(8) / 2;
                p.image_tinted(img, x, area.y + (area.h - img.h) / 2, text, 255);
                p.text(font, x + img.w + 8, area.y + (area.h - font.height()) / 2, &ellipsize(font, label, area.right() - x - img.w - 12), text);
            }
            (None, _) => {
                self.centered_text(p, self.fonts.get(size), area, label, text);
                if kind == ButtonKind::Link && hover {
                    let font = self.fonts.get(size);
                    let tw = font.measure(label);
                    let x = area.x + (area.w - tw).max(0) / 2;
                    let y = area.y + (area.h + font.height()) / 2;
                    p.fill(Area::new(x, y, tw.min(area.w), 1), text);
                }
            }
        }
    }

    pub fn field_frame(&self, p: &mut Painter, area: Area, focused: bool) {
        p.rounded(area, RADIUS, self.p.bg, 255);
        p.rounded_border(area, RADIUS, if focused { self.p.accent } else { self.p.border }, 255);
        if focused {
            p.rounded_border(area.expand(1), RADIUS + 1, self.p.accent, 70);
        }
    }

    pub fn switch(&self, p: &mut Painter, area: Area, on: bool, hover: bool) {
        let c = &self.p;
        let track = if on { if hover { c.accent_hover } else { c.accent } } else if hover { c.pressed } else { c.hover };
        p.rounded(area, area.h / 2, track, 255);
        let knob = area.h - 6;
        let x = if on { area.right() - knob - 3 } else { area.x + 3 };
        p.circle(x as f32 + knob as f32 / 2.0, area.y as f32 + area.h as f32 / 2.0, knob as f32 / 2.0, 0xffffff, 255);
    }

    pub fn checkbox(&self, p: &mut Painter, x: i32, y: i32, checked: bool, label: &str, hover: bool, color: u32) {
        let c = &self.p;
        let box_area = Area::new(x, y, 18, 18);
        if checked {
            p.rounded(box_area, 5, if hover { c.accent_hover } else { c.accent }, 255);
            p.line(x as f32 + 4.5, y as f32 + 9.5, x as f32 + 7.8, y as f32 + 12.8, 2.0, 0xffffff, 255);
            p.line(x as f32 + 7.8, y as f32 + 12.8, x as f32 + 13.5, y as f32 + 5.5, 2.0, 0xffffff, 255);
        } else {
            p.rounded(box_area, 5, if hover { c.hover } else { c.surface_2 }, 255);
            p.rounded_border(box_area, 5, c.faint, 255);
        }
        if !label.is_empty() {
            p.text(&self.fonts.body, x + 28, y + (18 - self.fonts.body.height()) / 2, label, color);
        }
    }

    pub fn progress(&self, p: &mut Painter, area: Area, fraction: f32) {
        p.rounded(area, area.h / 2, self.p.surface_2, 255);
        let filled = (area.w as f32 * fraction.clamp(0.0, 1.0)) as i32;
        if filled > 0 {
            p.rounded(Area::new(area.x, area.y, filled.max(area.h), area.h), area.h / 2, self.p.accent, 255);
        }
    }

    pub fn slider(&self, p: &mut Painter, area: Area, fraction: f32, hover: bool) {
        let track = Area::new(area.x + 8, area.y + area.h / 2 - 2, (area.w - 16).max(0), 4);
        p.rounded(track, 2, self.p.surface_2, 255);
        let x = track.x + (track.w as f32 * fraction.clamp(0.0, 1.0)) as i32;
        p.rounded(Area::new(track.x, track.y, x - track.x, 4), 2, self.p.accent, 255);
        p.circle(x as f32, area.y as f32 + area.h as f32 / 2.0, 8.0, if hover { self.p.accent_hover } else { self.p.accent }, 255);
        p.circle(x as f32, area.y as f32 + area.h as f32 / 2.0, 4.0, 0xffffff, 255);
    }

    pub fn row_background(&self, p: &mut Painter, area: Area, selected: bool, hover: bool) {
        if selected {
            p.rounded(area, 6, self.p.selection, 255);
        } else if hover {
            p.rounded(area, 6, self.p.hover, 255);
        }
    }

    pub fn scrollbar(&self, p: &mut Painter, track: Area, offset: i32, content: i32, visible: i32) {
        if content <= visible || content <= 0 {
            return;
        }
        let h = (track.h as i64 * visible as i64 / content as i64).max(24) as i32;
        let y = track.y + ((track.h - h) as i64 * offset as i64 / (content - visible).max(1) as i64) as i32;
        p.rounded(Area::new(track.x, y, track.w, h), track.w / 2, self.p.faint, 150);
    }

    pub fn card(&self, p: &mut Painter, area: Area) {
        p.rounded(area, 10, self.p.surface, 255);
        p.rounded_border(area, 10, self.p.border, 160);
    }

    pub fn chevron(&self, p: &mut Painter, cx: i32, cy: i32, color: u32) {
        p.line(cx as f32 - 4.0, cy as f32 - 2.0, cx as f32, cy as f32 + 2.0, 1.6, color, 255);
        p.line(cx as f32, cy as f32 + 2.0, cx as f32 + 4.0, cy as f32 - 2.0, 1.6, color, 255);
    }

    pub fn dots(&self, p: &mut Painter, cx: i32, cy: i32, color: u32) {
        for dy in [-5, 0, 5] {
            p.circle(cx as f32, (cy + dy) as f32, 1.8, color, 255);
        }
    }
}
