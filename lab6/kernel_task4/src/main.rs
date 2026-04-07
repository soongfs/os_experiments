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
    DirDiagnostics, FsStat, EEXIST, EFAULT, EINVAL, ENAMETOOLONG, ENOENT, ENOSYS, FS_KIND_DIR,
    FS_KIND_FILE, FS_PATH_MAX, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_DIR_DIAG, SYS_LIST_DIR,
    SYS_READ_AT, SYS_REMOVE, SYS_SHUTDOWN, SYS_STAT, SYS_TIME_US, SYS_WRITE, SYS_WRITE_AT,
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
const LEAF_PAYLOAD: &[u8] = b"kernel dir hierarchy validation payload\n";
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
    println!("[kernel] LAB6 kernel task4 directory structure expansion");
    println!(
        "[kernel] fs pools: max_inodes={} max_data_blocks={} max_ptr_blocks={} path_max={}",
        FS_MAX_INODES,
        FS_MAX_DATA_BLOCKS,
        FS_MAX_PTR_BLOCKS,
        FS_PATH_MAX
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
        SYS_DIR_DIAG => sys_dir_diag(frame.a0 as *mut DirDiagnostics),
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
    let mut dir_stat = FsStat::empty();
    let mut leaf_stat = FsStat::empty();
    let mut diag = DirDiagnostics::empty();
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
            uprintln!("[error] mkdir failed path={} code={}", path_as_str(&current_path, current_len), result);
            return false;
        }
        directories_created += 1;
        uprintln!("[mkdir] depth={} path={}", directories_created, path_as_str(&current_path, current_len));
    }
    let create_elapsed = syscall::time_us().saturating_sub(create_start);

    let existing_result = syscall::create_dir("/a");
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
            uprintln!("[error] list_dir failed path={} code={}", path_as_str(&parent_path, parent_len), result);
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

    if syscall::create_file(path_as_str(&leaf_path, leaf_len)) != 0 {
        uprintln!("[error] create_file failed");
        return false;
    }
    if syscall::write_at(path_as_str(&leaf_path, leaf_len), 0, LEAF_PAYLOAD)
        != LEAF_PAYLOAD.len() as isize
    {
        uprintln!("[error] write_at failed");
        return false;
    }
    if syscall::stat(path_as_str(&current_path, current_len), &mut dir_stat) != 0
        || syscall::stat(path_as_str(&leaf_path, leaf_len), &mut leaf_stat) != 0
    {
        uprintln!("[error] stat failed");
        return false;
    }
    if syscall::read_at(path_as_str(&leaf_path, leaf_len), 0, &mut readback)
        != LEAF_PAYLOAD.len() as isize
    {
        uprintln!("[error] readback failed");
        return false;
    }

    let missing_result = syscall::create_file("/ghost/branch/leaf.txt");
    let long_path_result = syscall::create_dir(long_path_string());

    let verify_elapsed = syscall::time_us().saturating_sub(verify_start);
    let readback_match = readback == *LEAF_PAYLOAD;
    uprintln!(
        "[leaf-file] path={} bytes={} highest_level={} readback_match={}",
        path_as_str(&leaf_path, leaf_len),
        leaf_stat.size_bytes,
        syscall::mapping_level_name(leaf_stat.highest_level),
        if readback_match { "yes" } else { "no" }
    );
    uprintln!(
        "[dir-stat] path={} kind={} child_count={} dirent_bytes={}",
        path_as_str(&current_path, current_len),
        if dir_stat.kind == FS_KIND_DIR { "directory" } else { "other" },
        dir_stat.child_count,
        dir_stat.size_bytes
    );
    uprintln!(
        "[errors] existing_dir={} missing_intermediate={} path_too_long={}",
        -existing_result,
        -missing_result,
        -long_path_result
    );
    if missing_result != ENOENT || long_path_result != ENAMETOOLONG {
        return false;
    }

    if syscall::dir_diag(&mut diag) != 0 {
        uprintln!("[error] dir_diag failed");
        return false;
    }
    uprintln!(
        "[kernel-diag] dir_inode_count={} dirent_bytes_per_inode={} resolve_calls={} path_components_split={} max_resolve_depth={} dirent_reads={} dirent_writes={}",
        diag.dir_inode_count,
        diag.dirent_bytes_per_inode,
        diag.resolve_calls,
        diag.path_components_split,
        diag.max_resolve_depth,
        diag.dirent_reads,
        diag.dirent_writes
    );

    let cleanup_start = syscall::time_us();
    if syscall::remove(path_as_str(&leaf_path, leaf_len)) != 0 {
        return false;
    }
    for _ in COMPONENTS.iter().rev() {
        if syscall::remove(path_as_str(&current_path, current_len)) != 0 {
            uprintln!("[error] remove dir failed path={}", path_as_str(&current_path, current_len));
            return false;
        }
        directories_removed += 1;
        truncate_last_component(&mut current_path, &mut current_len);
    }
    let cleanup_elapsed = syscall::time_us().saturating_sub(cleanup_start);

    let dirent_model_pass = dir_stat.kind == FS_KIND_DIR
        && dir_stat.child_count == 1
        && dir_stat.size_bytes == diag.dirent_bytes_per_inode
        && diag.dir_inode_count >= DEPTH as u64
        && diag.dirent_writes >= (DEPTH as u64 + 1);
    let resolver_pass = directories_traversed == DEPTH
        && diag.max_resolve_depth >= DEPTH as u64
        && diag.path_components_split >= DEPTH as u64 * 2;
    let file_ops_pass = leaf_stat.kind == FS_KIND_FILE
        && leaf_stat.size_bytes == LEAF_PAYLOAD.len() as u64
        && readback_match;

    uprintln!(
        "[summary] directories_created={} directories_traversed={} directories_removed={} file_bytes={}",
        directories_created,
        directories_traversed,
        directories_removed,
        leaf_stat.size_bytes
    );
    uprintln!(
        "[timing] create_us={} verify_us={} cleanup_us={}",
        create_elapsed,
        verify_elapsed,
        cleanup_elapsed
    );
    uprintln!(
        "[acceptance] directory inode stores dirents as internal directory content: {}",
        if dirent_model_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] path resolver recursively walks slash-separated components: {}",
        if resolver_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] deep directory tree create/traverse/remove and leaf file ops succeed: {}",
        if file_ops_pass && directories_removed == DEPTH { "PASS" } else { "FAIL" }
    );

    dirent_model_pass && resolver_pass && file_ops_pass && directories_removed == DEPTH
}

fn init_root_path(buffer: &mut [u8; FS_PATH_MAX]) -> usize {
    buffer[0] = b'/';
    1
}

fn append_component(buffer: &mut [u8; FS_PATH_MAX], len: &mut usize, component: &str) {
    if *len > 1 {
        buffer[*len] = b'/';
        *len += 1;
    }
    let bytes = component.as_bytes();
    buffer[*len..*len + bytes.len()].copy_from_slice(bytes);
    *len += bytes.len();
}

fn truncate_last_component(buffer: &mut [u8; FS_PATH_MAX], len: &mut usize) {
    while *len > 1 && buffer[*len - 1] != b'/' {
        *len -= 1;
    }
    if *len > 1 && buffer[*len - 1] == b'/' {
        *len -= 1;
    }
}

fn copy_path(dst: &mut [u8; FS_PATH_MAX], dst_len: &mut usize, src: &[u8; FS_PATH_MAX], src_len: usize) {
    dst[..src_len].copy_from_slice(&src[..src_len]);
    *dst_len = src_len;
}

fn path_as_str<'a>(buffer: &'a [u8; FS_PATH_MAX], len: usize) -> &'a str {
    unsafe { core::str::from_utf8_unchecked(&buffer[..len]) }
}

fn listing_contains(listing: &[u8], target: &[u8]) -> bool {
    let mut start = 0usize;
    while start < listing.len() {
        let mut end = start;
        while end < listing.len() && listing[end] != b'\n' {
            end += 1;
        }
        if &listing[start..end] == target {
            return true;
        }
        start = end + 1;
    }
    false
}

fn long_path_string() -> &'static str {
    static mut BUFFER: [u8; LONG_PATH_BUFFER_BYTES] = [0; LONG_PATH_BUFFER_BYTES];
    unsafe {
        BUFFER[0] = b'/';
        let mut len = 1usize;
        while len + 2 < LONG_PATH_BUFFER_BYTES {
            BUFFER[len] = b'x';
            len += 1;
            BUFFER[len] = b'/';
            len += 1;
        }
        core::str::from_utf8_unchecked(&BUFFER[..LONG_PATH_BUFFER_BYTES - 1])
    }
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

fn sys_write_at(path_ptr: *const u8, path_len: usize, offset: usize, buf_ptr: *const u8, buf_len: usize) -> isize {
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

fn sys_read_at(path_ptr: *const u8, path_len: usize, offset: usize, buf_ptr: *mut u8, buf_len: usize) -> isize {
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

fn sys_dir_diag(ptr: *mut DirDiagnostics) -> isize {
    let diag = match validated_user_mut::<DirDiagnostics>(ptr) {
        Ok(diag) => diag,
        Err(err) => return err,
    };
    *diag = fs::diagnostics();
    0
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
    let value = if code == 0 { 0x5555 } else { ((code << 1) | 1) as u16 };
    unsafe {
        ptr::write_volatile(0x100000 as *mut u16, value);
    }
    loop {
        unsafe {
            asm!("wfi", options(nostack, nomem));
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1);
}
