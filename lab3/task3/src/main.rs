#![no_std]
#![no_main]

mod console;
mod syscall;
mod trap;

use core::arch::{asm, global_asm};
use core::hint::black_box;
use core::panic::PanicInfo;
use core::ptr;
use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const TASK_COUNT: usize = 2;
const NO_TASK: usize = usize::MAX;

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;

const USER_ENV_CALL: usize = 8;

const COMPUTE_ITERATIONS: u64 = 4_000_000;
const SYSCALL_ITERATIONS: u64 = 60_000;
const KERNEL_PROBE_SPIN: u64 = 48;

pub const SYS_PROBE: usize = 0;
pub const SYS_FINISH: usize = 1;
pub const ENOSYS: isize = -38;

#[derive(Clone, Copy)]
struct TaskDefinition {
    name: &'static str,
    entry: extern "C" fn() -> !,
}

#[derive(Clone, Copy)]
struct TaskContext {
    frame: TrapFrame,
}

impl TaskContext {
    const fn empty() -> Self {
        Self {
            frame: TrapFrame::zeroed(),
        }
    }
}

#[derive(Clone, Copy)]
struct TaskStats {
    user_ticks: u64,
    kernel_ticks: u64,
    syscalls: u64,
    result: u64,
    finished: bool,
    kernel_probe_acc: u64,
}

impl TaskStats {
    const fn empty() -> Self {
        Self {
            user_ticks: 0,
            kernel_ticks: 0,
            syscalls: 0,
            result: 0,
            finished: false,
            kernel_probe_acc: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CpuMode {
    Idle,
    User,
    Kernel,
}

static TASK_DEFS: [TaskDefinition; TASK_COUNT] = [
    TaskDefinition {
        name: "compute_background",
        entry: compute_task_entry,
    },
    TaskDefinition {
        name: "syscall_probe",
        entry: syscall_task_entry,
    },
];

static mut TASK_CONTEXTS: [TaskContext; TASK_COUNT] = [TaskContext::empty(); TASK_COUNT];
static mut TASK_STATS: [TaskStats; TASK_COUNT] = [TaskStats::empty(); TASK_COUNT];
static mut CURRENT_TASK: usize = NO_TASK;
static mut CPU_MODE: CpuMode = CpuMode::Idle;
static mut LAST_ACCOUNT_TICK: u64 = 0;

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __user_task0_stack_top: u8;
    static __user_task1_stack_top: u8;

    fn enter_task(frame: *const TrapFrame, kernel_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_kernel() -> ! {
    clear_bss();
    trap::init_trap_vector();
    configure_pmp();

    unsafe {
        TASK_CONTEXTS = [TaskContext::empty(); TASK_COUNT];
        TASK_STATS = [TaskStats::empty(); TASK_COUNT];
        CPU_MODE = CpuMode::Idle;
        CURRENT_TASK = NO_TASK;
        LAST_ACCOUNT_TICK = read_mtime();
    }

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB3 task3 user/kernel CPU-time accounting verifier");
    println!(
        "[kernel] time source: mtime @ {:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] workloads: compute iterations={}, syscall iterations={}, kernel_probe_spin={}",
        COMPUTE_ITERATIONS,
        SYSCALL_ITERATIONS,
        KERNEL_PROBE_SPIN
    );

    prepare_all_tasks();
    launch_task(0);

    unsafe {
        enter_task(task_frame_ptr(0), kernel_stack_top());
    }
}

pub fn dispatch_trap(frame: &mut TrapFrame, mcause: usize, mtval: usize) {
    let trap_tick = read_mtime();
    account_user_entry(trap_tick);

    match mcause {
        USER_ENV_CALL => {
            frame.mepc = frame.mepc.wrapping_add(4);
            handle_syscall(frame);
        }
        _ => {
            println!(
                "[kernel] unexpected trap: mcause={:#x} mepc={:#x} mtval={:#x}",
                mcause, frame.mepc, mtval
            );
            qemu_exit(1);
        }
    }
}

fn handle_syscall(frame: &mut TrapFrame) {
    match frame.a7 {
        SYS_PROBE => {
            frame.a0 = sys_probe() as usize;
            return_to_current_task();
        }
        SYS_FINISH => finish_current_task(frame.a0 as u64, frame),
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            frame.a0 = ENOSYS as usize;
            return_to_current_task();
        }
    }
}

fn sys_probe() -> isize {
    let current = current_task();

    unsafe {
        TASK_STATS[current].syscalls += 1;
    }

    let mut acc = unsafe { TASK_STATS[current].kernel_probe_acc }
        .wrapping_add(0x9e37_79b9_7f4a_7c15u64 ^ current as u64);
    let mut index = 0u64;

    while index < KERNEL_PROBE_SPIN {
        acc = acc.rotate_left(7).wrapping_add(index ^ (current as u64 * 17));
        index += 1;
    }

    unsafe {
        TASK_STATS[current].kernel_probe_acc = acc;
    }

    0
}

fn return_to_current_task() {
    let current = current_task();
    let now = read_mtime();

    account_kernel_slice(current, now);

    unsafe {
        CPU_MODE = CpuMode::User;
        LAST_ACCOUNT_TICK = now;
    }
}

fn finish_current_task(result: u64, frame: &mut TrapFrame) {
    let current = current_task();

    unsafe {
        TASK_STATS[current].syscalls += 1;
        TASK_STATS[current].result = result;
    }

    let now = read_mtime();
    account_kernel_slice(current, now);

    unsafe {
        TASK_STATS[current].finished = true;
        CPU_MODE = CpuMode::Idle;
        CURRENT_TASK = NO_TASK;
        LAST_ACCOUNT_TICK = now;
    }

    println!(
        "[kernel] task finished: {} result={:#018x}",
        TASK_DEFS[current].name,
        result
    );

    if let Some(next) = next_unfinished_task_after(current) {
        println!("[kernel] launching next task: {}", TASK_DEFS[next].name);
        launch_task(next);

        unsafe {
            *frame = TASK_CONTEXTS[next].frame;
        }
        return;
    }

    print_final_report();
}

fn prepare_all_tasks() {
    unsafe {
        TASK_CONTEXTS[0].frame = build_initial_frame(0);
        TASK_CONTEXTS[1].frame = build_initial_frame(1);
    }
}

fn launch_task(task_index: usize) {
    let now = read_mtime();

    unsafe {
        CURRENT_TASK = task_index;
        CPU_MODE = CpuMode::User;
        LAST_ACCOUNT_TICK = now;
    }

    println!(
        "[kernel] launching task {} ({})",
        task_index,
        TASK_DEFS[task_index].name
    );
}

fn build_initial_frame(task_index: usize) -> TrapFrame {
    let mut frame = TrapFrame::zeroed();
    frame.gp = read_gp();
    frame.tp = read_tp();
    frame.user_sp = user_stack_top(task_index);
    frame.mepc = TASK_DEFS[task_index].entry as *const () as usize;
    frame
}

fn next_unfinished_task_after(current: usize) -> Option<usize> {
    let mut candidate = current + 1;

    while candidate < TASK_COUNT {
        unsafe {
            if !TASK_STATS[candidate].finished {
                return Some(candidate);
            }
        }
        candidate += 1;
    }

    None
}

fn current_task() -> usize {
    unsafe {
        if CURRENT_TASK == NO_TASK {
            println!("[kernel] no current task while accounting");
            qemu_exit(1);
        }

        CURRENT_TASK
    }
}

fn account_user_entry(now: u64) {
    let current = current_task();

    unsafe {
        if CPU_MODE != CpuMode::User {
            println!("[kernel] trap arrived while cpu_mode was not user");
            qemu_exit(1);
        }

        TASK_STATS[current].user_ticks += now.saturating_sub(LAST_ACCOUNT_TICK);
        CPU_MODE = CpuMode::Kernel;
        LAST_ACCOUNT_TICK = now;
    }
}

fn account_kernel_slice(task_index: usize, now: u64) {
    unsafe {
        if CPU_MODE != CpuMode::Kernel {
            println!("[kernel] attempted to account kernel time while not in kernel mode");
            qemu_exit(1);
        }

        TASK_STATS[task_index].kernel_ticks += now.saturating_sub(LAST_ACCOUNT_TICK);
        LAST_ACCOUNT_TICK = now;
    }
}

fn print_final_report() -> ! {
    let compute = unsafe { TASK_STATS[0] };
    let syscall = unsafe { TASK_STATS[1] };

    println!("[kernel] final accounting summary begins");
    print_task_stats(0, compute);
    print_task_stats(1, syscall);

    let compute_user_dominant = compute.user_ticks > compute.kernel_ticks.saturating_mul(10);
    let syscall_kernel_ratio_rises =
        kernel_ratio_bp(syscall) > kernel_ratio_bp(compute).saturating_add(3_000)
            && syscall.kernel_ticks > syscall.user_ticks;

    println!(
        "[kernel] acceptance task1 compute user_time >> kernel_time: {}",
        pass_fail(compute_user_dominant)
    );
    println!(
        "[kernel] acceptance task2 syscall kernel ratio increased clearly: {}",
        pass_fail(syscall_kernel_ratio_rises)
    );
    println!(
        "[kernel] explanation: compute task traps only once at finish, while syscall task enters the kernel on every probe call"
    );

    qemu_exit(if compute_user_dominant && syscall_kernel_ratio_rises {
        0
    } else {
        1
    })
}

fn print_task_stats(task_index: usize, stats: TaskStats) {
    let total_ticks = stats.user_ticks.saturating_add(stats.kernel_ticks);
    let user_bp = ratio_bp(stats.user_ticks, total_ticks);
    let kernel_bp = ratio_bp(stats.kernel_ticks, total_ticks);

    println!(
        "[kernel] stats[{}]: user={} ticks ({} us, {}.{:02}%), kernel={} ticks ({} us, {}.{:02}%), total={} ticks ({} us), syscalls={}, result={:#018x}",
        TASK_DEFS[task_index].name,
        stats.user_ticks,
        ticks_to_us(stats.user_ticks),
        user_bp / 100,
        user_bp % 100,
        stats.kernel_ticks,
        ticks_to_us(stats.kernel_ticks),
        kernel_bp / 100,
        kernel_bp % 100,
        total_ticks,
        ticks_to_us(total_ticks),
        stats.syscalls,
        stats.result
    );
}

fn ratio_bp(part: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        part.saturating_mul(10_000) / total
    }
}

fn kernel_ratio_bp(stats: TaskStats) -> u64 {
    ratio_bp(stats.kernel_ticks, stats.user_ticks.saturating_add(stats.kernel_ticks))
}

fn ticks_to_us(ticks: u64) -> u64 {
    ticks.saturating_mul(1_000_000) / MTIME_FREQ_HZ
}

fn pass_fail(condition: bool) -> &'static str {
    if condition {
        "PASS"
    } else {
        "FAIL"
    }
}

fn task_frame_ptr(task_index: usize) -> *const TrapFrame {
    unsafe { &TASK_CONTEXTS[task_index].frame as *const TrapFrame }
}

fn kernel_stack_top() -> usize {
    ptr::addr_of!(__kernel_stack_top) as usize
}

fn user_stack_top(task_index: usize) -> usize {
    match task_index {
        0 => ptr::addr_of!(__user_task0_stack_top) as usize,
        1 => ptr::addr_of!(__user_task1_stack_top) as usize,
        _ => qemu_exit(1),
    }
}

#[no_mangle]
pub extern "C" fn compute_task_entry() -> ! {
    let mut acc = 0x1234_5678_9abc_def0u64;
    let mut index = 0u64;

    while index < COMPUTE_ITERATIONS {
        acc = acc
            .rotate_left(9)
            .wrapping_add(index ^ 0x9e37_79b9)
            .wrapping_mul(0x5851_f42d_4c95_7f2d);
        index += 1;
    }

    black_box(acc);
    syscall::finish(acc)
}

#[no_mangle]
pub extern "C" fn syscall_task_entry() -> ! {
    let mut count = 0u64;

    while count < SYSCALL_ITERATIONS {
        syscall::probe();
        count += 1;
    }

    syscall::finish(count)
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

fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
}

fn read_gp() -> usize {
    let value: usize;

    unsafe {
        asm!(
            "mv {}, gp",
            out(reg) value,
            options(nostack, nomem, preserves_flags)
        );
    }

    value
}

fn read_tp() -> usize {
    let value: usize;

    unsafe {
        asm!(
            "mv {}, tp",
            out(reg) value,
            options(nostack, nomem, preserves_flags)
        );
    }

    value
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
