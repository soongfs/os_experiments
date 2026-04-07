#![no_std]
#![no_main]

mod abi;
mod console;
mod hostio;
mod journal;
mod syscall;
mod trap;
mod user_console;

use abi::{
    FsStat, JournalDiagnostics, BOOT_MODE_RECOVER, BOOT_MODE_RESET_AND_CRASH, EFAULT, EINVAL,
    ENAMETOOLONG, ENOSYS, FILE_PAYLOAD_BYTES, PATH_MAX, SYS_BOOT_MODE,
    SYS_JOURNAL_DIAG, SYS_READ_FILE, SYS_SHUTDOWN, SYS_STAT, SYS_TIME_US, SYS_TXN_WRITE,
    SYS_WRITE,
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
    journal::init();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB6 kernel task5 journaling and crash consistency");
    println!(
        "[kernel] semihosting paths: disk={} mode={}",
        hostio::disk_path(),
        hostio::mode_path()
    );
    println!(
        "[kernel] file payload capacity={} bytes",
        hostio::file_payload_bytes()
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
        SYS_BOOT_MODE => journal::boot_mode() as isize,
        SYS_TXN_WRITE => sys_txn_write(
            frame.a0 as *const u8,
            frame.a1,
            frame.a2 as *const u8,
            frame.a3,
        ),
        SYS_READ_FILE => sys_read_file(
            frame.a0 as *const u8,
            frame.a1,
            frame.a2 as *mut u8,
            frame.a3,
        ),
        SYS_STAT => sys_stat(frame.a0 as *const u8, frame.a1, frame.a2 as *mut FsStat),
        SYS_JOURNAL_DIAG => sys_journal_diag(frame.a0 as *mut JournalDiagnostics),
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
    let pass = run_journal_test();
    syscall::shutdown(if pass { 0 } else { 1 });
}

fn run_journal_test() -> bool {
    let mode = syscall::boot_mode();
    let mut diag = JournalDiagnostics::empty();
    let mut stat = FsStat::empty();
    let mut buffer = [0u8; FILE_PAYLOAD_BYTES];

    uprintln!(
        "[config] boot_mode={} journal_file={} payload_capacity={}",
        boot_mode_name(mode),
        journal::file_path(),
        FILE_PAYLOAD_BYTES
    );

    if mode == BOOT_MODE_RESET_AND_CRASH {
        let start = syscall::time_us();
        let result = syscall::txn_write(journal::file_path(), journal::expected_updated_bytes());
        let elapsed = syscall::time_us().saturating_sub(start);
        uprintln!(
            "[crash-run] txn_write_result={} elapsed_us={} expected_exit=abrupt_after_commit",
            result,
            elapsed
        );
        return false;
    }

    let verify_start = syscall::time_us();
    let read_result = syscall::read_file(journal::file_path(), &mut buffer);
    let stat_result = syscall::stat(journal::file_path(), &mut stat);
    let root_result = syscall::stat("/", &mut FsStat::empty());
    let diag_result = syscall::journal_diag(&mut diag);
    let verify_elapsed = syscall::time_us().saturating_sub(verify_start);

    if read_result < 0 || stat_result != 0 || root_result != 0 || diag_result != 0 {
        uprintln!(
            "[error] verify syscalls failed read={} stat={} root={} diag={}",
            read_result,
            stat_result,
            root_result,
            diag_result
        );
        return false;
    }

    let expected = if mode == BOOT_MODE_RECOVER {
        journal::expected_updated_bytes()
    } else {
        journal::expected_updated_bytes()
    };
    let actual = &buffer[..read_result as usize];
    let data_match = actual == expected;
    let root_stat_pass = {
        let mut root = FsStat::empty();
        syscall::stat("/", &mut root) == 0 && root.child_count == 1
    };

    uprintln!(
        "[verify] bytes={} data_match={} checksum={:#018x} journal_active={} journal_committed={}",
        read_result,
        if data_match { "yes" } else { "no" },
        stat.checksum,
        stat.journal_active,
        stat.journal_committed
    );
    uprintln!(
        "[diag] tx_begins={} tx_commits={} log_writes={} commit_writes={} home_writes={} recovery_replays={} crash_injections={} fsck_passes={} committed_logs_seen={} last_tx_seq={} last_home_checksum={:#018x} last_error={}",
        diag.tx_begins,
        diag.tx_commits,
        diag.log_writes,
        diag.commit_writes,
        diag.home_writes,
        diag.recovery_replays,
        diag.crash_injections,
        diag.fsck_passes,
        diag.committed_logs_seen,
        diag.last_tx_seq,
        diag.last_home_checksum,
        diag.last_error
    );
    uprintln!("[timing] verify_us={}", verify_elapsed);

    let tx_pass = if mode == BOOT_MODE_RECOVER {
        diag.committed_logs_seen >= 1 && diag.last_tx_seq >= 1
    } else {
        diag.tx_begins >= 1 && diag.tx_commits >= 1
    };
    let order_pass = if mode == BOOT_MODE_RECOVER {
        diag.committed_logs_seen >= 1 && diag.home_writes >= 1
    } else {
        diag.log_writes >= 1 && diag.commit_writes >= 1 && diag.home_writes >= 1
    };
    let replay_pass = if mode == BOOT_MODE_RECOVER {
        diag.recovery_replays >= 1 && diag.committed_logs_seen >= 1
    } else {
        true
    };
    let fsck_pass = diag.fsck_passes >= 1 && root_stat_pass && stat.journal_active == 0 && stat.journal_committed == 0;

    uprintln!(
        "[acceptance] all file modifications are wrapped in transactions: {}",
        if tx_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] journal blocks commit before home data installation: {}",
        if order_pass { "PASS" } else { "FAIL" }
    );
    uprintln!(
        "[acceptance] reboot recovery replays committed logs and repairs filesystem state: {}",
        if data_match && replay_pass && fsck_pass { "PASS" } else { "FAIL" }
    );

    data_match && tx_pass && order_pass && replay_pass && fsck_pass
}

fn boot_mode_name(mode: u64) -> &'static str {
    match mode {
        BOOT_MODE_RESET_AND_CRASH => "reset_then_crash_after_commit",
        BOOT_MODE_RECOVER => "recover_after_crash",
        _ => "verify",
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

fn sys_txn_write(
    path_ptr: *const u8,
    path_len: usize,
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
    let result = journal::transactional_write(path, bytes);
    if result == 1 {
        println!("[kernel] forced crash injection after committed journal");
        qemu_exit(2);
    }
    result
}

fn sys_read_file(path_ptr: *const u8, path_len: usize, buf_ptr: *mut u8, buf_len: usize) -> isize {
    let path = match validated_user_path(path_ptr, path_len) {
        Ok(path) => path,
        Err(err) => return err,
    };
    let bytes = match validated_user_mut_bytes(buf_ptr, buf_len) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    journal::read_file(path, bytes)
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
    journal::stat(path, stat)
}

fn sys_journal_diag(ptr: *mut JournalDiagnostics) -> isize {
    let diag = match validated_user_mut::<JournalDiagnostics>(ptr) {
        Ok(diag) => diag,
        Err(err) => return err,
    };
    *diag = journal::journal_diagnostics();
    0
}

fn validated_user_path<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Err(EINVAL);
    }
    if len >= PATH_MAX {
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
