# hxtoolkit

A small declarative desktop UI toolkit. One application code base runs

* on the **HamixOS Nook desktop** — the `nook` backend talks to `hxserver`
  directly over its Unix socket (hxproto messages, window buffers in POSIX shared
  memory) and draws in the native Nook style;
* in **GTK desktops** (GNOME, KDE Plasma, Enlightenment, Xfce, ...) — the `gtk`
  backend builds real GTK 4 widgets, so the program looks native there.

The backend is picked at run time: a running `hxserver` wins, otherwise GTK is used
when `WAYLAND_DISPLAY` or `DISPLAY` is set. `HXTOOLKIT_BACKEND=nook|gtk` overrides.

## Choosing the display server

By default a program connects wherever it can (`DisplayServer::Auto`). A program
that only makes sense in one environment says so in its `Settings`:

```rust
Settings::new("org.example.Panel").hxserver_only()
Settings::new("org.example.Viewer").wayland_only()
Settings::new("org.example.Tool").display(DisplayServer::Auto)
```

| setting | connects to | when it is missing |
|---|---|---|
| `Auto` (nothing set) | hxserver if it runs, else Wayland, else X11 | `no display` |
| `hxserver_only()` | the Nook desktop through the `nook` backend | exits with `hxserver is not running` |
| `wayland_only()` | a Wayland compositor through GTK (`GDK_BACKEND=wayland`, no X11 fallback) | exits with `WAYLAND_DISPLAY is not set` |

The user can narrow an `Auto` program with `HXTOOLKIT_DISPLAY=hxserver|wayland|auto`;
a program that fixed its server ignores a contradicting value. `hxtoolkit::detect_for`
returns the backend that would be used, or the reason why none fits.

The core is `no_std` + `alloc`, so it also works for HamixOS programs that do not
link the Rust standard library.

## Example

```rust
use hxtoolkit::{button, column, text, title, App, Context, Element, Settings};

#[derive(Default)]
struct Counter {
    clicks: u32,
}

#[derive(Clone)]
enum Msg {
    Press,
}

impl App for Counter {
    type Message = Msg;

    fn title(&self) -> String {
        "Counter".into()
    }

    fn view(&self) -> Element<Msg> {
        column![
            title("Hello from hxtoolkit"),
            text(format!("Pressed {} times", self.clicks)).dim(),
            button("Press me").primary().on_press(Msg::Press),
        ]
        .padding(24)
        .into()
    }

    fn update(&mut self, _: Msg, cx: &mut Context<Msg>) {
        self.clicks += 1;
        if self.clicks == 5 {
            cx.notify("Counter", "Five presses");
        }
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("counter").size(380, 220), Counter::default())
}

hxtoolkit::main!(run);
```

## Model

The application is a value. `view` turns the state into an element tree, widgets
report what happened as messages, `update` changes the state and may ask for side
effects through `Context`. After every update the view is rebuilt; backends keep
widget state (focus, text cursor, scroll offsets, selection) across rebuilds.

## Widgets

| builder | widget |
|---|---|
| `text`, `title` | labels (`.dim()`, `.strong()`, `.small()`, `.mono()`, `.wrap()`, `.tone(..)`) |
| `button`, `icon_button` | push buttons (`.primary()`, `.danger()`, `.flat()`, `.icon(..)`, `.size(..)`) |
| `input` | single-line text field (`.secret()`, `.on_submit(..)`, `.focused()`) |
| `editor`, `viewer` | multi-line text with selection, undo, line numbers, find (`.id(..)`) |
| `checkbox`, `switch`, `slider`, `dropdown`, `progress` | the usual controls |
| `list` | selectable, scrollable rows of any elements (`.on_select`, `.on_activate`, `.on_context`), or an icon grid with `.grid(w, h)` |
| `column!`, `row!`, `column`, `row` | layout (`.spacing`, `.padding`, `.align`, `.card()`, `.sidebar()`) |
| `scroll`, `space`, `hspace`, `vspace`, `divider` | scrolling and spacing |
| `icon`, `image`, `picture` | theme icons, image files, in-memory pixmaps |
| `canvas` | a `Pixmap` the program paints itself, with mouse and resize events |

Sizes are `Length::Shrink` (natural), `Length::Fill`/`Portion(n)` (share the free
space) or `Length::Fixed(px)`.

## Application hooks

* `menu()` — a menu; items with children become a menu bar on Nook and submenus
  of the header-bar menu on GTK. Shortcuts (`"Ctrl+S"`) work everywhere.
* `key()` — keys that no focused widget consumed.
* `tick()` — a periodic message (clocks, monitors, progress polling).
* `close_requested()` — ask before closing (unsaved work).
* `started()` — run commands once the window exists.
* `watch()` — file descriptors that wake the program when readable (a pty, a pipe).
* `appearance(dark)` — the desktop switched between light and dark.

## Context commands

`after(ms, msg)`, `copy(text)`, `paste(reply)`, `open_file`, `save_file`,
`choose_folder`, `message`, `confirm`, `notify`, `set_wallpaper`, `edit(id, action)`
and `find(id, query, reply)` for editors, `show_menu(items)` for context menus,
`resize`, `quit`.

Menus, dropdowns and context menus are real popup windows (on Nook through
`CreatePopup`, on GTK `PopoverMenu`), so they can extend past the window edge.
Text fields and editors get a Cut/Copy/Paste/Select all menu on right click.

`set_wallpaper` never changes the wallpaper by itself: on Nook the desktop shows a
system dialog with a preview and the reply is `true` only if the user pressed
"Set wallpaper"; on GTK desktops it goes through the desktop's own settings.

## Features

* `nook` — the HamixOS backend (depends on `hxproto` and `vellum`, both `no_std`
  crates without further dependencies).
* `gtk` — the GTK 4 backend (enables `std`, depends on `gtk4`).

## HamixOS programs

A HamixOS program enables `nook` and takes its entry point from `hxstd`; the same
crate enables `gtk` everywhere else:

```toml
[target.'cfg(target_os = "hamix")'.dependencies]
hxstd = { path = "../../libs/hxstd" }
hxtoolkit = { path = "../../libs/hxtoolkit", features = ["nook"] }

[target.'cfg(not(target_os = "hamix"))'.dependencies]
hxtoolkit = { path = "../../libs/hxtoolkit", features = ["gtk"] }
```

```rust
#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
```
