pub mod vesa;

pub trait Framebuffer: Send {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn pitch(&self) -> usize;
    fn bpp(&self) -> usize;
    fn put_pixel(&mut self, x: usize, y: usize, color: u32);
    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32);
    fn scroll_up(&mut self, lines: usize, bg: u32);
    fn clear(&mut self, color: u32);
}
