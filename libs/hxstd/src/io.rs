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

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let _ = Stdout.write_fmt(args);
}

#[doc(hidden)]
pub fn _eprint(args: fmt::Arguments) {
    use fmt::Write;
    let _ = Stderr.write_fmt(args);
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
    ($($arg:tt)*) => {{
        $crate::io::_print(core::format_args!($($arg)*));
        $crate::print!("\n");
    }};
}

#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        $crate::io::_eprint(core::format_args!($($arg)*));
        $crate::io::_eprint(core::format_args!("\n"));
    }};
}
