pub const SHIFT: u32 = 1;
pub const CTRL: u32 = 4;
pub const ALT: u32 = 8;

const LETTERS: [u32; 26] = [30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44];

fn ascii(c: u8) -> Option<(u32, bool)> {
    Some(match c {
        b'a'..=b'z' => (LETTERS[(c - b'a') as usize], false),
        b'A'..=b'Z' => (LETTERS[(c - b'A') as usize], true),
        b'1'..=b'9' => ((c - b'1') as u32 + 2, false),
        b'0' => (11, false),
        b'!' => (2, true),
        b'@' => (3, true),
        b'#' => (4, true),
        b'$' => (5, true),
        b'%' => (6, true),
        b'^' => (7, true),
        b'&' => (8, true),
        b'*' => (9, true),
        b'(' => (10, true),
        b')' => (11, true),
        b'-' => (12, false),
        b'_' => (12, true),
        b'=' => (13, false),
        b'+' => (13, true),
        b'[' => (26, false),
        b'{' => (26, true),
        b']' => (27, false),
        b'}' => (27, true),
        b';' => (39, false),
        b':' => (39, true),
        b'\'' => (40, false),
        b'"' => (40, true),
        b'`' => (41, false),
        b'~' => (41, true),
        b'\\' => (43, false),
        b'|' => (43, true),
        b',' => (51, false),
        b'<' => (51, true),
        b'.' => (52, false),
        b'>' => (52, true),
        b'/' => (53, false),
        b'?' => (53, true),
        b' ' => (57, false),
        _ => return None,
    })
}

pub fn translate(code: i32, mods: u32) -> Option<(u32, u32)> {
    let mut xkb_mods = 0;
    if mods & 1 != 0 {
        xkb_mods |= SHIFT;
    }
    if mods & 2 != 0 {
        xkb_mods |= ALT;
    }
    if mods & 4 != 0 {
        xkb_mods |= CTRL;
    }
    let key = match code {
        -1 => 103,
        -2 => 108,
        -3 => 105,
        -4 => 106,
        -5 => 102,
        -6 => 107,
        -7 => 111,
        -8 => 104,
        -9 => 109,
        -53 => 110,
        -50..=-41 => (58 + (-40 - code)) as u32,
        -51 => 87,
        -52 => 88,
        8 | 127 => 14,
        9 => 15,
        10 | 13 => 28,
        27 => 1,
        1..=26 => {
            xkb_mods |= CTRL;
            LETTERS[(code - 1) as usize]
        }
        32..=126 => {
            let (key, shift) = ascii(code as u8)?;
            if shift {
                xkb_mods |= SHIFT;
            }
            key
        }
        _ => return None,
    };
    Some((key, xkb_mods))
}
