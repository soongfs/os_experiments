use core::arch::asm;

use crate::abi::{EIO, FILE_PAYLOAD_BYTES};

const SEMIHOST_OPEN: usize = 0x01;
const SEMIHOST_CLOSE: usize = 0x02;
const SEMIHOST_WRITE: usize = 0x05;
const SEMIHOST_READ: usize = 0x06;
const SEMIHOST_SEEK: usize = 0x0a;
const SEMIHOST_FLEN: usize = 0x0c;

const MODE_RB: usize = 1;
const WRITE_OPEN_MODES: [usize; 6] = [5, 4, 7, 6, 9, 8];

const DISK_IMAGE_PATH: &[u8] = b"/root/os_experiments/lab6/kernel_task5/artifacts/journal_disk.bin";
const MODE_FILE_PATH: &[u8] = b"/root/os_experiments/lab6/kernel_task5/artifacts/mode.txt";
const MODE_BUFFER_BYTES: usize = 64;

#[repr(C)]
struct OpenArgs {
    path: *const u8,
    mode: usize,
    len: usize,
}

#[repr(C)]
struct ReadWriteArgs {
    fd: usize,
    buf: *mut u8,
    len: usize,
}

#[repr(C)]
struct SeekArgs {
    fd: usize,
    pos: usize,
}

#[inline(always)]
fn semihost_call(op: usize, arg: usize) -> isize {
    let mut a0 = op as isize;
    unsafe {
        asm!(
            ".option push",
            ".option norvc",
            "slli zero, zero, 0x1f",
            "ebreak",
            "srai zero, zero, 0x7",
            ".option pop",
            inlateout("a0") a0,
            in("a1") arg,
            options(nostack)
        );
    }
    a0
}

fn open_file(path: &[u8], mode: usize) -> isize {
    let args = OpenArgs {
        path: path.as_ptr(),
        mode,
        len: path.len(),
    };
    semihost_call(SEMIHOST_OPEN, &args as *const OpenArgs as usize)
}

fn open_file_for_write(path: &[u8]) -> isize {
    let mut i = 0usize;
    while i < WRITE_OPEN_MODES.len() {
        let fd = open_file(path, WRITE_OPEN_MODES[i]);
        if fd >= 0 {
            return fd;
        }
        i += 1;
    }
    -1
}

fn close_file(fd: usize) {
    let mut handle = fd;
    let _ = semihost_call(SEMIHOST_CLOSE, &mut handle as *mut usize as usize);
}

fn seek_file(fd: usize, pos: usize) -> isize {
    let args = SeekArgs { fd, pos };
    semihost_call(SEMIHOST_SEEK, &args as *const SeekArgs as usize)
}

fn flen_file(fd: usize) -> isize {
    let mut handle = fd;
    semihost_call(SEMIHOST_FLEN, &mut handle as *mut usize as usize)
}

fn write_bytes(fd: usize, bytes: &[u8]) -> isize {
    let args = ReadWriteArgs {
        fd,
        buf: bytes.as_ptr() as *mut u8,
        len: bytes.len(),
    };
    let unwritten = semihost_call(SEMIHOST_WRITE, &args as *const ReadWriteArgs as usize);
    if unwritten < 0 {
        return EIO;
    }
    (bytes.len() - unwritten as usize) as isize
}

fn read_bytes(fd: usize, bytes: &mut [u8]) -> isize {
    let args = ReadWriteArgs {
        fd,
        buf: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    let unread = semihost_call(SEMIHOST_READ, &args as *const ReadWriteArgs as usize);
    if unread < 0 {
        return EIO;
    }
    (bytes.len() - unread as usize) as isize
}

pub fn load_mode() -> [u8; MODE_BUFFER_BYTES] {
    let mut mode = [0u8; MODE_BUFFER_BYTES];
    let fd = open_file(MODE_FILE_PATH, MODE_RB);
    if fd < 0 {
        return mode;
    }
    let fd = fd as usize;
    let _ = read_bytes(fd, &mut mode);
    close_file(fd);
    mode
}

pub fn read_disk_image(buffer: &mut [u8]) -> Result<bool, isize> {
    let fd = open_file(DISK_IMAGE_PATH, MODE_RB);
    if fd < 0 {
        return Ok(false);
    }
    let fd = fd as usize;

    let length = flen_file(fd);
    if length < 0 || length as usize == 0 {
        close_file(fd);
        return if length < 0 { Err(-111) } else { Ok(false) };
    }

    if seek_file(fd, 0) != 0 {
        close_file(fd);
        return Err(-112);
    }
    let read = read_bytes(fd, buffer);
    close_file(fd);
    if read < 0 {
        return Err(-113);
    }
    Ok(true)
}

pub fn write_disk_image(buffer: &[u8]) -> Result<(), isize> {
    let fd = open_file_for_write(DISK_IMAGE_PATH);
    if fd < 0 {
        return Err(-121);
    }
    let fd = fd as usize;
    if seek_file(fd, 0) != 0 {
        close_file(fd);
        return Err(-122);
    }
    let wrote = write_bytes(fd, buffer);
    close_file(fd);
    if wrote != buffer.len() as isize {
        return Err(-123);
    }
    Ok(())
}

pub fn disk_path() -> &'static str {
    "/root/os_experiments/lab6/kernel_task5/artifacts/journal_disk.bin"
}

pub fn mode_path() -> &'static str {
    "/root/os_experiments/lab6/kernel_task5/artifacts/mode.txt"
}

pub fn file_payload_bytes() -> usize {
    FILE_PAYLOAD_BYTES
}
