#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{button, canvas, column, list, row, scroll, space, text, title, Align, App, CanvasEvent, Color, Context, Element, Key, KeyPress, Length, Pixmap, Settings, TextSize, Tone};

mod platform {
    use alloc::string::String;
    use alloc::vec::Vec;

    pub fn read(path: &str) -> Option<String> {
        hxport::read_to_string(path)
    }

    pub fn entries(dir: &str) -> Vec<String> {
        hxport::entries(dir).unwrap_or_default().into_iter().map(|e| e.name).collect()
    }

    pub fn kill(pid: i64) -> Result<(), String> {
        hxport::kill(pid)
    }

    pub fn first_arg() -> Option<String> {
        hxport::args().get(1).cloned()
    }
}

const HISTORY: usize = 120;
const CORE_COLORS: [u32; 8] = [0x5b8cff, 0x2dd4bf, 0xf59e0b, 0xf472b6, 0xa78bfa, 0x34d399, 0xfb7185, 0x60a5fa];
const MEMORY_COLOR: u32 = 0x2dd4bf;
const MEMORY_GRAPH: usize = usize::MAX;

struct Proc {
    pid: i64,
    name: String,
    user: String,
    state: char,
    memory: u64,
    cpu_ms: u64,
    usage: u32,
    core: i32,
}

#[derive(Default)]
struct Memory {
    total: u64,
    used: u64,
    cache: u64,
}

#[derive(Default)]
struct Monitor {
    tab: usize,
    procs: Vec<Proc>,
    selected: Option<i64>,
    cpu_history: VecDeque<u32>,
    core_history: Vec<VecDeque<u32>>,
    mem_history: VecDeque<u32>,
    last_cpu: Vec<(i64, u64)>,
    last_cores: Vec<(u64, u64)>,
    memory: Memory,
    users: BTreeMap<u32, String>,
    graphs: BTreeMap<usize, (u32, u32)>,
    cpu_model: String,
    message: String,
}

#[derive(Clone)]
enum Msg {
    Tab(usize),
    Select(usize),
    End,
    Sample,
    GraphSize(usize, u32, u32),
}

fn push(history: &mut VecDeque<u32>, value: u32) {
    history.push_back(value);
    if history.len() > HISTORY {
        history.pop_front();
    }
}

use hxport::human_size;

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(':'))).map(|v| v.trim())
}

fn kib(text: &str, key: &str) -> u64 {
    field(text, key).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024
}

fn percent(value: u32) -> String {
    format!("{}.{}%", value / 10, value % 10)
}

fn graph(history: &VecDeque<u32>, size: (u32, u32), color: u32) -> Pixmap {
    let (w, h) = (size.0.max(2), size.1.max(2));
    let mut px = Pixmap::new(w, h);
    let grid = Color::argb(36, 128, 128, 128);
    for g in 1..4 {
        px.fill_rect(0, (h * g / 4) as i32, w as i32, 1, grid);
    }
    let n = history.len();
    if n < 2 {
        return px;
    }
    let step = w as f32 / (HISTORY - 1) as f32;
    let base = w as f32 - (n - 1) as f32 * step;
    let point = |k: usize| ((base + k as f32 * step) as i32, (h as f32 - 2.0 - history[k].min(1000) as f32 / 1000.0 * (h as f32 - 4.0)) as i32);
    let fill = Color((0x28 << 24) | color);
    let stroke = Color(0xff00_0000 | color);
    for k in 1..n {
        let (x0, y0) = point(k - 1);
        let (x1, y1) = point(k);
        for x in x0..x1.max(x0 + 1) {
            let t = if x1 == x0 { 0.0 } else { (x - x0) as f32 / (x1 - x0) as f32 };
            let y = y0 as f32 + (y1 - y0) as f32 * t;
            px.fill_rect(x, y as i32, 1, h as i32 - y as i32, fill);
        }
        px.line(x0, y0, x1, y1, stroke);
        px.line(x0, y0 + 1, x1, y1 + 1, stroke);
    }
    px
}

impl Monitor {
    fn new() -> Monitor {
        let mut m = Monitor { tab: if platform::first_arg().as_deref() == Some("resources") { 1 } else { 0 }, ..Monitor::default() };
        m.users = hxport::user_names().into_iter().collect();
        m.cpu_model = platform::read("/proc/cpuinfo").and_then(|t| t.lines().find(|l| l.starts_with("model name")).map(|l| String::from(l.split(':').nth(1).unwrap_or("").trim()))).unwrap_or_default();
        m.sample();
        m
    }

    fn process(&self, pid: i64) -> Option<(Proc, u64)> {
        let stat = platform::read(&format!("/proc/{}/stat", pid))?;
        let status = platform::read(&format!("/proc/{}/status", pid)).unwrap_or_default();
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let comm = &stat[open + 1..close];
        let f: Vec<&str> = stat[close + 1..].split_whitespace().collect();
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let kernel = (num(6) as u64) & 0x0020_0000 != 0;
        let uid: u32 = field(&status, "Uid").and_then(|v| v.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0);
        let name = field(&status, "Name").map(String::from).unwrap_or_else(|| String::from(comm));
        let cpu_ms = (num(11) + num(12)) as u64 * 10;
        let proc_ = Proc {
            pid,
            name: if kernel { format!("[{}]", name) } else { name },
            user: self.users.get(&uid).cloned().unwrap_or_else(|| format!("{}", uid)),
            state: f.first().and_then(|s| s.chars().next()).unwrap_or('?'),
            memory: kib(&status, "VmRSS"),
            cpu_ms,
            usage: 0,
            core: f.get(36).and_then(|v| v.parse().ok()).unwrap_or(-1),
        };
        Some((proc_, cpu_ms))
    }

    fn sample(&mut self) {
        let mut procs = Vec::new();
        for entry in platform::entries("/proc") {
            let Ok(pid) = entry.parse::<i64>() else {
                continue;
            };
            if let Some((mut p, cpu_ms)) = self.process(pid) {
                let previous = self.last_cpu.iter().find(|(id, _)| *id == pid).map(|(_, c)| *c).unwrap_or(cpu_ms);
                p.usage = (cpu_ms.saturating_sub(previous)).min(1000) as u32;
                procs.push(p);
            }
        }
        self.last_cpu = procs.iter().map(|p| (p.pid, p.cpu_ms)).collect();
        procs.sort_by(|a, b| b.usage.cmp(&a.usage).then(b.memory.cmp(&a.memory)));
        self.procs = procs;

        let meminfo = platform::read("/proc/meminfo").unwrap_or_default();
        let total = kib(&meminfo, "MemTotal");
        let available = if field(&meminfo, "MemAvailable").is_some() { kib(&meminfo, "MemAvailable") } else { kib(&meminfo, "MemFree") + kib(&meminfo, "Cached") };
        self.memory = Memory { total, used: total.saturating_sub(available), cache: kib(&meminfo, "Cached") };
        push(&mut self.mem_history, if total > 0 { (self.memory.used * 1000 / total) as u32 } else { 0 });

        let stat = platform::read("/proc/stat").unwrap_or_default();
        let cores: Vec<(u64, u64)> = stat
            .lines()
            .filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).map(|b| b.is_ascii_digit()).unwrap_or(false))
            .map(|l| {
                let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
                let total: u64 = v.iter().sum();
                let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
                (total - idle.min(total), total)
            })
            .collect();
        if self.core_history.len() != cores.len() {
            self.core_history = (0..cores.len()).map(|_| VecDeque::new()).collect();
            self.last_cores = cores.clone();
        }
        let (mut busy_sum, mut total_sum) = (0u64, 0u64);
        for (i, (busy, total)) in cores.iter().enumerate() {
            let (last_busy, last_total) = self.last_cores[i];
            let (b, t) = (busy.saturating_sub(last_busy), total.saturating_sub(last_total));
            busy_sum += b;
            total_sum += t;
            push(&mut self.core_history[i], if t > 0 { (b * 1000 / t).min(1000) as u32 } else { 0 });
        }
        self.last_cores = cores;
        push(&mut self.cpu_history, if total_sum > 0 { (busy_sum * 1000 / total_sum).min(1000) as u32 } else { 0 });
    }

    fn graph_view(&self, id: usize, history: &VecDeque<u32>, color: u32) -> Element<Msg> {
        let size = self.graphs.get(&id).copied().unwrap_or((200, 60));
        canvas(graph(history, size, color))
            .fill()
            .on_event(move |ev| match ev {
                CanvasEvent::Resize { width, height } => Some(Msg::GraphSize(id, width, height)),
                _ => None,
            })
            .into()
    }

    fn processes(&self) -> Element<Msg> {
        let widths = [60u32, 0, 90, 80, 50, 64, 84];
        let cell = |s: String, i: usize, tone: Tone| -> Element<Msg> {
            let t = text(s).tone(tone);
            if widths[i] == 0 { t.fill().into() } else { t.width(widths[i]).into() }
        };
        let header = row(["PID", "NAME", "USER", "STATE", "CORE", "CPU", "MEMORY"].iter().enumerate().map(|(i, h)| {
            let t = text(*h).small().tone(Tone::Faint);
            if widths[i] == 0 { t.fill().into() } else { t.width(widths[i]).into() }
        }).collect()).padding([0, 14]);
        let rows = self
            .procs
            .iter()
            .map(|p| {
                let kernel = p.name.starts_with('[');
                let tone = if kernel { Tone::Faint } else { Tone::Normal };
                let state = match p.state {
                    'R' if p.core >= 0 => "running",
                    'R' => "ready",
                    'S' => "sleeping",
                    'Z' => "zombie",
                    _ => "sleeping",
                };
                row![
                    cell(format!("{}", p.pid), 0, Tone::Dim),
                    cell(p.name.clone(), 1, tone),
                    cell(p.user.clone(), 2, Tone::Dim),
                    cell(String::from(state), 3, Tone::Dim),
                    cell(if p.core >= 0 { format!("{}", p.core) } else { String::from("–") }, 4, Tone::Dim),
                    cell(percent(p.usage), 5, tone),
                    cell(human_size(p.memory), 6, tone),
                ]
                .into()
            })
            .collect();
        let selected = self.selected.and_then(|pid| self.procs.iter().position(|p| p.pid == pid));
        let can_end = selected.map(|i| !self.procs[i].name.starts_with('[')).unwrap_or(false);
        let footer_text = if self.message.is_empty() { format!("{} processes", self.procs.len()) } else { self.message.clone() };
        column![
            header,
            list(rows).selected(selected).on_select(Msg::Select).row_height(30),
            row![text(footer_text).dim(), space(), button("End process").danger().on_press_maybe(can_end.then_some(Msg::End))],
        ]
        .card()
        .fill()
        .into()
    }

    fn resources(&self) -> Element<Msg> {
        let cores = self.core_history.len().max(1);
        let current = self.cpu_history.back().copied().unwrap_or(0);
        let subtitle = format!("{} · {} {}", self.cpu_model, cores, if cores == 1 { "core" } else { "cores" });
        let cols = if cores <= 2 { cores } else if cores <= 6 { 2 } else if cores <= 12 { 3 } else { 4 };
        let mut grid = column![].spacing(10).fill();
        for chunk in (0..self.core_history.len()).collect::<Vec<_>>().chunks(cols) {
            let mut line = row![].spacing(10).height(Length::Fill).align(Align::Stretch);
            for &core in chunk {
                let color = CORE_COLORS[core % CORE_COLORS.len()];
                let usage = self.core_history[core].back().copied().unwrap_or(0);
                line = line.push(
                    column![row![text(format!("CPU {}", core)).strong(), space(), text(format!("{}%", usage / 10)).strong().tone(Tone::Accent)], self.graph_view(core, &self.core_history[core], color)]
                        .spacing(6)
                        .padding(10)
                        .card()
                        .fill(),
                );
            }
            grid = grid.push(line);
        }
        let mem_current = self.mem_history.back().copied().unwrap_or(0);
        let detail = format!("{} used of {} · file cache {}", human_size(self.memory.used), human_size(self.memory.total), human_size(self.memory.cache));
        column![
            column![row![text("Processor").strong(), text(subtitle).small().tone(Tone::Faint), space(), text(percent(current)).size(TextSize::Title).tone(Tone::Accent)], grid].card().fill(),
            column![row![text("Memory").strong(), text(detail).small().tone(Tone::Faint), space(), text(percent(mem_current)).size(TextSize::Title).tone(Tone::Success)], self.graph_view(MEMORY_GRAPH, &self.mem_history, MEMORY_COLOR)]
                .card()
                .height(160),
        ]
        .spacing(12)
        .fill()
        .into()
    }

    fn disks(&self) -> Element<Msg> {
        let mut volumes = column![title("Mounted volumes").size(TextSize::Strong)].spacing(10);
        for line in platform::read("/proc/mounts").unwrap_or_default().lines() {
            let tabbed: Vec<&str> = line.split('\t').collect();
            let (source, target, sizes) = if tabbed.len() >= 6 {
                (tabbed[0], tabbed[1], Some((tabbed[4].parse::<u64>().unwrap_or(0) * 1024, tabbed[5].parse::<u64>().unwrap_or(0) * 1024)))
            } else {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 3 || !f[0].starts_with('/') {
                    continue;
                }
                (f[0], f[1], None)
            };
            let mut entry = column![row![text(format!("{}  on  {}", source, target)), space()]].spacing(6);
            if let Some((total, free)) = sizes {
                let used = total.saturating_sub(free);
                entry = column![
                    row![text(format!("{}  on  {}", source, target)), space(), text(format!("{} used of {}", human_size(used), human_size(total))).dim()],
                    hxtoolkit::progress(if total > 0 { used as f32 / total as f32 } else { 0.0 }),
                ]
                .spacing(6);
            }
            volumes = volumes.push(entry);
        }
        let mut disks = column![title("Disks").size(TextSize::Strong)].spacing(6);
        let mut names = platform::entries("/sys/block");
        names.sort();
        for disk in names {
            let size = platform::read(&format!("/sys/block/{}/size", disk)).and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0) * 512;
            let model = platform::read(&format!("/sys/block/{}/device/model", disk)).map(|s| String::from(s.trim())).unwrap_or_default();
            disks = disks.push(text(format!("{}   {}   {}", disk, human_size(size), model)).dim());
        }
        scroll(column![volumes, disks].spacing(24).card()).into()
    }
}

impl App for Monitor {
    type Message = Msg;

    fn title(&self) -> String {
        String::from("Monitor")
    }

    fn view(&self) -> Element<Msg> {
        let mut tabs = row![].spacing(6).shrink();
        for (i, label) in ["Processes", "Resources", "Disks"].iter().enumerate() {
            let b = button(*label).on_press(Msg::Tab(i));
            tabs = tabs.push(if self.tab == i { b } else { b.flat() });
        }
        let summary = format!(
            "CPU {}% · {} cores   ·   Memory {} / {}",
            self.cpu_history.back().copied().unwrap_or(0) / 10,
            self.core_history.len().max(1),
            human_size(self.memory.used),
            human_size(self.memory.total)
        );
        let body = match self.tab {
            0 => self.processes(),
            1 => self.resources(),
            _ => self.disks(),
        };
        column![row![tabs, space(), text(summary).dim()], body].padding(16).spacing(12).fill().into()
    }

    fn update(&mut self, message: Msg, _cx: &mut Context<Msg>) {
        match message {
            Msg::Tab(i) => self.tab = i,
            Msg::Select(i) => self.selected = self.procs.get(i).map(|p| p.pid),
            Msg::End => {
                if let Some(pid) = self.selected {
                    self.message = match platform::kill(pid) {
                        Ok(()) => format!("Ended process {}", pid),
                        Err(e) => format!("Cannot end {}: {}", pid, e),
                    };
                    self.sample();
                }
            }
            Msg::Sample => self.sample(),
            Msg::GraphSize(id, w, h) => {
                self.graphs.insert(id, (w, h));
            }
        }
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        match press.key {
            Key::Char(c @ '1'..='3') if press.mods.alt => Some(Msg::Tab(c as usize - '1' as usize)),
            _ => None,
        }
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        Some((1000, Msg::Sample))
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("monitor").size(820, 580).min_size(520, 380), Monitor::new())
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
