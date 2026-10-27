use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::sys::{self, MOUSE_LEFT, MOUSE_RIGHT};
use hxclient::ui::{self, theme};
use vellum::gfx::ellipsize;
use vellum::Area;

use crate::config::{self, Config};
use crate::{apps, DesktopIcon, DesktopItem, Grab, MenuAction, MenuItem, Nook, Popup, ICON_CELL_H, ICON_CELL_W, ROLE_FILES, ROLE_INSTALLER, ROLE_SETTINGS, ROLE_TERMINAL, TOP_H};

const GRID: i32 = 8;

impl Nook {
    fn default_slot(&self, slot: usize) -> (i32, i32) {
        let rows = ((self.core.screen.height - TOP_H - 120) / ICON_CELL_H).max(1) as usize;
        let col = (slot / rows) as i32;
        let row = (slot % rows) as i32;
        (16 + col * (ICON_CELL_W + 4), TOP_H + 16 + row * ICON_CELL_H)
    }

    pub fn build_desktop(&mut self, cfg: &Config) {
        let mut items = Vec::new();
        if self.live {
            items.extend(apps::by_role(ROLE_INSTALLER).map(DesktopItem::App));
        }
        items.push(DesktopItem::Home);
        for app in apps::default_desktop() {
            let item = DesktopItem::App(app);
            if !items.contains(&item) && (self.live || !apps::get(app).live_only) {
                items.push(item);
            }
        }
        for key in cfg.desktop_added.iter() {
            if let Some(item) = DesktopItem::from_key(key) {
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        items.retain(|i| !cfg.desktop_removed.iter().any(|r| r == i.key()));
        self.desktop.clear();
        for (slot, item) in items.into_iter().enumerate() {
            let (x, y) = cfg.desktop.iter().find(|(k, _, _)| k == item.key()).map(|(_, x, y)| (*x, *y)).unwrap_or_else(|| self.default_slot(slot));
            self.desktop.push(DesktopIcon { item, x, y });
        }
        self.clamp_desktop_icons();
    }

    pub fn clamp_desktop_icons(&mut self) {
        let (w, h) = (self.core.screen.width, self.core.screen.height);
        for icon in self.desktop.iter_mut() {
            icon.x = icon.x.clamp(0, (w - ICON_CELL_W).max(0));
            icon.y = icon.y.clamp(TOP_H, (h - ICON_CELL_H).max(TOP_H));
        }
    }

    pub fn desktop_area(&self, index: usize) -> Area {
        let icon = &self.desktop[index];
        Area::new(icon.x, icon.y, ICON_CELL_W, ICON_CELL_H - 6)
    }

    pub fn desktop_icon_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.desktop.len()).rev().find(|i| self.desktop_area(*i).contains(x, y))
    }

    pub fn save_desktop(&mut self) {
        let positions: Vec<(String, i32, i32)> = self.desktop.iter().map(|d| (String::from(d.item.key()), d.x, d.y)).collect();
        config::save_desktop(&positions, &self.desktop_removed, &self.desktop_added);
    }

    pub fn open_desktop_item(&mut self, index: usize) {
        match self.desktop.get(index).map(|d| d.item) {
            Some(DesktopItem::App(app)) => self.launch(app, &[]),
            Some(DesktopItem::Home) => {
                let home = ui::home_dir();
                self.launch_role(ROLE_FILES, &[home.as_str()]);
            }
            None => {}
        }
    }

    pub fn add_to_desktop(&mut self, app: usize) {
        let item = DesktopItem::App(app);
        if self.desktop.iter().any(|d| d.item == item) {
            return;
        }
        self.desktop_removed.retain(|k| k != item.key());
        if !self.desktop_added.iter().any(|k| k == item.key()) {
            self.desktop_added.push(String::from(item.key()));
        }
        let mut slot = 0;
        let (x, y) = loop {
            let (x, y) = self.default_slot(slot);
            let candidate = Area::new(x, y, ICON_CELL_W, ICON_CELL_H - 6);
            if !(0..self.desktop.len()).any(|i| self.desktop_area(i).overlaps(&candidate)) || slot > 64 {
                break (x, y);
            }
            slot += 1;
        };
        self.desktop.push(DesktopIcon { item, x, y });
        let area = self.desktop_area(self.desktop.len() - 1);
        self.core.add_damage(area);
        self.save_desktop();
    }

    pub fn remove_from_desktop(&mut self, index: usize) {
        if index >= self.desktop.len() {
            return;
        }
        let area = self.desktop_area(index);
        let icon = self.desktop.remove(index);
        let key = String::from(icon.item.key());
        self.desktop_added.retain(|k| *k != key);
        if !self.desktop_removed.contains(&key) {
            self.desktop_removed.push(key);
        }
        self.desktop_selected = None;
        self.desktop_hover = None;
        self.core.add_damage(area);
        self.save_desktop();
    }

    pub fn arrange_icons(&mut self) {
        for i in 0..self.desktop.len() {
            let old = self.desktop_area(i);
            self.core.add_damage(old);
            let (x, y) = self.default_slot(i);
            self.desktop[i].x = x;
            self.desktop[i].y = y;
            let new = self.desktop_area(i);
            self.core.add_damage(new);
        }
        self.save_desktop();
    }

    pub fn desktop_press(&mut self, x: i32, y: i32, button: u32) {
        let hit = self.desktop_icon_at(x, y);
        if button == MOUSE_LEFT {
            if let Some(index) = hit {
                let now = sys::uptime_ms();
                let double = self.last_click.1 == index as i32 && self.last_click.2 == -1 && now.saturating_sub(self.last_click.0) < 500;
                self.last_click = (now, index as i32, -1);
                let old = self.desktop_selected.replace(index);
                if let Some(o) = old {
                    if o < self.desktop.len() {
                        let a = self.desktop_area(o);
                        self.core.add_damage(a);
                    }
                }
                let a = self.desktop_area(index);
                self.core.add_damage(a);
                if double {
                    self.open_desktop_item(index);
                    return;
                }
                let icon = &self.desktop[index];
                self.grab = Grab::IconDrag { index, dx: x - icon.x, dy: y - icon.y, start_x: x, start_y: y, moved: false };
                return;
            }
            if let Some(o) = self.desktop_selected.take() {
                if o < self.desktop.len() {
                    let a = self.desktop_area(o);
                    self.core.add_damage(a);
                }
            }
        }
        if button == MOUSE_RIGHT {
            let items = match hit {
                Some(index) => {
                    let mut items = alloc::vec![MenuItem { label: String::from("Open"), icon: "ui/open", action: MenuAction::OpenDesktopItem(index), danger: false }];
                    if let DesktopItem::App(app) = self.desktop[index].item {
                        if self.pinned.contains(&app) {
                            items.push(MenuItem { label: String::from("Unpin from dock"), icon: "ui/unpin", action: MenuAction::UnpinApp(app), danger: false });
                        } else {
                            items.push(MenuItem { label: String::from("Pin to dock"), icon: "ui/pin", action: MenuAction::PinApp(app), danger: false });
                        }
                    }
                    items.push(MenuItem { label: String::from("Remove from desktop"), icon: "ui/trash", action: MenuAction::RemoveFromDesktop(index), danger: true });
                    items
                }
                None => {
                    let mut items = Vec::new();
                    if apps::by_role(ROLE_TERMINAL).is_some() {
                        items.push(MenuItem { label: String::from("Open Terminal"), icon: "ui/run", action: MenuAction::LaunchRole(ROLE_TERMINAL, None), danger: false });
                    }
                    if apps::by_role(ROLE_FILES).is_some() {
                        items.push(MenuItem { label: String::from("Files"), icon: "ui/folder", action: MenuAction::LaunchRole(ROLE_FILES, Some(ui::home_dir())), danger: false });
                    }
                    if apps::by_role(ROLE_SETTINGS).is_some() {
                        items.push(MenuItem { label: String::from("Change wallpaper…"), icon: "ui/wallpaper", action: MenuAction::LaunchRole(ROLE_SETTINGS, Some(String::from("wallpaper"))), danger: false });
                        items.push(MenuItem { label: String::from("Display settings…"), icon: "ui/display", action: MenuAction::LaunchRole(ROLE_SETTINGS, Some(String::from("display"))), danger: false });
                    }
                    items.push(MenuItem { label: String::from("Arrange icons"), icon: "ui/apps", action: MenuAction::ArrangeIcons, danger: false });
                    items.push(MenuItem { label: String::from("About HamixOS"), icon: "ui/info", action: MenuAction::About, danger: false });
                    items
                }
            };
            self.set_popup(Popup::Menu { x, y, items });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn drag_icon(&mut self, index: usize, dx: i32, dy: i32, start_x: i32, start_y: i32, moved: bool, x: i32, y: i32) {
        if !moved && (x - start_x).abs() < 5 && (y - start_y).abs() < 5 {
            return;
        }
        if index >= self.desktop.len() {
            self.grab = Grab::None;
            return;
        }
        let old = self.desktop_area(index);
        let nx = (x - dx).clamp(0, self.core.screen.width - ICON_CELL_W);
        let ny = (y - dy).clamp(TOP_H, self.core.screen.height - ICON_CELL_H);
        self.desktop[index].x = nx;
        self.desktop[index].y = ny;
        self.core.add_damage(old);
        let new = self.desktop_area(index);
        self.core.add_damage(new);
        self.grab = Grab::IconDrag { index, dx, dy, start_x, start_y, moved: true };
    }

    pub fn snap_icon(&mut self, index: usize) {
        if index >= self.desktop.len() {
            return;
        }
        let old = self.desktop_area(index);
        let icon = &mut self.desktop[index];
        icon.x = (icon.x + GRID / 2) / GRID * GRID;
        icon.y = ((icon.y + GRID / 2) / GRID * GRID).max(TOP_H);
        self.core.add_damage(old);
        let new = self.desktop_area(index);
        self.core.add_damage(new);
    }

    pub fn draw_desktop_icons(&mut self) {
        let dragging = match self.grab {
            Grab::IconDrag { index, moved: true, .. } => Some(index),
            _ => None,
        };
        for index in 0..self.desktop.len() {
            let area = self.desktop_area(index);
            if !area.overlaps(&self.core.screen.clip) {
                continue;
            }
            let (label, icon) = match self.desktop[index].item {
                DesktopItem::App(app) => (apps::get(app).name.as_str(), format!("apps/{}", apps::get(app).key)),
                DesktopItem::Home => ("Home", String::from("apps/folder-home")),
            };
            let hover = self.desktop_hover == Some(index);
            let selected = self.desktop_selected == Some(index);
            let ui = self.ui();
            let mut p = self.core.painter_clipped();
            if selected {
                p.rounded(area, 10, theme::accent(), if dragging == Some(index) { 110 } else { 70 });
                p.rounded_border(area, 10, theme::accent(), 140);
            } else if hover {
                p.rounded(area, 10, 0xffffff, 26);
            }
            if let Some(img) = ui.icon(&icon) {
                p.image(img, area.x + (area.w - img.w) / 2, area.y + 8, 255);
            }
            let mut lines: Vec<String> = Vec::new();
            if ui.font.measure(label) <= area.w - 8 {
                lines.push(String::from(label));
            } else {
                let words: Vec<&str> = label.split(' ').collect();
                let mut first = String::new();
                let mut split = words.len();
                for (i, word) in words.iter().enumerate() {
                    let candidate = if first.is_empty() { String::from(*word) } else { format!("{} {}", first, word) };
                    if ui.font.measure(&candidate) > area.w - 8 && !first.is_empty() {
                        split = i;
                        break;
                    }
                    first = candidate;
                }
                lines.push(ellipsize(&ui.font, &first, area.w - 8));
                if split < words.len() {
                    lines.push(ellipsize(&ui.font, &words[split..].join(" "), area.w - 8));
                }
            }
            for (i, text) in lines.iter().enumerate() {
                let tw = ui.font.measure(text);
                let tx = area.x + (area.w - tw) / 2;
                let ty = area.y + 61 + i as i32 * 17;
                p.text_alpha(&ui.font, tx + 1, ty + 1, text, 0x000000, 160);
                p.text(&ui.font, tx, ty, text, 0xffffff);
            }
        }
    }
}
