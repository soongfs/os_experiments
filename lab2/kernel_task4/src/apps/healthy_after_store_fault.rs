use crate::syscall;

pub extern "C" fn healthy_after_store_fault() -> ! {
    let _ = syscall::write(b"[user] survived_store_page_fault\n");
    syscall::exit(0)
}
