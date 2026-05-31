use spin::Mutex;
use super::Framebuffer;

pub struct VesaFramebuffer {
    addr: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    bytes_per_pixel: usize,
}

unsafe impl Send for VesaFramebuffer {}

impl VesaFramebuffer {
    pub const fn uninit() -> Self {
        Self {
            addr: core::ptr::null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
            bytes_per_pixel: 0,
        }
    }

    pub fn setup(&mut self, addr: u64, width: usize, height: usize, pitch: usize, bpp: usize) {
        self.addr = addr as *mut u8;
        self.width = width;
        self.height = height;
        self.pitch = pitch;
        self.bpp = bpp;
        self.bytes_per_pixel = (bpp + 7) / 8;
    }

    pub fn is_ready(&self) -> bool {
        !self.addr.is_null()
    }

    #[inline(always)]
    fn pixel_offset(&self, x: usize, y: usize) -> usize {
        y * self.pitch + x * self.bytes_per_pixel
    }
}

impl Framebuffer for VesaFramebuffer {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn pitch(&self) -> usize {
        self.pitch
    }

    fn bpp(&self) -> usize {
        self.bpp
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height || self.addr.is_null() {
            return;
        }
        let off = self.pixel_offset(x, y);
        unsafe {
            let ptr = self.addr.add(off);
            if self.bytes_per_pixel == 4 {
                *(ptr as *mut u32) = color;
            } else if self.bytes_per_pixel == 3 {
                *ptr = color as u8;
                *ptr.add(1) = (color >> 8) as u8;
                *ptr.add(2) = (color >> 16) as u8;
            }
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        if self.bytes_per_pixel == 4 {
            for row in y..y_end {
                let off = self.pixel_offset(x, row);
                unsafe {
                    let ptr = self.addr.add(off) as *mut u32;
                    for col in 0..(x_end - x) {
                        *ptr.add(col) = color;
                    }
                }
            }
        } else {
            for row in y..y_end {
                for col in x..x_end {
                    self.put_pixel(col, row, color);
                }
            }
        }
    }

    fn scroll_up(&mut self, lines: usize, bg: u32) {
        if self.addr.is_null() || lines == 0 {
            return;
        }
        let line_bytes = self.pitch * lines;
        let total_bytes = self.pitch * self.height;
        unsafe {
            core::ptr::copy(
                self.addr.add(line_bytes),
                self.addr,
                total_bytes - line_bytes,
            );
        }
        let scroll_rows = lines.min(self.height);
        self.fill_rect(0, self.height - scroll_rows, self.width, scroll_rows, bg);
    }

    fn clear(&mut self, color: u32) {
        self.fill_rect(0, 0, self.width, self.height, color);
    }
}

pub static FRAMEBUFFER: Mutex<VesaFramebuffer> = Mutex::new(VesaFramebuffer::uninit());

pub fn init() {
}

pub fn setup_from_multiboot(addr: u64, width: usize, height: usize, pitch: usize, bpp: usize) {
    FRAMEBUFFER.lock().setup(addr, width, height, pitch, bpp);
}
