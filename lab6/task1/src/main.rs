#![no_std]
#![no_main]

pub const FS_MAX_INODES: usize = 32;
pub const FS_MAX_DATA_BLOCKS: usize = 34_000;
pub const FS_MAX_PTR_BLOCKS: usize = 512;

mod abi;
mod console;
mod fs;
mod syscall;
mod trap;
mod user_console;

use abi::{
    FsStat, EFAULT, EINVAL, ENAMETOOLONG, ENOSYS, FS_BLOCK_SIZE, FS_DIRECT_POINTERS,
    FS_DOUBLE_LIMIT_BYTES, FS_KIND_FILE, FS_LEVEL_TRIPLE, FS_POINTERS_PER_BLOCK, FS_PATH_MAX,
    FS_SINGLE_LIMIT_BYTES, FS_TRIPLE_LIMIT_BYTES, SYS_CREATE_DIR, SYS_CREATE_FILE, SYS_LIST_DIR,
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

const TARGET_PATH: &str = "/triple_data.bin";
const TARGET_BYTES: u64 = 16 * 1024 * 1024;
const CHUNK_BYTES: usize = 4096;
const MAX_MISMATCH_LOGS: usize = 4;

const PATTERN_SEED: u64 = 0x6c61_6236_5f74_6173;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

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
    println!("[kernel] LAB6 task1 guest large-file stress test");
    println!(
        "[kernel] fs model: block_size={} direct={} pointers_per_indirect={} triple_limit_bytes={}",
        FS_BLOCK_SIZE,
        FS_DIRECT_POINTERS,
        FS_POINTERS_PER_BLOCK,
        FS_TRIPLE_LIMIT_BYTES
    );
    println!(
        "[kernel] memory pools: max_inodes={} max_data_blocks={} max_ptr_blocks={}",
        FS_MAX_INODES,
        FS_MAX_DATA_BLOCKS,
        FS_MAX_PTR_BLOCKS
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
    let pass = run_large_file_test();
    syscall::shutdown(if pass { 0 } else { 1 });
}

fn run_large_file_test() -> bool {
    let mut write_buffer = [0u8; CHUNK_BYTES];
    let mut read_buffer = [0u8; CHUNK_BYTES];
    let mut stat = FsStat::empty();
    let mut write_checksum = FNV_OFFSET_BASIS;
    let mut read_checksum = FNV_OFFSET_BASIS;
    let mut mismatches = 0usize;
    let mut mismatch_logs = 0usize;
    let over_double_bytes = TARGET_BYTES.saturating_sub(FS_DOUBLE_LIMIT_BYTES);
    let over_double_ratio_milli = TARGET_BYTES * 1000 / FS_DOUBLE_LIMIT_BYTES;

    uprintln!("[user] large-file test started in U-mode");
    uprintln!(
        "[inode-model] block_size={} direct={} pointers_per_indirect={}",
        FS_BLOCK_SIZE,
        FS_DIRECT_POINTERS,
        FS_POINTERS_PER_BLOCK
    );
    uprintln!(
        "[inode-model] single_limit_bytes={} double_limit_bytes={} triple_limit_bytes={}",
        FS_SINGLE_LIMIT_BYTES,
        FS_DOUBLE_LIMIT_BYTES,
        FS_TRIPLE_LIMIT_BYTES
    );
    uprintln!(
        "[config] path={} target_bytes={} chunk_bytes={} over_double_bytes={} over_double_ratio={}.{:03}x",
        TARGET_PATH,
        TARGET_BYTES,
        CHUNK_BYTES,
        over_double_bytes,
        over_double_ratio_milli / 1000,
        over_double_ratio_milli % 1000
    );

    let create_result = syscall::create_file(TARGET_PATH);
    if create_result != 0 {
        uprintln!(
            "[error] create_file failed: {} ({})",
            syscall::describe_error(create_result),
            create_result
        );
        return false;
    }

    let write_start = syscall::time_us();
    let mut offset = 0u64;
    while offset < TARGET_BYTES {
        let chunk_len = core::cmp::min(CHUNK_BYTES as u64, TARGET_BYTES - offset) as usize;
        fill_pattern(&mut write_buffer[..chunk_len], offset);
        let result = syscall::write_at(TARGET_PATH, offset as usize, &write_buffer[..chunk_len]);
        if result != chunk_len as isize {
            uprintln!(
                "[error] write_at offset={} failed: {} ({})",
                offset,
                syscall::describe_error(result),
                result
            );
            return false;
        }
        write_checksum = fnv1a_update(write_checksum, &write_buffer[..chunk_len]);
        offset += chunk_len as u64;
    }
    let write_elapsed = syscall::time_us().saturating_sub(write_start);

    let stat_result = syscall::stat(TARGET_PATH, &mut stat);
    if stat_result != 0 {
        uprintln!(
            "[error] stat failed: {} ({})",
            syscall::describe_error(stat_result),
            stat_result
        );
        return false;
    }

    let read_start = syscall::time_us();
    offset = 0;
    while offset < TARGET_BYTES {
        let chunk_len = core::cmp::min(CHUNK_BYTES as u64, TARGET_BYTES - offset) as usize;
        let result = syscall::read_at(TARGET_PATH, offset as usize, &mut read_buffer[..chunk_len]);
        if result != chunk_len as isize {
            uprintln!(
                "[error] read_at offset={} failed: {} ({})",
                offset,
                syscall::describe_error(result),
                result
            );
            return false;
        }
        fill_pattern(&mut write_buffer[..chunk_len], offset);
        for index in 0..chunk_len {
            if read_buffer[index] != write_buffer[index] {
                mismatches += 1;
                if mismatch_logs < MAX_MISMATCH_LOGS {
                    uprintln!(
                        "[mismatch] offset={} expected=0x{:02x} actual=0x{:02x}",
                        offset + index as u64,
                        write_buffer[index],
                        read_buffer[index]
                    );
                    mismatch_logs += 1;
                }
            }
        }
        read_checksum = fnv1a_update(read_checksum, &read_buffer[..chunk_len]);
        offset += chunk_len as u64;
    }
    let read_elapsed = syscall::time_us().saturating_sub(read_start);

    let write_kib_per_s = kib_per_second(TARGET_BYTES, write_elapsed);
    let read_kib_per_s = kib_per_second(TARGET_BYTES, read_elapsed);
    let size_pass = stat.kind == FS_KIND_FILE && stat.size_bytes == TARGET_BYTES;
    let triple_needed = stat.highest_level == FS_LEVEL_TRIPLE;
    let integrity_pass = mismatches == 0 && write_checksum == read_checksum;
    let over_limit_pass = TARGET_BYTES > FS_DOUBLE_LIMIT_BYTES;

    uprintln!(
        "[write] bytes={} duration_us={} kib_per_s={} checksum={:#018x}",
        TARGET_BYTES,
        write_elapsed,
        write_kib_per_s,
        write_checksum
    );
    uprintln!(
        "[fs-stat] size_bytes={} blocks_used={} highest_level={}",
        stat.size_bytes,
        stat.blocks_used,
        syscall::mapping_level_name(stat.highest_level)
    );
    uprintln!(
        "[readback] bytes={} duration_us={} kib_per_s={} checksum={:#018x} mismatches={}",
        TARGET_BYTES,
        read_elapsed,
        read_kib_per_s,
        read_checksum,
        mismatches
    );
    uprintln!(
        "[acceptance] file size exceeds single and double indirect limits: {}",
        if size_pass && over_limit_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] file data readback matches written pattern: {}",
        if integrity_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] triple-indirect mapping was actually used: {}",
        if triple_needed { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] triple-indirect theoretical capacity reported: {}",
        if FS_TRIPLE_LIMIT_BYTES > FS_DOUBLE_LIMIT_BYTES {
            "PASS"
        } else {
            "FAIL"
        }
    );

    let pass = size_pass && over_limit_pass && integrity_pass && triple_needed;
    uprintln!(
        "[done] guest large-file stress {}",
        if pass { "completed successfully" } else { "failed" }
    );
    pass
}

fn kib_per_second(bytes: u64, duration_us: u64) -> u64 {
    if duration_us == 0 {
        return 0;
    }

    (bytes * 1_000_000 / 1024) / duration_us
}

fn splitmix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn fill_pattern(buffer: &mut [u8], base_offset: u64) {
    let mut index = 0usize;
    while index < buffer.len() {
        let word_index = (base_offset + index as u64) / size_of::<u64>() as u64;
        let value = splitmix64(PATTERN_SEED ^ word_index).to_le_bytes();
        let copy_len = core::cmp::min(value.len(), buffer.len() - index);
        buffer[index..index + copy_len].copy_from_slice(&value[..copy_len]);
        index += copy_len;
    }
}

fn fnv1a_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
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
