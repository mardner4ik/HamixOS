#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{button, canvas, column, icon_button, row, text, App, CanvasEvent, Color, Context, Element, Key, KeyPress, Pixmap, Settings, Tone};
use vellum::Image;

const BACKGROUND: u32 = 0xff0e_1014;

struct Viewer {
    files: Vec<String>,
    index: usize,
    image: Option<Image>,
    scaled: Option<(f32, Image)>,
    error: String,
    zoom: f32,
    fit: bool,
    pan: (f32, f32),
    drag: Option<(i32, i32, f32, f32)>,
    size: (u32, u32),
    frame: Pixmap,
    status: String,
}

#[derive(Clone)]
enum Msg {
    Step(i32),
    Zoom(f32),
    Fit,
    Wallpaper,
    WallpaperSet(bool),
    ClearStatus,
    Canvas(CanvasEvent),
}

fn is_image(name: &str) -> bool {
    imago::extension_format(name).is_some()
}

fn name_of(path: &str) -> String {
    String::from(path.rsplit('/').next().unwrap_or(path))
}

impl Viewer {
    fn new() -> Viewer {
        let start = hxport::args().get(1).cloned().unwrap_or_else(|| String::from("/usr/share/wallpapers/dusk.png"));
        let mut v = Viewer {
            files: Vec::new(),
            index: 0,
            image: None,
            scaled: None,
            error: String::new(),
            zoom: 1.0,
            fit: true,
            pan: (0.0, 0.0),
            drag: None,
            size: (900, 560),
            frame: Pixmap::new(1, 1),
            status: String::new(),
        };
        let start = if hxport::is_dir(&start) { format!("{}/", start.trim_end_matches('/')) } else { start };
        v.load_dir(&start);
        v.load_current();
        v
    }

    fn load_dir(&mut self, path: &str) {
        let (dir, name) = match path.rfind('/') {
            Some(i) => (String::from(&path[..i.max(1)]), String::from(&path[i + 1..])),
            None => (hxport::var("PWD").unwrap_or_else(|| String::from("/")), String::from(path)),
        };
        let mut files: Vec<String> = hxport::entries(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !e.is_dir && is_image(&e.name))
            .map(|e| if dir == "/" { format!("/{}", e.name) } else { format!("{}/{}", dir, e.name) })
            .collect();
        files.sort();
        self.index = files.iter().position(|f| name_of(f) == name).unwrap_or(0);
        self.files = files;
    }

    fn load_current(&mut self) {
        self.image = None;
        self.scaled = None;
        self.error.clear();
        self.fit = true;
        self.pan = (0.0, 0.0);
        let Some(path) = self.files.get(self.index).cloned() else {
            self.error = String::from("No pictures in this folder");
            self.render();
            return;
        };
        let name = name_of(&path);
        match hxport::read(&path) {
            None => self.error = format!("Cannot read {}", name),
            Some(bytes) => match Image::decode(&bytes) {
                Some(img) => self.image = Some(img),
                None => self.error = format!("{} is not an image this viewer understands", name),
            },
        }
        self.refit();
    }

    fn fit_zoom(&self) -> f32 {
        let Some(img) = &self.image else {
            return 1.0;
        };
        let (w, h) = ((self.size.0 as f32 - 48.0).max(1.0), (self.size.1 as f32 - 48.0).max(1.0));
        (w / img.w as f32).min(h / img.h as f32).min(1.0)
    }

    fn refit(&mut self) {
        if self.fit {
            self.zoom = self.fit_zoom();
        }
        self.render();
    }

    fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.05, 8.0);
        self.fit = false;
        self.clamp_pan();
        self.render();
    }

    fn clamp_pan(&mut self) {
        if let Some(img) = &self.image {
            let max_x = ((img.w as f32 * self.zoom - self.size.0 as f32) / 2.0).max(0.0);
            let max_y = ((img.h as f32 * self.zoom - self.size.1 as f32) / 2.0).max(0.0);
            self.pan.0 = self.pan.0.clamp(-max_x, max_x);
            self.pan.1 = self.pan.1.clamp(-max_y, max_y);
        }
    }

    fn render(&mut self) {
        let (cw, ch) = (self.size.0.max(1), self.size.1.max(1));
        let mut frame = Pixmap::filled(cw, ch, Color(BACKGROUND));
        if let Some(img) = &self.image {
            let w = ((img.w as f32 * self.zoom) as i32).max(1);
            let h = ((img.h as f32 * self.zoom) as i32).max(1);
            if self.scaled.as_ref().map(|(z, _)| (*z - self.zoom).abs() > f32::EPSILON).unwrap_or(true) {
                let scaled = if w == img.w && h == img.h { Image { w, h, px: img.px.clone() } } else { img.scaled(w, h) };
                self.scaled = Some((self.zoom, scaled));
            }
            let x = (cw as i32 - w) / 2 + self.pan.0 as i32;
            let y = (ch as i32 - h) / 2 + self.pan.1 as i32;
            for ty in (y.max(0)..(y + h).min(ch as i32)).step_by(16) {
                for tx in (x.max(0)..(x + w).min(cw as i32)).step_by(16) {
                    let dark = ((tx - x) / 16 + (ty - y) / 16) % 2 == 0;
                    frame.fill_rect(tx, ty, 16.min(x + w - tx), 16.min(y + h - ty), Color(if dark { 0xff2a2d33 } else { 0xff33363d }));
                }
            }
            if let Some((_, scaled)) = &self.scaled {
                let stride = cw as usize;
                let pixels = frame.pixels_mut();
                for sy in 0..scaled.h {
                    let ty = y + sy;
                    if ty < 0 || ty >= ch as i32 {
                        continue;
                    }
                    for sx in 0..scaled.w {
                        let tx = x + sx;
                        if tx < 0 || tx >= cw as i32 {
                            continue;
                        }
                        let src = scaled.px[(sy * scaled.w + sx) as usize];
                        let a = src >> 24;
                        let i = ty as usize * stride + tx as usize;
                        pixels[i] = if a == 255 {
                            src
                        } else if a == 0 {
                            pixels[i]
                        } else {
                            let dst = pixels[i];
                            let mix = |shift: u32| ((((src >> shift) & 0xff) * a + ((dst >> shift) & 0xff) * (255 - a)) / 255) << shift;
                            0xff00_0000 | mix(16) | mix(8) | mix(0)
                        };
                    }
                }
            }
        }
        self.frame = frame;
    }

    fn step(&mut self, delta: i32) {
        if self.files.is_empty() {
            return;
        }
        let n = self.files.len() as i32;
        self.index = (self.index as i32 + delta).rem_euclid(n) as usize;
        self.load_current();
    }
}

impl App for Viewer {
    type Message = Msg;

    fn title(&self) -> String {
        match self.files.get(self.index) {
            Some(path) => format!("{} — Images", name_of(path)),
            None => String::from("Images"),
        }
    }

    fn view(&self) -> Element<Msg> {
        let name = self.files.get(self.index).map(|f| name_of(f)).unwrap_or_default();
        let info = match &self.image {
            Some(img) => format!("{}   {} × {}   {}%   {} / {}", name, img.w, img.h, (self.zoom * 100.0) as i32, self.index + 1, self.files.len()),
            None if !self.error.is_empty() => self.error.clone(),
            None => name,
        };
        let bar = row![
            icon_button("ui/back", "Previous picture").on_press(Msg::Step(-1)),
            icon_button("ui/forward", "Next picture").on_press(Msg::Step(1)),
            text(info).fill(),
            text(self.status.clone()).tone(Tone::Success),
            icon_button("ui/zoom-out", "Zoom out").on_press(Msg::Zoom(1.0 / 1.25)),
            icon_button("ui/zoom-in", "Zoom in").on_press(Msg::Zoom(1.25)),
            icon_button("ui/fit", "Fit to the window").on_press(Msg::Fit),
            button("Set as wallpaper").on_press_maybe(self.image.as_ref().map(|_| Msg::Wallpaper)),
        ]
        .spacing(6)
        .padding([8, 10]);
        column![bar, canvas(self.frame.clone()).fill().on_event(|ev| Some(Msg::Canvas(ev)))].spacing(0).fill().into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Step(d) => self.step(d),
            Msg::Zoom(f) => self.set_zoom(self.zoom * f),
            Msg::Fit => {
                self.fit = true;
                self.pan = (0.0, 0.0);
                self.refit();
            }
            Msg::Wallpaper => {
                if let Some(path) = self.files.get(self.index).cloned() {
                    cx.set_wallpaper(path, Msg::WallpaperSet);
                }
            }
            Msg::WallpaperSet(ok) => {
                self.status = String::from(if ok { "Wallpaper changed" } else { "Could not change the wallpaper" });
                cx.after(2000, Msg::ClearStatus);
            }
            Msg::ClearStatus => self.status.clear(),
            Msg::Canvas(ev) => match ev {
                CanvasEvent::Resize { width, height } => {
                    self.size = (width.max(1), height.max(1));
                    self.refit();
                }
                CanvasEvent::Wheel { delta, .. } => self.set_zoom(self.zoom * if delta < 0 { 1.15 } else { 1.0 / 1.15 }),
                CanvasEvent::Press { x, y, .. } => {
                    if !self.fit {
                        self.drag = Some((x, y, self.pan.0, self.pan.1));
                    }
                }
                CanvasEvent::Move { x, y } => {
                    if let Some((sx, sy, px, py)) = self.drag {
                        self.pan = (px + (x - sx) as f32, py + (y - sy) as f32);
                        self.clamp_pan();
                        self.render();
                    }
                }
                CanvasEvent::Release { .. } => self.drag = None,
            },
        }
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        if press.mods.ctrl || press.mods.alt {
            return None;
        }
        match press.key {
            Key::Left | Key::Backspace => Some(Msg::Step(-1)),
            Key::Right | Key::Char(' ') => Some(Msg::Step(1)),
            Key::Char('+') | Key::Char('=') => Some(Msg::Zoom(1.25)),
            Key::Char('-') => Some(Msg::Zoom(1.0 / 1.25)),
            Key::Char('0') | Key::Char('f') => Some(Msg::Fit),
            _ => None,
        }
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("images").size(900, 620).min_size(480, 320), Viewer::new())
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
