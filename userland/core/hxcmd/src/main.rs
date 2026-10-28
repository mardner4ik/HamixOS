#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{button, column, icon, input, list, row, space, text, title, App, Context, Element, Length, Settings, Tone};

const COMMANDS_DIR: &str = "/var/lib/hamix/commands";
const COMMANDS_BIN: &str = "/var/lib/hamix/commands/bin";
const LEGACY_COMMANDS: &str = "/etc/commands";
const RESERVED: &[&str] = &[
    "help", "clear", "edit", "echo", "uname", "whoami", "id", "pwd", "ls", "cd", "cat", "mkdir", "touch", "rm", "chmod", "chown", "exec", "startx", "hostname", "date", "sudo", "passwd", "useradd",
    "halt", "poweroff", "reboot", "version", "history", "dmesg", "logout", "exit", "ps", "kill", "cmd", "sync", "free",
];

#[derive(Clone)]
struct Command {
    name: String,
    owner: u32,
    path: String,
    args: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn parse(line: &str) -> Option<Command> {
    let f: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
    if f.len() < 3 || !valid_name(f[0]) {
        return None;
    }
    Some(Command { name: String::from(f[0]), owner: f[1].parse().unwrap_or(0), path: String::from(f[2]), args: f[3..].iter().map(|a| String::from(*a)).collect() })
}

fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

fn registered() -> Vec<Command> {
    let mut out: Vec<Command> = hxport::entries(COMMANDS_DIR)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| hxport::read_to_string(&format!("{}/{}", COMMANDS_DIR, e.name)).as_deref().and_then(parse))
        .collect();
    for line in hxport::read_to_string(LEGACY_COMMANDS).unwrap_or_default().lines() {
        if let Some(c) = parse(line) {
            if !out.iter().any(|o| o.name == c.name) {
                out.push(c);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn register(name: &str, program: &str, args: &[&str]) -> Result<(), String> {
    if !valid_name(name) || RESERVED.contains(&name) {
        return Err(String::from("this name is not allowed"));
    }
    if args.iter().any(|a| a.contains('\t')) || program.contains('\t') {
        return Err(String::from("tabs are not allowed"));
    }
    if !hxport::exists(program) {
        return Err(format!("{} does not exist", program));
    }
    let uid = hxport::uid();
    if let Some(existing) = registered().into_iter().find(|c| c.name == name) {
        if existing.owner != uid && uid != 0 {
            return Err(String::from("it belongs to another user"));
        }
    }
    hxport::mkdir(COMMANDS_DIR);
    let mut line = format!("{}\t{}\t{}", name, uid, program);
    for a in args {
        line.push('\t');
        line.push_str(a);
    }
    line.push('\n');
    let file = format!("{}/{}", COMMANDS_DIR, name);
    if !hxport::write(&file, line.as_bytes()) {
        return Err(String::from("permission denied"));
    }
    hxport::chmod(&file, 0o644);
    hxport::mkdir(COMMANDS_BIN);
    let mut script = format!("#!/bin/sh\nexec {}", quote(program));
    for a in args {
        script.push(' ');
        script.push_str(&quote(a));
    }
    script.push_str(" \"$@\"\n");
    let wrapper = format!("{}/{}", COMMANDS_BIN, name);
    if hxport::write(&wrapper, script.as_bytes()) {
        hxport::chmod(&wrapper, 0o755);
    }
    Ok(())
}

fn unregister(command: &Command) -> Result<(), String> {
    let uid = hxport::uid();
    if command.owner != uid && uid != 0 {
        return Err(String::from("it belongs to another user"));
    }
    hxport::remove(&format!("{}/{}", COMMANDS_BIN, command.name));
    if hxport::remove(&format!("{}/{}", COMMANDS_DIR, command.name)) { Ok(()) } else { Err(String::from("permission denied")) }
}

struct Commands {
    name: String,
    program: String,
    args: String,
    commands: Vec<Command>,
    message: (String, Tone),
}

#[derive(Clone)]
enum Msg {
    Name(String),
    Program(String),
    Args(String),
    Add,
    Run(usize),
    Remove(usize),
    Refresh,
}

impl App for Commands {
    type Message = Msg;

    fn title(&self) -> String {
        String::from("Commands")
    }

    fn view(&self) -> Element<Msg> {
        let field = |label: &str, value: &str, hint: &str, msg: fn(String) -> Msg, width: Length| -> Element<Msg> {
            column![text(label).small().tone(Tone::Faint), input(value, hint, msg).on_submit(Msg::Add).width(Length::Fill)].spacing(4).width(width).into()
        };
        let form = column![
            row![
                field("Name", &self.name, "", Msg::Name, Length::Fixed(150)),
                field("Program", &self.program, "e.g. hxnotes", Msg::Program, Length::Fixed(200)),
                field("Arguments", &self.args, "", Msg::Args, Length::Fill),
            ]
            .spacing(12),
            row![text(self.message.0.clone()).tone(self.message.1), space(), button("Add").primary().on_press(Msg::Add).width(100)],
        ]
        .spacing(10)
        .card();
        let body: Element<Msg> = if self.commands.is_empty() {
            column![text("No commands yet").tone(Tone::Faint)].align(hxtoolkit::Align::Center).padding(40).card().fill().into()
        } else {
            list(
                self.commands
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        row![
                            icon("ui/run"),
                            text(c.name.clone()).strong().width(150),
                            text(format!("{} {}", c.path, c.args.join(" "))).dim().fill(),
                            button("Run").on_press(Msg::Run(i)),
                            button("Remove").flat().on_press(Msg::Remove(i)),
                        ]
                        .spacing(10)
                        .into()
                    })
                    .collect(),
            )
            .row_height(42)
            .into()
        };
        column![
            title("Command registry"),
            text("Commands registered here run by name in bash and appear in the Nook menu.").dim(),
            form,
            body,
        ]
        .padding(16)
        .spacing(12)
        .fill()
        .into()
    }

    fn update(&mut self, message: Msg, _cx: &mut Context<Msg>) {
        match message {
            Msg::Name(v) => self.name = v,
            Msg::Program(v) => self.program = v,
            Msg::Args(v) => self.args = v,
            Msg::Add => {
                let name = String::from(self.name.trim());
                let program = String::from(self.program.trim());
                if name.is_empty() || program.is_empty() {
                    self.message = (String::from("Name and program are required"), Tone::Danger);
                    return;
                }
                let path = if program.contains('/') { program } else { format!("/usr/bin/{}", program) };
                let args: Vec<&str> = self.args.split_whitespace().collect();
                match register(&name, &path, &args) {
                    Ok(()) => {
                        self.message = (format!("'{}' is now a command in every shell", name), Tone::Success);
                        self.name.clear();
                        self.program.clear();
                        self.args.clear();
                    }
                    Err(e) => self.message = (format!("Cannot register '{}': {}", name, e), Tone::Danger),
                }
                self.commands = registered();
            }
            Msg::Run(i) => {
                if let Some(c) = self.commands.get(i) {
                    let args: Vec<&str> = c.args.iter().map(|a| a.as_str()).collect();
                    if let Err(e) = hxport::spawn(&c.path, &args) {
                        self.message = (format!("'{}' failed to start: {}", c.name, e), Tone::Danger);
                    }
                }
            }
            Msg::Remove(i) => {
                if let Some(c) = self.commands.get(i).cloned() {
                    self.message = match unregister(&c) {
                        Ok(()) => (format!("Removed '{}'", c.name), Tone::Success),
                        Err(e) => (format!("Cannot remove: {}", e), Tone::Danger),
                    };
                    self.commands = registered();
                }
            }
            Msg::Refresh => self.commands = registered(),
        }
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        Some((2000, Msg::Refresh))
    }
}

fn run() -> i32 {
    let app = Commands { name: String::new(), program: String::new(), args: String::new(), commands: registered(), message: (String::new(), Tone::Dim) };
    hxtoolkit::run(Settings::new("commands").size(640, 480).min_size(480, 360), app)
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
