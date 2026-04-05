use core::arch::asm;

use crate::{TaskInfo, EFAULT, EINVAL, ENOSYS};

#[inline(never)]
fn invoke_syscall3(nr: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    let mut a0 = arg0 as isize;

    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0,
            in("a1") arg1,
            in("a2") arg2,
            in("a7") nr,
            options(nostack)
        );
    }

    a0
}

#[inline(never)]
pub fn write(bytes: &[u8]) -> isize {
    if bytes.is_empty() {
        return 0;
    }

    invoke_syscall3(crate::SYS_WRITE, bytes.as_ptr() as usize, bytes.len(), 0)
}

#[inline(never)]
pub fn get_taskinfo(task_info: *mut TaskInfo) -> isize {
    invoke_syscall3(crate::SYS_GET_TASKINFO, task_info as usize, 0, 0)
}

#[inline(never)]
pub fn shutdown(code: u32) -> ! {
    let _ = invoke_syscall3(crate::SYS_SHUTDOWN, code as usize, 0, 0);
    loop {
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }
}

pub fn describe_error(code: isize) -> &'static str {
    match code {
        EFAULT => "bad user pointer",
        EINVAL => "invalid argument",
        ENOSYS => "unknown syscall",
        _ => "unexpected error",
    }
}
