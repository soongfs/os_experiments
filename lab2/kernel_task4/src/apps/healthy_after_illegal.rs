use crate::syscall;

pub extern "C" fn healthy_after_illegal() -> ! {
    let _ = syscall::write(b"[user] survived_illegal_instruction\n");
    syscall::exit(0)
}
