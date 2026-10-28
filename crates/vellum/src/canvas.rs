use crate::color::Color;
use crate::geometry::{Point, Rect};

pub trait Canvas {
    fn dimensions(&self) -> (u32, u32);
    fn set_pixel(&mut self, x: u32, y: u32, color: Color);

    fn width(&self) -> u32 {
        self.dimensions().0
    }

    fn height(&self) -> u32 {
        self.dimensions().1
    }

    fn fill(&mut self, color: Color) {
        let (width, height) = self.dimensions();
        for y in 0..height {
            for x in 0..width {
                self.set_pixel(x, y, color);
            }
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let (width, height) = self.dimensions();
        let clamped = rect.clamp_to(width, height);
        for y in clamped.y..clamped.y + clamped.height {
            for x in clamped.x..clamped.x + clamped.width {
                self.set_pixel(x, y, color);
            }
        }
    }

    fn draw_point(&mut self, point: Point, color: Color) {
        self.set_pixel(point.x, point.y, color);
    }

    fn horizontal_gradient(&mut self, from: Color, to: Color) {
        let (width, height) = self.dimensions();
        for y in 0..height {
            for x in 0..width {
                let t = if width > 1 { (x * 255 / (width - 1)) as u8 } else { 0 };
                self.set_pixel(x, y, from.lerp(to, t));
            }
        }
    }
}
