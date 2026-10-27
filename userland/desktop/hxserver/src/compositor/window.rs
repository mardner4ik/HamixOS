use alloc::string::String;
use vellum::gfx::shadow_bounds;
use vellum::{Area, Image};

pub const TITLE_H: i32 = 36;
pub const RADIUS: i32 = 10;
pub const SHADOW_SIZE: i32 = 28;
pub const SHADOW_OFFSET: i32 = 8;

pub const EDGE_LEFT: u8 = 1;
pub const EDGE_RIGHT: u8 = 2;
pub const EDGE_TOP: u8 = 4;
pub const EDGE_BOTTOM: u8 = 8;

pub fn edge_cursor(edges: u8) -> usize {
    match edges {
        EDGE_LEFT | EDGE_RIGHT => 5,
        EDGE_TOP | EDGE_BOTTOM => 4,
        e if e == EDGE_LEFT | EDGE_TOP || e == EDGE_RIGHT | EDGE_BOTTOM => 6,
        e if e == EDGE_RIGHT | EDGE_TOP || e == EDGE_LEFT | EDGE_BOTTOM => 7,
        _ => 0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Left,
    Right,
    Maximize,
}

pub enum Body {
    About,
    Auth { action: crate::PowerAction, field: hxclient::ui::TextField, error: String },
    Client { pid: i64, shm: u32, pixels: *const u32, width: i32, height: i32 },
}

pub struct Cursor {
    pub image: Image,
    pub hot: (i32, i32),
}

pub struct Window {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub title: String,
    pub icon: String,
    pub body: Body,
    pub minimized: bool,
    pub cursor: u32,
    pub min_w: i32,
    pub min_h: i32,
    pub max_w: i32,
    pub max_h: i32,
    pub restore: Option<Area>,
    pub tiled: Option<Snap>,
    pub requested: (i32, i32),
    pub resized_at: u64,
    pub parent: Option<u32>,
    pub track: bool,
    pub placed: (i32, i32),
    pub frameless: bool,
}

impl Window {
    pub fn outer(&self) -> Area {
        Area::new(self.x, self.y, self.w, self.h)
    }

    pub fn radius(&self) -> i32 {
        if self.tiled.is_some() || self.parent.is_some() || self.frameless { 0 } else { RADIUS }
    }

    pub fn bar_h(&self) -> i32 {
        if self.is_popup() || self.frameless { 0 } else { TITLE_H }
    }

    pub fn is_popup(&self) -> bool {
        self.parent.is_some()
    }

    pub fn with_shadow(&self) -> Area {
        if self.tiled.is_some() {
            return self.outer();
        }
        shadow_bounds(self.outer(), SHADOW_SIZE, SHADOW_OFFSET).union(&self.outer())
    }

    pub fn content(&self) -> Area {
        let bar = self.bar_h();
        Area::new(self.x, self.y + bar, self.w, self.h - bar)
    }

    pub fn title_bar(&self) -> Area {
        if self.bar_h() == 0 {
            return Area::new(self.x, self.y, 0, 0);
        }
        Area::new(self.x, self.y, self.w, TITLE_H)
    }

    pub fn close_button(&self) -> Area {
        Area::new(self.x + self.w - 38, self.y + 6, 30, 24)
    }

    pub fn maximize_button(&self) -> Area {
        Area::new(self.x + self.w - 72, self.y + 6, 30, 24)
    }

    pub fn minimize_button(&self) -> Area {
        if self.resizable() {
            Area::new(self.x + self.w - 106, self.y + 6, 30, 24)
        } else {
            Area::new(self.x + self.w - 72, self.y + 6, 30, 24)
        }
    }

    pub fn resizable(&self) -> bool {
        if self.is_popup() {
            return false;
        }
        match self.body {
            Body::Client { .. } => self.max_w == 0 || self.max_h == 0 || self.max_w > self.min_w || self.max_h > self.min_h,
            _ => false,
        }
    }

    pub fn opaque_regions(&self) -> [Area; 2] {
        let r = if self.is_popup() { 12 } else { self.radius() };
        let o = self.outer();
        [Area::new(o.x, o.y + r, o.w, o.h - 2 * r), Area::new(o.x + r, o.y, o.w - 2 * r, o.h)]
    }

    pub fn client_pid(&self) -> Option<i64> {
        match self.body {
            Body::Client { pid, .. } => Some(pid),
            _ => None,
        }
    }

    pub fn client_shm(&self) -> Option<u32> {
        match self.body {
            Body::Client { shm, .. } => Some(shm),
            _ => None,
        }
    }

    pub fn min_outer(&self) -> (i32, i32) {
        (self.min_w.max(160), self.min_h.max(60) + self.bar_h())
    }

    pub fn max_outer(&self) -> (i32, i32) {
        let w = if self.max_w > 0 { self.max_w } else { i32::MAX / 4 };
        let h = if self.max_h > 0 { self.max_h + self.bar_h() } else { i32::MAX / 4 };
        (w, h)
    }
}
