use crate::drivers::input::mouse;

pub fn on_boot_mouse_report(report: &[u8]) {
    if report.len() < 3 {
        return;
    }
    let buttons = report[0] & 0x07;
    let dx = report[1] as i8 as i32;
    let dy = report[2] as i8 as i32;
    let wheel = if report.len() >= 4 { report[3] as i8 as i32 } else { 0 };
    mouse::inject(dx, -dy, buttons, wheel);
}
