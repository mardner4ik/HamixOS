use alloc::format;
use alloc::vec::Vec;
use hxclient::ui::theme;
use vellum::gfx::{blend, ellipsize, fast_sqrt};
use vellum::{Area, Painter};

use crate::compositor::damage::subtract_all;
use crate::compositor::window::{Body, Cursor, SHADOW_OFFSET, SHADOW_SIZE, TITLE_H};
use crate::Nook;

impl Nook {
    pub fn draw_window(&mut self, index: usize, focused: bool) {
        let clip = self.core.screen.clip;
        let window_area = self.core.windows[index].with_shadow();
        if !window_area.overlaps(&clip) {
            return;
        }
        let pointer = self.core.pointer;
        if self.core.windows[index].is_popup() {
            let outer = self.core.windows[index].outer();
            {
                let mut p = self.core.painter_clipped();
                p.shadow(outer, 0, SHADOW_SIZE, 150, SHADOW_OFFSET);
            }
            let (width, height) = (self.core.screen.width, self.core.screen.height);
            if let Body::Client { pixels, width: bw, height: bh, .. } = &self.core.windows[index].body {
                let (w, h) = (*bw, *bh);
                let source = unsafe { core::slice::from_raw_parts(*pixels, (w * h) as usize) };
                let area = clip.intersect(&outer).intersect(&Area::new(outer.x, outer.y, w, h));
                for ty in area.y..area.bottom() {
                    let srow = ((ty - outer.y) * w) as usize;
                    let drow = (ty * width) as usize;
                    for tx in area.x..area.right() {
                        let p = source[srow + (tx - outer.x) as usize];
                        let a = p >> 24;
                        let d = &mut self.core.screen.frame[drow + tx as usize];
                        if a == 255 {
                            *d = p & 0x00FF_FFFF;
                        } else if a != 0 {
                            let inv = 255 - a;
                            let mix = |shift: u32| ((((p >> shift) & 0xFF) + (((*d >> shift) & 0xFF) * inv) / 255).min(255)) << shift;
                            *d = mix(16) | mix(8) | mix(0);
                        }
                    }
                }
                let _ = height;
            }
            return;
        }
        let (outer, content, close, maximize, minimize, title, radius, resizable, tiled) = {
            let w = &self.core.windows[index];
            (w.outer(), w.content(), w.close_button(), w.maximize_button(), w.minimize_button(), w.title.clone(), w.radius(), w.resizable(), w.tiled.is_some())
        };
        let frameless = self.core.windows[index].frameless;
        let icon_name = format!("apps/{}-24", self.core.windows[index].icon);
        let has_icon = self.sized_icon(&icon_name, 18).is_some();
        let icon = if has_icon { self.sized_icons.get(&(icon_name.clone(), 18)).map(|i| i as *const vellum::Image) } else { None };
        let ui = self.ui();
        let mut hot = Vec::new();
        {
            let mut p = self.core.painter_clipped();
            if !tiled {
                p.shadow(outer, radius, SHADOW_SIZE, if focused { 235 } else { 140 }, SHADOW_OFFSET);
            }
            let pal = theme::palette();
            let bar = if focused { pal.title_bar } else { pal.title_bar_inactive };
            if !frameless {
                if radius > 0 {
                    p.rounded(Area::new(outer.x, outer.y, outer.w, TITLE_H + radius), radius, bar, 255);
                } else {
                    p.fill(Area::new(outer.x, outer.y, outer.w, TITLE_H), bar);
                }
                p.fill(Area::new(outer.x, outer.y + TITLE_H - 1, outer.w, 1), pal.title_line);
                let mut text_left = outer.x + 14;
                if let Some(img) = icon {
                    let img = unsafe { &*img };
                    p.image(img, outer.x + 12, outer.y + (TITLE_H - img.h) / 2, if focused { 255 } else { 170 });
                    text_left += 26;
                }
                let max = minimize.x - text_left - 12;
                let shown = ellipsize(&ui.medium, &title, max);
                let tw = ui.medium.measure(&shown);
                let tx = (outer.x + (outer.w - tw) / 2).max(text_left).min(minimize.x - 12 - tw);
                p.text(&ui.medium, tx, outer.y + (TITLE_H - ui.medium.height()) / 2, &shown, if focused { theme::text() } else { theme::faint() });
                let mut buttons: Vec<(Area, &str, bool)> = alloc::vec![(minimize, "ui/minimize", false)];
                if resizable {
                    buttons.push((maximize, if tiled { "ui/restore" } else { "ui/maximize" }, false));
                }
                buttons.push((close, "ui/close", true));
                for (area, name, danger) in buttons {
                    let is_hot = area.contains(pointer.0, pointer.1);
                    if is_hot {
                        p.rounded(area, 6, if danger { theme::danger() } else { theme::hover() }, 255);
                    }
                    if let Some(img) = ui.icon(name) {
                        p.image_tinted(img, area.x + (area.w - img.w) / 2, area.y + (area.h - img.h) / 2, if is_hot && danger { 0xffffff } else if focused { theme::dim() } else { theme::faint() }, 255);
                    }
                    hot.push(area);
                }
            }
        }
        self.hot_buttons.extend(hot);

        let corner = radius;
        let mut saved: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
        let corners = [(content.x, content.bottom() - corner), (content.right() - corner, content.bottom() - corner)];
        if corner > 0 {
            for (i, (cx, cy)) in corners.iter().enumerate() {
                let square = Area::new(*cx, *cy, corner, corner);
                if !square.overlaps(&clip) {
                    continue;
                }
                for yy in 0..corner {
                    for xx in 0..corner {
                        let (px, py) = (cx + xx, cy + yy);
                        if px >= 0 && py >= 0 && px < self.core.screen.width && py < self.core.screen.height {
                            saved[i].push(self.core.screen.frame[(py * self.core.screen.width + px) as usize]);
                        } else {
                            saved[i].push(0);
                        }
                    }
                }
            }
        }

        let (width, height) = (self.core.screen.width, self.core.screen.height);
        let stretch_ok = self.stretch_allowed(index, hamix_std::sys::uptime_ms());
        match &mut self.core.windows[index].body {
            Body::Client { pixels, width: bw, height: bh, .. } => {
                let (w, h) = (*bw, *bh);
                let source = unsafe { core::slice::from_raw_parts(*pixels, (w * h) as usize) };
                if (w, h) != (content.w, content.h) && w > 0 && h > 0 && stretch_ok {
                    crate::anim::scaled_block(source, (w, h), content, 255, &mut self.core.screen.frame, width, clip.intersect(&content));
                } else {
                    let mut p = Painter::new(&mut self.core.screen.frame, width, height);
                    p.set_clip(clip.intersect(&content));
                    p.blit(source, w, h, content.x, content.y);
                    if w < content.w {
                        p.fill(Area::new(content.x + w, content.y, content.w - w, content.h), theme::bg());
                    }
                    if h < content.h {
                        p.fill(Area::new(content.x, content.y + h, w.min(content.w), content.h - h), theme::bg());
                    }
                }
            }
            Body::About => {
                let mut p = Painter::new(&mut self.core.screen.frame, width, height);
                p.set_clip(clip);
                Nook::draw_about_window(&mut p, ui, content);
            }
            body @ Body::Auth { .. } => {
                let mut p = Painter::new(&mut self.core.screen.frame, width, height);
                p.set_clip(clip);
                Nook::draw_auth_window(&mut p, ui, content, body, pointer);
                self.hot_buttons.push(Area::new(content.right() - 112, content.bottom() - 52, 96, 36));
                self.hot_buttons.push(Area::new(content.right() - 216, content.bottom() - 52, 96, 36));
            }
        }

        if corner > 0 {
            let rf = corner as f32;
            for (i, (cx, cy)) in corners.iter().enumerate() {
                if saved[i].is_empty() {
                    continue;
                }
                for yy in 0..corner {
                    for xx in 0..corner {
                        let (px, py) = (cx + xx, cy + yy);
                        if !clip.contains(px, py) {
                            continue;
                        }
                        let dx = if i == 0 { rf - xx as f32 - 0.5 } else { xx as f32 + 0.5 };
                        let dy = yy as f32 + 0.5;
                        if dx <= 0.0 {
                            continue;
                        }
                        let d = fast_sqrt(dx * dx + dy * dy);
                        let coverage = (rf - d + 0.5).clamp(0.0, 1.0);
                        let outside = 1.0 - coverage;
                        if outside <= 0.0 {
                            continue;
                        }
                        let idx = (py * self.core.screen.width + px) as usize;
                        let under = saved[i][(yy * corner + xx) as usize];
                        self.core.screen.frame[idx] = blend(self.core.screen.frame[idx], under, (outside * 255.0) as u32);
                    }
                }
            }
            let mut p = self.core.painter_clipped();
            let pal = theme::palette();
            p.rounded_border(outer, radius, pal.edge, if focused { pal.edge_alpha } else { pal.edge_alpha * 2 / 3 });
        }
    }

    fn draw_snap_preview(&mut self) {
        let Some(area) = self.snap_preview else {
            return;
        };
        if !area.overlaps(&self.core.screen.clip) {
            return;
        }
        let mut p = self.core.painter_clipped();
        let inner = area.inset(6);
        p.rounded(inner, 12, theme::accent(), 60);
        p.rounded_border(inner, 12, theme::accent_hover(), 180);
    }

    fn draw_cursor(&mut self) {
        if self.core.hw_cursor {
            return;
        }
        let area = self.core.cursor_area();
        if !area.overlaps(&self.core.screen.clip) {
            return;
        }
        let kind = self.core.cursor_kind.min(self.core.cursors.len() - 1);
        let cursor = unsafe { &*(&self.core.cursors[kind] as *const Cursor) };
        let mut p = self.core.painter_clipped();
        p.image(&cursor.image, area.x, area.y, 255);
    }

    fn visible_plan(&self, clip: Area) -> (Vec<(usize, Vec<Area>)>, Vec<Area>) {
        let mut uncovered: Vec<Area> = alloc::vec![clip];
        let mut plan: Vec<(usize, Vec<Area>)> = Vec::new();
        for i in (0..self.core.windows.len()).rev() {
            if uncovered.is_empty() {
                break;
            }
            let w = &self.core.windows[i];
            if w.minimized || self.animating(w.id) {
                continue;
            }
            let bounds = w.with_shadow();
            let parts: Vec<Area> = uncovered.iter().map(|r| r.intersect(&bounds)).filter(|r| !r.is_empty()).collect();
            if parts.is_empty() {
                continue;
            }
            plan.push((i, parts));
            if uncovered.len() < MAX_REGION_RECTS {
                for hole in w.opaque_regions() {
                    uncovered = subtract_all(&uncovered, &hole);
                }
            }
        }
        plan.reverse();
        (plan, uncovered)
    }

    pub fn compose(&mut self) {
        let now = hamix_std::sys::uptime_ms();
        let damage = self.core.damage.take();
        let focused = self.core.focused();
        self.hot_buttons.clear();
        for area in damage {
            let full = area.intersect(&self.core.bounds());
            if full.is_empty() {
                continue;
            }
            let (plan, uncovered) = self.visible_plan(full);
            for rect in uncovered.iter() {
                self.core.screen.clip = *rect;
                let (w, h) = (self.core.screen.width, self.core.screen.height);
                let mut p = Painter::new(&mut self.core.screen.frame, w, h);
                p.set_clip(*rect);
                p.copy_area(&self.background, *rect);
                drop(p);
                self.draw_desktop_icons();
            }
            for (index, parts) in plan.iter() {
                let is_focused = focused == Some(self.core.windows[*index].id);
                for rect in parts.iter() {
                    self.core.screen.clip = *rect;
                    self.draw_window(*index, is_focused);
                }
            }
            self.core.screen.clip = full;
            self.draw_animations(now);
            self.draw_snap_preview();
            self.draw_dock();
            self.draw_top_bar();
            self.draw_popup();
            self.draw_toasts();
            self.draw_volume_osd();
            self.draw_switcher();
            self.draw_cursor();
            let clip = self.core.screen.clip;
            self.core.screen.present(clip);
        }
        self.core.screen.finish_frame();
        self.core.screen.clip = self.core.bounds();
        self.last_compose = now;
        self.core.send_placements();
    }
}

const MAX_REGION_RECTS: usize = 48;
