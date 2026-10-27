use hxclient::ui::{self, theme, Ui};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

pub const ROW_H: i32 = 60;
pub const CONTROL_H: i32 = 34;
pub const GAP: i32 = 20;
pub const PAD: i32 = 18;

pub struct Flow {
    pub x: i32,
    pub width: i32,
    pub y: i32,
    pub top: i32,
}

impl Flow {
    pub fn new(area: Area, scroll: i32) -> Flow {
        Flow { x: area.x, width: area.w, y: area.y - scroll, top: area.y - scroll }
    }

    pub fn heading(&mut self, p: &mut Painter, ui: &Ui, title: &str) {
        p.text(&ui.medium, self.x, self.y, title, theme::dim());
        self.y += 26;
    }

    pub fn note(&mut self, p: &mut Painter, ui: &Ui, text: &str) {
        p.text(&ui.small, self.x, self.y, &ellipsize(&ui.small, text, self.width), theme::faint());
        self.y += 22;
    }

    pub fn gap(&mut self, amount: i32) {
        self.y += amount;
    }

    pub fn card(&mut self, p: &mut Painter, rows: i32) -> Area {
        let area = Area::new(self.x, self.y, self.width, rows * ROW_H);
        ui::card(p, area);
        self.y = area.bottom() + GAP;
        area
    }

    pub fn block(&mut self, p: &mut Painter, height: i32) -> Area {
        let area = Area::new(self.x, self.y, self.width, height);
        ui::card(p, area);
        self.y = area.bottom() + GAP;
        area
    }

    pub fn free(&mut self, height: i32) -> Area {
        let area = Area::new(self.x, self.y, self.width, height);
        self.y = area.bottom() + GAP;
        area
    }

    pub fn height(&self) -> i32 {
        self.y - self.top
    }
}

pub fn row_at(card: Area, index: i32) -> Area {
    Area::new(card.x, card.y + index * ROW_H, card.w, ROW_H)
}

pub fn row(p: &mut Painter, ui: &Ui, card: Area, index: i32, total: i32, label: &str, description: &str) -> Area {
    let area = row_at(card, index);
    let control_w = (area.w / 2).clamp(120, 320);
    let text_w = area.w - control_w - PAD * 2 - 12;
    p.text(&ui.medium, area.x + PAD, area.y + if description.is_empty() { (ROW_H - ui.medium.height()) / 2 } else { 12 }, &ellipsize(&ui.medium, label, text_w), theme::text());
    if !description.is_empty() {
        p.text(&ui.small, area.x + PAD, area.y + 34, &ellipsize(&ui.small, description, text_w), theme::faint());
    }
    if index + 1 < total {
        ui::separator(p, area.x + PAD, area.bottom() - 1, area.w - PAD * 2);
    }
    Area::new(area.right() - PAD - control_w, area.y + (ROW_H - CONTROL_H) / 2, control_w, CONTROL_H)
}

pub fn value_row(p: &mut Painter, ui: &Ui, card: Area, index: i32, total: i32, label: &str, value: &str) {
    let area = row_at(card, index);
    let label_w = (area.w / 3).clamp(110, 220);
    p.text(&ui.font, area.x + PAD, area.y + (ROW_H - ui.font.height()) / 2, label, theme::faint());
    let room = area.w - label_w - PAD * 2;
    p.text(&ui.font, area.x + PAD + label_w, area.y + (ROW_H - ui.font.height()) / 2, &ellipsize(&ui.font, value, room), theme::text());
    if index + 1 < total {
        ui::separator(p, area.x + PAD, area.bottom() - 1, area.w - PAD * 2);
    }
}

pub fn segmented<T: Copy + PartialEq>(p: &mut Painter, ui: &Ui, area: Area, options: &[(&str, T)], selected: T, hover: Option<T>) -> alloc::vec::Vec<(Area, T)> {
    let mut out = alloc::vec::Vec::new();
    if options.is_empty() {
        return out;
    }
    let count = options.len() as i32;
    let gap = 6;
    let width = ((area.w - gap * (count - 1)) / count).max(40);
    for (i, (label, value)) in options.iter().enumerate() {
        let cell = Area::new(area.right() - (count - i as i32) * (width + gap) + gap, area.y, width, area.h);
        let active = *value == selected;
        let background = if active {
            theme::accent()
        } else if hover == Some(*value) {
            theme::hover()
        } else {
            theme::surface_2()
        };
        p.rounded(cell, 8, background, 255);
        ui::centered_text(p, &ui.font, cell, label, if active { theme::accent_text() } else { theme::text() });
        out.push((cell, *value));
    }
    out
}
