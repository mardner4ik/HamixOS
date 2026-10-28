#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Length {
    #[default]
    Shrink,
    Fill,
    Portion(u16),
    Fixed(u32),
}

impl Length {
    pub fn fills(self) -> bool {
        matches!(self, Length::Fill | Length::Portion(_))
    }

    pub fn portion(self) -> u32 {
        match self {
            Length::Fill => 1,
            Length::Portion(p) => p.max(1) as u32,
            _ => 0,
        }
    }
}

impl From<u32> for Length {
    fn from(px: u32) -> Length {
        Length::Fixed(px)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Padding {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

impl Padding {
    pub const ZERO: Padding = Padding { top: 0, right: 0, bottom: 0, left: 0 };

    pub fn all(px: u16) -> Padding {
        Padding { top: px, right: px, bottom: px, left: px }
    }

    pub fn symmetric(vertical: u16, horizontal: u16) -> Padding {
        Padding { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    pub fn horizontal(&self) -> i32 {
        self.left as i32 + self.right as i32
    }

    pub fn vertical(&self) -> i32 {
        self.top as i32 + self.bottom as i32
    }
}

impl From<u16> for Padding {
    fn from(px: u16) -> Padding {
        Padding::all(px)
    }
}

impl From<[u16; 2]> for Padding {
    fn from(v: [u16; 2]) -> Padding {
        Padding::symmetric(v[0], v[1])
    }
}

impl From<[u16; 4]> for Padding {
    fn from(v: [u16; 4]) -> Padding {
        Padding { top: v[0], right: v[1], bottom: v[2], left: v[3] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextSize {
    #[default]
    Body,
    Small,
    Strong,
    Title,
    Display,
    Mono,
    Heading1,
    Heading2,
    Heading3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Normal,
    Dim,
    Faint,
    Accent,
    Danger,
    Success,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonKind {
    #[default]
    Secondary,
    Primary,
    Danger,
    Flat,
    Link,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Color(pub u32);

impl From<u32> for Color {
    fn from(c: u32) -> Color {
        Color(c)
    }
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color(0xff00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32)
    }

    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Color {
        Color((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32)
    }

    pub fn alpha(self) -> u8 {
        (self.0 >> 24) as u8
    }

    pub fn red(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub fn green(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub fn blue(self) -> u8 {
        self.0 as u8
    }
}
