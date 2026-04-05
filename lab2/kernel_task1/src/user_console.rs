use core::fmt::{self, Write};

use crate::syscall;

struct UserConsole;

impl Write for UserConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if syscall::write(s.as_bytes()) < 0 {
            return Err(fmt::Error);
        }

        Ok(())
    }
}

pub fn print(args: fmt::Arguments<'_>) {
    let mut console = UserConsole;
    let _ = console.write_fmt(args);
}

#[macro_export]
macro_rules! uprint {
    ($($arg:tt)*) => {{
        $crate::user_console::print(core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! uprintln {
    () => {{
        $crate::uprint!("\n");
    }};
    ($fmt:expr) => {{
        $crate::uprint!(concat!($fmt, "\n"));
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        $crate::uprint!(concat!($fmt, "\n"), $($arg)*);
    }};
}
