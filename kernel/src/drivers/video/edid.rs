use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timing {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
}

#[derive(Clone, Default)]
pub struct Edid {
    pub name: String,
    pub manufacturer: [u8; 3],
    pub product: u16,
    pub preferred: Option<Timing>,
    pub modes: Vec<Timing>,
    pub vertical_range: (u32, u32),
    pub width_mm: u32,
    pub height_mm: u32,
}

const ESTABLISHED: [(u32, u32, u32); 17] = [
    (800, 600, 60),
    (800, 600, 56),
    (640, 480, 75),
    (640, 480, 72),
    (640, 480, 67),
    (640, 480, 60),
    (720, 400, 88),
    (720, 400, 70),
    (1280, 1024, 75),
    (1024, 768, 75),
    (1024, 768, 70),
    (1024, 768, 60),
    (1024, 768, 87),
    (832, 624, 75),
    (800, 600, 75),
    (800, 600, 72),
    (1152, 870, 75),
];

const VIC: [(u8, u32, u32, u32); 26] = [
    (1, 640, 480, 60),
    (2, 720, 480, 60),
    (3, 720, 480, 60),
    (4, 1280, 720, 60),
    (16, 1920, 1080, 60),
    (17, 720, 576, 50),
    (18, 720, 576, 50),
    (19, 1280, 720, 50),
    (31, 1920, 1080, 50),
    (32, 1920, 1080, 24),
    (33, 1920, 1080, 25),
    (34, 1920, 1080, 30),
    (60, 1280, 720, 24),
    (61, 1280, 720, 25),
    (62, 1280, 720, 30),
    (63, 1920, 1080, 120),
    (64, 1920, 1080, 100),
    (93, 3840, 2160, 24),
    (94, 3840, 2160, 25),
    (95, 3840, 2160, 30),
    (96, 3840, 2160, 50),
    (97, 3840, 2160, 60),
    (98, 4096, 2160, 24),
    (102, 4096, 2160, 60),
    (117, 3840, 2160, 100),
    (118, 3840, 2160, 120),
];

pub fn valid_block(block: &[u8]) -> bool {
    block.len() >= 128 && block[..128].iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

fn detailed(d: &[u8]) -> Option<Timing> {
    let clock = u16::from_le_bytes([d[0], d[1]]) as u64 * 10_000;
    if clock == 0 {
        return None;
    }
    let h_active = d[2] as u32 | ((d[4] as u32 & 0xF0) << 4);
    let h_blank = d[3] as u32 | ((d[4] as u32 & 0x0F) << 8);
    let v_active = d[5] as u32 | ((d[7] as u32 & 0xF0) << 4);
    let v_blank = d[6] as u32 | ((d[7] as u32 & 0x0F) << 8);
    let total = (h_active + h_blank) as u64 * (v_active + v_blank) as u64;
    if h_active < 320 || v_active < 200 || total == 0 {
        return None;
    }
    let interlaced = d[17] & 0x80 != 0;
    let refresh = ((clock + total / 2) / total) as u32;
    Some(Timing { width: h_active, height: if interlaced { v_active * 2 } else { v_active }, refresh })
}

fn push(list: &mut Vec<Timing>, timing: Timing) {
    if timing.width >= 320 && timing.height >= 200 && timing.refresh > 0 && !list.contains(&timing) {
        list.push(timing);
    }
}

fn text(d: &[u8]) -> String {
    d[5..18].iter().take_while(|b| **b != 0x0A && **b != 0).map(|b| *b as char).collect::<String>().trim().into()
}

fn parse_cea(block: &[u8], edid: &mut Edid) {
    if block[0] != 0x02 {
        return;
    }
    let dtd_start = (block[2] as usize).min(127);
    let mut at = 4;
    while at < dtd_start {
        let header = block[at];
        let kind = header >> 5;
        let len = (header & 0x1F) as usize;
        if kind == 2 {
            for &svd in &block[at + 1..(at + 1 + len).min(dtd_start)] {
                let vic = svd & 0x7F;
                if let Some((_, w, h, hz)) = VIC.iter().find(|(v, ..)| *v == vic) {
                    push(&mut edid.modes, Timing { width: *w, height: *h, refresh: *hz });
                }
            }
        }
        at += 1 + len;
    }
    let mut at = dtd_start.max(4);
    while at + 18 <= 127 {
        if let Some(t) = detailed(&block[at..at + 18]) {
            push(&mut edid.modes, t);
        }
        at += 18;
    }
}

pub fn parse(raw: &[u8]) -> Option<Edid> {
    if raw.len() < 128 || raw[0..8] != [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00] {
        return None;
    }
    let mut edid = Edid::default();
    let id = u16::from_be_bytes([raw[8], raw[9]]);
    edid.manufacturer = [b'@' + ((id >> 10) & 0x1F) as u8, b'@' + ((id >> 5) & 0x1F) as u8, b'@' + (id & 0x1F) as u8];
    edid.product = u16::from_le_bytes([raw[10], raw[11]]);
    edid.width_mm = raw[21] as u32 * 10;
    edid.height_mm = raw[22] as u32 * 10;
    for at in (54..126).step_by(18) {
        let d = &raw[at..at + 18];
        if d[0] == 0 && d[1] == 0 {
            match d[3] {
                0xFC => edid.name = text(d),
                0xFD => edid.vertical_range = (d[5] as u32, d[6] as u32),
                _ => {}
            }
            continue;
        }
        if let Some(t) = detailed(d) {
            if edid.preferred.is_none() {
                edid.preferred = Some(t);
            }
            push(&mut edid.modes, t);
        }
    }
    for (bit, (w, h, hz)) in ESTABLISHED.iter().enumerate() {
        let set = match bit {
            0..=7 => raw[35] & (0x80 >> bit) != 0,
            8..=15 => raw[36] & (0x80 >> (bit - 8)) != 0,
            _ => raw[37] & 0x80 != 0,
        };
        if set {
            push(&mut edid.modes, Timing { width: *w, height: *h, refresh: *hz });
        }
    }
    for i in 0..8 {
        let (a, b) = (raw[38 + i * 2], raw[39 + i * 2]);
        if a <= 1 && b <= 1 {
            continue;
        }
        let width = (a as u32 + 31) * 8;
        let height = match b >> 6 {
            0 => width * 10 / 16,
            1 => width * 3 / 4,
            2 => width * 4 / 5,
            _ => width * 9 / 16,
        };
        push(&mut edid.modes, Timing { width, height, refresh: (b as u32 & 0x3F) + 60 });
    }
    let extensions = raw[126] as usize;
    for index in 1..=extensions {
        let start = index * 128;
        if let Some(block) = raw.get(start..start + 128) {
            if valid_block(block) {
                parse_cea(block, &mut edid);
            }
        }
    }
    edid.modes.sort_by(|a, b| (b.width * b.height, b.refresh).cmp(&(a.width * a.height, a.refresh)));
    if let Some(p) = edid.preferred {
        edid.modes.retain(|m| *m != p);
        edid.modes.insert(0, p);
    }
    Some(edid)
}
