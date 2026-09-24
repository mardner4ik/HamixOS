pub mod console_mouse;
pub mod edid;
pub mod font8x8;
pub mod gpu;
pub mod modes;
pub mod registry;
pub mod sysfs;
pub mod text_mode;

pub fn resolution() -> Option<(u32, u32)> {
    crate::memory::FRAMEBUFFER.lock().map(|fb| (fb.width, fb.height))
}

pub fn framebuffer_ready() -> bool {
    crate::memory::FRAMEBUFFER.lock().is_some()
}
