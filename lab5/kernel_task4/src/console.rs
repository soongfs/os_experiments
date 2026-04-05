use crate::spinlock::SpinLock;
use core::fmt::{self, Write};
use core::ptr;

const UART0: *mut u8 = 0x1000_0000 as *mut u8;

static CONSOLE_LOCK: SpinLock<()> = SpinLock::new(());

pub fn write_byte(byte: u8) {
    unsafe {
        if byte == b'\n' {
            ptr::write_volatile(UART0, b'\r');
        }
        ptr::write_volatile(UART0, byte);
    }
}

struct KernelConsole;

impl Write for KernelConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            write_byte(byte);
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments<'_>) {
    let _guard = CONSOLE_LOCK.lock();
    let mut console = KernelConsole;
    let _ = console.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::console::print(core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::print!("\n");
    }};
    ($fmt:expr) => {{
        $crate::print!(concat!($fmt, "\n"));
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        $crate::print!(concat!($fmt, "\n"), $($arg)*);
    }};
}
