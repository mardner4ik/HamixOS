use super::{intel_graphics, text_mode};

pub trait VideoDriver: Sync {
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn is_ready(&self) -> bool;
    fn resolution(&self) -> Option<(u32, u32)>;
    fn kind(&self) -> &'static str;
}

pub struct TextModeDriver;

impl VideoDriver for TextModeDriver {
    fn name(&self) -> &'static str {
        "vga-text-mode"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn resolution(&self) -> Option<(u32, u32)> {
        Some((text_mode::COLS as u32, text_mode::ROWS as u32))
    }

    fn kind(&self) -> &'static str {
        "text"
    }
}

pub struct IntelGraphicsVideoDriver;

impl VideoDriver for IntelGraphicsVideoDriver {
    fn name(&self) -> &'static str {
        intel_graphics::driver_name()
    }

    fn version(&self) -> &'static str {
        intel_graphics::driver_version()
    }

    fn is_ready(&self) -> bool {
        intel_graphics::available()
    }

    fn resolution(&self) -> Option<(u32, u32)> {
        intel_graphics::resolution()
    }

    fn kind(&self) -> &'static str {
        "framebuffer"
    }
}

pub static TEXT_MODE_DRIVER: TextModeDriver = TextModeDriver;
pub static INTEL_GRAPHICS_DRIVER: IntelGraphicsVideoDriver = IntelGraphicsVideoDriver;

pub static DRIVERS: &[&dyn VideoDriver] = &[&TEXT_MODE_DRIVER, &INTEL_GRAPHICS_DRIVER];

pub fn for_each<F: FnMut(&dyn VideoDriver)>(mut f: F) {
    for driver in DRIVERS {
        f(*driver);
    }
}
