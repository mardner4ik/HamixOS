use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::app::{shortcut_message, App, Command, Context, EditAction, FileDialog, MenuItem, Settings};
use crate::element::{Callback, CanvasEvent, Element, ImageSource, MouseButton};
use crate::keys::{Key, KeyPress, Modifiers};
use crate::pixmap::Pixmap;
use crate::style::{Align, Axis, ButtonKind, Length, TextSize, Tone};

pub fn available() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"].iter().any(|v| std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false))
}

fn system_prefers_dark() -> bool {
    if gtk::Settings::default()
        .map(|settings| {
            settings.is_gtk_application_prefer_dark_theme()
                || settings.gtk_theme_name().map(|name| name.to_lowercase().contains("dark")).unwrap_or(false)
        })
        .unwrap_or(false)
    {
        return true;
    }

    if std::env::var("GTK_THEME").map(|theme| theme.to_lowercase().contains("dark")).unwrap_or(false) {
        return true;
    }

    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let home = std::path::PathBuf::from(home);
    for path in [home.join(".config/gtk-4.0/settings.ini"), home.join(".config/gtk-3.0/settings.ini")] {
        if let Ok(settings) = std::fs::read_to_string(path) {
            if settings.lines().any(|line| {
                let Some((key, value)) = line.split_once('=') else {
                    return false;
                };
                let value = value.trim().trim_matches('"').to_lowercase();
                (key.trim() == "gtk-application-prefer-dark-theme" && matches!(value.as_str(), "1" | "true"))
                    || (key.trim() == "gtk-theme-name" && value.contains("dark"))
            }) {
                return true;
            }
        }
    }

    std::fs::read_to_string(home.join(".config/kdeglobals"))
        .map(|settings| {
            settings.lines().any(|line| {
                line.split_once('=')
                    .map(|(key, value)| key.trim() == "ColorScheme" && value.to_lowercase().contains("dark"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

type Slot<T> = Rc<RefCell<Option<T>>>;

fn slot<T>() -> Slot<T> {
    Rc::new(RefCell::new(None))
}

struct Dispatch<M> {
    queue: RefCell<VecDeque<M>>,
    flush: RefCell<Option<Box<dyn Fn()>>>,
    scheduled: Cell<bool>,
}

impl<M: 'static> Dispatch<M> {
    fn push(self: &Rc<Self>, message: M) {
        self.queue.borrow_mut().push_back(message);
        if !self.scheduled.replace(true) {
            let me = self.clone();
            glib::idle_add_local_once(move || {
                me.scheduled.set(false);
                if let Some(flush) = me.flush.borrow().as_ref() {
                    flush();
                }
            });
        }
    }
}

struct Handlers<M> {
    press: Slot<M>,
    text: Slot<Callback<String, M>>,
    submit: Slot<M>,
    toggle: Slot<Callback<bool, M>>,
    value: Slot<Callback<f32, M>>,
    index: Slot<Callback<usize, M>>,
    activate: Slot<Callback<usize, M>>,
    context: Slot<Callback<Option<usize>, M>>,
    canvas: Slot<Callback<CanvasEvent, Option<M>>>,
    cursor: Slot<Callback<(usize, usize), M>>,
    pixmap: Rc<RefCell<Pixmap>>,
    guard: Rc<Cell<bool>>,
}

impl<M> Handlers<M> {
    fn new() -> Handlers<M> {
        Handlers {
            press: slot(),
            text: slot(),
            submit: slot(),
            toggle: slot(),
            value: slot(),
            index: slot(),
            activate: slot(),
            context: slot(),
            canvas: slot(),
            cursor: slot(),
            pixmap: Rc::new(RefCell::new(Pixmap::new(0, 0))),
            guard: Rc::new(Cell::new(false)),
        }
    }
}

struct Node<M> {
    kind: &'static str,
    widget: gtk::Widget,
    inner: Option<gtk::Widget>,
    children: Vec<Node<M>>,
    handlers: Handlers<M>,
    options: Vec<String>,
    custom_class: Option<String>,
}

struct DynamicCss {
    provider: gtk::CssProvider,
    rules: String,
    classes: HashSet<String>,
    installed: bool,
}

thread_local! {
    static DYNAMIC_CSS: RefCell<Option<DynamicCss>> = const { RefCell::new(None) };
}

fn apply_dynamic_style(widget: &gtk::Widget, current: &mut Option<String>, declarations: &str) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    declarations.hash(&mut hasher);
    let class = format!("hxtk-{:016x}", hasher.finish());
    if current.as_deref() == Some(class.as_str()) {
        widget.add_css_class(&class);
        return;
    }
    if let Some(old) = current.take() {
        widget.remove_css_class(&old);
    }
    DYNAMIC_CSS.with(|dynamic| {
        let mut dynamic = dynamic.borrow_mut();
        let css = dynamic.get_or_insert_with(|| DynamicCss {
            provider: gtk::CssProvider::new(),
            rules: String::from("popover.menu, popover.menu label, popover.menu * { font-weight: normal; }\n"),
            classes: HashSet::new(),
            installed: false,
        });
        if css.classes.insert(class.clone()) {
            css.rules.push_str(&format!(".{} {{ {} }}\n", class, declarations));
            css.provider.load_from_data(&css.rules);
        }
        if !css.installed {
            gtk::style_context_add_provider_for_display(
                &widget.display(),
                &css.provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            css.installed = true;
        }
    });
    widget.add_css_class(&class);
    *current = Some(class);
}

fn icon_name(name: &str) -> String {
    if let Some(kind) = name.strip_prefix("files/") {
        return String::from(match kind {
            "folder" => "folder",
            "image" => "image-x-generic",
            "video" => "video-x-generic",
            "exec" => "application-x-executable",
            "script" => "text-x-script",
            "device" => "drive-harddisk",
            _ => "text-x-generic",
        });
    }
    let name = name.trim_start_matches("ui/");
    let mapped = match name {
        "zoom-in" => "zoom-in-symbolic",
        "check" => "object-select-symbolic",
        "user" => "avatar-default-symbolic",
        "session" => "computer-symbolic",
        "logo" => "computer",
        "zoom-out" => "zoom-out-symbolic",
        "fit" => "zoom-fit-best-symbolic",
        "run" => "system-run-symbolic",
        "disk" => "drive-harddisk-symbolic",
        "add" | "plus" | "new" => "list-add-symbolic",
        "remove" | "minus" => "list-remove-symbolic",
        "back" | "previous" => "go-previous-symbolic",
        "forward" | "next" => "go-next-symbolic",
        "up" => "go-up-symbolic",
        "down" => "go-down-symbolic",
        "refresh" | "reload" => "view-refresh-symbolic",
        "search" => "system-search-symbolic",
        "settings" | "preferences" => "emblem-system-symbolic",
        "delete" | "trash" => "user-trash-symbolic",
        "folder" => "folder-symbolic",
        "file" => "text-x-generic-symbolic",
        "home" => "user-home-symbolic",
        "play" => "media-playback-start-symbolic",
        "pause" => "media-playback-pause-symbolic",
        "stop" => "media-playback-stop-symbolic",
        "close" => "window-close-symbolic",
        "menu" => "open-menu-symbolic",
        "edit" => "document-edit-symbolic",
        "save" => "document-save-symbolic",
        "open" => "document-open-symbolic",
        "copy" => "edit-copy-symbolic",
        "paste" => "edit-paste-symbolic",
        "info" => "dialog-information-symbolic",
        "warning" => "dialog-warning-symbolic",
        other => return other.to_string(),
    };
    mapped.to_string()
}

fn text_classes(size: TextSize, tone: Tone) -> Vec<&'static str> {
    let mut out = Vec::new();
    match size {
        TextSize::Small => out.push("caption"),
        TextSize::Strong => out.push("heading"),
        TextSize::Title | TextSize::Heading2 => out.push("title-2"),
        TextSize::Display | TextSize::Heading1 => out.push("title-1"),
        TextSize::Heading3 => out.push("heading"),
        TextSize::Mono => out.push("monospace"),
        TextSize::Body => {}
    }
    match tone {
        Tone::Dim | Tone::Faint => out.push("dim-label"),
        Tone::Accent => out.push("accent"),
        Tone::Danger => out.push("error"),
        Tone::Success => out.push("success"),
        Tone::Warning => out.push("warning"),
        Tone::Normal => {}
    }
    out
}

fn apply_size(widget: &gtk::Widget, width: Length, height: Length) {
    widget.set_hexpand(width.fills());
    widget.set_vexpand(height.fills());
    let w = if let Length::Fixed(px) = width { px as i32 } else { -1 };
    let h = if let Length::Fixed(px) = height { px as i32 } else { -1 };
    widget.set_size_request(w, h);
    if width.fills() {
        widget.set_halign(gtk::Align::Fill);
    }
    if height.fills() {
        widget.set_valign(gtk::Align::Fill);
    }
}

fn reset_scrolls<M>(node: &Node<M>) {
    if let Some(scroll) = node.widget.downcast_ref::<gtk::ScrolledWindow>() {
        scroll.hadjustment().set_value(0.0);
        scroll.vadjustment().set_value(0.0);
    }
    for child in &node.children {
        reset_scrolls(child);
    }
}

fn gtk_align(align: Align) -> gtk::Align {
    match align {
        Align::Start => gtk::Align::Start,
        Align::Center => gtk::Align::Center,
        Align::End => gtk::Align::End,
        Align::Stretch => gtk::Align::Fill,
    }
}

fn texture(pixmap: &Pixmap) -> gdk::MemoryTexture {
    let mut bytes = Vec::with_capacity(pixmap.pixels().len() * 4);
    for px in pixmap.pixels() {
        bytes.extend_from_slice(&px.to_le_bytes());
    }
    gdk::MemoryTexture::new(pixmap.width() as i32, pixmap.height() as i32, gdk::MemoryFormat::B8g8r8a8, &glib::Bytes::from_owned(bytes), pixmap.width() as usize * 4)
}

struct Builder<M: 'static> {
    dispatch: Rc<Dispatch<M>>,
    editors: RefCell<HashMap<String, gtk::TextView>>,
}

impl<M: Clone + 'static> Builder<M> {
    fn create(&self, kind: &'static str, axis: Axis) -> Node<M> {
        let handlers = Handlers::new();
        let d = self.dispatch.clone();
        let mut inner = None;
        let widget: gtk::Widget = match kind {
            "text" => gtk::Label::new(None).upcast(),
            "button" => {
                let b = gtk::Button::new();
                let press = handlers.press.clone();
                b.connect_clicked(move |_| {
                    if let Some(m) = press.borrow().clone() {
                        d.push(m);
                    }
                });
                b.upcast()
            }
            "input" => {
                let e = gtk::Entry::new();
                let (text, guard, d2) = (handlers.text.clone(), handlers.guard.clone(), d.clone());
                e.connect_changed(move |e| {
                    if !guard.get() {
                        if let Some(cb) = text.borrow().as_ref() {
                            d2.push(cb(e.text().to_string()));
                        }
                    }
                });
                let submit = handlers.submit.clone();
                e.connect_activate(move |_| {
                    if let Some(m) = submit.borrow().clone() {
                        d.push(m);
                    }
                });
                e.upcast()
            }
            "editor" => {
                let view = gtk::TextView::new();
                view.set_wrap_mode(gtk::WrapMode::WordChar);
                view.set_left_margin(8);
                view.set_right_margin(8);
                view.set_top_margin(6);
                let (text, guard, d2) = (handlers.text.clone(), handlers.guard.clone(), d.clone());
                view.buffer().connect_changed(move |buf| {
                    if !guard.get() {
                        if let Some(cb) = text.borrow().as_ref() {
                            let (a, b) = buf.bounds();
                            d2.push(cb(buf.text(&a, &b, false).to_string()));
                        }
                    }
                });
                let (cursor, guard) = (handlers.cursor.clone(), handlers.guard.clone());
                view.buffer().connect_cursor_position_notify(move |buf| {
                    if !guard.get() {
                        if let Some(cb) = cursor.borrow().as_ref() {
                            let it = buf.iter_at_offset(buf.cursor_position());
                            d.push(cb((it.line() as usize + 1, it.line_offset() as usize + 1)));
                        }
                    }
                });
                let scroller = gtk::ScrolledWindow::new();
                scroller.set_child(Some(&view));
                scroller.add_css_class("frame");
                inner = Some(view.upcast());
                scroller.upcast()
            }
            "checkbox" => {
                let c = gtk::CheckButton::new();
                let (toggle, guard) = (handlers.toggle.clone(), handlers.guard.clone());
                c.connect_toggled(move |c| {
                    if !guard.get() {
                        if let Some(cb) = toggle.borrow().as_ref() {
                            d.push(cb(c.is_active()));
                        }
                    }
                });
                c.upcast()
            }
            "switch" => {
                let s = gtk::Switch::new();
                s.set_valign(gtk::Align::Center);
                let (toggle, guard) = (handlers.toggle.clone(), handlers.guard.clone());
                s.connect_active_notify(move |s| {
                    if !guard.get() {
                        if let Some(cb) = toggle.borrow().as_ref() {
                            d.push(cb(s.is_active()));
                        }
                    }
                });
                s.upcast()
            }
            "slider" => {
                let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
                s.set_draw_value(false);
                let (value, guard) = (handlers.value.clone(), handlers.guard.clone());
                s.connect_value_changed(move |s| {
                    if !guard.get() {
                        if let Some(cb) = value.borrow().as_ref() {
                            d.push(cb(s.value() as f32));
                        }
                    }
                });
                s.upcast()
            }
            "progress" => {
                let p = gtk::ProgressBar::new();
                p.set_valign(gtk::Align::Center);
                p.upcast()
            }
            "dropdown" => {
                let dd = gtk::DropDown::from_strings(&[]);
                let (index, guard) = (handlers.index.clone(), handlers.guard.clone());
                dd.connect_selected_notify(move |dd| {
                    let i = dd.selected();
                    if !guard.get() && i != gtk::INVALID_LIST_POSITION {
                        if let Some(cb) = index.borrow().as_ref() {
                            d.push(cb(i as usize));
                        }
                    }
                });
                dd.upcast()
            }
            "grid" => {
                let flow = gtk::FlowBox::new();
                flow.set_selection_mode(gtk::SelectionMode::Single);
                flow.set_activate_on_single_click(false);
                flow.set_homogeneous(true);
                flow.set_valign(gtk::Align::Start);
                let (index, guard, d2) = (handlers.index.clone(), handlers.guard.clone(), d.clone());
                flow.connect_selected_children_changed(move |flow| {
                    if guard.get() {
                        return;
                    }
                    if let Some(child) = flow.selected_children().first() {
                        if let Some(cb) = index.borrow().as_ref() {
                            d2.push(cb(child.index() as usize));
                        }
                    }
                });
                let activate = handlers.activate.clone();
                let d3 = d.clone();
                flow.connect_child_activated(move |_, child| {
                    if let Some(cb) = activate.borrow().as_ref() {
                        d3.push(cb(child.index() as usize));
                    }
                });
                let secondary = gtk::GestureClick::new();
                secondary.set_button(3);
                let context = handlers.context.clone();
                let weak = flow.downgrade();
                secondary.connect_pressed(move |_, _, x, y| {
                    let Some(flow) = weak.upgrade() else {
                        return;
                    };
                    let child = flow.child_at_pos(x as i32, y as i32);
                    if let Some(child) = &child {
                        flow.select_child(child);
                    }
                    if let Some(cb) = context.borrow().as_ref() {
                        d.push(cb(child.map(|c| c.index() as usize)));
                    }
                });
                flow.add_controller(secondary);
                let scroller = gtk::ScrolledWindow::new();
                scroller.set_child(Some(&flow));
                inner = Some(flow.upcast());
                scroller.upcast()
            }
            "list" => {
                let list = gtk::ListBox::new();
                list.set_selection_mode(gtk::SelectionMode::Single);
                list.set_activate_on_single_click(false);
                list.add_css_class("navigation-sidebar");
                let (index, guard, d2) = (handlers.index.clone(), handlers.guard.clone(), d.clone());
                list.connect_row_selected(move |_, row| {
                    if let (false, Some(row)) = (guard.get(), row) {
                        if let Some(cb) = index.borrow().as_ref() {
                            d2.push(cb(row.index() as usize));
                        }
                    }
                });
                let activate = handlers.activate.clone();
                let d3 = d.clone();
                list.connect_row_activated(move |_, row| {
                    if let Some(cb) = activate.borrow().as_ref() {
                        d3.push(cb(row.index() as usize));
                    }
                });
                let secondary = gtk::GestureClick::new();
                secondary.set_button(3);
                let context = handlers.context.clone();
                let weak_list = list.downgrade();
                secondary.connect_pressed(move |_, _, _, y| {
                    let Some(list) = weak_list.upgrade() else {
                        return;
                    };
                    let row = list.row_at_y(y as i32);
                    if let Some(row) = &row {
                        list.select_row(Some(row));
                    }
                    if let Some(cb) = context.borrow().as_ref() {
                        d.push(cb(row.map(|r| r.index() as usize)));
                    }
                });
                list.add_controller(secondary);
                let scroller = gtk::ScrolledWindow::new();
                scroller.set_child(Some(&list));
                inner = Some(list.upcast());
                scroller.upcast()
            }
            "row" => gtk::Box::new(gtk::Orientation::Horizontal, 8).upcast(),
            "column" => gtk::Box::new(gtk::Orientation::Vertical, 8).upcast(),
            "scroll" => gtk::ScrolledWindow::new().upcast(),
            "space" => gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast(),
            "divider" => gtk::Separator::new(if axis == Axis::Horizontal { gtk::Orientation::Vertical } else { gtk::Orientation::Horizontal }).upcast(),
            "image" => gtk::Picture::new().upcast(),
            "canvas" => self.canvas(&handlers),
            _ => gtk::Box::new(gtk::Orientation::Vertical, 0).upcast(),
        };
        Node { kind, widget, inner, children: Vec::new(), handlers, options: Vec::new(), custom_class: None }
    }

    fn canvas(&self, handlers: &Handlers<M>) -> gtk::Widget {
        let area = gtk::DrawingArea::new();
        let pixmap = handlers.pixmap.clone();
        area.set_draw_func(move |_, cr, _, _| {
            let px = pixmap.borrow();
            if px.width() == 0 || px.height() == 0 {
                return;
            }
            let mut data = Vec::with_capacity(px.pixels().len() * 4);
            for p in px.pixels() {
                data.extend_from_slice(&p.to_ne_bytes());
            }
            if let Ok(surface) = gtk::cairo::ImageSurface::create_for_data(data, gtk::cairo::Format::ARgb32, px.width() as i32, px.height() as i32, px.width() as i32 * 4) {
                let _ = cr.set_source_surface(&surface, 0.0, 0.0);
                let _ = cr.paint();
            }
        });
        let emit = {
            let (canvas, d) = (handlers.canvas.clone(), self.dispatch.clone());
            Rc::new(move |ev: CanvasEvent| {
                let message = canvas.borrow().as_ref().and_then(|cb| cb(ev));
                if let Some(m) = message {
                    d.push(m);
                }
            })
        };
        let e = emit.clone();
        area.connect_resize(move |_, w, h| e(CanvasEvent::Resize { width: w.max(0) as u32, height: h.max(0) as u32 }));
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let button_of = |g: &gtk::GestureClick| match g.current_button() {
            3 => MouseButton::Right,
            2 => MouseButton::Middle,
            _ => MouseButton::Left,
        };
        let e = emit.clone();
        click.connect_pressed(move |g, _, x, y| e(CanvasEvent::Press { x: x as i32, y: y as i32, button: button_of(g) }));
        let e = emit.clone();
        click.connect_released(move |g, _, x, y| e(CanvasEvent::Release { x: x as i32, y: y as i32, button: button_of(g) }));
        area.add_controller(click);
        let motion = gtk::EventControllerMotion::new();
        let e = emit.clone();
        let last = Rc::new(Cell::new((0i32, 0i32)));
        let l = last.clone();
        motion.connect_motion(move |_, x, y| {
            l.set((x as i32, y as i32));
            e(CanvasEvent::Move { x: x as i32, y: y as i32 })
        });
        area.add_controller(motion);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _, dy| {
            let (x, y) = last.get();
            emit(CanvasEvent::Wheel { x, y, delta: if dy > 0.0 { 1 } else { -1 } });
            glib::Propagation::Stop
        });
        area.add_controller(scroll);
        area.set_focusable(true);
        area.upcast()
    }

    fn reconcile(&self, old: Option<Node<M>>, e: Element<M>, axis: Axis, align: Align) -> Node<M> {
        let kind = e.kind();
        let mut node = match old {
            Some(n) if n.kind == kind => n,
            _ => self.create(kind, axis),
        };
        let (width, height) = (e.width(), e.height());
        let guard = node.handlers.guard.clone();
        guard.set(true);
        match e {
            Element::Text(t) => {
                let label = node.widget.downcast_ref::<gtk::Label>().unwrap();
                if label.label() != t.content {
                    label.set_label(&t.content);
                }
                label.set_wrap(t.wrap);
                label.set_selectable(t.selectable);
                label.set_xalign(match t.align {
                    Align::Center => 0.5,
                    Align::End => 1.0,
                    _ => 0.0,
                });
                label.set_css_classes(&text_classes(t.size, t.tone));
                let mut declarations = String::new();
                if let Some(color) = t.color {
                    declarations.push_str(&format!("color: #{:06x};", color & 0xffffff));
                }
                if let Some(font) = t.font_family.as_deref() {
                    let font = font.replace('"', "").replace('\\', "");
                    declarations.push_str(&format!("font-family: \"{}\";", font));
                }
                if let Some(bg) = t.bg {
                    declarations.push_str(&format!("background-color: #{:06x};", bg & 0xffffff));
                }
                if declarations.is_empty() {
                    if let Some(class) = node.custom_class.take() {
                        node.widget.remove_css_class(&class);
                    }
                } else {
                    apply_dynamic_style(&node.widget, &mut node.custom_class, &declarations);
                }
            }
            Element::Button(b) => {
                let button = node.widget.downcast_ref::<gtk::Button>().unwrap();
                match (&b.icon, b.label.is_empty()) {
                    (Some(icon), true) => button.set_icon_name(&icon_name(icon)),
                    (Some(icon), false) => {
                        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                        content.append(&gtk::Image::from_icon_name(&icon_name(icon)));
                        content.append(&gtk::Label::new(Some(&b.label)));
                        button.set_child(Some(&content));
                    }
                    (None, _) => {
                        if button.label().map(|l| l != b.label).unwrap_or(true) {
                            button.set_label(&b.label);
                        }
                    }
                }
                let mut classes = Vec::new();
                match b.kind {
                    ButtonKind::Primary => classes.push("suggested-action"),
                    ButtonKind::Danger => classes.push("destructive-action"),
                    ButtonKind::Flat => classes.push("flat"),
                    ButtonKind::Link => {
                        classes.push("link");
                        classes.push("flat");
                    }
                    ButtonKind::Secondary => {}
                }
                match b.size {
                    TextSize::Title | TextSize::Display => classes.push("title-3"),
                    TextSize::Small => classes.push("caption"),
                    _ => {}
                }
                button.set_css_classes(&classes);
                if let Some(color) = b.text_color {
                    apply_dynamic_style(&node.widget, &mut node.custom_class, &format!("color: #{:06x};", color & 0xffffff));
                } else if let Some(class) = node.custom_class.take() {
                    node.widget.remove_css_class(&class);
                }
                button.set_tooltip_text(b.tooltip.as_deref());
                button.set_sensitive(b.on_press.is_some());
                *node.handlers.press.borrow_mut() = b.on_press;
            }
            Element::Input(i) => {
                let entry = node.widget.downcast_ref::<gtk::Entry>().unwrap();
                if entry.text() != i.value {
                    entry.set_text(&i.value);
                }
                entry.set_visibility(!i.secret);
                entry.set_placeholder_text(Some(&i.placeholder));
                if i.focus && !entry.has_focus() && node.options.is_empty() {
                    entry.grab_focus();
                    node.options.push(String::new());
                }
                *node.handlers.text.borrow_mut() = i.on_change;
                *node.handlers.submit.borrow_mut() = i.on_submit;
            }
            Element::Editor(ed) => {
                let view = node.inner.as_ref().unwrap().downcast_ref::<gtk::TextView>().unwrap();
                let buffer = view.buffer();
                let (a, b) = buffer.bounds();
                if buffer.text(&a, &b, false) != ed.value {
                    buffer.set_text(&ed.value);
                }
                view.set_monospace(ed.monospace);
                view.set_editable(!ed.read_only);
                view.set_cursor_visible(!ed.read_only);
                if let Some(id) = &ed.id {
                    self.editors.borrow_mut().insert(id.clone(), view.clone());
                }
                *node.handlers.text.borrow_mut() = ed.on_change;
                *node.handlers.cursor.borrow_mut() = ed.on_cursor;
            }
            Element::Checkbox(c) => {
                let check = node.widget.downcast_ref::<gtk::CheckButton>().unwrap();
                check.set_label(Some(&c.label));
                check.set_active(c.checked);
                check.set_sensitive(c.on_toggle.is_some());
                *node.handlers.toggle.borrow_mut() = c.on_toggle;
            }
            Element::Switch(s) => {
                let sw = node.widget.downcast_ref::<gtk::Switch>().unwrap();
                sw.set_active(s.checked);
                sw.set_sensitive(s.on_toggle.is_some());
                *node.handlers.toggle.borrow_mut() = s.on_toggle;
            }
            Element::Slider(s) => {
                let scale = node.widget.downcast_ref::<gtk::Scale>().unwrap();
                scale.set_range(s.min as f64, s.max as f64);
                let step = if s.step > 0.0 { s.step as f64 } else { ((s.max - s.min) as f64 / 1000.0).abs().max(f64::EPSILON) };
                scale.set_increments(step, step * 10.0);
                if (scale.value() - s.value as f64).abs() > step / 2.0 {
                    scale.set_value(s.value as f64);
                }
                *node.handlers.value.borrow_mut() = s.on_change;
            }
            Element::Progress(p) => node.widget.downcast_ref::<gtk::ProgressBar>().unwrap().set_fraction(p.fraction as f64),
            Element::Dropdown(d) => {
                let dd = node.widget.downcast_ref::<gtk::DropDown>().unwrap();
                if node.options != d.options {
                    let strings: Vec<&str> = d.options.iter().map(|s| s.as_str()).collect();
                    dd.set_model(Some(&gtk::StringList::new(&strings)));
                    node.options = d.options.clone();
                }
                dd.set_selected(d.selected.map(|i| i as u32).unwrap_or(gtk::INVALID_LIST_POSITION));
                *node.handlers.index.borrow_mut() = d.on_select;
            }
            Element::List(l) if l.grid.is_some() => {
                let flow = node.inner.as_ref().unwrap().downcast_ref::<gtk::FlowBox>().unwrap().clone();
                let (cw, ch) = l.grid.unwrap();
                let mut old: VecDeque<Node<M>> = std::mem::take(&mut node.children).into();
                let count = l.rows.len();
                for (i, row) in l.rows.into_iter().enumerate() {
                    let previous = old.pop_front();
                    let cell = previous.as_ref().and_then(|p| p.widget.parent()).and_then(|w| w.downcast::<gtk::FlowBoxChild>().ok());
                    let child = self.reconcile(previous, row, Axis::Vertical, Align::Center);
                    let cell = match cell {
                        Some(c) => c,
                        None => {
                            let c = gtk::FlowBoxChild::new();
                            c.set_size_request(cw as i32, ch as i32);
                            flow.insert(&c, i as i32);
                            c
                        }
                    };
                    if cell.child().as_ref() != Some(&child.widget) {
                        child.widget.unparent_from_any();
                        cell.set_child(Some(&child.widget));
                    }
                    node.children.push(child);
                }
                while let Some(cell) = flow.child_at_index(count as i32) {
                    flow.remove(&cell);
                }
                node.handlers.guard.set(true);
                match l.selected.and_then(|i| flow.child_at_index(i as i32)) {
                    Some(cell) => flow.select_child(&cell),
                    None => flow.unselect_all(),
                }
                node.handlers.guard.set(false);
                *node.handlers.index.borrow_mut() = l.on_select;
                *node.handlers.activate.borrow_mut() = l.on_activate;
                *node.handlers.context.borrow_mut() = l.on_context;
            }
            Element::List(l) => {
                let list = node.inner.as_ref().unwrap().downcast_ref::<gtk::ListBox>().unwrap().clone();
                let mut old: VecDeque<Node<M>> = std::mem::take(&mut node.children).into();
                let count = l.rows.len();
                for (i, row) in l.rows.into_iter().enumerate() {
                    let previous = old.pop_front();
                    let row_widget = previous.as_ref().and_then(|p| p.widget.parent()).and_then(|w| w.downcast::<gtk::ListBoxRow>().ok());
                    let child = self.reconcile(previous, row, Axis::Horizontal, Align::Stretch);
                    let row_widget = match row_widget {
                        Some(r) => r,
                        None => {
                            let r = gtk::ListBoxRow::new();
                            list.insert(&r, i as i32);
                            r
                        }
                    };
                    if row_widget.child().as_ref() != Some(&child.widget) {
                        child.widget.unparent_from_any();
                        row_widget.set_child(Some(&child.widget));
                    }
                    node.children.push(child);
                }
                while let Some(row) = list.row_at_index(count as i32) {
                    list.remove(&row);
                }
                match l.selected.and_then(|i| list.row_at_index(i as i32)) {
                    Some(row) => list.select_row(Some(&row)),
                    None => list.unselect_all(),
                }
                *node.handlers.index.borrow_mut() = l.on_select;
                *node.handlers.activate.borrow_mut() = l.on_activate;
                *node.handlers.context.borrow_mut() = l.on_context;
            }
            Element::Stack(s) => {
                let bx = node.widget.downcast_ref::<gtk::Box>().unwrap().clone();
                bx.set_spacing(s.spacing as i32);
                bx.set_margin_top(s.padding.top as i32);
                bx.set_margin_bottom(s.padding.bottom as i32);
                bx.set_margin_start(s.padding.left as i32);
                bx.set_margin_end(s.padding.right as i32);
                let mut declarations = String::new();
                if let Some(bg) = s.bg {
                    declarations.push_str(&format!("background-color: #{:06x};", bg & 0xffffff));
                }
                if let Some((color, width)) = s.border {
                    declarations.push_str(&format!("border: {}px solid #{:06x};", width, color & 0xffffff));
                }
                if s.radius > 0 {
                    declarations.push_str(&format!("border-radius: {}px;", s.radius));
                }
                if declarations.is_empty() {
                    if let Some(class) = node.custom_class.take() {
                        node.widget.remove_css_class(&class);
                    }
                } else {
                    apply_dynamic_style(&node.widget, &mut node.custom_class, &declarations);
                }
                if s.card {
                    bx.add_css_class("card");
                } else {
                    bx.remove_css_class("card");
                }
                if s.sidebar {
                    bx.add_css_class("sidebar-pane");
                    bx.add_css_class("background");
                } else {
                    bx.remove_css_class("sidebar-pane");
                }
                let mut old: VecDeque<Node<M>> = std::mem::take(&mut node.children).into();
                let mut previous: Option<gtk::Widget> = None;
                for child in s.children {
                    let prior = old.pop_front();
                    let prior_widget = prior.as_ref().map(|p| p.widget.clone());
                    let fresh = self.reconcile(prior, child, s.axis, s.align);
                    if prior_widget.as_ref() != Some(&fresh.widget) {
                        if let Some(w) = prior_widget {
                            bx.remove(&w);
                        }
                        fresh.widget.unparent_from_any();
                        bx.insert_child_after(&fresh.widget, previous.as_ref());
                    }
                    previous = Some(fresh.widget.clone());
                    node.children.push(fresh);
                }
                for stale in old {
                    bx.remove(&stale.widget);
                }
            }
            Element::Scroll(sc) => {
                let scroller = node.widget.downcast_ref::<gtk::ScrolledWindow>().unwrap().clone();
                let prior = node.children.pop();
                let prior_widget = prior.as_ref().map(|p| p.widget.clone());
                let child = self.reconcile(prior, *sc.child, Axis::Vertical, Align::Stretch);
                if prior_widget.as_ref() != Some(&child.widget) {
                    scroller.set_child(Some(&child.widget));
                }
                node.children.push(child);
            }
            Element::Space(_) | Element::Divider => {}
            Element::Image(i) => {
                let picture = node.widget.downcast_ref::<gtk::Picture>().unwrap();
                let tone = i.tone;
                match i.source {
                    ImageSource::File(path) => picture.set_filename(Some(path)),
                    ImageSource::Icon(name) => {
                        let theme = gtk::IconTheme::for_display(&picture.display());
                        let paintable = theme.lookup_icon(&icon_name(&name), &[], 24, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
                        picture.set_paintable(Some(&paintable));
                    }
                    ImageSource::Pixels(px) => picture.set_paintable(Some(&texture(&px))),
                }
                picture.set_can_shrink(true);
                let classes: Vec<&str> = tone.map(|t| text_classes(TextSize::Body, t)).unwrap_or_default();
                picture.set_css_classes(&classes);
            }
            Element::Canvas(c) => {
                let area = node.widget.downcast_ref::<gtk::DrawingArea>().unwrap();
                if let Length::Fixed(px) = c.width {
                    area.set_content_width(px as i32);
                }
                if let Length::Fixed(px) = c.height {
                    area.set_content_height(px as i32);
                }
                *node.handlers.pixmap.borrow_mut() = c.pixmap;
                *node.handlers.canvas.borrow_mut() = c.on_event;
                area.queue_draw();
            }
        }
        guard.set(false);
        apply_size(&node.widget, width, height);
        let cross_fill = if axis == Axis::Horizontal { height.fills() } else { width.fills() };
        if !cross_fill {
            let a = gtk_align(align);
            if axis == Axis::Horizontal {
                node.widget.set_valign(a);
            } else {
                node.widget.set_halign(a);
            }
        }
        node
    }
}

trait Unparent {
    fn unparent_from_any(&self);
}

impl Unparent for gtk::Widget {
    fn unparent_from_any(&self) {
        if let Some(parent) = self.parent() {
            if let Some(bx) = parent.downcast_ref::<gtk::Box>() {
                bx.remove(self);
            } else if let Some(row) = parent.downcast_ref::<gtk::ListBoxRow>() {
                row.set_child(None::<&gtk::Widget>);
            } else {
                self.unparent();
            }
        }
    }
}

fn key_press(key: gdk::Key, state: gdk::ModifierType) -> Option<KeyPress> {
    let mods = Modifiers { ctrl: state.contains(gdk::ModifierType::CONTROL_MASK), alt: state.contains(gdk::ModifierType::ALT_MASK), shift: state.contains(gdk::ModifierType::SHIFT_MASK) };
    let k = match key {
        gdk::Key::Return | gdk::Key::KP_Enter => Key::Enter,
        gdk::Key::Escape => Key::Escape,
        gdk::Key::BackSpace => Key::Backspace,
        gdk::Key::Delete => Key::Delete,
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => Key::Tab,
        gdk::Key::Up => Key::Up,
        gdk::Key::Down => Key::Down,
        gdk::Key::Left => Key::Left,
        gdk::Key::Right => Key::Right,
        gdk::Key::Home => Key::Home,
        gdk::Key::End => Key::End,
        gdk::Key::Page_Up => Key::PageUp,
        gdk::Key::Page_Down => Key::PageDown,
        gdk::Key::Insert => Key::Insert,
        k => {
            let name = k.name().map(|n| n.to_string()).unwrap_or_default();
            if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
                Key::F(n)
            } else {
                Key::Char(k.to_unicode()?)
            }
        }
    };
    Some(KeyPress { key: k, mods })
}

fn app_id(id: &str) -> String {
    if id.contains('.') { id.to_string() } else { format!("org.hamix.{}", id.replace('-', "_")) }
}

struct Runtime<A: App> {
    app: RefCell<A>,
    builder: Builder<A::Message>,
    window: gtk::ApplicationWindow,
    application: gtk::Application,
    content: gtk::Box,
    root: RefCell<Option<Node<A::Message>>>,
    menu: RefCell<Vec<MenuItem<A::Message>>>,
    menu_button: gtk::MenuButton,
    menu_signature: RefCell<Vec<String>>,
    menu_messages: RefCell<Vec<Option<A::Message>>>,
    tick: RefCell<Option<(u32, glib::SourceId)>>,
    pointer: Rc<Cell<(f64, f64)>>,
    watched: RefCell<Vec<(i32, glib::SourceId)>>,
    watch_messages: RefCell<Vec<(i32, A::Message)>>,
}

impl<A: App> Runtime<A> {
    fn dispatch(&self) -> Rc<Dispatch<A::Message>> {
        self.builder.dispatch.clone()
    }

    fn flush(self: &Rc<Self>) {
        let messages: Vec<A::Message> = self.dispatch().queue.borrow_mut().drain(..).collect();
        for message in messages {
            let mut cx = Context::new();
            self.app.borrow_mut().update(message, &mut cx);
            self.apply(cx.take());
        }
        self.render();
    }

    fn render(self: &Rc<Self>) {
        let view = self.app.borrow().view();
        let old = self.root.borrow_mut().take();
        let old_widget = old.as_ref().map(|n| n.widget.clone());
        let node = self.builder.reconcile(old, view, Axis::Vertical, Align::Stretch);
        if old_widget.as_ref() != Some(&node.widget) {
            if let Some(w) = old_widget {
                self.content.remove(&w);
            }
            node.widget.set_vexpand(true);
            self.content.append(&node.widget);
        }
        *self.root.borrow_mut() = Some(node);
        self.window.set_title(Some(&self.app.borrow().title()));
        self.update_menu();
        self.update_tick();
        self.update_watch();
    }

    fn update_watch(self: &Rc<Self>) {
        let wanted = self.app.borrow().watch();
        let fds: Vec<i32> = wanted.iter().map(|(fd, _)| *fd).collect();
        *self.watch_messages.borrow_mut() = wanted;
        let mut watched = self.watched.borrow_mut();
        let mut kept = Vec::new();
        for (fd, id) in watched.drain(..) {
            if fds.contains(&fd) {
                kept.push((fd, id));
            } else {
                id.remove();
            }
        }
        *watched = kept;
        for fd in fds {
            if watched.iter().any(|(w, _)| *w == fd) {
                continue;
            }
            let me = Rc::downgrade(self);
            let id = glib::source::unix_fd_add_local(fd, glib::IOCondition::IN | glib::IOCondition::HUP, move |fd, _| {
                let Some(me) = me.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let message = me.watch_messages.borrow().iter().find(|(w, _)| *w == fd).map(|(_, m)| m.clone());
                match message {
                    Some(m) => {
                        me.dispatch().push(m);
                        glib::ControlFlow::Continue
                    }
                    None => {
                        me.watched.borrow_mut().retain(|(w, _)| *w != fd);
                        glib::ControlFlow::Break
                    }
                }
            });
            watched.push((fd, id));
        }
    }

    fn update_menu(self: &Rc<Self>) {
        let items = self.app.borrow().menu();
        fn signature<M>(items: &[MenuItem<M>], out: &mut Vec<String>) {
            for i in items {
                out.push(format!("{}|{:?}|{}|{}|{}", i.label, i.checked, i.message.is_some(), i.separator, i.children.len()));
                signature(&i.children, out);
            }
        }
        let mut sig = Vec::new();
        signature(&items, &mut sig);
        if *self.menu_signature.borrow() != sig {
            let mut messages: Vec<Option<A::Message>> = Vec::new();
            let menu = self.build_menu(&items, &mut messages);
            *self.menu_messages.borrow_mut() = messages;
            self.menu_button.set_menu_model(Some(&menu));
            self.menu_button.set_visible(!items.is_empty());
            *self.menu_signature.borrow_mut() = sig;
        }
        *self.menu.borrow_mut() = items;
    }

    fn build_menu(self: &Rc<Self>, items: &[MenuItem<A::Message>], messages: &mut Vec<Option<A::Message>>) -> gio::Menu {
        let menu = gio::Menu::new();
        let mut section = gio::Menu::new();
        for item in items {
            if item.separator {
                menu.append_section(None, &section);
                section = gio::Menu::new();
                continue;
            }
            if !item.children.is_empty() {
                let sub = self.build_menu(&item.children, messages);
                section.append_submenu(Some(&item.label), &sub);
                continue;
            }
            let index = messages.len();
            messages.push(item.message.clone());
            let name = format!("item{}", index);
            let label = match item.checked {
                Some(true) => format!("✓ {}", item.label),
                _ => item.label.clone(),
            };
            let entry = gio::MenuItem::new(Some(&label), Some(&format!("win.{}", name)));
            if let Some(s) = item.shortcut {
                let mut accel = String::new();
                if s.mods.ctrl {
                    accel.push_str("<Control>");
                }
                if s.mods.alt {
                    accel.push_str("<Alt>");
                }
                if s.mods.shift {
                    accel.push_str("<Shift>");
                }
                accel.push_str(&match s.key {
                    Key::Char(c) => c.to_ascii_lowercase().to_string(),
                    Key::F(n) => format!("F{}", n),
                    Key::Home => String::from("Home"),
                    Key::End => String::from("End"),
                    Key::Delete => String::from("Delete"),
                    other => format!("{:?}", other),
                });
                entry.set_attribute_value("accel", Some(&accel.to_variant()));
            }
            section.append_item(&entry);
            self.window.remove_action(&name);
            let action = gio::SimpleAction::new(&name, None);
            action.set_enabled(item.message.is_some());
            let me = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(me) = me.upgrade() {
                    let message = me.menu_messages.borrow().get(index).cloned().flatten();
                    if let Some(m) = message {
                        me.dispatch().push(m);
                    }
                }
            });
            self.window.add_action(&action);
        }
        menu.append_section(None, &section);
        menu
    }

    fn update_tick(self: &Rc<Self>) {
        let wanted = self.app.borrow().tick().map(|(ms, _)| ms);
        let current = self.tick.borrow().as_ref().map(|(ms, _)| *ms);
        if wanted == current {
            return;
        }
        if let Some((_, id)) = self.tick.borrow_mut().take() {
            id.remove();
        }
        if let Some(ms) = wanted {
            let me = Rc::downgrade(self);
            let id = glib::timeout_add_local(Duration::from_millis(ms as u64), move || {
                let Some(me) = me.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let message = me.app.borrow().tick().map(|(_, m)| m);
                if let Some(m) = message {
                    me.dispatch().push(m);
                }
                glib::ControlFlow::Continue
            });
            *self.tick.borrow_mut() = Some((ms, id));
        }
    }

    fn apply(self: &Rc<Self>, commands: Vec<Command<A::Message>>) {
        for command in commands {
            let d = self.dispatch();
            match command {
                Command::Quit => self.application.quit(),
                Command::Edit { id, action } => {
                    let Some(view) = self.builder.editors.borrow().get(&id).cloned() else {
                        continue;
                    };
                    let buffer = view.buffer();
                    let clipboard = view.clipboard();
                    let editable = view.is_editable();
                    match action {
                        EditAction::SelectAll => {
                            let (a, b) = buffer.bounds();
                            buffer.select_range(&a, &b);
                        }
                        EditAction::Copy => buffer.copy_clipboard(&clipboard),
                        EditAction::Cut => buffer.cut_clipboard(&clipboard, editable),
                        EditAction::Paste => buffer.paste_clipboard(&clipboard, None, editable),
                        EditAction::Delete => {
                            buffer.delete_selection(true, editable);
                        }
                        EditAction::Undo => buffer.undo(),
                        EditAction::Redo => buffer.redo(),
                        EditAction::GoToStart | EditAction::GoToEnd => {
                            let (a, b) = buffer.bounds();
                            let mut target = if action == EditAction::GoToStart { a } else { b };
                            buffer.place_cursor(&target);
                            view.scroll_to_iter(&mut target, 0.0, false, 0.0, 0.0);
                        }
                        EditAction::Focus => {
                            view.grab_focus();
                        }
                    }
                }
                Command::Find { id, query, reply } => {
                    let found = self.builder.editors.borrow().get(&id).cloned().map(|view| {
                        let buffer = view.buffer();
                        let from = buffer.selection_bounds().map(|(_, b)| b).unwrap_or_else(|| buffer.iter_at_offset(buffer.cursor_position()));
                        let flags = gtk::TextSearchFlags::CASE_INSENSITIVE;
                        let hit = from.forward_search(&query, flags, None).or_else(|| buffer.start_iter().forward_search(&query, flags, None));
                        match hit {
                            Some((mut a, b)) => {
                                buffer.select_range(&a, &b);
                                view.scroll_to_iter(&mut a, 0.1, false, 0.0, 0.0);
                                true
                            }
                            None => false,
                        }
                    });
                    d.push(reply(found.unwrap_or(false)));
                }
                Command::After { ms, message } => {
                    glib::timeout_add_local_once(Duration::from_millis(ms as u64), move || d.push(message));
                }
                Command::Copy(text) => self.window.clipboard().set_text(&text),
                Command::Paste(reply) => {
                    self.window.clipboard().read_text_async(None::<&gio::Cancellable>, move |res| {
                        let text = res.ok().flatten().map(|s| s.to_string());
                        d.push(reply(text));
                    });
                }
                Command::ChooseFile { kind, title, filters, start, reply } => {
                    let dialog = gtk::FileDialog::new();
                    dialog.set_title(&title);
                    if !filters.is_empty() {
                        let filter = gtk::FileFilter::new();
                        filter.set_name(Some("Files"));
                        for f in &filters {
                            filter.add_suffix(f.trim_start_matches("*.").trim_start_matches('.'));
                        }
                        let store = gio::ListStore::new::<gtk::FileFilter>();
                        store.append(&filter);
                        dialog.set_filters(Some(&store));
                    }
                    let finish = move |res: Result<gio::File, glib::Error>| {
                        let path = res.ok().and_then(|f| f.path()).map(|p| p.to_string_lossy().into_owned());
                        d.push(reply(path));
                    };
                    match kind {
                        FileDialog::Open => dialog.open(Some(&self.window), None::<&gio::Cancellable>, finish),
                        FileDialog::Save => {
                            if !start.is_empty() {
                                dialog.set_initial_name(Some(&start));
                            }
                            dialog.save(Some(&self.window), None::<&gio::Cancellable>, finish)
                        }
                        FileDialog::Folder => dialog.select_folder(Some(&self.window), None::<&gio::Cancellable>, finish),
                    }
                }
                Command::Message { title, text } => gtk::AlertDialog::builder().message(title).detail(text).modal(true).build().show(Some(&self.window)),
                Command::Confirm { title, text, reply } => {
                    let dialog = gtk::AlertDialog::builder().message(title).detail(text).buttons(["Cancel", "OK"]).cancel_button(0).default_button(1).modal(true).build();
                    dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |res| d.push(reply(matches!(res, Ok(1)))));
                }
                Command::Notify { title, body } => {
                    let n = gio::Notification::new(&title);
                    n.set_body(Some(&body));
                    self.application.send_notification(None, &n);
                }
                Command::Resize { width, height } => self.window.set_default_size(width as i32, height as i32),
                Command::ShowMenu(items) => {
                    let mut messages = self.menu_messages.borrow().clone();
                    let model = self.build_menu(&items, &mut messages);
                    *self.menu_messages.borrow_mut() = messages;
                    let popover = gtk::PopoverMenu::from_model(Some(&model));
                    popover.set_parent(&self.window);
                    popover.set_has_arrow(false);
                    let (x, y) = self.pointer.get();
                    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                    popover.connect_closed(|p| {
                        let p = p.clone();
                        glib::idle_add_local_once(move || p.unparent());
                    });
                    popover.popup();
                }
                Command::ResetScroll => {
                    if let Some(root) = self.root.borrow().as_ref() {
                        reset_scrolls(root);
                    }
                }
                Command::SetWallpaper { path, reply } => {
                    let uri = format!("file://{}", path);
                    let ok = ["picture-uri", "picture-uri-dark"].iter().all(|key| {
                        std::process::Command::new("gsettings").args(["set", "org.gnome.desktop.background", key, &uri]).status().map(|s| s.success()).unwrap_or(false)
                    });
                    d.push(reply(ok));
                }
            }
        }
    }
}

pub fn run<A: App>(settings: Settings, app: A) -> i32 {
    let application = gtk::Application::builder().application_id(app_id(&settings.app_id)).flags(gio::ApplicationFlags::NON_UNIQUE).build();
    let holder = Rc::new(RefCell::new(Some(app)));
    let runtime: Rc<RefCell<Option<Rc<Runtime<A>>>>> = Rc::new(RefCell::new(None));
    let keep = runtime.clone();
    application.connect_activate(move |application| {
        let Some(mut app) = holder.borrow_mut().take() else {
            return;
        };
        let dark = system_prefers_dark();
        if let Some(gtk_settings) = gtk::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(dark);
        }
        app.appearance(dark);
        let (w, h) = settings.size;
        let window = gtk::ApplicationWindow::builder().application(application).default_width(w as i32).default_height(h as i32).resizable(settings.resizable).build();
        if let Some((mw, mh)) = settings.min_size {
            window.set_size_request(mw as i32, mh as i32);
        }
        if let Some(icon) = &settings.icon {
            window.set_icon_name(Some(icon));
        }
        let header = gtk::HeaderBar::new();
        let menu_button = gtk::MenuButton::new();
        menu_button.set_icon_name("open-menu-symbolic");
        menu_button.set_visible(false);
        header.pack_end(&menu_button);
        window.set_titlebar(Some(&header));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.set_child(Some(&content));
        let pointer = Rc::new(Cell::new((0.0, 0.0)));
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        let track = pointer.clone();
        motion.connect_motion(move |_, x, y| track.set((x, y)));
        window.add_controller(motion);
        let dispatch = Rc::new(Dispatch { queue: RefCell::new(VecDeque::new()), flush: RefCell::new(None), scheduled: Cell::new(false) });
        let rt = Rc::new(Runtime {
            app: RefCell::new(app),
            builder: Builder { dispatch: dispatch.clone(), editors: RefCell::new(HashMap::new()) },
            window: window.clone(),
            application: application.clone(),
            content,
            root: RefCell::new(None),
            menu: RefCell::new(Vec::new()),
            menu_button,
            menu_signature: RefCell::new(Vec::new()),
            menu_messages: RefCell::new(Vec::new()),
            tick: RefCell::new(None),
            pointer: pointer.clone(),
            watched: RefCell::new(Vec::new()),
            watch_messages: RefCell::new(Vec::new()),
        });
        let weak = Rc::downgrade(&rt);
        *dispatch.flush.borrow_mut() = Some(Box::new(move || {
            if let Some(rt) = weak.upgrade() {
                rt.flush();
            }
        }));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&rt);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(rt) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let Some(press) = key_press(key, state) else {
                return glib::Propagation::Proceed;
            };
            let typing = GtkWindowExt::focus(&rt.window).map(|w| w.is::<gtk::Text>() || w.is::<gtk::TextView>()).unwrap_or(false);
            let shortcut = press.mods.ctrl || press.mods.alt || matches!(press.key, Key::F(_));
            let message = if shortcut { shortcut_message(&rt.menu.borrow(), &press) } else { None };
            let message = message.or_else(|| if typing && !shortcut { None } else { rt.app.borrow().key(&press) });
            match message {
                Some(m) => {
                    rt.dispatch().push(m);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        window.add_controller(keys);
        let weak = Rc::downgrade(&rt);
        window.connect_close_request(move |_| {
            let Some(rt) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mut cx = Context::new();
            let close = rt.app.borrow_mut().close_requested(&mut cx);
            rt.apply(cx.take());
            if close { glib::Propagation::Proceed } else { glib::Propagation::Stop }
        });
        let mut cx = Context::new();
        rt.app.borrow_mut().started(&mut cx);
        rt.apply(cx.take());
        rt.render();
        window.present();
        *keep.borrow_mut() = Some(rt);
    });
    let code = application.run_with_args(&[] as &[&str]);
    runtime.borrow_mut().take();
    i32::from(code)
}
