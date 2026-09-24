use alloc::vec::Vec;
use hamix_std::sys;
use hxclient::ui;
use vellum::gfx::blend;
use vellum::Area;

use crate::Nook;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Open,
    Close,
    Minimize,
    Restore,
    Snap,
}

impl Effect {
    pub fn key(self) -> &'static str {
        match self {
            Effect::Open => "anim_open",
            Effect::Close => "anim_close",
            Effect::Minimize => "anim_minimize",
            Effect::Restore => "anim_restore",
            Effect::Snap => "anim_snap",
        }
    }

    pub fn choices(self) -> &'static [Kind] {
        match self {
            Effect::Open | Effect::Close => &[Kind::Appear, Kind::Disabled],
            Effect::Snap => &[Kind::Morph, Kind::Disabled],
            _ => &[Kind::MagicLamp, Kind::Disabled],
        }
    }

    pub fn default_kind(self) -> Kind {
        self.choices()[0]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Disabled,
    MagicLamp,
    Appear,
    Morph,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Disabled => "disabled",
            Kind::MagicLamp => "magic-lamp",
            Kind::Appear => "appear",
            Kind::Morph => "morph",
        }
    }

    pub fn from_name(value: &str) -> Option<Kind> {
        match value {
            "disabled" | "none" | "off" => Some(Kind::Disabled),
            "magic-lamp" | "lamp" => Some(Kind::MagicLamp),
            "appear" | "fade" | "scale" => Some(Kind::Appear),
            "morph" | "zoom" => Some(Kind::Morph),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Prefs {
    pub open: Kind,
    pub close: Kind,
    pub minimize: Kind,
    pub restore: Kind,
    pub snap: Kind,
    pub speed: u32,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { open: Kind::Appear, close: Kind::Appear, minimize: Kind::MagicLamp, restore: Kind::MagicLamp, snap: Kind::Morph, speed: 100 }
    }
}

impl Prefs {
    pub fn load() -> Prefs {
        let entries = ui::read_nook_config();
        let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let kind = |effect: Effect| {
            get(effect.key())
                .and_then(|v| Kind::from_name(v.trim()))
                .filter(|k| effect.choices().contains(k))
                .unwrap_or_else(|| effect.default_kind())
        };
        Prefs {
            open: kind(Effect::Open),
            close: kind(Effect::Close),
            minimize: kind(Effect::Minimize),
            restore: kind(Effect::Restore),
            snap: kind(Effect::Snap),
            speed: get("anim_speed").and_then(|v| v.trim().parse().ok()).unwrap_or(100).clamp(25, 400),
        }
    }

    pub fn kind(&self, effect: Effect) -> Kind {
        match effect {
            Effect::Open => self.open,
            Effect::Close => self.close,
            Effect::Minimize => self.minimize,
            Effect::Restore => self.restore,
            Effect::Snap => self.snap,
        }
    }

    pub fn duration(&self, kind: Kind) -> u64 {
        let base: u64 = match kind {
            Kind::MagicLamp => 300,
            Kind::Appear => 180,
            Kind::Morph => 230,
            Kind::Disabled => 0,
        };
        (base * 100 / self.speed.max(25) as u64).clamp(60, 1500)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    None,
    Hide,
}

pub struct Animation {
    pub window: u32,
    pub effect: Effect,
    pub kind: Kind,
    pub from: Area,
    pub to: Area,
    pub started: u64,
    pub duration: u64,
    pub pixels: Vec<u32>,
    pub size: (i32, i32),
    pub finish: Finish,
    pub last: Area,
    pub fresh: Vec<u32>,
    pub fresh_size: (i32, i32),
    pub fresh_at: Option<u64>,
}

const APPEAR_SCALE: f32 = 0.88;
const NECK_GAP: f32 = 0.45;
const MIN_NECK_GAP: f32 = 60.0;
const OPEN_WAIT_MS: u64 = 600;
const FADE_MS: u64 = 150;
const MORPH_WAIT_MS: u64 = 350;
const STRETCH_MS: u64 = 1500;

fn lerp(a: i32, b: i32, t: f32) -> i32 {
    a + ((b - a) as f32 * t) as i32
}


fn ease_out(t: f32) -> f32 {
    let inverted = 1.0 - t;
    1.0 - inverted * inverted * inverted
}


fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn floor(v: f32) -> i32 {
    let truncated = v as i32;
    if (truncated as f32) > v { truncated - 1 } else { truncated }
}

fn ceil(v: f32) -> i32 {
    let truncated = v as i32;
    if (truncated as f32) < v { truncated + 1 } else { truncated }
}


struct Lamp {
    progress: f32,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    target: f32,
    icon_left: f32,
    icon_width: f32,
}

impl Lamp {
    fn row(&self, y: f32) -> (f32, f32, f32) {
        let p = self.progress;
        let factor = y + (self.height - y) * p;
        let cube = (factor * factor * factor) / (self.height * self.height * self.height).max(1.0);
        let offset = (self.target + y) * p * cube;
        let distance = self.target - y;
        let gap = distance.abs().max(self.height * NECK_GAP).max(MIN_NECK_GAP);
        let fraction = (offset / gap).min(1.0);
        let travel = if distance > 0.0 { offset.min(distance) } else { distance * fraction };
        let dest = self.top + y + travel;
        let left = self.left + (self.icon_left - self.left) * fraction;
        let right_from = self.left + self.width;
        let right_to = self.icon_left + self.icon_width;
        let right = right_from + (right_to - right_from) * fraction;
        (dest, left, right)
    }
}

impl Animation {
    pub fn progress(&self, now: u64) -> f32 {
        if self.duration == 0 {
            return 1.0;
        }
        (now.saturating_sub(self.started) as f32 / self.duration as f32).clamp(0.0, 1.0)
    }

    pub fn done(&self, now: u64) -> bool {
        let timed = now.saturating_sub(self.started) >= self.duration;
        if self.kind != Kind::Morph {
            return timed;
        }
        match self.fresh_at {
            Some(at) => timed && now.saturating_sub(at) >= FADE_MS,
            None => now.saturating_sub(self.started) >= self.duration + MORPH_WAIT_MS,
        }
    }

    fn reversed(&self) -> bool {
        matches!(self.effect, Effect::Open | Effect::Restore)
    }

    fn shape(&self, now: u64) -> f32 {
        let raw = self.progress(now);
        if self.reversed() { 1.0 - raw } else { raw }
    }

    fn appear_shape(&self, now: u64) -> f32 {
        let raw = self.progress(now);
        if self.reversed() { 1.0 - ease_out(raw) } else { ease_out(raw) }
    }

    fn morph_area(&self, now: u64) -> Area {
        let k = ease_out(self.progress(now));
        Area::new(lerp(self.from.x, self.to.x, k), lerp(self.from.y, self.to.y, k), lerp(self.from.w, self.to.w, k).max(1), lerp(self.from.h, self.to.h, k).max(1))
    }

    fn appear_area(&self, shape: f32) -> Area {
        let scale = 1.0 - (1.0 - APPEAR_SCALE) * shape;
        let w = ((self.from.w as f32 * scale) as i32).max(1);
        let h = ((self.from.h as f32 * scale) as i32).max(1);
        Area::new(self.from.x + (self.from.w - w) / 2, self.from.y + (self.from.h - h) / 2, w, h)
    }

    fn lamp(&self, shape: f32) -> Lamp {
        let (from, to) = (self.from, self.to);
        Lamp {
            progress: shape.clamp(0.0, 1.0),
            left: from.x as f32,
            top: from.y as f32,
            width: from.w as f32,
            height: from.h as f32,
            target: (to.y - from.y) as f32,
            icon_left: to.x as f32,
            icon_width: to.w as f32,
        }
    }

    fn lamp_bounds(&self, shape: f32) -> Area {
        let lamp = self.lamp(shape);
        let (top, _, _) = lamp.row(0.0);
        let (bottom, _, _) = lamp.row(lamp.height);
        let first = floor(top.min(bottom).min(self.to.y as f32)) - 1;
        let last = ceil(top.max(bottom).max(self.to.y as f32)) + 2;
        let left = self.from.x.min(self.to.x) - 1;
        let right = self.from.right().max(self.to.right()) + 1;
        Area::new(left, first, right - left, last - first)
    }

    pub fn bounds(&self, now: u64) -> Area {
        let area = match self.kind {
            Kind::Morph => self.morph_area(now),
            Kind::Appear => self.appear_area(self.appear_shape(now)),
            Kind::MagicLamp => self.lamp_bounds(self.shape(now)),
            Kind::Disabled => Area::default(),
        };
        area.expand(2)
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_row(pixels: &[u32], size: (i32, i32), source_y: i32, x0: i32, width: i32, alpha: u32, frame: &mut [u32], frame_w: i32, y: i32, clip: Area) {
    if width <= 0 || size.0 <= 0 || source_y < 0 || source_y >= size.1 || y < clip.y || y >= clip.bottom() || alpha == 0 {
        return;
    }
    let start = x0.max(clip.x);
    let end = (x0 + width).min(clip.right());
    if start >= end {
        return;
    }
    let row_start = (source_y * size.0) as usize;
    let row = &pixels[row_start..row_start + size.0 as usize];
    let step = ((size.0 as u64) << 16) / width as u64;
    let mut accumulator = (start - x0) as u64 * step;
    let base = (y * frame_w) as usize;
    let out = &mut frame[base + start as usize..base + end as usize];
    if alpha >= 255 {
        for slot in out.iter_mut() {
            *slot = row[(accumulator >> 16) as usize];
            accumulator += step;
        }
    } else {
        for slot in out.iter_mut() {
            *slot = blend(*slot, row[(accumulator >> 16) as usize], alpha);
            accumulator += step;
        }
    }
}

fn edge_pixel(frame: &mut [u32], frame_w: i32, x: i32, y: i32, clip: Area, pixel: u32, coverage: f32) {
    if coverage <= 0.02 || !clip.contains(x, y) {
        return;
    }
    let index = (y * frame_w + x) as usize;
    frame[index] = blend(frame[index], pixel, (coverage.min(1.0) * 255.0) as u32);
}

pub fn scaled_block(pixels: &[u32], size: (i32, i32), area: Area, alpha: u32, frame: &mut [u32], frame_w: i32, clip: Area) {
    if size.1 <= 0 || area.h <= 0 {
        return;
    }
    let step_y = ((size.1 as u64) << 16) / area.h as u64;
    let top = area.y.max(clip.y);
    let bottom = area.bottom().min(clip.bottom());
    for y in top..bottom {
        let source_y = ((((y - area.y) as u64) * step_y) >> 16) as i32;
        sample_row(pixels, size, source_y, area.x, area.w, alpha, frame, frame_w, y, clip);
    }
}

impl Nook {
    pub fn animations_active(&self) -> bool {
        !self.animations.is_empty() || !self.pending_open.is_empty()
    }

    pub fn animating(&self, id: u32) -> bool {
        self.animations.iter().any(|a| a.window == id) || self.pending_open.iter().any(|(w, _)| *w == id)
    }

    pub fn dock_anchored(&self) -> bool {
        self.animations.iter().any(|a| a.kind == Kind::MagicLamp)
    }

    pub fn stretch_allowed(&self, index: usize, now: u64) -> bool {
        now.saturating_sub(self.core.windows[index].resized_at) < STRETCH_MS
    }

    pub fn defer_open(&mut self, id: u32) {
        if self.anim.kind(Effect::Open) == Kind::Disabled {
            return;
        }
        self.pending_open.push((id, sys::uptime_ms() + OPEN_WAIT_MS));
    }

    fn buffer_matches(&self, index: usize) -> bool {
        let content = self.core.windows[index].content();
        match self.core.windows[index].body {
            crate::Body::Client { width, height, .. } => width == content.w && height == content.h,
            _ => true,
        }
    }

    fn capture_fresh(&mut self, slot: usize, now: u64) {
        let id = self.animations[slot].window;
        let Some(index) = self.core.index_of(id) else {
            self.animations[slot].fresh_at = Some(now);
            return;
        };
        if let Some((area, pixels)) = self.capture(index) {
            let animation = &mut self.animations[slot];
            animation.fresh = pixels;
            animation.fresh_size = (area.w, area.h);
        }
        self.animations[slot].fresh_at = Some(now);
    }

    pub fn client_presented(&mut self, id: u32) {
        if let Some(index) = self.pending_open.iter().position(|(w, _)| *w == id) {
            self.pending_open.remove(index);
            if !self.start_animation(id, Effect::Open, None, Finish::None) {
                if let Some(i) = self.core.index_of(id) {
                    let area = self.core.windows[i].with_shadow();
                    self.core.add_damage(area);
                }
            }
            return;
        }
        let Some(slot) = self.animations.iter().position(|a| a.window == id && a.kind == Kind::Morph && a.fresh_at.is_none()) else {
            return;
        };
        let Some(index) = self.core.index_of(id) else {
            return;
        };
        if self.buffer_matches(index) {
            self.capture_fresh(slot, sys::uptime_ms());
        }
    }

    fn expire_pending(&mut self, now: u64) {
        let mut index = 0;
        while index < self.pending_open.len() {
            let (id, deadline) = self.pending_open[index];
            if now < deadline && self.core.index_of(id).is_some() {
                index += 1;
                continue;
            }
            self.pending_open.remove(index);
            if let Some(i) = self.core.index_of(id) {
                let area = self.core.windows[i].with_shadow();
                self.core.add_damage(area);
            }
        }
        for slot in 0..self.animations.len() {
            let animation = &self.animations[slot];
            if animation.kind == Kind::Morph && animation.fresh_at.is_none() && now.saturating_sub(animation.started) >= MORPH_WAIT_MS {
                self.capture_fresh(slot, now);
            }
        }
    }

    fn dock_target(&self, id: u32) -> Area {
        let items = self.dock_items();
        let icon = self.core.index_of(id).map(|i| self.core.windows[i].icon.clone()).unwrap_or_default();
        let wanted = crate::app_by_icon(&icon);
        let index = items.iter().position(|item| match item {
            crate::dock::DockItem::Window(w) => *w == id,
            crate::dock::DockItem::App(app) => Some(*app) == wanted,
            crate::dock::DockItem::More => false,
        });
        let visible = self.dock_visible_items();
        match index {
            Some(index) if index < visible.len() && visible[index] != crate::dock::DockItem::More => self.dock_slot_area(index, true),
            Some(_) => match visible.iter().position(|i| *i == crate::dock::DockItem::More) {
                Some(more) => self.dock_slot_area(more, true),
                None => self.dock_slot_area(0, true),
            },
            None => {
                let dock = self.dock_shown_area();
                let icon = self.dock_icon();
                Area::new(self.core.screen.width / 2 - icon / 2, dock.y + self.dock_pad(), icon, icon)
            }
        }
    }

    pub fn capture(&mut self, index: usize) -> Option<(Area, Vec<u32>)> {
        let area = self.core.windows[index].outer().intersect(&self.core.bounds());
        if area.is_empty() || area.w > 4096 || area.h > 4096 {
            return None;
        }
        let mut saved = Vec::with_capacity((area.w * area.h) as usize);
        for y in area.y..area.bottom() {
            let start = (y * self.core.screen.width + area.x) as usize;
            saved.extend_from_slice(&self.core.screen.frame[start..start + area.w as usize]);
        }
        let previous_clip = self.core.screen.clip;
        let buttons = self.hot_buttons.len();
        self.core.screen.clip = area;
        let focused = self.core.focused() == Some(self.core.windows[index].id);
        self.draw_window(index, focused);
        self.hot_buttons.truncate(buttons);
        self.core.screen.clip = previous_clip;
        let mut pixels = Vec::with_capacity((area.w * area.h) as usize);
        for y in area.y..area.bottom() {
            let start = (y * self.core.screen.width + area.x) as usize;
            pixels.extend_from_slice(&self.core.screen.frame[start..start + area.w as usize]);
        }
        for (row, y) in (area.y..area.bottom()).enumerate() {
            let start = (y * self.core.screen.width + area.x) as usize;
            let source = row * area.w as usize;
            self.core.screen.frame[start..start + area.w as usize].copy_from_slice(&saved[source..source + area.w as usize]);
        }
        Some((area, pixels))
    }

    fn push_animation(&mut self, id: u32, effect: Effect, kind: Kind, from: Area, to: Area, pixels: Vec<u32>, size: (i32, i32), finish: Finish) {
        let now = sys::uptime_ms();
        let mut animation = Animation {
            window: id,
            effect,
            kind,
            from,
            to,
            started: now,
            duration: self.anim.duration(kind),
            pixels,
            size,
            finish,
            last: Area::default(),
            fresh: Vec::new(),
            fresh_size: (0, 0),
            fresh_at: None,
        };
        let first = animation.bounds(now).union(&from);
        animation.last = first;
        self.animations.push(animation);
        self.core.add_damage(first);
    }

    pub fn start_animation(&mut self, id: u32, effect: Effect, target: Option<Area>, finish: Finish) -> bool {
        let kind = self.anim.kind(effect);
        if kind == Kind::Disabled || self.core.screen.width <= 0 {
            return false;
        }
        let Some(index) = self.core.index_of(id) else {
            return false;
        };
        if self.core.windows[index].minimized && effect != Effect::Restore {
            return false;
        }
        let previous = self.animations.iter().position(|a| a.window == id);
        let inherited = previous.map(|i| self.animations.remove(i));
        self.pending_open.retain(|(w, _)| *w != id);
        let (area, pixels, size) = match inherited {
            Some(old) if old.kind == Kind::Morph && kind == Kind::Morph => {
                let area = old.morph_area(sys::uptime_ms());
                if old.fresh_at.is_some() && !old.fresh.is_empty() { (area, old.fresh, old.fresh_size) } else { (area, old.pixels, old.size) }
            }
            _ => match self.capture(index) {
                Some((area, pixels)) => (area, pixels, (area.w, area.h)),
                None => return false,
            },
        };
        let destination = match kind {
            Kind::MagicLamp => self.dock_target(id),
            Kind::Morph => target.unwrap_or(area),
            _ => area,
        };
        self.push_animation(id, effect, kind, area, destination, pixels, size, finish);
        true
    }

    pub fn start_settle(&mut self, id: u32, pixels: Vec<u32>, area: Area) {
        if self.anim.kind(Effect::Snap) == Kind::Disabled || self.animating(id) || pixels.len() != (area.w * area.h) as usize {
            return;
        }
        self.push_animation(id, Effect::Snap, Kind::Morph, area, area, pixels, (area.w, area.h), Finish::None);
    }

    pub fn tick_animations(&mut self, now: u64) {
        self.expire_pending(now);
        if self.animations.is_empty() {
            return;
        }
        let mut finished: Vec<(u32, Finish)> = Vec::new();
        let mut damage: Vec<Area> = Vec::new();
        let mut index = 0;
        while index < self.animations.len() {
            let bounds = self.animations[index].bounds(now);
            let previous = self.animations[index].last;
            self.animations[index].last = bounds;
            damage.push(bounds.union(&previous));
            if self.animations[index].done(now) {
                let animation = self.animations.remove(index);
                damage.push(animation.to.expand(2));
                finished.push((animation.window, animation.finish));
                continue;
            }
            index += 1;
        }
        for area in damage {
            self.core.add_damage(area);
        }
        for (id, finish) in finished {
            match finish {
                Finish::Hide => {
                    if let Some(index) = self.core.index_of(id) {
                        self.core.windows[index].minimized = true;
                        let area = self.core.windows[index].with_shadow();
                        self.core.add_damage(area);
                    }
                    self.sync_focus();
                    let region = self.dock_region();
                    self.core.add_damage(region);
                }
                Finish::None => {
                    if let Some(index) = self.core.index_of(id) {
                        let area = self.core.windows[index].with_shadow();
                        self.core.add_damage(area);
                    }
                }
            }
        }
    }

    fn draw_lamp(&mut self, animation: &Animation, now: u64, clip: Area) {
        let width = self.core.screen.width;
        let lamp = animation.lamp(animation.shape(now));
        let (columns, rows) = animation.size;
        if rows <= 0 || columns <= 0 {
            return;
        }
        let last_column = (columns - 1) as usize;
        let scale = lamp.height / rows as f32;
        let mut previous = lamp.row(0.0);
        let mut drawn = i32::MIN;
        for source_y in 0..rows {
            let next = lamp.row((source_y + 1) as f32 * scale);
            let (top, left, right) = previous;
            let bottom = next.0;
            previous = next;
            let first = floor(top.min(bottom));
            let mut last = floor(top.max(bottom));
            if last <= first {
                if first == drawn {
                    continue;
                }
                last = first + 1;
            }
            let row = source_y as usize * columns as usize;
            let row_left = animation.pixels[row];
            let row_right = animation.pixels[row + last_column];
            for y in first.max(clip.y)..last.min(clip.bottom()) {
                let inner_left = ceil(left);
                let inner_right = floor(right);
                if inner_right <= inner_left {
                    edge_pixel(&mut self.core.screen.frame, width, floor(left), y, clip, animation.pixels[row + last_column / 2], right - left);
                    continue;
                }
                sample_row(&animation.pixels, animation.size, source_y, inner_left, inner_right - inner_left, 255, &mut self.core.screen.frame, width, y, clip);
                edge_pixel(&mut self.core.screen.frame, width, inner_left - 1, y, clip, row_left, inner_left as f32 - left);
                edge_pixel(&mut self.core.screen.frame, width, inner_right, y, clip, row_right, right - inner_right as f32);
            }
            drawn = last - 1;
        }
    }

    fn draw_morph(&mut self, animation: &Animation, now: u64, clip: Area) {
        let width = self.core.screen.width;
        let area = animation.morph_area(now);
        let fade = match animation.fresh_at {
            Some(at) if !animation.fresh.is_empty() => smoothstep((now.saturating_sub(at) as f32 / FADE_MS as f32).clamp(0.0, 1.0)),
            _ => 0.0,
        };
        if fade < 1.0 {
            scaled_block(&animation.pixels, animation.size, area, 255, &mut self.core.screen.frame, width, clip);
        }
        if fade > 0.0 {
            scaled_block(&animation.fresh, animation.fresh_size, area, (fade * 255.0) as u32, &mut self.core.screen.frame, width, clip);
        }
    }

    pub fn draw_animations(&mut self, now: u64) {
        if self.animations.is_empty() {
            return;
        }
        let clip = self.core.screen.clip;
        let width = self.core.screen.width;
        for i in 0..self.animations.len() {
            let animation = unsafe { &*(&self.animations[i] as *const Animation) };
            if !animation.bounds(now).overlaps(&clip) {
                continue;
            }
            match animation.kind {
                Kind::Morph => self.draw_morph(animation, now, clip),
                Kind::Appear => {
                    let shape = animation.appear_shape(now);
                    let area = animation.appear_area(shape);
                    let alpha = (255.0 * (1.0 - shape * shape)) as u32;
                    scaled_block(&animation.pixels, animation.size, area, alpha.min(255), &mut self.core.screen.frame, width, clip);
                }
                Kind::MagicLamp => self.draw_lamp(animation, now, clip),
                Kind::Disabled => {}
            }
        }
    }
}

