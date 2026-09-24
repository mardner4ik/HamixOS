use super::text_mode;

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

pub struct BootFramebufferDriver;

impl VideoDriver for BootFramebufferDriver {
    fn name(&self) -> &'static str {
        "boot-framebuffer"
    }

    fn version(&self) -> &'static str {
        "0.2.0"
    }

    fn is_ready(&self) -> bool {
        super::framebuffer_ready()
    }

    fn resolution(&self) -> Option<(u32, u32)> {
        super::resolution()
    }

    fn kind(&self) -> &'static str {
        "framebuffer"
    }
}

pub static TEXT_MODE_DRIVER: TextModeDriver = TextModeDriver;
pub static BOOT_FRAMEBUFFER_DRIVER: BootFramebufferDriver = BootFramebufferDriver;

pub static DRIVERS: &[&dyn VideoDriver] = &[&TEXT_MODE_DRIVER, &BOOT_FRAMEBUFFER_DRIVER];

pub fn for_each<F: FnMut(&dyn VideoDriver)>(mut f: F) {
    for driver in DRIVERS {
        f(*driver);
    }
}
