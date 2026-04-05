#![no_std]
#![no_main]

mod apps;
mod console;
mod syscall;
mod trap;

use core::arch::{asm, global_asm};
use core::mem::{align_of, size_of};
use core::panic::PanicInfo;
use core::ptr;

global_asm!(include_str!("boot.S"));

const DRAM_START: usize = 0x8000_0000;
const TASK_NAME_LEN: usize = 32;
const TASK_COUNT: usize = 4;

pub const IO_BURST_WRITES: u64 = 24;
pub const INFO_FLOOD_CALLS: u64 = 20;

pub const SYS_WRITE: usize = 0;
pub const SYS_GET_TASKINFO: usize = 1;
pub const SYS_EXIT: usize = 2;

pub const EFAULT: isize = -14;
pub const EINVAL: isize = -22;
pub const ENOSYS: isize = -38;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub task_id: u64,
    pub task_name: [u8; TASK_NAME_LEN],
    pub total_syscalls: u64,
    pub write_calls: u64,
    pub get_taskinfo_calls: u64,
    pub error_syscalls: u64,
    pub elapsed_cycles: u64,
}

impl TaskInfo {
    pub const fn empty() -> Self {
        Self {
            task_id: 0,
            task_name: [0; TASK_NAME_LEN],
            total_syscalls: 0,
            write_calls: 0,
            get_taskinfo_calls: 0,
            error_syscalls: 0,
            elapsed_cycles: 0,
        }
    }

    pub fn name(&self) -> &str {
        bytes_to_str(&self.task_name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskStatus {
    Pending,
    Exited,
    Faulted,
}

#[derive(Clone, Copy)]
struct TaskStats {
    total_syscalls: u64,
    write_calls: u64,
    get_taskinfo_calls: u64,
    error_syscalls: u64,
    start_cycle: u64,
    end_cycle: u64,
    exit_code: i32,
    fault_cause: u64,
    fault_tval: u64,
    status: TaskStatus,
}

impl TaskStats {
    const fn empty() -> Self {
        Self {
            total_syscalls: 0,
            write_calls: 0,
            get_taskinfo_calls: 0,
            error_syscalls: 0,
            start_cycle: 0,
            end_cycle: 0,
            exit_code: 0,
            fault_cause: 0,
            fault_tval: 0,
            status: TaskStatus::Pending,
        }
    }

    fn elapsed_cycles(self) -> u64 {
        self.end_cycle.wrapping_sub(self.start_cycle)
    }
}

#[derive(Clone, Copy)]
struct TaskConfig {
    id: u64,
    name: &'static str,
    task_name: [u8; TASK_NAME_LEN],
    entry: extern "C" fn() -> !,
    expected_profile: &'static str,
}

const TASKS: [TaskConfig; TASK_COUNT] = [
    TaskConfig {
        id: 1,
        name: "io_burst",
        task_name: padded_name(b"io_burst"),
        entry: apps::io_burst::io_burst,
        expected_profile: "write syscall count should dominate; elapsed cycles should stay modest",
    },
    TaskConfig {
        id: 2,
        name: "compute_spin",
        task_name: padded_name(b"compute_spin"),
        entry: apps::compute_spin::compute_spin,
        expected_profile: "almost no syscalls, but elapsed cycles should be the highest",
    },
    TaskConfig {
        id: 3,
        name: "info_flood",
        task_name: padded_name(b"info_flood"),
        entry: apps::info_flood::info_flood,
        expected_profile:
            "get_taskinfo calls should dominate and returned snapshots should grow monotonically",
    },
    TaskConfig {
        id: 4,
        name: "illegal_trap",
        task_name: padded_name(b"illegal_trap"),
        entry: apps::illegal_trap::illegal_trap,
        expected_profile: "should trigger an illegal-instruction trap and be recorded as faulted",
    },
];

static mut TASK_STATS: [TaskStats; TASK_COUNT] = [TaskStats::empty(); TASK_COUNT];
static mut CURRENT_TASK_INDEX: usize = 0;

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

    println!("[kernel] booted in M-mode");
    println!("[kernel] starting LAB2 task2 statistics suite");

    launch_task(0)
}

pub fn handle_syscall(frame: &mut trap::TrapFrame) {
    let result = match frame.a7 {
        SYS_WRITE => {
            note_syscall(SyscallClass::Write);
            sys_write(frame.a0 as *const u8, frame.a1)
        }
        SYS_GET_TASKINFO => {
            note_syscall(SyscallClass::GetTaskInfo);
            sys_get_taskinfo(frame.a0 as *mut TaskInfo)
        }
        SYS_EXIT => {
            note_syscall(SyscallClass::Other);
            finish_current_task(frame.a0 as i32)
        }
        _ => {
            note_syscall(SyscallClass::Other);
            ENOSYS
        }
    };

    if result < 0 {
        note_syscall_error();
    }

    frame.a0 = result as usize;
}

pub fn handle_user_fault(mcause: usize, mepc: usize, mtval: usize) -> ! {
    let task = current_task();
    println!(
        "[kernel] task {} faulted: mcause={:#x} mepc={:#x} mtval={:#x}",
        task.name, mcause, mepc, mtval
    );

    unsafe {
        let stats = &mut TASK_STATS[CURRENT_TASK_INDEX];
        stats.end_cycle = read_cycle();
        stats.fault_cause = mcause as u64;
        stats.fault_tval = mtval as u64;
        stats.status = TaskStatus::Faulted;
    }

    advance_to_next_task()
}

fn launch_task(index: usize) -> ! {
    unsafe {
        CURRENT_TASK_INDEX = index;
        TASK_STATS[index] = TaskStats::empty();
        TASK_STATS[index].start_cycle = read_cycle();
    }

    let task = task_at(index);
    println!(
        "[kernel] launch task id={} name={} | expected: {}",
        task.id, task.name, task.expected_profile
    );

    unsafe {
        enter_user_mode(
            task.entry as *const () as usize,
            ptr::addr_of!(__user_stack_top) as usize,
            ptr::addr_of!(__kernel_stack_top) as usize,
        )
    }
}

fn finish_current_task(code: i32) -> ! {
    unsafe {
        let stats = &mut TASK_STATS[CURRENT_TASK_INDEX];
        stats.end_cycle = read_cycle();
        stats.exit_code = code;
        stats.status = TaskStatus::Exited;
    }

    advance_to_next_task()
}

fn advance_to_next_task() -> ! {
    let finished_index = unsafe { CURRENT_TASK_INDEX };
    print_task_result(finished_index);

    let next_index = finished_index + 1;
    if next_index < TASK_COUNT {
        launch_task(next_index)
    } else {
        print_final_report()
    }
}

fn print_task_result(index: usize) {
    let task = task_at(index);
    let stats = unsafe { TASK_STATS[index] };

    match stats.status {
        TaskStatus::Exited => println!(
            "[kernel] result {}: status=exit({}) cycles={} total={} write={} get_taskinfo={} error={}",
            task.name,
            stats.exit_code,
            stats.elapsed_cycles(),
            stats.total_syscalls,
            stats.write_calls,
            stats.get_taskinfo_calls,
            stats.error_syscalls
        ),
        TaskStatus::Faulted => println!(
            "[kernel] result {}: status=fault(cause={:#x}, mtval={:#x}) cycles={} total={} write={} get_taskinfo={} error={}",
            task.name,
            stats.fault_cause,
            stats.fault_tval,
            stats.elapsed_cycles(),
            stats.total_syscalls,
            stats.write_calls,
            stats.get_taskinfo_calls,
            stats.error_syscalls
        ),
        TaskStatus::Pending => println!(
            "[kernel] result {}: status=pending cycles={} total={} write={} get_taskinfo={} error={}",
            task.name,
            stats.elapsed_cycles(),
            stats.total_syscalls,
            stats.write_calls,
            stats.get_taskinfo_calls,
            stats.error_syscalls
        ),
    }
}

fn print_final_report() -> ! {
    let io = unsafe { TASK_STATS[0] };
    let compute = unsafe { TASK_STATS[1] };
    let info = unsafe { TASK_STATS[2] };
    let illegal = unsafe { TASK_STATS[3] };

    println!("[kernel] final statistics report:");
    for index in 0..TASK_COUNT {
        print_task_result(index);
    }

    let io_pass = io.status == TaskStatus::Exited
        && io.exit_code == 0
        && io.write_calls == IO_BURST_WRITES
        && io.get_taskinfo_calls == 0
        && io.error_syscalls == 0;
    let compute_pass = compute.status == TaskStatus::Exited
        && compute.exit_code == 0
        && compute.total_syscalls == 1
        && compute.write_calls == 0
        && compute.get_taskinfo_calls == 0;
    let info_pass = info.status == TaskStatus::Exited
        && info.exit_code == 0
        && info.get_taskinfo_calls == INFO_FLOOD_CALLS
        && info.write_calls == 0
        && info.error_syscalls == 0;
    let illegal_pass = illegal.status == TaskStatus::Faulted
        && illegal.fault_cause == 2
        && illegal.total_syscalls == 0
        && illegal.write_calls == 0
        && illegal.get_taskinfo_calls == 0;
    let trend_pass = io.write_calls > compute.write_calls
        && io.write_calls > info.write_calls
        && info.get_taskinfo_calls > io.get_taskinfo_calls
        && info.get_taskinfo_calls > compute.get_taskinfo_calls
        && compute.elapsed_cycles() > io.elapsed_cycles()
        && compute.elapsed_cycles() > info.elapsed_cycles();

    println!("[kernel] check io_burst writes: {}", pass_fail(io_pass));
    println!(
        "[kernel] check compute_spin low-syscall/high-time trend: {}",
        pass_fail(compute_pass && compute.elapsed_cycles() > 0)
    );
    println!(
        "[kernel] check info_flood get_taskinfo trend: {}",
        pass_fail(info_pass)
    );
    println!(
        "[kernel] check illegal_trap robustness path: {}",
        pass_fail(illegal_pass)
    );
    println!(
        "[kernel] cross-task trend comparison: {}",
        pass_fail(trend_pass)
    );

    if io_pass && compute_pass && info_pass && illegal_pass && trend_pass {
        qemu_exit(0)
    } else {
        qemu_exit(1)
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

fn sys_get_taskinfo(ptr: *mut TaskInfo) -> isize {
    let task_info = match validated_user_mut::<TaskInfo>(ptr) {
        Ok(task_info) => task_info,
        Err(err) => return err,
    };

    let task = current_task();
    let stats = current_stats();
    *task_info = TaskInfo {
        task_id: task.id,
        task_name: task.task_name,
        total_syscalls: stats.total_syscalls,
        write_calls: stats.write_calls,
        get_taskinfo_calls: stats.get_taskinfo_calls,
        error_syscalls: stats.error_syscalls,
        elapsed_cycles: read_cycle().wrapping_sub(stats.start_cycle),
    };

    0
}

fn note_syscall(class: SyscallClass) {
    unsafe {
        let stats = &mut TASK_STATS[CURRENT_TASK_INDEX];
        stats.total_syscalls += 1;
        match class {
            SyscallClass::Write => stats.write_calls += 1,
            SyscallClass::GetTaskInfo => stats.get_taskinfo_calls += 1,
            SyscallClass::Other => {}
        }
    }
}

fn note_syscall_error() {
    unsafe {
        TASK_STATS[CURRENT_TASK_INDEX].error_syscalls += 1;
    }
}

fn current_task() -> TaskConfig {
    unsafe { TASKS[CURRENT_TASK_INDEX] }
}

fn task_at(index: usize) -> TaskConfig {
    TASKS[index]
}

fn current_stats() -> TaskStats {
    unsafe { TASK_STATS[CURRENT_TASK_INDEX] }
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

fn read_cycle() -> u64 {
    let value: u64;

    unsafe {
        asm!("rdcycle {}", out(reg) value, options(nostack, nomem));
    }

    value
}

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
    }
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

const fn padded_name(name: &[u8]) -> [u8; TASK_NAME_LEN] {
    let mut padded = [0; TASK_NAME_LEN];
    let mut index = 0;

    while index < name.len() && index < TASK_NAME_LEN - 1 {
        padded[index] = name[index];
        index += 1;
    }

    padded
}

fn bytes_to_str(bytes: &[u8; TASK_NAME_LEN]) -> &str {
    let mut len = 0;

    while len < bytes.len() && bytes[len] != 0 {
        len += 1;
    }

    core::str::from_utf8(&bytes[..len]).unwrap_or("<invalid>")
}

fn pass_fail(ok: bool) -> &'static str {
    if ok {
        "PASS"
    } else {
        "FAIL"
    }
}

enum SyscallClass {
    Write,
    GetTaskInfo,
    Other,
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1)
}
