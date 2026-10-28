use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::sys;
use hxclient::ui::{self, theme, Style};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

use crate::{Body, Nook, PowerAction};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    ToggleMaximize,
    Minimize,
    Close,
}

pub struct WindowMenu {
    pub window: u32,
    pub x: i32,
    pub y: i32,
    pub items: Vec<(String, &'static str, WindowAction, bool)>,
    pub hover: Option<usize>,
}

const MENU_W: i32 = 236;
const MENU_ROW: i32 = 34;

impl Nook {
    fn menu_area(&self) -> Option<Area> {
        let menu = self.menu.as_ref()?;
        let h = menu.items.len() as i32 * MENU_ROW + 12;
        let (sw, sh) = (self.core.screen.width, self.core.screen.height);
        Some(Area::new(menu.x.clamp(4, (sw - MENU_W - 4).max(4)), menu.y.clamp(4, (sh - h - 4).max(4)), MENU_W, h))
    }

    fn menu_row(area: Area, index: usize) -> Area {
        Area::new(area.x + 6, area.y + 6 + index as i32 * MENU_ROW, area.w - 12, MENU_ROW)
    }

    pub fn open_menu(&mut self, window: u32, x: i32, y: i32, items: Vec<(String, &'static str, WindowAction, bool)>) {
        self.close_menu();
        for (_, icon, _, _) in &items {
            self.ui.load_icon(icon);
        }
        self.menu = Some(WindowMenu { window, x, y, items, hover: None });
        if let Some(area) = self.menu_area() {
            self.core.add_damage(area.expand(32));
        }
    }

    pub fn close_menu(&mut self) {
        if let Some(area) = self.menu_area() {
            self.core.add_damage(area.expand(32));
        }
        self.menu = None;
    }

    pub fn menu_press(&mut self, x: i32, y: i32) {
        let Some(area) = self.menu_area() else {
            return;
        };
        let picked = self.menu.as_ref().and_then(|m| (0..m.items.len()).find(|i| Self::menu_row(area, *i).contains(x, y)).map(|i| (m.window, m.items[i].2)));
        self.close_menu();
        let Some((id, action)) = picked else {
            return;
        };
        match action {
            WindowAction::ToggleMaximize => self.toggle_maximize(id),
            WindowAction::Minimize => self.minimize(id),
            WindowAction::Close => self.close_window(id),
        }
    }

    pub fn menu_motion(&mut self, x: i32, y: i32) {
        let Some(area) = self.menu_area() else {
            return;
        };
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let hover = (0..menu.items.len()).find(|i| Self::menu_row(area, *i).contains(x, y));
        if hover != menu.hover {
            menu.hover = hover;
            self.core.add_damage(area);
        }
    }

    pub fn draw_window_menu(&mut self) {
        let Some(area) = self.menu_area() else {
            return;
        };
        if !area.expand(32).overlaps(&self.core.screen.clip) {
            return;
        }
        let ui = self.ui();
        let menu = self.menu.as_ref().unwrap();
        let items: Vec<(String, &'static str, bool)> = menu.items.iter().map(|(l, i, _, d)| (l.clone(), *i, *d)).collect();
        let hover = menu.hover;
        let mut p = self.core.painter_clipped();
        popup_frame(&mut p, area, 10);
        for (i, (label, icon, danger)) in items.iter().enumerate() {
            let row = Self::menu_row(area, i);
            let hot = hover == Some(i);
            if hot {
                p.rounded(row, 6, if *danger { theme::danger() } else { theme::accent() }, 255);
            }
            if let Some(img) = ui.icon(icon) {
                p.image_tinted(img, row.x + 10, row.y + (row.h - img.h) / 2, if hot { 0xffffff } else if *danger { theme::danger() } else { theme::dim() }, 255);
            }
            p.text(&ui.font, row.x + 36, row.y + (row.h - ui.font.height()) / 2, label, if hot { 0xffffff } else { theme::text() });
        }
    }

    pub fn draw_toasts(&mut self) {
        if self.toasts.is_empty() || !self.toast_region().overlaps(&self.core.screen.clip) {
            return;
        }
        let ui = self.ui();
        let areas: Vec<Area> = (0..self.toasts.len()).map(|i| self.toast_area(i)).collect();
        let toasts: Vec<(String, String, String)> = self.toasts.iter().map(|t| (t.title.clone(), t.body.clone(), t.icon.clone())).collect();
        let mut p = self.core.painter_clipped();
        for (i, (title, body, icon)) in toasts.iter().enumerate() {
            let a = areas[i];
            p.shadow(a, 12, 18, 200, 5);
            p.rounded(a, 12, theme::palette().osd, 248);
            p.rounded_border(a, 12, theme::palette().edge, theme::palette().edge_alpha + 4);
            let mut text_x = a.x + 16;
            if let Some(img) = ui.icon(icon) {
                if img.w <= 16 {
                    p.rounded(Area::new(a.x + 14, a.y + 16, 40, 40), 10, theme::surface_2(), 255);
                    p.image_tinted(img, a.x + 26, a.y + 28, theme::accent_hover(), 255);
                } else {
                    p.image_scaled(img, Area::new(a.x + 14, a.y + 16, 40, 40), 255);
                }
                text_x = a.x + 66;
            }
            let max = a.right() - text_x - 14;
            p.text(&ui.medium, text_x, a.y + 16, &ellipsize(&ui.medium, title, max), theme::text());
            p.text(&ui.font, text_x, a.y + 38, &ellipsize(&ui.font, body, max), theme::dim());
        }
    }

    pub fn draw_switcher(&mut self) {
        let Some(selected) = self.switcher else {
            return;
        };
        let area = self.switcher_area();
        if !area.expand(24).overlaps(&self.core.screen.clip) {
            return;
        }
        let order = self.switcher_order();
        let entries: Vec<(String, String)> = order
            .iter()
            .filter_map(|id| self.core.index_of(*id))
            .map(|i| (self.core.windows[i].title.clone(), format!("apps/{}", if self.core.windows[i].icon.is_empty() { "hello" } else { self.core.windows[i].icon.as_str() })))
            .collect();
        for (_, icon) in &entries {
            self.ui.load_icon(icon);
        }
        let ui = self.ui();
        let mut p = self.core.painter_clipped();
        popup_frame(&mut p, area, 16);
        for (i, (title, icon)) in entries.iter().enumerate() {
            let cell = Area::new(area.x + 12 + i as i32 * 96, area.y + 14, 90, 88);
            if i == selected {
                p.rounded(cell, 12, theme::accent(), 90);
                p.rounded_border(cell, 12, theme::accent_hover(), 200);
            }
            if let Some(img) = ui.icon(icon) {
                p.image(img, cell.x + (cell.w - img.w) / 2, cell.y + 10, 255);
            }
            if i == selected {
                ui::centered_text(&mut p, &ui.font, Area::new(area.x, area.bottom() - 34, area.w, 24), title, theme::text());
            }
        }
    }

    pub fn draw_about_window(p: &mut Painter, ui: &hxclient::ui::Ui, content: Area) {
        p.fill(content, theme::bg());
        let wordmark = if theme::is_light() { "about-logo-light" } else { "about-logo-dark" };
        if let Some(logo) = ui.icon(wordmark) {
            p.image(logo, content.x + (content.w - logo.w) / 2, content.y + 16, 255);
        }
        ui::centered_text(p, &ui.medium, Area::new(content.x, content.y + 120, content.w, 22), "HamixOS 0.6.1", theme::text());
        ui::centered_text(p, &ui.font, Area::new(content.x, content.y + 142, content.w, 20), "Nook desktop · bash on hxlibc · hext filesystem · smoltcp", theme::dim());
        let info = sys::sysinfo();
        let cpus = sys::cpu_stats().len();
        let lines = [
            ("Memory", format!("{} of {}", ui::human_size(info.mem_used()), ui::human_size(info.mem_total))),
            ("Processors", if cpus == 1 { String::from("1 core") } else { format!("{} cores", cpus) }),
            ("Uptime", format!("{} min", info.uptime_ms / 60000)),
            ("Processes", format!("{}", info.processes)),
        ];
        let mut y = content.y + 178;
        for (k, v) in lines {
            p.text(&ui.font, content.x + 110, y, k, theme::faint());
            p.text(&ui.font, content.x + 210, y, &v, theme::text());
            y += 24;
        }
        ui::centered_text(p, &ui.small, Area::new(content.x, content.bottom() - 34, content.w, 20), "Written from scratch in Rust", theme::faint());
    }

    pub fn draw_auth_window(p: &mut Painter, ui: &hxclient::ui::Ui, content: Area, body: &mut Body, pointer: (i32, i32)) {
        let Body::Auth { action, field, error } = body else {
            return;
        };
        p.fill(content, theme::bg());
        let verb = if *action == PowerAction::Reboot { "restart" } else { "shut down" };
        p.text(&ui.medium, content.x + 20, content.y + 18, &format!("Authentication is required to {} the computer", verb), theme::text());
        p.text(&ui.font, content.x + 20, content.y + 42, &format!("Enter the password of {}.", ui::current_user()), theme::dim());
        let input = Area::new(content.x + 20, content.y + 70, content.w - 40, 38);
        ui::text_field(p, ui, input, field, true, "Password");
        if !error.is_empty() {
            p.text(&ui.font, content.x + 20, content.y + 116, error, theme::danger());
        }
        let ok = Area::new(content.right() - 112, content.bottom() - 52, 96, 36);
        let cancel = Area::new(content.right() - 216, content.bottom() - 52, 96, 36);
        ui::button(p, ui, cancel, "Cancel", Style::Secondary, cancel.contains(pointer.0, pointer.1), true);
        let label = if *action == PowerAction::Reboot { "Restart" } else { "Shut down" };
        ui::button(p, ui, ok, label, Style::Danger, ok.contains(pointer.0, pointer.1), true);
    }
}

impl Nook {
    pub fn wallpaper_buttons(content: Area) -> (Area, Area) {
        let set = Area::new(content.right() - 156, content.bottom() - 52, 140, 36);
        let cancel = Area::new(set.x - 108, set.y, 96, 36);
        (set, cancel)
    }

    pub fn draw_wallpaper_window(p: &mut Painter, ui: &hxclient::ui::Ui, content: Area, body: &mut Body, pointer: (i32, i32)) {
        let Body::Wallpaper { path, app, preview, .. } = body else {
            return;
        };
        p.fill(content, theme::bg());
        p.text(&ui.medium, content.x + 20, content.y + 18, &format!("{} wants to change the wallpaper", app), theme::text());
        let file = path.rsplit('/').next().unwrap_or(path.as_str());
        p.text(&ui.font, content.x + 20, content.y + 42, &ellipsize(&ui.font, file, content.w - 40), theme::dim());
        let frame = Area::new(content.x + 20, content.y + 72, content.w - 40, content.h - 72 - 68);
        p.rounded(frame, 10, theme::palette().sidebar, 255);
        if preview.w > 0 {
            let x = frame.x + (frame.w - preview.w) / 2;
            let y = frame.y + (frame.h - preview.h) / 2;
            p.image(preview, x, y, 255);
        }
        let (set, cancel) = Nook::wallpaper_buttons(content);
        ui::button(p, ui, cancel, "Cancel", Style::Secondary, cancel.contains(pointer.0, pointer.1), true);
        ui::button(p, ui, set, "Set wallpaper", Style::Primary, set.contains(pointer.0, pointer.1), true);
    }
}

pub fn popup_frame(p: &mut Painter, area: Area, radius: i32) {
    p.shadow(area, radius, 26, 230, 8);
    p.rounded(area, radius, theme::palette().osd, 250);
    p.rounded_border(area, radius, theme::palette().edge, theme::palette().edge_alpha + 2);
}
