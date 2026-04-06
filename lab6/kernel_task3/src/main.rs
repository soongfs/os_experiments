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
    FsStat, MmapDiagnostics, EBUSY, EFAULT, EINVAL, ENAMETOOLONG, ENOSYS, FS_DEVICE_ID,
    FS_KIND_FILE, FS_PATH_MAX, MMAP_PAGE_SIZE, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_LIST_DIR,
    SYS_MMAP, SYS_MMAP_DIAG, SYS_MSYNC, SYS_MUNMAP, SYS_READ_AT, SYS_REMOVE, SYS_SHUTDOWN,
    SYS_STAT, SYS_TIME_US, SYS_WRITE, SYS_WRITE_AT,
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

const MAP_PATH: &str = "/mapped.bin";
const VERIFY_POS_A: usize = 0;
const VERIFY_POS_B: usize = 37;
const VERIFY_POS_C: usize = 1024;
const VERIFY_POS_D: usize = MMAP_PAGE_SIZE - 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct MappingState {
    active: bool,
    loaded: bool,
    writable: bool,
    _padding: u8,
    path_len: usize,
    file_size: usize,
    path: [u8; FS_PATH_MAX],
}

impl MappingState {
    const fn empty() -> Self {
        Self {
            active: false,
            loaded: false,
            writable: false,
            _padding: 0,
            path_len: 0,
            file_size: 0,
            path: [0; FS_PATH_MAX],
        }
    }
}

static mut MMAP_PAGE: [u8; MMAP_PAGE_SIZE] = [0; MMAP_PAGE_SIZE];
static mut MMAP_SHADOW: [u8; MMAP_PAGE_SIZE] = [0; MMAP_PAGE_SIZE];
static mut MAPPING_STATE: MappingState = MappingState::empty();
static mut MMAP_DIAGNOSTICS: MmapDiagnostics = MmapDiagnostics::empty();

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
    fs::init();
    configure_pmp(false);

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB6 kernel task3 mmap file mapping");
    println!(
        "[kernel] fs pools: max_inodes={} max_data_blocks={} max_ptr_blocks={} path_max={}",
        FS_MAX_INODES,
        FS_MAX_DATA_BLOCKS,
        FS_MAX_PTR_BLOCKS,
        FS_PATH_MAX
    );
    println!(
        "[kernel] mmap window: addr={:#x} length={} bytes",
        mmap_window_start(),
        MMAP_PAGE_SIZE
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
        SYS_MMAP => sys_mmap(frame.a0 as *const u8, frame.a1, frame.a2),
        SYS_MSYNC => sys_msync(frame.a0, frame.a1),
        SYS_MUNMAP => sys_munmap(frame.a0, frame.a1),
        SYS_MMAP_DIAG => sys_mmap_diag(frame.a0 as *mut MmapDiagnostics),
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

pub fn handle_page_fault(mcause: usize, mtval: usize) -> bool {
    let start = mmap_window_start();
    let end = start + MMAP_PAGE_SIZE;

    if mtval < start || mtval >= end {
        return false;
    }

    unsafe {
        if !MAPPING_STATE.active {
            MMAP_DIAGNOSTICS.last_error = EINVAL as i64;
            return false;
        }

        MMAP_DIAGNOSTICS.page_faults += 1;
        MMAP_DIAGNOSTICS.last_fault_addr = mtval as u64;
    }

    let loaded_bytes = load_mapped_page();
    if loaded_bytes < 0 {
        unsafe {
            MMAP_DIAGNOSTICS.last_error = loaded_bytes as i64;
        }
        return false;
    }

    configure_pmp(true);
    println!(
        "[page-fault] cause={} addr={:#x} loaded_bytes={}",
        match mcause {
            5 => "load-access",
            7 => "store-access",
            13 => "load",
            15 => "store",
            _ => "unknown",
        },
        mtval,
        loaded_bytes
    );
    true
}

#[no_mangle]
pub extern "C" fn user_entry() -> ! {
    let pass = run_mmap_test();
    syscall::shutdown(if pass { 0 } else { 1 });
}

fn run_mmap_test() -> bool {
    let mut initial = [0u8; MMAP_PAGE_SIZE];
    let mut verify = [0u8; MMAP_PAGE_SIZE];
    let mut stat = FsStat::empty();
    let mut diag = MmapDiagnostics::empty();

    fill_pattern(&mut initial);
    uprintln!(
        "[config] file_path={} mmap_len={} mmap_window={:#x}",
        MAP_PATH,
        MMAP_PAGE_SIZE,
        mmap_window_start()
    );

    let setup_start = syscall::time_us();
    if syscall::create_file(MAP_PATH) != 0 {
        uprintln!("[error] create_file failed");
        return false;
    }
    if syscall::write_at(MAP_PATH, 0, &initial) != MMAP_PAGE_SIZE as isize {
        uprintln!("[error] seed write failed");
        return false;
    }
    let setup_elapsed = syscall::time_us().saturating_sub(setup_start);

    let mmap_result = syscall::mmap(MAP_PATH, MMAP_PAGE_SIZE);
    if mmap_result < 0 {
        uprintln!("[error] mmap failed: {}", mmap_result);
        return false;
    }
    let mapped_addr = mmap_result as usize;

    let fault_start = syscall::time_us();
    let first_before = unsafe { ptr::read_volatile(mapped_addr as *const u8) };
    let second_before = unsafe { ptr::read_volatile((mapped_addr + VERIFY_POS_B) as *const u8) };
    let fault_elapsed = syscall::time_us().saturating_sub(fault_start);

    let initial_fault_match =
        first_before == initial[VERIFY_POS_A] && second_before == initial[VERIFY_POS_B];

    unsafe {
        ptr::write_volatile((mapped_addr + VERIFY_POS_A) as *mut u8, 0xa5);
        ptr::write_volatile((mapped_addr + VERIFY_POS_B) as *mut u8, 0x5a);
    }

    let msync_start = syscall::time_us();
    let msync_result = syscall::msync(mapped_addr, MMAP_PAGE_SIZE);
    let msync_elapsed = syscall::time_us().saturating_sub(msync_start);
    if msync_result != 0 {
        uprintln!("[error] msync failed: {}", msync_result);
        return false;
    }

    if syscall::read_at(MAP_PATH, 0, &mut verify) != MMAP_PAGE_SIZE as isize {
        uprintln!("[error] readback after msync failed");
        return false;
    }
    let msync_persisted = verify[VERIFY_POS_A] == 0xa5 && verify[VERIFY_POS_B] == 0x5a;

    unsafe {
        ptr::write_volatile((mapped_addr + VERIFY_POS_C) as *mut u8, 0x11);
        ptr::write_volatile((mapped_addr + VERIFY_POS_D) as *mut u8, 0xee);
    }

    let munmap_start = syscall::time_us();
    let munmap_result = syscall::munmap(mapped_addr, MMAP_PAGE_SIZE);
    let munmap_elapsed = syscall::time_us().saturating_sub(munmap_start);
    if munmap_result != 0 {
        uprintln!("[error] munmap failed: {}", munmap_result);
        return false;
    }

    if syscall::read_at(MAP_PATH, 0, &mut verify) != MMAP_PAGE_SIZE as isize {
        uprintln!("[error] readback after munmap failed");
        return false;
    }
    if syscall::stat(MAP_PATH, &mut stat) != 0 {
        uprintln!("[error] stat failed");
        return false;
    }
    if syscall::mmap_diag(&mut diag) != 0 {
        uprintln!("[error] mmap_diag failed");
        return false;
    }

    let munmap_persisted = verify[VERIFY_POS_C] == 0x11 && verify[VERIFY_POS_D] == 0xee;
    let diag_match = diag.mmap_calls == 1
        && diag.page_faults == 1
        && diag.pages_loaded == 1
        && diag.msync_writebacks == 1
        && diag.munmap_writebacks == 1
        && diag.dirty_detections == 2
        && diag.last_loaded_bytes == MMAP_PAGE_SIZE as u64
        && diag.last_writeback_bytes == MMAP_PAGE_SIZE as u64
        && diag.mapping_addr == mapped_addr as u64
        && diag.mapping_length == MMAP_PAGE_SIZE as u64;

    uprintln!(
        "[mmap] returned_addr={:#x} fault_elapsed_us={} initial_match={}",
        mapped_addr,
        fault_elapsed,
        if initial_fault_match { "yes" } else { "no" }
    );
    uprintln!(
        "[flush] msync_elapsed_us={} munmap_elapsed_us={} msync_persisted={} munmap_persisted={}",
        msync_elapsed,
        munmap_elapsed,
        if msync_persisted { "yes" } else { "no" },
        if munmap_persisted { "yes" } else { "no" }
    );
    uprintln!(
        "[diag] mmap_calls={} page_faults={} pages_loaded={} msync_writebacks={} munmap_writebacks={} dirty_detections={} last_fault_addr={:#x} last_loaded_bytes={} last_writeback_bytes={}",
        diag.mmap_calls,
        diag.page_faults,
        diag.pages_loaded,
        diag.msync_writebacks,
        diag.munmap_writebacks,
        diag.dirty_detections,
        diag.last_fault_addr,
        diag.last_loaded_bytes,
        diag.last_writeback_bytes
    );
    uprintln!(
        "[file-stat] inode={} size_bytes={} device_id={:#x} highest_level={}",
        stat.inode_number,
        stat.size_bytes,
        stat.device_id,
        syscall::mapping_level_name(stat.highest_level)
    );
    uprintln!(
        "[timing] setup_us={} fault_us={} msync_us={} munmap_us={}",
        setup_elapsed,
        fault_elapsed,
        msync_elapsed,
        munmap_elapsed
    );

    let cleanup_start = syscall::time_us();
    let remove_result = syscall::remove(MAP_PATH);
    let cleanup_elapsed = syscall::time_us().saturating_sub(cleanup_start);
    if remove_result != 0 {
        uprintln!("[error] cleanup remove failed: {}", remove_result);
        return false;
    }

    let pass = mapped_addr == mmap_window_start()
        && initial_fault_match
        && msync_persisted
        && munmap_persisted
        && stat.kind == FS_KIND_FILE
        && stat.size_bytes == MMAP_PAGE_SIZE as u64
        && stat.device_id == FS_DEVICE_ID
        && diag_match;

    uprintln!(
        "[acceptance] mmap triggers a file-backed page fault on first access: {}",
        if mapped_addr == mmap_window_start() && diag.page_faults == 1 && diag.pages_loaded == 1 {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!(
        "[acceptance] file data is loaded into the mapped physical page content: {}",
        if initial_fault_match && diag.last_loaded_bytes == MMAP_PAGE_SIZE as u64 {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!(
        "[acceptance] dirty page changes are written back on msync or munmap: {}",
        if msync_persisted && munmap_persisted && diag.msync_writebacks == 1 && diag.munmap_writebacks == 1 {
            "PASS"
        } else {
            "FAIL"
        }
    );
    uprintln!("[timing] cleanup_us={}", cleanup_elapsed);
    uprintln!(
        "[done] kernel mmap validation {}",
        if pass { "completed successfully" } else { "failed" }
    );
    pass
}

fn fill_pattern(buffer: &mut [u8]) {
    let mut i = 0usize;
    while i < buffer.len() {
        buffer[i] = (((i as u32 * 37) ^ 0x5a) & 0xff) as u8;
        i += 1;
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

fn sys_mmap(path_ptr: *const u8, path_len: usize, length: usize) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => {
            unsafe { MMAP_DIAGNOSTICS.last_error = err as i64; }
            return err;
        }
    };
    if length == 0 || length > MMAP_PAGE_SIZE {
        unsafe { MMAP_DIAGNOSTICS.last_error = EINVAL as i64; }
        return EINVAL;
    }

    let mut stat = FsStat::empty();
    let stat_result = fs::stat(path, &mut stat);
    if stat_result != 0 {
        unsafe { MMAP_DIAGNOSTICS.last_error = stat_result as i64; }
        return stat_result;
    }
    if stat.kind != FS_KIND_FILE {
        unsafe { MMAP_DIAGNOSTICS.last_error = EINVAL as i64; }
        return EINVAL;
    }

    unsafe {
        if MAPPING_STATE.active {
            MMAP_DIAGNOSTICS.last_error = EBUSY as i64;
            return EBUSY;
        }

        MAPPING_STATE = MappingState::empty();
        MAPPING_STATE.active = true;
        MAPPING_STATE.loaded = false;
        MAPPING_STATE.writable = true;
        MAPPING_STATE.path_len = path.len();
        MAPPING_STATE.file_size = length;
        MAPPING_STATE.path[..path.len()].copy_from_slice(path);
        MMAP_DIAGNOSTICS.mmap_calls += 1;
        MMAP_DIAGNOSTICS.mapping_addr = mmap_window_start() as u64;
        MMAP_DIAGNOSTICS.mapping_length = length as u64;
        MMAP_DIAGNOSTICS.last_error = 0;
        zero_mmap_buffers();
    }
    configure_pmp(false);
    mmap_window_start() as isize
}

fn sys_msync(addr: usize, length: usize) -> isize {
    if !mapping_request_valid(addr, length) {
        unsafe { MMAP_DIAGNOSTICS.last_error = EINVAL as i64; }
        return EINVAL;
    }
    flush_mapping(true)
}

fn sys_munmap(addr: usize, length: usize) -> isize {
    if !mapping_request_valid(addr, length) {
        unsafe { MMAP_DIAGNOSTICS.last_error = EINVAL as i64; }
        return EINVAL;
    }

    let result = flush_mapping(false);
    if result != 0 {
        return result;
    }

    unsafe {
        MAPPING_STATE = MappingState::empty();
        zero_mmap_buffers();
    }
    configure_pmp(false);
    0
}

fn sys_mmap_diag(ptr: *mut MmapDiagnostics) -> isize {
    let diag = match validated_user_mut::<MmapDiagnostics>(ptr) {
        Ok(diag) => diag,
        Err(err) => return err,
    };
    unsafe {
        *diag = MMAP_DIAGNOSTICS;
    }
    0
}

fn load_mapped_page() -> isize {
    let (path_len, file_size) = unsafe {
        if !MAPPING_STATE.active {
            return EINVAL;
        }
        if MAPPING_STATE.loaded {
            return MAPPING_STATE.file_size as isize;
        }
        (MAPPING_STATE.path_len, MAPPING_STATE.file_size)
    };

    let path = unsafe { &MAPPING_STATE.path[..path_len] };
    let bytes = unsafe { &mut MMAP_PAGE[..file_size] };
    let read_result = fs::read_at(path, 0, bytes);
    if read_result < 0 {
        return read_result;
    }
    if read_result as usize != file_size {
        return EFAULT;
    }

    unsafe {
        MMAP_SHADOW[..file_size].copy_from_slice(&MMAP_PAGE[..file_size]);
        MAPPING_STATE.loaded = true;
        MMAP_DIAGNOSTICS.pages_loaded += 1;
        MMAP_DIAGNOSTICS.last_loaded_bytes = file_size as u64;
        MMAP_DIAGNOSTICS.last_error = 0;
    }

    file_size as isize
}

fn flush_mapping(is_msync: bool) -> isize {
    let (path_len, file_size, loaded) = unsafe {
        if !MAPPING_STATE.active {
            MMAP_DIAGNOSTICS.last_error = EINVAL as i64;
            return EINVAL;
        }
        (
            MAPPING_STATE.path_len,
            MAPPING_STATE.file_size,
            MAPPING_STATE.loaded,
        )
    };

    if !loaded {
        unsafe {
            MMAP_DIAGNOSTICS.last_writeback_bytes = 0;
            MMAP_DIAGNOSTICS.last_error = 0;
        }
        return 0;
    }

    let dirty = unsafe { MMAP_PAGE[..file_size] != MMAP_SHADOW[..file_size] };
    if !dirty {
        unsafe {
            MMAP_DIAGNOSTICS.last_writeback_bytes = 0;
            MMAP_DIAGNOSTICS.last_error = 0;
        }
        return 0;
    }

    let path = unsafe { &MAPPING_STATE.path[..path_len] };
    let bytes = unsafe { &MMAP_PAGE[..file_size] };
    let write_result = fs::write_at(path, 0, bytes);
    if write_result != file_size as isize {
        unsafe { MMAP_DIAGNOSTICS.last_error = write_result as i64; }
        return if write_result >= 0 { EFAULT } else { write_result };
    }

    unsafe {
        MMAP_SHADOW[..file_size].copy_from_slice(&MMAP_PAGE[..file_size]);
        MMAP_DIAGNOSTICS.dirty_detections += 1;
        MMAP_DIAGNOSTICS.last_writeback_bytes = file_size as u64;
        MMAP_DIAGNOSTICS.last_error = 0;
        if is_msync {
            MMAP_DIAGNOSTICS.msync_writebacks += 1;
        } else {
            MMAP_DIAGNOSTICS.munmap_writebacks += 1;
        }
    }
    0
}

fn mapping_request_valid(addr: usize, length: usize) -> bool {
    addr == mmap_window_start() && length == MMAP_PAGE_SIZE
}

fn mmap_window_start() -> usize {
    ptr::addr_of!(MMAP_PAGE) as usize
}

unsafe fn zero_mmap_buffers() {
    ptr::write_bytes(ptr::addr_of_mut!(MMAP_PAGE) as *mut u8, 0, MMAP_PAGE_SIZE);
    ptr::write_bytes(ptr::addr_of_mut!(MMAP_SHADOW) as *mut u8, 0, MMAP_PAGE_SIZE);
}

fn configure_pmp(allow_mmap: bool) {
    let mmap_start = mmap_window_start();
    let mmap_end = mmap_start + MMAP_PAGE_SIZE;
    let entry0: usize = 0x0f;
    let entry1: usize = if allow_mmap { 0x0b } else { 0x08 };
    let entry2: usize = 0x0f;
    let cfg = entry0 | (entry1 << 8) | (entry2 << 16);

    unsafe {
        asm!("csrw pmpaddr0, {}", in(reg) (mmap_start >> 2), options(nostack, nomem));
        asm!("csrw pmpaddr1, {}", in(reg) (mmap_end >> 2), options(nostack, nomem));
        asm!("csrw pmpaddr2, {}", in(reg) (usize::MAX >> 2), options(nostack, nomem));
        asm!("csrw pmpcfg0, {}", in(reg) cfg, options(nostack, nomem));
    }
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

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
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
