use alloc::vec::Vec;
use hxproto::Event;
use vellum::Area;

use crate::shell::{Core, Kind};

#[cfg(feature = "hxserver")]
pub mod hxserver;
#[cfg(feature = "wayland")]
pub mod wayland;

#[cfg(not(any(feature = "hxserver", feature = "wayland")))]
compile_error!("nook-shell needs the hxserver or the wayland feature");

pub enum Platform {
    #[cfg(feature = "hxserver")]
    Hx(hxserver::Platform),
    #[cfg(feature = "wayland")]
    Wl(wayland::Platform),
}

macro_rules! each {
    ($self:expr, $p:ident => $e:expr) => {
        match $self {
            #[cfg(feature = "hxserver")]
            Platform::Hx($p) => $e,
            #[cfg(feature = "wayland")]
            Platform::Wl($p) => $e,
        }
    };
}

impl Platform {
    pub fn open() -> Result<Platform, &'static str> {
        let wanted = hxstd::env::var("NOOK_PLATFORM").unwrap_or_default();
        #[cfg(feature = "hxserver")]
        {
            let usable = wanted == "hxserver" || (wanted.is_empty() && hxclient::server().is_some());
            if usable || !cfg!(feature = "wayland") {
                return hxserver::Platform::open().map(Platform::Hx);
            }
        }
        #[cfg(feature = "wayland")]
        {
            let _ = &wanted;
            return wayland::Platform::open().map(Platform::Wl);
        }
        #[allow(unreachable_code)]
        Err("no display platform")
    }

    pub fn name(&self) -> &'static str {
        match self {
            #[cfg(feature = "hxserver")]
            Platform::Hx(_) => "hxserver",
            #[cfg(feature = "wayland")]
            Platform::Wl(_) => "wayland",
        }
    }

    pub fn screen(&self) -> (i32, i32) {
        each!(self, p => p.screen())
    }

    pub fn workspace(&self) -> u32 {
        each!(self, p => p.workspace())
    }

    pub fn alive(&self) -> bool {
        each!(self, p => p.alive())
    }

    pub fn toast(&mut self, title: &str, body: &str, icon: &str) {
        each!(self, p => p.toast(title, body, icon))
    }

    pub fn shell_action(&mut self, action: u32) {
        each!(self, p => p.shell_action(action))
    }

    pub fn launched(&mut self, pid: i64, key: &str) {
        each!(self, p => p.launched(pid, key))
    }

    pub fn minimize_target(&mut self, toplevel: u32, area: Area) {
        each!(self, p => p.minimize_target(toplevel, area))
    }

    pub fn set_cursor(&mut self, window: u32, cursor: u32) {
        each!(self, p => p.set_cursor(window, cursor))
    }

    pub fn toplevel_action(&mut self, toplevel: u32, action: u32) {
        each!(self, p => p.toplevel_action(toplevel, action))
    }

    pub fn screen_changed(&mut self, width: u32, height: u32) {
        each!(self, p => p.screen_changed(width, height))
    }

    pub fn pump(&mut self, timeout: u64, out: &mut Vec<Event>) {
        each!(self, p => p.pump(timeout, out))
    }

    pub fn rebuild(&mut self, core: &mut Core, dock_height: i32, exclusive: i32) {
        each!(self, p => p.rebuild(core, dock_height, exclusive))
    }

    pub fn ensure_dock(&mut self, core: &mut Core, height: i32, exclusive: i32) {
        each!(self, p => p.ensure_dock(core, height, exclusive))
    }

    pub fn configure_dock(&mut self, exclusive: i32) {
        each!(self, p => p.configure_dock(exclusive))
    }

    pub fn set_popup(&mut self, core: &mut Core, wanted: Option<Area>) {
        each!(self, p => p.set_popup(core, wanted))
    }

    pub fn set_osd(&mut self, core: &mut Core, wanted: Option<Area>) {
        each!(self, p => p.set_osd(core, wanted))
    }

    pub fn origin_of(&self, window: u32) -> Option<(i32, i32)> {
        each!(self, p => p.origin_of(window))
    }

    pub fn is_desktop(&self, window: u32) -> bool {
        each!(self, p => p.is_desktop(window))
    }

    pub fn configured(&mut self, window: u32, area: Area) {
        each!(self, p => p.configured(window, area))
    }

    pub fn target(&mut self, kind: Kind) -> Option<(*mut u32, Area)> {
        each!(self, p => p.target(kind))
    }

    pub fn present(&mut self, kind: Kind, rect: Area) {
        each!(self, p => p.present(kind, rect))
    }

    pub fn input_region(&mut self, kind: Kind, areas: &[Area]) {
        each!(self, p => p.input_region(kind, areas))
    }
}
