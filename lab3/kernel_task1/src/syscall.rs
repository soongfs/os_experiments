use core::arch::asm;

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
pub fn yield_now() {
    let _ = invoke_syscall3(crate::SYS_YIELD, 0, 0, 0);
}

#[inline(never)]
pub fn finish(code: u64) -> ! {
    let _ = invoke_syscall3(crate::SYS_FINISH, code as usize, 0, 0);

    loop {
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }
}
