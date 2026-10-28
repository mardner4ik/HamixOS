use alloc::string::String;
use alloc::vec::Vec;
use vellum::{Area, Painter};

const MAX_DAMAGE: usize = 16;
const HOT_EDGE: i32 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Desktop,
    Bar,
    Dock,
    Popup,
    Osd,
}

pub const KINDS: [Kind; 5] = [Kind::Desktop, Kind::Bar, Kind::Dock, Kind::Popup, Kind::Osd];

pub struct ScreenInfo {
    pub width: i32,
    pub height: i32,
    pub clip: Area,
}

pub struct Win {
    pub id: u32,
    pub title: String,
    pub icon: String,
    pub minimized: bool,
    pub active: bool,
    pub workspace: u32,
    pub area: Area,
    pub parent: Option<u32>,
}

impl Win {
    pub fn is_popup(&self) -> bool {
        false
    }

    pub fn outer(&self) -> Area {
        self.area
    }
}

pub struct Core {
    pub screen: ScreenInfo,
    pub windows: Vec<Win>,
    pub pointer: (i32, i32),
    pub damage: Vec<Area>,
    pub workspace: u32,
    target: Option<(*mut u32, Area)>,
}

impl Core {
    pub fn new(width: i32, height: i32, workspace: u32) -> Core {
        Core { screen: ScreenInfo { width, height, clip: Area::new(0, 0, width, height) }, windows: Vec::new(), pointer: (-1, -1), damage: Vec::new(), workspace, target: None }
    }

    pub fn bounds(&self) -> Area {
        Area::new(0, 0, self.screen.width, self.screen.height)
    }

    pub fn add_damage(&mut self, area: Area) {
        let area = area.intersect(&self.bounds());
        if area.is_empty() {
            return;
        }
        if let Some(i) = self.damage.iter().position(|d| d.overlaps(&area)) {
            let merged = self.damage.remove(i).union(&area);
            return self.add_damage(merged);
        }
        self.damage.push(area);
        if self.damage.len() > MAX_DAMAGE {
            let all = self.damage.iter().skip(1).fold(self.damage[0], |acc, d| acc.union(d));
            self.damage.clear();
            self.damage.push(all);
        }
    }

    pub fn damage_all(&mut self) {
        self.damage.clear();
        self.damage.push(self.bounds());
    }

    pub fn begin(&mut self, pixels: *mut u32, area: Area, rect: Area, hot_edge: bool) {
        self.target = Some((pixels, area));
        self.screen.clip = rect;
        let buf = unsafe { core::slice::from_raw_parts_mut(pixels, (area.w * area.h) as usize) };
        let edge = self.screen.height - HOT_EDGE;
        for y in rect.y..rect.bottom() {
            let start = ((y - area.y) * area.w + rect.x - area.x) as usize;
            let fill = if hot_edge && y >= edge { 0x0100_0000 } else { 0 };
            buf[start..start + rect.w as usize].fill(fill);
        }
    }

    pub fn end(&mut self) {
        self.target = None;
        self.screen.clip = self.bounds();
    }

    pub fn painter_clipped(&mut self) -> Painter<'_> {
        match self.target {
            Some((pixels, area)) => {
                let buf = unsafe { core::slice::from_raw_parts_mut(pixels, (area.w * area.h) as usize) };
                let mut p = Painter::layer(buf, area);
                p.set_clip(self.screen.clip);
                p
            }
            None => Painter::layer(&mut [], Area::new(0, 0, 0, 0)),
        }
    }

    pub fn focused(&self) -> Option<u32> {
        self.windows.iter().find(|w| w.active && self.shown(w)).map(|w| w.id)
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    pub fn shown(&self, w: &Win) -> bool {
        !w.minimized && w.workspace == self.workspace
    }

    pub fn update_window(&mut self, id: u32, state: u32, workspace: u32, title: &str, app_id: &str) {
        let active = state & hxproto::TOPLEVEL_ACTIVE != 0;
        if active {
            for w in self.windows.iter_mut() {
                w.active = false;
            }
        }
        let minimized = state & hxproto::TOPLEVEL_MINIMIZED != 0;
        match self.windows.iter_mut().find(|w| w.id == id) {
            Some(w) => {
                w.title = String::from(title);
                w.icon = String::from(app_id);
                w.minimized = minimized;
                w.active = active;
                w.workspace = workspace;
            }
            None => self.windows.push(Win { id, title: String::from(title), icon: String::from(app_id), minimized, active, workspace, area: Area::new(0, 0, 0, 0), parent: None }),
        }
    }

    pub fn move_window(&mut self, id: u32, area: Area) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.area = area;
        }
    }
}

