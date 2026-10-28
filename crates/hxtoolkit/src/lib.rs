#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod app;
mod element;
pub mod font;
mod keys;
pub mod layout;
mod pixmap;
mod style;

#[cfg(feature = "nook")]
mod nook;

#[cfg(feature = "gtk")]
mod gtk;

use alloc::string::String;
use alloc::vec::Vec;

pub use app::{shortcut_message, App, Command, Context, DisplayServer, EditAction, FileDialog, MenuItem, Reply, Settings};
pub use element::*;
pub use font::{ellipsize_text, line_height, load_font_bytes, load_font_file, load_system_font, measure_text, measure_text_bounds, wrap_text, Font, FontFamily, FontStore, FontWeight, Glyph};
pub use keys::{Key, KeyPress, Modifiers, Shortcut};
pub use pixmap::Pixmap;
pub use style::{Align, Axis, ButtonKind, Color, Length, Padding, TextSize, Tone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Nook,
    Wayland,
    Gtk,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::Nook => "nook",
            Backend::Wayland => "wayland",
            Backend::Gtk => "gtk",
        }
    }
}

fn env_var(name: &str) -> Option<String> {
    #[cfg(feature = "std")]
    {
        if let Ok(v) = std::env::var(name) {
            return Some(v);
        }
    }
    #[cfg(feature = "nook")]
    {
        if let Some(v) = nook::env(name) {
            return Some(v);
        }
    }
    let _ = name;
    None
}

pub fn compiled_backends() -> Vec<Backend> {
    let mut out = Vec::new();
    #[cfg(feature = "nook")]
    out.push(Backend::Nook);
    #[cfg(feature = "gtk")]
    out.push(Backend::Gtk);
    #[cfg(feature = "nook")]
    out.push(Backend::Wayland);
    out
}

#[cfg(feature = "gtk")]
fn wayland_running() -> bool {
    env_var("WAYLAND_DISPLAY").map(|v| !v.is_empty()).unwrap_or(false)
}

fn usable(backend: Backend, server: DisplayServer) -> bool {
    match (backend, server) {
        (Backend::Nook, DisplayServer::Wayland) | (Backend::Gtk, DisplayServer::Hxserver) | (Backend::Wayland, DisplayServer::Hxserver) => false,
        #[cfg(feature = "nook")]
        (Backend::Nook, _) => nook::available(),
        #[cfg(feature = "nook")]
        (Backend::Wayland, _) => nook::wayland_available(),
        #[cfg(feature = "gtk")]
        (Backend::Gtk, DisplayServer::Wayland) => wayland_running(),
        #[cfg(feature = "gtk")]
        (Backend::Gtk, _) => gtk::available(),
        #[allow(unreachable_patterns)]
        _ => false,
    }
}

fn allowed(backend: Backend, server: DisplayServer) -> bool {
    !matches!((backend, server), (Backend::Nook, DisplayServer::Wayland) | (Backend::Gtk, DisplayServer::Hxserver) | (Backend::Wayland, DisplayServer::Hxserver))
}

fn requested(server: DisplayServer) -> DisplayServer {
    let Some(wanted) = env_var("HXTOOLKIT_DISPLAY") else {
        return server;
    };
    match DisplayServer::parse(&wanted) {
        Some(DisplayServer::Auto) | None => server,
        Some(env) if server == DisplayServer::Auto || server == env => env,
        Some(_) => server,
    }
}

pub fn detect_for(server: DisplayServer) -> Result<Backend, String> {
    let server = requested(server);
    let compiled = compiled_backends();
    if let Some(wanted) = env_var("HXTOOLKIT_BACKEND") {
        if let Some(b) = compiled.iter().copied().find(|b| b.name() == wanted.trim() && allowed(*b, server)) {
            return Ok(b);
        }
    }
    if let Some(b) = compiled.iter().copied().find(|b| usable(*b, server)) {
        return Ok(b);
    }
    let reason = match server {
        DisplayServer::Hxserver if !compiled.contains(&Backend::Nook) => "this program runs only on hxserver, but was built without the Nook backend",
        DisplayServer::Hxserver => "this program runs only on the Nook desktop: hxserver is not running",
        DisplayServer::Wayland if !compiled.contains(&Backend::Gtk) && !compiled.contains(&Backend::Wayland) => "this program runs only on Wayland, but was built without a Wayland backend",
        DisplayServer::Wayland => "this program runs only on Wayland: WAYLAND_DISPLAY is not set",
        DisplayServer::Auto => "no display: start the Nook desktop (hxserver) or a Wayland/X11 session",
    };
    Err(String::from(reason))
}

pub fn detect() -> Option<Backend> {
    detect_for(DisplayServer::Auto).ok()
}

fn report(text: &str) {
    #[cfg(feature = "std")]
    {
        std::eprintln!("{}", text);
    }
    #[cfg(all(feature = "nook", not(feature = "std")))]
    {
        nook::report(text);
    }
    let _ = text;
}

pub fn run<A: App>(settings: Settings, app: A) -> i32 {
    match detect_for(settings.display) {
        #[cfg(feature = "nook")]
        Ok(Backend::Nook) => nook::run(settings, app, false),
        #[cfg(feature = "nook")]
        Ok(Backend::Wayland) => nook::run(settings, app, true),
        #[cfg(feature = "gtk")]
        Ok(Backend::Gtk) => {
            if requested(settings.display) == DisplayServer::Wayland {
                std::env::set_var("GDK_BACKEND", "wayland");
            }
            gtk::run(settings, app)
        }
        #[allow(unreachable_patterns)]
        Ok(_) => {
            let _ = (settings, app);
            1
        }
        Err(reason) => {
            let _ = (settings, app);
            report(&alloc::format!("hxtoolkit: {}", reason));
            1
        }
    }
}

#[doc(hidden)]
pub mod __private {
    pub use alloc::vec;
    pub use alloc::vec::Vec;
}

#[macro_export]
macro_rules! main {
    ($run:path) => {
        fn main() {
            ::std::process::exit($run())
        }
    };
}
