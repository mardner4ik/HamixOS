#![no_std]
#![no_main]

extern crate alloc;

mod player;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{audio, env, fs, sys};
use hxtoolkit::{button, canvas, column, icon, row, slider, text, Align, App, CanvasEvent, Context, Element, Key, KeyPress, Length, MenuItem, MouseButton, Pixmap, Settings, TextSize, Tone};
use player::{Player, State};

const VIDEO_FILTERS: [&str; 4] = ["*.mp4", "*.m4v", "*.mov", "*.3gp"];
const RECENT_MAX: usize = 6;

#[derive(Clone)]
enum Msg {
    Tick,
    Open,
    Opened(Option<String>),
    OpenFolder,
    FolderChosen(Option<String>),
    Recent(usize),
    ClearRecent,
    CloseVideo,
    Quit,
    PlayPause,
    Stop,
    Seek(f32),
    Back,
    Forward,
    Previous,
    Next,
    Loop,
    Volume(f32),
    Mute,
    Info,
    Canvas(CanvasEvent),
}

struct Videos {
    player: Option<Player>,
    scaler: hx264::Scaler,
    frame: Pixmap,
    name: String,
    error: String,
    note: String,
    playlist: Vec<String>,
    index: usize,
    recent: Vec<String>,
    looping: bool,
    volume: Option<audio::Volume>,
    next_tick: u32,
}

fn is_video(name: &str) -> bool {
    let lower = name.to_lowercase();
    VIDEO_FILTERS.iter().any(|f| lower.ends_with(&f[1..]))
}

fn time_text(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn recent_file() -> String {
    format!("{}/.config/hxvideo/recent", env::var("HOME").unwrap_or("/"))
}

fn videos_in(dir: &str) -> Vec<String> {
    let mut files: Vec<String> = sys::read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && is_video(&e.name))
        .map(|e| if dir.ends_with('/') { format!("{}{}", dir, e.name) } else { format!("{}/{}", dir, e.name) })
        .collect();
    files.sort_by_key(|f| f.to_lowercase());
    files
}

fn parent(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => String::from(&path[..i]),
    }
}

impl Videos {
    fn new() -> Videos {
        let recent = fs::read_to_string(&recent_file()).unwrap_or_default().lines().filter(|l| !l.is_empty()).map(String::from).collect();
        Videos {
            player: None,
            scaler: hx264::Scaler::new(),
            frame: Pixmap::new(1, 1),
            name: String::new(),
            error: String::new(),
            note: String::new(),
            playlist: Vec::new(),
            index: 0,
            recent,
            looping: false,
            volume: audio::volume(),
            next_tick: 50,
        }
    }

    fn save_recent(&self) {
        let path = recent_file();
        let dir = parent(&path);
        sys::mkdir(&parent(&dir));
        sys::mkdir(&dir);
        fs::write(&path, self.recent.join("\n").as_bytes());
    }

    fn remember(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, String::from(path));
        self.recent.truncate(RECENT_MAX);
        self.save_recent();
    }

    fn load(&mut self, path: &str) {
        self.player = None;
        self.error.clear();
        self.note.clear();
        self.name = String::from(path.rsplit('/').next().unwrap_or(path));
        match Player::open(path) {
            Ok(mut p) => {
                if !p.notes.is_empty() {
                    self.note = p.notes.join(" · ");
                }
                p.play();
                self.player = Some(p);
                self.remember(path);
            }
            Err(e) => self.error = e,
        }
        self.render();
    }

    fn open_path(&mut self, path: &str) {
        if sys::stat(path).map(|s| s.is_dir()).unwrap_or(false) {
            self.open_folder(path);
            return;
        }
        self.playlist = videos_in(&parent(path));
        self.index = self.playlist.iter().position(|p| p == path).unwrap_or(usize::MAX);
        if self.index == usize::MAX {
            self.playlist = alloc::vec![String::from(path)];
            self.index = 0;
        }
        self.load(path);
    }

    fn open_folder(&mut self, dir: &str) {
        let list = videos_in(dir);
        if list.is_empty() {
            self.player = None;
            self.name.clear();
            self.error = format!("There are no MP4 videos in {}", dir);
            return;
        }
        self.playlist = list;
        self.index = 0;
        let first = self.playlist[0].clone();
        self.load(&first);
        if self.playlist.len() > 1 {
            self.note = format!("Playing {} videos from {}", self.playlist.len(), dir.rsplit('/').next().unwrap_or(dir));
        }
    }

    fn step(&mut self, delta: i32) -> bool {
        let next = self.index as i32 + delta;
        if self.playlist.len() < 2 || next < 0 || next >= self.playlist.len() as i32 {
            return false;
        }
        self.index = next as usize;
        let path = self.playlist[self.index].clone();
        self.load(&path);
        true
    }

    fn render(&mut self) {
        let (w, h) = (self.frame.width() as usize, self.frame.height() as usize);
        let pixels = self.frame.pixels_mut();
        pixels.fill(0xff00_0000);
        let Some(picture) = self.player.as_ref().and_then(|p| p.current.as_ref()) else {
            return;
        };
        let (pw, ph) = (picture.width() as usize, picture.height() as usize);
        if pw == 0 || ph == 0 || w == 0 || h == 0 {
            return;
        }
        let (dw, dh) = if pw * h > ph * w { (w, (ph * w / pw).max(1)) } else { ((pw * h / ph).max(1), h) };
        let (dx, dy) = ((w - dw) / 2, (h - dh) / 2);
        let matrix = hx264::ColorMatrix::for_size(picture.width(), picture.height());
        self.scaler.draw(&picture.planes(), matrix, pixels, w, dx, dy, dw, dh);
        for y in dy..dy + dh {
            for px in &mut pixels[y * w + dx..y * w + dx + dw] {
                *px |= 0xff00_0000;
            }
        }
    }

    fn open_dialog(&mut self, cx: &mut Context<Msg>) {
        if let Some(p) = self.player.as_mut() {
            p.pause();
        }
        cx.open_file("Open a video", &VIDEO_FILTERS, Msg::Opened);
    }

    fn seek_by(&mut self, delta: i64) {
        if let Some(p) = self.player.as_mut() {
            let target = p.position_ms() + delta;
            p.seek(target);
        }
    }

    fn context_menu(&self) -> Vec<MenuItem<Msg>> {
        let playing = self.player.as_ref().map(|p| p.state == State::Playing).unwrap_or(false);
        alloc::vec![
            MenuItem::new(if playing { "Pause" } else { "Play" }, Msg::PlayPause).shortcut("Space"),
            MenuItem::new("Back 10 seconds", Msg::Back).shortcut("Left"),
            MenuItem::new("Forward 10 seconds", Msg::Forward).shortcut("Right"),
            MenuItem::separator(),
            MenuItem::new("Repeat", Msg::Loop).checked(self.looping),
            MenuItem::new("Mute", Msg::Mute).checked(self.volume.as_ref().map(|v| v.muted).unwrap_or(false)),
            MenuItem::separator(),
            MenuItem::new("Open video…", Msg::Open),
            MenuItem::new("Information", Msg::Info),
        ]
    }

    fn welcome(&self) -> Element<Msg> {
        let (title, detail, tone) = if !self.error.is_empty() {
            ("This video cannot be played", self.error.clone(), Tone::Warning)
        } else {
            ("Videos", String::from("Plays MP4 files with H.264 video and AAC sound"), Tone::Dim)
        };
        let mut col = column![
            icon("apps/videos").size(72u32, 72u32),
            text(title).size(TextSize::Title),
            text(detail).tone(tone),
            row![button("Open video…").primary().on_press(Msg::Open), button("Play a folder…").on_press(Msg::OpenFolder)].spacing(10),
        ]
        .spacing(14)
        .align(Align::Center);
        if !self.recent.is_empty() {
            let mut list = column![text("Recently played").strong().tone(Tone::Dim)].spacing(6).align(Align::Center).padding([20, 0, 0, 0]);
            for (i, path) in self.recent.iter().enumerate() {
                list = list.push(button(String::from(path.rsplit('/').next().unwrap_or(path))).flat().icon("ui/play").on_press(Msg::Recent(i)));
            }
            col = col.push(list);
        }
        column![hxtoolkit::space(), col, hxtoolkit::space()].align(Align::Center).width(Length::Fill).height(Length::Fill).into()
    }

    fn controls(&self) -> Element<Msg> {
        let (position, duration, playing) = match &self.player {
            Some(p) => (p.position_ms(), p.duration_ms.max(1), p.state == State::Playing),
            None => (0, 1, false),
        };
        let volume = self.volume.as_ref();
        let level = volume.map(|v| v.level).unwrap_or(0);
        let muted = volume.map(|v| v.muted || v.level == 0).unwrap_or(true);
        let glyph = if muted { "ui/volume-mute" } else if level < 34 { "ui/volume-1" } else if level < 67 { "ui/volume-2" } else { "ui/volume-3" };
        let mut bar = row![
            button("").icon("ui/seek-back").flat().tooltip("Back 10 seconds").on_press(Msg::Back),
            button("").icon(if playing { "ui/pause" } else { "ui/play" }).flat().tooltip(if playing { "Pause" } else { "Play" }).on_press(Msg::PlayPause),
            button("").icon("ui/seek-forward").flat().tooltip("Forward 10 seconds").on_press(Msg::Forward),
            text(time_text(position)).small().width(52u32),
            slider(0.0..=duration as f32, position.clamp(0, duration) as f32, Msg::Seek).width(Length::Fill),
            text(time_text(duration)).small().tone(Tone::Dim).width(52u32),
        ]
        .spacing(8)
        .align(Align::Center)
        .padding([6, 12]);
        if volume.is_some() {
            bar = bar.push(button("").icon(glyph).flat().tooltip("Mute").on_press(Msg::Mute)).push(slider(0.0..=100.0, level as f32, Msg::Volume).step(1.0).width(100u32));
        }
        let status = if !self.note.is_empty() { self.note.clone() } else if self.playlist.len() > 1 { format!("{} · {} of {}", self.name, self.index + 1, self.playlist.len()) } else { self.name.clone() };
        column![bar, row![text(status).small().tone(Tone::Dim)].padding([0, 16, 6, 16])].spacing(0).into()
    }
}

impl App for Videos {
    type Message = Msg;

    fn title(&self) -> String {
        if self.name.is_empty() { String::from("Videos") } else { format!("{} — Videos", self.name) }
    }

    fn view(&self) -> Element<Msg> {
        if self.player.is_none() {
            return self.welcome();
        }
        column![canvas(self.frame.clone()).fill().on_event(|e| Some(Msg::Canvas(e))), self.controls()].spacing(0).width(Length::Fill).height(Length::Fill).into()
    }

    fn menu(&self) -> Vec<MenuItem<Msg>> {
        let has = self.player.is_some();
        let on = |m: MenuItem<Msg>, enabled: bool| if enabled { m } else { m.disabled() };
        let mut recent: Vec<MenuItem<Msg>> = self.recent.iter().enumerate().map(|(i, p)| MenuItem::new(String::from(p.rsplit('/').next().unwrap_or(p)), Msg::Recent(i))).collect();
        if !recent.is_empty() {
            recent.push(MenuItem::separator());
            recent.push(MenuItem::new("Clear the list", Msg::ClearRecent));
        } else {
            recent.push(MenuItem::new("Nothing yet", Msg::ClearRecent).disabled());
        }
        let playing = self.player.as_ref().map(|p| p.state == State::Playing).unwrap_or(false);
        alloc::vec![
            MenuItem::submenu(
                "File",
                alloc::vec![
                    MenuItem::new("Open video…", Msg::Open).shortcut("Ctrl+O"),
                    MenuItem::new("Play a folder…", Msg::OpenFolder),
                    MenuItem::submenu("Recent", recent),
                    MenuItem::separator(),
                    on(MenuItem::new("Close video", Msg::CloseVideo).shortcut("Ctrl+W"), has),
                    MenuItem::new("Quit", Msg::Quit).shortcut("Ctrl+Q"),
                ]
            ),
            MenuItem::submenu(
                "Playback",
                alloc::vec![
                    on(MenuItem::new(if playing { "Pause" } else { "Play" }, Msg::PlayPause).shortcut("Space"), has),
                    on(MenuItem::new("Stop", Msg::Stop), has),
                    MenuItem::separator(),
                    on(MenuItem::new("Back 10 seconds", Msg::Back).shortcut("Left"), has),
                    on(MenuItem::new("Forward 10 seconds", Msg::Forward).shortcut("Right"), has),
                    MenuItem::separator(),
                    on(MenuItem::new("Previous video", Msg::Previous).shortcut("P"), self.index > 0),
                    on(MenuItem::new("Next video", Msg::Next).shortcut("N"), self.index + 1 < self.playlist.len()),
                    MenuItem::new("Repeat", Msg::Loop).checked(self.looping),
                    MenuItem::separator(),
                    on(MenuItem::new("Information", Msg::Info).shortcut("Ctrl+I"), has),
                ]
            ),
            MenuItem::submenu(
                "Sound",
                alloc::vec![
                    MenuItem::new("Louder", Msg::Volume((self.volume.as_ref().map(|v| v.level).unwrap_or(0) + 5).min(100) as f32)).shortcut("Up"),
                    MenuItem::new("Quieter", Msg::Volume(self.volume.as_ref().map(|v| v.level).unwrap_or(0).saturating_sub(5) as f32)).shortcut("Down"),
                    MenuItem::new("Mute", Msg::Mute).shortcut("M").checked(self.volume.as_ref().map(|v| v.muted).unwrap_or(false)),
                ]
            ),
        ]
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        if press.mods.ctrl || press.mods.alt {
            return None;
        }
        let level = self.volume.as_ref().map(|v| v.level).unwrap_or(0);
        match press.key {
            Key::Char(' ') => Some(Msg::PlayPause),
            Key::Left => Some(Msg::Back),
            Key::Right => Some(Msg::Forward),
            Key::Up => Some(Msg::Volume((level + 5).min(100) as f32)),
            Key::Down => Some(Msg::Volume(level.saturating_sub(5) as f32)),
            Key::Char('m') | Key::Char('M') => Some(Msg::Mute),
            Key::Char('n') | Key::Char('N') => Some(Msg::Next),
            Key::Char('p') | Key::Char('P') => Some(Msg::Previous),
            _ => None,
        }
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        match &self.player {
            Some(p) if p.state == State::Playing => Some((self.next_tick.max(5), Msg::Tick)),
            Some(_) => Some((250, Msg::Tick)),
            None => None,
        }
    }

    fn started(&mut self, _cx: &mut Context<Msg>) {
        if let Some(path) = env::args().get(1).cloned() {
            self.open_path(&path);
        }
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Tick => {
                let mut ended = false;
                if let Some(p) = self.player.as_mut() {
                    self.next_tick = p.tick() as u32;
                    if p.new_frame {
                        self.render();
                    }
                    ended = self.player.as_ref().map(|p| p.state == State::Ended).unwrap_or(false);
                }
                if ended {
                    if self.looping {
                        if let Some(p) = self.player.as_mut() {
                            p.seek(0);
                            p.play();
                        }
                    } else {
                        self.step(1);
                    }
                }
            }
            Msg::Open => self.open_dialog(cx),
            Msg::Opened(path) => match path {
                Some(path) => self.open_path(&path),
                None => {
                    if let Some(p) = self.player.as_mut() {
                        p.play();
                    }
                }
            },
            Msg::OpenFolder => cx.choose_folder("Play every video in a folder", Msg::FolderChosen),
            Msg::FolderChosen(dir) => {
                if let Some(dir) = dir {
                    self.open_folder(&dir);
                }
            }
            Msg::Recent(i) => {
                if let Some(path) = self.recent.get(i).cloned() {
                    self.open_path(&path);
                }
            }
            Msg::ClearRecent => {
                self.recent.clear();
                self.save_recent();
            }
            Msg::CloseVideo => {
                self.player = None;
                self.name.clear();
                self.error.clear();
                self.playlist.clear();
            }
            Msg::Quit => cx.quit(),
            Msg::PlayPause => match self.player.as_mut() {
                Some(p) if p.state == State::Ended => {
                    p.seek(0);
                    p.play();
                }
                Some(p) => p.toggle(),
                None => self.open_dialog(cx),
            },
            Msg::Stop => {
                if let Some(p) = self.player.as_mut() {
                    p.pause();
                    p.seek(0);
                }
            }
            Msg::Seek(ms) => {
                if let Some(p) = self.player.as_mut() {
                    p.seek(ms as i64);
                }
            }
            Msg::Back => self.seek_by(-10_000),
            Msg::Forward => self.seek_by(10_000),
            Msg::Previous => {
                self.step(-1);
            }
            Msg::Next => {
                self.step(1);
            }
            Msg::Loop => {
                self.looping = !self.looping;
                self.note = String::from(if self.looping { "Repeat is on" } else { "Repeat is off" });
            }
            Msg::Volume(level) => {
                let level = (level as u32).min(100);
                self.volume = audio::set_volume(level);
                if level > 0 && self.volume.as_ref().map(|v| v.muted).unwrap_or(false) {
                    self.volume = audio::set_muted(false);
                }
            }
            Msg::Mute => self.volume = audio::toggle_mute().or(self.volume.take()),
            Msg::Info => {
                if let Some(p) = &self.player {
                    let (decoded, dropped, skipped) = p.stats();
                    cx.message(self.name.clone(), format!("{}\nFrames decoded: {} · dropped: {} · skipped: {}", p.describe(), decoded, dropped, skipped));
                }
            }
            Msg::Canvas(event) => match event {
                CanvasEvent::Resize { width, height } => {
                    self.frame.resize(width.max(1), height.max(1));
                    self.render();
                }
                CanvasEvent::Press { button: MouseButton::Left, .. } => self.update(Msg::PlayPause, cx),
                CanvasEvent::Press { button: MouseButton::Right, .. } => cx.show_menu(self.context_menu()),
                CanvasEvent::Wheel { delta, .. } => self.seek_by(-delta as i64 * 5_000),
                _ => {}
            },
        }
    }

    fn close_requested(&mut self, _cx: &mut Context<Msg>) -> bool {
        self.player = None;
        true
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("videos").icon("videos").size(960, 600).min_size(520, 360), Videos::new())
}

hxstd::entry!(run);
