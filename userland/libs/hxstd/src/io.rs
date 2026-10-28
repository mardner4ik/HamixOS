use core::fmt;

pub struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::sys::write(1, s.as_bytes());
        Ok(())
    }
}

pub struct Stderr;

impl fmt::Write for Stderr {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::sys::write(2, s.as_bytes());
        Ok(())
    }
}

fn emit(fd: u64, args: fmt::Arguments, newline: bool) {
    match args.as_str() {
        Some(text) if !newline => {
            crate::sys::write(fd, text.as_bytes());
        }
        _ => {
            let mut text = alloc::fmt::format(args);
            if newline {
                text.push('\n');
            }
            crate::sys::write(fd, text.as_bytes());
        }
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    emit(1, args, false);
}

#[doc(hidden)]
pub fn _println(args: fmt::Arguments) {
    emit(1, args, true);
}

#[doc(hidden)]
pub fn _eprint(args: fmt::Arguments) {
    emit(2, args, false);
}

#[doc(hidden)]
pub fn _eprintln(args: fmt::Arguments) {
    emit(2, args, true);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::io::_print(core::format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {
        $crate::io::_println(core::format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {
        $crate::io::_eprintln(core::format_args!($($arg)*))
    };
}
