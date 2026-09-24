const BOX: &[u8; 512] = b"11002200001100221100220000110022110022000011002201010201010202021001200110022002011002100120022010102010102020200111021101210112012202210212022210112011102110121022202120122022110121011201220111022102120222021110211012102210112021201220222011112111121122111121111211222121122121121212222122122122122222221100220000110022330000330301010303033001100330030310013003303010103030300311013303333011103330333301110333033310113033303311113333330101100110100110000000000000100000100100000120000020020000021200001221000021";

pub fn box_lines(ch: char) -> Option<[u8; 4]> {
    let cp = ch as u32;
    if !(0x2500..0x2580).contains(&cp) {
        return None;
    }
    let i = (cp - 0x2500) as usize * 4;
    let entry = [BOX[i] - b'0', BOX[i + 1] - b'0', BOX[i + 2] - b'0', BOX[i + 3] - b'0'];
    if entry == [0; 4] {
        None
    } else {
        Some(entry)
    }
}

fn lines(w: i32, h: i32, weights: [u8; 4], fill: &mut dyn FnMut(i32, i32, i32, i32, u8)) {
    let light = (w.min(h) / 8).max(1);
    let heavy = light * 2;
    let gap = light;
    let cx = w / 2;
    let cy = h / 2;
    let [left, right, up, down] = weights;
    let thickness = |weight: u8| if weight == 2 { heavy } else { light };
    let span = |a: u8, b: u8| -> (i32, i32) {
        if a == 0 && b == 0 {
            return (0, 0);
        }
        if a == 3 || b == 3 {
            return (gap + light, gap + light);
        }
        let t = thickness(a.max(b));
        (t / 2, t - t / 2)
    };
    let (v_before, v_after) = span(up, down);
    let (h_before, h_after) = span(left, right);
    let mut horizontal = |x0: i32, x1: i32, weight: u8| {
        if weight == 3 {
            fill(x0, cy - gap - light, x1 - x0, light, 255);
            fill(x0, cy + gap, x1 - x0, light, 255);
        } else {
            let t = thickness(weight);
            fill(x0, cy - t / 2, x1 - x0, t, 255);
        }
    };
    if left != 0 {
        horizontal(0, cx + v_after, left);
    }
    if right != 0 {
        horizontal(cx - v_before, w, right);
    }
    let mut vertical = |y0: i32, y1: i32, weight: u8| {
        if weight == 3 {
            fill(cx - gap - light, y0, light, y1 - y0, 255);
            fill(cx + gap, y0, light, y1 - y0, 255);
        } else {
            let t = thickness(weight);
            fill(cx - t / 2, y0, t, y1 - y0, 255);
        }
    };
    if up != 0 {
        vertical(0, cy + h_after, up);
    }
    if down != 0 {
        vertical(cy - h_before, h, down);
    }
}

fn quadrants(w: i32, h: i32, mask: u8, fill: &mut dyn FnMut(i32, i32, i32, i32, u8)) {
    let hw = w / 2;
    let hh = h / 2;
    if mask & 1 != 0 {
        fill(0, 0, hw, hh, 255);
    }
    if mask & 2 != 0 {
        fill(hw, 0, w - hw, hh, 255);
    }
    if mask & 4 != 0 {
        fill(0, hh, hw, h - hh, 255);
    }
    if mask & 8 != 0 {
        fill(hw, hh, w - hw, h - hh, 255);
    }
}

pub fn rasterize(ch: char, w: i32, h: i32, fill: &mut dyn FnMut(i32, i32, i32, i32, u8)) -> bool {
    let cp = ch as u32;
    if let Some(weights) = box_lines(ch) {
        lines(w, h, weights, fill);
        return true;
    }
    match cp {
        0x2580 => fill(0, 0, w, h / 2, 255),
        0x2581..=0x2588 => {
            let eighths = (cp - 0x2580) as i32;
            let height = (h * eighths + 4) / 8;
            fill(0, h - height, w, height, 255);
        }
        0x2589..=0x258F => {
            let eighths = (0x2590 - cp) as i32;
            fill(0, 0, (w * eighths + 4) / 8, h, 255);
        }
        0x2590 => fill(w / 2, 0, w - w / 2, h, 255),
        0x2591 => fill(0, 0, w, h, 64),
        0x2592 => fill(0, 0, w, h, 128),
        0x2593 => fill(0, 0, w, h, 192),
        0x2594 => fill(0, 0, w, (h + 4) / 8, 255),
        0x2595 => {
            let width = (w + 4) / 8;
            fill(w - width, 0, width, h, 255)
        }
        0x2596..=0x259F => {
            const MASKS: [u8; 10] = [4, 8, 1, 1 | 4 | 8, 1 | 8, 1 | 2 | 4, 1 | 2 | 8, 2, 2 | 4, 2 | 4 | 8];
            quadrants(w, h, MASKS[(cp - 0x2596) as usize], fill);
        }
        0x2800..=0x28FF => {
            let bits = (cp - 0x2800) as u8;
            let dot = (w / 4).max(1).min((h / 8).max(1)).max(1);
            const POS: [(i32, i32); 8] = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)];
            for (i, (col, row)) in POS.iter().enumerate() {
                if bits & (1 << i) != 0 {
                    let x = w * (1 + 2 * col) / 4 - dot / 2;
                    let y = h * (1 + 2 * row) / 8 - dot / 2;
                    fill(x, y, dot + if w >= 12 { 1 } else { 0 }, dot + if h >= 16 { 1 } else { 0 }, 255);
                }
            }
        }
        0x25A0 => {
            let m = w / 8;
            fill(m, (h - (w - 2 * m)) / 2, w - 2 * m, w - 2 * m, 255);
        }
        0x25AC => fill(0, h / 3, w, h / 3, 255),
        0x25AE => fill(w / 4, h / 5, w / 2, h * 3 / 5, 255),
        0xE0B0 | 0xE0B2 => {
            for y in 0..h {
                let dist = if y < h / 2 { y } else { h - 1 - y };
                let span = (w * (2 * dist + 1) / h).min(w);
                if cp == 0xE0B0 {
                    fill(0, y, span, 1, 255);
                } else {
                    fill(w - span, y, span, 1, 255);
                }
            }
        }
        _ => return false,
    }
    true
}
