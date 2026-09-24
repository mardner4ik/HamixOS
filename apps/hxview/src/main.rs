#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, env, eprintln, fs, sys};
use hxclient::ui::{self, theme, Hits, Style, Ui};
use hxclient::{Event, Window, CURSOR_ARROW, CURSOR_MOVE, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_RELEASE, MOUSE_WHEEL};
use vellum::gfx::ellipsize;
use vellum::{Area, Image, Painter};

const INITIAL_W: i32 = 900;
const INITIAL_H: i32 = 620;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}
const BAR: i32 = 52;

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Prev,
    Next,
    ZoomOut,
    ZoomIn,
    Fit,
    Wallpaper,
    Canvas,
}

struct App {
    window: Window,
    ui: Ui,
    files: Vec<String>,
    index: usize,
    image: Option<Image>,
    fitted: Option<Image>,
    error: String,
    zoom: f32,
    fit: bool,
    pan: (f32, f32),
    drag: Option<(i32, i32, f32, f32)>,
    hits: Hits<Hit>,
    hover: Option<Hit>,
    status: String,
}

fn canvas() -> Area {
    Area::new(0, BAR, ww(), hh() - BAR)
}

fn is_image(name: &str) -> bool {
    name.to_lowercase().ends_with(".png")
}

impl App {
    fn load_dir(&mut self, path: &str) {
        let (dir, name) = match path.rfind('/') {
            Some(i) => (String::from(&path[..i.max(1)]), String::from(&path[i + 1..])),
            None => (sys::getcwd(), String::from(path)),
        };
        let mut files: Vec<String> = sys::read_dir(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !e.is_dir && is_image(&e.name))
            .map(|e| if dir == "/" { format!("/{}", e.name) } else { format!("{}/{}", dir, e.name) })
            .collect();
        files.sort();
        self.index = files.iter().position(|f| f.rsplit('/').next() == Some(name.as_str())).unwrap_or(0);
        self.files = files;
    }

    fn load_current(&mut self) {
        self.image = None;
        self.fitted = None;
        self.error.clear();
        self.fit = true;
        self.pan = (0.0, 0.0);
        let Some(path) = self.files.get(self.index).cloned() else {
            self.error = String::from("No pictures in this folder");
            self.window.set_title("Images");
            return;
        };
        let name = String::from(path.rsplit('/').next().unwrap_or(&path));
        self.window.set_title(&format!("{} — Images", name));
        match fs::read(&path) {
            None => self.error = format!("Cannot read {}", name),
            Some(bytes) => match Image::from_png(&bytes) {
                Some(img) => {
                    self.image = Some(img);
                    self.refit();
                }
                None => self.error = format!("{} is not a PNG image this viewer understands", name),
            },
        }
    }

    fn fit_zoom(&self) -> f32 {
        let Some(img) = &self.image else {
            return 1.0;
        };
        let c = canvas().inset(24);
        let zx = c.w as f32 / img.w as f32;
        let zy = c.h as f32 / img.h as f32;
        zx.min(zy).min(1.0)
    }

    fn refit(&mut self) {
        let zoom = self.fit_zoom();
        self.zoom = zoom;
        if let Some(img) = &self.image {
            let w = ((img.w as f32 * zoom) as i32).max(1);
            let h = ((img.h as f32 * zoom) as i32).max(1);
            self.fitted = Some(if w == img.w && h == img.h { Image { w: img.w, h: img.h, px: img.px.clone() } } else { img.scaled(w, h) });
        }
    }

    fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.05, 8.0);
        self.fit = false;
        self.clamp_pan();
    }

    fn clamp_pan(&mut self) {
        if let Some(img) = &self.image {
            let c = canvas();
            let max_x = ((img.w as f32 * self.zoom - c.w as f32) / 2.0).max(0.0);
            let max_y = ((img.h as f32 * self.zoom - c.h as f32) / 2.0).max(0.0);
            self.pan.0 = self.pan.0.clamp(-max_x, max_x);
            self.pan.1 = self.pan.1.clamp(-max_y, max_y);
        }
    }

    fn step(&mut self, delta: i32) {
        if self.files.is_empty() {
            return;
        }
        let n = self.files.len() as i32;
        self.index = ((self.index as i32 + delta).rem_euclid(n)) as usize;
        self.load_current();
    }

    fn draw(&mut self) {
        let ui = &self.ui;
        let (fit, zoom, pan) = (self.fit, self.zoom, self.pan);
        let buffer = self.window.buffer();
        let mut p = Painter::new(buffer, ww(), hh());
        let hits = &mut self.hits;
        hits.clear();
        let hover = self.hover;
        let c = canvas();
        p.fill(c, 0x0e1014);
        if let Some(img) = &self.image {
            let w = (img.w as f32 * zoom) as i32;
            let h = (img.h as f32 * zoom) as i32;
            let x = c.x + (c.w - w) / 2 + pan.0 as i32;
            let y = c.y + (c.h - h) / 2 + pan.1 as i32;
            let frame = Area::new(x, y, w, h);
            p.set_clip(c);
            p.shadow(frame, 2, 14, 160, 3);
            for cy in (frame.y.max(c.y)..frame.bottom().min(c.bottom())).step_by(16) {
                for cx in (frame.x.max(c.x)..frame.right().min(c.right())).step_by(16) {
                    let dark = ((cx - frame.x) / 16 + (cy - frame.y) / 16) % 2 == 0;
                    p.fill(Area::new(cx, cy, 16, 16).intersect(&frame), if dark { 0x2a2d33 } else { 0x33363d });
                }
            }
            match (&self.fitted, fit) {
                (Some(f), true) => p.image(f, x, y, 255),
                _ => p.image_scaled(img, frame, 255),
            }
            p.reset_clip();
        } else {
            ui::centered_text(&mut p, &ui.medium, c, if self.error.is_empty() { "Loading…" } else { &self.error }, theme::dim());
        }
        hits.add(c, Hit::Canvas);

        let bar = Area::new(0, 0, ww(), BAR);
        p.fill(bar, theme::surface());
        p.fill(Area::new(0, BAR - 1, ww(), 1), theme::border());
        let mut x = 10;
        for (hit, icon) in [(Hit::Prev, "ui/back"), (Hit::Next, "ui/forward")] {
            let area = Area::new(x, 10, 34, 32);
            ui::icon_button(&mut p, ui, area, icon, hover == Some(hit), false);
            hits.add(area, hit);
            x += 38;
        }
        let name = self.files.get(self.index).map(|f| String::from(f.rsplit('/').next().unwrap_or(f))).unwrap_or_default();
        let info = match &self.image {
            Some(img) => format!("{}   {} × {}   {}%   {} / {}", name, img.w, img.h, (zoom * 100.0) as i32, self.index + 1, self.files.len()),
            None => name,
        };
        let info_area = Area::new(x + 10, 10, ww() - x - 10 - 330, 32);
        ui::text_in(&mut p, &ui.font, info_area.x, info_area, &ellipsize(&ui.font, &info, info_area.w), theme::text());
        let mut rx = ww() - 320;
        for (hit, icon) in [(Hit::ZoomOut, "ui/zoom-out"), (Hit::ZoomIn, "ui/zoom-in"), (Hit::Fit, "ui/fit")] {
            let area = Area::new(rx, 10, 34, 32);
            ui::icon_button(&mut p, ui, area, icon, hover == Some(hit), hit == Hit::Fit && fit);
            hits.add(area, hit);
            rx += 38;
        }
        let wall = Area::new(ww() - 196, 10, 184, 32);
        ui::button(&mut p, ui, wall, "Set as wallpaper", Style::Secondary, hover == Some(Hit::Wallpaper), self.image.is_some());
        hits.add(wall, Hit::Wallpaper);
        if !self.status.is_empty() {
            let tw = ui.font.measure(&self.status) + 28;
            let toast = Area::new((ww() - tw) / 2, hh() - 56, tw, 36);
            p.rounded(toast, 18, 0x000000, 190);
            ui::centered_text(&mut p, &ui.font, toast, &self.status, 0xffffff);
        }
        self.window.present();
    }
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&["ui/back", "ui/forward", "ui/zoom-in", "ui/zoom-out", "ui/fit"]);
    let window = match Window::open("Images", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxview: {}", e);
            return 1;
        }
    };
    let mut app = App {
        window,
        ui,
        files: Vec::new(),
        index: 0,
        image: None,
        fitted: None,
        error: String::new(),
        zoom: 1.0,
        fit: true,
        pan: (0.0, 0.0),
        drag: None,
        hits: Hits::new(),
        hover: None,
        status: String::new(),
    };
    app.window.set_icon("images");
    let start = env::args().get(1).cloned().unwrap_or_else(|| String::from("/usr/share/wallpapers/dusk.png"));
    let start = if sys::stat(&start).map(|s| s.is_dir()).unwrap_or(false) { format!("{}/", start.trim_end_matches('/')) } else { start };
    app.window.set_min_size(480, 320);
    app.load_dir(&start);
    app.draw();
    app.load_current();
    app.draw();
    let mut status_until = 0u64;

    loop {
        let timeout = if status_until > 0 { 300 } else { -1 };
        let event = app.window.wait_event(timeout);
        let mut redraw = false;
        match event {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Theme { .. }) => redraw = true,
            Some(Event::Resize { .. }) => {
                if app.fit {
                    app.refit();
                }
                redraw = true;
            }
            Some(Event::Key { code, .. }) => {
                redraw = true;
                match code {
                    -3 | 8 => app.step(-1),
                    -4 | 32 => app.step(1),
                    43 | 61 => app.set_zoom(app.zoom * 1.25),
                    45 => app.set_zoom(app.zoom / 1.25),
                    48 | 102 => {
                        app.fit = true;
                        app.pan = (0.0, 0.0);
                        app.refit();
                    }
                    27 => return 0,
                    _ => redraw = false,
                }
            }
            Some(Event::Mouse { x, y, kind, wheel, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    if let Some((sx, sy, px, py)) = app.drag {
                        app.pan = (px + (x - sx) as f32, py + (y - sy) as f32);
                        app.clamp_pan();
                        redraw = true;
                    } else {
                        let hover = if kind == MOUSE_LEAVE { None } else { app.hits.at(x, y) };
                        if hover != app.hover {
                            app.hover = hover;
                            redraw = true;
                        }
                    }
                }
                MOUSE_WHEEL => {
                    let factor = if wheel < 0 { 1.15 } else { 1.0 / 1.15 };
                    app.set_zoom(app.zoom * factor);
                    redraw = true;
                }
                MOUSE_PRESS => {
                    redraw = true;
                    match app.hits.at(x, y) {
                        Some(Hit::Prev) => app.step(-1),
                        Some(Hit::Next) => app.step(1),
                        Some(Hit::ZoomIn) => app.set_zoom(app.zoom * 1.25),
                        Some(Hit::ZoomOut) => app.set_zoom(app.zoom / 1.25),
                        Some(Hit::Fit) => {
                            app.fit = true;
                            app.pan = (0.0, 0.0);
                            app.refit();
                        }
                        Some(Hit::Wallpaper) => {
                            if let Some(path) = app.files.get(app.index).cloned() {
                                if ui::write_nook_config("wallpaper", &path) && hxclient::reload_desktop() {
                                    app.status = String::from("Wallpaper changed");
                                } else {
                                    app.status = String::from("Could not change the wallpaper");
                                }
                                status_until = sys::uptime_ms() + 2000;
                            }
                        }
                        Some(Hit::Canvas) => {
                            if !app.fit {
                                app.drag = Some((x, y, app.pan.0, app.pan.1));
                                app.window.set_cursor(CURSOR_MOVE);
                            }
                        }
                        None => {}
                    }
                }
                MOUSE_RELEASE => {
                    if app.drag.take().is_some() {
                        app.window.set_cursor(CURSOR_ARROW);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if status_until > 0 && sys::uptime_ms() >= status_until {
            status_until = 0;
            app.status.clear();
            redraw = true;
        }
        if redraw {
            app.draw();
        }
    }
}

entry!(main);
