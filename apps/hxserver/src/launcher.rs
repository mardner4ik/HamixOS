use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::sys::{self, MOUSE_LEFT, MOUSE_RIGHT};
use hxclient::ui::{self, theme, Style};
use vellum::gfx::ellipsize;
use vellum::Area;

use crate::apps::{self, CATEGORIES};
use crate::panel::popup_frame;
use crate::{DesktopItem, Grab, MenuAction, MenuItem, Nook, Popup, TOP_H};

const WIDTH: i32 = 292;
const ROW: i32 = 32;
const HEADER: i32 = 26;
const LIST_Y: i32 = 54;
const FOOTER_H: i32 = 48;
const ICON: i32 = 24;
const WHEEL_ROWS: i32 = 2;
const HOVER_LOGOUT: usize = 2000;
const HOVER_TRACK: usize = 3000;

#[derive(Clone, PartialEq)]
pub enum Entry {
    App(usize),
    Command(String),
}

#[derive(Clone, PartialEq)]
enum Row {
    Header(&'static str),
    Item(Entry),
}

impl Row {
    fn height(&self) -> i32 {
        match self {
            Row::Header(_) => HEADER,
            Row::Item(_) => ROW,
        }
    }
}

impl Nook {
    fn launcher_query(&self) -> String {
        String::from(self.search.text.trim()).to_lowercase()
    }

    fn launcher_rows(&self) -> Vec<Row> {
        let query = self.launcher_query();
        let visible = |i: &usize| self.live || !apps::get(*i).live_only;
        let mut rows = Vec::new();
        if query.is_empty() {
            let order = apps::launcher_order();
            for category in CATEGORIES {
                let members: Vec<usize> = order.iter().copied().filter(visible).filter(|i| apps::get(*i).category == category).collect();
                if members.is_empty() {
                    continue;
                }
                rows.push(Row::Header(category.label()));
                rows.extend(members.into_iter().map(|i| Row::Item(Entry::App(i))));
            }
            if !self.known_commands.is_empty() {
                rows.push(Row::Header("Commands"));
                rows.extend(self.known_commands.iter().cloned().map(|c| Row::Item(Entry::Command(c))));
            }
            return rows;
        }
        let mut scored: Vec<(u32, usize)> = apps::launcher_order()
            .into_iter()
            .filter(visible)
            .filter_map(|i| {
                let app = apps::get(i);
                let name = app.name.to_lowercase();
                if name.starts_with(&query) {
                    Some((0, i))
                } else if name.contains(&query) {
                    Some((1, i))
                } else if app.keywords.contains(&query) || app.description.to_lowercase().contains(&query) {
                    Some((2, i))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by_key(|(score, _)| *score);
        rows.extend(scored.into_iter().map(|(_, i)| Row::Item(Entry::App(i))));
        let commands: Vec<String> = self.known_commands.iter().filter(|c| c.to_lowercase().contains(&query)).cloned().collect();
        if !commands.is_empty() {
            rows.push(Row::Header("Commands"));
            rows.extend(commands.into_iter().map(|c| Row::Item(Entry::Command(c))));
        }
        rows
    }

    fn launcher_content_height(rows: &[Row]) -> i32 {
        rows.iter().map(|r| r.height()).sum::<i32>().max(ROW * 2)
    }

    pub fn launcher_area(&self) -> Area {
        let rows = self.launcher_rows();
        let max_h = self.core.screen.height - TOP_H - 16;
        let h = (LIST_Y + Self::launcher_content_height(&rows) + 8 + FOOTER_H).min(max_h).max(220.min(max_h));
        let w = WIDTH.min(self.core.screen.width - 16);
        Area::new(8, TOP_H + 8, w, h)
    }

    fn list_area(&self) -> Area {
        let a = self.launcher_area();
        Area::new(a.x + 6, a.y + LIST_Y, a.w - 12, a.h - LIST_Y - FOOTER_H - 4)
    }

    fn launcher_max_offset(&self) -> i32 {
        let rows = self.launcher_rows();
        (Self::launcher_content_height(&rows) - self.list_area().h).max(0)
    }

    fn row_areas(&self, rows: &[Row]) -> Vec<Area> {
        let list = self.list_area();
        let mut y = list.y - self.launcher_scroll as i32;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let h = row.height();
            out.push(Area::new(list.x, y, list.w - 8, h));
            y += h;
        }
        out
    }

    fn row_visible(&self, area: &Area) -> bool {
        let list = self.list_area();
        area.y >= list.y - 1 && area.bottom() <= list.bottom() + 1
    }

    fn launcher_logout(&self) -> Area {
        let a = self.launcher_area();
        Area::new(a.right() - 96, a.bottom() - FOOTER_H + 10, 84, 28)
    }

    fn launcher_track(&self) -> Area {
        let list = self.list_area();
        Area::new(list.right() - 5, list.y + 2, 4, list.h - 4)
    }

    fn launcher_thumb(&self) -> Option<Area> {
        let max = self.launcher_max_offset();
        if max == 0 {
            return None;
        }
        let track = self.launcher_track();
        let content = track.h + max;
        let h = (track.h * track.h / content.max(1)).max(24).min(track.h);
        let y = track.y + (track.h - h) * (self.launcher_scroll as i32).min(max) / max;
        Some(Area::new(track.x, y, track.w, h))
    }

    fn damage_launcher(&mut self) {
        let area = self.launcher_area();
        self.core.add_damage(Area::new(area.x - 32, area.y - 8, area.w + 64, self.core.screen.height - area.y + 8));
    }

    fn set_launcher_offset(&mut self, offset: i32) {
        let offset = offset.clamp(0, self.launcher_max_offset()) as usize;
        if offset != self.launcher_scroll {
            self.launcher_scroll = offset;
            self.damage_launcher();
        }
    }

    pub fn launcher_wheel(&mut self, wheel: i32) {
        let next = self.launcher_scroll as i32 + wheel * ROW * WHEEL_ROWS;
        self.set_launcher_offset(next);
    }

    pub fn drag_launcher_scroll(&mut self, y: i32, offset: i32) {
        let Some(thumb) = self.launcher_thumb() else {
            return;
        };
        let max = self.launcher_max_offset();
        let track = self.launcher_track();
        let room = (track.h - thumb.h).max(1);
        let position = (y - offset - track.y).clamp(0, room);
        self.set_launcher_offset(position * max / room);
    }

    pub fn reset_launcher(&mut self) {
        self.launcher_scroll = 0;
        self.launcher_focus = None;
    }

    fn reveal_row(&mut self, index: usize) {
        let rows = self.launcher_rows();
        let list = self.list_area();
        let top: i32 = rows.iter().take(index).map(|r| r.height()).sum();
        let mut header_top = top;
        if index > 0 {
            if let Some(Row::Header(_)) = rows.get(index - 1) {
                header_top -= HEADER;
            }
        }
        let bottom = top + rows.get(index).map(|r| r.height()).unwrap_or(ROW);
        let scroll = self.launcher_scroll as i32;
        if header_top < scroll {
            self.set_launcher_offset(header_top);
        } else if bottom > scroll + list.h {
            self.set_launcher_offset(bottom - list.h);
        }
    }

    fn open_entry(&mut self, entry: Entry) {
        self.set_popup(Popup::None);
        match entry {
            Entry::App(app) => self.launch(app, &[]),
            Entry::Command(name) => {
                if sys::cmd_run(&name, &[] as &[&str], sys::SPAWN_DETACH) < 0 {
                    self.toast("Command failed", &name, "ui/run");
                }
            }
        }
    }

    pub fn launcher_press(&mut self, x: i32, y: i32, button: u32) {
        let rows = self.launcher_rows();
        let areas = self.row_areas(&rows);
        for (i, row) in rows.iter().enumerate() {
            let Row::Item(entry) = row else {
                continue;
            };
            if !self.row_visible(&areas[i]) || !areas[i].contains(x, y) {
                continue;
            }
            match (button, entry) {
                (MOUSE_RIGHT, Entry::App(app)) => {
                    let app = *app;
                    let mut items = alloc::vec![MenuItem { label: String::from("Open"), icon: "ui/open", action: MenuAction::Launch(app), danger: false }];
                    if self.pinned.contains(&app) {
                        items.push(MenuItem { label: String::from("Unpin from dock"), icon: "ui/unpin", action: MenuAction::UnpinApp(app), danger: false });
                    } else {
                        items.push(MenuItem { label: String::from("Pin to dock"), icon: "ui/pin", action: MenuAction::PinApp(app), danger: false });
                    }
                    if !self.desktop.iter().any(|d| d.item == DesktopItem::App(app)) {
                        items.push(MenuItem { label: String::from("Add to desktop"), icon: "ui/plus", action: MenuAction::AddToDesktop(app), danger: false });
                    }
                    self.set_popup(Popup::Menu { x, y, items });
                }
                (MOUSE_LEFT, entry) => self.open_entry(entry.clone()),
                _ => {}
            }
            return;
        }
        if button != MOUSE_LEFT {
            return;
        }
        if let Some(thumb) = self.launcher_thumb() {
            if thumb.expand(4).contains(x, y) {
                self.grab = Grab::LauncherScroll { offset: y - thumb.y };
                return;
            }
            if self.launcher_track().expand(4).contains(x, y) {
                let page = self.list_area().h - ROW;
                let next = if y < thumb.y { self.launcher_scroll as i32 - page } else { self.launcher_scroll as i32 + page };
                self.set_launcher_offset(next);
                return;
            }
        }
        if self.launcher_logout().contains(x, y) {
            self.running = false;
        }
    }

    pub fn launcher_hover(&self, x: i32, y: i32) -> Option<usize> {
        let rows = self.launcher_rows();
        let areas = self.row_areas(&rows);
        if let Some(i) = (0..rows.len()).find(|i| matches!(rows[*i], Row::Item(_)) && self.row_visible(&areas[*i]) && areas[*i].contains(x, y)) {
            return Some(i);
        }
        if self.launcher_logout().contains(x, y) {
            return Some(HOVER_LOGOUT);
        }
        if self.launcher_thumb().is_some() && self.launcher_track().expand(4).contains(x, y) {
            return Some(HOVER_TRACK);
        }
        None
    }

    fn step_focus(&mut self, forward: bool) {
        let rows = self.launcher_rows();
        let items: Vec<usize> = (0..rows.len()).filter(|i| matches!(rows[*i], Row::Item(_))).collect();
        if items.is_empty() {
            return;
        }
        let position = self.launcher_focus.and_then(|f| items.iter().position(|i| *i == f));
        let next = match (position, forward) {
            (None, _) => items[0],
            (Some(p), true) => items[(p + 1).min(items.len() - 1)],
            (Some(p), false) => items[p.saturating_sub(1)],
        };
        self.launcher_focus = Some(next);
        self.reveal_row(next);
        self.damage_launcher();
    }

    pub fn launcher_key(&mut self, code: i32) {
        match code {
            27 => self.set_popup(Popup::None),
            10 => {
                let rows = self.launcher_rows();
                let focused = self.launcher_focus.and_then(|f| match rows.get(f) {
                    Some(Row::Item(entry)) => Some(entry.clone()),
                    _ => None,
                });
                let first = rows.iter().find_map(|r| match r {
                    Row::Item(entry) => Some(entry.clone()),
                    _ => None,
                });
                if let Some(entry) = focused.or(first) {
                    self.open_entry(entry);
                }
            }
            -1 => self.step_focus(false),
            -2 | 9 => self.step_focus(true),
            -8 | -9 => {
                let page = self.list_area().h - ROW;
                let next = if code == -8 { self.launcher_scroll as i32 - page } else { self.launcher_scroll as i32 + page };
                self.set_launcher_offset(next);
            }
            _ => {
                self.damage_launcher();
                self.search.key(code);
                self.reset_launcher();
                self.damage_launcher();
            }
        }
    }

    pub fn draw_launcher(&mut self, area: Area, hover: Option<usize>) {
        let rows = self.launcher_rows();
        let areas = self.row_areas(&rows);
        let list = self.list_area();
        let thumb = self.launcher_thumb();
        let track = self.launcher_track();
        let logout = self.launcher_logout();
        let user = self.user.clone();
        let focus = self.launcher_focus;
        let dragging = matches!(self.grab, Grab::LauncherScroll { .. });
        let icons: Vec<Option<*const vellum::Image>> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let Row::Item(entry) = row else {
                    return None;
                };
                if !areas[i].overlaps(&list) {
                    return None;
                }
                let name = match entry {
                    Entry::App(app) => format!("apps/{}-24", apps::get(*app).key),
                    Entry::Command(_) => String::from("apps/commands-24"),
                };
                self.sized_icon(&name, ICON).map(|img| img as *const vellum::Image)
            })
            .collect();
        let ui = self.ui();
        let mut search = core::mem::take(&mut self.search);
        {
            let mut p = self.core.painter_clipped();
            popup_frame(&mut p, area, 12);
            let field = Area::new(area.x + 10, area.y + 10, area.w - 20, 34);
            ui::text_field(&mut p, ui, field, &mut search, true, "Search");
            if let Some(img) = ui.icon("ui/search") {
                p.image_tinted(img, field.right() - 26, field.y + (field.h - img.h) / 2, theme::faint(), 255);
            }
            let previous_clip = p.clip;
            p.clip = previous_clip.intersect(&list);
            for (i, row) in rows.iter().enumerate() {
                let r = areas[i];
                if !r.overlaps(&list) {
                    continue;
                }
                match row {
                    Row::Header(label) => {
                        p.text(&ui.small, r.x + 10, r.bottom() - ui.small.height() - 5, &label.to_uppercase(), theme::faint());
                    }
                    Row::Item(entry) => {
                        if hover == Some(i) {
                            p.rounded(r, 7, theme::hover(), 255);
                        }
                        if focus == Some(i) {
                            p.rounded_border(r, 7, theme::accent_hover(), 255);
                        }
                        if let Some(img) = icons[i].map(|ptr| unsafe { &*ptr }) {
                            p.image(img, r.x + 8, r.y + (r.h - img.h) / 2, 255);
                        }
                        let name = match entry {
                            Entry::App(app) => apps::get(*app).name.clone(),
                            Entry::Command(name) => name.clone(),
                        };
                        let label = ellipsize(&ui.font, &name, r.w - 50);
                        p.text(&ui.font, r.x + 42, r.y + (r.h - ui.font.height()) / 2, &label, theme::text());
                    }
                }
            }
            p.clip = previous_clip;
            if rows.is_empty() {
                p.text(&ui.font, list.x + 10, list.y + 10, "Nothing matches your search", theme::faint());
            }
            if let Some(thumb) = thumb {
                p.rounded(track, 2, theme::border(), 255);
                let active = dragging || hover == Some(HOVER_TRACK);
                p.rounded(thumb, 2, if active { theme::accent_hover() } else { theme::dim() }, 255);
            }
            let footer = Area::new(area.x, area.bottom() - FOOTER_H, area.w, FOOTER_H);
            p.fill(Area::new(footer.x + 1, footer.y, footer.w - 2, 1), theme::border());
            if let Some(img) = ui.icon("ui/user") {
                p.image_tinted(img, footer.x + 16, footer.y + (footer.h - img.h) / 2, theme::dim(), 255);
            }
            let name = ellipsize(&ui.medium, &user, logout.x - footer.x - 50);
            p.text(&ui.medium, footer.x + 40, footer.y + (footer.h - ui.medium.height()) / 2, &name, theme::text());
            ui::button(&mut p, ui, logout, "Log out", Style::Secondary, hover == Some(HOVER_LOGOUT), true);
        }
        self.search = search;
    }
}
