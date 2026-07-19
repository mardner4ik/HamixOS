use alloc::format;
use alloc::string::String;

use crate::memory::FRAMEBUFFER;

pub fn available() -> bool {
    FRAMEBUFFER.lock().is_some()
}

pub fn resolution() -> Option<(u32, u32)> {
    FRAMEBUFFER.lock().map(|fb| (fb.width, fb.height))
}

fn with_fb<F: FnOnce(u64, u32, u32, u32, u8)>(f: F) {
    if let Some(fb) = *FRAMEBUFFER.lock() {
        f(fb.addr, fb.pitch, fb.width, fb.height, fb.bpp);
    }
}

unsafe fn write_pixel(ptr: *mut u8, bytes_per_pixel: u32, color: u32) {
    unsafe {
        match bytes_per_pixel {
            4 => core::ptr::write_volatile(ptr as *mut u32, color),
            3 => {
                core::ptr::write_volatile(ptr, (color & 0xFF) as u8);
                core::ptr::write_volatile(ptr.add(1), ((color >> 8) & 0xFF) as u8);
                core::ptr::write_volatile(ptr.add(2), ((color >> 16) & 0xFF) as u8);
            }
            2 => {
                let r = ((color >> 16) & 0xFF) as u16;
                let g = ((color >> 8) & 0xFF) as u16;
                let b = (color & 0xFF) as u16;
                let val = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                core::ptr::write_volatile(ptr as *mut u16, val);
            }
            _ => {}
        }
    }
}

pub fn put_pixel(x: u32, y: u32, color: u32) {
    with_fb(|addr, pitch, width, height, bpp| {
        if x >= width || y >= height {
            return;
        }
        let bytes_per_pixel = (bpp as u32 / 8).max(1);
        let offset = y as u64 * pitch as u64 + x as u64 * bytes_per_pixel as u64;
        unsafe {
            let ptr = (addr + offset) as *mut u8;
            write_pixel(ptr, bytes_per_pixel, color);
        }
    });
}

pub fn fill_screen(color: u32) {
    with_fb(|addr, pitch, width, height, bpp| {
        let bytes_per_pixel = (bpp as u32 / 8).max(1);
        for y in 0..height {
            let row_addr = addr + y as u64 * pitch as u64;
            for x in 0..width {
                let offset = x as u64 * bytes_per_pixel as u64;
                unsafe {
                    let ptr = (row_addr + offset) as *mut u8;
                    write_pixel(ptr, bytes_per_pixel, color);
                }
            }
        }
    });
}

pub fn fill_gradient() {
    with_fb(|_, _, width, height, _| {
        for y in 0..height {
            for x in 0..width {
                let r = if width > 0 { x * 255 / width } else { 0 };
                let g = if height > 0 { y * 255 / height } else { 0 };
                let b = 128u32;
                put_pixel(x, y, (r << 16) | (g << 8) | b);
            }
        }
    });
}

pub fn draw_rect(x0: u32, y0: u32, w: u32, h: u32, color: u32) {
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            put_pixel(x, y, color);
        }
    }
}

pub fn info_string() -> String {
    match *FRAMEBUFFER.lock() {
        Some(fb) => format!(
            "intel_penryn: {}x{}x{} framebuffer at {:#x}, pitch {}",
            fb.width, fb.height, fb.bpp, fb.addr, fb.pitch
        ),
        None => String::from("intel_penryn: no linear framebuffer reported by firmware"),
    }
}
