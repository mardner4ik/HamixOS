#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use hxtoolkit::{button, column, text, title, App, Context, Element, Settings};

#[derive(Default)]
struct Hello {
    clicks: u32,
}

#[derive(Clone)]
enum Msg {
    Press,
}

impl App for Hello {
    type Message = Msg;

    fn title(&self) -> String {
        String::from("Hello")
    }

    fn view(&self) -> Element<Msg> {
        column![
            title("Hello from hxtoolkit"),
            text("A tiny example of a Nook application.").dim(),
            text(format!("The button was pressed {} times.", self.clicks)).dim(),
            button("Press me").primary().on_press(Msg::Press),
        ]
        .padding(24)
        .spacing(10)
        .into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Press => {
                self.clicks += 1;
                if self.clicks == 5 {
                    cx.notify("Hello", "You pressed the button five times");
                }
            }
        }
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("hello").size(380, 220).fixed(), Hello::default())
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
