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
const SYSCALL_HIST_LEN: usize = 3;

pub const IO_BURST_WRITES: u64 = 24;
pub const INFO_FLOOD_CALLS: u64 = 20;

pub const SYS_WRITE: usize = 0;
pub const SYS_GET_TASKINFO: usize = 1;
pub const SYS_EXIT: usize = 2;

pub const EFAULT: isize = -14;
pub const EINVAL: isize = -22;
pub const ENOSYS: isize = -38;

const SYSCALL_LABELS: [&str; SYSCALL_HIST_LEN] = ["write", "get_taskinfo", "exit"];

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
    Ready,
    Exited,
    Faulted,
}

#[derive(Clone, Copy)]
struct SyscallStats {
    total_syscalls: u64,
    histogram: [u64; SYSCALL_HIST_LEN],
    error_syscalls: u64,
    unknown_syscalls: u64,
}

impl SyscallStats {
    const fn empty() -> Self {
        Self {
            total_syscalls: 0,
            histogram: [0; SYSCALL_HIST_LEN],
            error_syscalls: 0,
            unknown_syscalls: 0,
        }
    }

    fn write_calls(self) -> u64 {
        self.histogram[SYS_WRITE]
    }

    fn get_taskinfo_calls(self) -> u64 {
        self.histogram[SYS_GET_TASKINFO]
    }
}

#[derive(Clone, Copy)]
struct TaskControlBlock {
    id: u64,
    name: &'static str,
    task_name: [u8; TASK_NAME_LEN],
    entry: extern "C" fn() -> !,
    expected_profile: &'static str,
    stats: SyscallStats,
    start_cycle: u64,
    end_cycle: u64,
    exit_code: i32,
    fault_cause: u64,
    fault_tval: u64,
    status: TaskStatus,
}

impl TaskControlBlock {
    const fn new(
        id: u64,
        name: &'static str,
        task_name: [u8; TASK_NAME_LEN],
        entry: extern "C" fn() -> !,
        expected_profile: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            task_name,
            entry,
            expected_profile,
            stats: SyscallStats::empty(),
            start_cycle: 0,
            end_cycle: 0,
            exit_code: 0,
            fault_cause: 0,
            fault_tval: 0,
            status: TaskStatus::Ready,
        }
    }

    fn elapsed_cycles(self) -> u64 {
        self.end_cycle.wrapping_sub(self.start_cycle)
    }
}

static mut TASKS: [TaskControlBlock; TASK_COUNT] = [
    TaskControlBlock::new(
        1,
        "io_burst",
        padded_name(b"io_burst"),
        apps::io_burst::io_burst,
        "write bucket should dominate; exit bucket should be exactly 1",
    ),
    TaskControlBlock::new(
        2,
        "compute_spin",
        padded_name(b"compute_spin"),
        apps::compute_spin::compute_spin,
        "almost only exit syscall; elapsed cycles should be the largest",
    ),
    TaskControlBlock::new(
        3,
        "info_flood",
        padded_name(b"info_flood"),
        apps::info_flood::info_flood,
        "get_taskinfo bucket should dominate; exit bucket should be exactly 1",
    ),
    TaskControlBlock::new(
        4,
        "illegal_trap",
        padded_name(b"illegal_trap"),
        apps::illegal_trap::illegal_trap,
        "should fault before any syscall, keeping every bucket at 0",
    ),
];

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
    println!("[kernel] starting LAB2 kernel task2 syscall histogram suite");
    println!("[kernel] tracked syscall numbers: nr=0(write), nr=1(get_taskinfo), nr=2(exit)");

    launch_task(0)
}

pub fn handle_syscall(frame: &mut trap::TrapFrame) {
    match frame.a7 {
        SYS_WRITE => {
            record_syscall_number(SYS_WRITE);
            let result = sys_write(frame.a0 as *const u8, frame.a1);
            if result < 0 {
                record_syscall_error();
            }
            frame.a0 = result as usize;
        }
        SYS_GET_TASKINFO => {
            record_syscall_number(SYS_GET_TASKINFO);
            let result = sys_get_taskinfo(frame.a0 as *mut TaskInfo);
            if result < 0 {
                record_syscall_error();
            }
            frame.a0 = result as usize;
        }
        SYS_EXIT => {
            record_syscall_number(SYS_EXIT);
            finish_current_task(frame.a0 as i32)
        }
        nr => {
            record_syscall_number(nr);
            record_syscall_error();
            frame.a0 = ENOSYS as usize;
        }
    }
}

pub fn handle_user_fault(mcause: usize, mepc: usize, mtval: usize) -> ! {
    unsafe {
        let task = &mut TASKS[CURRENT_TASK_INDEX];
        task.end_cycle = read_cycle();
        task.fault_cause = mcause as u64;
        task.fault_tval = mtval as u64;
        task.status = TaskStatus::Faulted;
        println!(
            "[kernel] task {} faulted: mcause={:#x} mepc={:#x} mtval={:#x}",
            task.name, mcause, mepc, mtval
        );
    }

    advance_to_next_task()
}

fn launch_task(index: usize) -> ! {
    unsafe {
        CURRENT_TASK_INDEX = index;
        let task = &mut TASKS[index];
        task.stats = SyscallStats::empty();
        task.start_cycle = read_cycle();
        task.end_cycle = 0;
        task.exit_code = 0;
        task.fault_cause = 0;
        task.fault_tval = 0;
        task.status = TaskStatus::Ready;

        println!(
            "[kernel] launch task id={} name={} | expected: {}",
            task.id, task.name, task.expected_profile
        );

        enter_user_mode(
            task.entry as *const () as usize,
            ptr::addr_of!(__user_stack_top) as usize,
            ptr::addr_of!(__kernel_stack_top) as usize,
        )
    }
}

fn finish_current_task(code: i32) -> ! {
    unsafe {
        let task = &mut TASKS[CURRENT_TASK_INDEX];
        task.end_cycle = read_cycle();
        task.exit_code = code;
        task.status = TaskStatus::Exited;
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
    let task = unsafe { TASKS[index] };

    match task.status {
        TaskStatus::Exited => println!(
            "[kernel] result {}: status=exit({}) cycles={} total={} errors={} unknown={}",
            task.name,
            task.exit_code,
            task.elapsed_cycles(),
            task.stats.total_syscalls,
            task.stats.error_syscalls,
            task.stats.unknown_syscalls
        ),
        TaskStatus::Faulted => println!(
            "[kernel] result {}: status=fault(cause={:#x}, mtval={:#x}) cycles={} total={} errors={} unknown={}",
            task.name,
            task.fault_cause,
            task.fault_tval,
            task.elapsed_cycles(),
            task.stats.total_syscalls,
            task.stats.error_syscalls,
            task.stats.unknown_syscalls
        ),
        TaskStatus::Ready => println!(
            "[kernel] result {}: status=ready cycles={} total={} errors={} unknown={}",
            task.name,
            task.elapsed_cycles(),
            task.stats.total_syscalls,
            task.stats.error_syscalls,
            task.stats.unknown_syscalls
        ),
    }

    print_task_histogram(task);
}

fn print_task_histogram(task: TaskControlBlock) {
    println!("[kernel] syscall histogram for {}:", task.name);
    let mut nr = 0;
    while nr < SYSCALL_HIST_LEN {
        println!(
            "[kernel]   nr={} ({}) -> {}",
            nr, SYSCALL_LABELS[nr], task.stats.histogram[nr]
        );
        nr += 1;
    }
    println!("[kernel]   unknown -> {}", task.stats.unknown_syscalls);
}

fn print_final_report() -> ! {
    let io = unsafe { TASKS[0] };
    let compute = unsafe { TASKS[1] };
    let info = unsafe { TASKS[2] };
    let illegal = unsafe { TASKS[3] };

    println!("[kernel] final per-task summary:");
    for index in 0..TASK_COUNT {
        print_task_result(index);
    }

    let io_pass = io.status == TaskStatus::Exited
        && io.exit_code == 0
        && io.stats.histogram[SYS_WRITE] == IO_BURST_WRITES
        && io.stats.histogram[SYS_EXIT] == 1
        && io.stats.histogram[SYS_GET_TASKINFO] == 0;
    let compute_pass = compute.status == TaskStatus::Exited
        && compute.exit_code == 0
        && compute.stats.histogram[SYS_WRITE] == 0
        && compute.stats.histogram[SYS_GET_TASKINFO] == 0
        && compute.stats.histogram[SYS_EXIT] == 1;
    let info_pass = info.status == TaskStatus::Exited
        && info.exit_code == 0
        && info.stats.histogram[SYS_GET_TASKINFO] == INFO_FLOOD_CALLS
        && info.stats.histogram[SYS_WRITE] == 0
        && info.stats.histogram[SYS_EXIT] == 1;
    let illegal_pass = illegal.status == TaskStatus::Faulted
        && illegal.stats.total_syscalls == 0
        && illegal.stats.histogram[SYS_WRITE] == 0
        && illegal.stats.histogram[SYS_GET_TASKINFO] == 0
        && illegal.stats.histogram[SYS_EXIT] == 0;
    let isolation_pass = io.stats.total_syscalls == IO_BURST_WRITES + 1
        && compute.stats.total_syscalls == 1
        && info.stats.total_syscalls == INFO_FLOOD_CALLS + 1
        && io.stats.histogram[SYS_GET_TASKINFO] == 0
        && info.stats.histogram[SYS_WRITE] == 0
        && compute.stats.histogram[SYS_WRITE] == 0;
    let trend_pass = io.stats.histogram[SYS_WRITE] > info.stats.histogram[SYS_WRITE]
        && io.stats.histogram[SYS_WRITE] > compute.stats.histogram[SYS_WRITE]
        && info.stats.histogram[SYS_GET_TASKINFO] > io.stats.histogram[SYS_GET_TASKINFO]
        && info.stats.histogram[SYS_GET_TASKINFO] > compute.stats.histogram[SYS_GET_TASKINFO]
        && compute.elapsed_cycles() > io.elapsed_cycles()
        && compute.elapsed_cycles() > info.elapsed_cycles();

    println!(
        "[kernel] check io_burst write bucket dominance: {}",
        pass_fail(io_pass)
    );
    println!(
        "[kernel] check compute_spin exit-only histogram: {}",
        pass_fail(compute_pass)
    );
    println!(
        "[kernel] check info_flood get_taskinfo bucket dominance: {}",
        pass_fail(info_pass)
    );
    println!(
        "[kernel] check illegal_trap keeps histogram clean: {}",
        pass_fail(illegal_pass)
    );
    println!(
        "[kernel] check per-task isolation across launches: {}",
        pass_fail(isolation_pass)
    );
    println!(
        "[kernel] cross-check with user-side workload trends: {}",
        pass_fail(trend_pass)
    );

    if io_pass && compute_pass && info_pass && illegal_pass && isolation_pass && trend_pass {
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
    let snapshot = TaskInfo {
        task_id: task.id,
        task_name: task.task_name,
        total_syscalls: task.stats.total_syscalls,
        write_calls: task.stats.write_calls(),
        get_taskinfo_calls: task.stats.get_taskinfo_calls(),
        error_syscalls: task.stats.error_syscalls,
        elapsed_cycles: read_cycle().wrapping_sub(task.start_cycle),
    };
    *task_info = snapshot;

    0
}

fn record_syscall_number(nr: usize) {
    unsafe {
        let task = &mut TASKS[CURRENT_TASK_INDEX];
        task.stats.total_syscalls += 1;
        if nr < SYSCALL_HIST_LEN {
            task.stats.histogram[nr] += 1;
        } else {
            task.stats.unknown_syscalls += 1;
        }
    }
}

fn record_syscall_error() {
    unsafe {
        TASKS[CURRENT_TASK_INDEX].stats.error_syscalls += 1;
    }
}

fn current_task() -> TaskControlBlock {
    unsafe { TASKS[CURRENT_TASK_INDEX] }
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

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1)
}
