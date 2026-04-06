use core::arch::asm;

use crate::abi::{
    FsStat, StatDiagnostics, EEXIST, EFBIG, EFAULT, EINVAL, EISDIR, ENAMETOOLONG, ENOENT,
    ENOSPC, ENOTDIR, ENOTEMPTY, ENOSYS, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_LIST_DIR,
    SYS_READ_AT, SYS_REMOVE, SYS_SHUTDOWN, SYS_STAT, SYS_STAT_DIAG, SYS_TIME_US, SYS_WRITE,
    SYS_WRITE_AT,
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

pub fn create_dir(path: &str) -> isize {
    invoke_syscall5(SYS_CREATE_DIR, path.as_ptr() as usize, path.len(), 0, 0, 0)
}

pub fn create_file(path: &str) -> isize {
    invoke_syscall5(SYS_CREATE_FILE, path.as_ptr() as usize, path.len(), 0, 0, 0)
}

pub fn write_at(path: &str, offset: usize, bytes: &[u8]) -> isize {
    invoke_syscall5(
        SYS_WRITE_AT,
        path.as_ptr() as usize,
        path.len(),
        offset,
        bytes.as_ptr() as usize,
        bytes.len(),
    )
}

pub fn read_at(path: &str, offset: usize, buffer: &mut [u8]) -> isize {
    invoke_syscall5(
        SYS_READ_AT,
        path.as_ptr() as usize,
        path.len(),
        offset,
        buffer.as_mut_ptr() as usize,
        buffer.len(),
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

pub fn stat_bad_buffer(path: &str, stat_ptr: usize) -> isize {
    invoke_syscall5(SYS_STAT, path.as_ptr() as usize, path.len(), stat_ptr, 0, 0)
}

pub fn stat_diag(diag: &mut StatDiagnostics) -> isize {
    invoke_syscall5(
        SYS_STAT_DIAG,
        diag as *mut StatDiagnostics as usize,
        0,
        0,
        0,
        0,
    )
}

pub fn remove(path: &str) -> isize {
    invoke_syscall5(SYS_REMOVE, path.as_ptr() as usize, path.len(), 0, 0, 0)
}

#[allow(dead_code)]
pub fn list_dir(path: &str, buffer: &mut [u8]) -> isize {
    invoke_syscall5(
        SYS_LIST_DIR,
        path.as_ptr() as usize,
        path.len(),
        buffer.as_mut_ptr() as usize,
        buffer.len(),
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

pub fn mapping_level_name(level: u8) -> &'static str {
    match level {
        0 => "direct",
        1 => "single",
        2 => "double",
        3 => "triple",
        _ => "unknown",
    }
}

#[allow(dead_code)]
pub fn describe_error(code: isize) -> &'static str {
    match code {
        EFAULT => "bad user pointer",
        EINVAL => "invalid argument",
        ENOENT => "no such file or directory",
        EEXIST => "already exists",
        ENOTDIR => "not a directory",
        EISDIR => "is a directory",
        ENOSPC => "no space left on device",
        ENAMETOOLONG => "path too long",
        ENOTEMPTY => "directory not empty",
        EFBIG => "file too large",
        ENOSYS => "unknown syscall",
        _ => "unexpected error",
    }
}
