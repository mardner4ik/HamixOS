#![no_std]
#![no_main]

extern crate alloc;

mod unit;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::sys::{self, WaitEntry};
use hxstd::{entry, env, fs, println, users};

use unit::{Restart, Start, Unit};

const STOP_GRACE_MS: u64 = 5000;
const CONTROL_SERVICE: &str = "hamix.svc";
const LOG_DIR: &str = "/var/log/hx";
const RING_LINES: usize = 200;
const CHECK_MS: u64 = 2000;
const RESTART_DELAY_MS: u64 = 1000;
const RESTART_BURST: u32 = 5;
const REPLY_CHUNK: usize = 3000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Stopped,
    Running,
    Exited,
    Failed,
}

impl State {
    fn word(self) -> &'static str {
        match self {
            State::Stopped => "stopped",
            State::Running => "running",
            State::Exited => "exited",
            State::Failed => "failed",
        }
    }
}

struct Instance {
    unit: usize,
    name: String,
    arg: Option<String>,
    pid: Option<i64>,
    state: State,
    wanted: bool,
    restarts: u32,
    last_exit: Option<i64>,
    started_ms: u64,
    restart_at: Option<u64>,
    kill_at: Option<u64>,
    log_fd: Option<u64>,
    partial: Vec<u8>,
    ring: VecDeque<String>,
    memory_kb: u64,
}

struct Supervisor {
    units: Vec<Unit>,
    instances: Vec<Instance>,
    own_log: VecDeque<String>,
    test: Option<(String, i64)>,
    last_check: u64,
    uevents: Option<hxstd::uevent::Monitor>,
}

fn stamp() -> String {
    let ms = sys::uptime_ms();
    format!("[{:>5}.{:03}]", ms / 1000, ms % 1000)
}

fn log_file(name: &str) -> String {
    format!("{}/{}.log", LOG_DIR, name.replace('@', "-"))
}

fn push_ring(ring: &mut VecDeque<String>, line: String) {
    if ring.len() >= RING_LINES {
        ring.pop_front();
    }
    ring.push_back(line);
}

impl Supervisor {
    fn note(&mut self, text: String) {
        let line = format!("{} {}", stamp(), text);
        fs::append(&log_file("hxsvc"), format!("{}\n", line).as_bytes());
        push_ring(&mut self.own_log, line);
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.instances.iter().position(|i| i.name == name)
    }

    fn instance_for(&mut self, name: &str) -> Option<usize> {
        if let Some(i) = self.find(name) {
            return Some(i);
        }
        let (base, arg) = match name.split_once('@') {
            Some((b, a)) if !a.is_empty() => (format!("{}@", b), Some(String::from(a))),
            _ => (String::from(name), None),
        };
        let unit = self.units.iter().position(|u| u.name == base)?;
        if self.units[unit].template != arg.is_some() {
            return None;
        }
        self.instances.push(Instance {
            unit,
            name: String::from(name),
            arg,
            pid: None,
            state: State::Stopped,
            wanted: false,
            restarts: 0,
            last_exit: None,
            started_ms: 0,
            restart_at: None,
            kill_at: None,
            log_fd: None,
            partial: Vec::new(),
            ring: VecDeque::new(),
            memory_kb: 0,
        });
        Some(self.instances.len() - 1)
    }

    fn start(&mut self, index: usize) -> Result<i64, String> {
        if let Some(pid) = self.instances[index].pid {
            return Ok(pid);
        }
        let unit = self.units[self.instances[index].unit].clone();
        if unit.linux_layer && sys::linux_control(sys::LINUX_STATUS) == 0 {
            return Err(String::from("the Linux compatibility layer is off"));
        }
        let mut command = unit.command(self.instances[index].arg.as_deref());
        if let Some(user) = unit.user.as_ref().filter(|u| u.as_str() != "root") {
            let mut wrapped = alloc::vec![String::from("/usr/libexec/hamix/hxsvc"), String::from("--as"), user.clone(), String::from("--")];
            wrapped.extend(command);
            command = wrapped;
        }
        let program = command[0].clone();
        let args = &command[1..];
        let (pid, log_fd) = if unit.console {
            (sys::spawn_fds_env::<String, &str>(&program, args, None, 0, [None, None, None, None]), None)
        } else {
            let (read_end, write_end) = sys::pipe_flags(0).map_err(|e| String::from(sys::error_name(e)))?;
            sys::set_nonblocking(read_end);
            let pid = sys::spawn_fds_env::<String, &str>(&program, args, None, 0, [None, Some(write_end), Some(write_end), None]);
            sys::close(write_end);
            if pid < 0 {
                sys::close(read_end);
            }
            (pid, Some(read_end))
        };
        if pid < 0 {
            let inst = &mut self.instances[index];
            inst.state = State::Failed;
            return Err(format!("{}: {}", program, sys::error_name(pid)));
        }
        let inst = &mut self.instances[index];
        inst.pid = Some(pid);
        inst.state = State::Running;
        inst.wanted = true;
        inst.started_ms = sys::uptime_ms();
        inst.restart_at = None;
        inst.log_fd = log_fd;
        let name = inst.name.clone();
        self.note(format!("started {} (pid {})", name, pid));
        Ok(pid)
    }

    fn stop(&mut self, index: usize) {
        self.instances[index].wanted = false;
        self.instances[index].restart_at = None;
        if let Some(pid) = self.instances[index].pid {
            sys::send_signal(pid, 15);
            self.instances[index].kill_at = Some(sys::uptime_ms() + STOP_GRACE_MS);
            let name = self.instances[index].name.clone();
            self.note(format!("stopping {} (pid {})", name, pid));
        }
    }

    fn drain(&mut self, index: usize) {
        let Some(fd) = self.instances[index].log_fd else {
            return;
        };
        let mut buf = [0u8; 2048];
        let mut closed = false;
        loop {
            let n = sys::read(fd, &mut buf);
            if n <= 0 {
                closed = n == 0;
                break;
            }
            self.instances[index].partial.extend_from_slice(&buf[..n as usize]);
        }
        let name = self.instances[index].name.clone();
        let mut lines = Vec::new();
        {
            let inst = &mut self.instances[index];
            while let Some(pos) = inst.partial.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = inst.partial.drain(..=pos).collect();
                lines.push(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
            }
            if closed && !inst.partial.is_empty() {
                lines.push(String::from_utf8_lossy(&inst.partial).into_owned());
                inst.partial.clear();
            }
        }
        if !lines.is_empty() {
            let mut text = String::new();
            for line in lines {
                let stamped = format!("{} {}", stamp(), line);
                text.push_str(&stamped);
                text.push('\n');
                push_ring(&mut self.instances[index].ring, stamped);
            }
            fs::append(&log_file(&name), text.as_bytes());
        }
        if closed {
            sys::close(fd);
            self.instances[index].log_fd = None;
        }
    }

    fn reap(&mut self) {
        for index in 0..self.instances.len() {
            let Some(pid) = self.instances[index].pid else {
                continue;
            };
            let code = sys::waitpid(pid, true);
            if code == sys::EAGAIN {
                continue;
            }
            self.drain(index);
            let unit = &self.units[self.instances[index].unit];
            let restart = unit.restart;
            let inst = &mut self.instances[index];
            inst.pid = None;
            inst.kill_at = None;
            inst.last_exit = Some(code);
            inst.state = if code == 0 { State::Exited } else { State::Failed };
            let quick = sys::uptime_ms().saturating_sub(inst.started_ms) < 10_000;
            inst.restarts = if quick { inst.restarts + 1 } else { 1 };
            let again = inst.wanted && (restart == Restart::Always || (restart == Restart::OnFailure && code != 0));
            let name = inst.name.clone();
            let burst = inst.restarts > RESTART_BURST;
            if again && !burst {
                inst.restart_at = Some(sys::uptime_ms() + RESTART_DELAY_MS * inst.restarts as u64);
            } else {
                inst.wanted = false;
            }
            self.note(format!("{} (pid {}) exited with code {}{}", name, pid, code, if again && burst { ", restarting too often, giving up" } else { "" }));
            let freed = if self.instances[index].wanted { None } else { self.units[self.instances[index].unit].vt.first().cloned() };
            if let Some(tty) = freed {
                self.start_vt(&tty);
            }
        }
    }

    fn finish_test(&mut self, script: &str, code: i64) {
        println!("hxtest: {} finished", script);
        for index in 0..self.instances.len() {
            self.drain(index);
        }
        sys::sync();
        println!("hxtest: exit {}", code);
        sys::sleep_ms(100);
        sys::power(sys::POWER_OFF);
    }

    fn check_memory(&mut self) {
        let now = sys::uptime_ms();
        if now.saturating_sub(self.last_check) < CHECK_MS {
            return;
        }
        self.last_check = now;
        let procs = sys::proc_list();
        let mut over = Vec::new();
        for (index, inst) in self.instances.iter_mut().enumerate() {
            let Some(pid) = inst.pid else {
                continue;
            };
            inst.memory_kb = procs.iter().find(|p| p.pid == pid).map(|p| p.heap_kb).unwrap_or(0);
            let limit = self.units[inst.unit].memory_max_kb;
            if limit > 0 && inst.memory_kb > limit {
                over.push((index, inst.memory_kb, limit));
            }
        }
        for (index, used, limit) in over {
            let name = self.instances[index].name.clone();
            self.note(format!("{} uses {} KiB, over its memory_max of {} KiB; killing it", name, used, limit));
            if let Some(pid) = self.instances[index].pid {
                sys::kill(pid);
            }
        }
    }

    fn kill_overdue(&mut self) {
        let now = sys::uptime_ms();
        for index in 0..self.instances.len() {
            let inst = &mut self.instances[index];
            if inst.kill_at.map(|t| now >= t).unwrap_or(false) {
                inst.kill_at = None;
                if let Some(pid) = inst.pid {
                    sys::kill(pid);
                }
            }
        }
    }

    fn restart_due(&mut self) {
        self.kill_overdue();
        let now = sys::uptime_ms();
        for index in 0..self.instances.len() {
            if self.instances[index].restart_at.map(|t| now >= t).unwrap_or(false) {
                self.instances[index].restart_at = None;
                if let Err(e) = self.start(index) {
                    let name = self.instances[index].name.clone();
                    self.note(format!("cannot restart {}: {}", name, e));
                }
            }
        }
    }

    fn next_deadline(&self) -> Option<u64> {
        self.instances.iter().filter_map(|i| i.restart_at).chain(self.instances.iter().filter_map(|i| i.kill_at)).min()
    }

    fn start_named(&mut self, name: &str) -> Result<i64, String> {
        let index = self.instance_for(name).ok_or_else(|| format!("no service called {}", name))?;
        let result = self.start(index);
        if let Err(e) = &result {
            self.note(format!("cannot start {}: {}", name, e));
        }
        result
    }

    fn apply_devfs(&mut self) {
        let groups = users::group_ids();
        let Some(text) = fs::read_to_string("/etc/devfs.conf") else {
            return;
        };
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 3 || f[0].starts_with('#') {
                continue;
            }
            let pattern = format!("/dev/{}", f[1]);
            let targets: Vec<String> = match pattern.rsplit_once('/') {
                Some((dir, name)) if name.ends_with('*') => {
                    let prefix = name.trim_end_matches('*');
                    sys::read_dir(dir).unwrap_or_default().into_iter().filter(|e| e.name.starts_with(prefix)).map(|e| format!("{}/{}", dir, e.name)).collect()
                }
                _ => alloc::vec![pattern.clone()],
            };
            for target in targets {
                match f[0] {
                    "own" => {
                        let (owner, group) = f[2].split_once(':').unwrap_or((f[2], ""));
                        let uid = users::find("/", owner).map(|u| u.uid);
                        let gid = groups.iter().find(|(n, _)| n == group).map(|(_, g)| *g);
                        sys::chown_group(&target, uid, gid);
                    }
                    "perm" => {
                        if let Ok(mode) = u32::from_str_radix(f[2], 8) {
                            sys::chmod(&target, mode);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn boot(&mut self) {
        self.apply_devfs();
        for index in unit::boot_order(&self.units) {
            let unit = &self.units[index];
            if unit.start != Start::Boot || unit.template || (unit.console && self.test.is_some()) {
                continue;
            }
            if !unit.vt.is_empty() && !Self::display_managers_enabled() {
                continue;
            }
            let name = unit.name.clone();
            let _ = self.start_named(&name);
        }
        let providers: Vec<String> = self.units.iter().filter(|u| u.start == Start::Demand).flat_map(|u| u.provides.clone()).collect();
        for service in providers {
            sys::service_activator(&service);
        }
    }

    fn display_managers_enabled() -> bool {
        cmdline_value("dm").map(|v| v != "none").unwrap_or(true)
    }

    fn vt_claimed(&self, tty: &str) -> bool {
        Self::display_managers_enabled()
            && self.units.iter().enumerate().any(|(index, u)| {
                u.vt.iter().any(|v| v == tty) && self.instances.iter().any(|i| i.unit == index && (i.pid.is_some() || i.wanted))
            })
    }

    fn start_vt(&mut self, tty: &str) {
        if self.vt_claimed(tty) {
            return;
        }
        let templates: Vec<String> = self.units.iter().filter(|u| u.start == Start::Vt && u.template).map(|u| u.name.clone()).collect();
        for template in templates {
            let name = format!("{}{}", template, tty);
            let _ = self.start_named(&name);
        }
    }

    fn activate(&mut self, service: &str) {
        let unit = self.units.iter().find(|u| u.provides.iter().any(|p| p == service)).map(|u| u.name.clone());
        match unit {
            Some(name) => {
                self.note(format!("activating {} for {}", name, service));
                let _ = self.start_named(&name);
            }
            None => self.note(format!("nothing provides {}", service)),
        }
    }

    fn status_line(&self, inst: &Instance) -> String {
        let unit = &self.units[inst.unit];
        format!(
            "{:<18} {:<8} {:>6} {:>3} {:>8} {:<6} {}",
            inst.name,
            inst.state.word(),
            inst.pid.map(|p| format!("{}", p)).unwrap_or_else(|| String::from("-")),
            inst.restarts,
            if inst.pid.is_some() { format!("{}K", inst.memory_kb) } else { String::from("-") },
            if unit.linux_layer { "linux" } else { "native" },
            unit.description
        )
    }

    fn status(&mut self, name: Option<&str>) -> String {
        let mut out = String::new();
        match name {
            None => {
                out.push_str(&format!("{:<18} {:<8} {:>6} {:>3} {:>8} {:<6} {}\n", "SERVICE", "STATE", "PID", "RST", "MEMORY", "LAYER", "DESCRIPTION"));
                for (i, unit) in self.units.iter().enumerate() {
                    if unit.template || self.instances.iter().any(|x| x.unit == i) {
                        continue;
                    }
                    out.push_str(&format!("{:<18} {:<8} {:>6} {:>3} {:>8} {:<6} {}\n", unit.name, "stopped", "-", 0, "-", if unit.linux_layer { "linux" } else { "native" }, unit.description));
                }
                for inst in self.instances.iter() {
                    out.push_str(&self.status_line(inst));
                    out.push('\n');
                }
            }
            Some(name) => match self.instance_for(name) {
                Some(index) => {
                    let inst = &self.instances[index];
                    let unit = &self.units[inst.unit];
                    out.push_str(&self.status_line(inst));
                    out.push('\n');
                    out.push_str(&format!("  exec     {}\n", unit.command(inst.arg.as_deref()).join(" ")));
                    if let Some(code) = inst.last_exit {
                        out.push_str(&format!("  last exit code {}\n", code));
                    }
                    for line in inst.ring.iter().rev().take(10).collect::<Vec<_>>().into_iter().rev() {
                        out.push_str(&format!("  {}\n", line));
                    }
                }
                None => out.push_str(&format!("no service called {}\n", name)),
            },
        }
        out
    }

    fn log_text(&self, name: Option<&str>, lines: usize) -> String {
        let ring = match name {
            None | Some("hxsvc") => &self.own_log,
            Some(n) => match self.find(n) {
                Some(i) => &self.instances[i].ring,
                None => return format!("no log for {}\n", n),
            },
        };
        let mut out = String::new();
        let skip = ring.len().saturating_sub(lines);
        for line in ring.iter().skip(skip) {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    fn set_layer(&mut self, linux_on: bool) -> String {
        let mut changed = Vec::new();
        for index in 0..self.instances.len() {
            if self.units[self.instances[index].unit].linux_layer && !linux_on && self.instances[index].pid.is_some() {
                self.stop(index);
                changed.push(self.instances[index].name.clone());
            }
        }
        if linux_on {
            let names: Vec<String> = self.units.iter().filter(|u| u.linux_layer && u.start == Start::Boot && !u.template).map(|u| u.name.clone()).collect();
            for name in names {
                if self.start_named(&name).is_ok() {
                    changed.push(name);
                }
            }
        }
        format!("{} Linux-layer service(s) {}: {}\n", changed.len(), if linux_on { "started" } else { "stopped" }, changed.join(" "))
    }

    fn shutdown(&mut self, mode: u64) -> ! {
        self.note(String::from(if mode == sys::POWER_REBOOT { "rebooting" } else { "powering off" }));
        for index in 0..self.instances.len() {
            self.stop(index);
        }
        sys::sleep_ms(300);
        self.reap();
        sys::sync();
        sys::power(mode);
        sys::exit(0)
    }

    fn reload(&mut self) -> String {
        let mut notes = Vec::new();
        let fresh = unit::load_all(&mut |e| notes.push(e));
        let mut added = 0;
        for unit in fresh {
            match self.units.iter().position(|u| u.name == unit.name) {
                Some(i) => self.units[i] = unit,
                None => {
                    self.units.push(unit);
                    added += 1;
                }
            }
        }
        for n in notes {
            self.note(n);
        }
        format!("{} unit(s), {} new\n", self.units.len(), added)
    }

    fn command(&mut self, sender: i64, text: &str) -> Option<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let root = sender == 0 || sys::proc_list().iter().find(|p| p.pid == sender).map(|p| p.uid == 0).unwrap_or(false);
        let denied = || Some(String::from("permission denied: run it as root (sudo hxctl ...)\n"));
        match words.as_slice() {
            ["activate", service] => {
                self.activate(service);
                None
            }
            ["status"] => Some(self.status(None)),
            ["status", name] => Some(self.status(Some(name))),
            ["log"] => Some(self.log_text(None, 50)),
            ["log", name] => Some(self.log_text(Some(name), 50)),
            ["log", name, n] => Some(self.log_text(Some(name), n.parse().unwrap_or(50))),
            ["start", ..] | ["stop", ..] | ["restart", ..] | ["reload"] | ["layer", ..] | ["poweroff"] | ["reboot"] if !root => denied(),
            ["start", name] => Some(match self.start_named(name) {
                Ok(pid) => format!("{} is running (pid {})\n", name, pid),
                Err(e) => format!("cannot start {}: {}\n", name, e),
            }),
            ["stop", name] => Some(match self.find(name) {
                Some(i) => {
                    self.stop(i);
                    format!("{} stopped\n", name)
                }
                None => format!("{} is not running\n", name),
            }),
            ["restart", name] => Some(match self.find(name) {
                Some(i) => {
                    self.stop(i);
                    self.instances[i].restart_at = Some(sys::uptime_ms() + 300);
                    self.instances[i].wanted = true;
                    self.instances[i].restarts = 0;
                    format!("{} restarting\n", name)
                }
                None => match self.start_named(name) {
                    Ok(pid) => format!("{} is running (pid {})\n", name, pid),
                    Err(e) => format!("cannot start {}: {}\n", name, e),
                },
            }),
            ["reload"] => Some(self.reload()),
            ["layer", "linux", "off"] => Some(self.set_layer(false)),
            ["layer", "linux", "on"] => Some(self.set_layer(true)),
            ["poweroff"] => self.shutdown(sys::POWER_OFF),
            ["reboot"] => self.shutdown(sys::POWER_REBOOT),
            _ => Some(format!("unknown request: {}\n", text)),
        }
    }

    fn handle_uevents(&mut self) {
        let mut ttys = Vec::new();
        if let Some(monitor) = self.uevents.as_mut() {
            while let Some(event) = monitor.next() {
                if event.subsystem() == "tty" && event.get("HAMIX_LOGIN") == Some("1") {
                    if let Some(tty) = event.get("DEVNAME").and_then(|d| d.strip_prefix("tty")) {
                        ttys.push(String::from(tty));
                    }
                }
            }
        }
        for tty in ttys {
            self.start_vt(&tty);
        }
    }

    fn wait_entries(&self, extra: &[WaitEntry]) -> Vec<WaitEntry> {
        let mut entries = Vec::with_capacity(self.instances.len() + 4 + extra.len());
        entries.push(WaitEntry::mailbox());
        entries.extend_from_slice(extra);
        if let Some(monitor) = self.uevents.as_ref() {
            entries.push(WaitEntry::fd(monitor.fd()));
        }
        entries
    }

    fn handle_messages(&mut self) {
        let mut buf = [0u8; 4096];
        while let Some((sender, n)) = sys::msg_recv(&mut buf, 0) {
            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
            if let Some(reply) = self.command(sender, text.trim()) {
                send_reply(sender, &reply);
            }
        }
    }

    fn run(&mut self) -> ! {
        loop {
            let mut entries = self.wait_entries(&[WaitEntry::child(0)]);
            for inst in self.instances.iter() {
                if let Some(fd) = inst.log_fd {
                    entries.push(WaitEntry::fd(fd));
                }
            }
            let now = sys::uptime_ms();
            let wake = self.next_deadline().map(|t| t.min(now + CHECK_MS)).unwrap_or(now + CHECK_MS);
            entries.push(WaitEntry::timer_at_ms(wake));
            sys::wait_any(&mut entries, Some(CHECK_MS));
            self.handle_uevents();
            self.handle_messages();
            for index in 0..self.instances.len() {
                self.drain(index);
            }
            self.reap();
            self.restart_due();
            self.check_memory();
        }
    }
}

fn send_reply(pid: i64, text: &str) {
    let bytes = text.as_bytes();
    let mut chunks: Vec<&[u8]> = bytes.chunks(REPLY_CHUNK).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.iter().enumerate() {
        let mut message = Vec::with_capacity(chunk.len() + 1);
        message.push(if i == last { b'E' } else { b'M' });
        message.extend_from_slice(chunk);
        sys::msg_send(pid, &message);
    }
}

fn run_as(args: &[String]) -> i32 {
    let Some(sep) = args.iter().position(|a| a == "--") else {
        println!("usage: hxsvc --as USER -- PROGRAM [ARGS...]");
        return 2;
    };
    let (Some(user), Some(program)) = (args.get(1), args.get(sep + 1)) else {
        println!("usage: hxsvc --as USER -- PROGRAM [ARGS...]");
        return 2;
    };
    let Some(record) = users::find("/", user) else {
        println!("hxsvc: no user {}", user);
        return 1;
    };
    sys::init_groups(&record.name, record.gid);
    sys::setgid(record.gid);
    if sys::setuid(record.uid) < 0 {
        println!("hxsvc: cannot become {}", user);
        return 1;
    }
    sys::chdir(&record.home);
    let env: Vec<String> = env::vars().iter().map(|(k, v)| match k.as_str() {
        "HOME" => format!("HOME={}", record.home),
        "USER" | "LOGNAME" => format!("{}={}", k, record.name),
        _ => format!("{}={}", k, v),
    }).collect();
    let code = sys::exec(program, &args[sep + 1..], &env);
    println!("hxsvc: cannot run {}: {}", program, sys::error_name(code));
    127
}

fn cmdline_value(key: &str) -> Option<String> {
    let text = fs::read_to_string("/proc/cmdline")?;
    text.split_whitespace().find_map(|w| w.strip_prefix(key).and_then(|r| r.strip_prefix('=')).map(String::from))
}

fn main() -> i32 {
    let args = env::args();
    if args.get(1).map(|a| a == "--as").unwrap_or(false) {
        return run_as(&args[1..]);
    }
    if args.get(1).map(|a| a == "--help" || a == "-h").unwrap_or(false) {
        println!("hxsvc -- the HamixOS service supervisor (PID 1)");
        println!("services: {}/*.svc, logs: {}, control: hxctl", unit::UNIT_DIR, LOG_DIR);
        return 0;
    }
    if sys::getpid() != 1 {
        println!("hxsvc: already running as PID 1; use hxctl to control it");
        return 1;
    }
    sys::mkdir("/var/log");
    sys::mkdir(LOG_DIR);
    let mut early = Vec::new();
    let units = unit::load_all(&mut |e| early.push(e));
    let mut sv = Supervisor { units, instances: Vec::new(), own_log: VecDeque::new(), test: None, last_check: 0, uevents: hxstd::uevent::Monitor::open() };
    let (version, _) = sys::abi_info();
    sv.note(format!("hxsvc started, Hamix ABI {}.{}, {} unit(s)", version >> 8, version & 0xFF, sv.units.len()));
    for e in early {
        sv.note(e);
    }
    sys::service_register(CONTROL_SERVICE);
    let test = cmdline_value("hxtest");
    if test.is_some() {
        sv.test = Some((String::new(), -1));
    }
    sv.boot();
    match test {
        Some(script) => {
            let pid = sys::spawn_fds_env::<&str, &str>("/usr/bin/bash", &[script.as_str()], None, 0, [None, None, None, None]);
            if pid < 0 {
                println!("hxtest: cannot start bash: {}", sys::error_name(pid));
                sv.finish_test(&script, -1);
            }
            sv.test = Some((script, pid));
            loop {
                let code = sys::waitpid(pid, true);
                if code != sys::EAGAIN {
                    let script = sv.test.as_ref().map(|t| t.0.clone()).unwrap_or_default();
                    sv.finish_test(&script, code);
                }
                let mut entries = sv.wait_entries(&[WaitEntry::child(pid as u64)]);
                sys::wait_any(&mut entries, Some(500));
                sv.handle_uevents();
                sv.handle_messages();
                for index in 0..sv.instances.len() {
                    sv.drain(index);
                }
                sv.reap();
                sv.restart_due();
            }
        }
        None => {
            sv.start_vt("1");
            sv.run()
        }
    }
}

entry!(main);
