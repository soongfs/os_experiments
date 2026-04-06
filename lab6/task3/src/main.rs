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
    FsStat, EFAULT, EINVAL, ENAMETOOLONG, ENOSYS, FS_DEVICE_ID, FS_KIND_DIR, FS_KIND_FILE,
    FS_PATH_MAX, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_READ_AT, SYS_REMOVE, SYS_SHUTDOWN, SYS_STAT,
    SYS_TIME_US, SYS_WRITE, SYS_WRITE_AT,
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

const META_DIR: &str = "/meta";
const SAMPLE_FILE: &str = "/meta/sample.txt";
const SAMPLE_PAYLOAD: &[u8] = b"guest stat metadata sample\n";
const EXPECTED_DIR_INODE: u64 = 2;
const EXPECTED_FILE_INODE: u64 = 3;

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
    println!("[kernel] LAB6 task3 guest stat metadata validation");
    println!(
        "[kernel] fs pools: max_inodes={} max_data_blocks={} max_ptr_blocks={} path_max={}",
        FS_MAX_INODES,
        FS_MAX_DATA_BLOCKS,
        FS_MAX_PTR_BLOCKS,
        FS_PATH_MAX
    );
    println!(
        "[kernel] metadata fields: kind,size,inode_number,device_id,created_us,modified_us,blocks_used"
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
    let pass = run_stat_test();
    syscall::shutdown(if pass { 0 } else { 1 });
}

fn run_stat_test() -> bool {
    let mut dir_stat = FsStat::empty();
    let mut file_stat = FsStat::empty();
    let mut readback = [0u8; SAMPLE_PAYLOAD.len()];

    uprintln!(
        "[config] dir_path={} file_path={} payload_bytes={}",
        META_DIR,
        SAMPLE_FILE,
        SAMPLE_PAYLOAD.len()
    );

    let setup_start = syscall::time_us();
    let dir_create = syscall::create_dir(META_DIR);
    if dir_create != 0 {
        uprintln!(
            "[error] create_dir failed: {} ({})",
            syscall::describe_error(dir_create),
            dir_create
        );
        return false;
    }
    let file_create = syscall::create_file(SAMPLE_FILE);
    if file_create != 0 {
        uprintln!(
            "[error] create_file failed: {} ({})",
            syscall::describe_error(file_create),
            file_create
        );
        return false;
    }
    let write_result = syscall::write_at(SAMPLE_FILE, 0, SAMPLE_PAYLOAD);
    if write_result != SAMPLE_PAYLOAD.len() as isize {
        uprintln!(
            "[error] write_at failed: {} ({})",
            syscall::describe_error(write_result),
            write_result
        );
        return false;
    }
    let setup_elapsed = syscall::time_us().saturating_sub(setup_start);

    let stat_start = syscall::time_us();
    let dir_stat_result = syscall::stat(META_DIR, &mut dir_stat);
    let file_stat_result = syscall::stat(SAMPLE_FILE, &mut file_stat);
    if dir_stat_result != 0 || file_stat_result != 0 {
        uprintln!(
            "[error] stat failed: dir={} ({}) file={} ({})",
            syscall::describe_error(dir_stat_result),
            dir_stat_result,
            syscall::describe_error(file_stat_result),
            file_stat_result
        );
        return false;
    }
    let stat_elapsed = syscall::time_us().saturating_sub(stat_start);

    let read_result = syscall::read_at(SAMPLE_FILE, 0, &mut readback);
    if read_result != SAMPLE_PAYLOAD.len() as isize || readback != *SAMPLE_PAYLOAD {
        uprintln!(
            "[error] readback failed: result={} match={}",
            read_result,
            if readback == *SAMPLE_PAYLOAD { "yes" } else { "no" }
        );
        return false;
    }

    print_formatted_stat("directory", META_DIR, &dir_stat);
    print_formatted_stat("file", SAMPLE_FILE, &file_stat);

    let file_size_match = file_stat.size_bytes == SAMPLE_PAYLOAD.len() as u64;
    let file_inode_match = file_stat.inode_number == EXPECTED_FILE_INODE;
    let file_device_match = file_stat.device_id == FS_DEVICE_ID;
    let dir_inode_match = dir_stat.inode_number == EXPECTED_DIR_INODE;
    let dir_device_match = dir_stat.device_id == FS_DEVICE_ID;
    let dir_kind_match = dir_stat.kind == FS_KIND_DIR && dir_stat.child_count == 1;
    let file_kind_match = file_stat.kind == FS_KIND_FILE;
    let timestamp_supported = file_stat.created_us > 0
        && file_stat.modified_us >= file_stat.created_us
        && dir_stat.created_us > 0
        && dir_stat.modified_us >= dir_stat.created_us;

    uprintln!(
        "[compare] file_size expected={} actual={} match={}",
        SAMPLE_PAYLOAD.len(),
        file_stat.size_bytes,
        if file_size_match { "yes" } else { "no" }
    );
    uprintln!(
        "[compare] file_inode expected={} actual={} match={}",
        EXPECTED_FILE_INODE,
        file_stat.inode_number,
        if file_inode_match { "yes" } else { "no" }
    );
    uprintln!(
        "[compare] file_device expected={:#x} actual={:#x} match={}",
        FS_DEVICE_ID,
        file_stat.device_id,
        if file_device_match { "yes" } else { "no" }
    );
    uprintln!(
        "[compare] dir_inode expected={} actual={} match={}",
        EXPECTED_DIR_INODE,
        dir_stat.inode_number,
        if dir_inode_match { "yes" } else { "no" }
    );
    uprintln!(
        "[compare] dir_device expected={:#x} actual={:#x} match={}",
        FS_DEVICE_ID,
        dir_stat.device_id,
        if dir_device_match { "yes" } else { "no" }
    );
    uprintln!(
        "[support] unsupported_fields=uid,gid,mode,nlink reason=minimal teaching fs exports only kind,size,inode,device,timestamps,block usage"
    );

    let cleanup_start = syscall::time_us();
    let remove_file = syscall::remove(SAMPLE_FILE);
    let remove_dir = syscall::remove(META_DIR);
    let cleanup_elapsed = syscall::time_us().saturating_sub(cleanup_start);
    if remove_file != 0 || remove_dir != 0 {
        uprintln!(
            "[error] cleanup failed: file={} ({}) dir={} ({})",
            syscall::describe_error(remove_file),
            remove_file,
            syscall::describe_error(remove_dir),
            remove_dir
        );
        return false;
    }

    let pass = file_size_match
        && file_inode_match
        && file_device_match
        && dir_inode_match
        && dir_device_match
        && dir_kind_match
        && file_kind_match
        && timestamp_supported;

    uprintln!(
        "[timing] setup_us={} stat_us={} cleanup_us={}",
        setup_elapsed,
        stat_elapsed,
        cleanup_elapsed
    );
    uprintln!("[acceptance] stat structure printed in formatted form: PASS");
    uprintln!(
        "[acceptance] file size, inode number, and device id match actual filesystem values: {}",
        if file_size_match && file_inode_match && file_device_match {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!(
        "[acceptance] directory inode number and device id match actual filesystem values: {}",
        if dir_inode_match && dir_device_match && dir_kind_match {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!(
        "[acceptance] supported timestamp fields are populated consistently: {}",
        if timestamp_supported { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[done] guest stat metadata validation {}",
        if pass { "completed successfully" } else { "failed" }
    );
    pass
}

fn print_formatted_stat(label: &str, path: &str, stat: &FsStat) {
    uprintln!("[stat] label={} path={}", label, path);
    uprintln!("  Type: {}", stat_kind_name(stat.kind));
    uprintln!("  File Size: {}", stat.size_bytes);
    uprintln!("  Inode Number: {}", stat.inode_number);
    uprintln!("  Device ID: {:#x}", stat.device_id);
    uprintln!("  Blocks Used: {}", stat.blocks_used);
    uprintln!("  Child Count: {}", stat.child_count);
    uprintln!("  Mapping Level: {}", syscall::mapping_level_name(stat.highest_level));
    uprintln!("  Created Timestamp (us): {}", stat.created_us);
    uprintln!("  Modified Timestamp (us): {}", stat.modified_us);
}

fn stat_kind_name(kind: u8) -> &'static str {
    match kind {
        FS_KIND_FILE => "regular",
        FS_KIND_DIR => "directory",
        _ => "unknown",
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
    let value = if code == 0 { 0x5555 } else { (code << 16) | 0x3333 };

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
