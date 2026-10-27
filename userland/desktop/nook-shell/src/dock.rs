use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::sys::{MOUSE_LEFT, MOUSE_RIGHT};
use hxclient::ui::theme;
use vellum::gfx::ellipsize;
use vellum::Area;

use crate::config::{self, DOCK_LIMIT_MIN, DOCK_SIZE_MAX, DOCK_SIZE_MIN};
use crate::{app_by_icon, apps, Grab, MenuAction, MenuItem, Nook, Popup, DOCK_MARGIN};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DockItem {
    App(usize),
    Window(u32),
    More,
}

const GROUP_GAP: i32 = 12;
const OVERFLOW_ROW: i32 = 40;
const OVERFLOW_W: i32 = 260;

impl Nook {
    pub fn dock_items(&self) -> Vec<DockItem> {
        let mut items: Vec<DockItem> = self.pinned.iter().map(|a| DockItem::App(*a)).collect();
        for w in self.core.windows.iter().filter(|w| !w.is_popup() && w.parent.is_none()) {
            match app_by_icon(&w.icon) {
                Some(app) => {
                    if !items.contains(&DockItem::App(app)) {
                        items.push(DockItem::App(app));
                    }
                }
                None => items.push(DockItem::Window(w.id)),
            }
        }
        items
    }

    pub fn dock_visible_items(&self) -> Vec<DockItem> {
        let mut items = self.dock_items();
        let limit = self.dock_limit.max(DOCK_LIMIT_MIN);
        if items.len() > limit {
            items.truncate(limit - 1);
            items.push(DockItem::More);
        }
        items
    }

    pub fn dock_overflow(&self) -> Vec<DockItem> {
        let items = self.dock_items();
        let limit = self.dock_limit.max(DOCK_LIMIT_MIN);
        if items.len() > limit { items[limit - 1..].to_vec() } else { Vec::new() }
    }

    fn dock_pinned_shown(&self, count: usize) -> usize {
        self.pinned.len().min(count)
    }

    fn dock_has_gap(&self, items: &[DockItem]) -> bool {
        let pinned = self.dock_pinned_shown(items.len());
        pinned > 0 && items.len() > pinned
    }

    pub fn dock_icon(&self) -> i32 {
        let items = self.dock_visible_items();
        let count = items.len().max(1) as i32;
        let gap = if self.dock_has_gap(&items) { GROUP_GAP } else { 0 };
        let room = self.core.screen.width - 2 * DOCK_MARGIN - gap;
        let mut size = self.dock_size.clamp(DOCK_SIZE_MIN, DOCK_SIZE_MAX);
        while size > 24 && count * (size + Self::pad_for(size)) + Self::pad_for(size) > room {
            size -= 2;
        }
        size
    }

    fn pad_for(icon: i32) -> i32 {
        (icon / 5).clamp(6, 14)
    }

    pub fn dock_pad(&self) -> i32 {
        Self::pad_for(self.dock_icon())
    }

    pub fn dock_h(&self) -> i32 {
        self.dock_icon() + 2 * self.dock_pad()
    }

    fn dock_hidden_offset(&self) -> i32 {
        self.dock_h() + DOCK_MARGIN + 8
    }

    pub fn dock_shown_area(&self) -> Area {
        let items = self.dock_visible_items();
        let (icon, pad) = (self.dock_icon(), self.dock_pad());
        let gap = if self.dock_has_gap(&items) { GROUP_GAP } else { 0 };
        let w = items.len().max(1) as i32 * (icon + pad) + pad + gap;
        let h = icon + 2 * pad;
        Area::new((self.core.screen.width - w) / 2, self.core.screen.height - h - DOCK_MARGIN, w, h)
    }

    pub fn dock_area(&self) -> Area {
        let a = self.dock_shown_area();
        Area::new(a.x, a.y + self.dock_offset, a.w, a.h)
    }

    pub fn dock_region(&self) -> Area {
        let a = self.dock_shown_area();
        let top = a.y - 48 - (DOCK_SIZE_MAX - self.dock_icon()).max(0);
        Area::new(0, top, self.core.screen.width, self.core.screen.height - top)
    }

    pub fn dock_visible(&self) -> bool {
        self.dock_offset < self.dock_hidden_offset()
    }

    pub fn dock_slot_area(&self, index: usize, shown: bool) -> Area {
        let dock = if shown { self.dock_shown_area() } else { self.dock_area() };
        let items = self.dock_visible_items();
        let (icon, pad) = (self.dock_icon(), self.dock_pad());
        let gap = if self.dock_has_gap(&items) && index >= self.dock_pinned_shown(items.len()) { GROUP_GAP } else { 0 };
        Area::new(dock.x + pad + index as i32 * (icon + pad) + gap, dock.y + pad, icon, icon)
    }

    pub fn dock_item_area(&self, index: usize) -> Area {
        self.dock_slot_area(index, false)
    }

    fn dock_anchored_popup(&self) -> bool {
        match &self.popup {
            Popup::Menu { .. } => self.popup_dock,
            Popup::DockOverflow => true,
            _ => false,
        }
    }

    fn dock_wanted_hidden(&self) -> bool {
        if !self.autohide || matches!(self.grab, Grab::DockDrag { .. }) || self.dock_anchored() || self.dock_anchored_popup() {
            return false;
        }
        let (px, py) = self.core.pointer;
        let shown = self.dock_shown_area();
        if py >= self.core.screen.height - 2 {
            return false;
        }
        if self.dock_offset < self.dock_hidden_offset() / 2 && shown.expand(6).contains(px, py) && !matches!(self.grab, Grab::Move { .. } | Grab::Resize { .. }) {
            return false;
        }
        let zone = shown.expand(4);
        self.core.windows.iter().any(|w| !w.minimized && w.outer().overlaps(&zone))
    }

    pub fn dock_animating(&self) -> bool {
        let target = if self.dock_wanted_hidden() { self.dock_hidden_offset() } else { 0 };
        target != self.dock_offset
    }

    pub fn update_dock_visibility(&mut self) {
        let hidden = self.dock_hidden_offset();
        if self.dock_offset > hidden {
            self.dock_offset = hidden;
        }
        let target = if self.dock_wanted_hidden() { hidden } else { 0 };
        if target == self.dock_offset {
            return;
        }
        let distance = target - self.dock_offset;
        let step = (distance.abs() / 3).max(6).min(distance.abs());
        self.dock_offset += step * distance.signum();
        let region = self.dock_region();
        self.core.add_damage(region);
        if !self.dock_visible() {
            self.dock_hover = None;
        }
    }

    pub fn dock_hit(&self, x: i32, y: i32) -> Option<usize> {
        if !self.dock_visible() || !self.dock_area().contains(x, y) {
            return None;
        }
        (0..self.dock_visible_items().len()).find(|i| self.dock_item_area(*i).expand(self.dock_pad() / 2).contains(x, y))
    }

    pub fn activate_dock_item(&mut self, item: DockItem) {
        let target = match item {
            DockItem::App(app) => {
                let ids = self.app_windows(app);
                if ids.is_empty() {
                    self.launch(app, &[]);
                    return;
                }
                let focused = self.core.focused();
                if ids.len() > 1 && focused.map(|f| ids.contains(&f)).unwrap_or(false) {
                    let position = ids.iter().position(|id| Some(*id) == focused).unwrap_or(0);
                    let next = ids[(position + ids.len() - 1) % ids.len()];
                    self.focus(next);
                    return;
                }
                ids.last().copied()
            }
            DockItem::Window(id) => Some(id),
            DockItem::More => {
                if matches!(self.popup, Popup::DockOverflow) {
                    self.set_popup(Popup::None);
                } else {
                    self.set_popup(Popup::DockOverflow);
                }
                return;
            }
        };
        if let Some(id) = target {
            let focused = self.core.focused() == Some(id);
            if let Some(i) = self.core.index_of(id) {
                if focused && !self.core.windows[i].minimized {
                    self.minimize(id);
                } else {
                    self.focus(id);
                }
            }
        }
    }

    pub fn activate_dock(&mut self, index: usize) {
        if let Some(&item) = self.dock_visible_items().get(index) {
            self.activate_dock_item(item);
        }
    }

    fn dock_settings_items(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { label: String::from(if self.autohide { "Always show the dock" } else { "Hide the dock under windows" }), icon: "ui/display", action: MenuAction::ToggleAutohide, danger: false });
    }

    pub fn app_menu_items(&self, app: usize) -> Vec<MenuItem> {
        let mut items = Vec::new();
        let windows = self.app_windows(app);
        items.push(MenuItem { label: String::from(if windows.is_empty() { "Open" } else { "New window" }), icon: "ui/open", action: MenuAction::Launch(app), danger: false });
        if self.pinned.contains(&app) {
            items.push(MenuItem { label: String::from("Unpin from dock"), icon: "ui/unpin", action: MenuAction::UnpinApp(app), danger: false });
        } else {
            items.push(MenuItem { label: String::from("Pin to dock"), icon: "ui/pin", action: MenuAction::PinApp(app), danger: false });
        }
        if !self.desktop.iter().any(|d| d.item == crate::DesktopItem::App(app)) {
            items.push(MenuItem { label: String::from("Add to desktop"), icon: "ui/plus", action: MenuAction::AddToDesktop(app), danger: false });
        }
        if !windows.is_empty() {
            let label = if windows.len() > 1 { format!("Close {} windows", windows.len()) } else { String::from("Close") };
            items.push(MenuItem { label, icon: "ui/close", action: MenuAction::CloseApp(app), danger: true });
        }
        items
    }

    pub fn dock_press(&mut self, x: i32, y: i32, button: u32) {
        let hit = self.dock_hit(x, y);
        if button == MOUSE_LEFT {
            let Some(index) = hit else {
                return;
            };
            if index < self.dock_pinned_shown(self.dock_visible_items().len()) {
                self.grab = Grab::DockDrag { index, start_x: x, moved: false };
            } else {
                self.activate_dock(index);
            }
            return;
        }
        if button != MOUSE_RIGHT {
            return;
        }
        let item = hit.and_then(|i| self.dock_visible_items().get(i).copied());
        let mut items = match item {
            Some(DockItem::App(app)) => self.app_menu_items(app),
            Some(DockItem::Window(id)) => alloc::vec![MenuItem { label: String::from("Close"), icon: "ui/close", action: MenuAction::CloseWindow(id), danger: true }],
            Some(DockItem::More) | None => Vec::new(),
        };
        self.dock_settings_items(&mut items);
        let anchor = hit.map(|i| self.dock_item_area(i)).unwrap_or_else(|| Area::new(x, y, 0, 0));
        let menu_h = items.len() as i32 * 34 + 12;
        self.set_popup(Popup::Menu { x: anchor.x + anchor.w / 2 - 118, y: self.dock_area().y - menu_h - 8, items });
        self.popup_dock = true;
    }

    pub fn drag_dock(&mut self, index: usize, start_x: i32, moved: bool, x: i32) {
        if !moved && (x - start_x).abs() < 6 {
            return;
        }
        let pinned = self.dock_pinned_shown(self.dock_visible_items().len());
        if pinned == 0 {
            return;
        }
        let slot = self.dock_icon() + self.dock_pad();
        let first = self.dock_item_area(0).x;
        let relative = x - first;
        let target = if relative < 0 { 0 } else { ((relative / slot.max(1)) as usize).min(pinned - 1) };
        if target != index && index < self.pinned.len() && target < self.pinned.len() {
            let app = self.pinned.remove(index);
            self.pinned.insert(target, app);
        }
        self.grab = Grab::DockDrag { index: target, start_x, moved: true };
        self.dock_hover = Some(target);
        let region = self.dock_region();
        self.core.add_damage(region);
    }

    pub fn pin_app(&mut self, app: usize, pin: bool) {
        let before = self.dock_region();
        if pin {
            if !self.pinned.contains(&app) {
                self.pinned.push(app);
            }
        } else {
            self.pinned.retain(|a| *a != app);
        }
        self.save_pinned();
        self.core.add_damage(before);
        let after = self.dock_region();
        self.core.add_damage(after);
    }

    pub fn save_pinned(&self) {
        let names: Vec<String> = self.pinned.iter().filter(|a| apps::alive(**a)).map(|a| apps::get(*a).key.clone()).collect();
        config::save_dock(&names);
    }

    fn dock_label(&self, item: DockItem) -> (String, String) {
        match item {
            DockItem::App(a) => (apps::get(a).name.clone(), format!("apps/{}", apps::get(a).key)),
            DockItem::Window(id) => {
                let w = self.core.index_of(id).map(|i| &self.core.windows[i]);
                let icon = w.map(|w| if w.icon.is_empty() { String::from(apps::GENERIC_LINUX_ICON) } else { w.icon.clone() }).unwrap_or_default();
                (w.map(|w| w.title.clone()).unwrap_or_default(), format!("apps/{}", icon))
            }
            DockItem::More => (format!("{} more", self.dock_overflow().len()), String::from("ui/apps")),
        }
    }

    fn dock_running(&self, item: DockItem) -> (usize, bool) {
        let focused = self.core.focused();
        match item {
            DockItem::App(a) => {
                let ids = self.app_windows(a);
                (ids.len(), ids.iter().any(|id| Some(*id) == focused))
            }
            DockItem::Window(id) => (1, Some(id) == focused),
            DockItem::More => {
                let hidden = self.dock_overflow();
                let running = hidden.iter().filter(|i| self.dock_running(**i).0 > 0).count();
                (running, hidden.iter().any(|i| self.dock_running(*i).1))
            }
        }
    }

    pub fn overflow_area(&self) -> Area {
        let rows = self.dock_overflow().len().max(1) as i32;
        let items = self.dock_visible_items();
        let anchor = items.iter().position(|i| *i == DockItem::More).map(|i| self.dock_item_area(i)).unwrap_or_else(|| self.dock_area());
        let max_rows = ((self.dock_area().y - crate::TOP_H - 24) / OVERFLOW_ROW).max(1);
        let h = rows.min(max_rows) * OVERFLOW_ROW + 16;
        let x = (anchor.x + anchor.w / 2 - OVERFLOW_W / 2).clamp(8, (self.core.screen.width - OVERFLOW_W - 8).max(8));
        Area::new(x, self.dock_area().y - h - 10, OVERFLOW_W, h)
    }

    pub fn overflow_row(&self, index: usize) -> Area {
        let a = self.overflow_area();
        Area::new(a.x + 8, a.y + 8 + index as i32 * OVERFLOW_ROW, a.w - 16, OVERFLOW_ROW - 2)
    }

    pub fn overflow_visible_rows(&self) -> usize {
        ((self.overflow_area().h - 16) / OVERFLOW_ROW).max(0) as usize
    }

    pub fn overflow_press(&mut self, x: i32, y: i32, button: u32) {
        let hidden = self.dock_overflow();
        let rows = self.overflow_visible_rows();
        let Some(index) = (0..hidden.len().min(rows)).find(|i| self.overflow_row(*i).contains(x, y)) else {
            return;
        };
        let item = hidden[index];
        if button == MOUSE_RIGHT {
            if let DockItem::App(app) = item {
                let items = self.app_menu_items(app);
                self.set_popup(Popup::Menu { x, y, items });
                self.popup_dock = true;
            }
            return;
        }
        self.set_popup(Popup::None);
        self.activate_dock_item(item);
    }

    pub fn draw_overflow(&mut self, area: Area, hover: Option<usize>) {
        let hidden = self.dock_overflow();
        let rows = self.overflow_visible_rows().min(hidden.len());
        let entries: Vec<(String, String, (usize, bool))> = hidden.iter().take(rows).map(|i| {
            let (label, icon) = self.dock_label(*i);
            (label, icon, self.dock_running(*i))
        }).collect();
        for (_, icon, _) in &entries {
            self.sized_icon(icon, 24);
        }
        let scaled: Vec<Option<*const vellum::Image>> = entries.iter().map(|(_, icon, _)| self.sized_icons.get(&(icon.clone(), 24)).map(|i| i as *const vellum::Image)).collect();
        let row_areas: Vec<Area> = (0..rows).map(|i| self.overflow_row(i)).collect();
        let ui = self.ui();
        let mut p = self.core.painter_clipped();
        crate::panel::popup_frame(&mut p, area, 12);
        for (i, (label, _, (count, active))) in entries.iter().enumerate() {
            let row = row_areas[i];
            if hover == Some(i) {
                p.rounded(row, 8, theme::hover(), 255);
            }
            if let Some(img) = scaled[i].map(|ptr| unsafe { &*ptr }) {
                p.image(img, row.x + 10, row.y + (row.h - img.h) / 2, 255);
            }
            p.text(&ui.font, row.x + 44, row.y + (row.h - ui.font.height()) / 2, &ellipsize(&ui.font, label, row.w - 70), theme::text());
            if *count > 0 {
                p.circle((row.right() - 14) as f32, (row.y + row.h / 2) as f32, 3.0, if *active { theme::accent_hover() } else { theme::dim() }, 255);
            }
        }
    }

    pub fn draw_dock(&mut self) {
        if !self.dock_visible() {
            return;
        }
        let dock = self.dock_area();
        let region = self.dock_region();
        if !region.overlaps(&self.core.screen.clip) {
            return;
        }
        let items = self.dock_visible_items();
        let hover = self.dock_hover;
        let dragging = match self.grab {
            Grab::DockDrag { index, moved: true, .. } => Some(index),
            _ => None,
        };
        let areas: Vec<Area> = (0..items.len()).map(|i| self.dock_item_area(i)).collect();
        let running: Vec<(usize, bool)> = items.iter().map(|i| self.dock_running(*i)).collect();
        let labels: Vec<(String, String)> = items.iter().map(|i| self.dock_label(*i)).collect();
        for (i, (_, icon)) in labels.iter().enumerate() {
            if items[i] != DockItem::More {
                self.sized_icon(icon, areas[i].w);
            }
        }
        let scaled: Vec<Option<*const vellum::Image>> = labels
            .iter()
            .enumerate()
            .map(|(i, (_, icon))| if items[i] == DockItem::More { None } else { self.sized_icons.get(&(icon.clone(), areas[i].w)).map(|img| img as *const vellum::Image) })
            .collect();
        let ui = self.ui();
        let pinned = self.dock_pinned_shown(items.len());
        let pad = self.dock_pad();
        let radius = 18;
        let overflow_open = matches!(self.popup, Popup::DockOverflow);
        let mut p = self.core.painter_clipped();
        let pal = theme::palette();
        p.shadow(dock, radius, 20, if theme::is_light() { 110 } else { 170 }, 4);
        p.rounded(dock, radius, pal.dock, pal.dock_alpha);
        p.rounded_border(dock, radius, pal.edge, pal.edge_alpha + 4);
        if items.len() > pinned && pinned > 0 {
            let sep_x = areas[pinned].x - pad / 2 - GROUP_GAP / 2;
            p.fill(Area::new(sep_x, dock.y + 14, 1, dock.h - 28), pal.dock_line);
        }
        for (i, area) in areas.iter().enumerate() {
            let is_hover = hover == Some(i);
            if is_hover || dragging == Some(i) || (items[i] == DockItem::More && overflow_open) {
                p.rounded(area.expand(4), 12, pal.overlay, if dragging == Some(i) { 48 } else { 30 });
            }
            if items[i] == DockItem::More {
                let dots = area.w / 10;
                for k in -1..=1 {
                    p.circle((area.x + area.w / 2 + k * dots * 2) as f32, (area.y + area.h / 2) as f32, (dots as f32 * 0.7).max(2.0), pal.panel_text, 230);
                }
            } else if let Some(img) = scaled[i].map(|ptr| unsafe { &*ptr }) {
                if img.w == area.w && img.h == area.h {
                    p.image(img, area.x, area.y, 255);
                } else {
                    p.image_scaled(img, *area, 255);
                }
            }
            let (count, active) = running[i];
            if count > 0 {
                let cx = area.x as f32 + area.w as f32 / 2.0;
                let cy = (area.bottom() + pad / 2) as f32;
                if active {
                    p.rounded(Area::new(cx as i32 - 7, cy as i32 - 2, 14, 4), 2, theme::accent_hover(), 255);
                } else if count > 1 {
                    p.circle(cx - 4.0, cy, 2.0, pal.dock_dot, 230);
                    p.circle(cx + 4.0, cy, 2.0, pal.dock_dot, 230);
                } else {
                    p.circle(cx, cy, 2.2, pal.dock_dot, 230);
                }
            }
            if is_hover && dragging.is_none() {
                let label = ellipsize(&ui.font, &labels[i].0, 220);
                let tw = ui.font.measure(&label);
                let tip = Area::new(area.x + area.w / 2 - tw / 2 - 12, dock.y - 38, tw + 24, 28);
                p.rounded(tip, 8, pal.osd, 240);
                p.rounded_border(tip, 8, pal.edge, pal.edge_alpha + 2);
                p.text(&ui.font, tip.x + 12, tip.y + (tip.h - ui.font.height()) / 2, &label, pal.text);
            }
        }
    }
}
