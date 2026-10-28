use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{button, column, divider, icon, row, scroll, text, title, Align, App, Backend, Context, Element, Key, KeyPress, Length, Settings, TextSize, Tone};

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    About,
    Credits,
    System,
}

#[derive(Clone)]
pub enum Msg {
    Show(Page),
    CopyDetails,
    Close,
}

pub struct About {
    page: Page,
    os: String,
    kernel: String,
    backend: String,
    copied: bool,
}

fn os_release() -> String {
    let text = hxport::read_to_string("/etc/os-release").unwrap_or_default();
    let value = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)).map(|v| String::from(v.trim_matches('"')));
    value("PRETTY_NAME=").or_else(|| value("NAME=")).unwrap_or_else(|| String::from("unknown system"))
}

fn kernel() -> String {
    hxport::read_to_string("/proc/version").and_then(|v| v.lines().next().map(String::from)).unwrap_or_else(|| String::from("unknown"))
}

fn backend() -> String {
    match hxtoolkit::detect() {
        Some(Backend::Nook) => String::from("Nook desktop (hxserver)"),
        Some(Backend::Wayland) => String::from("Wayland, HamixOS widgets"),
        Some(Backend::Gtk) if hxport::var("WAYLAND_DISPLAY").map(|v| !v.is_empty()).unwrap_or(false) => String::from("GTK 4 on Wayland"),
        Some(Backend::Gtk) => String::from("GTK 4 on X11"),
        None => String::from("none"),
    }
}

fn line(label: &str, value: impl Into<String>) -> Element<Msg> {
    row![text(String::from(label)).small().tone(Tone::Dim).width(120u32), text(value.into()).small().selectable().wrap().width(Length::Fill)].spacing(12).width(Length::Fill).into()
}

impl About {
    pub fn new() -> About {
        About { page: Page::About, os: os_release(), kernel: kernel(), backend: backend(), copied: false }
    }

    pub fn settings() -> Settings {
        Settings::new("files").icon("files").size(440, 580).fixed()
    }

    fn details(&self) -> String {
        format!("Files {}\nSystem: {}\nKernel: {}\nToolkit: hxtoolkit, {}\n", env!("CARGO_PKG_VERSION"), self.os, self.kernel, self.backend)
    }

    fn tab(&self, label: &str, page: Page) -> Element<Msg> {
        let b = button(String::from(label)).on_press(Msg::Show(page));
        if self.page == page { b.primary().into() } else { b.flat().into() }
    }

    fn body(&self) -> Element<Msg> {
        match self.page {
            Page::About => column![
                text("Browse, organise and inspect the files on your computer.").wrap().align(Align::Center).width(Length::Fill),
                text("Copy, move, rename and delete files and folders, open them with the right program, check their permissions, owners and disk usage, and pick files for other programs.").small().tone(Tone::Dim).wrap().align(Align::Center).width(Length::Fill),
                text("Files is free software: you can redistribute it and modify it under the terms of the GNU General Public License, version 3.").small().tone(Tone::Dim).wrap().align(Align::Center).width(Length::Fill),
            ]
            .spacing(12)
            .width(Length::Fill)
            .into(),
            Page::Credits => column![
                line("Written by", "The HamixOS developers"),
                line("Toolkit", "hxtoolkit, the declarative UI toolkit of HamixOS"),
                line("Drawing", "vellum (Nook) or GTK 4"),
                line("Icons", "Nook Icons"),
                line("License", "GNU GPL v3"),
            ]
            .spacing(10)
            .width(Length::Fill)
            .into(),
            Page::System => {
                let mut rows: Vec<Element<Msg>> = alloc::vec![line("Version", env!("CARGO_PKG_VERSION")), line("System", self.os.clone()), line("Kernel", self.kernel.clone()), line("Running on", self.backend.clone())];
                if let Some(exe) = hxport::current_exe() {
                    rows.push(line("Program", exe));
                }
                let mut col = column(rows).spacing(10).width(Length::Fill);
                col = col.push(row![hxtoolkit::space(), button(if self.copied { "Copied" } else { "Copy details" }).flat().on_press(Msg::CopyDetails)].width(Length::Fill));
                col.into()
            }
        }
    }
}

impl App for About {
    type Message = Msg;

    fn title(&self) -> String {
        String::from("About Files")
    }

    fn view(&self) -> Element<Msg> {
        let header = column![
            icon("apps/files").size(96u32, 96u32),
            title("Files").size(TextSize::Display),
            text(format!("Version {}", env!("CARGO_PKG_VERSION"))).tone(Tone::Dim),
            text(self.os.clone()).small().tone(Tone::Faint),
        ]
        .spacing(6)
        .align(Align::Center)
        .padding([24, 16, 12, 16])
        .width(Length::Fill);
        let tabs = row![hxtoolkit::space(), self.tab("About", Page::About), self.tab("Credits", Page::Credits), self.tab("System", Page::System), hxtoolkit::space()].spacing(6).align(Align::Center).width(Length::Fill);
        let footer = row![hxtoolkit::space(), button("Close").primary().on_press(Msg::Close)].padding(12).width(Length::Fill);
        column![header, tabs, scroll(column![self.body()].padding([16, 24])).height(Length::Fill).width(Length::Fill), divider(), footer].spacing(8).width(Length::Fill).height(Length::Fill).into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Show(page) => self.page = page,
            Msg::CopyDetails => {
                cx.copy(self.details());
                self.copied = true;
            }
            Msg::Close => cx.quit(),
        }
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        match press.key {
            Key::Escape => Some(Msg::Close),
            _ => None,
        }
    }
}
