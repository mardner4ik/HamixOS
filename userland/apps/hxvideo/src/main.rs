#![no_std]
#![no_main]

extern crate alloc;

mod player;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{audio, entry, env, eprintln, sys};
use hxclient::ui::{self, theme, Hits, Menu, MenuBar, MenuEntry, MenuResult, Ui};
use hxclient::{Event, Window, CURSOR_ARROW, CURSOR_HAND, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_RELEASE, MOUSE_WHEEL};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

use player::{Player, State};

const INITIAL_W: u32 = 960;
const INITIAL_H: u32 = 580;
const BAR_H: i32 = 76;
const HIDE_AFTER_MS: u64 = 2600;
const FILTERS: &str = "Videos|mp4,m4v,mov,3gp;All files|*";
const RECENT_KEY: &str = "video_recent";
const RECENT_MAX: usize = 6;

const CMD_OPEN: u32 = 1;
const CMD_OPEN_FOLDER: u32 = 2;
const CMD_CLOSE_VIDEO: u32 = 3;
const CMD_QUIT: u32 = 4;
const CMD_CLEAR_RECENT: u32 = 5;
const CMD_RECENT: u32 = 10;
const CMD_PLAY_PAUSE: u32 = 30;
const CMD_STOP: u32 = 31;
const CMD_BACK: u32 = 32;
const CMD_FORWARD: u32 = 33;
const CMD_PREVIOUS: u32 = 34;
const CMD_NEXT: u32 = 35;
const CMD_LOOP: u32 = 36;
const CMD_RESTART: u32 = 37;
const CMD_VOLUME_UP: u32 = 40;
const CMD_VOLUME_DOWN: u32 = 41;
const CMD_MUTE: u32 = 42;
const CMD_FULL: u32 = 50;
const CMD_PIN_CONTROLS: u32 = 51;
const CMD_INFO: u32 = 52;
const CMD_SHORTCUTS: u32 = 60;
const CMD_ABOUT: u32 = 61;

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Seek,
    PlayPause,
    Back,
    Forward,
    Mute,
    Volume,
    Fullscreen,
    Canvas,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Seek,
    Volume,
}

struct App {
    window: Window,
    ui: Ui,
    menu: MenuBar,
    player: Option<Player>,
    scaler: hx264::Scaler,
    hits: Hits<Hit>,
    hover: Option<Hit>,
    mouse: (i32, i32),
    last_motion: u64,
    controls_visible: bool,
    drag: Drag,
    drag_ms: i64,
    error: String,
    toast: String,
    toast_until: u64,
    name: String,
    maximized: bool,
    last_click: u64,
    volume: audio::Volume,
    full_redraw: bool,
    last_second: i64,
    playlist: Vec<String>,
    playlist_index: usize,
    recent: Vec<String>,
    looping: bool,
    pin_controls: bool,
}

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}

fn time_text(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 { format!("{}:{:02}:{:02}", h, m, s) } else { format!("{}:{:02}", m, s) }
}

fn load_recent() -> Vec<String> {
    ui::read_nook_config().into_iter().find(|(k, _)| k == RECENT_KEY).map(|(_, v)| v.split('|').filter(|p| !p.is_empty()).map(String::from).collect()).unwrap_or_default()
}

impl App {
    fn menu_shown(&self) -> bool {
        !self.maximized || self.controls_visible || self.menu.is_open()
    }

    fn top(&self) -> i32 {
        if self.maximized { 0 } else { MenuBar::HEIGHT }
    }

    fn refresh_menus(&mut self) {
        let has = self.player.is_some();
        let playing = self.player.as_ref().map(|p| p.state == State::Playing).unwrap_or(false);
        let mut file = alloc::vec![MenuEntry::item("Open video…", "Ctrl+O", CMD_OPEN), MenuEntry::item("Open folder…", "D", CMD_OPEN_FOLDER), MenuEntry::separator()];
        if self.recent.is_empty() {
            file.push(MenuEntry::item("No recent videos", "", 0).enabled(false));
        } else {
            for (i, path) in self.recent.iter().enumerate() {
                let name = path.rsplit('/').next().unwrap_or(path);
                file.push(MenuEntry::item(name, "", CMD_RECENT + i as u32).enabled(sys::stat(path).is_ok()));
            }
            file.push(MenuEntry::item("Clear the list", "", CMD_CLEAR_RECENT));
        }
        file.push(MenuEntry::separator());
        file.push(MenuEntry::item("Close video", "", CMD_CLOSE_VIDEO).enabled(has));
        file.push(MenuEntry::item("Quit", "Ctrl+Q", CMD_QUIT));
        let menus = alloc::vec![
            Menu::new("File", file),
            Menu::new(
                "Playback",
                alloc::vec![
                    MenuEntry::item(if playing { "Pause" } else { "Play" }, "Space", CMD_PLAY_PAUSE).enabled(has),
                    MenuEntry::item("Stop", "", CMD_STOP).enabled(has),
                    MenuEntry::item("Play from the start", "Home", CMD_RESTART).enabled(has),
                    MenuEntry::separator(),
                    MenuEntry::item("Back 10 seconds", "Left", CMD_BACK).enabled(has),
                    MenuEntry::item("Forward 10 seconds", "Right", CMD_FORWARD).enabled(has),
                    MenuEntry::separator(),
                    MenuEntry::item("Previous video", "P", CMD_PREVIOUS).enabled(self.playlist_index > 0 && self.playlist.len() > 1),
                    MenuEntry::item("Next video", "N", CMD_NEXT).enabled(self.playlist_index + 1 < self.playlist.len()),
                    MenuEntry::item("Repeat", "", CMD_LOOP).checked(self.looping),
                ]
            ),
            Menu::new(
                "Audio",
                alloc::vec![
                    MenuEntry::item("Volume up", "Up", CMD_VOLUME_UP),
                    MenuEntry::item("Volume down", "Down", CMD_VOLUME_DOWN),
                    MenuEntry::item("Mute", "M", CMD_MUTE).checked(self.volume.muted),
                ]
            ),
            Menu::new(
                "View",
                alloc::vec![
                    MenuEntry::item("Full window", "F", CMD_FULL).checked(self.maximized),
                    MenuEntry::item("Always show the controls", "", CMD_PIN_CONTROLS).checked(self.pin_controls),
                    MenuEntry::item("Video information", "I", CMD_INFO).enabled(has),
                ]
            ),
            Menu::new("Help", alloc::vec![MenuEntry::item("Keyboard shortcuts", "", CMD_SHORTCUTS), MenuEntry::item("About Videos", "", CMD_ABOUT)]),
        ];
        self.menu.set_menus(menus);
    }

    fn remember_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, String::from(path));
        self.recent.truncate(RECENT_MAX);
        ui::write_nook_config(RECENT_KEY, &self.recent.join("|"));
    }

    fn stage(&self) -> Area {
        let top = self.top();
        Area::new(0, top, ww(), hh() - top)
    }

    fn video_rect(&self) -> Area {
        let stage = self.stage();
        let (vw, vh) = self.player.as_ref().and_then(|p| p.video_size()).unwrap_or((16, 9));
        let (aw, ah) = (stage.w, stage.h);
        if vw == 0 || vh == 0 || aw <= 0 || ah <= 0 {
            return stage;
        }
        let scale_w = aw as i64 * vh as i64;
        let scale_h = ah as i64 * vw as i64;
        let (w, h) = if scale_w <= scale_h { (aw, (aw as i64 * vh as i64 / vw as i64) as i32) } else { ((ah as i64 * vw as i64 / vh as i64) as i32, ah) };
        Area::new((aw - w) / 2, stage.y + (ah - h) / 2, w & !1, h & !1)
    }

    fn bar_area(&self) -> Area {
        Area::new(0, hh() - BAR_H, ww(), BAR_H)
    }

    fn seek_track(&self) -> Area {
        let bar = self.bar_area();
        Area::new(bar.x + 18, bar.y + 12, bar.w - 36, 14)
    }

    fn volume_track(&self) -> Area {
        let bar = self.bar_area();
        Area::new(bar.right() - 16 - 36 - 16 - 96, bar.y + 44, 96, 16)
    }

    fn show_toast(&mut self, text: &str) {
        self.toast = String::from(text);
        self.toast_until = sys::uptime_ms() + 2500;
        self.full_redraw = true;
    }

    fn load(&mut self, path: &str) {
        self.player = None;
        self.error.clear();
        self.name = String::from(path.rsplit('/').next().unwrap_or(path));
        self.window.set_title(&format!("{} — Videos", self.name));
        self.full_redraw = true;
        self.draw();
        match Player::open(path) {
            Ok(mut p) => {
                if !p.notes.is_empty() {
                    let notes = p.notes.join(" · ");
                    self.show_toast(&notes);
                }
                p.play();
                self.player = Some(p);
                self.controls_visible = true;
                self.last_motion = sys::uptime_ms();
                self.remember_recent(path);
            }
            Err(e) => {
                self.error = e;
                self.window.set_title("Videos");
            }
        }
        self.full_redraw = true;
        self.refresh_menus();
    }

    fn close_video(&mut self) {
        self.player = None;
        self.error.clear();
        self.name.clear();
        self.playlist.clear();
        self.playlist_index = 0;
        self.window.set_title("Videos");
        self.full_redraw = true;
        self.refresh_menus();
    }

    fn videos_in(dir: &str) -> Vec<String> {
        let mut files: Vec<String> = sys::read_dir(dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !e.is_dir && ui::is_video_name(&e.name))
            .map(|e| if dir.ends_with('/') { format!("{}{}", dir, e.name) } else { format!("{}/{}", dir, e.name) })
            .collect();
        files.sort_by_key(|f| f.to_lowercase());
        files
    }

    fn open_path(&mut self, path: &str) {
        if sys::stat(path).map(|s| s.is_dir()).unwrap_or(false) {
            self.open_folder(path);
            return;
        }
        let dir = match path.rfind('/') {
            Some(0) => String::from("/"),
            Some(i) => String::from(&path[..i]),
            None => sys::getcwd(),
        };
        self.playlist = App::videos_in(&dir);
        self.playlist_index = self.playlist.iter().position(|p| p == path).unwrap_or(0);
        if self.playlist.is_empty() || self.playlist[self.playlist_index] != path {
            self.playlist = alloc::vec![String::from(path)];
            self.playlist_index = 0;
        }
        self.load(path);
    }

    fn open_folder(&mut self, dir: &str) {
        let list = App::videos_in(dir);
        if list.is_empty() {
            self.player = None;
            self.error = format!("No MP4 videos in {}", dir);
            self.full_redraw = true;
            self.refresh_menus();
            return;
        }
        self.playlist = list;
        self.playlist_index = 0;
        let first = self.playlist[0].clone();
        self.load(&first);
        if self.playlist.len() > 1 {
            let text = format!("Playing {} videos from {}", self.playlist.len(), dir.rsplit('/').next().unwrap_or(dir));
            self.show_toast(&text);
        }
    }

    fn step_playlist(&mut self, delta: i32) -> bool {
        if self.playlist.len() < 2 {
            return false;
        }
        let next = self.playlist_index as i32 + delta;
        if next < 0 || next >= self.playlist.len() as i32 {
            return false;
        }
        self.playlist_index = next as usize;
        let path = self.playlist[self.playlist_index].clone();
        self.load(&path);
        true
    }

    fn folder_dialog(&mut self) {
        let start = format!("{}/Videos", ui::home_dir());
        if let Some(p) = &mut self.player {
            p.pause();
        }
        if let Some(dir) = self.window.choose_folder_dialog("Play every video in a folder", &start) {
            self.open_folder(&dir);
        }
        self.full_redraw = true;
    }

    fn open_dialog(&mut self) {
        let start = self.playlist.get(self.playlist_index).and_then(|p| p.rfind('/').map(|i| String::from(&p[..i.max(1)]))).unwrap_or_else(|| format!("{}/Videos", ui::home_dir()));
        let was_playing = self.player.as_ref().map(|p| p.state == State::Playing).unwrap_or(false);
        if let Some(p) = &mut self.player {
            p.pause();
        }
        match self.window.open_file_dialog("Open a video", FILTERS, &start) {
            Some(path) => self.open_path(&path),
            None => {
                if was_playing {
                    if let Some(p) = &mut self.player {
                        p.play();
                    }
                }
            }
        }
        self.full_redraw = true;
    }

    fn seek_by(&mut self, delta: i64) {
        if let Some(p) = &mut self.player {
            let target = p.position_ms() + delta;
            p.seek(target);
            self.full_redraw = true;
        }
    }

    fn toggle_fullscreen(&mut self) {
        self.maximized = !self.maximized;
        self.window.set_maximized(self.maximized);
        self.full_redraw = true;
        self.refresh_menus();
    }

    fn change_volume(&mut self, result: Option<audio::Volume>) {
        if let Some(v) = result {
            self.volume = v;
        }
        self.full_redraw = true;
        self.last_motion = sys::uptime_ms();
        self.controls_visible = true;
        self.refresh_menus();
    }

    fn set_volume_from_x(&mut self, x: i32) {
        let track = self.volume_track();
        let level = ((x - track.x) as i64 * 100 / track.w.max(1) as i64).clamp(0, 100) as u32;
        let mut result = audio::set_volume(level);
        if self.volume.muted && level > 0 {
            result = audio::set_muted(false);
        }
        self.change_volume(result);
    }

    fn seek_from_x(&mut self, x: i32) -> i64 {
        let track = self.seek_track();
        let duration = self.player.as_ref().map(|p| p.duration_ms).unwrap_or(0);
        ((x - track.x) as i64 * duration / track.w.max(1) as i64).clamp(0, duration)
    }

    fn command(&mut self, command: u32) -> bool {
        match command {
            CMD_OPEN => self.open_dialog(),
            CMD_OPEN_FOLDER => self.folder_dialog(),
            CMD_CLOSE_VIDEO => self.close_video(),
            CMD_QUIT => return false,
            CMD_CLEAR_RECENT => {
                self.recent.clear();
                ui::write_nook_config(RECENT_KEY, "");
            }
            c if (CMD_RECENT..CMD_RECENT + RECENT_MAX as u32).contains(&c) => {
                if let Some(path) = self.recent.get((c - CMD_RECENT) as usize).cloned() {
                    self.open_path(&path);
                }
            }
            CMD_PLAY_PAUSE => {
                if let Some(p) = &mut self.player {
                    p.toggle();
                } else {
                    self.open_dialog();
                }
            }
            CMD_STOP => {
                if let Some(p) = &mut self.player {
                    p.pause();
                    p.seek(0);
                }
            }
            CMD_RESTART => {
                if let Some(p) = &mut self.player {
                    p.seek(0);
                    p.play();
                }
            }
            CMD_BACK => self.seek_by(-10_000),
            CMD_FORWARD => self.seek_by(10_000),
            CMD_PREVIOUS => {
                self.step_playlist(-1);
            }
            CMD_NEXT => {
                self.step_playlist(1);
            }
            CMD_LOOP => {
                self.looping = !self.looping;
                let text = if self.looping { "Repeat is on" } else { "Repeat is off" };
                self.show_toast(text);
            }
            CMD_VOLUME_UP => {
                let result = audio::step_volume(5);
                self.change_volume(result);
            }
            CMD_VOLUME_DOWN => {
                let result = audio::step_volume(-5);
                self.change_volume(result);
            }
            CMD_MUTE => {
                let result = audio::toggle_mute();
                self.change_volume(result);
            }
            CMD_FULL => self.toggle_fullscreen(),
            CMD_PIN_CONTROLS => self.pin_controls = !self.pin_controls,
            CMD_INFO => {
                if let Some(p) = &self.player {
                    let (decoded, dropped, skipped) = p.stats();
                    let text = format!("{} · decoded {} · dropped {} · skipped {}", p.describe(), decoded, dropped, skipped);
                    self.show_toast(&text);
                }
            }
            CMD_SHORTCUTS => self.show_toast("Space pause · Left/Right seek · Up/Down volume · N/P next, previous · F full window · Ctrl+O open"),
            CMD_ABOUT => self.show_toast("Videos — plays MP4 files with H.264 video and AAC sound"),
            _ => {}
        }
        self.last_motion = sys::uptime_ms();
        self.controls_visible = true;
        self.full_redraw = true;
        self.refresh_menus();
        true
    }

    fn draw_video(&mut self, p: &mut Painter) -> Area {
        let rect = self.video_rect();
        if let Some(player) = &self.player {
            if let Some(picture) = &player.current {
                let matrix = hx264::ColorMatrix::for_size(picture.width(), picture.height());
                let (x, y, w, h) = (rect.x.max(0) as usize, rect.y.max(0) as usize, rect.w.max(0) as usize, rect.h.max(0) as usize);
                let stride = ww() as usize;
                self.scaler.draw(&picture.planes(), matrix, p.buf, stride, x, y, w, h);
                return rect;
            }
        }
        p.fill(rect, 0x000000);
        rect
    }

    fn draw_placeholder(&mut self, p: &mut Painter) {
        let ui = &self.ui;
        let stage = self.stage();
        p.fill(stage, 0x000000);
        let center = Area::new(0, stage.y + (stage.h - BAR_H) / 2 - 70, ww(), 150);
        if let Some(icon) = ui.icon("apps/videos") {
            p.image(icon, (ww() - icon.w) / 2, center.y, if self.error.is_empty() { 150 } else { 110 });
        }
        let (title, detail, color) = if self.error.is_empty() {
            ("Choose a video", String::from("Open File in the menu and pick “Open video…” or “Open folder…”"), 0x9aa0ab)
        } else if self.error.starts_with("No MP4") {
            ("No videos here", self.error.clone(), 0xe3a33c)
        } else {
            ("This video cannot be played", self.error.clone(), 0xe3a33c)
        };
        ui::centered_text(p, &ui.title, Area::new(0, center.y + 62, ww(), 30), title, 0xeef0f4);
        ui::centered_text(p, &ui.font, Area::new(0, center.y + 96, ww(), 24), &detail, color);
    }

    fn draw_controls(&mut self, p: &mut Painter) {
        let ui = &self.ui;
        let hover = self.hover;
        let bar = self.bar_area();
        let has = self.player.is_some();
        for i in 0..40 {
            p.blend_fill(Area::new(0, bar.y - 40 + i, bar.w, 1), 0x000000, (i * 3) as u32);
        }
        p.blend_fill(bar, 0x000000, 150);
        let duration = self.player.as_ref().map(|pl| pl.duration_ms).unwrap_or(0).max(1);
        let position = if self.drag == Drag::Seek { self.drag_ms } else { self.player.as_ref().map(|pl| pl.position_ms()).unwrap_or(0) };
        let track = self.seek_track();
        let line = Area::new(track.x, track.y + track.h / 2 - 2, track.w, if has && (hover == Some(Hit::Seek) || self.drag == Drag::Seek) { 6 } else { 4 });
        p.rounded(line, 3, 0xffffff, if has { 60 } else { 30 });
        let filled = if has { (line.w as i64 * position.clamp(0, duration) / duration) as i32 } else { 0 };
        if filled > 0 {
            p.rounded(Area::new(line.x, line.y, filled.max(line.h), line.h), 3, theme::accent(), 255);
        }
        if has && (hover == Some(Hit::Seek) || self.drag == Drag::Seek) {
            p.circle((line.x + filled) as f32, line.y as f32 + line.h as f32 / 2.0, 7.0, 0xffffff, 255);
        }
        if has {
            self.hits.add(track.expand(4), Hit::Seek);
        }

        let row_y = bar.y + 36;
        let mut x = bar.x + 14;
        let playing = self.player.as_ref().map(|pl| pl.state == State::Playing).unwrap_or(false);
        for (hit, icon) in [(Hit::Back, "ui/seek-back"), (Hit::PlayPause, if playing { "ui/pause" } else { "ui/play" }), (Hit::Forward, "ui/seek-forward")] {
            let area = Area::new(x, row_y, 36, 32);
            if has && hover == Some(hit) {
                p.rounded(area, 8, 0xffffff, 40);
            }
            if let Some(img) = ui.icon(icon) {
                p.image_tinted(img, area.x + (area.w - img.w) / 2, area.y + (area.h - img.h) / 2, 0xffffff, if has { 255 } else { 90 });
            }
            if has {
                self.hits.add(area, hit);
            }
            x += 40;
        }
        let time = if has { format!("{}  /  {}", time_text(position), time_text(duration)) } else { String::from("0:00  /  0:00") };
        p.text(&ui.medium, x + 8, row_y + (32 - ui.medium.height()) / 2, &time, if has { 0xffffff } else { 0x80858f });

        let right = bar.right() - 14;
        let full = Area::new(right - 36, row_y, 36, 32);
        if hover == Some(Hit::Fullscreen) {
            p.rounded(full, 8, 0xffffff, 40);
        }
        if let Some(img) = ui.icon(if self.maximized { "ui/restore" } else { "ui/fullscreen" }) {
            p.image_tinted(img, full.x + (full.w - img.w) / 2, full.y + (full.h - img.h) / 2, 0xffffff, 255);
        }
        self.hits.add(full, Hit::Fullscreen);
        let vol_track = self.volume_track();
        let mute = Area::new(vol_track.x - 42, row_y, 36, 32);
        if hover == Some(Hit::Mute) {
            p.rounded(mute, 8, 0xffffff, 40);
        }
        let icon = match (self.volume.muted, self.volume.level) {
            (true, _) | (_, 0) => "ui/volume-mute",
            (_, 1..=33) => "ui/volume-1",
            (_, 34..=66) => "ui/volume-2",
            _ => "ui/volume-3",
        };
        if let Some(img) = ui.icon(icon) {
            p.image_tinted(img, mute.x + (mute.w - img.w) / 2, mute.y + (mute.h - img.h) / 2, 0xffffff, 255);
        }
        self.hits.add(mute, Hit::Mute);
        let vline = Area::new(vol_track.x, vol_track.y + vol_track.h / 2 - 2, vol_track.w, 4);
        p.rounded(vline, 2, 0xffffff, 60);
        let level = if self.volume.muted { 0 } else { self.volume.level };
        let vf = (vline.w as i64 * level as i64 / 100) as i32;
        if vf > 0 {
            p.rounded(Area::new(vline.x, vline.y, vf.max(4), 4), 2, 0xffffff, 230);
        }
        p.circle((vline.x + vf) as f32, vline.y as f32 + 2.0, if hover == Some(Hit::Volume) || self.drag == Drag::Volume { 7.0 } else { 5.0 }, 0xffffff, 255);
        self.hits.add(vol_track.expand(6), Hit::Volume);

        let info_x = x + 8 + ui.medium.measure(&time) + 24;
        let info_w = mute.x - 8 - info_x;
        if info_w > 80 && ww() > 700 {
            let info = match &self.player {
                Some(pl) => pl.describe(),
                None => String::from("No video"),
            };
            let text = ellipsize(&ui.small, &info, info_w);
            p.text(&ui.small, info_x, row_y + (32 - ui.small.height()) / 2, &text, 0xc8ccd4);
        }
    }

    fn draw_overlays(&mut self, p: &mut Painter) -> Option<Area> {
        let ui = &self.ui;
        let mut dirty = None;
        if let Some(player) = &self.player {
            if player.state != State::Playing && !self.controls_visible {
                let stage = self.stage();
                let area = Area::new((ww() - 72) / 2, stage.y + (stage.h - 72) / 2, 72, 72);
                p.circle(area.x as f32 + 36.0, area.y as f32 + 36.0, 36.0, 0x000000, 150);
                if let Some(img) = ui.icon("ui/play") {
                    p.image_tinted(img, area.x + 28, area.y + 28, 0xffffff, 255);
                }
                dirty = Some(area);
            }
        }
        if !self.toast.is_empty() {
            let tw = ui.font.measure(&self.toast).min(ww() - 80) + 32;
            let top = self.top() + 18;
            let area = Area::new((ww() - tw) / 2, top, tw, 36);
            p.rounded(area, 18, 0x000000, 190);
            let shown = ellipsize(&ui.font, &self.toast, tw - 24);
            ui::centered_text(p, &ui.font, area, &shown, 0xffffff);
            dirty = Some(dirty.map(|d| d.union(&area)).unwrap_or(area));
        }
        dirty
    }

    fn draw(&mut self) {
        let (w, h) = (ww(), hh());
        let buffer = self.window.buffer();
        let buf_ptr = buffer.as_mut_ptr();
        let len = buffer.len();
        let mut p = Painter::new(unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) }, w, h);
        self.hits.clear();
        let menu_open = self.menu.is_open();
        let full = self.full_redraw || menu_open;
        let mut rect;
        if self.player.is_none() {
            self.draw_placeholder(&mut p);
            rect = Area::new(0, 0, w, h);
        } else {
            if full {
                p.fill(Area::new(0, 0, w, h), 0x000000);
            }
            rect = self.draw_video(&mut p);
        }
        self.hits.add(self.stage(), Hit::Canvas);
        if self.controls_visible || self.player.is_none() {
            self.draw_controls(&mut p);
            rect = rect.union(&Area::new(0, h - BAR_H - 40, w, BAR_H + 40));
        }
        if let Some(extra) = self.draw_overlays(&mut p) {
            rect = rect.union(&extra);
        }
        if self.menu_shown() {
            let ui = &self.ui;
            self.menu.draw(&mut p, ui, w);
            rect = rect.union(&Area::new(0, 0, w, MenuBar::HEIGHT));
            self.menu.draw_dropdown(&mut p, ui, w, h);
        }
        if full {
            self.window.present();
        } else {
            self.window.present_area(rect);
        }
        self.full_redraw = menu_open;
    }

    fn update_controls(&mut self, now: u64) {
        let playing = self.player.as_ref().map(|p| p.state == State::Playing).unwrap_or(false);
        let over_bar = self.mouse.1 >= hh() - BAR_H - 20 && self.mouse.1 >= 0;
        let over_menu = self.mouse.1 >= 0 && self.mouse.1 < MenuBar::HEIGHT;
        let want = !playing || self.pin_controls || over_bar || over_menu || self.menu.is_open() || self.drag != Drag::None || now.saturating_sub(self.last_motion) < HIDE_AFTER_MS;
        if want != self.controls_visible {
            self.controls_visible = want;
            self.full_redraw = true;
            self.window.set_cursor(CURSOR_ARROW);
        }
    }

    fn press(&mut self, x: i32, y: i32, button: i32) {
        let now = sys::uptime_ms();
        match self.hits.at(x, y) {
            Some(Hit::PlayPause) => {
                if let Some(p) = &mut self.player {
                    p.toggle();
                }
            }
            Some(Hit::Back) => self.seek_by(-10_000),
            Some(Hit::Forward) => self.seek_by(10_000),
            Some(Hit::Fullscreen) => self.toggle_fullscreen(),
            Some(Hit::Mute) => {
                let result = audio::toggle_mute();
                self.change_volume(result);
            }
            Some(Hit::Volume) => {
                self.drag = Drag::Volume;
                self.set_volume_from_x(x);
            }
            Some(Hit::Seek) => {
                self.drag = Drag::Seek;
                self.drag_ms = self.seek_from_x(x);
            }
            Some(Hit::Canvas) if button == 1 && self.player.is_some() => {
                if now.saturating_sub(self.last_click) < 400 {
                    self.toggle_fullscreen();
                    self.last_click = 0;
                } else {
                    self.last_click = now;
                    if let Some(p) = &mut self.player {
                        p.toggle();
                    }
                }
            }
            _ => {}
        }
        self.full_redraw = true;
        self.refresh_menus();
    }

    fn release(&mut self, x: i32) {
        match self.drag {
            Drag::Seek => {
                let target = self.seek_from_x(x);
                if let Some(p) = &mut self.player {
                    p.seek(target);
                }
            }
            Drag::Volume => self.set_volume_from_x(x),
            Drag::None => {}
        }
        self.drag = Drag::None;
        self.full_redraw = true;
    }

    fn key(&mut self, code: i32) -> bool {
        let command = match code {
            32 | 107 | 10 => CMD_PLAY_PAUSE,
            -3 | 106 => {
                self.seek_by(-5_000);
                0
            }
            -4 | 108 => {
                self.seek_by(5_000);
                0
            }
            -8 => {
                self.seek_by(-60_000);
                0
            }
            -9 => {
                self.seek_by(60_000);
                0
            }
            -5 => CMD_RESTART,
            -1 => CMD_VOLUME_UP,
            -2 => CMD_VOLUME_DOWN,
            109 | 77 => CMD_MUTE,
            102 | 70 => CMD_FULL,
            110 | 78 => CMD_NEXT,
            112 | 80 => CMD_PREVIOUS,
            100 | 68 => CMD_OPEN_FOLDER,
            111 | 79 | 15 => CMD_OPEN,
            105 | 73 => CMD_INFO,
            17 => CMD_QUIT,
            27 => {
                if self.maximized {
                    CMD_FULL
                } else {
                    return false;
                }
            }
            _ => return true,
        };
        if command != 0 && !self.command(command) {
            return false;
        }
        self.last_motion = sys::uptime_ms();
        self.controls_visible = true;
        self.full_redraw = true;
        true
    }
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&["apps/videos", "ui/play", "ui/pause", "ui/seek-back", "ui/seek-forward", "ui/fullscreen", "ui/restore", "ui/volume-mute", "ui/volume-1", "ui/volume-2", "ui/volume-3"]);
    let window = match Window::open("Videos", INITIAL_W, INITIAL_H) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxvideo: {}", e);
            return 1;
        }
    };
    let mut app = App {
        window,
        ui,
        menu: MenuBar::new(Vec::new()),
        player: None,
        scaler: hx264::Scaler::new(),
        hits: Hits::new(),
        hover: None,
        mouse: (-1, -1),
        last_motion: sys::uptime_ms(),
        controls_visible: true,
        drag: Drag::None,
        drag_ms: 0,
        error: String::new(),
        toast: String::new(),
        toast_until: 0,
        name: String::new(),
        maximized: false,
        last_click: 0,
        volume: audio::volume().unwrap_or_default(),
        full_redraw: true,
        last_second: -1,
        playlist: Vec::new(),
        playlist_index: 0,
        recent: load_recent(),
        looping: false,
        pin_controls: false,
    };
    app.refresh_menus();
    app.window.set_icon("videos");
    app.window.set_min_size(420, 280);
    app.draw();
    if let Some(path) = env::args().get(1).cloned() {
        app.open_path(&path);
        app.draw();
    }
    let mut was_ended = false;

    loop {
        let timeout = match &mut app.player {
            Some(p) => p.tick() as i64,
            None => -1,
        };
        let now = sys::uptime_ms();
        let mut redraw = false;
        let ended = app.player.as_ref().map(|p| p.state == State::Ended).unwrap_or(false);
        if ended && !was_ended {
            if app.looping {
                if let Some(p) = &mut app.player {
                    p.seek(0);
                    p.play();
                }
            } else {
                app.step_playlist(1);
            }
            app.refresh_menus();
            redraw = true;
        }
        was_ended = app.player.as_ref().map(|p| p.state == State::Ended).unwrap_or(false);
        if let Some(p) = &app.player {
            if p.new_frame {
                redraw = true;
            }
            let second = p.position_ms() / 1000;
            if app.controls_visible && second != app.last_second {
                app.last_second = second;
                redraw = true;
            }
        }
        if let Some(v) = audio::volume() {
            if v != app.volume {
                app.volume = v;
                app.refresh_menus();
                if app.controls_visible {
                    redraw = true;
                }
            }
        }
        if app.toast_until != 0 && now >= app.toast_until {
            app.toast_until = 0;
            app.toast.clear();
            app.full_redraw = true;
            redraw = true;
        }
        let controls_before = app.controls_visible;
        app.update_controls(now);
        if controls_before != app.controls_visible {
            redraw = true;
        }
        let playing = app.player.as_ref().map(|p| p.state == State::Playing).unwrap_or(false);
        let wait = if redraw {
            0
        } else if playing {
            timeout.clamp(1, 20)
        } else if app.player.is_some() {
            250
        } else if app.toast_until != 0 {
            300
        } else {
            -1
        };
        let mut closing = false;
        let mut first = true;
        while let Some(event) = app.window.wait_event(if first { wait } else { 0 }) {
            first = false;
            match event {
                Event::Close { .. } => {
                    closing = true;
                    break;
                }
                Event::Resize { .. } | Event::Theme { .. } => {
                    app.full_redraw = true;
                    redraw = true;
                }
                Event::Focus { focused, .. } => {
                    if !focused && app.menu.close() {
                        app.full_redraw = true;
                        redraw = true;
                    }
                }
                Event::Mouse { x, y, kind, wheel, .. } => {
                    app.mouse = (x, y);
                    match kind {
                        MOUSE_MOVE => {
                            app.last_motion = sys::uptime_ms();
                            if app.menu_shown() && app.menu.motion(x, y) {
                                app.full_redraw = true;
                                redraw = true;
                            }
                            match app.drag {
                                Drag::Seek => {
                                    app.drag_ms = app.seek_from_x(x);
                                    redraw = true;
                                }
                                Drag::Volume => {
                                    app.set_volume_from_x(x);
                                    redraw = true;
                                }
                                Drag::None => {
                                    let hover = if app.menu.contains(x, y) { None } else { app.hits.at(x, y).filter(|h| *h != Hit::Canvas) };
                                    if hover != app.hover {
                                        app.hover = hover;
                                        app.window.set_cursor(if matches!(hover, Some(Hit::Seek) | Some(Hit::Volume)) { CURSOR_HAND } else { CURSOR_ARROW });
                                        redraw = true;
                                    }
                                }
                            }
                            if !app.controls_visible {
                                app.update_controls(sys::uptime_ms());
                                redraw = true;
                            }
                        }
                        MOUSE_LEAVE => {
                            app.mouse = (-1, -1);
                            if app.menu.leave() {
                                app.full_redraw = true;
                                redraw = true;
                            }
                            if app.hover.take().is_some() {
                                redraw = true;
                            }
                        }
                        MOUSE_PRESS => {
                            app.last_motion = sys::uptime_ms();
                            app.update_controls(app.last_motion);
                            redraw = true;
                            let result = if app.menu_shown() { app.menu.press(x, y) } else { MenuResult::Ignored };
                            match result {
                                MenuResult::Command(c) => {
                                    if !app.command(c) {
                                        closing = true;
                                        break;
                                    }
                                }
                                MenuResult::Consumed => app.full_redraw = true,
                                MenuResult::Ignored => app.press(x, y, wheel),
                            }
                        }
                        MOUSE_RELEASE => {
                            if app.drag != Drag::None {
                                app.release(x);
                                redraw = true;
                            }
                        }
                        MOUSE_WHEEL => {
                            if !app.menu.is_open() {
                                let result = audio::step_volume(if wheel < 0 { 5 } else { -5 });
                                app.change_volume(result);
                                redraw = true;
                            }
                        }
                        _ => {}
                    }
                }
                Event::Key { code, .. } => {
                    redraw = true;
                    match app.menu.key(code) {
                        MenuResult::Command(c) => {
                            if !app.command(c) {
                                closing = true;
                                break;
                            }
                        }
                        MenuResult::Consumed => {
                            app.controls_visible = true;
                            app.full_redraw = true;
                        }
                        MenuResult::Ignored => {
                            if !app.key(code) {
                                closing = true;
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if closing || app.window.is_closed() {
            return 0;
        }
        if redraw {
            app.draw();
        }
    }
}

entry!(main);
