#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, eprintln, sys};
use hxclient::ui::{self, theme, FieldAction, Hits, Style, TextField, Ui};
use hxclient::{Event, Window, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS};
use vellum::{Area, Painter};

const INITIAL_W: i32 = 640;
const INITIAL_H: i32 = 480;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Field(usize),
    Add,
    Run(usize),
    Remove(usize),
}

struct App {
    window: Window,
    ui: Ui,
    fields: [TextField; 3],
    focus: usize,
    commands: Vec<sys::CommandEntry>,
    hits: Hits<Hit>,
    hover: Option<Hit>,
    message: (String, bool),
}

impl App {
    fn refresh(&mut self) {
        self.commands = sys::cmd_list();
    }

    fn add(&mut self) {
        let name = String::from(self.fields[0].text.trim());
        let program = String::from(self.fields[1].text.trim());
        if name.is_empty() || program.is_empty() {
            self.message = (String::from("Name and program are required"), false);
            return;
        }
        let path = if program.contains('/') { program.clone() } else { format!("/usr/bin/{}", program) };
        let args: Vec<&str> = self.fields[2].text.split_whitespace().collect();
        let r = sys::cmd_register(&name, &path, &args);
        if r < 0 {
            self.message = (format!("Cannot register '{}': {}", name, sys::error_name(r)), false);
        } else {
            self.message = (format!("'{}' is now a command in every shell", name), true);
            for f in self.fields.iter_mut() {
                f.set("");
            }
            self.focus = 0;
        }
        self.refresh();
    }

    fn draw(&mut self) {
        let ui = &self.ui;
        let buffer = self.window.buffer();
        let mut p = Painter::new(buffer, ww(), hh());
        let hits = &mut self.hits;
        hits.clear();
        let hover = self.hover;
        p.fill(Area::new(0, 0, ww(), hh()), theme::bg());
        p.text(&ui.title, 20, 16, "Command registry", theme::text());
        p.text(&ui.font, 20, 44, "Commands registered here run by name in hsh and appear in the Nook menu.", theme::dim());
        let form = Area::new(16, 72, ww() - 32, 128);
        ui::card(&mut p, form);
        let labels = ["Name", "Program", "Arguments"];
        let widths = [150, 200, form.w - 150 - 200 - 48];
        let mut x = form.x + 12;
        for i in 0..3 {
            p.text(&ui.small, x + 2, form.y + 12, labels[i], theme::faint());
            let area = Area::new(x, form.y + 30, widths[i], 36);
            ui::text_field(&mut p, ui, area, &mut self.fields[i], self.focus == i, if i == 1 { "e.g. hxnotes" } else { "" });
            hits.add(area, Hit::Field(i));
            x += widths[i] + 12;
        }
        let add = Area::new(form.right() - 112, form.bottom() - 46, 100, 34);
        ui::button(&mut p, ui, add, "Add", Style::Primary, hover == Some(Hit::Add), true);
        hits.add(add, Hit::Add);
        if !self.message.0.is_empty() {
            ui::text_in(&mut p, &ui.font, form.x + 14, add, &self.message.0, if self.message.1 { theme::success() } else { theme::danger() });
        }
        let list = Area::new(16, 214, ww() - 32, hh() - 230);
        ui::card(&mut p, list);
        if self.commands.is_empty() {
            ui::centered_text(&mut p, &ui.font, list, "No commands yet", theme::faint());
        }
        for (i, c) in self.commands.iter().enumerate() {
            let row = Area::new(list.x + 8, list.y + 8 + i as i32 * 40, list.w - 16, 36);
            if row.bottom() > list.bottom() {
                break;
            }
            ui::symbol(&mut p, ui, "ui/run", row.x + 10, row.y + 10, theme::accent_hover());
            ui::text_in(&mut p, &ui.medium, row.x + 36, Area::new(row.x, row.y, 170, row.h), &c.name, theme::text());
            let target = format!("{} {}", c.path, c.args.join(" "));
            ui::text_in(&mut p, &ui.font, row.x + 180, Area::new(row.x, row.y, row.w - 180, row.h), &target, theme::dim());
            let run = Area::new(row.right() - 150, row.y + 2, 68, 32);
            let remove = Area::new(row.right() - 76, row.y + 2, 76, 32);
            ui::button(&mut p, ui, run, "Run", Style::Secondary, hover == Some(Hit::Run(i)), true);
            ui::button(&mut p, ui, remove, "Remove", Style::Flat, hover == Some(Hit::Remove(i)), true);
            hits.add(run, Hit::Run(i));
            hits.add(remove, Hit::Remove(i));
        }
        self.window.present();
    }
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&["ui/run"]);
    let window = match Window::open("Commands", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxcmd: {}", e);
            return 1;
        }
    };
    let mut app = App { window, ui, fields: [TextField::default(), TextField::default(), TextField::default()], focus: 0, commands: Vec::new(), hits: Hits::new(), hover: None, message: (String::new(), true) };
    app.window.set_icon("commands");
    app.window.set_min_size(480, 360);
    app.refresh();
    app.draw();
    loop {
        let event = app.window.wait_event(2000);
        match event {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Resize { .. }) | Some(Event::Theme { .. }) => app.draw(),
            Some(Event::Mouse { x, y, kind, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    let hover = if kind == MOUSE_LEAVE { None } else { app.hits.at(x, y) };
                    if hover != app.hover {
                        app.hover = hover;
                        app.draw();
                    }
                }
                MOUSE_PRESS => {
                    match app.hits.at(x, y) {
                        Some(Hit::Field(i)) => {
                            app.focus = i;
                            if let Some(area) = app.hits.area_of(Hit::Field(i)) {
                                let font = &app.ui.font;
                                app.fields[i].click(font, x - area.x - 10);
                            }
                        }
                        Some(Hit::Add) => app.add(),
                        Some(Hit::Run(i)) => {
                            if let Some(c) = app.commands.get(i) {
                                let name = c.name.clone();
                                if sys::cmd_run(&name, &[] as &[&str], sys::SPAWN_DETACH) < 0 {
                                    app.message = (format!("'{}' failed to start", name), false);
                                }
                            }
                        }
                        Some(Hit::Remove(i)) => {
                            if let Some(c) = app.commands.get(i) {
                                let r = sys::cmd_unregister(&c.name);
                                app.message = if r < 0 { (format!("Cannot remove: {}", sys::error_name(r)), false) } else { (format!("Removed '{}'", c.name), true) };
                                app.refresh();
                            }
                        }
                        None => {}
                    }
                    app.draw();
                }
                _ => {}
            },
            Some(Event::Key { code, .. }) => {
                match app.fields[app.focus].key(code) {
                    FieldAction::Submit => app.add(),
                    FieldAction::Next => app.focus = (app.focus + 1) % 3,
                    _ => {}
                }
                app.draw();
            }
            None => {
                let before = app.commands.len();
                app.refresh();
                if before != app.commands.len() {
                    app.draw();
                }
            }
            _ => {}
        }
    }
}

entry!(main);
