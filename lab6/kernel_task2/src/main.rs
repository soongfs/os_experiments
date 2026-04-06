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
    FsStat, StatDiagnostics, EFAULT, EINVAL, ENAMETOOLONG, ENOENT, ENOSYS, FS_DEVICE_ID,
    FS_KIND_DIR, FS_KIND_FILE, FS_PATH_MAX, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_LIST_DIR,
    SYS_READ_AT, SYS_REMOVE, SYS_SHUTDOWN, SYS_STAT, SYS_STAT_DIAG, SYS_TIME_US, SYS_WRITE,
    SYS_WRITE_AT,
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
const SAMPLE_PAYLOAD: &[u8] = b"kernel stat syscall sample\n";
const EXPECTED_DIR_INODE: u64 = 2;
const EXPECTED_FILE_INODE: u64 = 3;
const INVALID_RELATIVE_PATH: &str = "relative/path";
const MISSING_PATH: &str = "/missing/stat.txt";

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
    println!("[kernel] LAB6 kernel task2 stat syscall support");
    println!(
        "[kernel] fs pools: max_inodes={} max_data_blocks={} max_ptr_blocks={} path_max={}",
        FS_MAX_INODES,
        FS_MAX_DATA_BLOCKS,
        FS_MAX_PTR_BLOCKS,
        FS_PATH_MAX
    );
    println!(
        "[kernel] stat export fields: kind,size,inode_number,device_id,blocks_used,child_count,created_us,modified_us"
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
        SYS_STAT_DIAG => sys_stat_diag(frame.a0 as *mut StatDiagnostics),
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
    let pass = run_stat_syscall_test();
    syscall::shutdown(if pass { 0 } else { 1 });
}

fn run_stat_syscall_test() -> bool {
    let mut dir_stat = FsStat::empty();
    let mut file_stat = FsStat::empty();
    let mut diag = StatDiagnostics::empty();
    let mut readback = [0u8; SAMPLE_PAYLOAD.len()];

    uprintln!(
        "[config] dir_path={} file_path={} payload_bytes={} invalid_path={} missing_path={}",
        META_DIR,
        SAMPLE_FILE,
        SAMPLE_PAYLOAD.len(),
        INVALID_RELATIVE_PATH,
        MISSING_PATH
    );

    let setup_start = syscall::time_us();
    if syscall::create_dir(META_DIR) != 0 {
        uprintln!("[error] create_dir failed");
        return false;
    }
    if syscall::create_file(SAMPLE_FILE) != 0 {
        uprintln!("[error] create_file failed");
        return false;
    }
    if syscall::write_at(SAMPLE_FILE, 0, SAMPLE_PAYLOAD) != SAMPLE_PAYLOAD.len() as isize {
        uprintln!("[error] write_at failed");
        return false;
    }
    let setup_elapsed = syscall::time_us().saturating_sub(setup_start);

    poison_stat(&mut dir_stat);
    poison_stat(&mut file_stat);

    let stat_start = syscall::time_us();
    let dir_stat_result = syscall::stat(META_DIR, &mut dir_stat);
    let file_stat_result = syscall::stat(SAMPLE_FILE, &mut file_stat);
    let missing_result = syscall::stat(MISSING_PATH, &mut FsStat::empty());
    let invalid_path_result = syscall::stat_bad_buffer(INVALID_RELATIVE_PATH, &mut FsStat::empty() as *mut FsStat as usize);
    let bad_buffer_result = syscall::stat_bad_buffer(SAMPLE_FILE, 0);
    let stat_elapsed = syscall::time_us().saturating_sub(stat_start);

    if dir_stat_result != 0 || file_stat_result != 0 {
        uprintln!(
            "[error] stat success path failed: dir={} file={}",
            dir_stat_result,
            file_stat_result
        );
        return false;
    }

    if syscall::read_at(SAMPLE_FILE, 0, &mut readback) != SAMPLE_PAYLOAD.len() as isize
        || readback != *SAMPLE_PAYLOAD
    {
        uprintln!("[error] readback failed");
        return false;
    }

    if syscall::stat_diag(&mut diag) != 0 {
        uprintln!("[error] stat_diag failed");
        return false;
    }

    print_formatted_stat("directory", META_DIR, &dir_stat);
    print_formatted_stat("file", SAMPLE_FILE, &file_stat);
    uprintln!(
        "[kernel-stat] calls={} successful_lookups={} failed_lookups={} successful_copyouts={} last_inode={} last_kind={} last_size={} last_error={}",
        diag.stat_calls,
        diag.successful_lookups,
        diag.failed_lookups,
        diag.successful_copyouts,
        diag.last_inode_number,
        diag.last_kind,
        diag.last_size_bytes,
        diag.last_error
    );
    uprintln!(
        "[errors] missing_path_result={} invalid_path_result={} bad_buffer_result={}",
        missing_result,
        invalid_path_result,
        bad_buffer_result
    );

    let file_size_match = file_stat.size_bytes == SAMPLE_PAYLOAD.len() as u64;
    let file_inode_match = file_stat.inode_number == EXPECTED_FILE_INODE;
    let file_device_match = file_stat.device_id == FS_DEVICE_ID;
    let dir_inode_match = dir_stat.inode_number == EXPECTED_DIR_INODE;
    let dir_device_match = dir_stat.device_id == FS_DEVICE_ID;
    let dir_kind_match = dir_stat.kind == FS_KIND_DIR && dir_stat.child_count == 1;
    let file_kind_match = file_stat.kind == FS_KIND_FILE && file_stat.blocks_used == 1;
    let timestamp_supported = file_stat.created_us > 0
        && file_stat.modified_us >= file_stat.created_us
        && dir_stat.created_us > 0
        && dir_stat.modified_us >= dir_stat.created_us;
    let error_paths_match =
        missing_result == ENOENT && invalid_path_result == EINVAL && bad_buffer_result == EFAULT;
    let diag_match = diag.stat_calls == 5
        && diag.successful_lookups == 2
        && diag.failed_lookups == 3
        && diag.successful_copyouts == 2
        && diag.last_error == EFAULT as i64;

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
        "[timing] setup_us={} stat_us={}",
        setup_elapsed,
        stat_elapsed
    );

    let cleanup_start = syscall::time_us();
    let remove_file = syscall::remove(SAMPLE_FILE);
    let remove_dir = syscall::remove(META_DIR);
    let cleanup_elapsed = syscall::time_us().saturating_sub(cleanup_start);
    if remove_file != 0 || remove_dir != 0 {
        uprintln!(
            "[error] cleanup failed: file={} dir={}",
            remove_file,
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
        && timestamp_supported
        && error_paths_match
        && diag_match;

    uprintln!(
        "[acceptance] stat locates the correct inode for file and directory paths: {}",
        if dir_inode_match && file_inode_match && dir_kind_match && file_kind_match {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!(
        "[acceptance] metadata is converted into user-space stat buffers correctly: {}",
        if file_size_match && file_device_match && dir_device_match && timestamp_supported && diag.successful_copyouts == 2 {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!(
        "[acceptance] missing file, invalid path, and bad user buffer return expected errors: {}",
        if error_paths_match { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] kernel stat diagnostics report lookup and copyout activity: {}",
        if diag_match { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[timing] cleanup_us={}",
        cleanup_elapsed
    );
    uprintln!(
        "[done] kernel stat syscall validation {}",
        if pass { "completed successfully" } else { "failed" }
    );
    pass
}

fn poison_stat(stat: &mut FsStat) {
    *stat = FsStat {
        kind: 0xaa,
        highest_level: 0xbb,
        _reserved: [0xcc; 6],
        inode_number: u64::MAX,
        device_id: u64::MAX,
        size_bytes: u64::MAX,
        blocks_used: u64::MAX,
        child_count: u64::MAX,
        created_us: u64::MAX,
        modified_us: u64::MAX,
    };
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
        Err(err) => {
            fs::note_stat_fault(err);
            return err;
        }
    };
    let stat = match validated_user_mut::<FsStat>(stat_ptr) {
        Ok(stat) => stat,
        Err(err) => {
            fs::note_stat_fault(err);
            return err;
        }
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

fn sys_stat_diag(ptr: *mut StatDiagnostics) -> isize {
    let diag = match validated_user_mut::<StatDiagnostics>(ptr) {
        Ok(diag) => diag,
        Err(err) => return err,
    };
    *diag = fs::stat_diagnostics();
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
    let value = if code == 0 {
        0x5555
    } else {
        ((code << 1) | 1) as u16
    };
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
