use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::pixmap::Pixmap;
use crate::style::{Align, Axis, ButtonKind, Length, Padding, TextSize, Tone};

pub type Callback<T, M> = Box<dyn Fn(T) -> M>;

pub enum Element<M> {
    Text(Text),
    Button(Button<M>),
    Input(TextInput<M>),
    Editor(TextEditor<M>),
    Checkbox(Checkbox<M>),
    Switch(Switch<M>),
    Slider(Slider<M>),
    Progress(Progress),
    Dropdown(Dropdown<M>),
    List(List<M>),
    Stack(Stack<M>),
    Scroll(Scroll<M>),
    Space(Space),
    Divider,
    Image(ImageView),
    Canvas(Canvas<M>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub content: String,
    pub size: TextSize,
    pub tone: Tone,
    pub wrap: bool,
    pub align: Align,
    pub width: Length,
    pub selectable: bool,
    pub color: Option<u32>,
    pub font_family: Option<String>,
    pub bg: Option<u32>,
}

pub struct Button<M> {
    pub label: String,
    pub icon: Option<String>,
    pub kind: ButtonKind,
    pub on_press: Option<M>,
    pub width: Length,
    pub height: Length,
    pub tooltip: Option<String>,
    pub size: TextSize,
    pub text_color: Option<u32>,
}

pub struct TextInput<M> {
    pub value: String,
    pub placeholder: String,
    pub secret: bool,
    pub on_change: Option<Callback<String, M>>,
    pub on_submit: Option<M>,
    pub width: Length,
    pub focus: bool,
}

pub struct TextEditor<M> {
    pub value: String,
    pub on_change: Option<Callback<String, M>>,
    pub on_cursor: Option<Callback<(usize, usize), M>>,
    pub id: Option<String>,
    pub monospace: bool,
    pub read_only: bool,
    pub line_numbers: bool,
    pub highlight_line: bool,
    pub width: Length,
    pub height: Length,
}

pub struct Checkbox<M> {
    pub label: String,
    pub checked: bool,
    pub on_toggle: Option<Callback<bool, M>>,
}

pub struct Switch<M> {
    pub checked: bool,
    pub on_toggle: Option<Callback<bool, M>>,
}

pub struct Slider<M> {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub value: f32,
    pub on_change: Option<Callback<f32, M>>,
    pub width: Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub fraction: f32,
    pub width: Length,
}

pub struct Dropdown<M> {
    pub options: Vec<String>,
    pub selected: Option<usize>,
    pub on_select: Option<Callback<usize, M>>,
    pub width: Length,
}

pub struct List<M> {
    pub rows: Vec<Element<M>>,
    pub selected: Option<usize>,
    pub on_select: Option<Callback<usize, M>>,
    pub on_activate: Option<Callback<usize, M>>,
    pub on_context: Option<Callback<Option<usize>, M>>,
    pub width: Length,
    pub height: Length,
    pub row_height: Option<u32>,
    pub grid: Option<(u32, u32)>,
}

pub struct Stack<M> {
    pub axis: Axis,
    pub children: Vec<Element<M>>,
    pub spacing: u16,
    pub padding: Padding,
    pub align: Align,
    pub width: Length,
    pub height: Length,
    pub card: bool,
    pub sidebar: bool,
    pub bg: Option<u32>,
    pub border: Option<(u32, u16)>,
    pub radius: u16,
}

pub struct Scroll<M> {
    pub child: Box<Element<M>>,
    pub width: Length,
    pub height: Length,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub width: Length,
    pub height: Length,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImageSource {
    File(String),
    Icon(String),
    Pixels(Pixmap),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageView {
    pub source: ImageSource,
    pub width: Length,
    pub height: Length,
    pub tone: Option<Tone>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasEvent {
    Press { x: i32, y: i32, button: MouseButton },
    Release { x: i32, y: i32, button: MouseButton },
    Move { x: i32, y: i32 },
    Wheel { x: i32, y: i32, delta: i32 },
    Resize { width: u32, height: u32 },
}

pub struct Canvas<M> {
    pub pixmap: Pixmap,
    pub width: Length,
    pub height: Length,
    pub on_event: Option<Callback<CanvasEvent, Option<M>>>,
}

impl<M> Element<M> {
    pub fn kind(&self) -> &'static str {
        match self {
            Element::Text(_) => "text",
            Element::Button(_) => "button",
            Element::Input(_) => "input",
            Element::Editor(_) => "editor",
            Element::Checkbox(_) => "checkbox",
            Element::Switch(_) => "switch",
            Element::Slider(_) => "slider",
            Element::Progress(_) => "progress",
            Element::Dropdown(_) => "dropdown",
            Element::List(l) if l.grid.is_some() => "grid",
            Element::List(_) => "list",
            Element::Stack(s) if s.axis == Axis::Horizontal => "row",
            Element::Stack(_) => "column",
            Element::Scroll(_) => "scroll",
            Element::Space(_) => "space",
            Element::Divider => "divider",
            Element::Image(_) => "image",
            Element::Canvas(_) => "canvas",
        }
    }

    pub fn width(&self) -> Length {
        match self {
            Element::Text(t) => t.width,
            Element::Button(b) => b.width,
            Element::Input(i) => i.width,
            Element::Editor(e) => e.width,
            Element::Slider(s) => s.width,
            Element::Progress(p) => p.width,
            Element::Dropdown(d) => d.width,
            Element::List(l) => l.width,
            Element::Stack(s) => s.width,
            Element::Scroll(s) => s.width,
            Element::Space(s) => s.width,
            Element::Image(i) => i.width,
            Element::Canvas(c) => c.width,
            Element::Divider | Element::Checkbox(_) | Element::Switch(_) => Length::Shrink,
        }
    }

    pub fn height(&self) -> Length {
        match self {
            Element::Editor(e) => e.height,
            Element::List(l) => l.height,
            Element::Stack(s) => s.height,
            Element::Scroll(s) => s.height,
            Element::Space(s) => s.height,
            Element::Image(i) => i.height,
            Element::Canvas(c) => c.height,
            _ => Length::Shrink,
        }
    }

    pub fn children(&self) -> &[Element<M>] {
        match self {
            Element::Stack(s) => &s.children,
            Element::List(l) => &l.rows,
            Element::Scroll(s) => core::slice::from_ref(&s.child),
            _ => &[],
        }
    }

    pub fn release_pixmaps(&mut self) {
        match self {
            Element::Canvas(c) => c.pixmap.release(),
            Element::Image(i) => {
                if let ImageSource::Pixels(p) = &mut i.source {
                    p.release();
                }
            }
            Element::Stack(s) => s.children.iter_mut().for_each(|c| c.release_pixmaps()),
            Element::List(l) => l.rows.iter_mut().for_each(|c| c.release_pixmaps()),
            Element::Scroll(s) => s.child.release_pixmaps(),
            _ => {}
        }
    }

    pub fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> Element<N>
    where
        M: 'static,
    {
        let f: Rc<dyn Fn(M) -> N> = Rc::new(f);
        map_element(self, &f)
    }
}

fn map_callback<T: 'static, M: 'static, N: 'static>(cb: Option<Callback<T, M>>, f: &Rc<dyn Fn(M) -> N>) -> Option<Callback<T, N>> {
    cb.map(|cb| {
        let f = f.clone();
        Box::new(move |v: T| f(cb(v))) as Callback<T, N>
    })
}

fn map_element<M: 'static, N: 'static>(e: Element<M>, f: &Rc<dyn Fn(M) -> N>) -> Element<N> {
    match e {
        Element::Text(t) => Element::Text(t),
        Element::Button(b) => Element::Button(Button { label: b.label, icon: b.icon, kind: b.kind, on_press: b.on_press.map(|m| f(m)), width: b.width, height: b.height, tooltip: b.tooltip, size: b.size, text_color: b.text_color }),
        Element::Input(i) => Element::Input(TextInput {
            value: i.value,
            placeholder: i.placeholder,
            secret: i.secret,
            on_change: map_callback(i.on_change, f),
            on_submit: i.on_submit.map(|m| f(m)),
            width: i.width,
            focus: i.focus,
        }),
        Element::Editor(e) => Element::Editor(TextEditor {
            value: e.value,
            on_change: map_callback(e.on_change, f),
            on_cursor: map_callback(e.on_cursor, f),
            id: e.id,
            monospace: e.monospace,
            read_only: e.read_only,
            line_numbers: e.line_numbers,
            highlight_line: e.highlight_line,
            width: e.width,
            height: e.height,
        }),
        Element::Checkbox(c) => Element::Checkbox(Checkbox { label: c.label, checked: c.checked, on_toggle: map_callback(c.on_toggle, f) }),
        Element::Switch(s) => Element::Switch(Switch { checked: s.checked, on_toggle: map_callback(s.on_toggle, f) }),
        Element::Slider(s) => Element::Slider(Slider { min: s.min, max: s.max, step: s.step, value: s.value, on_change: map_callback(s.on_change, f), width: s.width }),
        Element::Progress(p) => Element::Progress(p),
        Element::Dropdown(d) => Element::Dropdown(Dropdown { options: d.options, selected: d.selected, on_select: map_callback(d.on_select, f), width: d.width }),
        Element::List(l) => Element::List(List {
            rows: l.rows.into_iter().map(|r| map_element(r, f)).collect(),
            selected: l.selected,
            on_select: map_callback(l.on_select, f),
            on_activate: map_callback(l.on_activate, f),
            on_context: map_callback(l.on_context, f),
            width: l.width,
            height: l.height,
            row_height: l.row_height,
            grid: l.grid,
        }),
        Element::Stack(s) => Element::Stack(Stack {
            axis: s.axis,
            children: s.children.into_iter().map(|c| map_element(c, f)).collect(),
            spacing: s.spacing,
            padding: s.padding,
            align: s.align,
            width: s.width,
            height: s.height,
            card: s.card,
            sidebar: s.sidebar,
            bg: s.bg,
            border: s.border,
            radius: s.radius,
        }),
        Element::Scroll(s) => Element::Scroll(Scroll { child: Box::new(map_element(*s.child, f)), width: s.width, height: s.height }),
        Element::Space(s) => Element::Space(s),
        Element::Divider => Element::Divider,
        Element::Image(i) => Element::Image(i),
        Element::Canvas(c) => {
            let on_event = c.on_event.map(|cb| {
                let f = f.clone();
                Box::new(move |ev: CanvasEvent| cb(ev).map(|m| f(m))) as Callback<CanvasEvent, Option<N>>
            });
            Element::Canvas(Canvas { pixmap: c.pixmap, width: c.width, height: c.height, on_event })
        }
    }
}

pub fn text(content: impl Into<String>) -> Text {
    Text { content: content.into(), size: TextSize::Body, tone: Tone::Normal, wrap: false, align: Align::Start, width: Length::Shrink, selectable: false, color: None, font_family: None, bg: None }
}

pub fn title(content: impl Into<String>) -> Text {
    text(content).size(TextSize::Title)
}

pub fn heading(content: impl Into<String>) -> Text {
    text(content).size(TextSize::Heading1)
}

pub fn subheading(content: impl Into<String>) -> Text {
    text(content).size(TextSize::Heading2)
}

impl Text {
    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn dim(self) -> Self {
        self.tone(Tone::Dim)
    }

    pub fn small(self) -> Self {
        self.size(TextSize::Small)
    }

    pub fn strong(self) -> Self {
        self.size(TextSize::Strong)
    }

    pub fn mono(self) -> Self {
        self.size(TextSize::Mono)
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    pub fn font(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    pub fn bg(mut self, bg: u32) -> Self {
        self.bg = Some(bg);
        self
    }

    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn fill(self) -> Self {
        self.width(Length::Fill)
    }

    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }
}

pub fn button<M>(label: impl Into<String>) -> Button<M> {
    Button { label: label.into(), icon: None, kind: ButtonKind::Secondary, on_press: None, width: Length::Shrink, height: Length::Shrink, tooltip: None, size: TextSize::Strong, text_color: None }
}

pub fn link<M>(label: impl Into<String>) -> Button<M> {
    Button { label: label.into(), icon: None, kind: ButtonKind::Link, on_press: None, width: Length::Shrink, height: Length::Shrink, tooltip: None, size: TextSize::Body, text_color: None }
}

pub fn icon_button<M>(icon: impl Into<String>, tooltip: impl Into<String>) -> Button<M> {
    Button { label: String::new(), icon: Some(icon.into()), kind: ButtonKind::Flat, on_press: None, width: Length::Shrink, height: Length::Shrink, tooltip: Some(tooltip.into()), size: TextSize::Strong, text_color: None }
}

impl<M> Button<M> {
    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn primary(self) -> Self {
        self.kind(ButtonKind::Primary)
    }

    pub fn danger(self) -> Self {
        self.kind(ButtonKind::Danger)
    }

    pub fn flat(self) -> Self {
        self.kind(ButtonKind::Flat)
    }

    pub fn link(self) -> Self {
        self.kind(ButtonKind::Link)
    }

    pub fn text_color(mut self, color: u32) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    pub fn fill(self) -> Self {
        self.width(Length::Fill)
    }
}

pub fn input<M>(value: impl Into<String>, placeholder: impl Into<String>, on_change: impl Fn(String) -> M + 'static) -> TextInput<M> {
    TextInput { value: value.into(), placeholder: placeholder.into(), secret: false, on_change: Some(Box::new(on_change)), on_submit: None, width: Length::Fill, focus: false }
}

impl<M> TextInput<M> {
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    pub fn on_submit(mut self, message: M) -> Self {
        self.on_submit = Some(message);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn focused(mut self) -> Self {
        self.focus = true;
        self
    }
}

pub fn editor<M>(value: impl Into<String>, on_change: impl Fn(String) -> M + 'static) -> TextEditor<M> {
    TextEditor { value: value.into(), on_change: Some(Box::new(on_change)), on_cursor: None, id: None, monospace: false, read_only: false, line_numbers: false, highlight_line: false, width: Length::Fill, height: Length::Fill }
}

pub fn viewer<M>(value: impl Into<String>) -> TextEditor<M> {
    TextEditor { value: value.into(), on_change: None, on_cursor: None, id: None, monospace: false, read_only: true, line_numbers: false, highlight_line: false, width: Length::Fill, height: Length::Fill }
}

impl<M> TextEditor<M> {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn on_cursor(mut self, f: impl Fn((usize, usize)) -> M + 'static) -> Self {
        self.on_cursor = Some(Box::new(f));
        self
    }

    pub fn line_numbers(mut self, on: bool) -> Self {
        self.line_numbers = on;
        self
    }

    pub fn highlight_line(mut self, on: bool) -> Self {
        self.highlight_line = on;
        self
    }

    pub fn monospace(mut self) -> Self {
        self.monospace = true;
        self
    }

    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

pub fn checkbox<M>(label: impl Into<String>, checked: bool, on_toggle: impl Fn(bool) -> M + 'static) -> Checkbox<M> {
    Checkbox { label: label.into(), checked, on_toggle: Some(Box::new(on_toggle)) }
}

pub fn switch<M>(checked: bool, on_toggle: impl Fn(bool) -> M + 'static) -> Switch<M> {
    Switch { checked, on_toggle: Some(Box::new(on_toggle)) }
}

pub fn slider<M>(range: core::ops::RangeInclusive<f32>, value: f32, on_change: impl Fn(f32) -> M + 'static) -> Slider<M> {
    Slider { min: *range.start(), max: *range.end(), step: 0.0, value, on_change: Some(Box::new(on_change)), width: Length::Fixed(180) }
}

impl<M> Slider<M> {
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn snap(&self, value: f32) -> f32 {
        let clamped = value.clamp(self.min.min(self.max), self.max.max(self.min));
        if self.step > 0.0 {
            let steps = ((clamped - self.min) / self.step + 0.5) as i64;
            (self.min + steps as f32 * self.step).clamp(self.min.min(self.max), self.max.max(self.min))
        } else {
            clamped
        }
    }

    pub fn fraction(&self) -> f32 {
        if self.max == self.min {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }
}

pub fn progress(fraction: f32) -> Progress {
    Progress { fraction: fraction.clamp(0.0, 1.0), width: Length::Fill }
}

impl Progress {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

pub fn dropdown<M, S: Into<String>>(options: impl IntoIterator<Item = S>, selected: Option<usize>, on_select: impl Fn(usize) -> M + 'static) -> Dropdown<M> {
    Dropdown { options: options.into_iter().map(Into::into).collect(), selected, on_select: Some(Box::new(on_select)), width: Length::Shrink }
}

impl<M> Dropdown<M> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

pub fn list<M>(rows: Vec<Element<M>>) -> List<M> {
    List { rows, selected: None, on_select: None, on_activate: None, on_context: None, width: Length::Fill, height: Length::Fill, row_height: None, grid: None }
}

impl<M> List<M> {
    pub fn selected(mut self, index: Option<usize>) -> Self {
        self.selected = index;
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize) -> M + 'static) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    pub fn on_activate(mut self, f: impl Fn(usize) -> M + 'static) -> Self {
        self.on_activate = Some(Box::new(f));
        self
    }

    pub fn on_context(mut self, f: impl Fn(Option<usize>) -> M + 'static) -> Self {
        self.on_context = Some(Box::new(f));
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn row_height(mut self, px: u32) -> Self {
        self.row_height = Some(px);
        self
    }

    pub fn grid(mut self, cell_width: u32, cell_height: u32) -> Self {
        self.grid = Some((cell_width, cell_height));
        self
    }
}

pub fn column<M>(children: Vec<Element<M>>) -> Stack<M> {
    Stack { axis: Axis::Vertical, children, spacing: 8, padding: Padding::ZERO, align: Align::Start, width: Length::Fill, height: Length::Shrink, card: false, sidebar: false, bg: None, border: None, radius: 0 }
}

pub fn row<M>(children: Vec<Element<M>>) -> Stack<M> {
    Stack { axis: Axis::Horizontal, children, spacing: 8, padding: Padding::ZERO, align: Align::Center, width: Length::Fill, height: Length::Shrink, card: false, sidebar: false, bg: None, border: None, radius: 0 }
}

pub fn container<M>(child: impl Into<Element<M>>) -> Stack<M> {
    column(alloc::vec![child.into()]).width(Length::Fill).height(Length::Fill)
}

impl<M> Stack<M> {
    pub fn spacing(mut self, px: u16) -> Self {
        self.spacing = px;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn bg(mut self, color: u32) -> Self {
        self.bg = Some(color);
        self
    }

    pub fn border(mut self, color: u32, width: u16) -> Self {
        self.border = Some((color, width));
        self
    }

    pub fn radius(mut self, r: u16) -> Self {
        self.radius = r;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn fill(self) -> Self {
        self.width(Length::Fill).height(Length::Fill)
    }

    pub fn shrink(self) -> Self {
        self.width(Length::Shrink)
    }

    pub fn card(mut self) -> Self {
        self.card = true;
        if self.padding == Padding::ZERO {
            self.padding = Padding::all(14);
        }
        self
    }

    pub fn sidebar(mut self) -> Self {
        self.sidebar = true;
        if self.padding == Padding::ZERO {
            self.padding = Padding::all(18);
        }
        self
    }

    pub fn push(mut self, child: impl Into<Element<M>>) -> Self {
        self.children.push(child.into());
        self
    }
}

pub fn scroll<M>(child: impl Into<Element<M>>) -> Scroll<M> {
    Scroll { child: Box::new(child.into()), width: Length::Fill, height: Length::Fill }
}

impl<M> Scroll<M> {
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

pub fn space() -> Space {
    Space { width: Length::Fill, height: Length::Fill }
}

pub fn hspace(px: u32) -> Space {
    Space { width: Length::Fixed(px), height: Length::Shrink }
}

pub fn vspace(px: u32) -> Space {
    Space { width: Length::Shrink, height: Length::Fixed(px) }
}

pub fn divider<M>() -> Element<M> {
    Element::Divider
}

pub fn image(path: impl Into<String>) -> ImageView {
    ImageView { source: ImageSource::File(path.into()), width: Length::Shrink, height: Length::Shrink, tone: None }
}

pub fn icon(name: impl Into<String>) -> ImageView {
    ImageView { source: ImageSource::Icon(name.into()), width: Length::Shrink, height: Length::Shrink, tone: None }
}

pub fn picture(pixmap: Pixmap) -> ImageView {
    ImageView { source: ImageSource::Pixels(pixmap), width: Length::Shrink, height: Length::Shrink, tone: None }
}

impl ImageView {
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    pub fn size(mut self, width: impl Into<Length>, height: impl Into<Length>) -> Self {
        self.width = width.into();
        self.height = height.into();
        self
    }
}

pub fn canvas<M>(pixmap: Pixmap) -> Canvas<M> {
    let (w, h) = (pixmap.width(), pixmap.height());
    Canvas { pixmap, width: Length::Fixed(w), height: Length::Fixed(h), on_event: None }
}

impl<M> Canvas<M> {
    pub fn on_event(mut self, f: impl Fn(CanvasEvent) -> Option<M> + 'static) -> Self {
        self.on_event = Some(Box::new(f));
        self
    }

    pub fn size(mut self, width: impl Into<Length>, height: impl Into<Length>) -> Self {
        self.width = width.into();
        self.height = height.into();
        self
    }

    pub fn fill(self) -> Self {
        self.size(Length::Fill, Length::Fill)
    }
}

macro_rules! into_element {
    ($($ty:ident => $variant:ident),* $(,)?) => {
        $(impl<M> From<$ty<M>> for Element<M> {
            fn from(v: $ty<M>) -> Element<M> {
                Element::$variant(v)
            }
        })*
    };
}

into_element!(Button => Button, TextInput => Input, TextEditor => Editor, Checkbox => Checkbox, Switch => Switch, Slider => Slider, Dropdown => Dropdown, List => List, Stack => Stack, Scroll => Scroll, Canvas => Canvas);

impl<M> From<Text> for Element<M> {
    fn from(v: Text) -> Element<M> {
        Element::Text(v)
    }
}

impl<M> From<Progress> for Element<M> {
    fn from(v: Progress) -> Element<M> {
        Element::Progress(v)
    }
}

impl<M> From<Space> for Element<M> {
    fn from(v: Space) -> Element<M> {
        Element::Space(v)
    }
}

impl<M> From<ImageView> for Element<M> {
    fn from(v: ImageView) -> Element<M> {
        Element::Image(v)
    }
}

impl<M> From<&str> for Element<M> {
    fn from(v: &str) -> Element<M> {
        Element::Text(text(v))
    }
}

impl<M> From<String> for Element<M> {
    fn from(v: String) -> Element<M> {
        Element::Text(text(v))
    }
}

#[macro_export]
macro_rules! column {
    () => { $crate::column($crate::__private::Vec::new()) };
    ($($child:expr),+ $(,)?) => { $crate::column($crate::__private::vec![$($crate::Element::from($child)),+]) };
}

#[macro_export]
macro_rules! row {
    () => { $crate::row($crate::__private::Vec::new()) };
    ($($child:expr),+ $(,)?) => { $crate::row($crate::__private::vec![$($crate::Element::from($child)),+]) };
}
