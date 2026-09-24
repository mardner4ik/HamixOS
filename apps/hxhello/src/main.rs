#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use hamix_std::{entry, eprintln, sys};
use hxclient::ui::{self, theme, Style, Ui};
use hxclient::{Event, Window, MOUSE_MOVE, MOUSE_PRESS};
use vellum::{Area, Painter};

const INITIAL_W: i32 = 380;
const INITIAL_H: i32 = 220;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}

fn draw(window: &mut Window, ui: &Ui, clicks: u32, hover: bool) {
    let buffer = window.buffer();
    let mut p = Painter::new(buffer, ww(), hh());
    p.fill(Area::new(0, 0, ww(), hh()), theme::bg());
    p.text(&ui.title, 24, 24, "Hello from hxclient", theme::text());
    p.text(&ui.font, 24, 54, "A tiny example of a Nook application.", theme::dim());
    p.text(&ui.font, 24, 76, &format!("The button was pressed {} times.", clicks), theme::dim());
    let button = Area::new(24, hh() - 60, 140, 36);
    ui::button(&mut p, ui, button, "Press me", Style::Primary, hover, true);
    window.present();
}

fn main() -> i32 {
    let ui = Ui::load();
    let mut window = match Window::open("Hello", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxhello: {}", e);
            return 1;
        }
    };
    window.set_icon("hello");
    window.set_fixed_size();
    sys::cmd_register("hello-window", "/usr/bin/hxhello", &[] as &[&str]);
    let button = Area::new(24, hh() - 60, 140, 36);
    let mut clicks = 0;
    let mut hover = false;
    draw(&mut window, &ui, clicks, hover);
    loop {
        match window.wait_event(-1) {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Mouse { x, y, kind: MOUSE_MOVE, .. }) => {
                if button.contains(x, y) != hover {
                    hover = !hover;
                    draw(&mut window, &ui, clicks, hover);
                }
            }
            Some(Event::Mouse { x, y, kind: MOUSE_PRESS, .. }) => {
                if button.contains(x, y) {
                    clicks += 1;
                    if clicks == 5 {
                        hxclient::notify("Hello", "You pressed the button five times");
                    }
                    draw(&mut window, &ui, clicks, hover);
                }
            }
            _ => {}
        }
    }
}

entry!(main);
