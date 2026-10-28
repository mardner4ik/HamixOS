#![no_std]
#![no_main]

extern crate alloc;

use hxclient::v2::{self, Connection, LayerSpec, LayerSurface};
use hxclient::{Event, Window};
use hxstd::{entry, env, println, sys};

struct Checks {
    failures: u32,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{} {}", if ok { "PASS" } else { "FAIL" }, what);
        if !ok {
            self.failures += 1;
        }
    }
}

fn main() -> i32 {
    let hold = env::args().iter().any(|a| a == "--hold");
    let mut c = Checks { failures: 0 };
    let mut conn = match Connection::open() {
        Ok(conn) => conn,
        Err(e) => {
            println!("FAIL connect: {}", e);
            return 1;
        }
    };
    c.check(conn.version == hxproto::VERSION, "Hello/Welcome negotiates hxproto v2");
    c.check(conn.has(hxproto::FEATURE_LAYERS) && conn.has(hxproto::FEATURE_TOPLEVELS), "server announces layers and toplevels");
    c.check(conn.screen.0 > 0 && conn.screen.2 == 100, "Output event gives the screen size and scale");

    let spec = LayerSpec { layer: v2::LAYER_TOP, anchor: v2::ANCHOR_LEFT | v2::ANCHOR_RIGHT | v2::ANCHOR_BOTTOM, exclusive: 40, margins: [0, 0, 0, 0], keyboard: false, width: 0, height: 40 };
    let bar = LayerSurface::open(&mut conn, &spec);
    c.check(bar.is_ok(), "a bottom-anchored layer surface is created");
    let mut bar = bar.ok();
    if let Some(bar) = bar.as_mut() {
        c.check(bar.width == conn.screen.0 && bar.y == conn.screen.1 as i32 - 40, "the layer is stretched along its anchors");
        for p in bar.pixels() {
            *p = 0xFF20_80C0;
        }
        c.check(bar.present(), "the layer is presented");
    }

    c.check(conn.subscribe_toplevels(), "subscribe to toplevels");
    let window = Window::open("hxproto-check", 320, 200);
    c.check(window.is_ok(), "a normal window opens next to the layer");
    let window = window.ok();
    let seen = conn.wait(|e| matches!(e, Event::ToplevelState { title, .. } if title.as_str() == "hxproto-check"), 3000);
    let toplevel = match seen {
        Some(Event::ToplevelState { toplevel, state, .. }) => {
            c.check(state & v2::TOPLEVEL_ACTIVE != 0, "the new window is reported as an active toplevel");
            Some(toplevel)
        }
        _ => {
            c.check(false, "the new window is reported as a toplevel");
            None
        }
    };
    if let Some(id) = toplevel {
        conn.toplevel_action(id, v2::ACTION_MINIMIZE);
        let minimized = conn.wait(|e| matches!(e, Event::ToplevelState { toplevel, state, .. } if *toplevel == id && state & v2::TOPLEVEL_MINIMIZED != 0), 3000);
        c.check(minimized.is_some(), "toplevel action minimizes the window");
        conn.toplevel_action(id, v2::ACTION_ACTIVATE);
        let active = conn.wait(|e| matches!(e, Event::ToplevelState { toplevel, state, .. } if *toplevel == id && state & v2::TOPLEVEL_ACTIVE != 0), 3000);
        c.check(active.is_some(), "toplevel action activates it again");
        conn.move_to_workspace(id, 1);
        let moved = conn.wait(|e| matches!(e, Event::ToplevelState { toplevel, workspace, .. } if *toplevel == id && *workspace == 1), 3000);
        c.check(moved.is_some(), "the window moves to workspace 2");
        conn.switch_workspace(1);
        let switched = conn.wait(|e| matches!(e, Event::Workspaces { current: 1, .. }), 2000);
        c.check(switched.is_some(), "switching workspaces is announced");
        conn.switch_workspace(0);
    }

    conn.set_selection("text/plain", "copied through hxproto v2");
    let selection = conn.selection("text/plain");
    c.check(selection.map(|(m, d)| m == "text/plain" && d == "copied through hxproto v2").unwrap_or(false), "clipboard keeps a typed selection");

    if hold {
        sys::sleep_ms(30000);
    }
    drop(window);
    drop(bar);
    if let Some(id) = toplevel {
        let closed = conn.wait(|e| matches!(e, Event::ToplevelClosed { toplevel } if *toplevel == id), 3000);
        c.check(closed.is_some(), "closing the window is announced");
    }
    println!("hxproto-check: {} failure(s)", c.failures);
    if c.failures == 0 { 0 } else { 1 }
}

entry!(main);
