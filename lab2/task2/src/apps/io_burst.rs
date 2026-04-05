use crate::syscall;

const IO_WRITES: usize = crate::IO_BURST_WRITES as usize;

pub extern "C" fn io_burst() -> ! {
    let mut index = 0;

    while index < IO_WRITES {
        if syscall::write(b"io\n") < 0 {
            syscall::exit(1);
        }
        index += 1;
    }

    syscall::exit(0)
}
