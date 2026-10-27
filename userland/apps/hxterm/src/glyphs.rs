use vellum::{Area, Painter};

pub fn draw(p: &mut Painter, x: i32, y: i32, w: i32, h: i32, ch: char, color: u32) -> bool {
    let cp = ch as u32;
    if !(0x2500..0x2600).contains(&cp) && !(0x2800..0x2900).contains(&cp) && !(0xE0B0..=0xE0B3).contains(&cp) {
        return false;
    }
    hxvt::rasterize(ch, w, h, &mut |fx, fy, fw, fh, alpha| {
        if fw <= 0 || fh <= 0 {
            return;
        }
        let area = Area::new(x + fx, y + fy, fw, fh);
        if alpha == 255 {
            p.fill(area, color);
        } else {
            p.blend_fill(area, color, alpha as u32);
        }
    })
}
