#![no_std]
#![no_main]

pub const FS_MAX_INODES: usize = 48;
pub const FS_MAX_DATA_BLOCKS: usize = 256;
pub const FS_MAX_PTR_BLOCKS: usize = 64;

mod abi;
mod console;
mod fs;
mod syscall;
mod trap;
mod user_console;

use abi::{
    FsStat, EEXIST, EFAULT, EINVAL, ENAMETOOLONG, ENOENT, ENOSYS, FS_KIND_FILE,
    FS_LEVEL_DIRECT, FS_PATH_MAX, FS_SINGLE_LIMIT_BYTES, FS_DOUBLE_LIMIT_BYTES,
    FS_TRIPLE_LIMIT_BYTES, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_LIST_DIR, SYS_READ_AT, SYS_REMOVE,
    SYS_SHUTDOWN, SYS_STAT, SYS_TIME_US, SYS_WRITE, SYS_WRITE_AT,
};
use core::arch::{asm, global_asm};
use core::mem::{align_of, size_of};
use core::panic::PanicInfo;
use core::ptr;

global_asm!(include_str!("boot.S"));

const DRAM_START: usize = 0x8000_0000;
const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;

const DEPTH: usize = 8;
const COMPONENTS: [&str; DEPTH] = ["a", "b", "c", "d", "e", "f", "g", "h"];
const LEAF_FILE_NAME: &str = "leaf_payload.txt";
const LEAF_PAYLOAD: &[u8] = b"qemu deep directory validation payload\n";
const LISTING_BUFFER_BYTES: usize = 256;
const LONG_PATH_BUFFER_BYTES: usize = FS_PATH_MAX + 64;

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __user_stack_top: u8;
    static __image_end: u8;

    fn enter_user_mode(user_entry: usize, user_sp: usize, kernel_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_kernel() -> ! {
    clear_bss();
    trap::init_trap_vector();
    configure_pmp();
    fs::init();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB6 task2 guest deep-directory validation");
    println!(
        "[kernel] fs pools: max_inodes={} max_data_blocks={} max_ptr_blocks={} path_max={}",
        FS_MAX_INODES,
        FS_MAX_DATA_BLOCKS,
        FS_MAX_PTR_BLOCKS,
        FS_PATH_MAX
    );
    println!(
        "[kernel] fs limits: single={} double={} triple={}",
        FS_SINGLE_LIMIT_BYTES,
        FS_DOUBLE_LIMIT_BYTES,
        FS_TRIPLE_LIMIT_BYTES
    );

    unsafe {
        enter_user_mode(
            user_entry as *const () as usize,
            ptr::addr_of!(__user_stack_top) as usize,
            ptr::addr_of!(__kernel_stack_top) as usize,
        )
    }
}

pub fn handle_syscall(frame: &mut trap::TrapFrame) {
    let result = match frame.a7 {
        SYS_WRITE => sys_write(frame.a0 as *const u8, frame.a1),
        SYS_TIME_US => mtime_us() as isize,
        SYS_CREATE_DIR => sys_create_dir(frame.a0 as *const u8, frame.a1),
        SYS_CREATE_FILE => sys_create_file(frame.a0 as *const u8, frame.a1),
        SYS_WRITE_AT => sys_write_at(
            frame.a0 as *const u8,
            frame.a1,
            frame.a2,
            frame.a3 as *const u8,
            frame.a4,
        ),
        SYS_READ_AT => sys_read_at(
            frame.a0 as *const u8,
            frame.a1,
            frame.a2,
            frame.a3 as *mut u8,
            frame.a4,
        ),
        SYS_STAT => sys_stat(frame.a0 as *const u8, frame.a1, frame.a2 as *mut FsStat),
        SYS_REMOVE => sys_remove(frame.a0 as *const u8, frame.a1),
        SYS_LIST_DIR => sys_list_dir(frame.a0 as *const u8, frame.a1, frame.a2 as *mut u8, frame.a3),
        SYS_SHUTDOWN => {
            let code = frame.a0 as u32;
            println!("[kernel] user requested shutdown with code {}", code);
            qemu_exit(code);
        }
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            ENOSYS
        }
    };

    frame.a0 = result as usize;
}

#[no_mangle]
pub extern "C" fn user_entry() -> ! {
    let pass = run_directory_test();
    syscall::shutdown(if pass { 0 } else { 1 });
}

fn run_directory_test() -> bool {
    let mut current_path = [0u8; FS_PATH_MAX];
    let mut current_len = init_root_path(&mut current_path);
    let mut leaf_path = [0u8; FS_PATH_MAX];
    let mut leaf_len = 0usize;
    let mut listing = [0u8; LISTING_BUFFER_BYTES];
    let mut readback = [0u8; LEAF_PAYLOAD.len()];
    let mut stat = FsStat::empty();
    let mut directories_created = 0usize;
    let mut directories_traversed = 0usize;
    let mut directories_removed = 0usize;

    uprintln!(
        "[config] depth={} example_prefix=/a/b/c/d/e path_max={}",
        DEPTH,
        FS_PATH_MAX
    );

    let create_start = syscall::time_us();
    for component in COMPONENTS {
        append_component(&mut current_path, &mut current_len, component);
        let result = syscall::create_dir(path_as_str(&current_path, current_len));
        if result != 0 {
            uprintln!(
                "[error] mkdir {} failed: {} ({})",
                path_as_str(&current_path, current_len),
                syscall::describe_error(result),
                result
            );
            return false;
        }
        directories_created += 1;
        uprintln!(
            "[mkdir] depth={} path={}",
            directories_created,
            path_as_str(&current_path, current_len)
        );
    }
    let create_elapsed = syscall::time_us().saturating_sub(create_start);

    let existing_result = syscall::create_dir("/a");
    uprintln!(
        "[existing-dir] path=/a errno={} message={}",
        -existing_result,
        syscall::describe_error(existing_result)
    );
    if existing_result != EEXIST {
        uprintln!("[error] expected EEXIST but got {}", existing_result);
        return false;
    }

    let verify_start = syscall::time_us();
    let mut parent_path = [0u8; FS_PATH_MAX];
    let mut parent_len = init_root_path(&mut parent_path);
    for component in COMPONENTS {
        let result = syscall::list_dir(path_as_str(&parent_path, parent_len), &mut listing);
        if result < 0 {
            uprintln!(
                "[error] list_dir {} failed: {} ({})",
                path_as_str(&parent_path, parent_len),
                syscall::describe_error(result),
                result
            );
            return false;
        }
        let found = listing_contains(&listing[..result as usize], component.as_bytes());
        uprintln!(
            "[traverse] parent={} child={} found={}",
            path_as_str(&parent_path, parent_len),
            component,
            if found { "yes" } else { "no" }
        );
        if !found {
            return false;
        }
        append_component(&mut parent_path, &mut parent_len, component);
        directories_traversed += 1;
    }

    copy_path(&mut leaf_path, &mut leaf_len, &current_path, current_len);
    append_component(&mut leaf_path, &mut leaf_len, LEAF_FILE_NAME);

    let file_create = syscall::create_file(path_as_str(&leaf_path, leaf_len));
    if file_create != 0 {
        uprintln!(
            "[error] create_file {} failed: {} ({})",
            path_as_str(&leaf_path, leaf_len),
            syscall::describe_error(file_create),
            file_create
        );
        return false;
    }

    let write_result = syscall::write_at(path_as_str(&leaf_path, leaf_len), 0, LEAF_PAYLOAD);
    if write_result != LEAF_PAYLOAD.len() as isize {
        uprintln!(
            "[error] write_at {} failed: {} ({})",
            path_as_str(&leaf_path, leaf_len),
            syscall::describe_error(write_result),
            write_result
        );
        return false;
    }

    let stat_result = syscall::stat(path_as_str(&leaf_path, leaf_len), &mut stat);
    if stat_result != 0 {
        uprintln!(
            "[error] stat {} failed: {} ({})",
            path_as_str(&leaf_path, leaf_len),
            syscall::describe_error(stat_result),
            stat_result
        );
        return false;
    }

    let read_result = syscall::read_at(path_as_str(&leaf_path, leaf_len), 0, &mut readback);
    if read_result != LEAF_PAYLOAD.len() as isize {
        uprintln!(
            "[error] read_at {} failed: {} ({})",
            path_as_str(&leaf_path, leaf_len),
            syscall::describe_error(read_result),
            read_result
        );
        return false;
    }
    let readback_match = readback == *LEAF_PAYLOAD;
    uprintln!(
        "[leaf-file] path={} bytes={} highest_level={} readback_match={}",
        path_as_str(&leaf_path, leaf_len),
        stat.size_bytes,
        syscall::mapping_level_name(stat.highest_level),
        if readback_match { "yes" } else { "no" }
    );
    if stat.kind != FS_KIND_FILE || stat.size_bytes != LEAF_PAYLOAD.len() as u64 || !readback_match {
        return false;
    }

    let missing_result = syscall::create_file("/ghost/branch/leaf.txt");
    uprintln!(
        "[missing-intermediate] path=/ghost/branch/leaf.txt errno={} message={}",
        -missing_result,
        syscall::describe_error(missing_result)
    );
    if missing_result != ENOENT {
        return false;
    }

    let mut long_path = [0u8; LONG_PATH_BUFFER_BYTES];
    let long_len = build_long_path(&mut long_path);
    let long_result = syscall::create_dir(path_as_str(&long_path, long_len));
    uprintln!(
        "[path-too-long] limit={} attempted_length={} errno={} message={}",
        FS_PATH_MAX - 1,
        long_len,
        -long_result,
        syscall::describe_error(long_result)
    );
    if long_result != ENAMETOOLONG {
        return false;
    }

    let verify_elapsed = syscall::time_us().saturating_sub(verify_start);

    let cleanup_start = syscall::time_us();
    let remove_leaf = syscall::remove(path_as_str(&leaf_path, leaf_len));
    if remove_leaf != 0 {
        uprintln!(
            "[error] remove file {} failed: {} ({})",
            path_as_str(&leaf_path, leaf_len),
            syscall::describe_error(remove_leaf),
            remove_leaf
        );
        return false;
    }
    uprintln!(
        "[cleanup-file] path={} removed=yes",
        path_as_str(&leaf_path, leaf_len)
    );

    while current_len > 1 {
        let result = syscall::remove(path_as_str(&current_path, current_len));
        if result != 0 {
            uprintln!(
                "[error] remove dir {} failed: {} ({})",
                path_as_str(&current_path, current_len),
                syscall::describe_error(result),
                result
            );
            return false;
        }
        uprintln!(
            "[cleanup-dir] depth={} path={} removed=yes",
            DEPTH - directories_removed,
            path_as_str(&current_path, current_len)
        );
        directories_removed += 1;
        pop_last_component(&mut current_len, &current_path);
    }
    let cleanup_elapsed = syscall::time_us().saturating_sub(cleanup_start);

    let deep_pass = directories_created == DEPTH && directories_traversed == DEPTH;
    let leaf_pass = readback_match && stat.highest_level == FS_LEVEL_DIRECT;
    let missing_pass = missing_result == ENOENT;
    let pass = deep_pass && leaf_pass && missing_pass && existing_result == EEXIST && long_result == ENAMETOOLONG;

    uprintln!(
        "[summary] directories_created={} directories_traversed={} directories_removed={} file_bytes={}",
        directories_created,
        directories_traversed,
        directories_removed,
        stat.size_bytes
    );
    uprintln!(
        "[timing] create_us={} verify_us={} cleanup_us={}",
        create_elapsed,
        verify_elapsed,
        cleanup_elapsed
    );
    uprintln!(
        "[acceptance] deep directory hierarchy created successfully: {}",
        if deep_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] leaf directory file operations succeeded: {}",
        if leaf_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] missing intermediate directory returned ENOENT: {}",
        if missing_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] existing directory returned EEXIST: {}",
        if existing_result == EEXIST { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] path too long returned ENAMETOOLONG: {}",
        if long_result == ENAMETOOLONG { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[done] guest deep-directory validation {}",
        if pass { "completed successfully" } else { "failed" }
    );
    pass
}

fn init_root_path(buffer: &mut [u8]) -> usize {
    buffer[0] = b'/';
    1
}

fn append_component(buffer: &mut [u8], length: &mut usize, component: &str) {
    let bytes = component.as_bytes();
    if *length > 1 {
        buffer[*length] = b'/';
        *length += 1;
    }
    buffer[*length..*length + bytes.len()].copy_from_slice(bytes);
    *length += bytes.len();
}

fn copy_path(target: &mut [u8], target_len: &mut usize, source: &[u8], source_len: usize) {
    target[..source_len].copy_from_slice(&source[..source_len]);
    *target_len = source_len;
}

fn pop_last_component(length: &mut usize, buffer: &[u8]) {
    if *length <= 1 {
        return;
    }

    let mut index = *length;
    while index > 1 && buffer[index - 1] != b'/' {
        index -= 1;
    }
    *length = if index > 1 { index - 1 } else { 1 };
}

fn path_as_str<'a>(buffer: &'a [u8], length: usize) -> &'a str {
    core::str::from_utf8(&buffer[..length]).unwrap_or("<invalid>")
}

fn listing_contains(listing: &[u8], name: &[u8]) -> bool {
    let mut start = 0usize;
    while start < listing.len() {
        let mut end = start;
        while end < listing.len() && listing[end] != b'\n' {
            end += 1;
        }
        if &listing[start..end] == name {
            return true;
        }
        start = end.saturating_add(1);
    }
    false
}

fn build_long_path(buffer: &mut [u8; LONG_PATH_BUFFER_BYTES]) -> usize {
    let mut length = init_root_path(buffer);
    let mut segment = 0usize;

    while length < FS_PATH_MAX + 8 {
        let bytes: &[u8] = if segment % 2 == 0 {
            b"segmentalpha"
        } else {
            b"segmentbeta"
        };
        if length > 1 {
            buffer[length] = b'/';
            length += 1;
        }
        buffer[length..length + bytes.len()].copy_from_slice(bytes);
        length += bytes.len();
        segment += 1;
    }

    length
}

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
    }
}

fn sys_write(ptr: *const u8, len: usize) -> isize {
    let bytes = match validated_user_bytes(ptr, len) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };

    for &byte in bytes {
        console::write_byte(byte);
    }

    len as isize
}

fn sys_create_dir(path_ptr: *const u8, path_len: usize) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    fs::create_dir(path)
}

fn sys_create_file(path_ptr: *const u8, path_len: usize) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    fs::create_file(path)
}

fn sys_write_at(
    path_ptr: *const u8,
    path_len: usize,
    offset: usize,
    buf_ptr: *const u8,
    buf_len: usize,
) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    let bytes = match validated_user_bytes(buf_ptr, buf_len) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    fs::write_at(path, offset, bytes)
}

fn sys_read_at(
    path_ptr: *const u8,
    path_len: usize,
    offset: usize,
    buf_ptr: *mut u8,
    buf_len: usize,
) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    let bytes = match validated_user_mut_bytes(buf_ptr, buf_len) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    fs::read_at(path, offset, bytes)
}

fn sys_stat(path_ptr: *const u8, path_len: usize, stat_ptr: *mut FsStat) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    let stat = match validated_user_mut::<FsStat>(stat_ptr) {
        Ok(stat) => stat,
        Err(err) => return err,
    };
    fs::stat(path, stat)
}

fn sys_remove(path_ptr: *const u8, path_len: usize) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    fs::remove(path)
}

fn sys_list_dir(path_ptr: *const u8, path_len: usize, buf_ptr: *mut u8, buf_len: usize) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    let bytes = match validated_user_mut_bytes(buf_ptr, buf_len) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    fs::list_dir(path, bytes)
}

fn validated_user_path<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Err(EINVAL);
    }
    if len >= FS_PATH_MAX {
        return Err(ENAMETOOLONG);
    }
    validated_user_bytes(ptr, len)
}

fn validated_user_bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Ok(&[]);
    }

    let addr = ptr as usize;
    if addr == 0 || !user_range_valid(addr, len) {
        return Err(EFAULT);
    }

    unsafe { Ok(core::slice::from_raw_parts(ptr, len)) }
}

fn validated_user_mut_bytes<'a>(ptr: *mut u8, len: usize) -> Result<&'a mut [u8], isize> {
    if len == 0 {
        return Ok(&mut []);
    }

    let addr = ptr as usize;
    if addr == 0 || !user_range_valid(addr, len) {
        return Err(EFAULT);
    }

    unsafe { Ok(core::slice::from_raw_parts_mut(ptr, len)) }
}

fn validated_user_mut<T>(ptr: *mut T) -> Result<&'static mut T, isize> {
    let addr = ptr as usize;
    if addr == 0 {
        return Err(EFAULT);
    }
    if addr % align_of::<T>() != 0 {
        return Err(EINVAL);
    }
    if !user_range_valid(addr, size_of::<T>()) {
        return Err(EFAULT);
    }

    unsafe { Ok(&mut *ptr) }
}

fn user_range_valid(addr: usize, len: usize) -> bool {
    if len == 0 {
        return true;
    }

    let end = match addr.checked_add(len) {
        Some(end) => end,
        None => return false,
    };

    addr >= DRAM_START && end <= user_memory_end()
}

fn user_memory_end() -> usize {
    ptr::addr_of!(__image_end) as usize
}

fn mtime_us() -> u64 {
    (read_mtime() * 1_000_000) / MTIME_FREQ_HZ
}

fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
}

fn configure_pmp() {
    unsafe {
        asm!(
            "csrw pmpaddr0, {}",
            in(reg) usize::MAX >> 2,
            options(nostack, nomem)
        );
        asm!("csrw pmpcfg0, {}", in(reg) 0x1fusize, options(nostack, nomem));
    }
}

pub fn qemu_exit(code: u32) -> ! {
    let value = if code == 0 {
        0x5555
    } else {
        (code << 16) | 0x3333
    };

    unsafe {
        ptr::write_volatile(0x0010_0000 as *mut u32, value);
    }

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1)
}
