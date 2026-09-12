#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0x00, 0x00, 0x00);
    pub const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);
    pub const RED: Color = Color::rgb(0xFF, 0x00, 0x00);
    pub const GREEN: Color = Color::rgb(0x00, 0xFF, 0x00);
    pub const BLUE: Color = Color::rgb(0x00, 0x00, 0xFF);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xFF }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_u32(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
            a: 0xFF,
        }
    }

    pub const fn as_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    pub fn blend(self, over: Color) -> Color {
        let alpha = over.a as u32;
        let inv = 255 - alpha;
        let mix = |base: u8, top: u8| (((base as u32 * inv) + (top as u32 * alpha)) / 255) as u8;
        Color::rgb(mix(self.r, over.r), mix(self.g, over.g), mix(self.b, over.b))
    }

    pub fn lerp(self, other: Color, t: u8) -> Color {
        let mix = |a: u8, b: u8| (((a as u32 * (255 - t as u32)) + (b as u32 * t as u32)) / 255) as u8;
        Color::rgb(mix(self.r, other.r), mix(self.g, other.g), mix(self.b, other.b))
    }
}
