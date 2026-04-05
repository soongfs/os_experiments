use crate::syscall;

pub extern "C" fn healthy_before_faults() -> ! {
    let _ = syscall::write(b"[user] healthy_before_faults\n");
    syscall::exit(0)
}
