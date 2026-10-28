#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{
    button, checkbox, column, divider, icon, input, list, progress, row, switch, text, viewer, Align, App, Context, Element, Length, Settings, TextSize, Tone,
};

const SCRIPT: &str = "/usr/share/hamix/install.sh";
const MIN_SECTORS: u64 = 512 * 2048;
const STEPS: [&str; 5] = ["Welcome", "Disk", "Account", "System", "Install"];
const INSTALL_STEPS: [&str; 6] = ["Preparing the disk", "Formatting", "Copying the system", "Creating accounts", "Installing the boot loader", "Writing everything to disk"];

const USERNAME: usize = 0;
const PASSWORD: usize = 1;
const CONFIRM: usize = 2;
const ROOT: usize = 3;
const ROOT_CONFIRM: usize = 4;
const HOSTNAME: usize = 5;
const AUTH: usize = 6;

#[derive(Clone)]
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

struct Installer {
    page: usize,
    disks: Vec<Disk>,
    disk: Option<usize>,
    fields: [String; 7],
    admin: bool,
    autostart: bool,
    linux: bool,
    linux_available: bool,
    understand: bool,
    error: String,
    phase: Phase,
    step: usize,
    detail: String,
    log: Vec<String>,
    child: Option<hxport::Child>,
    partial: String,
    conf_path: String,
    password: String,
    started: u64,
    now: u64,
}

#[derive(Clone)]
enum Msg {
    Next,
    Back,
    PickDisk(usize),
    Field(usize, String),
    Admin(bool),
    Autostart(bool),
    Linux(bool),
    Understand(bool),
    Poll,
    Reboot,
    Close,
}

fn valid_user(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.chars().next().map(|c| c.is_ascii_lowercase() || c == '_').unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn attr(dir: &str, name: &str) -> String {
    hxport::read_to_string(&format!("{}/{}", dir, name)).map(|s| String::from(s.trim())).unwrap_or_default()
}

fn uevent(dir: &str, key: &str) -> String {
    hxport::read_to_string(&format!("{}/uevent", dir)).unwrap_or_default().lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('='))).map(String::from).unwrap_or_default()
}

fn load_disks() -> Vec<Disk> {
    let mounts = hxport::read_to_string("/proc/mounts").unwrap_or_default();
    let mut names: Vec<String> = hxport::entries("/sys/block").unwrap_or_default().into_iter().map(|e| e.name).collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let dir = format!("/sys/block/{}", name);
            let mut detail = String::new();
            let fs = uevent(&dir, "ID_FS_TYPE");
            if !fs.is_empty() {
                detail = format!("{} filesystem", fs);
            }
            let mut parts: Vec<String> = hxport::entries(&dir).unwrap_or_default().into_iter().filter(|e| e.is_dir && e.name.starts_with(name.as_str())).map(|e| e.name).collect();
            parts.sort();
            let mut in_use = mounts.contains(&format!("/dev/{} ", name)) || mounts.contains(&format!("/dev/{}\t", name));
            for part in parts {
                let pdir = format!("{}/{}", dir, part);
                let size = attr(&pdir, "size").parse::<u64>().unwrap_or(0) * 512;
                let pfs = uevent(&pdir, "ID_FS_TYPE");
                if !detail.is_empty() {
                    detail.push_str(", ");
                }
                detail.push_str(&format!("{} {}{}", part, hxport::human_size(size), if pfs.is_empty() { String::new() } else { format!(" {}", pfs) }));
                if mounts.contains(&format!("/dev/{}", part)) {
                    in_use = true;
                }
            }
            let model = attr(&dir, "device/model");
            Disk { model: if model.is_empty() { String::from("Disk") } else { model }, sectors: attr(&dir, "size").parse().unwrap_or(0), bus: uevent(&dir, "ID_BUS"), detail, in_use, name }
        })
        .collect()
}

fn marker(done: bool, active: bool, number: usize) -> Element<Msg> {
    if done {
        row![icon("ui/check").tone(Tone::Accent)].width(26).align(Align::Center).into()
    } else if active {
        text(format!("{}", number)).strong().tone(Tone::Accent).width(26).align(Align::Center).into()
    } else {
        text(format!("{}", number)).tone(Tone::Faint).width(26).align(Align::Center).into()
    }
}

impl Installer {
    fn new() -> Installer {
        Installer {
            page: 0,
            disks: Vec::new(),
            disk: None,
            fields: [String::new(), String::new(), String::new(), String::new(), String::new(), String::from("hamix"), String::new()],
            admin: true,
            autostart: true,
            linux: true,
            linux_available: hxport::exists("/linux/bin/busybox"),
            understand: false,
            error: String::new(),
            phase: Phase::Setup,
            step: 0,
            detail: String::new(),
            log: Vec::new(),
            child: None,
            partial: String::new(),
            conf_path: String::new(),
            password: String::new(),
            started: 0,
            now: 0,
        }
    }

    fn need_auth(&self) -> bool {
        hxport::uid() != 0
    }

    fn validate(&mut self) -> bool {
        self.error.clear();
        let f = &self.fields;
        match self.page {
            1 => match self.disk {
                None => self.error = String::from("Choose the disk HamixOS will be installed on"),
                Some(i) if self.disks[i].sectors < MIN_SECTORS => self.error = String::from("That disk is too small (at least 512 MB is needed)"),
                Some(i) if self.disks[i].in_use => self.error = String::from("That disk is in use by the running system"),
                _ => {}
            },
            2 => {
                if !valid_user(&f[USERNAME]) {
                    self.error = String::from("User name: lowercase letters, digits, - and _, starting with a letter");
                } else if f[USERNAME] == "root" {
                    self.error = String::from("Pick a user name other than root");
                } else if f[PASSWORD].len() < 4 {
                    self.error = String::from("Your password must have at least 4 characters");
                } else if f[PASSWORD] != f[CONFIRM] {
                    self.error = String::from("Your passwords do not match");
                } else if f[ROOT].len() < 4 {
                    self.error = String::from("The root password must have at least 4 characters");
                } else if f[ROOT] != f[ROOT_CONFIRM] {
                    self.error = String::from("The root passwords do not match");
                }
            }
            3 => {
                let host = &f[HOSTNAME];
                if host.is_empty() || host.len() > 63 || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    self.error = String::from("Computer name: letters, digits and -");
                }
            }
            4 => {
                if !self.understand {
                    self.error = String::from("Confirm that the disk may be erased");
                } else if self.need_auth() && f[AUTH].is_empty() {
                    self.error = String::from("Enter your password to authorize the installation");
                }
            }
            _ => {}
        }
        self.error.is_empty()
    }

    fn start(&mut self) {
        let disk = self.disks[self.disk.unwrap()].name.clone();
        self.conf_path = format!("/tmp/hamix-install-{}.conf", hxport::now_ms());
        let f = &self.fields;
        let conf = format!(
            "DISK={}\nUSERNAME={}\nUSER_PASSWORD={}\nROOT_PASSWORD={}\nADMIN={}\nHOSTNAME={}\nAUTOSTART={}\nLINUX={}\n",
            disk,
            f[USERNAME],
            f[PASSWORD],
            f[ROOT],
            if self.admin { "yes" } else { "no" },
            f[HOSTNAME],
            if self.autostart { "yes" } else { "no" },
            if self.linux && self.linux_available { "yes" } else { "no" }
        );
        if !hxport::write(&self.conf_path, conf.as_bytes()) {
            self.error = String::from("Cannot write the installer configuration to /tmp");
            return;
        }
        hxport::chmod(&self.conf_path, 0o600);
        self.password = self.fields[AUTH].clone();
        match hxport::spawn_elevated(&self.password, "/usr/bin/bash", &[SCRIPT, self.conf_path.as_str()]) {
            Ok(child) => {
                self.child = Some(child);
                self.phase = Phase::Running;
                self.step = 0;
                self.log.clear();
                self.started = hxport::now_ms();
                self.now = self.started;
                self.page = 5;
            }
            Err(e) => {
                self.forget_config();
                self.error = e;
                self.fields[AUTH].clear();
            }
        }
    }

    fn forget_config(&mut self) {
        hxport::write(&self.conf_path, b"");
        hxport::remove(&self.conf_path);
    }

    fn handle_line(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("@step ") {
            let (n, label) = rest.split_once(' ').unwrap_or((rest, ""));
            self.step = n.parse::<usize>().unwrap_or(1).saturating_sub(1);
            self.detail = String::from(label);
        } else if let Some(rest) = line.strip_prefix("@detail ") {
            self.detail = format!("Copying {}", rest);
        } else if let Some(rest) = line.strip_prefix("@error ") {
            self.phase = Phase::Failed(String::from(rest));
        } else if line.starts_with("@done") {
            self.step = INSTALL_STEPS.len();
            self.phase = Phase::Done;
        } else if !line.trim().is_empty() {
            self.log.push(String::from(line));
            if self.log.len() > 300 {
                self.log.remove(0);
            }
        }
    }

    fn poll(&mut self, cx: &mut Context<Msg>) {
        self.now = hxport::now_ms();
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let bytes = child.read();
        let status = child.try_wait();
        let tail = if status.is_some() { child.read() } else { Vec::new() };
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        self.partial.push_str(&String::from_utf8_lossy(&tail));
        while let Some(i) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=i).collect();
            self.handle_line(line.trim_end());
        }
        let Some(code) = status else {
            return;
        };
        self.child = None;
        if self.phase == Phase::Running {
            let auth_failed = self.log.iter().any(|l| l.contains("incorrect password") || l.contains("Sorry, try again") || l.contains("authentication failure"));
            self.phase = if code == 0 {
                Phase::Done
            } else if auth_failed {
                Phase::Failed(String::from("Wrong password — the installation was not authorized"))
            } else {
                Phase::Failed(format!("The installer stopped with code {}", code))
            };
        }
        self.forget_config();
        if self.phase == Phase::Done {
            self.page = 6;
            cx.notify("HamixOS is installed", "Remove the live USB and restart the computer");
        }
    }

    fn field(&self, index: usize, label: &str, hint: &str, secret: bool) -> Element<Msg> {
        let mut field = input(self.fields[index].clone(), hint, move |v| Msg::Field(index, v)).on_submit(Msg::Next);
        if matches!(index, USERNAME | HOSTNAME | AUTH) {
            field = field.focused();
        }
        if secret {
            field = field.secret();
        }
        column![text(label).small().tone(Tone::Dim), field].spacing(4).into()
    }

    fn sidebar(&self) -> Element<Msg> {
        let current = self.page.min(4);
        let caption = if self.page >= 6 {
            String::from("FINISHED")
        } else if self.page == 5 {
            String::from("INSTALLING")
        } else {
            format!("STEP {} OF {}", current + 1, STEPS.len())
        };
        let mut steps = column![].spacing(14);
        for (i, label) in STEPS.iter().enumerate() {
            let done = i < current || self.page >= 6;
            let active = i == current && self.page < 6;
            let t = text(*label);
            steps = steps.push(row![marker(done, active, i + 1), if active { t.strong() } else if done { t } else { t.tone(Tone::Faint) }].spacing(10));
        }
        column![
            row![icon("logo").size(40u32, 40u32), column![text("HamixOS").strong(), text("Installer").small().tone(Tone::Dim)].spacing(2)].spacing(12),
            text(caption).small().tone(Tone::Faint),
            steps,
            hxtoolkit::space(),
            divider(),
            text("Nothing is written to the disk until you press Install.").small().tone(Tone::Faint).wrap(),
        ]
        .spacing(22)
        .sidebar()
        .width(220)
        .height(Length::Fill)
        .into()
    }

    fn page_view(&self) -> Element<Msg> {
        match self.page {
            0 => {
                let mut points = column![].spacing(18);
                for (ic, heading, about) in [
                    ("ui/disk", "Disk", "the whole disk is erased and repartitioned"),
                    ("ui/user", "Account", "your user, and the root password"),
                    ("ui/session", "Session", "computer name, and whether Nook starts at login"),
                ] {
                    points = points.push(row![icon(ic), column![text(heading).strong(), text(about).small().dim()].spacing(2)].spacing(14));
                }
                column![
                    text("Install HamixOS").size(TextSize::Display),
                    text("This assistant copies the running live system onto a disk of this computer, so HamixOS starts without the USB stick.").dim().wrap().fill(),
                    points,
                    row![icon("ui/warning"), column![text("The selected disk will be erased completely").strong(), text("Back up anything you need from it before continuing.").dim()].spacing(2)].spacing(14).card(),
                ]
                .spacing(20)
                .into()
            }
            1 => {
                let body: Element<Msg> = if self.disks.is_empty() {
                    column![text("No disks were found").strong(), text("Check that the disk is connected and SATA mode is AHCI or IDE in the BIOS.").dim().wrap().fill()].spacing(6).card().into()
                } else {
                    list(
                        self.disks
                            .iter()
                            .map(|d| {
                                let usable = d.sectors >= MIN_SECTORS && !d.in_use;
                                let sub = if d.in_use {
                                    format!("/dev/{} · in use by the running system", d.name)
                                } else if d.sectors < MIN_SECTORS {
                                    format!("/dev/{} · too small", d.name)
                                } else {
                                    format!("/dev/{} · {} · {}", d.name, d.bus.to_uppercase(), if d.detail.is_empty() { String::from("empty") } else { d.detail.clone() })
                                };
                                let heading = text(format!("{}  ·  {}", d.model, hxport::human_size(d.sectors * 512))).strong();
                                row![icon("ui/disk"), column![if usable { heading } else { heading.tone(Tone::Faint) }, text(sub).dim().fill()].spacing(4)].spacing(14).into()
                            })
                            .collect(),
                    )
                    .selected(self.disk)
                    .on_select(Msg::PickDisk)
                    .row_height(64)
                    .into()
                };
                column![text("Where should HamixOS go?").size(TextSize::Title), text("SATA (AHCI) and IDE disks are supported. The whole disk is used.").dim(), body].spacing(12).fill().into()
            }
            2 => column![
                text("Who will use this computer?").size(TextSize::Title),
                row![self.field(USERNAME, "User name", "e.g. matvii", false), space_fill()].spacing(24),
                row![self.field(PASSWORD, "Password", "", true), self.field(CONFIRM, "Repeat password", "", true)].spacing(24),
                checkbox("Administrator — may use sudo", self.admin, Msg::Admin),
                divider(),
                text("Root account").strong(),
                text("The superuser password, used by su and for system maintenance.").dim(),
                row![self.field(ROOT, "Root password", "", true), self.field(ROOT_CONFIRM, "Repeat root password", "", true)].spacing(24),
            ]
            .spacing(12)
            .into(),
            3 => {
                let about = if self.linux_available { "Run Linux programs and install Alpine packages with pantry. Off: only HamixOS programs run." } else { "This image has no Linux layer; HamixOS programs only." };
                let mut linux = row![column![text("Linux compatibility").strong(), text(about).dim().wrap().fill()].spacing(4).fill()].spacing(12).card();
                if self.linux_available {
                    linux = linux.push(switch(self.linux, Msg::Linux));
                }
                column![
                    text("System settings").size(TextSize::Title),
                    row![self.field(HOSTNAME, "Computer name", "hamix", false).map_width(300)],
                    row![column![text("Start Nook right after login").strong(), text("Off: you log in to the bash text shell and type startx when needed.").dim().wrap().fill()].spacing(4).fill(), switch(self.autostart, Msg::Autostart)]
                        .spacing(12)
                        .card(),
                    linux,
                    text("Both can be changed later: /etc/hamix/login.conf and Settings -> System.").small().tone(Tone::Faint),
                ]
                .spacing(14)
                .into()
            }
            4 => {
                let disk = self.disk.map(|i| &self.disks[i]);
                let rows = [
                    ("Disk", disk.map(|d| format!("{} ({}, /dev/{})", d.model, hxport::human_size(d.sectors * 512), d.name)).unwrap_or_default(), true),
                    ("Partitions", disk.map(|d| format!("/dev/{}1 — hext, whole disk, bootable (GRUB)", d.name)).unwrap_or_default(), true),
                    ("User", format!("{}{}", self.fields[USERNAME], if self.admin { " (administrator)" } else { "" }), false),
                    ("Root password", String::from("set"), false),
                    ("Computer name", self.fields[HOSTNAME].clone(), false),
                    ("After login", String::from(if self.autostart { "start Nook" } else { "text shell (bash)" }), false),
                    ("Linux programs", String::from(if self.linux && self.linux_available { "allowed (compatibility layer on)" } else { "off (HamixOS programs only)" }), false),
                ];
                let mut summary = column![].spacing(10).card();
                for (k, v, warn) in rows {
                    summary = summary.push(row![text(k).tone(Tone::Faint).width(150), text(v).tone(if warn { Tone::Warning } else { Tone::Normal }).fill()]);
                }
                let label = format!("I understand that everything on /dev/{} will be erased", disk.map(|d| d.name.as_str()).unwrap_or("?"));
                let mut page = column![text("Ready to install").size(TextSize::Title), summary, checkbox(label, self.understand, Msg::Understand)].spacing(14);
                if self.need_auth() {
                    page = page.push(row![self.field(AUTH, "Your password (the live account's password is \"user\")", "", true).map_width(360)]);
                }
                page.into()
            }
            5 => {
                let failed = matches!(self.phase, Phase::Failed(_));
                let elapsed = self.now.saturating_sub(self.started) / 1000;
                let fraction = (self.step as f32 / INSTALL_STEPS.len() as f32).max(0.03);
                let mut steps = column![].spacing(8);
                for (i, label) in INSTALL_STEPS.iter().enumerate() {
                    let mark: Element<Msg> = if i < self.step {
                        icon("ui/check").tone(Tone::Success).into()
                    } else if i == self.step && failed {
                        icon("ui/close").tone(Tone::Danger).into()
                    } else if i == self.step {
                        text(["•  ", " • ", "  •", " • "][(self.now / 250 % 4) as usize]).strong().tone(Tone::Accent).into()
                    } else {
                        text("–").tone(Tone::Faint).into()
                    };
                    steps = steps.push(row![row![mark].width(22).align(Align::Center), text(*label).tone(if i <= self.step { Tone::Normal } else { Tone::Faint })].spacing(8));
                }
                let mut page = column![
                    text(if failed { "Installation failed" } else { "Installing HamixOS" }).size(TextSize::Title),
                    text(format!("{} · {}:{:02}", if self.detail.is_empty() { "Starting…" } else { self.detail.as_str() }, elapsed / 60, elapsed % 60)).dim(),
                    row![progress(fraction), text(format!("{}%", (fraction * 100.0) as u32)).small().dim()].spacing(12),
                    steps,
                    viewer::<Msg>(self.log.join("\n")).monospace(),
                ]
                .spacing(12)
                .fill();
                if let Phase::Failed(message) = &self.phase {
                    page = page.push(text(message.clone()).strong().tone(Tone::Danger));
                }
                page.into()
            }
            _ => {
                let disk = self.disk.map(|i| self.disks[i].name.clone()).unwrap_or_default();
                column![
                    icon("ui/check").tone(Tone::Success).size(32u32, 32u32),
                    text("All done").size(TextSize::Display),
                    text(format!(
                        "HamixOS was installed on /dev/{}.\nRemove the USB stick and restart. If the old system still starts, choose the disk in the BIOS boot menu (usually F12).\n\nLog in as {} with the password you chose.",
                        disk, self.fields[USERNAME]
                    ))
                    .dim()
                    .wrap()
                    .fill(),
                ]
                .spacing(14)
                .into()
            }
        }
    }

    fn footer(&self) -> Element<Msg> {
        let mut footer = row![].spacing(12);
        if !self.error.is_empty() && self.page < 5 {
            footer = footer.push(text(self.error.clone()).tone(Tone::Danger).fill());
        } else {
            footer = footer.push(space_fill());
        }
        match self.page {
            0..=4 => {
                if self.page > 0 {
                    footer = footer.push(button("Back").on_press(Msg::Back).width(100));
                }
                let next = button(if self.page == 4 { "Install" } else { "Continue" }).on_press(Msg::Next).width(130);
                footer = footer.push(if self.page == 4 { next.danger() } else { next.primary() });
            }
            5 => {
                if matches!(self.phase, Phase::Failed(_)) {
                    footer = footer.push(button("Start over").on_press(Msg::Back).width(130));
                }
            }
            _ => {
                footer = footer.push(button("Close").on_press(Msg::Close).width(100));
                footer = footer.push(button("Restart now").primary().on_press(Msg::Reboot).width(150));
            }
        }
        footer.into()
    }
}

fn space_fill() -> Element<Msg> {
    hxtoolkit::space().into()
}

trait FixedWidth {
    fn map_width(self, px: u32) -> Element<Msg>;
}

impl FixedWidth for Element<Msg> {
    fn map_width(self, px: u32) -> Element<Msg> {
        match self {
            Element::Stack(s) => Element::Stack(s.width(px)),
            other => other,
        }
    }
}

impl App for Installer {
    type Message = Msg;

    fn title(&self) -> String {
        String::from(if self.page == 5 && self.phase == Phase::Running { "Installing HamixOS…" } else { "Install HamixOS" })
    }

    fn view(&self) -> Element<Msg> {
        row![self.sidebar(), column![self.page_view(), space_fill(), divider(), self.footer()].spacing(16).padding([30, 36]).fill()].spacing(0).align(Align::Stretch).fill().into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Next => {
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
            }
            Msg::Back => {
                self.error.clear();
                if self.page == 5 {
                    self.page = 4;
                    self.phase = Phase::Setup;
                    self.fields[AUTH].clear();
                } else {
                    self.page = self.page.saturating_sub(1);
                }
            }
            Msg::PickDisk(i) => self.disk = Some(i),
            Msg::Field(i, v) => self.fields[i] = if i == USERNAME { v.to_lowercase() } else { v },
            Msg::Admin(v) => self.admin = v,
            Msg::Autostart(v) => self.autostart = v,
            Msg::Linux(v) => self.linux = v,
            Msg::Understand(v) => self.understand = v,
            Msg::Poll => self.poll(cx),
            Msg::Reboot => {
                let _ = hxport::spawn_elevated(&self.password, "/usr/bin/bash", &["-c", "reboot"]);
            }
            Msg::Close => cx.quit(),
        }
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        (self.phase == Phase::Running).then_some((250, Msg::Poll))
    }

    fn close_requested(&mut self, cx: &mut Context<Msg>) -> bool {
        if self.phase == Phase::Running {
            cx.message("The installation is running", "Wait until it finishes, closing now would leave the disk half-written.");
            false
        } else {
            true
        }
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("install").icon("install").size(860, 600).min_size(760, 540), Installer::new())
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
