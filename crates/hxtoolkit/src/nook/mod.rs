mod editor;
mod field;
mod theme;
mod transport;
mod wayland;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use hxproto::client::{posix, Surface};
use transport::Transport;
use hxproto::Event;
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

use crate::app::{shortcut_message, App, Command, Context, FileDialog, MenuItem, Reply, Settings};
use crate::element::{CanvasEvent, Element, ImageSource, MouseButton};
use crate::keys::{Key, KeyPress, Modifiers};
use crate::layout::{self, Metrics, Node, Rect, Sizes};
use crate::style::{Align, TextSize};

use editor::{EditorState, Look};
use field::{Field, FieldAction};
use theme::Theme;

pub fn available() -> bool {
    hxproto::client::server_running()
}

pub fn wayland_available() -> bool {
    wayland::available()
}

pub(crate) fn env(name: &str) -> Option<String> {
    posix::env(name)
}

pub(crate) fn report(text: &str) {
    posix::write_stderr(text);
    posix::write_stderr("\n");
}

type Path = Vec<u16>;

const HEADER: i32 = 44;
const MENUBAR: i32 = 36;
const MENU_ROW: i32 = 30;
const MENU_SEPARATOR: i32 = 9;
const DOUBLE_CLICK_MS: u64 = 400;
const CURSOR_ARROW: u32 = 0;
const CURSOR_TEXT: u32 = 1;
const MOUSE_MOVE: u32 = 0;
const MOUSE_PRESS: u32 = 1;
const MOUSE_RELEASE: u32 = 2;
const MOUSE_WHEEL: u32 = 3;
const MOUSE_LEAVE: u32 = 4;

struct Measure<'a> {
    th: &'a Theme,
    sizes: &'a Sizes,
}

impl Metrics for Measure<'_> {
    fn text_width(&self, text: &str, size: TextSize) -> i32 {
        self.th.fonts.get(size).measure(text)
    }

    fn line_height(&self, size: TextSize) -> i32 {
        self.th.fonts.get(size).height() + 2
    }

    fn image_size(&self, source: &ImageSource) -> (i32, i32) {
        match source {
            ImageSource::Pixels(p) => (p.width() as i32, p.height() as i32),
            ImageSource::File(path) => self.th.image(path).map(|i| (i.w, i.h)).unwrap_or((0, 0)),
            ImageSource::Icon(name) => self.th.image(&Theme::icon_path(name)).map(|i| (i.w, i.h)).unwrap_or((16, 16)),
        }
    }

    fn sizes(&self) -> &Sizes {
        self.sizes
    }
}

fn area(r: Rect) -> Area {
    Area::new(r.x, r.y, r.w, r.h)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Button,
    Text,
    Input,
    Editor,
    Check,
    Switch,
    Slider,
    Dropdown,
    Row(usize),
    ListBlank,
    Canvas,
}

enum PopupKind<M> {
    Dropdown(Path),
    Menu(Option<usize>),
    Context(Vec<MenuItem<M>>),
    Edit(Path, bool),
}

struct Popup<M> {
    kind: PopupKind<M>,
    origin: (i32, i32),
    size: (i32, i32),
    rows: Vec<(Rect, Option<usize>)>,
    hover: Option<usize>,
    surface: Option<Surface>,
    opened: u64,
}

struct Entry {
    label: String,
    detail: String,
    enabled: bool,
    checked: Option<bool>,
    separator: bool,
}

const EDIT_ITEMS: [(&str, &str); 5] = [("Cut", "Ctrl+X"), ("Copy", "Ctrl+C"), ("Paste", "Ctrl+V"), ("", ""), ("Select all", "Ctrl+A")];
const POPUP_RADIUS: i32 = 8;

fn round_corners(buf: &mut [u32], w: i32, h: i32, r: i32) {
    for px in buf.iter_mut() {
        *px |= 0xff00_0000;
    }
    let r = r.min(w / 2).min(h / 2);
    for y in 0..r {
        for x in 0..r {
            let dx = r as f32 - x as f32 - 0.5;
            let dy = r as f32 - y as f32 - 0.5;
            let coverage = (r as f32 - vellum::gfx::fast_sqrt(dx * dx + dy * dy) + 0.5).clamp(0.0, 1.0);
            if coverage >= 1.0 {
                continue;
            }
            let a = (coverage * 255.0) as u32;
            for (cx, cy) in [(x, y), (w - 1 - x, y), (x, h - 1 - y), (w - 1 - x, h - 1 - y)] {
                let i = (cy * w + cx) as usize;
                let p = buf[i];
                let ch = |shift: u32| (((p >> shift) & 0xff) * a / 255) << shift;
                buf[i] = (a << 24) | ch(16) | ch(8) | ch(0);
            }
        }
    }
}

struct Modal<M> {
    title: String,
    text: String,
    reply: Option<Reply<bool, M>>,
    ok: Rect,
    cancel: Rect,
    hover: u8,
}

enum Pending<M> {
    Field(Path),
    Editor(Path),
    App(Reply<Option<String>, M>),
}

#[derive(Default)]
struct Interaction {
    hover: Option<(Path, Hit)>,
    pressed: Option<Path>,
    focus: Option<Path>,
    fields: BTreeMap<Path, Field>,
    editors: BTreeMap<Path, EditorState>,
    scrolls: BTreeMap<Path, i32>,
    drag: Option<Path>,
    select_drag: Option<Path>,
    text_drag: Option<(Path, usize)>,
    text_selection: Option<(Path, usize, usize)>,
    canvas_sizes: BTreeMap<Path, (u32, u32)>,
    last_click: (u64, Option<(Path, usize)>),
    button: Option<MouseButton>,
    menu_hover: Option<usize>,
    autofocus: Option<(Path, String)>,
}

fn child_node<'a, M>(e: &Element<M>, n: &'a Node, i: usize) -> Option<&'a Node> {
    match e {
        Element::List(_) => n.children.get(i).and_then(|row| row.children.first()),
        _ => n.children.get(i),
    }
}

fn scroll_of(ui: &Interaction, path: &Path) -> i32 {
    ui.scrolls.get(path).copied().unwrap_or(0)
}

fn find<'a, M>(e: &'a Element<M>, n: &'a Node, path: &[u16], ui: &Interaction, prefix: &mut Path, dy: i32) -> Option<(&'a Element<M>, &'a Node, i32)> {
    let Some((&first, rest)) = path.split_first() else {
        return Some((e, n, dy));
    };
    let inner_dy = match e {
        Element::Scroll(_) | Element::List(_) => dy + scroll_of(ui, prefix),
        _ => dy,
    };
    let child = e.children().get(first as usize)?;
    let node = child_node(e, n, first as usize)?;
    prefix.push(first);
    find(child, node, rest, ui, prefix, inner_dy)
}

fn locate<'a, M>(view: &'a Element<M>, tree: &'a Node, path: &[u16], ui: &Interaction) -> Option<(&'a Element<M>, &'a Node, i32)> {
    let mut prefix = Vec::new();
    find(view, tree, path, ui, &mut prefix, 0)
}

fn hit<M>(e: &Element<M>, n: &Node, path: &mut Path, x: i32, y: i32, dy: i32, ui: &Interaction) -> Option<(Path, Hit)> {
    let r = n.rect.offset(0, -dy);
    if !r.contains(x, y) {
        return None;
    }
    match e {
        Element::Stack(s) => {
            for (i, child) in s.children.iter().enumerate() {
                path.push(i as u16);
                let found = child_node(e, n, i).and_then(|cn| hit(child, cn, path, x, y, dy, ui));
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        Element::Scroll(sc) => {
            let inner = dy + scroll_of(ui, path);
            path.push(0);
            let found = n.children.first().and_then(|cn| hit(&sc.child, cn, path, x, y, inner, ui));
            path.pop();
            found
        }
        Element::List(l) => {
            let inner = dy + scroll_of(ui, path);
            for (i, row) in l.rows.iter().enumerate() {
                let Some(row_node) = n.children.get(i) else {
                    continue;
                };
                if !row_node.rect.offset(0, -inner).contains(x, y) {
                    continue;
                }
                path.push(i as u16);
                let found = row_node.children.first().and_then(|cn| hit(row, cn, path, x, y, inner, ui));
                path.pop();
                if let Some(found) = found {
                    if !matches!(found.1, Hit::Row(_)) {
                        return Some(found);
                    }
                }
                return Some((path.clone(), Hit::Row(i)));
            }
            l.on_context.as_ref().map(|_| (path.clone(), Hit::ListBlank))
        }
        Element::Button(b) if b.on_press.is_some() => Some((path.clone(), Hit::Button)),
        Element::Text(t) if t.selectable => Some((path.clone(), Hit::Text)),
        Element::Input(_) => Some((path.clone(), Hit::Input)),
        Element::Editor(_) => Some((path.clone(), Hit::Editor)),
        Element::Checkbox(c) if c.on_toggle.is_some() => Some((path.clone(), Hit::Check)),
        Element::Switch(s) if s.on_toggle.is_some() => Some((path.clone(), Hit::Switch)),
        Element::Slider(s) if s.on_change.is_some() => Some((path.clone(), Hit::Slider)),
        Element::Dropdown(d) if d.on_select.is_some() => Some((path.clone(), Hit::Dropdown)),
        Element::Canvas(c) if c.on_event.is_some() => Some((path.clone(), Hit::Canvas)),
        _ => None,
    }
}

fn scrollable<M>(e: &Element<M>, n: &Node, path: &mut Path, x: i32, y: i32, dy: i32, ui: &Interaction) -> Option<Path> {
    let r = n.rect.offset(0, -dy);
    if !r.contains(x, y) {
        return None;
    }
    let inner = match e {
        Element::Scroll(_) | Element::List(_) => dy + scroll_of(ui, path),
        _ => dy,
    };
    for (i, child) in e.children().iter().enumerate() {
        if let Some(cn) = child_node(e, n, i) {
            path.push(i as u16);
            let found = scrollable(child, cn, path, x, y, inner, ui);
            path.pop();
            if found.is_some() {
                return found;
            }
        }
    }
    match e {
        Element::Scroll(_) | Element::List(_) | Element::Editor(_) => Some(path.clone()),
        _ => None,
    }
}

fn collect_focusable<M>(e: &Element<M>, path: &mut Path, out: &mut Vec<Path>) {
    if matches!(e, Element::Input(_) | Element::Editor(_)) {
        out.push(path.clone());
    }
    for (i, child) in e.children().iter().enumerate() {
        path.push(i as u16);
        collect_focusable(child, path, out);
        path.pop();
    }
}

fn autofocus<M>(e: &Element<M>, path: &mut Path) -> Option<(Path, String)> {
    if let Element::Input(i) = e {
        if i.focus {
            return Some((path.clone(), alloc::format!("{}|{}", i.placeholder, i.secret)));
        }
    }
    for (i, child) in e.children().iter().enumerate() {
        path.push(i as u16);
        let found = autofocus(child, path);
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

fn preload<M>(e: &Element<M>, th: &mut Theme) {
    match e {
        Element::Image(i) => match &i.source {
            ImageSource::File(path) => th.preload(path),
            ImageSource::Icon(name) => th.preload(&Theme::icon_path(name)),
            ImageSource::Pixels(_) => {}
        },
        Element::Button(b) => {
            if let Some(icon) = &b.icon {
                th.preload(&Theme::icon_path(icon));
            }
        }
        _ => {}
    }
    for child in e.children() {
        preload(child, th);
    }
}

fn canvas_resizes<M: Clone>(e: &Element<M>, n: &Node, path: &mut Path, ui: &mut Interaction, out: &mut Vec<M>) {
    if let Element::Canvas(c) = e {
        let size = (n.rect.w.max(0) as u32, n.rect.h.max(0) as u32);
        if ui.canvas_sizes.get(path) != Some(&size) {
            ui.canvas_sizes.insert(path.clone(), size);
            if let Some(cb) = &c.on_event {
                if let Some(m) = cb(CanvasEvent::Resize { width: size.0, height: size.1 }) {
                    out.push(m);
                }
            }
        }
    }
    for (i, child) in e.children().iter().enumerate() {
        if let Some(cn) = child_node(e, n, i) {
            path.push(i as u16);
            canvas_resizes(child, cn, path, ui, out);
            path.pop();
        }
    }
}

struct Painting<'a, 'b> {
    p: &'a mut Painter<'b>,
    th: &'a Theme,
    sizes: &'a Sizes,
    ui: &'a mut Interaction,
}

impl Painting<'_, '_> {
    fn hovered(&self, path: &Path) -> bool {
        self.ui.hover.as_ref().map(|(p, _)| p == path).unwrap_or(false)
    }

    fn focused(&self, path: &Path) -> bool {
        self.ui.focus.as_ref() == Some(path)
    }

    fn element<M>(&mut self, e: &Element<M>, n: &Node, path: &mut Path, dy: i32, clip: Rect, inverted: bool) {
        let r = n.rect.offset(0, -dy);
        if r.intersect(&clip).is_empty() {
            return;
        }
        let a = area(r);
        let hover = self.hovered(path);
        match e {
            Element::Text(t) => {
                let font = if let Some(ref family) = t.font_family {
                    self.th.fonts.get_by_name(family).unwrap_or_else(|| self.th.fonts.get(t.size))
                } else {
                    self.th.fonts.get(t.size)
                };
                let color = t.color.unwrap_or_else(|| self.th.tone(t.tone, inverted));
                let lh = font.height() + 2;
                let lines: Vec<&str> = if t.wrap { layout::wrap_lines(&t.content, r.w, |s| font.measure(s)) } else { t.content.split('\n').collect() };
                let mut y = r.y + 1;
                let mut source_offset = 0;
                let selection = self.ui.text_selection.as_ref().filter(|(selected_path, _, _)| selected_path == path).map(|(_, start, end)| (*start, *end));
                for line in lines {
                    let shown = if t.wrap { String::from(line) } else { ellipsize(font, line, r.w) };
                    let w = font.measure(&shown);
                    let x = match t.align {
                        Align::Center => r.x + (r.w - w) / 2,
                        Align::End => r.right() - w,
                        _ => r.x,
                    };
                    if let Some(bg) = t.bg {
                        self.p.fill(Area::new(x - 2, y - 1, w + 4, lh), bg);
                    }
                    let line_start = source_offset + t.content[source_offset..].find(line).unwrap_or(0);
                    let line_char_start = t.content[..line_start].chars().count();
                    let line_char_count = shown.chars().count();
                    let selected_start = selection.map(|(start, _)| start.saturating_sub(line_char_start).min(line_char_count)).unwrap_or(line_char_count);
                    let selected_end = selection.map(|(_, end)| end.saturating_sub(line_char_start).min(line_char_count)).unwrap_or(0);
                    if selected_end > selected_start {
                        let chars: Vec<char> = shown.chars().collect();
                        let prefix: String = chars.iter().take(selected_start).collect();
                        let selected: String = chars.iter().take(selected_end).skip(selected_start).collect();
                        let suffix: String = chars.iter().skip(selected_end).collect();
                        let prefix_width = font.measure(&prefix);
                        let selected_width = font.measure(&selected);
                        self.p.text(font, x, y, &prefix, color);
                        self.p.fill(Area::new(x + prefix_width, y - 1, selected_width.max(1), lh), self.th.p.selection);
                        self.p.text(font, x + prefix_width, y, &selected, self.th.p.on_selection);
                        self.p.text(font, x + prefix_width + selected_width, y, &suffix, color);
                    } else {
                        self.p.text(font, x, y, &shown, color);
                    }
                    source_offset = line_start + line.len();
                    y += lh;
                }
            }
            Element::Button(b) => {
                let pressed = self.ui.pressed.as_ref() == Some(path) && hover;
                self.th.button(self.p, a, &b.label, b.icon.as_deref(), b.kind, b.size, hover, pressed, b.on_press.is_some(), b.text_color);
            }
            Element::Input(i) => {
                let focused = self.focused(path);
                let field = self.ui.fields.entry(path.clone()).or_insert_with(|| Field::new(&i.value));
                field.sync(&i.value);
                field.paint(self.p, self.th, a, focused, i.secret, &i.placeholder);
            }
            Element::Editor(ed) => {
                let focused = self.focused(path);
                let font = if ed.monospace { &self.th.fonts.mono } else { &self.th.fonts.body };
                let look = Look { font, line_numbers: ed.line_numbers, highlight_line: ed.highlight_line, read_only: ed.read_only };
                let state = self.ui.editors.entry(path.clone()).or_default();
                state.sync(&ed.value);
                state.paint(self.p, self.th, &look, a, &ed.value, focused);
            }
            Element::Checkbox(c) => {
                let color = self.th.tone(crate::style::Tone::Normal, inverted);
                self.th.checkbox(self.p, r.x, r.y + (r.h - 18) / 2, c.checked, &c.label, hover, color);
            }
            Element::Switch(s) => {
                let (w, h) = self.sizes.switch;
                self.th.switch(self.p, Area::new(r.x, r.y + (r.h - h) / 2, w, h), s.checked, hover);
            }
            Element::Slider(s) => self.th.slider(self.p, a, s.fraction(), hover || self.ui.drag.as_ref() == Some(path)),
            Element::Progress(pr) => {
                let h = self.sizes.progress_height;
                self.th.progress(self.p, Area::new(r.x, r.y + (r.h - h) / 2, r.w, h), pr.fraction);
            }
            Element::Dropdown(d) => {
                self.th.button(self.p, a, "", None, crate::style::ButtonKind::Secondary, TextSize::Strong, hover, false, d.on_select.is_some(), None);
                let label = d.selected.and_then(|i| d.options.get(i)).map(|s| s.as_str()).unwrap_or("");
                let font = &self.th.fonts.body;
                self.p.text(font, r.x + 12, r.y + (r.h - font.height()) / 2, &ellipsize(font, label, r.w - 40), self.th.p.text);
                self.th.chevron(self.p, r.right() - 18, r.y + r.h / 2, self.th.p.dim);
            }
            Element::List(l) => {
                let offset = scroll_of(self.ui, path);
                let inner = dy + offset;
                let view_clip = r.intersect(&clip);
                let saved = self.p.clip;
                self.p.set_clip(area(view_clip).intersect(&saved));
                let hover_row = match &self.ui.hover {
                    Some((p, Hit::Row(i))) if p == path => Some(*i),
                    _ => None,
                };
                for (i, row) in l.rows.iter().enumerate() {
                    let Some(row_node) = n.children.get(i) else {
                        continue;
                    };
                    let rr = row_node.rect.offset(0, -inner);
                    if rr.intersect(&view_clip).is_empty() {
                        continue;
                    }
                    let selected = l.selected == Some(i);
                    self.th.row_background(self.p, area(rr), selected, hover_row == Some(i));
                    if let Some(content) = row_node.children.first() {
                        path.push(i as u16);
                        self.element(row, content, path, inner, view_clip, selected);
                        path.pop();
                    }
                }
                self.p.clip = saved;
                self.th.scrollbar(self.p, Area::new(r.right() - 7, r.y + 3, 4, r.h - 6), offset, n.content.1, r.h);
            }
            Element::Stack(s) => {
                if let Some(bg) = s.bg {
                    if s.radius > 0 {
                        self.p.rounded(a, s.radius as i32, bg, 255);
                    } else {
                        self.p.fill(a, bg);
                    }
                } else if s.card {
                    self.th.card(self.p, a);
                }
                if let Some((border_color, border_w)) = s.border {
                    if s.radius > 0 {
                        self.p.rounded_border(a, s.radius as i32, border_color, 255);
                    } else {
                        let bw = border_w as i32;
                        self.p.fill(Area::new(a.x, a.y, a.w, bw), border_color);
                        self.p.fill(Area::new(a.x, a.bottom() - bw, a.w, bw), border_color);
                        self.p.fill(Area::new(a.x, a.y, bw, a.h), border_color);
                        self.p.fill(Area::new(a.right() - bw, a.y, bw, a.h), border_color);
                    }
                }
                if s.sidebar {
                    self.p.fill(a, self.th.p.sidebar);
                    self.p.fill(Area::new(a.right() - 1, a.y, 1, a.h), self.th.p.border);
                }
                for (i, child) in s.children.iter().enumerate() {
                    if let Some(cn) = child_node(e, n, i) {
                        path.push(i as u16);
                        self.element(child, cn, path, dy, clip, inverted);
                        path.pop();
                    }
                }
            }
            Element::Scroll(sc) => {
                let offset = scroll_of(self.ui, path);
                let view_clip = r.intersect(&clip);
                let saved = self.p.clip;
                self.p.set_clip(area(view_clip).intersect(&saved));
                if let Some(cn) = n.children.first() {
                    path.push(0);
                    self.element(&sc.child, cn, path, dy + offset, view_clip, inverted);
                    path.pop();
                }
                self.p.clip = saved;
                self.th.scrollbar(self.p, Area::new(r.right() - 6, r.y + 2, 4, r.h - 4), offset, n.content.1, r.h);
            }
            Element::Space(_) => {}
            Element::Divider => {
                if r.w >= r.h {
                    self.p.fill(Area::new(r.x, r.y + r.h / 2, r.w, 1), self.th.p.border);
                } else {
                    self.p.fill(Area::new(r.x + r.w / 2, r.y, 1, r.h), self.th.p.border);
                }
            }
            Element::Image(i) => {
                let img = match &i.source {
                    ImageSource::File(path) => self.th.image(path),
                    ImageSource::Icon(name) => self.th.image(&Theme::icon_path(name)),
                    ImageSource::Pixels(px) => {
                        self.p.pixels(px.pixels(), px.width() as i32, px.height() as i32, r.x, r.y);
                        None
                    }
                };
                if let Some(img) = img {
                    let symbolic = matches!(&i.source, ImageSource::Icon(name) if name.starts_with("ui/")) || i.tone.is_some();
                    if img.w > r.w || img.h > r.h {
                        let scale = (r.w as f32 / img.w as f32).min(r.h as f32 / img.h as f32);
                        let (w, h) = ((img.w as f32 * scale) as i32, (img.h as f32 * scale) as i32);
                        self.p.image_scaled(img, Area::new(r.x + (r.w - w) / 2, r.y + (r.h - h) / 2, w, h), 255);
                    } else if symbolic {
                        self.p.image_tinted(img, r.x + (r.w - img.w) / 2, r.y + (r.h - img.h) / 2, self.th.tone(i.tone.unwrap_or(crate::style::Tone::Normal), inverted), 255);
                    } else {
                        self.p.image(img, r.x + (r.w - img.w) / 2, r.y + (r.h - img.h) / 2, 255);
                    }
                }
            }
            Element::Canvas(c) => {
                let saved = self.p.clip;
                self.p.set_clip(a.intersect(&saved));
                self.p.pixels(c.pixmap.pixels(), c.pixmap.width() as i32, c.pixmap.height() as i32, r.x, r.y);
                self.p.clip = saved;
            }

        }
    }
}

struct Runtime<A: App> {
    app: A,
    win: Transport,
    back: Vec<u32>,
    th: Theme,
    sizes: Sizes,
    settings: Settings,
    view: Element<A::Message>,
    tree: Node,
    menu: Vec<MenuItem<A::Message>>,
    header: i32,
    ui: Interaction,
    popup: Option<Popup<A::Message>>,
    pointer: (i32, i32),
    modal: Option<Modal<A::Message>>,
    timers: Vec<(u64, A::Message)>,
    tick: Option<(u64, u32)>,
    files: Vec<(u32, Reply<Option<String>, A::Message>)>,
    wallpapers: Vec<(u32, Reply<bool, A::Message>)>,
    pastes: VecDeque<Pending<A::Message>>,
    queue: VecDeque<A::Message>,
    title: String,
    quit: bool,
    repaint: bool,
}

fn key_from(code: i32, mods: u32) -> Option<KeyPress> {
    let m = Modifiers { shift: mods & 1 != 0, alt: mods & 2 != 0, ctrl: mods & 4 != 0 };
    let key = match code {
        10 | 13 => Key::Enter,
        8 | 127 if !m.ctrl => Key::Backspace,
        9 if !m.ctrl => Key::Tab,
        27 => Key::Escape,
        -1 => Key::Up,
        -2 => Key::Down,
        -3 => Key::Left,
        -4 => Key::Right,
        -5 => Key::Home,
        -6 => Key::End,
        -7 => Key::Delete,
        -8 => Key::PageUp,
        -9 => Key::PageDown,
        -53 => Key::Insert,
        c @ -52..=-41 => Key::F((-40 - c) as u8),
        c @ 1..=26 => {
            return Some(KeyPress { key: Key::Char((b'a' + (c - 1) as u8) as char), mods: Modifiers { ctrl: true, ..m } });
        }
        c if c >= 32 => Key::Char(char::from_u32(c as u32)?),
        _ => return None,
    };
    Some(KeyPress { key, mods: m })
}

fn filter_spec(filters: &[String]) -> String {
    if filters.is_empty() {
        return String::from("All files|*");
    }
    let exts: Vec<String> = filters.iter().map(|f| String::from(f.trim_start_matches("*.").trim_start_matches('.'))).collect();
    alloc::format!("Files|{};All files|*", exts.join(","))
}

impl<A: App> Runtime<A> {
    fn measure(&self) -> Measure<'_> {
        Measure { th: &self.th, sizes: &self.sizes }
    }

    fn text_index_at(&self, path: &Path, x: i32, y: i32) -> Option<usize> {
        let (Element::Text(text), node, dy) = locate(&self.view, &self.tree, path, &self.ui)? else {
            return None;
        };
        let rect = node.rect.offset(0, -dy);
        let font = text.font_family.as_deref().and_then(|name| self.th.fonts.get_by_name(name)).unwrap_or_else(|| self.th.fonts.get(text.size));
        let line_height = font.height() + 2;
        let lines = if text.wrap {
            layout::wrap_lines(&text.content, rect.w, |line| font.measure(line))
        } else {
            text.content.split('\n').collect()
        };
        if lines.is_empty() {
            return Some(0);
        }
        let line_index = ((y - rect.y - 1).max(0) / line_height.max(1)) as usize;
        let line_index = line_index.min(lines.len() - 1);
        let mut source_offset = 0;
        for (index, line) in lines.iter().enumerate().take(line_index + 1) {
            let remaining = &text.content[source_offset..];
            let line_start = source_offset + remaining.find(line).unwrap_or(0);
            if index == line_index {
                let line_chars: Vec<char> = line.chars().collect();
                let line_x = match text.align {
                    Align::Center => rect.x + (rect.w - font.measure(line)) / 2,
                    Align::End => rect.right() - font.measure(line),
                    _ => rect.x,
                };
                let local_x = x - line_x;
                let mut prefix = String::new();
                for (char_index, ch) in line_chars.iter().enumerate() {
                    let midpoint = font.measure(&prefix) + font.measure(&ch.to_string()) / 2;
                    if local_x < midpoint {
                        return Some(text.content[..line_start].chars().count() + char_index);
                    }
                    prefix.push(*ch);
                }
                return Some(text.content[..line_start].chars().count() + line_chars.len());
            }
            source_offset = line_start + line.len();
        }
        None
    }

    fn selected_text(&self) -> Option<String> {
        let (path, start, end) = self.ui.text_selection.as_ref()?;
        let (Element::Text(text), _, _) = locate(&self.view, &self.tree, path, &self.ui)? else {
            return None;
        };
        Some(text.content.chars().skip(*start).take(end.saturating_sub(*start)).collect())
    }

    fn content_rect(&self) -> Rect {
        Rect::new(0, self.header, self.win.width() as i32, self.win.height() as i32 - self.header)
    }

    fn apply(&mut self, commands: Vec<Command<A::Message>>) {
        for command in commands {
            match command {
                Command::Quit => self.quit = true,
                Command::Edit { id, action } => {
                    if let Some(path) = self.editor_by_id(&id) {
                        self.edit_action(&path, action);
                    }
                }
                Command::Find { id, query, reply } => {
                    let mut found = false;
                    if let Some(path) = self.editor_by_id(&id) {
                        self.with_editor(&path, |state, text, _, _| {
                            found = state.find(text, &query);
                            (false, None)
                        });
                    }
                    self.queue.push_back(reply(found));
                }
                Command::After { ms, message } => self.timers.push((posix::now_ms() + ms as u64, message)),
                Command::Copy(text) => self.win.set_selection(&text),
                Command::Paste(reply) => {
                    self.pastes.push_back(Pending::App(reply));
                    self.win.request_selection();
                }
                Command::ChooseFile { kind, title, filters, start, reply } => {
                    let mode = match kind {
                        FileDialog::Open => hxproto::PICK_OPEN_FILE,
                        FileDialog::Save => hxproto::PICK_SAVE_FILE,
                        FileDialog::Folder => hxproto::PICK_FOLDER,
                    };
                    let id = self.win.choose_file(mode, &title, &filter_spec(&filters), &start);
                    self.files.push((id, reply));
                }
                Command::Message { title, text } => self.modal = Some(Modal { title, text, reply: None, ok: Rect::default(), cancel: Rect::default(), hover: 0 }),
                Command::Confirm { title, text, reply } => self.modal = Some(Modal { title, text, reply: Some(reply), ok: Rect::default(), cancel: Rect::default(), hover: 0 }),
                Command::Notify { title, body } => self.win.notify(&title, &body),
                Command::Resize { width, height } => {
                    self.win.resize(width, height);
                }
                Command::SetWallpaper { path, reply } => {
                    let id = self.win.set_wallpaper(&path);
                    self.wallpapers.push((id, reply));
                }
                Command::ShowMenu(items) => {
                    let (x, y) = self.pointer;
                    self.open_popup(PopupKind::Context(items), x + 2, y + 2, 0);
                }
                Command::ResetScroll => {
                    self.ui.scrolls.clear();
                    self.repaint = true;
                }
            }
        }
    }

    fn process(&mut self) -> bool {
        if !self.queue.is_empty() {
            self.view.release_pixmaps();
        }
        let mut any = false;
        let mut rounds = 0;
        while let Some(message) = self.queue.pop_front() {
            let mut cx = Context::new();
            self.app.update(message, &mut cx);
            let commands = cx.take();
            self.apply(commands);
            any = true;
            rounds += 1;
            if rounds > 256 {
                break;
            }
        }
        any
    }

    fn rebuild(&mut self) {
        for _ in 0..3 {
            self.view = self.app.view();
            self.menu = self.app.menu();
            self.header = if self.menu.is_empty() { 0 } else if self.menubar() { MENUBAR } else { HEADER };
            preload(&self.view, &mut self.th);
            let rect = self.content_rect();
            self.tree = layout::layout(&self.view, rect, &self.measure());
            let mut resized = Vec::new();
            canvas_resizes(&self.view, &self.tree, &mut Vec::new(), &mut self.ui, &mut resized);
            if resized.is_empty() {
                break;
            }
            self.queue.extend(resized);
            self.process();
        }
        let title = self.app.title();
        if title != self.title {
            self.win.set_title(&title);
            self.title = title;
        }
        match self.app.tick() {
            Some((ms, _)) => {
                if self.tick.map(|(_, i)| i != ms).unwrap_or(true) {
                    self.tick = Some((posix::now_ms() + ms as u64, ms));
                }
            }
            None => self.tick = None,
        }
        let wanted = autofocus(&self.view, &mut Vec::new());
        if let Some((path, _)) = &wanted {
            if self.ui.focus.is_none() || wanted != self.ui.autofocus {
                self.ui.focus = Some(path.clone());
            }
        }
        self.ui.autofocus = wanted;
        self.clamp_scrolls();
        self.repaint = true;
    }

    fn clamp_scrolls(&mut self) {
        let paths: Vec<Path> = self.ui.scrolls.keys().cloned().collect();
        for path in paths {
            let max = match locate(&self.view, &self.tree, &path, &self.ui) {
                Some((Element::Scroll(_) | Element::List(_), n, _)) => (n.content.1 - n.rect.h).max(0),
                _ => 0,
            };
            if let Some(v) = self.ui.scrolls.get_mut(&path) {
                *v = (*v).clamp(0, max);
            }
        }
    }

    fn paint(&mut self) {
        let (w, h) = (self.win.width() as i32, self.win.height() as i32);
        let header = self.header;
        let content = self.content_rect();
        let bg = self.th.p.bg;
        let mut back = core::mem::take(&mut self.back);
        back.resize((w * h) as usize, bg);
        let mut p = Painter::new(&mut back, w, h);
        p.fill(Area::new(0, 0, w, h), bg);
        if header > 0 {
            p.fill(Area::new(0, header - 1, w, 1), self.th.p.border);
            let open = match &self.popup {
                Some(Popup { kind: PopupKind::Menu(top), .. }) => Some(top.unwrap_or(usize::MAX)),
                Some(_) => None,
                _ => None,
            };
            for (i, title) in self.menu_titles() {
                let r = area(title);
                if open == Some(i) {
                    p.rounded(r, 6, self.th.p.selection, 255);
                } else if self.ui.menu_hover == Some(i) {
                    p.rounded(r, 6, self.th.p.hover, 255);
                }
                if i == usize::MAX {
                    self.th.dots(&mut p, r.x + r.w / 2, r.y + r.h / 2, self.th.p.text);
                } else {
                    let color = if open == Some(i) { self.th.p.on_selection } else { self.th.p.text };
                    self.th.centered_text(&mut p, &self.th.fonts.body, r, &self.menu[i].label, color);
                }
            }
        }
        {
            let mut painting = Painting { p: &mut p, th: &self.th, sizes: &self.sizes, ui: &mut self.ui };
            painting.element(&self.view, &self.tree, &mut Vec::new(), 0, content, false);
        }
        self.paint_modal(&mut p, w, h);
        drop(p);
        self.win.update(&back, w as usize, h as usize);
        self.back = back;
        self.paint_popup();
        self.repaint = false;
    }

    fn entries(&self, kind: &PopupKind<A::Message>) -> Vec<Entry> {
        let from_menu = |items: &[MenuItem<A::Message>]| -> Vec<Entry> {
            items
                .iter()
                .map(|m| Entry {
                    label: m.label.clone(),
                    detail: if !m.children.is_empty() { String::from("›") } else { m.shortcut.map(|s| s.label()).unwrap_or_default() },
                    enabled: m.message.is_some(),
                    checked: m.checked,
                    separator: m.separator,
                })
                .collect()
        };
        match kind {
            PopupKind::Menu(top) => from_menu(self.menu_items(*top)),
            PopupKind::Context(items) => from_menu(items),
            PopupKind::Dropdown(path) => match locate(&self.view, &self.tree, path, &self.ui) {
                Some((Element::Dropdown(d), _, _)) => d
                    .options
                    .iter()
                    .enumerate()
                    .map(|(i, o)| Entry { label: o.clone(), detail: String::new(), enabled: true, checked: Some(d.selected == Some(i)), separator: false })
                    .collect(),
                _ => Vec::new(),
            },
            PopupKind::Edit(path, editor) => {
                let (read_only, secret) = match locate(&self.view, &self.tree, path, &self.ui) {
                    Some((Element::Editor(e), _, _)) => (e.read_only, false),
                    Some((Element::Input(i), _, _)) => (false, i.secret),
                    _ => (true, false),
                };
                EDIT_ITEMS
                    .iter()
                    .enumerate()
                    .map(|(i, (label, detail))| Entry {
                        label: String::from(*label),
                        detail: String::from(*detail),
                        enabled: match i {
                            0 => !read_only && !secret,
                            1 => !secret,
                            2 => !read_only,
                            4 => *editor,
                            _ => false,
                        },
                        checked: None,
                        separator: i == 3,
                    })
                    .collect()
            }
        }
    }

    fn open_popup(&mut self, kind: PopupKind<A::Message>, x: i32, y: i32, min_width: i32) {
        self.close_popup();
        let entries = self.entries(&kind);
        if entries.is_empty() {
            return;
        }
        let font = &self.th.fonts.body;
        let width = entries.iter().map(|e| font.measure(&e.label) + if e.detail.is_empty() { 0 } else { font.measure(&e.detail) + 30 }).max().unwrap_or(120).max(100) + 60;
        let width = width.max(min_width);
        let mut ry = 6;
        let mut rows = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let h = if e.separator { MENU_SEPARATOR } else { MENU_ROW };
            rows.push((Rect::new(0, ry, width, h), if e.separator { None } else { Some(i) }));
            ry += h;
        }
        let height = ry + 6;
        let hover = match &kind {
            PopupKind::Dropdown(_) => entries.iter().position(|e| e.checked == Some(true)),
            _ => None,
        };
        let surface = self.win.create_popup(x, y, width as u32, height as u32);
        self.popup = Some(Popup { kind, origin: (x, y), size: (width, height), rows, hover, surface, opened: posix::now_ms() });
        self.repaint = true;
    }

    fn close_popup(&mut self) {
        if let Some(popup) = self.popup.take() {
            if let Some(surface) = popup.surface {
                self.win.destroy_surface(surface);
            }
            self.repaint = true;
        }
    }

    fn paint_popup(&mut self) {
        let Some(popup) = &self.popup else {
            return;
        };
        let entries = self.entries(&popup.kind);
        let (w, h) = popup.size;
        let mut pixels = alloc::vec![0u32; (w * h).max(1) as usize];
        {
            let mut p = Painter::new(&mut pixels, w, h);
            let r = Area::new(0, 0, w, h);
            p.fill(r, self.th.p.popup);
            p.rounded_border(r, POPUP_RADIUS, self.th.p.border, 255);
            let font = &self.th.fonts.body;
            for (i, (row, item)) in popup.rows.iter().enumerate() {
                let Some(index) = item else {
                    p.fill(Area::new(row.x + 10, row.y + row.h / 2, row.w - 20, 1), self.th.p.border);
                    continue;
                };
                let Some(e) = entries.get(*index) else {
                    continue;
                };
                let hovered = popup.hover == Some(i);
                if hovered && e.enabled {
                    p.rounded(Area::new(row.x + 5, row.y + 1, row.w - 10, row.h - 2), 6, self.th.p.selection, 255);
                }
                let color = if !e.enabled { self.th.p.faint } else if hovered { self.th.p.on_selection } else { self.th.p.text };
                if e.checked == Some(true) {
                    p.line(row.x as f32 + 12.0, (row.y + row.h / 2) as f32, row.x as f32 + 15.5, (row.y + row.h / 2 + 4) as f32, 1.8, color, 255);
                    p.line(row.x as f32 + 15.5, (row.y + row.h / 2 + 4) as f32, row.x as f32 + 22.0, (row.y + row.h / 2 - 4) as f32, 1.8, color, 255);
                }
                p.text(font, row.x + 30, row.y + (row.h - font.height()) / 2, &e.label, color);
                if !e.detail.is_empty() {
                    let dw = font.measure(&e.detail);
                    p.text(font, row.right() - dw - 14, row.y + (row.h - font.height()) / 2, &e.detail, if hovered && e.enabled { color } else { self.th.p.faint });
                }
            }
        }
        round_corners(&mut pixels, w, h, POPUP_RADIUS);
        let origin = popup.origin;
        let popup = self.popup.as_mut().unwrap();
        match popup.surface.as_mut() {
            Some(surface) => {
                surface.buffer().copy_from_slice(&pixels);
                self.win.present_surface(surface);
            }
            None => {
                let (ww, wh) = (self.win.width() as i32, self.win.height() as i32);
                let buffer = self.win.buffer();
                for y in 0..h {
                    let ty = origin.1 + y;
                    if ty < 0 || ty >= wh {
                        continue;
                    }
                    for x in 0..w {
                        let tx = origin.0 + x;
                        if tx < 0 || tx >= ww {
                            continue;
                        }
                        let src = pixels[(y * w + x) as usize];
                        if src >> 24 == 255 {
                            buffer[(ty * ww + tx) as usize] = src & 0x00ff_ffff;
                        }
                    }
                }
            }
        }
    }

    fn paint_modal(&mut self, p: &mut Painter, w: i32, h: i32) {
        let th = &self.th;
        let Some(modal) = &mut self.modal else {
            return;
        };
        p.blend_fill(Area::new(0, 0, w, h), 0x000000, 110);
        let width = 380.min(w - 32);
        let text_w = width - 40;
        let lines = layout::wrap_lines(&modal.text, text_w, |s| th.fonts.body.measure(s));
        let lh = th.fonts.body.height() + 4;
        let height = 20 + th.fonts.title.height() + 14 + lines.len() as i32 * lh + 24 + 34 + 20;
        let card = Area::new((w - width) / 2, (h - height) / 2, width, height);
        p.shadow(card, 12, 16, 110, 4);
        th.card(p, card);
        let mut y = card.y + 20;
        p.text(&th.fonts.title, card.x + 20, y, &modal.title, th.p.text);
        y += th.fonts.title.height() + 14;
        for line in lines {
            p.text(&th.fonts.body, card.x + 20, y, line, th.p.dim);
            y += lh;
        }
        let by = card.bottom() - 20 - 34;
        modal.ok = Rect::new(card.right() - 20 - 96, by, 96, 34);
        th.button(p, area(modal.ok), "OK", None, crate::style::ButtonKind::Primary, TextSize::Strong, modal.hover == 1, false, true, None);
        if modal.reply.is_some() {
            modal.cancel = Rect::new(modal.ok.x - 10 - 96, by, 96, 34);
            th.button(p, area(modal.cancel), "Cancel", None, crate::style::ButtonKind::Secondary, TextSize::Strong, modal.hover == 2, false, true, None);
        } else {
            modal.cancel = Rect::default();
        }
    }

    fn close_modal(&mut self, accepted: bool) {
        if let Some(modal) = self.modal.take() {
            if let Some(reply) = modal.reply {
                self.queue.push_back(reply(accepted));
            }
        }
        self.repaint = true;
    }

    fn menubar(&self) -> bool {
        self.menu.iter().any(|m| !m.children.is_empty())
    }

    fn menu_items(&self, top: Option<usize>) -> &[MenuItem<A::Message>] {
        match top {
            Some(i) => self.menu.get(i).map(|m| m.children.as_slice()).unwrap_or(&[]),
            None => &self.menu,
        }
    }

    fn menu_titles(&self) -> Vec<(usize, Rect)> {
        if self.menu.is_empty() {
            return Vec::new();
        }
        if !self.menubar() {
            return alloc::vec![(usize::MAX, Rect::new(self.win.width() as i32 - 44, 6, 36, 32))];
        }
        let font = &self.th.fonts.body;
        let mut x = 6;
        self.menu
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let w = font.measure(&m.label) + 20;
                let r = Rect::new(x, 4, w, MENUBAR - 9);
                x += w + 2;
                (i, r)
            })
            .collect()
    }

    fn open_menu(&mut self, top: Option<usize>) {
        let (x, y) = match top {
            Some(i) => {
                let title = self.menu_titles().into_iter().find(|(t, _)| *t == i).map(|(_, r)| r).unwrap_or_default();
                (title.x, self.header - 2)
            }
            None => {
                let font = &self.th.fonts.body;
                let width = self.menu.iter().map(|m| font.measure(&m.label) + m.shortcut.map(|s| font.measure(&s.label()) + 30).unwrap_or(0)).max().unwrap_or(120).max(100) + 60;
                (self.win.width() as i32 - width - 8, self.header - 4)
            }
        };
        self.open_popup(PopupKind::Menu(top), x, y, 0);
    }

    fn open_dropdown(&mut self, path: Path) {
        let Some((Element::Dropdown(_), n, dy)) = locate(&self.view, &self.tree, &path, &self.ui) else {
            return;
        };
        let r = n.rect.offset(0, -dy);
        self.open_popup(PopupKind::Dropdown(path), r.x, r.bottom() + 4, r.w);
    }

    fn choose_popup(&mut self, row: usize) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        self.repaint = true;
        let index = popup.rows.get(row).and_then(|r| r.1);
        if let Some(surface) = popup.surface {
            self.win.destroy_surface(surface);
        }
        let Some(index) = index else {
            return;
        };
        match popup.kind {
            PopupKind::Menu(top) => {
                let item = self.menu_items(top).get(index);
                if let Some(item) = item {
                    if !item.children.is_empty() {
                        let (x, y) = (popup.origin.0 + popup.size.0 - 6, popup.origin.1 + popup.rows[row].0.y);
                        let children: Vec<MenuItem<A::Message>> = item.children.iter().map(clone_item).collect();
                        self.open_popup(PopupKind::Context(children), x, y, 0);
                        return;
                    }
                    if let Some(m) = item.message.clone() {
                        self.queue.push_back(m);
                    }
                }
            }
            PopupKind::Context(items) => {
                if let Some(item) = items.get(index) {
                    if !item.children.is_empty() {
                        let (x, y) = (popup.origin.0 + popup.size.0 - 6, popup.origin.1 + popup.rows[row].0.y);
                        let children: Vec<MenuItem<A::Message>> = item.children.iter().map(clone_item).collect();
                        self.open_popup(PopupKind::Context(children), x, y, 0);
                        return;
                    }
                    if let Some(m) = item.message.clone() {
                        self.queue.push_back(m);
                    }
                }
            }
            PopupKind::Dropdown(path) => {
                if let Some((Element::Dropdown(d), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                    if let Some(cb) = &d.on_select {
                        self.queue.push_back(cb(index));
                    }
                }
            }
            PopupKind::Edit(path, editor) => self.edit_entry(&path, editor, index),
        }
    }

    fn edit_entry(&mut self, path: &Path, editor: bool, index: usize) {
        use crate::app::EditAction;
        if editor {
            let action = match index {
                0 => EditAction::Cut,
                1 => EditAction::Copy,
                2 => EditAction::Paste,
                _ => EditAction::SelectAll,
            };
            self.edit_action(path, action);
            return;
        }
        let Some((Element::Input(i), _, _)) = locate(&self.view, &self.tree, path, &self.ui) else {
            return;
        };
        let value = i.value.clone();
        match index {
            0 | 1 => {
                self.win.set_selection(&value);
                if index == 0 {
                    let field = self.ui.fields.entry(path.clone()).or_insert_with(|| Field::new(&value));
                    field.sync(&value);
                    field.text.clear();
                    field.cursor = 0;
                    field.sent();
                    if let Some(cb) = &i.on_change {
                        self.queue.push_back(cb(String::new()));
                    }
                }
            }
            2 => {
                self.pastes.push_back(Pending::Field(path.clone()));
                self.win.request_selection();
            }
            _ => {}
        }
        self.ui.focus = Some(path.clone());
    }

    fn popup_mouse(&mut self, x: i32, y: i32, kind: u32) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        let hover = popup.rows.iter().position(|(r, item)| item.is_some() && r.contains(x, y));
        if hover != popup.hover && kind != MOUSE_LEAVE {
            popup.hover = hover;
            self.repaint = true;
        }
        let late = posix::now_ms().saturating_sub(popup.opened) > 300;
        match (kind, hover) {
            (MOUSE_PRESS, Some(i)) => self.choose_popup(i),
            (MOUSE_RELEASE, Some(i)) if late => self.choose_popup(i),
            _ => {}
        }
    }

    fn slider_value(&mut self, path: &Path, x: i32) {
        if let Some((Element::Slider(s), n, _)) = locate(&self.view, &self.tree, path, &self.ui) {
            let track_x = n.rect.x + 8;
            let track_w = (n.rect.w - 16).max(1);
            let fraction = ((x - track_x) as f32 / track_w as f32).clamp(0.0, 1.0);
            let value = s.snap(s.min + (s.max - s.min) * fraction);
            if (value - s.value).abs() > f32::EPSILON {
                if let Some(cb) = &s.on_change {
                    self.queue.push_back(cb(value));
                }
            }
        }
    }

    fn canvas_event(&mut self, path: &Path, make: impl Fn(i32, i32) -> CanvasEvent, x: i32, y: i32) {
        if let Some((Element::Canvas(c), n, dy)) = locate(&self.view, &self.tree, path, &self.ui) {
            let r = n.rect.offset(0, -dy);
            if let Some(cb) = &c.on_event {
                if let Some(m) = cb(make(x - r.x, y - r.y)) {
                    self.queue.push_back(m);
                }
            }
        }
    }

    fn mouse(&mut self, x: i32, y: i32, buttons: u32, kind: u32, wheel: i32) {
        if let Some(modal) = &mut self.modal {
            let hover = if modal.ok.contains(x, y) { 1 } else if modal.cancel.contains(x, y) { 2 } else { 0 };
            if hover != modal.hover {
                modal.hover = hover;
                self.repaint = true;
            }
            if kind == MOUSE_PRESS && hover != 0 {
                self.close_modal(hover == 1);
            }
            return;
        }
        if kind != MOUSE_LEAVE {
            self.pointer = (x, y);
        }
        if let Some(Popup { kind: PopupKind::Menu(Some(open)), .. }) = &self.popup {
            let open = *open;
            if let Some((t, _)) = self.menu_titles().into_iter().find(|(_, r)| r.contains(x, y)) {
                if kind == MOUSE_PRESS && t == open {
                    self.close_popup();
                } else if t != open {
                    self.open_menu(Some(t));
                }
                return;
            }
        }
        if let Some(popup) = &self.popup {
            let (ox, oy) = popup.origin;
            let (pw, ph) = popup.size;
            let inside = x >= ox && y >= oy && x < ox + pw && y < oy + ph;
            if popup.surface.is_none() && inside {
                self.popup_mouse(x - ox, y - oy, kind);
                return;
            }
            if kind == MOUSE_PRESS {
                self.close_popup();
                return;
            }
            if kind == MOUSE_RELEASE {
                return;
            }
        }
        if self.header > 0 {
            let over = self.menu_titles().into_iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| i);
            if over != self.ui.menu_hover {
                self.ui.menu_hover = over;
                self.repaint = true;
            }
            if let (Some(i), MOUSE_PRESS) = (over, kind) {
                self.open_menu(if i == usize::MAX { None } else { Some(i) });
                return;
            }
        }
        match kind {
            MOUSE_WHEEL => {
                if let Some((path, Hit::Canvas)) = hit(&self.view, &self.tree, &mut Vec::new(), x, y, 0, &self.ui) {
                    self.canvas_event(&path, |x, y| CanvasEvent::Wheel { x, y, delta: wheel.signum() }, x, y);
                    return;
                }
                let Some(path) = scrollable(&self.view, &self.tree, &mut Vec::new(), x, y, 0, &self.ui) else {
                    return;
                };
                let found = locate(&self.view, &self.tree, &path, &self.ui).map(|(e, n, _)| match e {
                    Element::Editor(ed) => (true, n.rect, ed.monospace, ed.value.clone(), 0),
                    _ => (false, n.rect, false, String::new(), (n.content.1 - n.rect.h).max(0)),
                });
                if let Some((is_editor, rect, mono, value, max)) = found {
                    if is_editor {
                        let font = if mono { &self.th.fonts.mono } else { &self.th.fonts.body };
                        self.ui.editors.entry(path).or_default().wheel(&value, font, area(rect), wheel.signum());
                    } else {
                        let v = self.ui.scrolls.entry(path).or_insert(0);
                        *v = (*v + wheel.signum() * 48).clamp(0, max);
                    }
                    self.repaint = true;
                }
            }
            MOUSE_MOVE | MOUSE_LEAVE => {
                if let Some(path) = self.ui.drag.clone() {
                    self.slider_value(&path, x);
                    return;
                }
                if let Some((path, anchor)) = self.ui.text_drag.clone() {
                    let end = self.text_index_at(&path, x, y).unwrap_or(anchor);
                    self.ui.text_selection = Some((path, anchor.min(end), anchor.max(end)));
                    self.repaint = true;
                    return;
                }
                if let Some(path) = self.ui.select_drag.clone() {
                    self.with_editor(&path, |state, text, look, rect| {
                        state.drag(text, look, area(rect), x, y);
                        (false, None)
                    });
                    return;
                }
                let target = if kind == MOUSE_LEAVE { None } else { hit(&self.view, &self.tree, &mut Vec::new(), x, y, 0, &self.ui) };
                let cursor = match target.as_ref().map(|t| t.1) {
                    Some(Hit::Input) | Some(Hit::Editor) | Some(Hit::Text) => CURSOR_TEXT,
                    _ => CURSOR_ARROW,
                };
                self.win.set_cursor(cursor);
                if let Some((path, Hit::Canvas)) = &target {
                    let path = path.clone();
                    self.canvas_event(&path, |x, y| CanvasEvent::Move { x, y }, x, y);
                }
                if target != self.ui.hover {
                    self.ui.hover = target;
                    self.repaint = true;
                }
            }
            MOUSE_PRESS => {
                let button = if buttons & 2 != 0 { MouseButton::Right } else if buttons & 4 != 0 { MouseButton::Middle } else { MouseButton::Left };
                self.ui.button = Some(button);
                let target = hit(&self.view, &self.tree, &mut Vec::new(), x, y, 0, &self.ui);
                self.repaint = true;
                let Some((path, what)) = target else {
                    self.ui.focus = None;
                    return;
                };
                if button == MouseButton::Right && matches!(what, Hit::Input | Hit::Editor | Hit::Row(_) | Hit::ListBlank) {
                    match what {
                        Hit::Row(i) => {
                            if let Some((Element::List(l), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                                if l.selected != Some(i) {
                                    self.queue.extend(l.on_select.as_ref().map(|cb| cb(i)));
                                }
                                self.queue.extend(l.on_context.as_ref().map(|cb| cb(Some(i))));
                            }
                        }
                        Hit::ListBlank => {
                            if let Some((Element::List(l), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                                self.queue.extend(l.on_context.as_ref().map(|cb| cb(None)));
                            }
                        }
                        _ => {
                            let editor = what == Hit::Editor;
                            self.open_popup(PopupKind::Edit(path.clone(), editor), x + 2, y + 2, 0);
                        }
                    }
                    self.ui.focus = Some(path);
                    return;
                }
                match what {
                    Hit::Button => self.ui.pressed = Some(path),
                    Hit::Input => {
                        if let Some((Element::Input(i), n, dy)) = locate(&self.view, &self.tree, &path, &self.ui) {
                            let r = n.rect.offset(0, -dy);
                            let secret = i.secret;
                            let value = i.value.clone();
                            let field = self.ui.fields.entry(path.clone()).or_insert_with(|| Field::new(&value));
                            field.click(&self.th.fonts.body, secret, x - r.x - 10);
                        }
                        self.ui.focus = Some(path);
                    }
                    Hit::Text if button == MouseButton::Left => {
                        if let Some(index) = self.text_index_at(&path, x, y) {
                            self.ui.text_drag = Some((path.clone(), index));
                            self.ui.text_selection = Some((path.clone(), index, index));
                            self.ui.focus = Some(path);
                        }
                    }
                    Hit::Text => {}
                    Hit::Editor => {
                        self.with_editor(&path, |state, text, look, rect| {
                            state.press(text, look, area(rect), x, y, false);
                            (false, None)
                        });
                        self.ui.select_drag = Some(path.clone());
                        self.ui.focus = Some(path);
                    }
                    Hit::Check | Hit::Switch => {
                        let message = match locate(&self.view, &self.tree, &path, &self.ui) {
                            Some((Element::Checkbox(c), _, _)) => c.on_toggle.as_ref().map(|cb| cb(!c.checked)),
                            Some((Element::Switch(s), _, _)) => s.on_toggle.as_ref().map(|cb| cb(!s.checked)),
                            _ => None,
                        };
                        self.queue.extend(message);
                    }
                    Hit::Slider => {
                        self.ui.drag = Some(path.clone());
                        self.slider_value(&path, x);
                    }
                    Hit::Dropdown => self.open_dropdown(path),
                    Hit::ListBlank => self.ui.focus = Some(path),
                    Hit::Row(i) => {
                        let now = posix::now_ms();
                        let double = self.ui.last_click.1.as_ref() == Some(&(path.clone(), i)) && now - self.ui.last_click.0 < DOUBLE_CLICK_MS;
                        self.ui.last_click = (now, Some((path.clone(), i)));
                        if let Some((Element::List(l), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                            if double {
                                self.queue.extend(l.on_activate.as_ref().map(|cb| cb(i)));
                            } else if l.selected != Some(i) {
                                self.queue.extend(l.on_select.as_ref().map(|cb| cb(i)));
                            }
                        }
                        self.ui.focus = Some(path);
                    }
                    Hit::Canvas => self.canvas_event(&path, |x, y| CanvasEvent::Press { x, y, button }, x, y),
                }
            }
            MOUSE_RELEASE => {
                self.ui.drag = None;
                self.ui.select_drag = None;
                if self.ui.text_drag.take().is_some() {
                    if let Some(selected) = self.selected_text().filter(|text| !text.is_empty()) {
                        self.win.set_selection(&selected);
                    }
                    self.repaint = true;
                }
                if let Some(path) = self.ui.pressed.take() {
                    let target = hit(&self.view, &self.tree, &mut Vec::new(), x, y, 0, &self.ui);
                    if target.as_ref().map(|t| &t.0) == Some(&path) {
                        if let Some((Element::Button(b), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                            self.queue.extend(b.on_press.clone());
                        }
                    }
                    self.repaint = true;
                }
                if let Some((path, Hit::Canvas)) = hit(&self.view, &self.tree, &mut Vec::new(), x, y, 0, &self.ui) {
                    let button = self.ui.button.unwrap_or(MouseButton::Left);
                    self.canvas_event(&path, |x, y| CanvasEvent::Release { x, y, button }, x, y);
                }
            }
            _ => {}
        }
    }

    fn move_focus(&mut self, forward: bool) {
        let mut all = Vec::new();
        collect_focusable(&self.view, &mut Vec::new(), &mut all);
        if all.is_empty() {
            return;
        }
        let current = self.ui.focus.as_ref().and_then(|f| all.iter().position(|p| p == f));
        let next = match current {
            Some(i) if forward => (i + 1) % all.len(),
            Some(i) => (i + all.len() - 1) % all.len(),
            None => 0,
        };
        self.ui.focus = Some(all[next].clone());
        self.repaint = true;
    }

    fn key(&mut self, code: i32, mods: u32) {
        let press = key_from(code, mods);
        if self.modal.is_some() {
            match press.map(|p| p.key) {
                Some(Key::Enter) => self.close_modal(true),
                Some(Key::Escape) => self.close_modal(false),
                _ => {}
            }
            return;
        }
        if let (Some(Popup { kind: PopupKind::Menu(Some(open)), .. }), Some(p)) = (&self.popup, press) {
            let open = *open;
            let count = self.menu.len();
            match p.key {
                Key::Left => {
                    self.open_menu(Some((open + count - 1) % count));
                    return;
                }
                Key::Right => {
                    self.open_menu(Some((open + 1) % count));
                    return;
                }
                _ => {}
            }
        }
        if let Some(popup) = &mut self.popup {
            let selectable: Vec<usize> = popup.rows.iter().enumerate().filter(|(_, r)| r.1.is_some()).map(|(i, _)| i).collect();
            let pos = popup.hover.and_then(|h| selectable.iter().position(|s| *s == h));
            match press.map(|p| p.key) {
                Some(Key::Down) => popup.hover = selectable.get(pos.map(|p| (p + 1).min(selectable.len() - 1)).unwrap_or(0)).copied(),
                Some(Key::Up) => popup.hover = selectable.get(pos.map(|p| p.saturating_sub(1)).unwrap_or(0)).copied(),
                Some(Key::Enter) => {
                    if let Some(h) = popup.hover {
                        self.choose_popup(h);
                    }
                }
                Some(Key::Escape) => self.close_popup(),
                _ => {}
            }
            self.repaint = true;
            return;
        }
        let Some(press) = press else {
            return;
        };
        if press.ctrl('c') {
            if self.ui.text_selection.as_ref().map(|(path, _, _)| Some(path) == self.ui.focus.as_ref()).unwrap_or(false) {
                if let Some(selected) = self.selected_text().filter(|text| !text.is_empty()) {
                    self.win.set_selection(&selected);
                }
                return;
            }
        }
        if press.ctrl('a') {
            if let Some(path) = self.ui.focus.clone() {
                if let Some((Element::Text(text), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                    let count = text.content.chars().count();
                    self.ui.text_selection = Some((path, 0, count));
                    self.repaint = true;
                    return;
                }
            }
        }
        if press.mods.ctrl || press.mods.alt || matches!(press.key, Key::F(_)) {
            if let Some(m) = shortcut_message(&self.menu, &press) {
                self.queue.push_back(m);
                return;
            }
        }
        let focus = self.ui.focus.clone();
        let focused = focus.as_ref().and_then(|f| locate(&self.view, &self.tree, f, &self.ui).map(|(e, n, _)| (e.kind(), n.rect)));
        match (focus, focused) {
            (Some(path), Some(("input", _))) => {
                if press.ctrl('v') {
                    self.pastes.push_back(Pending::Field(path));
                    self.win.request_selection();
                    return;
                }
                if press.ctrl('c') {
                    if let Some(f) = self.ui.fields.get(&path) {
                        self.win.set_selection(&f.text.clone());
                    }
                    return;
                }
                if press.key == Key::Tab {
                    self.move_focus(!press.mods.shift);
                    return;
                }
                if press.key == Key::Escape {
                    self.ui.focus = None;
                    self.repaint = true;
                    return;
                }
                let raw = if press.mods.ctrl { if let Key::Char(c) = press.key { c as i32 - 'a' as i32 + 1 } else { code } } else { code };
                self.field_key(path, raw);
            }
            (Some(path), Some(("editor", _))) => {
                let action = match press.key {
                    _ if press.ctrl('v') => Some(crate::app::EditAction::Paste),
                    _ if press.ctrl('c') => Some(crate::app::EditAction::Copy),
                    _ if press.ctrl('x') => Some(crate::app::EditAction::Cut),
                    _ if press.ctrl('a') => Some(crate::app::EditAction::SelectAll),
                    _ if press.ctrl('z') => Some(crate::app::EditAction::Undo),
                    _ if press.ctrl('y') => Some(crate::app::EditAction::Redo),
                    _ => None,
                };
                if let Some(action) = action {
                    self.edit_action(&path, action);
                    return;
                }
                if press.key == Key::Escape && self.with_editor(&path, |_, _, _, _| (false, None)).map(|s| s.selection().is_none()).unwrap_or(true) {
                    self.ui.focus = None;
                    self.repaint = true;
                    return;
                }
                let shift = press.mods.shift;
                let raw = if press.mods.ctrl { if let Key::Char(c) = press.key { c as i32 - 'a' as i32 + 1 } else { code } } else { code };
                self.with_editor(&path, |state, text, look, rect| {
                    let page = EditorState::page_lines(look.font, area(rect));
                    (state.key(text, look, raw, shift, page), None)
                });
            }
            (Some(path), Some(("list" | "grid", _))) if matches!(press.key, Key::Up | Key::Down | Key::Left | Key::Right | Key::Enter | Key::Home | Key::End) => {
                if let Some((Element::List(l), n, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                    let count = l.rows.len();
                    if count == 0 {
                        return;
                    }
                    let step = match l.grid {
                        Some((cw, _)) => layout::grid_columns((n.rect.w - self.sizes.scrollbar - 8).max(cw as i32), cw as i32) as usize,
                        None => 1,
                    };
                    let current = l.selected;
                    let next = match press.key {
                        Key::Left if l.grid.is_some() => Some(current.map(|c| c.saturating_sub(1)).unwrap_or(0)),
                        Key::Right if l.grid.is_some() => Some(current.map(|c| (c + 1).min(count - 1)).unwrap_or(0)),
                        Key::Up => Some(current.map(|c| c.saturating_sub(step)).unwrap_or(0)),
                        Key::Down => Some(current.map(|c| (c + step).min(count - 1)).unwrap_or(0)),
                        Key::Home => Some(0),
                        Key::End => Some(count - 1),
                        _ => None,
                    };
                    match (press.key, next) {
                        (Key::Enter, _) => {
                            if let (Some(i), Some(cb)) = (current, &l.on_activate) {
                                self.queue.push_back(cb(i));
                            }
                        }
                        (_, Some(i)) if Some(i) != current => {
                            if let Some(cb) = &l.on_select {
                                self.queue.push_back(cb(i));
                            }
                            self.scroll_into_view(&path, i);
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                if let Some(m) = shortcut_message(&self.menu, &press).or_else(|| self.app.key(&press)) {
                    self.queue.push_back(m);
                    return;
                }
                if press.key == Key::Tab {
                    self.move_focus(!press.mods.shift);
                }
            }
        }
    }

    fn scroll_into_view(&mut self, path: &Path, row: usize) {
        if let Some((Element::List(_), n, _)) = locate(&self.view, &self.tree, path, &self.ui) {
            if let Some(rn) = n.children.get(row) {
                let top = rn.rect.y - n.rect.y;
                let bottom = top + rn.rect.h;
                let v = self.ui.scrolls.entry(path.clone()).or_insert(0);
                if top < *v {
                    *v = top;
                } else if bottom > *v + n.rect.h {
                    *v = bottom - n.rect.h;
                }
            }
        }
    }

    fn field_key(&mut self, path: Path, code: i32) {
        let Some((Element::Input(i), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) else {
            return;
        };
        let field = self.ui.fields.entry(path.clone()).or_insert_with(|| Field::new(&i.value));
        field.sync(&i.value);
        match field.key(code) {
            FieldAction::Changed => {
                field.sent();
                if let Some(cb) = &i.on_change {
                    self.queue.push_back(cb(field.text.clone()));
                }
            }
            FieldAction::Submit => {
                if let Some(m) = &i.on_submit {
                    self.queue.push_back(m.clone());
                }
            }
            FieldAction::Next => self.move_focus(true),
            FieldAction::None => {}
        }
        self.repaint = true;
    }

    fn with_editor(&mut self, path: &Path, op: impl FnOnce(&mut EditorState, &mut String, &Look, Rect) -> (bool, Option<String>)) -> Option<EditorState> {
        let (ed, n, dy) = match locate(&self.view, &self.tree, path, &self.ui) {
            Some((Element::Editor(ed), n, dy)) => (ed, n, dy),
            _ => return None,
        };
        let font = if ed.monospace { &self.th.fonts.mono } else { &self.th.fonts.body };
        let look = Look { font, line_numbers: ed.line_numbers, highlight_line: ed.highlight_line, read_only: ed.read_only };
        let mut text = ed.value.clone();
        let state = self.ui.editors.entry(path.clone()).or_default();
        state.sync(&text);
        let before = state.position(&text);
        let (changed, copy) = op(state, &mut text, &look, n.rect.offset(0, -dy));
        let after = state.position(&text);
        let snapshot = state.clone();
        if changed {
            if let Some(cb) = &ed.on_change {
                self.queue.push_back(cb(text));
            }
        }
        if before != after || changed {
            if let Some(cb) = &ed.on_cursor {
                self.queue.push_back(cb(after));
            }
        }
        if let Some(copy) = copy {
            self.win.set_selection(&copy);
        }
        self.repaint = true;
        Some(snapshot)
    }

    fn edit_action(&mut self, path: &Path, action: crate::app::EditAction) {
        use crate::app::EditAction;
        if action == EditAction::Paste {
            self.pastes.push_back(Pending::Editor(path.clone()));
            self.win.request_selection();
            return;
        }
        if action == EditAction::Focus {
            self.ui.focus = Some(path.clone());
            self.repaint = true;
            return;
        }
        self.with_editor(path, |state, text, _, _| match action {
            EditAction::SelectAll => {
                state.select_all(text);
                (false, None)
            }
            EditAction::Copy => (false, Some(String::from(state.selected(text))).filter(|s| !s.is_empty())),
            EditAction::Cut => {
                let piece = state.cut(text);
                (piece.is_some(), piece)
            }
            EditAction::Delete => (state.delete(text), None),
            EditAction::Undo => (state.restore(text, true), None),
            EditAction::Redo => (state.restore(text, false), None),
            EditAction::GoToStart => {
                state.anchor = None;
                state.cursor = 0;
                (false, None)
            }
            EditAction::GoToEnd => {
                state.anchor = None;
                state.cursor = text.len();
                (false, None)
            }
            EditAction::Paste | EditAction::Focus => (false, None),
        });
    }

    fn editor_by_id(&self, id: &str) -> Option<Path> {
        fn walk<M>(e: &Element<M>, id: &str, path: &mut Path) -> Option<Path> {
            if let Element::Editor(ed) = e {
                if ed.id.as_deref() == Some(id) {
                    return Some(path.clone());
                }
            }
            for (i, child) in e.children().iter().enumerate() {
                path.push(i as u16);
                let found = walk(child, id, path);
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        walk(&self.view, id, &mut Vec::new())
    }

    fn paste(&mut self, text: String) {
        match self.pastes.pop_front() {
            Some(Pending::App(reply)) => self.queue.push_back(reply(Some(text))),
            Some(Pending::Field(path)) => {
                if let Some((Element::Input(i), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                    let field = self.ui.fields.entry(path.clone()).or_insert_with(|| Field::new(&i.value));
                    field.sync(&i.value);
                    field.insert(&text);
                    field.sent();
                    if let Some(cb) = &i.on_change {
                        self.queue.push_back(cb(field.text.clone()));
                    }
                }
            }
            Some(Pending::Editor(path)) => {
                if let Some((Element::Editor(ed), _, _)) = locate(&self.view, &self.tree, &path, &self.ui) {
                    if !ed.read_only {
                        let _ = ed;
                        self.with_editor(&path, |state, value, _, _| {
                            state.insert(value, &text);
                            (true, None)
                        });
                    }
                }
            }
            None => {}
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Mouse { window, x, y, buttons, kind, wheel } => {
                if self.popup.as_ref().and_then(|p| p.surface.as_ref()).map(|s| s.id) == Some(window) {
                    self.popup_mouse(x, y, kind);
                } else {
                    self.mouse(x, y, buttons, kind, wheel);
                }
            }
            Event::Key { code, mods, .. } => self.key(code, mods),
            Event::Close { window } if self.popup.as_ref().and_then(|p| p.surface.as_ref()).map(|s| s.id) == Some(window) => {
                self.popup = None;
                self.repaint = true;
            }
            Event::Close { .. } => {
                let mut cx = Context::new();
                let close = self.app.close_requested(&mut cx);
                let commands = cx.take();
                self.apply(commands);
                if close {
                    self.quit = true;
                }
                self.repaint = true;
            }
            Event::Resize { width, height, .. } => {
                self.win.resize(width, height);
                self.close_popup();
                self.rebuild();
            }
            Event::Theme { light } => {
                self.th.set_light(light);
                self.app.appearance(!light);
                self.rebuild();
            }
            Event::FileChosen { request, status, path } => {
                if let Some(i) = self.files.iter().position(|(id, _)| *id == request) {
                    let (_, reply) = self.files.remove(i);
                    self.queue.push_back(reply(if status == hxproto::PICK_OK { Some(String::from(path.as_str())) } else { None }));
                }
            }
            Event::WallpaperResult { request, accepted } => {
                if let Some(i) = self.wallpapers.iter().position(|(id, _)| *id == request) {
                    let (_, reply) = self.wallpapers.remove(i);
                    self.queue.push_back(reply(accepted));
                }
            }
            Event::Selection { data, .. } => self.paste(String::from(data.as_str())),
            Event::Focus { focused: false, .. } => {
                self.ui.hover = None;
                self.repaint = true;
            }
            _ => {}
        }
    }

    fn next_timeout(&self) -> i64 {
        let now = posix::now_ms();
        let mut due: Option<u64> = self.timers.iter().map(|t| t.0).min();
        if let Some((at, _)) = self.tick {
            due = Some(due.map(|d| d.min(at)).unwrap_or(at));
        }
        match due {
            Some(d) => d.saturating_sub(now) as i64,
            None => 5000,
        }
    }

    fn fire_timers(&mut self) {
        let now = posix::now_ms();
        let mut i = 0;
        while i < self.timers.len() {
            if self.timers[i].0 <= now {
                let (_, m) = self.timers.remove(i);
                self.queue.push_back(m);
            } else {
                i += 1;
            }
        }
        if let Some((at, interval)) = self.tick {
            if at <= now {
                if let Some((_, m)) = self.app.tick() {
                    self.queue.push_back(m);
                }
                self.tick = Some((now + interval as u64, interval));
            }
        }
    }
}

pub fn run<A: App>(settings: Settings, mut app: A, wayland: bool) -> i32 {
    let Some(th) = Theme::load() else {
        report("hxtoolkit: the Nook fonts are missing (/usr/share/nook/fonts)");
        return 1;
    };
    let title = app.title();
    let (w, h) = settings.size;
    let opened = if wayland { Transport::open_wayland(&title, &settings.app_id, w, h) } else { Transport::open_hxserver(&title, w, h) };
    let mut win = match opened {
        Ok(win) => win,
        Err(e) => {
            report(&alloc::format!("hxtoolkit: {}", e));
            return 1;
        }
    };
    win.set_app_id(&settings.app_id);
    win.set_icon(settings.icon.as_deref().unwrap_or(&settings.app_id));
    if !settings.resizable {
        win.size_hints((w, h), (w, h));
    } else if let Some(min) = settings.min_size {
        win.size_hints(min, (0, 0));
    }
    app.appearance(!th.is_light());
    let mut cx = Context::new();
    app.started(&mut cx);
    let started = cx.take();
    let view = app.view();
    let mut rt = Runtime {
        back: Vec::new(),
        app,
        win,
        th,
        sizes: Sizes::default(),
        settings,
        view,
        tree: Node::default(),
        menu: Vec::new(),
        header: 0,
        ui: Interaction::default(),
        popup: None,
        pointer: (0, 0),
        modal: None,
        timers: Vec::new(),
        tick: None,
        files: Vec::new(),
        wallpapers: Vec::new(),
        pastes: VecDeque::new(),
        queue: VecDeque::new(),
        title,
        quit: false,
        repaint: true,
    };
    let _ = &rt.settings;
    rt.apply(started);
    rt.process();
    rt.rebuild();
    rt.paint();
    while !rt.quit {
        let timeout = rt.next_timeout();
        let watch = rt.app.watch();
        let event = if watch.is_empty() {
            rt.win.next_event(timeout)
        } else {
            match rt.win.next_event(0) {
                Some(e) => Some(e),
                None => {
                    let mut fds = alloc::vec![rt.win.fd()];
                    fds.extend(watch.iter().map(|(fd, _)| *fd));
                    let ready = posix::wait_readable(&fds, timeout);
                    for (i, (_, m)) in watch.into_iter().enumerate() {
                        if ready.get(i + 1).copied().unwrap_or(false) {
                            rt.queue.push_back(m);
                        }
                    }
                    rt.win.next_event(0)
                }
            }
        };
        if let Some(event) = event {
            rt.event(event);
            while let Some(event) = rt.win.next_event(0) {
                rt.event(event);
            }
        } else if !rt.win.alive() {
            break;
        }
        rt.fire_timers();
        if rt.process() {
            rt.rebuild();
        }
        if rt.repaint && !rt.quit {
            rt.paint();
        }
    }
    0
}

fn clone_item<M: Clone>(m: &MenuItem<M>) -> MenuItem<M> {
    MenuItem { label: m.label.clone(), shortcut: m.shortcut, message: m.message.clone(), checked: m.checked, separator: m.separator, children: m.children.iter().map(clone_item).collect() }
}
