use alloc::format;
use alloc::string::String;
use hamix_std::audio;
use hamix_std::sys;
use hxclient::ui::{self, theme, Style};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

use crate::panel::popup_frame;
use crate::{Grab, Nook, Popup, DOCK_MARGIN, TOP_H};

const SOUND_W: i32 = 320;
const OSD_MS: u64 = 1600;
const SAVE_DELAY_MS: u64 = 1500;

pub struct SoundState {
    pub available: bool,
    pub level: u32,
    pub muted: bool,
    pub changes: u64,
    pub device: String,
    pub outputs: String,
    pub osd_until: u64,
    pub osd_shown: bool,
    pub quiet_changes: u64,
    pub save_at: u64,
    pub last_info: u64,
    pub last_poll: u64,
}

impl SoundState {
    pub fn new() -> SoundState {
        SoundState { available: false, level: 0, muted: false, changes: 0, device: String::new(), outputs: String::new(), osd_until: 0, osd_shown: false, quiet_changes: 0, save_at: 0, last_info: 0, last_poll: 0 }
    }
}

pub fn volume_icon(level: u32, muted: bool, available: bool) -> &'static str {
    if !available {
        return "ui/volume-off";
    }
    if muted || level == 0 {
        return "ui/volume-mute";
    }
    match level {
        1..=33 => "ui/volume-1",
        34..=66 => "ui/volume-2",
        _ => "ui/volume-3",
    }
}

impl Nook {
    pub fn volume_button(&self) -> Area {
        let net = self.network_button();
        Area::new(net.x - 42, 4, 40, TOP_H - 8)
    }

    pub fn sound_area(&self) -> Area {
        let button = self.volume_button();
        let x = (button.x + button.w / 2 - SOUND_W / 2).clamp(8, (self.core.screen.width - SOUND_W - 8).max(8));
        let h = if self.sound.available { 186 } else { 110 };
        Area::new(x, TOP_H + 6, SOUND_W, h)
    }

    fn sound_slider(&self) -> Area {
        let a = self.sound_area();
        Area::new(a.x + 58, a.y + 92, a.w - 58 - 64, 24)
    }

    fn sound_mute_button(&self) -> Area {
        let a = self.sound_area();
        Area::new(a.x + 14, a.y + 86, 36, 36)
    }

    fn sound_settings_button(&self) -> Area {
        let a = self.sound_area();
        Area::new(a.x + 16, a.bottom() - 46, 150, 32)
    }

    pub fn init_sound(&mut self) {
        let cfg = ui::read_nook_config();
        let get = |key: &str| cfg.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        if audio::volume().is_some() {
            if let Some(level) = get("audio_volume").and_then(|v| v.parse::<u32>().ok()) {
                audio::set_volume(level);
            }
            if let Some(muted) = get("audio_muted") {
                audio::set_muted(muted == "yes");
            }
        }
        self.refresh_sound_info();
        self.sound.quiet_changes = self.sound.changes;
    }

    pub fn refresh_sound_info(&mut self) {
        let info = audio::info();
        self.sound.available = info.available;
        self.sound.device = info.device;
        self.sound.outputs = info.outputs;
        self.sound.last_info = sys::uptime_ms();
        if let Some(v) = audio::volume() {
            self.sound.level = v.level;
            self.sound.muted = v.muted;
            self.sound.changes = v.changes;
        }
    }

    pub fn poll_sound(&mut self, now: u64) {
        if now.saturating_sub(self.sound.last_poll) < 120 && self.sound.save_at == 0 && !self.sound.osd_shown {
            return;
        }
        self.sound.last_poll = now;
        if self.sound.available || now.saturating_sub(self.sound.last_info) > 5000 {
            if let Some(v) = audio::volume() {
                if !self.sound.available {
                    self.refresh_sound_info();
                    let button = self.volume_button();
                    self.core.add_damage(button.expand(4));
                }
                if v.changes != self.sound.changes {
                    let icon_before = volume_icon(self.sound.level, self.sound.muted, true);
                    self.sound.level = v.level;
                    self.sound.muted = v.muted;
                    self.sound.changes = v.changes;
                    self.sound.save_at = now + SAVE_DELAY_MS;
                    if icon_before != volume_icon(v.level, v.muted, true) {
                        let button = self.volume_button();
                        self.core.add_damage(button.expand(4));
                    }
                    if matches!(self.popup, Popup::Sound) {
                        let area = self.sound_area();
                        self.core.add_damage(area.expand(32));
                    } else if v.changes != self.sound.quiet_changes {
                        self.show_volume_osd(now);
                    }
                }
            } else if self.sound.available {
                self.sound.available = false;
                let button = self.volume_button();
                self.core.add_damage(button.expand(4));
            }
        }
        if self.sound.osd_shown && now >= self.sound.osd_until {
            self.sound.osd_shown = false;
            let area = self.volume_osd_area();
            self.core.add_damage(area.expand(24));
        }
        if self.sound.save_at != 0 && now >= self.sound.save_at {
            self.sound.save_at = 0;
            ui::write_nook_config("audio_volume", &format!("{}", self.sound.level));
            ui::write_nook_config("audio_muted", if self.sound.muted { "yes" } else { "no" });
        }
    }

    fn show_volume_osd(&mut self, now: u64) {
        self.sound.osd_until = now + OSD_MS;
        self.sound.osd_shown = true;
        let area = self.volume_osd_area();
        self.core.add_damage(area.expand(24));
    }

    fn apply_volume(&mut self, result: Option<audio::Volume>) {
        if let Some(v) = result {
            self.sound.quiet_changes = v.changes;
        }
        self.poll_sound(sys::uptime_ms());
    }

    pub fn set_volume_from_x(&mut self, x: i32) {
        let slider = self.sound_slider();
        let level = ((x - slider.x) as i64 * 100 / slider.w.max(1) as i64).clamp(0, 100) as u32;
        let result = audio::set_volume(level);
        let result = if level > 0 && self.sound.muted { audio::set_muted(false) } else { result };
        self.apply_volume(result);
    }

    pub fn volume_wheel(&mut self, wheel: i32) {
        let result = audio::step_volume(if wheel < 0 { 5 } else { -5 });
        if let Some(v) = result {
            self.sound.changes = v.changes.wrapping_sub(1);
        }
        self.poll_sound(sys::uptime_ms());
    }

    pub fn sound_press(&mut self, x: i32, y: i32) {
        if !self.sound.available {
            return;
        }
        let area = self.sound_area();
        if self.sound_mute_button().contains(x, y) {
            let result = audio::toggle_mute();
            self.apply_volume(result);
        } else if self.sound_slider().expand(8).contains(x, y) {
            self.grab = Grab::VolumeDrag;
            self.set_volume_from_x(x);
        } else if self.sound_settings_button().contains(x, y) {
            self.set_popup(Popup::None);
            self.launch_role(crate::ROLE_SETTINGS, &["sound"]);
            return;
        }
        self.core.add_damage(area.expand(32));
    }

    pub fn sound_hover_index(&self, x: i32, y: i32) -> Option<usize> {
        if self.sound_mute_button().contains(x, y) {
            Some(0)
        } else if self.sound_slider().expand(8).contains(x, y) {
            Some(1)
        } else if self.sound_settings_button().contains(x, y) {
            Some(2)
        } else {
            None
        }
    }

    pub fn draw_sound(&mut self, area: Area, hover: Option<usize>) {
        let ui = self.ui();
        let (mute, slider, settings) = (self.sound_mute_button(), self.sound_slider(), self.sound_settings_button());
        let state = (self.sound.available, self.sound.level, self.sound.muted);
        let device = self.sound.device.clone();
        let outputs = self.sound.outputs.clone();
        let mut p = self.core.painter_clipped();
        popup_frame(&mut p, area, 14);
        p.text(&ui.title, area.x + 18, area.y + 14, "Sound", theme::text());
        let (available, level, muted) = state;
        if !available {
            p.text(&ui.font, area.x + 18, area.y + 48, "No sound device was found", theme::dim());
            p.text(&ui.small, area.x + 18, area.y + 72, "Supported: Intel HD Audio, AC'97", theme::faint());
            return;
        }
        p.text(&ui.font, area.x + 18, area.y + 46, &ellipsize(&ui.font, &device, area.w - 36), theme::dim());
        if !outputs.is_empty() {
            p.text(&ui.small, area.x + 18, area.y + 66, &ellipsize(&ui.small, &outputs, area.w - 36), theme::faint());
        }
        if hover == Some(0) || muted {
            p.rounded(mute, 8, if muted { theme::selection() } else { theme::hover() }, 255);
        }
        if let Some(img) = ui.icon(volume_icon(level, muted, true)) {
            p.image_tinted(img, mute.x + (mute.w - img.w) / 2, mute.y + (mute.h - img.h) / 2, if muted { theme::accent_hover() } else { theme::text() }, 255);
        }
        draw_slider(&mut p, slider, level, muted, hover == Some(1));
        let label = if muted { String::from("Muted") } else { format!("{}%", level) };
        let lw = ui.medium.measure(&label);
        p.text(&ui.medium, area.right() - 18 - lw, slider.y + (slider.h - ui.medium.height()) / 2, &label, if muted { theme::faint() } else { theme::text() });
        ui::button(&mut p, ui, settings, "Sound settings…", Style::Flat, hover == Some(2), true);
    }

    pub fn volume_osd_area(&self) -> Area {
        let w = 300;
        let h = 58;
        let bottom = if self.dock_visible() { self.core.screen.height - self.dock_h() - DOCK_MARGIN * 2 - 12 } else { self.core.screen.height - 48 };
        Area::new((self.core.screen.width - w) / 2, bottom - h, w, h)
    }

    pub fn draw_volume_osd(&mut self) {
        if !self.sound.osd_shown {
            return;
        }
        let area = self.volume_osd_area();
        if !area.expand(24).overlaps(&self.core.screen.clip) {
            return;
        }
        let ui = self.ui();
        let (level, muted) = (self.sound.level, self.sound.muted);
        let mut p = self.core.painter_clipped();
        p.shadow(area, 16, 20, 200, 6);
        let pal = theme::palette();
        p.rounded(area, 16, pal.osd, 245);
        p.rounded_border(area, 16, pal.edge, pal.edge_alpha + 4);
        if let Some(img) = ui.icon(volume_icon(level, muted, true)) {
            p.image_tinted(img, area.x + 20, area.y + (area.h - img.h) / 2, pal.text, 255);
        }
        let bar = Area::new(area.x + 52, area.y + area.h / 2 - 4, area.w - 52 - 64, 8);
        p.rounded(bar, 4, theme::surface_2(), 255);
        if !muted && level > 0 {
            let filled = (bar.w as i64 * level as i64 / 100) as i32;
            p.rounded(Area::new(bar.x, bar.y, filled.max(bar.h), bar.h), 4, theme::accent(), 255);
        }
        let label = if muted { String::from("Muted") } else { format!("{}%", level) };
        let lw = ui.medium.measure(&label);
        p.text(&ui.medium, area.right() - 18 - lw, area.y + (area.h - ui.medium.height()) / 2, &label, pal.text);
    }
}

fn draw_slider(p: &mut Painter, area: Area, level: u32, muted: bool, hover: bool) {
    let track = Area::new(area.x, area.y + area.h / 2 - 3, area.w, 6);
    p.rounded(track, 3, theme::surface_2(), 255);
    let filled = (track.w as i64 * level.min(100) as i64 / 100) as i32;
    let color = if muted { theme::faint() } else if hover { theme::accent_hover() } else { theme::accent() };
    if filled > 0 {
        p.rounded(Area::new(track.x, track.y, filled.max(track.h), track.h), 3, color, 255);
    }
    let knob_x = track.x + filled;
    p.circle(knob_x as f32, area.y as f32 + area.h as f32 / 2.0, if hover { 9.0 } else { 8.0 }, 0xffffff, 255);
    p.circle(knob_x as f32, area.y as f32 + area.h as f32 / 2.0, 3.0, color, 255);
}
