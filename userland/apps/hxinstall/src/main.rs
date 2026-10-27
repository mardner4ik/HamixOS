#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, eprintln, fs, sys, users};
use hxclient::ui::{self, theme, FieldAction, Hits, Style, TextField, Ui};
use hxclient::{Event, Window, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

const INITIAL_W: i32 = 860;
const INITIAL_H: i32 = 580;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}
const SIDEBAR: i32 = 220;
const SCRIPT: &str = "/usr/share/hamix/install.sh";
const MIN_SECTORS: u64 = 512 * 2048;

const STEPS: [&str; 5] = ["Welcome", "Disk", "Account", "System", "Install"];
const INSTALL_STEPS: [&str; 6] = ["Preparing the disk", "Formatting", "Copying the system", "Creating accounts", "Installing the boot loader", "Writing everything to disk"];

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Back,
    Next,
    Disk(usize),
    Field(usize),
    Admin,
    Autostart,
    Understand,
    Reboot,
    Close,
}

struct Disk {
    name: String,
    model: String,
    sectors: u64,
    bus: String,
    detail: String,
    in_use: bool,
}

#[derive(PartialEq)]
enum Phase {
    Setup,
    Running,
    Failed(String),
    Done,
}

struct App {
    window: Window,
    ui: Ui,
    page: usize,
    disks: Vec<Disk>,
    disk: Option<usize>,
    fields: [TextField; 7],
    focus: Option<usize>,
    admin: bool,
    autostart: bool,
    understand: bool,
    error: String,
    hits: Hits<Hit>,
    hover: Option<Hit>,
    phase: Phase,
    step: usize,
    detail: String,
    log: Vec<String>,
    child: i64,
    output: u64,
    partial: String,
    conf_path: String,
    password: String,
    started: u64,
}

const USERNAME: usize = 0;
const PASSWORD: usize = 1;
const CONFIRM: usize = 2;
const ROOT: usize = 3;
const ROOT_CONFIRM: usize = 4;
const HOSTNAME: usize = 5;
const AUTH: usize = 6;

fn load_disks() -> Vec<Disk> {
    let mut disks: Vec<Disk> = Vec::new();
    let mounted_root = fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in sys::disk_listing().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("disk") if f.len() >= 5 => disks.push(Disk {
                name: String::from(f[1]),
                sectors: f[2].parse().unwrap_or(0),
                bus: String::from(f[3]),
                model: String::from(if f[4].is_empty() { "Disk" } else { f[4] }),
                detail: if f.get(5).map(|s| !s.is_empty()).unwrap_or(false) { format!("{} filesystem", f[5]) } else { String::new() },
                in_use: f.get(8).map(|s| !s.is_empty()).unwrap_or(false),
            }),
            Some("part") if f.len() >= 7 => {
                if let Some(disk) = disks.iter_mut().find(|d| d.name == f[2]) {
                    let part = format!("{} {}{}", f[1], ui::human_size(f[4].parse::<u64>().unwrap_or(0) * 512), if f[6].is_empty() { String::new() } else { format!(" {}", f[6]) });
                    if !disk.detail.is_empty() {
                        disk.detail.push_str(", ");
                    }
                    disk.detail.push_str(&part);
                    if f.get(9).map(|s| !s.is_empty() && mounted_root.contains(&format!("/dev/{}", f[1]))).unwrap_or(false) {
                        disk.in_use = true;
                    }
                }
            }
            _ => {}
        }
    }
    disks
}

impl App {
    fn validate(&mut self) -> bool {
        self.error.clear();
        match self.page {
            1 => {
                match self.disk {
                    None => self.error = String::from("Choose the disk HamixOS will be installed on"),
                    Some(i) if self.disks[i].sectors < MIN_SECTORS => self.error = String::from("That disk is too small (at least 512 MB is needed)"),
                    Some(i) if self.disks[i].in_use => self.error = String::from("That disk is in use by the running system"),
                    _ => {}
                }
            }
            2 => {
                let name = self.fields[USERNAME].text.clone();
                if !users::valid_name(&name) {
                    self.error = String::from("User name: lowercase letters, digits, - and _, starting with a letter");
                } else if name == "root" {
                    self.error = String::from("Pick a user name other than root");
                } else if self.fields[PASSWORD].text.len() < 4 {
                    self.error = String::from("Your password must have at least 4 characters");
                } else if self.fields[PASSWORD].text != self.fields[CONFIRM].text {
                    self.error = String::from("Your passwords do not match");
                } else if self.fields[ROOT].text.len() < 4 {
                    self.error = String::from("The root password must have at least 4 characters");
                } else if self.fields[ROOT].text != self.fields[ROOT_CONFIRM].text {
                    self.error = String::from("The root passwords do not match");
                }
            }
            3 => {
                let host = &self.fields[HOSTNAME].text;
                if host.is_empty() || host.len() > 63 || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    self.error = String::from("Computer name: letters, digits and -");
                }
            }
            4 => {
                if !self.understand {
                    self.error = String::from("Confirm that the disk may be erased");
                } else if sys::geteuid() != 0 && self.fields[AUTH].text.is_empty() {
                    self.error = String::from("Enter your password to authorize the installation");
                }
            }
            _ => {}
        }
        self.error.is_empty()
    }

    fn first_field(&self) -> Option<usize> {
        match self.page {
            2 => Some(USERNAME),
            3 => Some(HOSTNAME),
            4 if sys::geteuid() != 0 => Some(AUTH),
            _ => None,
        }
    }

    fn fields_of_page(&self) -> Vec<usize> {
        match self.page {
            2 => alloc::vec![USERNAME, PASSWORD, CONFIRM, ROOT, ROOT_CONFIRM],
            3 => alloc::vec![HOSTNAME],
            4 if sys::geteuid() != 0 => alloc::vec![AUTH],
            _ => Vec::new(),
        }
    }

    fn next(&mut self) {
        if !self.validate() {
            return;
        }
        if self.page == 4 {
            self.start();
            return;
        }
        if self.page == 0 {
            self.disks = load_disks();
            if self.disk.is_none() && self.disks.len() == 1 {
                self.disk = Some(0);
            }
        }
        self.page += 1;
        self.focus = self.first_field();
    }

    fn start(&mut self) {
        let disk = self.disks[self.disk.unwrap()].name.clone();
        self.conf_path = format!("/tmp/hamix-install-{}.conf", sys::getpid());
        let conf = format!(
            "DISK={}\nUSERNAME={}\nUSER_PASSWORD={}\nROOT_PASSWORD={}\nADMIN={}\nHOSTNAME={}\nAUTOSTART={}\n",
            disk,
            self.fields[USERNAME].text,
            self.fields[PASSWORD].text,
            self.fields[ROOT].text,
            if self.admin { "yes" } else { "no" },
            self.fields[HOSTNAME].text,
            if self.autostart { "yes" } else { "no" }
        );
        if !fs::write(&self.conf_path, conf.as_bytes()) {
            self.error = String::from("Cannot write the installer configuration to /tmp");
            return;
        }
        sys::chmod(&self.conf_path, 0o600);
        let Ok((read, write)) = sys::pipe() else {
            self.error = String::from("Cannot create a pipe");
            return;
        };
        self.password = self.fields[AUTH].text.clone();
        let password = if self.password.is_empty() { None } else { Some(self.password.as_str()) };
        match ui::elevated_spawn(password, "/usr/bin/hsh", &[SCRIPT, self.conf_path.as_str()], Some(write)) {
            Ok(pid) => {
                sys::close(write);
                self.child = pid;
                self.output = read;
                self.phase = Phase::Running;
                self.step = 0;
                self.log.clear();
                self.started = sys::uptime_ms();
                self.page = 5;
                self.window.set_title("Installing HamixOS…");
            }
            Err(e) => {
                sys::close(read);
                sys::close(write);
                fs::write(&self.conf_path, b"");
                sys::unlink(&self.conf_path);
                self.error = String::from(e);
                self.fields[AUTH].set("");
            }
        }
    }

    fn handle_line(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("@step ") {
            let (n, text) = rest.split_once(' ').unwrap_or((rest, ""));
            self.step = n.parse::<usize>().unwrap_or(1).saturating_sub(1);
            self.detail = String::from(text);
        } else if let Some(rest) = line.strip_prefix("@detail ") {
            self.detail = format!("Copying {}", rest);
        } else if let Some(rest) = line.strip_prefix("@error ") {
            self.phase = Phase::Failed(String::from(rest));
        } else if line.starts_with("@done") {
            self.step = INSTALL_STEPS.len();
            self.phase = Phase::Done;
        } else if !line.trim().is_empty() {
            self.log.push(String::from(line));
            if self.log.len() > 200 {
                self.log.remove(0);
            }
        }
    }

    fn pump(&mut self) -> bool {
        if self.phase != Phase::Running {
            return false;
        }
        let mut changed = false;
        let mut buf = [0u8; 4096];
        loop {
            let available = sys::fstat_size(self.output);
            if available <= 0 {
                break;
            }
            let n = sys::read(self.output, &mut buf[..(available as usize).min(4096)]);
            if n <= 0 {
                break;
            }
            self.partial.push_str(&String::from_utf8_lossy(&buf[..n as usize]));
            while let Some(i) = self.partial.find('\n') {
                let line: String = self.partial.drain(..=i).collect();
                let line = String::from(line.trim_end());
                self.handle_line(&line);
            }
            changed = true;
        }
        if !sys::proc_alive(self.child) && sys::fstat_size(self.output) <= 0 {
            let code = sys::waitpid(self.child, true);
            sys::close(self.output);
            if self.phase == Phase::Running {
                self.phase = if code == 0 { Phase::Done } else { Phase::Failed(format!("The installer stopped with code {}", code)) };
            }
            fs::write(&self.conf_path, b"");
            sys::unlink(&self.conf_path);
            match self.phase {
                Phase::Done => {
                    self.page = 6;
                    self.window.set_title("Install HamixOS");
                    hxclient::notify("HamixOS is installed", "Remove the live USB and restart the computer");
                }
                _ => self.window.set_title("Install HamixOS"),
            }
            changed = true;
        }
        changed
    }

    fn draw_field(p: &mut Painter, ui: &Ui, hits: &mut Hits<Hit>, field: &mut TextField, index: usize, focus: Option<usize>, x: i32, y: i32, w: i32, label: &str, placeholder: &str) {
        p.text(&ui.small, x + 2, y, label, theme::dim());
        let area = Area::new(x, y + 18, w, 38);
        ui::text_field(p, ui, area, field, focus == Some(index), placeholder);
        hits.add(area, Hit::Field(index));
    }

    fn draw(&mut self) {
        let ui = &self.ui;
        let buffer = self.window.buffer();
        let mut p = Painter::new(buffer, ww(), hh());
        let hits = &mut self.hits;
        hits.clear();
        let hover = self.hover;
        p.fill(Area::new(0, 0, ww(), hh()), theme::bg());

        let light = theme::is_light();
        let (side_top, side_bottom) = if light { (0xeef1f8, 0xe2e6f0) } else { (0x1a2140, 0x141820) };
        let side_text = if light { 0x171b26 } else { 0xffffff };
        let idle_ring = if light { 0xc4cad8 } else { 0x3a4260 };
        let rail = if light { 0xd2d7e4 } else { 0x2d3448 };
        p.gradient_v(Area::new(0, 0, SIDEBAR, hh()), side_top, side_bottom, 255);
        p.fill(Area::new(SIDEBAR - 1, 0, 1, hh()), theme::border());
        if let Some(logo) = ui.icon("logo") {
            p.image_scaled(logo, Area::new(24, 26, 40, 40), 255);
        }
        p.text(&ui.medium, 76, 26, "HamixOS", side_text);
        p.text(&ui.small, 76, 46, "Installer", theme::dim());
        let current = self.page.min(4);
        let caption = if self.page >= 6 {
            String::from("FINISHED")
        } else if self.page == 5 {
            String::from("INSTALLING")
        } else {
            format!("STEP {} OF {}", current + 1, STEPS.len())
        };
        p.text(&ui.small, 24, 84, &caption, theme::faint());
        for (i, label) in STEPS.iter().enumerate() {
            let y = 116 + i as i32 * 46;
            let done = i < current || self.page >= 6;
            let active = i == current && self.page < 6;
            let (cx, cy) = (38.0, y as f32 + 13.0);
            if i + 1 < STEPS.len() {
                p.fill(Area::new(37, y + 26, 2, 20), if done { theme::accent() } else { rail });
            }
            if done {
                p.circle(cx, cy, 12.0, theme::accent(), 255);
                p.line(cx - 5.0, cy, cx - 1.5, cy + 3.5, 2.0, theme::accent_text(), 255);
                p.line(cx - 1.5, cy + 3.5, cx + 5.0, cy - 3.5, 2.0, theme::accent_text(), 255);
            } else if active {
                p.circle(cx, cy, 13.0, theme::accent(), 60);
                p.circle(cx, cy, 12.0, theme::accent(), 255);
                ui::centered_text(&mut p, &ui.small, Area::new(26, y + 1, 24, 24), &format!("{}", i + 1), theme::accent_text());
            } else {
                p.ring(cx, cy, 12.0, 2.0, idle_ring, 255);
                ui::centered_text(&mut p, &ui.small, Area::new(26, y + 1, 24, 24), &format!("{}", i + 1), theme::faint());
            }
            p.text(if active { &ui.medium } else { &ui.font }, 62, y + 13 - ui.font.height() / 2, label, if active || done { side_text } else { theme::faint() });
        }
        ui::separator(&mut p, 24, hh() - 58, SIDEBAR - 48);
        p.text(&ui.small, 24, hh() - 44, "Nothing is written to the disk", theme::faint());
        p.text(&ui.small, 24, hh() - 28, "until you press Install.", theme::faint());

        let content = Area::new(SIDEBAR + 36, 30, ww() - SIDEBAR - 72, hh() - 110);
        let footer_y = hh() - 64;
        let mut back_label = "Back";
        let mut next_label = "Continue";
        let mut next_style = Style::Primary;
        let mut show_back = self.page > 0 && self.page < 5;
        let mut show_next = self.page < 5;

        match self.page {
            0 => {
                p.text(&ui.big, content.x, content.y + 6, "Install HamixOS", theme::text());
                p.text(&ui.font, content.x, content.y + 44, "This assistant copies the running live system onto a disk of this", theme::dim());
                p.text(&ui.font, content.x, content.y + 66, "computer, so HamixOS starts without the USB stick.", theme::dim());
                let mut y = content.y + 104;
                for (icon, title, text) in [
                    ("ui/disk", "Disk", "the whole disk is erased and repartitioned"),
                    ("ui/user", "Account", "your user, and the root password"),
                    ("ui/session", "Session", "computer name, and whether Nook starts at login"),
                ] {
                    let row = Area::new(content.x, y, content.w, 56);
                    p.rounded(row, 10, theme::surface(), 255);
                    p.rounded(Area::new(row.x + 12, row.y + 12, 32, 32), 8, theme::selection(), 255);
                    ui::symbol(&mut p, ui, icon, row.x + 28, row.y + 28, theme::accent_hover());
                    p.text(&ui.medium, row.x + 58, row.y + 10, title, theme::text());
                    p.text(&ui.small, row.x + 58, row.y + 32, text, theme::dim());
                    y += 64;
                }
                let warn = Area::new(content.x, y + 10, content.w, 64);
                p.rounded(warn, 10, theme::warn(), 40);
                p.rounded_border(warn, 10, theme::warn(), 140);
                p.rounded(Area::new(warn.x, warn.y + 10, 3, warn.h - 20), 2, theme::warn(), 255);
                ui::symbol(&mut p, ui, "ui/warning", warn.x + 26, warn.y + 32, theme::warn());
                p.text(&ui.medium, warn.x + 52, warn.y + 14, "The selected disk will be erased completely", theme::text());
                p.text(&ui.font, warn.x + 52, warn.y + 36, "Back up anything you need from it before continuing.", theme::dim());
                back_label = "";
            }
            1 => {
                p.text(&ui.title, content.x, content.y, "Where should HamixOS go?", theme::text());
                p.text(&ui.font, content.x, content.y + 30, "SATA (AHCI) and IDE disks are supported. The whole disk is used.", theme::dim());
                if self.disks.is_empty() {
                    let card = Area::new(content.x, content.y + 70, content.w, 90);
                    ui::card(&mut p, card);
                    p.text(&ui.medium, card.x + 20, card.y + 24, "No disks were found", theme::text());
                    p.text(&ui.font, card.x + 20, card.y + 48, "Check that the disk is connected and SATA mode is AHCI or IDE in the BIOS.", theme::dim());
                }
                for (i, disk) in self.disks.iter().enumerate() {
                    let card = Area::new(content.x, content.y + 66 + i as i32 * 86, content.w, 76);
                    let selected = self.disk == Some(i);
                    let usable = disk.sectors >= MIN_SECTORS && !disk.in_use;
                    p.rounded(card, 12, if selected { theme::selection() } else if hover == Some(Hit::Disk(i)) && usable { theme::hover() } else { theme::surface() }, 255);
                    p.rounded_border(card, 12, if selected { theme::accent() } else { theme::border() }, 255);
                    p.rounded(Area::new(card.x + 16, card.y + 16, 44, 44), 10, if usable { theme::selection() } else { theme::surface_2() }, 255);
                    ui::symbol(&mut p, ui, "ui/disk", card.x + 30, card.y + 30, if usable { theme::accent_hover() } else { theme::faint() });
                    p.text(&ui.medium, card.x + 76, card.y + 16, &format!("{}  ·  {}", disk.model, ui::human_size(disk.sectors * 512)), if usable { theme::text() } else { theme::faint() });
                    let sub = if disk.in_use {
                        format!("/dev/{} · in use by the running system", disk.name)
                    } else if disk.sectors < MIN_SECTORS {
                        format!("/dev/{} · too small", disk.name)
                    } else {
                        format!("/dev/{} · {} · {}", disk.name, disk.bus.to_uppercase(), if disk.detail.is_empty() { String::from("empty") } else { disk.detail.clone() })
                    };
                    p.text(&ui.font, card.x + 76, card.y + 42, &ellipsize(&ui.font, &sub, card.w - 120), theme::dim());
                    if selected {
                        p.circle(card.right() as f32 - 30.0, card.y as f32 + 38.0, 11.0, theme::accent(), 255);
                        p.line(card.right() as f32 - 35.0, card.y as f32 + 38.0, card.right() as f32 - 31.5, card.y as f32 + 41.5, 2.0, 0xffffff, 255);
                        p.line(card.right() as f32 - 31.5, card.y as f32 + 41.5, card.right() as f32 - 25.0, card.y as f32 + 34.5, 2.0, 0xffffff, 255);
                    }
                    if usable {
                        hits.add(card, Hit::Disk(i));
                    }
                }
            }
            2 => {
                p.text(&ui.title, content.x, content.y, "Who will use this computer?", theme::text());
                let col = (content.w - 24) / 2;
                let y = content.y + 44;
                let focus = self.focus;
                let [username, password, confirm, root, root_confirm, _, _] = &mut self.fields;
                Self::draw_field(&mut p, ui, hits, username, USERNAME, focus, content.x, y, col, "User name", "e.g. matvii");
                Self::draw_field(&mut p, ui, hits, password, PASSWORD, focus, content.x, y + 70, col, "Password", "");
                Self::draw_field(&mut p, ui, hits, confirm, CONFIRM, focus, content.x + col + 24, y + 70, col, "Repeat password", "");
                let cb = ui::checkbox(&mut p, ui, content.x, y + 150, self.admin, "Administrator — may use sudo", hover == Some(Hit::Admin));
                hits.add(cb, Hit::Admin);
                ui::separator(&mut p, content.x, y + 190, content.w);
                p.text(&ui.medium, content.x, y + 206, "Root account", theme::text());
                p.text(&ui.font, content.x, y + 228, "The superuser password, used by su and for system maintenance.", theme::dim());
                Self::draw_field(&mut p, ui, hits, root, ROOT, focus, content.x, y + 254, col, "Root password", "");
                Self::draw_field(&mut p, ui, hits, root_confirm, ROOT_CONFIRM, focus, content.x + col + 24, y + 254, col, "Repeat root password", "");
            }
            3 => {
                p.text(&ui.title, content.x, content.y, "System settings", theme::text());
                let focus = self.focus;
                Self::draw_field(&mut p, ui, hits, &mut self.fields[HOSTNAME], HOSTNAME, focus, content.x, content.y + 44, 300, "Computer name", "hamix");
                let card = Area::new(content.x, content.y + 130, content.w, 92);
                ui::card(&mut p, card);
                p.text(&ui.medium, card.x + 18, card.y + 20, "Start Nook right after login", theme::text());
                p.text(&ui.font, card.x + 18, card.y + 44, "Off: you log in to the hsh text shell and type startx when needed.", theme::dim());
                let toggle = Area::new(card.right() - 70, card.y + 32, 50, 28);
                ui::switch(&mut p, toggle, self.autostart, hover == Some(Hit::Autostart));
                hits.add(toggle, Hit::Autostart);
                p.text(&ui.small, content.x, card.bottom() + 16, "The login shell can be changed later in /etc/hamix/login.conf.", theme::faint());
            }
            4 => {
                p.text(&ui.title, content.x, content.y, "Ready to install", theme::text());
                let disk = self.disk.map(|i| &self.disks[i]);
                let rows = [
                    ("Disk", disk.map(|d| format!("{} ({}, /dev/{})", d.model, ui::human_size(d.sectors * 512), d.name)).unwrap_or_default()),
                    ("Partitions", disk.map(|d| format!("/dev/{}1 — hext, whole disk, bootable (GRUB)", d.name)).unwrap_or_default()),
                    ("User", format!("{}{}", self.fields[USERNAME].text, if self.admin { " (administrator)" } else { "" })),
                    ("Root password", String::from("set")),
                    ("Computer name", self.fields[HOSTNAME].text.clone()),
                    ("After login", String::from(if self.autostart { "start Nook" } else { "text shell (hsh)" })),
                ];
                let card = Area::new(content.x, content.y + 40, content.w, rows.len() as i32 * 34 + 16);
                ui::card(&mut p, card);
                for (i, (k, v)) in rows.iter().enumerate() {
                    let y = card.y + 8 + i as i32 * 34;
                    if i > 0 {
                        ui::separator(&mut p, card.x + 14, y, card.w - 28);
                    }
                    p.text(&ui.font, card.x + 18, y + 9, k, theme::faint());
                    p.text(&ui.font, card.x + 170, y + 9, &ellipsize(&ui.font, v, card.w - 190), if i < 2 { theme::warn() } else { theme::text() });
                }
                let mut y = card.bottom() + 18;
                let label = format!("I understand that everything on /dev/{} will be erased", disk.map(|d| d.name.as_str()).unwrap_or("?"));
                let cb = ui::checkbox(&mut p, ui, content.x, y, self.understand, &label, hover == Some(Hit::Understand));
                hits.add(cb, Hit::Understand);
                y += 34;
                if sys::geteuid() != 0 {
                    let focus = self.focus;
                    let who = ui::current_user();
                    let hint = if who == "user" { format!("Password of {} (the live account's password is \"user\")", who) } else { format!("Password of {}", who) };
                    Self::draw_field(&mut p, ui, hits, &mut self.fields[AUTH], AUTH, focus, content.x, y, 320, &hint, "");
                }
                next_label = "Install";
                next_style = Style::Danger;
            }
            5 => {
                let failed = matches!(self.phase, Phase::Failed(_));
                p.text(&ui.title, content.x, content.y, if failed { "Installation failed" } else { "Installing HamixOS" }, theme::text());
                let elapsed = sys::uptime_ms().saturating_sub(self.started) / 1000;
                p.text(&ui.font, content.x, content.y + 30, &format!("{} · {}:{:02}", if self.detail.is_empty() { "Starting…" } else { self.detail.as_str() }, elapsed / 60, elapsed % 60), theme::dim());
                let per_mille = (self.step as u32 * 1000 / INSTALL_STEPS.len() as u32).max(30);
                let bar = Area::new(content.x, content.y + 62, content.w - 52, 8);
                ui::progress(&mut p, bar, per_mille, if failed { theme::danger() } else { theme::accent() });
                ui::text_in(&mut p, &ui.small, bar.right() + 12, Area::new(bar.right() + 12, bar.y - 8, 40, 24), &format!("{}%", per_mille / 10), theme::dim());
                for (i, label) in INSTALL_STEPS.iter().enumerate() {
                    let y = content.y + 90 + i as i32 * 30;
                    let (cx, cy) = (content.x as f32 + 9.0, y as f32 + 10.0);
                    if i < self.step {
                        p.circle(cx, cy, 8.0, theme::success(), 255);
                        p.line(cx - 3.5, cy, cx - 1.0, cy + 2.5, 1.8, 0xffffff, 255);
                        p.line(cx - 1.0, cy + 2.5, cx + 3.5, cy - 2.5, 1.8, 0xffffff, 255);
                    } else if i == self.step && !failed {
                        let t = (sys::uptime_ms() / 120 % 8) as f32;
                        p.ring(cx, cy, 8.0, 2.0, 0x3a4260, 255);
                        let angle = t * 0.785;
                        p.circle(cx + 6.0 * cos(angle), cy + 6.0 * sin(angle), 2.2, theme::accent_hover(), 255);
                    } else if i == self.step && failed {
                        p.circle(cx, cy, 8.0, theme::danger(), 255);
                        p.line(cx - 3.0, cy - 3.0, cx + 3.0, cy + 3.0, 1.8, 0xffffff, 255);
                        p.line(cx + 3.0, cy - 3.0, cx - 3.0, cy + 3.0, 1.8, 0xffffff, 255);
                    } else {
                        p.ring(cx, cy, 8.0, 1.5, 0x3a4260, 255);
                    }
                    p.text(&ui.font, content.x + 28, y + 10 - ui.font.height() / 2, label, if i <= self.step { theme::text() } else { theme::faint() });
                }
                let log = Area::new(content.x, content.y + 280, content.w, content.h - 280);
                p.rounded(log, 8, if theme::is_light() { 0xf0f2f6 } else { 0x111318 }, 255);
                p.rounded_border(log, 8, theme::border(), 160);
                let lines = ((log.h - 16) / (ui.mono.height() + 2)) as usize;
                let start = self.log.len().saturating_sub(lines);
                for (i, line) in self.log[start..].iter().enumerate() {
                    let text = ellipsize(&ui.small, line, log.w - 24);
                    p.text(&ui.small, log.x + 12, log.y + 8 + i as i32 * (ui.mono.height() + 2), &text, theme::dim());
                }
                if let Phase::Failed(message) = &self.phase {
                    let card = Area::new(content.x, footer_y - 4, content.w - 130, 44);
                    p.text(&ui.medium, card.x, card.y + 12, &ellipsize(&ui.medium, message, card.w), theme::danger());
                    show_back = true;
                    back_label = "Start over";
                }
                show_next = false;
            }
            _ => {
                let (bx, by) = (content.x as f32 + 28.0, content.y as f32 + 48.0);
                p.circle(bx, by, 32.0, theme::success(), 40);
                p.circle(bx, by, 26.0, theme::success(), 255);
                p.line(bx - 11.0, by + 1.0, bx - 3.5, by + 8.5, 3.5, 0xffffff, 255);
                p.line(bx - 3.5, by + 8.5, bx + 11.0, by - 8.0, 3.5, 0xffffff, 255);
                p.text(&ui.big, content.x, content.y + 100, "All done", theme::text());
                let disk = self.disk.map(|i| self.disks[i].name.clone()).unwrap_or_default();
                let lines = [
                    format!("HamixOS was installed on /dev/{}.", disk),
                    String::from("Remove the USB stick and restart. If the old system still"),
                    String::from("starts, choose the disk in the BIOS boot menu (usually F12)."),
                    String::new(),
                    format!("Log in as {} with the password you chose.", self.fields[USERNAME].text),
                ];
                let mut y = content.y + 150;
                for line in lines {
                    p.text(&ui.font, content.x, y, &line, theme::dim());
                    y += 22;
                }
                let reboot = Area::new(ww() - 36 - 150, footer_y, 150, 38);
                let close = Area::new(reboot.x - 112, footer_y, 100, 38);
                ui::button(&mut p, ui, reboot, "Restart now", Style::Primary, hover == Some(Hit::Reboot), true);
                ui::button(&mut p, ui, close, "Close", Style::Secondary, hover == Some(Hit::Close), true);
                hits.add(reboot, Hit::Reboot);
                hits.add(close, Hit::Close);
                show_back = false;
                show_next = false;
            }
        }

        ui::separator(&mut p, SIDEBAR + 36, footer_y - 18, ww() - SIDEBAR - 72);
        if !self.error.is_empty() && self.page < 5 {
            let area = Area::new(content.x, footer_y + 8, ww() - content.x - 290, 24);
            ui::symbol(&mut p, ui, "ui/warning", area.x, area.y + 4, theme::danger());
            ui::text_in(&mut p, &ui.font, area.x + 24, area, &self.error, theme::danger());
        }
        if show_next {
            let next = Area::new(ww() - 36 - 130, footer_y, 130, 38);
            ui::button(&mut p, ui, next, next_label, next_style, hover == Some(Hit::Next), true);
            hits.add(next, Hit::Next);
        }
        if show_back && !back_label.is_empty() {
            let back = Area::new(ww() - 36 - 130 - (if show_next { 112 } else { 0 }) - if self.page == 5 { 30 } else { 0 }, footer_y, if self.page == 5 { 130 } else { 100 }, 38);
            ui::button(&mut p, ui, back, back_label, Style::Secondary, hover == Some(Hit::Back), true);
            hits.add(back, Hit::Back);
        }
        self.window.present();
    }
}

fn sin(x: f32) -> f32 {
    let x = x % 6.283_185;
    let mut term = x;
    let mut sum = x;
    for n in 1..8 {
        term *= -x * x / ((2 * n) as f32 * (2 * n + 1) as f32);
        sum += term;
    }
    sum
}

fn cos(x: f32) -> f32 {
    sin(x + 1.570_796)
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&["logo", "ui/disk", "ui/user", "ui/session", "ui/warning"]);
    let window = match Window::open("Install HamixOS", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxinstall: {}", e);
            return 1;
        }
    };
    let mut app = App {
        window,
        ui,
        page: 0,
        disks: Vec::new(),
        disk: None,
        fields: [TextField::default(), TextField::secret(), TextField::secret(), TextField::secret(), TextField::secret(), TextField::new("hamix"), TextField::secret()],
        focus: None,
        admin: true,
        autostart: false,
        understand: false,
        error: String::new(),
        hits: Hits::new(),
        hover: None,
        phase: Phase::Setup,
        step: 0,
        detail: String::new(),
        log: Vec::new(),
        child: 0,
        output: 0,
        partial: String::new(),
        conf_path: String::new(),
        password: String::new(),
        started: 0,
    };
    app.window.set_icon("install");
    app.window.set_min_size(760, 540);
    app.draw();
    loop {
        let timeout = if app.phase == Phase::Running { 120 } else { -1 };
        let event = app.window.wait_event(timeout);
        let mut redraw = app.pump() || app.phase == Phase::Running;
        match event {
            Some(Event::Resize { .. }) | Some(Event::Theme { .. }) => redraw = true,
            Some(Event::Close { .. }) => {
                if app.phase == Phase::Running {
                    hxclient::notify("Installer closed", "The installation keeps running in the background");
                }
                return 0;
            }
            Some(Event::Mouse { x, y, kind, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    let hover = if kind == MOUSE_LEAVE { None } else { app.hits.at(x, y) };
                    if hover != app.hover {
                        app.hover = hover;
                        redraw = true;
                    }
                }
                MOUSE_PRESS => {
                    redraw = true;
                    app.focus = None;
                    match app.hits.at(x, y) {
                        Some(Hit::Next) => app.next(),
                        Some(Hit::Back) => {
                            app.error.clear();
                            if app.page == 5 {
                                app.phase = Phase::Setup;
                                app.page = 1;
                                app.disks = load_disks();
                                app.understand = false;
                            } else {
                                app.page = app.page.saturating_sub(1);
                            }
                            app.focus = app.first_field();
                        }
                        Some(Hit::Disk(i)) => {
                            app.disk = Some(i);
                            app.error.clear();
                        }
                        Some(Hit::Field(i)) => {
                            app.focus = Some(i);
                            if let Some(area) = app.hits.area_of(Hit::Field(i)) {
                                let font = &app.ui.font;
                                app.fields[i].click(font, x - area.x - 10);
                            }
                        }
                        Some(Hit::Admin) => app.admin = !app.admin,
                        Some(Hit::Autostart) => app.autostart = !app.autostart,
                        Some(Hit::Understand) => app.understand = !app.understand,
                        Some(Hit::Reboot) => {
                            if sys::geteuid() == 0 {
                                sys::power(sys::POWER_REBOOT);
                            } else {
                                let password = app.password.clone();
                                let _ = ui::elevated_spawn(Some(&password), "/usr/bin/hsh", &["-c", "reboot"], None);
                            }
                        }
                        Some(Hit::Close) => return 0,
                        None => {}
                    }
                }
                _ => {}
            },
            Some(Event::Key { code, .. }) => {
                redraw = true;
                match app.focus {
                    Some(i) => match app.fields[i].key(code) {
                        FieldAction::Submit => {
                            let order = app.fields_of_page();
                            match order.iter().position(|f| *f == i) {
                                Some(pos) if pos + 1 < order.len() => app.focus = Some(order[pos + 1]),
                                _ => app.next(),
                            }
                        }
                        FieldAction::Next => {
                            let order = app.fields_of_page();
                            if let Some(pos) = order.iter().position(|f| *f == i) {
                                app.focus = Some(order[(pos + 1) % order.len()]);
                            }
                        }
                        _ => {
                            if i == USERNAME {
                                let lower = app.fields[i].text.to_lowercase();
                                if lower != app.fields[i].text {
                                    let cursor = app.fields[i].cursor;
                                    app.fields[i].text = lower;
                                    app.fields[i].cursor = cursor;
                                }
                            }
                        }
                    },
                    None => match code {
                        10 => app.next(),
                        -1 | -2 if app.page == 1 && !app.disks.is_empty() => {
                            let n = app.disks.len();
                            let cur = app.disk.unwrap_or(0);
                            app.disk = Some(if code == -1 { (cur + n - 1) % n } else { (cur + 1) % n });
                        }
                        _ => {}
                    },
                }
            }
            _ => {}
        }
        if redraw {
            app.draw();
        }
    }
}

entry!(main);
