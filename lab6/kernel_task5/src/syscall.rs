use core::arch::asm;

use crate::abi::{
    FsStat, JournalDiagnostics, EFAULT, EINVAL, ENOENT, ENOSYS, SYS_BOOT_MODE,
    SYS_JOURNAL_DIAG, SYS_READ_FILE, SYS_SHUTDOWN, SYS_STAT, SYS_TIME_US, SYS_TXN_WRITE,
    SYS_WRITE,
};

#[inline(never)]
fn invoke_syscall5(
    nr: usize,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> isize {
    let mut a0 = arg0 as isize;

    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0,
            in("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
            in("a4") arg4,
            in("a7") nr,
            options(nostack)
        );
    }

    a0
}

pub fn write(bytes: &[u8]) -> isize {
    if bytes.is_empty() {
        return 0;
    }
    invoke_syscall5(SYS_WRITE, bytes.as_ptr() as usize, bytes.len(), 0, 0, 0)
}

pub fn time_us() -> u64 {
    invoke_syscall5(SYS_TIME_US, 0, 0, 0, 0, 0) as u64
}

pub fn boot_mode() -> u64 {
    invoke_syscall5(SYS_BOOT_MODE, 0, 0, 0, 0, 0) as u64
}

pub fn txn_write(path: &str, bytes: &[u8]) -> isize {
    invoke_syscall5(
        SYS_TXN_WRITE,
        path.as_ptr() as usize,
        path.len(),
        bytes.as_ptr() as usize,
        bytes.len(),
        0,
    )
}

pub fn read_file(path: &str, buffer: &mut [u8]) -> isize {
    invoke_syscall5(
        SYS_READ_FILE,
        path.as_ptr() as usize,
        path.len(),
        buffer.as_mut_ptr() as usize,
        buffer.len(),
        0,
    )
}

pub fn stat(path: &str, stat: &mut FsStat) -> isize {
    invoke_syscall5(
        SYS_STAT,
        path.as_ptr() as usize,
        path.len(),
        stat as *mut FsStat as usize,
        0,
        0,
    )
}

pub fn journal_diag(diag: &mut JournalDiagnostics) -> isize {
    invoke_syscall5(
        SYS_JOURNAL_DIAG,
        diag as *mut JournalDiagnostics as usize,
        0,
        0,
        0,
        0,
    )
}

pub fn shutdown(code: u32) -> ! {
    let _ = invoke_syscall5(SYS_SHUTDOWN, code as usize, 0, 0, 0, 0);
    loop {
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }
}

#[allow(dead_code)]
pub fn describe_error(code: isize) -> &'static str {
    match code {
        EFAULT => "bad user pointer",
        EINVAL => "invalid argument",
        ENOENT => "no such file or directory",
        ENOSYS => "unknown syscall",
        _ => "unexpected error",
    }
}
