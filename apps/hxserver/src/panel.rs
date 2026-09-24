use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::net;
use hamix_std::sys::{self, MOUSE_LEFT};
use hxclient::ui::{self, theme, FieldAction, Style, TextField};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

use crate::{civil, days_from_civil, weekday, Body, MenuAction, MenuItem, Nook, Popup, PowerAction, TOP_H};

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const NET_W: i32 = 340;

impl Nook {
    pub fn top_bar(&self) -> Area {
        Area::new(0, 0, self.core.screen.width, TOP_H)
    }

    fn apps_button(&self) -> Area {
        Area::new(6, 4, 168, TOP_H - 8)
    }

    fn power_button(&self) -> Area {
        Area::new(self.core.screen.width - 50, 4, 42, TOP_H - 8)
    }

    fn user_x(&self) -> i32 {
        self.core.screen.width - 62 - self.ui.font.measure(&self.user)
    }

    pub fn network_button(&self) -> Area {
        Area::new(self.user_x() - 22 - 14 - 40, 4, 40, TOP_H - 8)
    }

    fn clock_text(&self) -> String {
        let now = sys::realtime().sec;
        let (_, month, day, hour, minute) = civil(now);
        format!("{} {} {}   {:02}:{:02}", weekday(now.div_euclid(86400)), day, &MONTHS[(month - 1) as usize][..3], hour, minute)
    }

    fn clock_button(&self) -> Area {
        let cw = self.ui.medium.measure(&self.clock_text());
        Area::new((self.core.screen.width - cw) / 2 - 12, 4, cw + 24, TOP_H - 8)
    }

    pub fn set_popup(&mut self, popup: Popup) {
        if let Some(old) = self.popup_area() {
            self.core.add_damage(old.expand(32));
        }
        if matches!(popup, Popup::Launcher) {
            self.reset_launcher();
            self.refresh_apps();
        }
        if matches!(popup, Popup::Network) {
            self.poll_network(true);
            self.net.message.clear();
        }
        if matches!(popup, Popup::Calendar) {
            let (year, month, _, _, _) = civil(sys::realtime().sec);
            self.calendar_month = (year, month);
        }
        if matches!(popup, Popup::Sound) {
            self.refresh_sound_info();
        }
        self.popup = popup;
        self.popup_dock = false;
        self.popup_hover = None;
        self.search = TextField::default();
        if let Some(new) = self.popup_area() {
            self.core.add_damage(new.expand(32));
        }
        let bar = self.top_bar();
        self.core.add_damage(bar);
    }

    fn power_area(&self) -> Area {
        Area::new(self.core.screen.width - 216, TOP_H + 6, 208, 3 * 36 + 16)
    }

    fn power_row(&self, index: usize) -> Area {
        let a = self.power_area();
        Area::new(a.x + 8, a.y + 8 + index as i32 * 36, a.w - 16, 34)
    }

    fn menu_area(&self, x: i32, y: i32, items: &[MenuItem]) -> Area {
        let w = 236;
        let h = items.len() as i32 * 34 + 12;
        Area::new(x.clamp(4, (self.core.screen.width - w - 4).max(4)), y.clamp(TOP_H + 2, (self.core.screen.height - h - 4).max(TOP_H + 2)), w, h)
    }

    fn menu_row(area: Area, index: usize) -> Area {
        Area::new(area.x + 6, area.y + 6 + index as i32 * 34, area.w - 12, 34)
    }

    fn calendar_area(&self) -> Area {
        let c = self.clock_button();
        Area::new((c.x + c.w / 2 - 160).clamp(8, self.core.screen.width - 328), TOP_H + 6, 320, 332)
    }

    fn network_mode_segments(&self) -> [Area; 3] {
        let a = self.network_area();
        let seg_w = (a.w - 32) / 3;
        [0, 1, 2].map(|i| Area::new(a.x + 16 + i * seg_w, a.y + 76, seg_w, 32))
    }

    fn network_rows_top(&self) -> i32 {
        self.network_area().y + 122
    }

    fn network_list(&self) -> Vec<net::WifiNetwork> {
        let mut list = self.net.networks.clone();
        list.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
        list.truncate(6);
        list
    }

    fn network_body_height(&self) -> i32 {
        match self.net.status.mode.as_str() {
            "wifi" => {
                if !self.net.status.has_kind("wifi") {
                    60
                } else {
                    let rows = self.network_list().len().max(1) as i32 * 42;
                    let expanded = if self.selected_needs_password() { 88 } else { 0 };
                    rows + expanded + 40
                }
            }
            "ethernet" => 5 * 24 + 12,
            _ => 44,
        }
    }

    fn selected_needs_password(&self) -> bool {
        match &self.net.selected {
            Some(ssid) => self.net.networks.iter().any(|n| &n.ssid == ssid && n.secured() && !n.connected),
            None => false,
        }
    }

    pub fn network_area(&self) -> Area {
        let x = (self.network_button().x + 20 - NET_W / 2).clamp(8, self.core.screen.width - NET_W - 8);
        let body = if self.net.status.available { self.network_body_height() } else { 60 };
        Area::new(x, TOP_H + 6, NET_W, 122 + body + 56)
    }

    fn network_row(&self, index: usize) -> Area {
        let a = self.network_area();
        let mut y = self.network_rows_top() + index as i32 * 42;
        if self.selected_needs_password() {
            let list = self.network_list();
            if let Some(pos) = list.iter().position(|n| Some(&n.ssid) == self.net.selected.as_ref()) {
                if index > pos {
                    y += 88;
                }
            }
        }
        Area::new(a.x + 8, y, a.w - 16, 40)
    }

    fn network_password_field(&self) -> Option<(Area, Area)> {
        if !self.selected_needs_password() {
            return None;
        }
        let list = self.network_list();
        let pos = list.iter().position(|n| Some(&n.ssid) == self.net.selected.as_ref())?;
        let row = self.network_row(pos);
        let field = Area::new(row.x + 8, row.bottom() + 6, row.w - 16, 36);
        let button = Area::new(row.right() - 104, field.bottom() + 8, 96, 32);
        Some((field, button))
    }

    fn network_settings_button(&self) -> Area {
        let a = self.network_area();
        Area::new(a.x + 16, a.bottom() - 44, 170, 32)
    }

    fn network_scan_button(&self) -> Area {
        let a = self.network_area();
        Area::new(a.right() - 16 - 96, a.bottom() - 44, 96, 32)
    }

    pub fn popup_area(&self) -> Option<Area> {
        match &self.popup {
            Popup::None => None,
            Popup::Launcher => Some(self.launcher_area()),
            Popup::Power => Some(self.power_area()),
            Popup::Network => Some(self.network_area()),
            Popup::Calendar => Some(self.calendar_area()),
            Popup::Sound => Some(self.sound_area()),
            Popup::DockOverflow => Some(self.overflow_area()),
            Popup::Menu { x, y, items } => Some(self.menu_area(*x, *y, items)),
        }
    }

    pub fn top_bar_press(&mut self, x: i32, button: u32) {
        if button != MOUSE_LEFT {
            return;
        }
        if self.apps_button().contains(x, TOP_H / 2) {
            self.set_popup(Popup::Launcher);
        } else if self.power_button().contains(x, TOP_H / 2) {
            self.set_popup(Popup::Power);
        } else if self.network_button().contains(x, TOP_H / 2) {
            self.set_popup(Popup::Network);
        } else if self.volume_button().contains(x, TOP_H / 2) {
            self.set_popup(Popup::Sound);
        } else if self.clock_button().contains(x, TOP_H / 2) {
            self.set_popup(Popup::Calendar);
        }
    }

    pub fn popup_press(&mut self, x: i32, y: i32, button: u32) -> bool {
        let Some(area) = self.popup_area() else {
            return false;
        };
        if !area.contains(x, y) {
            let toggled = match self.popup {
                Popup::Launcher => self.apps_button().contains(x, y),
                Popup::Power => self.power_button().contains(x, y),
                Popup::Network => self.network_button().contains(x, y),
                Popup::Calendar => self.clock_button().contains(x, y),
                Popup::Sound => self.volume_button().contains(x, y),
                Popup::DockOverflow => self.dock_hit(x, y).map(|i| self.dock_visible_items().get(i) == Some(&crate::dock::DockItem::More)).unwrap_or(false),
                _ => false,
            };
            self.set_popup(Popup::None);
            return toggled;
        }
        match self.popup.clone() {
            Popup::Launcher => self.launcher_press(x, y, button),
            Popup::DockOverflow => self.overflow_press(x, y, button),
            Popup::Power => {
                for (i, action) in [PowerAction::Logout, PowerAction::Reboot, PowerAction::Shutdown].iter().enumerate() {
                    if self.power_row(i).contains(x, y) {
                        let action = *action;
                        self.set_popup(Popup::None);
                        self.start_power(action);
                        return true;
                    }
                }
            }
            Popup::Menu { items, .. } => {
                for (i, item) in items.iter().enumerate() {
                    if Self::menu_row(area, i).contains(x, y) {
                        let action = item.action.clone();
                        self.set_popup(Popup::None);
                        self.perform(action);
                        return true;
                    }
                }
            }
            Popup::Calendar => {
                let prev = Area::new(area.x + 12, area.y + 12, 32, 30);
                let next = Area::new(area.right() - 44, area.y + 12, 32, 30);
                let (year, month) = self.calendar_month;
                if prev.contains(x, y) {
                    self.calendar_month = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
                } else if next.contains(x, y) {
                    self.calendar_month = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
                }
                self.core.add_damage(area);
            }
            Popup::Network => self.network_press(x, y),
            Popup::Sound => self.sound_press(x, y),
            Popup::None => {}
        }
        true
    }

    fn network_press(&mut self, x: i32, y: i32) {
        let area = self.network_area();
        let segments = self.network_mode_segments();
        for (i, seg) in segments.iter().enumerate() {
            if seg.contains(x, y) {
                let mode = [net::MODE_ETHERNET, net::MODE_WIFI, net::MODE_OFF][i];
                let r = net::set_mode(mode);
                if r < 0 {
                    self.net.message = format!("Cannot switch: {}", sys::error_name(r));
                } else {
                    self.net.message.clear();
                    if mode == net::MODE_WIFI {
                        self.net.networks = net::wifi_scan(true);
                        self.net.last_scan = sys::uptime_ms();
                    }
                }
                self.net.selected = None;
                self.poll_network(true);
                self.core.add_damage(area.expand(32));
                let new_area = self.network_area();
                self.core.add_damage(new_area.expand(32));
                return;
            }
        }
        if self.network_settings_button().contains(x, y) {
            self.set_popup(Popup::None);
            self.launch_role(crate::ROLE_SETTINGS, &["network"]);
            return;
        }
        if self.net.status.mode == "wifi" && self.network_scan_button().contains(x, y) {
            self.net.networks = net::wifi_scan(true);
            self.net.last_scan = sys::uptime_ms();
            self.core.add_damage(area.expand(32));
            return;
        }
        if let Some((field, button)) = self.network_password_field() {
            if field.contains(x, y) {
                return;
            }
            if button.contains(x, y) {
                self.connect_selected();
                return;
            }
        }
        if self.net.status.mode == "wifi" {
            let list = self.network_list();
            for (i, network) in list.iter().enumerate() {
                if self.network_row(i).contains(x, y) {
                    if network.connected {
                        net::wifi_disconnect();
                        self.net.message = format!("Disconnected from {}", network.ssid);
                    } else if network.secured() {
                        self.net.selected = if self.net.selected.as_ref() == Some(&network.ssid) { None } else { Some(network.ssid.clone()) };
                        self.net.password = TextField::secret();
                    } else {
                        self.net.selected = Some(network.ssid.clone());
                        self.connect_selected();
                    }
                    self.core.add_damage(area.expand(32));
                    let new_area = self.network_area();
                    self.core.add_damage(new_area.expand(32));
                    return;
                }
            }
        }
    }

    fn connect_selected(&mut self) {
        let Some(ssid) = self.net.selected.clone() else {
            return;
        };
        let r = net::wifi_connect(&ssid, &self.net.password.text);
        self.net.message = if r < 0 { format!("Cannot connect to {}: {}", ssid, sys::error_name(r)) } else { format!("Connecting to {}…", ssid) };
        self.net.password = TextField::secret();
        self.net.selected = None;
        self.poll_network(true);
        let area = self.network_area();
        self.core.add_damage(area.expand(120));
    }

    pub fn popup_hover_index(&self, x: i32, y: i32) -> Option<usize> {
        let area = self.popup_area()?;
        match &self.popup {
            Popup::Launcher => self.launcher_hover(x, y),
            Popup::DockOverflow => (0..self.dock_overflow().len().min(self.overflow_visible_rows())).find(|i| self.overflow_row(*i).contains(x, y)),
            Popup::Power => (0..3).find(|i| self.power_row(*i).contains(x, y)),
            Popup::Menu { items, .. } => (0..items.len()).find(|i| Self::menu_row(area, *i).contains(x, y)),
            Popup::Calendar => {
                if Area::new(area.x + 12, area.y + 12, 32, 30).contains(x, y) {
                    Some(0)
                } else if Area::new(area.right() - 44, area.y + 12, 32, 30).contains(x, y) {
                    Some(1)
                } else {
                    None
                }
            }
            Popup::Network => {
                if let Some(i) = self.network_mode_segments().iter().position(|s| s.contains(x, y)) {
                    return Some(i);
                }
                if self.net.status.mode == "wifi" {
                    if let Some(i) = (0..self.network_list().len()).find(|i| self.network_row(*i).contains(x, y)) {
                        return Some(10 + i);
                    }
                    if self.network_scan_button().contains(x, y) {
                        return Some(41);
                    }
                    if let Some((_, button)) = self.network_password_field() {
                        if button.contains(x, y) {
                            return Some(30);
                        }
                    }
                }
                if self.network_settings_button().contains(x, y) {
                    return Some(40);
                }
                None
            }
            Popup::Sound => self.sound_hover_index(x, y),
            Popup::None => None,
        }
    }

    pub fn popup_key(&mut self, code: i32) -> bool {
        match self.popup {
            Popup::None => false,
            Popup::Launcher => {
                self.launcher_key(code);
                true
            }
            Popup::Network if self.selected_needs_password() => {
                match code {
                    27 => {
                        self.net.selected = None;
                    }
                    _ => {
                        if let FieldAction::Submit = self.net.password.key(code) {
                            self.connect_selected();
                        }
                    }
                }
                let area = self.network_area();
                self.core.add_damage(area.expand(120));
                true
            }
            _ => {
                if code == 27 {
                    self.set_popup(Popup::None);
                }
                true
            }
        }
    }

    pub fn perform(&mut self, action: MenuAction) {
        match action {
            MenuAction::Launch(app) => self.launch(app, &[]),
            MenuAction::LaunchWith(app, arg) => self.launch(app, &[arg.as_str()]),
            MenuAction::PinApp(app) => self.pin_app(app, true),
            MenuAction::UnpinApp(app) => self.pin_app(app, false),
            MenuAction::CloseApp(app) => {
                for id in self.app_windows(app) {
                    self.close_window(id);
                }
            }
            MenuAction::CloseWindow(id) => self.close_window(id),
            MenuAction::ToggleMaximize(id) => self.toggle_maximize(id),
            MenuAction::Minimize(id) => self.minimize(id),
            MenuAction::AddToDesktop(app) => self.add_to_desktop(app),
            MenuAction::RemoveFromDesktop(index) => self.remove_from_desktop(index),
            MenuAction::OpenDesktopItem(index) => self.open_desktop_item(index),
            MenuAction::ArrangeIcons => self.arrange_icons(),
            MenuAction::ToggleAutohide => {
                self.autohide = !self.autohide;
                ui::write_nook_config("dock_autohide", if self.autohide { "yes" } else { "no" });
                self.update_work_area();
            }
            MenuAction::About => self.open_about(),
            MenuAction::LaunchRole(role, arg) => match arg {
                Some(arg) => self.launch_role(role, &[arg.as_str()]),
                None => self.launch_role(role, &[]),
            },
        }
    }

    pub fn poll_network(&mut self, force: bool) {
        let now = sys::uptime_ms();
        let interval = if matches!(self.popup, Popup::Network) { 1000 } else { 3000 };
        if !force && now.saturating_sub(self.net.last_poll) < interval {
            return;
        }
        self.net.last_poll = now;
        let status = net::status();
        let changed_icon = network_icon(&status) != network_icon(&self.net.status);
        let connected = status.connected();
        if let Some(previous) = self.net.last_connected {
            if previous != connected && status.available {
                let body = match status.active() {
                    Some(i) if i.kind == "wifi" => format!("Wi-Fi {} · {}", i.ssid, i.address),
                    Some(i) => format!("Ethernet · {}", i.address),
                    None => String::from("The network connection was lost"),
                };
                self.toast(if connected { "Connected" } else { "Disconnected" }, &body, "ui/network");
            }
        }
        self.net.last_connected = Some(connected);
        self.net.status = status;
        if self.net.status.mode == "wifi" && matches!(self.popup, Popup::Network) && now.saturating_sub(self.net.last_scan) > 8000 {
            self.net.networks = net::wifi_scan(false);
            self.net.last_scan = now;
        }
        if changed_icon {
            let button = self.network_button();
            self.core.add_damage(button.expand(4));
        }
        if matches!(self.popup, Popup::Network) {
            let area = self.network_area();
            self.core.add_damage(area.expand(120));
        }
    }

    pub fn draw_top_bar(&mut self) {
        let bar = self.top_bar();
        if !bar.overlaps(&self.core.screen.clip) {
            return;
        }
        let ui = self.ui();
        let pointer = self.core.pointer;
        let width = self.core.screen.width;
        let clock = self.clock_text();
        let user = self.user.clone();
        let live = self.live;
        let (apps_btn, power_btn, net_btn, clock_btn, vol_btn) = (self.apps_button(), self.power_button(), self.network_button(), self.clock_button(), self.volume_button());
        let (launcher_open, power_open, net_open, calendar_open, sound_open) = (matches!(self.popup, Popup::Launcher), matches!(self.popup, Popup::Power), matches!(self.popup, Popup::Network), matches!(self.popup, Popup::Calendar), matches!(self.popup, Popup::Sound));
        let net_icon = network_icon(&self.net.status);
        let vol_icon = crate::sound::volume_icon(self.sound.level, self.sound.muted, self.sound.available);
        let ux = self.user_x();
        let mut p = self.core.painter_clipped();
        let pal = theme::palette();
        p.blend_fill(bar, pal.panel, pal.panel_alpha);
        p.blend_fill(Area::new(0, TOP_H - 1, width, 1), if theme::is_light() { 0xc9ced6 } else { 0x000000 }, 255);
        for (button, open) in [(apps_btn, launcher_open), (power_btn, power_open), (net_btn, net_open), (clock_btn, calendar_open), (vol_btn, sound_open)] {
            if open || button.contains(pointer.0, pointer.1) {
                p.rounded(button, 7, pal.overlay, if open { 40 } else { 22 });
            }
        }
        if let Some(logo) = ui.icon("logo-20") {
            p.image_tinted(logo, 14, (TOP_H - logo.h) / 2, pal.panel_text, 255);
        }
        p.text(&ui.medium, 42, (TOP_H - ui.medium.height()) / 2, "Applications", pal.panel_text);
        let cw = ui.medium.measure(&clock);
        p.text(&ui.medium, (width - cw) / 2, (TOP_H - ui.medium.height()) / 2, &clock, pal.panel_text);
        if let Some(img) = ui.icon("ui/power") {
            p.image_tinted(img, power_btn.x + (power_btn.w - img.w) / 2, power_btn.y + (power_btn.h - img.h) / 2, pal.panel_text, 255);
        }
        if let Some(img) = ui.icon(net_icon) {
            let tint = if net_icon == "ui/network-off" || net_icon == "ui/wifi-off" { pal.panel_dim } else { pal.panel_text };
            p.image_tinted(img, net_btn.x + (net_btn.w - img.w) / 2, net_btn.y + (net_btn.h - img.h) / 2, tint, 255);
        }
        if let Some(img) = ui.icon(vol_icon) {
            let tint = if vol_icon == "ui/volume-off" || vol_icon == "ui/volume-mute" { pal.panel_dim } else { pal.panel_text };
            p.image_tinted(img, vol_btn.x + (vol_btn.w - img.w) / 2, vol_btn.y + (vol_btn.h - img.h) / 2, tint, 255);
        }
        p.text(&ui.font, ux, (TOP_H - ui.font.height()) / 2, &user, pal.panel_text);
        if let Some(img) = ui.icon("ui/user") {
            p.image_tinted(img, ux - 22, (TOP_H - img.h) / 2, pal.panel_text, 255);
        }
        if live {
            let tag = "LIVE";
            let tw = ui.small.measure(tag);
            let area = Area::new(vol_btn.x - 12 - tw - 14, 8, tw + 14, TOP_H - 16);
            p.rounded(area, 8, theme::warn(), 230);
            p.text(&ui.small, area.x + 7, area.y + (area.h - ui.small.height()) / 2, tag, 0x1b1405);
        }
        self.hot_buttons.extend_from_slice(&[apps_btn, power_btn, net_btn, clock_btn, vol_btn]);
    }

    pub fn draw_popup(&mut self) {
        let Some(area) = self.popup_area() else {
            return;
        };
        if !area.expand(32).overlaps(&self.core.screen.clip) {
            return;
        }
        let ui = self.ui();
        let hover = self.popup_hover;
        match self.popup.clone() {
            Popup::Launcher => self.draw_launcher(area, hover),
            Popup::DockOverflow => self.draw_overflow(area, hover),
            Popup::Power => {
                let rows: Vec<Area> = (0..3).map(|i| self.power_row(i)).collect();
                let mut p = self.core.painter_clipped();
                popup_frame(&mut p, area, 12);
                for (i, (label, icon)) in [("Log out", "ui/logout"), ("Restart…", "ui/reboot"), ("Shut down…", "ui/power")].iter().enumerate() {
                    let row = rows[i];
                    if hover == Some(i) {
                        p.rounded(row, 7, theme::hover(), 255);
                    }
                    if let Some(img) = ui.icon(icon) {
                        p.image_tinted(img, row.x + 12, row.y + (row.h - img.h) / 2, if i == 2 { theme::danger() } else { theme::dim() }, 255);
                    }
                    p.text(&ui.font, row.x + 40, row.y + (row.h - ui.font.height()) / 2, label, theme::text());
                }
            }
            Popup::Menu { items, .. } => {
                let mut p = self.core.painter_clipped();
                popup_frame(&mut p, area, 10);
                for (i, item) in items.iter().enumerate() {
                    let row = Self::menu_row(area, i);
                    let hot = hover == Some(i);
                    if hot {
                        p.rounded(row, 6, if item.danger { theme::danger() } else { theme::accent() }, 255);
                    }
                    if let Some(img) = ui.icon(item.icon) {
                        p.image_tinted(img, row.x + 10, row.y + (row.h - img.h) / 2, if hot { 0xffffff } else if item.danger { theme::danger() } else { theme::dim() }, 255);
                    }
                    p.text(&ui.font, row.x + 36, row.y + (row.h - ui.font.height()) / 2, &item.label, if hot { 0xffffff } else { theme::text() });
                }
            }
            Popup::Calendar => self.draw_calendar(area, hover),
            Popup::Network => self.draw_network(area, hover),
            Popup::Sound => self.draw_sound(area, hover),
            Popup::None => {}
        }
    }

    fn draw_calendar(&mut self, area: Area, hover: Option<usize>) {
        let ui = self.ui();
        let (year, month) = self.calendar_month;
        let now = sys::realtime().sec;
        let (ty, tm, td, _, _) = civil(now);
        let first = days_from_civil(year, month, 1);
        let next_first = if month == 12 { days_from_civil(year + 1, 1, 1) } else { days_from_civil(year, month + 1, 1) };
        let days = (next_first - first) as i32;
        let lead = ((first + 3).rem_euclid(7)) as i32;
        let mut p = self.core.painter_clipped();
        popup_frame(&mut p, area, 14);
        let title = format!("{} {}", MONTHS[(month - 1) as usize], year);
        ui::centered_text(&mut p, &ui.title, Area::new(area.x, area.y + 12, area.w, 30), &title, theme::text());
        for (i, (icon, button)) in [("ui/chevron-left", Area::new(area.x + 12, area.y + 12, 32, 30)), ("ui/chevron-right", Area::new(area.right() - 44, area.y + 12, 32, 30))].iter().enumerate() {
            ui::icon_button(&mut p, ui, *button, icon, hover == Some(i), false);
        }
        let cell_w = (area.w - 24) / 7;
        for (i, name) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate() {
            ui::centered_text(&mut p, &ui.small, Area::new(area.x + 12 + i as i32 * cell_w, area.y + 52, cell_w, 20), name, if i >= 5 { theme::accent_hover() } else { theme::faint() });
        }
        for day in 1..=days {
            let slot = lead + day - 1;
            let cell = Area::new(area.x + 12 + (slot % 7) * cell_w, area.y + 76 + (slot / 7) * 40, cell_w, 36);
            let today = year == ty && month == tm && day as u32 == td;
            if today {
                p.circle(cell.x as f32 + cell.w as f32 / 2.0, cell.y as f32 + cell.h as f32 / 2.0, 15.0, theme::accent(), 255);
            }
            ui::centered_text(&mut p, &ui.font, cell, &format!("{}", day), if today { 0xffffff } else { theme::text() });
        }
    }

    fn draw_network(&mut self, area: Area, hover: Option<usize>) {
        let ui = self.ui();
        let status = self.net.status.clone();
        let list = self.network_list();
        let segments = self.network_mode_segments();
        let rows: Vec<Area> = (0..list.len()).map(|i| self.network_row(i)).collect();
        let password = self.network_password_field();
        let settings = self.network_settings_button();
        let scan = self.network_scan_button();
        let selected = self.net.selected.clone();
        let message = self.net.message.clone();
        let mut field = core::mem::replace(&mut self.net.password, TextField::secret());
        {
            let mut p = self.core.painter_clipped();
            popup_frame(&mut p, area, 14);
            p.text(&ui.title, area.x + 18, area.y + 14, "Network", theme::text());
            let (line, color) = if !status.available {
                (String::from("Networking is not available in this kernel"), theme::faint())
            } else {
                match status.active().or_else(|| linked(&status)) {
                    Some(i) if i.kind == "wifi" && !i.address.is_empty() => (format!("Connected to {} · {}", i.ssid, i.address), theme::success()),
                    Some(i) if i.kind == "wifi" => (format!("Joined {} · getting an address…", i.ssid), theme::warn()),
                    Some(i) if !i.address.is_empty() => (format!("Connected via Ethernet · {}", i.address), theme::success()),
                    Some(_) => (String::from("Cable connected · getting an address…"), theme::warn()),
                    None => {
                        let pending = status.interfaces.iter().find(|i| i.kind == (if status.mode == "wifi" { "wifi" } else { "ethernet" })).map(|i| i.state.clone()).unwrap_or_default();
                        (if status.mode == "off" { String::from("Networking is turned off") } else if pending.is_empty() { String::from("Not connected") } else { pending }, theme::dim())
                    }
                }
            };
            p.text(&ui.font, area.x + 18, area.y + 46, &ellipsize(&ui.font, if message.is_empty() { &line } else { &message }, area.w - 36), if message.is_empty() { color } else { theme::warn() });
            let track = Area::new(segments[0].x, segments[0].y, segments[2].right() - segments[0].x, 32);
            p.rounded(track, 8, theme::bg(), 255);
            for (i, (seg, label, key, icon)) in [(segments[0], "Ethernet", "ethernet", "ui/ethernet"), (segments[1], "Wi-Fi", "wifi", "ui/wifi-3"), (segments[2], "Off", "off", "ui/network-off")].iter().enumerate() {
                let active = status.mode == *key;
                if active {
                    p.rounded(seg.inset(3), 6, theme::accent(), 255);
                } else if hover == Some(i) {
                    p.rounded(seg.inset(3), 6, theme::hover(), 255);
                }
                let tw = ui.medium.measure(label) + 22;
                let x0 = seg.x + (seg.w - tw) / 2;
                if let Some(img) = ui.icon(icon) {
                    p.image_tinted(img, x0, seg.y + (seg.h - img.h) / 2, if active { 0xffffff } else { theme::dim() }, 255);
                }
                p.text(&ui.medium, x0 + 22, seg.y + (seg.h - ui.medium.height()) / 2, label, if active { 0xffffff } else { theme::text() });
            }
            let top = area.y + 122;
            if !status.available {
                p.text(&ui.font, area.x + 18, top + 8, "Rebuild the kernel with the network stack.", theme::faint());
            } else if status.mode == "ethernet" {
                let iface = status.interfaces.iter().find(|i| i.kind == "ethernet");
                let rows_text: [(&str, String); 5] = match iface {
                    Some(i) => [
                        ("Adapter", format!("{} ({})", i.name, i.driver)),
                        ("Cable", String::from(if i.link { "connected" } else { "unplugged" })),
                        ("Address", if i.address.is_empty() { String::from(i.state.as_str()) } else { i.address.clone() }),
                        ("Gateway", if i.gateway.is_empty() { String::from("—") } else { i.gateway.clone() }),
                        ("DNS", if i.dns.is_empty() { String::from("—") } else { i.dns.clone() }),
                    ],
                    None => [
                        ("Adapter", String::from("no Ethernet adapter found")),
                        ("Cable", String::from("—")),
                        ("Address", String::from("—")),
                        ("Gateway", String::from("—")),
                        ("DNS", String::from("—")),
                    ],
                };
                for (i, (k, v)) in rows_text.iter().enumerate() {
                    let y = top + 4 + i as i32 * 24;
                    p.text(&ui.font, area.x + 18, y, k, theme::faint());
                    p.text(&ui.font, area.x + 110, y, &ellipsize(&ui.font, v, area.w - 128), theme::text());
                }
            } else if status.mode == "wifi" {
                if !status.has_kind("wifi") {
                    p.text(&ui.medium, area.x + 18, top + 6, "No Wi-Fi adapter was found", theme::text());
                    p.text(&ui.font, area.x + 18, top + 30, "Supported: Qualcomm Atheros AR9285 (experimental)", theme::faint());
                } else {
                    if list.is_empty() {
                        p.text(&ui.font, area.x + 18, top + 12, "Scanning for networks…", theme::faint());
                    }
                    for (i, network) in list.iter().enumerate() {
                        let row = rows[i];
                        let is_selected = selected.as_ref() == Some(&network.ssid);
                        if hover == Some(10 + i) || is_selected {
                            p.rounded(row, 8, theme::hover(), 255);
                        }
                        let bars = match network.signal {
                            0..=25 => "ui/wifi-0",
                            26..=50 => "ui/wifi-1",
                            51..=75 => "ui/wifi-2",
                            _ => "ui/wifi-3",
                        };
                        if let Some(img) = ui.icon(bars) {
                            p.image_tinted(img, row.x + 12, row.y + (row.h - img.h) / 2, if network.connected { theme::accent_hover() } else { theme::text() }, 255);
                        }
                        let name = if network.ssid.is_empty() { String::from("(hidden network)") } else { network.ssid.clone() };
                        p.text(&ui.medium, row.x + 40, row.y + 5, &ellipsize(&ui.medium, &name, row.w - 90), theme::text());
                        let detail = if network.connected { String::from("Connected · click to disconnect") } else { format!("{} · channel {}", network.security.to_uppercase(), network.channel) };
                        p.text(&ui.small, row.x + 40, row.y + 23, &detail, if network.connected { theme::success() } else { theme::faint() });
                        let mark = if network.connected { "ui/check" } else if network.secured() { "ui/lock" } else { "" };
                        if let Some(img) = ui.icon(mark) {
                            p.image_tinted(img, row.right() - 28, row.y + (row.h - img.h) / 2, theme::dim(), 255);
                        }
                    }
                    if let Some((input, button)) = password {
                        ui::text_field(&mut p, ui, input, &mut field, true, "Password");
                        ui::button(&mut p, ui, button, "Connect", Style::Primary, hover == Some(30), field.text.len() >= 8);
                    }
                    ui::button(&mut p, ui, scan, "Scan", Style::Secondary, hover == Some(41), true);
                }
            } else {
                p.text(&ui.font, area.x + 18, top + 10, "Choose Ethernet or Wi-Fi to connect.", theme::faint());
            }
            ui::button(&mut p, ui, settings, "Network settings…", Style::Flat, hover == Some(40), true);
        }
        self.net.password = field;
    }

    pub fn draw_toasts(&mut self) {
        if self.toasts.is_empty() || !self.toast_region().overlaps(&self.core.screen.clip) {
            return;
        }
        let ui = self.ui();
        let areas: Vec<Area> = (0..self.toasts.len()).map(|i| self.toast_area(i)).collect();
        let toasts: Vec<(String, String, String)> = self.toasts.iter().map(|t| (t.title.clone(), t.body.clone(), t.icon.clone())).collect();
        let mut p = self.core.painter_clipped();
        for (i, (title, body, icon)) in toasts.iter().enumerate() {
            let a = areas[i];
            p.shadow(a, 12, 18, 200, 5);
            p.rounded(a, 12, theme::palette().osd, 248);
            p.rounded_border(a, 12, theme::palette().edge, theme::palette().edge_alpha + 4);
            let mut text_x = a.x + 16;
            if let Some(img) = ui.icon(icon) {
                if img.w <= 16 {
                    p.rounded(Area::new(a.x + 14, a.y + 16, 40, 40), 10, theme::surface_2(), 255);
                    p.image_tinted(img, a.x + 26, a.y + 28, theme::accent_hover(), 255);
                } else {
                    p.image_scaled(img, Area::new(a.x + 14, a.y + 16, 40, 40), 255);
                }
                text_x = a.x + 66;
            }
            let max = a.right() - text_x - 14;
            p.text(&ui.medium, text_x, a.y + 16, &ellipsize(&ui.medium, title, max), theme::text());
            p.text(&ui.font, text_x, a.y + 38, &ellipsize(&ui.font, body, max), theme::dim());
        }
    }

    pub fn draw_switcher(&mut self) {
        let Some(selected) = self.switcher else {
            return;
        };
        let area = self.switcher_area();
        if !area.expand(24).overlaps(&self.core.screen.clip) {
            return;
        }
        let order = self.switcher_order();
        let entries: Vec<(String, String)> = order
            .iter()
            .filter_map(|id| self.core.index_of(*id))
            .map(|i| (self.core.windows[i].title.clone(), format!("apps/{}", if self.core.windows[i].icon.is_empty() { "hello" } else { self.core.windows[i].icon.as_str() })))
            .collect();
        for (_, icon) in &entries {
            self.ui.load_icon(icon);
        }
        let ui = self.ui();
        let mut p = self.core.painter_clipped();
        popup_frame(&mut p, area, 16);
        for (i, (title, icon)) in entries.iter().enumerate() {
            let cell = Area::new(area.x + 12 + i as i32 * 96, area.y + 14, 90, 88);
            if i == selected {
                p.rounded(cell, 12, theme::accent(), 90);
                p.rounded_border(cell, 12, theme::accent_hover(), 200);
            }
            if let Some(img) = ui.icon(icon) {
                p.image(img, cell.x + (cell.w - img.w) / 2, cell.y + 10, 255);
            }
            if i == selected {
                ui::centered_text(&mut p, &ui.font, Area::new(area.x, area.bottom() - 34, area.w, 24), title, theme::text());
            }
        }
    }

    pub fn draw_about_window(p: &mut Painter, ui: &hxclient::ui::Ui, content: Area) {
        p.fill(content, theme::bg());
        let wordmark = if theme::is_light() { "about-logo-light" } else { "about-logo-dark" };
        if let Some(logo) = ui.icon(wordmark) {
            p.image(logo, content.x + (content.w - logo.w) / 2, content.y + 16, 255);
        }
        ui::centered_text(p, &ui.medium, Area::new(content.x, content.y + 120, content.w, 22), "HamixOS 0.6.1", theme::text());
        ui::centered_text(p, &ui.font, Area::new(content.x, content.y + 142, content.w, 20), "Nook desktop · hsh shell · hext filesystem · smoltcp", theme::dim());
        let info = sys::sysinfo();
        let cpus = sys::cpu_stats().len();
        let lines = [
            ("Memory", format!("{} of {}", ui::human_size(info.mem_used()), ui::human_size(info.mem_total))),
            ("Processors", if cpus == 1 { String::from("1 core") } else { format!("{} cores", cpus) }),
            ("Uptime", format!("{} min", info.uptime_ms / 60000)),
            ("Processes", format!("{}", info.processes)),
        ];
        let mut y = content.y + 178;
        for (k, v) in lines {
            p.text(&ui.font, content.x + 110, y, k, theme::faint());
            p.text(&ui.font, content.x + 210, y, &v, theme::text());
            y += 24;
        }
        ui::centered_text(p, &ui.small, Area::new(content.x, content.bottom() - 34, content.w, 20), "Written from scratch in Rust", theme::faint());
    }

    pub fn draw_auth_window(p: &mut Painter, ui: &hxclient::ui::Ui, content: Area, body: &mut Body, pointer: (i32, i32)) {
        let Body::Auth { action, field, error } = body else {
            return;
        };
        p.fill(content, theme::bg());
        let verb = if *action == PowerAction::Reboot { "restart" } else { "shut down" };
        p.text(&ui.medium, content.x + 20, content.y + 18, &format!("Authentication is required to {} the computer", verb), theme::text());
        p.text(&ui.font, content.x + 20, content.y + 42, &format!("Enter the password of {}.", ui::current_user()), theme::dim());
        let input = Area::new(content.x + 20, content.y + 70, content.w - 40, 38);
        ui::text_field(p, ui, input, field, true, "Password");
        if !error.is_empty() {
            p.text(&ui.font, content.x + 20, content.y + 116, error, theme::danger());
        }
        let ok = Area::new(content.right() - 112, content.bottom() - 52, 96, 36);
        let cancel = Area::new(content.right() - 216, content.bottom() - 52, 96, 36);
        ui::button(p, ui, cancel, "Cancel", Style::Secondary, cancel.contains(pointer.0, pointer.1), true);
        let label = if *action == PowerAction::Reboot { "Restart" } else { "Shut down" };
        ui::button(p, ui, ok, label, Style::Danger, ok.contains(pointer.0, pointer.1), true);
    }
}

pub fn popup_frame(p: &mut Painter, area: Area, radius: i32) {
    p.shadow(area, radius, 26, 230, 8);
    p.rounded(area, radius, theme::palette().osd, 250);
    p.rounded_border(area, radius, theme::palette().edge, theme::palette().edge_alpha + 2);
}

fn signal_icon(signal: u32) -> &'static str {
    match signal {
        0..=25 => "ui/wifi-0",
        26..=50 => "ui/wifi-1",
        51..=75 => "ui/wifi-2",
        _ => "ui/wifi-3",
    }
}

pub fn linked(status: &net::Status) -> Option<&net::Interface> {
    let wanted = if status.mode == "wifi" { "wifi" } else { "ethernet" };
    status.interfaces.iter().find(|i| i.kind == wanted && i.link)
}

pub fn network_icon(status: &net::Status) -> &'static str {
    if !status.available || status.mode == "off" {
        return "ui/network-off";
    }
    match status.active().or_else(|| linked(status)) {
        Some(i) if i.kind == "wifi" => signal_icon(i.signal),
        Some(_) => "ui/ethernet",
        None => {
            if status.mode == "wifi" { "ui/wifi-off" } else { "ui/network-off" }
        }
    }
}
