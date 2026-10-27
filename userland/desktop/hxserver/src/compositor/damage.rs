use alloc::vec::Vec;
use vellum::Area;

const MAX_RECTS: usize = 16;

#[derive(Default)]
pub struct Damage {
    rects: Vec<Area>,
}

impl Damage {
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn add(&mut self, area: Area, bounds: Area) {
        let mut area = area.intersect(&bounds);
        if area.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.rects.len() {
            if self.rects[i].overlaps(&area.expand(8)) {
                area = area.union(&self.rects[i]);
                self.rects.swap_remove(i);
                i = 0;
                continue;
            }
            i += 1;
        }
        self.rects.push(area);
        if self.rects.len() > MAX_RECTS {
            let total = self.rects.iter().fold(Area::new(0, 0, 0, 0), |acc, a| acc.union(a));
            self.rects.clear();
            self.rects.push(total);
        }
    }

    pub fn all(&mut self, bounds: Area) {
        self.rects.clear();
        self.rects.push(bounds);
    }

    pub fn take(&mut self) -> Vec<Area> {
        core::mem::take(&mut self.rects)
    }
}

pub fn subtract(rect: &Area, hole: &Area, out: &mut Vec<Area>) {
    let cut = rect.intersect(hole);
    if cut.is_empty() {
        out.push(*rect);
        return;
    }
    if cut.y > rect.y {
        out.push(Area::new(rect.x, rect.y, rect.w, cut.y - rect.y));
    }
    if cut.bottom() < rect.bottom() {
        out.push(Area::new(rect.x, cut.bottom(), rect.w, rect.bottom() - cut.bottom()));
    }
    if cut.x > rect.x {
        out.push(Area::new(rect.x, cut.y, cut.x - rect.x, cut.h));
    }
    if cut.right() < rect.right() {
        out.push(Area::new(cut.right(), cut.y, rect.right() - cut.right(), cut.h));
    }
}

pub fn subtract_all(rects: &[Area], hole: &Area) -> Vec<Area> {
    let mut out = Vec::with_capacity(rects.len() + 4);
    for r in rects {
        subtract(r, hole, &mut out);
    }
    out
}
