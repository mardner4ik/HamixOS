#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, eprintln, fs, sys, users};
use hxclient::ui::{self, theme, Hits, Style, Ui};
use hxclient::{Event, Window, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_WHEEL};
use vellum::{Area, Painter};

const INITIAL_W: i32 = 820;
const INITIAL_H: i32 = 580;
const ROW: i32 = 30;
const HISTORY: usize = 120;
const CORE_COLORS: [u32; 8] = [0x5b8cff, 0x2dd4bf, 0xf59e0b, 0xf472b6, 0xa78bfa, 0x34d399, 0xfb7185, 0x60a5fa];

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Tab(usize),
    Row(usize),
    End,
}

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

struct App {
    window: Window,
    ui: Ui,
    tab: usize,
    procs: Vec<Proc>,
    selected: Option<i64>,
    scroll: i32,
    hits: Hits<Hit>,
    hover: Option<Hit>,
    cpu_history: VecDeque<u32>,
    core_history: Vec<VecDeque<u32>>,
    mem_history: VecDeque<u32>,
    last_cpu: Vec<(i64, u64)>,
    last_cores: Vec<(u64, u64)>,
    last_sample: u64,
    info: sys::SysInfo,
    message: String,
}

fn push(history: &mut VecDeque<u32>, value: u32) {
    history.push_back(value);
    if history.len() > HISTORY {
        history.pop_front();
    }
}

fn graph(p: &mut Painter, area: Area, history: &VecDeque<u32>, color: u32) {
    p.rounded(area, 6, theme::bg(), 255);
    for g in 1..4 {
        p.fill(Area::new(area.x + 1, area.y + area.h * g / 4, area.w - 2, 1), theme::surface_2());
    }
    let n = history.len();
    if n < 2 {
        return;
    }
    let step = area.w as f32 / (HISTORY - 1) as f32;
    let base_x = area.right() as f32 - (n - 1) as f32 * step;
    let point = |k: usize| (base_x + k as f32 * step, area.bottom() as f32 - 2.0 - history[k].min(1000) as f32 / 1000.0 * (area.h - 4) as f32);
    let saved = p.clip;
    p.set_clip(area.intersect(&saved));
    for k in 1..n {
        let (x0, y0) = point(k - 1);
        let (x1, y1) = point(k);
        let top = y0.min(y1) as i32;
        let fill = Area::new(x0 as i32, top, (x1 as i32 - x0 as i32).max(1), area.bottom() - top);
        p.blend_fill(fill.intersect(&area), color, 40);
        p.line(x0, y0, x1, y1, if area.h > 80 { 2.0 } else { 1.5 }, color, 255);
    }
    p.clip = saved;
}

impl App {
    fn size(&self) -> (i32, i32) {
        (self.window.width() as i32, self.window.height() as i32)
    }

    fn sample(&mut self) {
        let now = sys::uptime_ms();
        let elapsed = now.saturating_sub(self.last_sample).max(1);
        let list = sys::proc_list();
        let mut procs = Vec::new();
        for p in list {
            let previous = self.last_cpu.iter().find(|(pid, _)| *pid == p.pid).map(|(_, c)| *c).unwrap_or(p.cpu_ms);
            let delta = p.cpu_ms.saturating_sub(previous);
            procs.push(Proc {
                pid: p.pid,
                name: p.name,
                user: users::name_of(p.uid),
                state: p.state,
                memory: p.heap_kb * 1024,
                cpu_ms: p.cpu_ms,
                usage: (delta * 1000 / elapsed).min(1000) as u32,
                core: p.cpu,
            });
        }
        self.last_cpu = procs.iter().map(|p| (p.pid, p.cpu_ms)).collect();
        self.last_sample = now;
        procs.sort_by(|a, b| b.usage.cmp(&a.usage).then(b.memory.cmp(&a.memory)));
        self.procs = procs;
        self.info = sys::sysinfo();

        let cores = sys::cpu_stats();
        if self.core_history.len() != cores.len() {
            self.core_history = (0..cores.len()).map(|_| VecDeque::new()).collect();
            self.last_cores = cores.iter().map(|c| (c.busy_ms, c.total_ms)).collect();
        }
        let mut busy_sum = 0u64;
        let mut total_sum = 0u64;
        for (i, core) in cores.iter().enumerate() {
            let (last_busy, last_total) = self.last_cores[i];
            let busy = core.busy_ms.saturating_sub(last_busy);
            let total = core.total_ms.saturating_sub(last_total);
            busy_sum += busy;
            total_sum += total;
            let value = if total > 0 { (busy * 1000 / total).min(1000) as u32 } else { 0 };
            push(&mut self.core_history[i], value);
            self.last_cores[i] = (core.busy_ms, core.total_ms);
        }
        let cpu = if total_sum > 0 { (busy_sum * 1000 / total_sum).min(1000) as u32 } else { 0 };
        push(&mut self.cpu_history, cpu);
        let mem = if self.info.mem_total > 0 { (self.info.mem_used() * 1000 / self.info.mem_total) as u32 } else { 0 };
        push(&mut self.mem_history, mem);
    }

    fn draw(&mut self) {
        let (w, h) = self.size();
        let buffer = unsafe { &mut *(self.window.buffer() as *mut [u32]) };
        let mut p = Painter::new(buffer, w, h);
        self.hits.clear();
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hits = unsafe { &mut *(&mut self.hits as *mut Hits<Hit>) };
        let hover = self.hover;
        p.fill(Area::new(0, 0, w, h), theme::bg());
        let tabs = ["Processes", "Resources", "Disks"];
        let mut x = 16;
        for (i, label) in tabs.iter().enumerate() {
            let tw = ui.medium.measure(label) + 28;
            let area = Area::new(x, 12, tw, 32);
            let active = self.tab == i;
            if active {
                p.rounded(area, 8, theme::surface_2(), 255);
            } else if hover == Some(Hit::Tab(i)) {
                p.rounded(area, 8, theme::hover(), 255);
            }
            ui::centered_text(&mut p, &ui.medium, area, label, if active { theme::text() } else { theme::dim() });
            hits.add(area, Hit::Tab(i));
            x += tw + 6;
        }
        let used = self.info.mem_used();
        let summary = format!(
            "CPU {}% · {} cores   ·   Memory {} / {}",
            self.cpu_history.back().copied().unwrap_or(0) / 10,
            self.core_history.len().max(1),
            ui::human_size(used),
            ui::human_size(self.info.mem_total)
        );
        let sw = ui.font.measure(&summary);
        if x + sw + 30 < w {
            p.text(&ui.font, w - sw - 18, 20, &summary, theme::dim());
        }
        let body = Area::new(16, 56, w - 32, h - 72);
        match self.tab {
            0 => self.draw_processes(&mut p, body),
            1 => self.draw_resources(&mut p, body),
            _ => draw_disks(&mut p, &self.ui, body),
        }
        drop(p);
        self.window.present();
    }

    fn draw_processes(&mut self, p: &mut Painter, body: Area) {
        let ui = unsafe { &*(&self.ui as *const Ui) };
        let hover = self.hover;
        let hits = unsafe { &mut *(&mut self.hits as *mut Hits<Hit>) };
        ui::card(p, body);
        let header_y = body.y + 12;
        let wide = body.w >= 700;
        let name_w = (body.w - 480).clamp(120, 360);
        let col_user = body.x + 76 + name_w;
        let col_state = col_user + 90;
        let col_core = col_state + 80;
        let col_cpu = if wide { col_core + 60 } else { col_core };
        let col_mem = col_cpu + 70;
        let mut cols = alloc::vec![(body.x + 16, "PID"), (body.x + 76, "NAME"), (col_user, "USER"), (col_state, "STATE")];
        if wide {
            cols.push((col_core, "CORE"));
        }
        cols.push((col_cpu, "CPU"));
        cols.push((col_mem, "MEMORY"));
        for (cx, label) in cols {
            p.text(&ui.small, cx, header_y, label, theme::faint());
        }
        let list = Area::new(body.x, body.y + 34, body.w, body.h - 90);
        let saved = p.clip;
        p.set_clip(list);
        for (i, proc_) in self.procs.iter().enumerate() {
            let row = Area::new(list.x + 6, list.y + i as i32 * ROW - self.scroll, list.w - 12, ROW - 2);
            if row.bottom() < list.y || row.y > list.bottom() {
                continue;
            }
            ui::list_row(p, row, self.selected == Some(proc_.pid), hover == Some(Hit::Row(i)));
            let kernel = proc_.name.starts_with('[');
            let color = if kernel { theme::faint() } else { theme::text() };
            ui::text_in(p, &ui.font, body.x + 16, row, &format!("{}", proc_.pid), theme::dim());
            ui::text_in(p, &ui.font, body.x + 76, Area::new(row.x, row.y, 76 + name_w - 10, row.h), &proc_.name, color);
            ui::text_in(p, &ui.font, col_user, row, &proc_.user, theme::dim());
            let state = match proc_.state {
                'R' if proc_.core >= 0 => "running",
                'R' => "ready",
                'S' => "sleeping",
                _ => "zombie",
            };
            ui::text_in(p, &ui.font, col_state, row, state, theme::dim());
            if wide {
                ui::text_in(p, &ui.font, col_core, row, &if proc_.core >= 0 { format!("{}", proc_.core) } else { String::from("–") }, theme::dim());
            }
            ui::text_in(p, &ui.font, col_cpu, row, &format!("{}.{}%", proc_.usage / 10, proc_.usage % 10), color);
            ui::text_in(p, &ui.font, col_mem, row, &ui::human_size(proc_.memory), color);
            hits.add(row.intersect(&list), Hit::Row(i));
        }
        p.clip = saved;
        let end = Area::new(body.right() - 136, body.bottom() - 46, 120, 34);
        let can_end = self.selected.map(|pid| self.procs.iter().any(|p| p.pid == pid && !p.name.starts_with('['))).unwrap_or(false);
        ui::button(p, ui, end, "End process", Style::Danger, hover == Some(Hit::End), can_end);
        if can_end {
            hits.add(end, Hit::End);
        }
        let text = if self.message.is_empty() { format!("{} processes", self.procs.len()) } else { self.message.clone() };
        ui::text_in(p, &ui.font, body.x + 16, Area::new(body.x, end.y, end.x - body.x - 8, end.h), &text, theme::dim());
    }

    fn draw_resources(&self, p: &mut Painter, body: Area) {
        let ui = &self.ui;
        let cores = self.core_history.len().max(1);
        let memory_h = (body.h / 4).clamp(110, 170);
        let cpu_card = Area::new(body.x, body.y, body.w, body.h - memory_h - 12);
        ui::card(p, cpu_card);
        let current = self.cpu_history.back().copied().unwrap_or(0);
        p.text(&ui.medium, cpu_card.x + 18, cpu_card.y + 14, "Processor", theme::text());
        let brand = fs::read_to_string("/proc/cpuinfo").and_then(|t| t.lines().find(|l| l.starts_with("model name")).map(|l| String::from(l.split(':').nth(1).unwrap_or("").trim()))).unwrap_or_default();
        let subtitle = format!("{} · {} {}", brand, cores, if cores == 1 { "core" } else { "cores" });
        let value = format!("{}.{}%", current / 10, current % 10);
        let vw = ui.title.measure(&value);
        p.text(&ui.title, cpu_card.right() - vw - 18, cpu_card.y + 10, &value, theme::accent());
        ui::text_in(p, &ui.small, cpu_card.x + 110, Area::new(cpu_card.x, cpu_card.y + 14, cpu_card.w - vw - 140, 18), &subtitle, theme::faint());

        let grid = Area::new(cpu_card.x + 14, cpu_card.y + 44, cpu_card.w - 28, cpu_card.h - 56);
        let gap = 10;
        let min_cell_w = 170;
        let max_cols = ((grid.w + gap) / (min_cell_w + gap)).max(1) as usize;
        let mut cols = 1usize;
        let mut best = f32::MAX;
        for candidate in 1..=cores.min(max_cols) {
            let rows = cores.div_ceil(candidate);
            let cell_h = ((grid.h - gap * (rows as i32 - 1)) / rows as i32).max(1);
            let cell_w = (grid.w - gap * (candidate as i32 - 1)) / candidate as i32;
            let ratio = cell_w as f32 / cell_h as f32;
            let score = if ratio > 2.6 { ratio / 2.6 } else { 2.6 / ratio };
            let wasted = (rows * candidate - cores) as f32 * 0.35;
            if score + wasted < best {
                best = score + wasted;
                cols = candidate;
            }
        }
        let rows = cores.div_ceil(cols);
        let cell_w = (grid.w - gap * (cols as i32 - 1)) / cols as i32;
        let cell_h = (grid.h - gap * (rows as i32 - 1)) / rows as i32;
        for core in 0..cores {
            let col = (core % cols) as i32;
            let row = (core / cols) as i32;
            let cell = Area::new(grid.x + col * (cell_w + gap), grid.y + row * (cell_h + gap), cell_w, cell_h);
            p.rounded(cell, 8, theme::surface_2(), 255);
            let color = CORE_COLORS[core % CORE_COLORS.len()];
            let history = self.core_history.get(core);
            let usage = history.and_then(|h| h.back().copied()).unwrap_or(0);
            p.circle(cell.x as f32 + 14.0, cell.y as f32 + 15.0, 4.0, color, 255);
            p.text(&ui.medium, cell.x + 24, cell.y + 6, &format!("CPU {}", core), theme::text());
            let label = format!("{}%", usage / 10);
            let lw = ui.medium.measure(&label);
            p.text(&ui.medium, cell.right() - lw - 10, cell.y + 6, &label, color);
            let area = Area::new(cell.x + 8, cell.y + 28, cell.w - 16, cell.h - 36);
            if area.h > 8 {
                match history {
                    Some(h) => graph(p, area, h, color),
                    None => graph(p, area, &VecDeque::new(), color),
                }
            }
        }

        let mem_card = Area::new(body.x, cpu_card.bottom() + 12, body.w, memory_h);
        ui::card(p, mem_card);
        let used = self.info.mem_used();
        p.text(&ui.medium, mem_card.x + 18, mem_card.y + 14, "Memory", theme::text());
        let detail = format!("{} used of {} · file cache {} · kernel heap {}", ui::human_size(used), ui::human_size(self.info.mem_total), ui::human_size(self.info.file_cache), ui::human_size(self.info.heap_total - self.info.heap_free));
        ui::text_in(p, &ui.small, mem_card.x + 100, Area::new(mem_card.x, mem_card.y + 14, mem_card.w - 200, 18), &detail, theme::faint());
        let current = self.mem_history.back().copied().unwrap_or(0);
        let value = format!("{}.{}%", current / 10, current % 10);
        let vw = ui.title.measure(&value);
        p.text(&ui.title, mem_card.right() - vw - 18, mem_card.y + 10, &value, 0x2dd4bf);
        graph(p, Area::new(mem_card.x + 18, mem_card.y + 46, mem_card.w - 36, mem_card.h - 60), &self.mem_history, 0x2dd4bf);
    }
}

fn draw_disks(p: &mut Painter, ui: &Ui, body: Area) {
    ui::card(p, body);
    let mut y = body.y + 16;
    p.text(&ui.medium, body.x + 18, y, "Mounted volumes", theme::text());
    y += 30;
    for line in fs::read_to_string("/proc/mounts").unwrap_or_default().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let total: u64 = f[4].parse().unwrap_or(0) * 1024;
        let free: u64 = f[5].parse().unwrap_or(0) * 1024;
        let used = total.saturating_sub(free);
        ui::symbol(p, ui, "ui/disk", body.x + 18, y + 2, theme::dim());
        p.text(&ui.font, body.x + 44, y, &format!("{}  on  {}", f[0], f[1]), theme::text());
        let detail = format!("{} used of {}", ui::human_size(used), ui::human_size(total));
        let dw = ui.font.measure(&detail);
        p.text(&ui.font, body.right() - dw - 18, y, &detail, theme::dim());
        ui::progress(p, Area::new(body.x + 44, y + 24, body.w - 62, 8), if total > 0 { (used * 1000 / total) as u32 } else { 0 }, theme::accent());
        y += 54;
    }
    y += 10;
    p.text(&ui.medium, body.x + 18, y, "Disks", theme::text());
    y += 30;
    for line in sys::disk_listing().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let text = match f.first().copied() {
            Some("disk") if f.len() > 4 => format!("{}   {}   {}", f[1], ui::human_size(f[2].parse::<u64>().unwrap_or(0) * 512), f[4]),
            Some("part") if f.len() > 6 => format!("    {}   {}   {}", f[1], ui::human_size(f[4].parse::<u64>().unwrap_or(0) * 512), f[6]),
            _ => continue,
        };
        p.text(&ui.font, body.x + 18, y, &text, theme::dim());
        y += 22;
        if y > body.bottom() - 20 {
            break;
        }
    }
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&["ui/disk"]);
    let window = match Window::open("Monitor", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxmon: {}", e);
            return 1;
        }
    };
    let mut app = App {
        window,
        ui,
        tab: if hamix_std::env::args().get(1).map(|a| a == "resources").unwrap_or(false) { 1 } else { 0 },
        procs: Vec::new(),
        selected: None,
        scroll: 0,
        hits: Hits::new(),
        hover: None,
        cpu_history: VecDeque::new(),
        core_history: Vec::new(),
        mem_history: VecDeque::new(),
        last_cpu: Vec::new(),
        last_cores: Vec::new(),
        last_sample: sys::uptime_ms(),
        info: sys::sysinfo(),
        message: String::new(),
    };
    app.window.set_icon("monitor");
    app.window.set_min_size(520, 380);
    app.sample();
    app.draw();
    let mut next = sys::uptime_ms() + 1000;
    loop {
        let wait = next.saturating_sub(sys::uptime_ms()) as i64;
        match app.window.wait_event(wait.max(1)) {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Resize { .. }) | Some(Event::Theme { .. }) => app.draw(),
            Some(Event::Mouse { x, y, kind, wheel, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    let hover = if kind == MOUSE_LEAVE { None } else { app.hits.at(x, y) };
                    if hover != app.hover {
                        app.hover = hover;
                        app.draw();
                    }
                }
                MOUSE_WHEEL => {
                    let (_, h) = app.size();
                    let max = (app.procs.len() as i32 * ROW - (h - 162)).max(0);
                    app.scroll = (app.scroll + wheel * ROW * 2).clamp(0, max);
                    app.draw();
                }
                MOUSE_PRESS => {
                    match app.hits.at(x, y) {
                        Some(Hit::Tab(i)) => app.tab = i,
                        Some(Hit::Row(i)) => app.selected = app.procs.get(i).map(|p| p.pid),
                        Some(Hit::End) => {
                            if let Some(pid) = app.selected {
                                let r = sys::kill(pid);
                                app.message = if r < 0 { format!("Cannot end {}: {}", pid, sys::error_name(r)) } else { format!("Ended process {}", pid) };
                            }
                        }
                        None => {}
                    }
                    app.draw();
                }
                _ => {}
            },
            Some(Event::Key { code: 27, .. }) => return 0,
            _ => {}
        }
        if sys::uptime_ms() >= next {
            next = sys::uptime_ms() + 1000;
            app.sample();
            app.draw();
        }
    }
}

entry!(main);
