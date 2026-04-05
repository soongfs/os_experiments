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

const ENABLE_ACCOUNTING_TRACE: bool = true;
const ACCOUNTING_TRACE_LIMIT: usize = 8;

pub const SYS_PROBE: usize = 0;
pub const SYS_FINISH: usize = 1;
pub const ENOSYS: isize = -38;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskMode {
    Dormant,
    User,
    Kernel,
    Finished,
}

#[derive(Clone, Copy)]
struct TaskDefinition {
    id: u64,
    name: &'static str,
    entry: extern "C" fn() -> !,
}

#[derive(Clone, Copy)]
struct ProcessControlBlock {
    frame: TrapFrame,
    utime: u64,
    stime: u64,
    syscalls: u64,
    trap_entries: u64,
    result: u64,
    start_tick: u64,
    finish_tick: u64,
    last_timestamp: u64,
    kernel_probe_acc: u64,
    started: bool,
    finished: bool,
    mode: TaskMode,
}

impl ProcessControlBlock {
    const fn empty() -> Self {
        Self {
            frame: TrapFrame::zeroed(),
            utime: 0,
            stime: 0,
            syscalls: 0,
            trap_entries: 0,
            result: 0,
            start_tick: 0,
            finish_tick: 0,
            last_timestamp: 0,
            kernel_probe_acc: 0,
            started: false,
            finished: false,
            mode: TaskMode::Dormant,
        }
    }
}

static TASK_DEFS: [TaskDefinition; TASK_COUNT] = [
    TaskDefinition {
        id: 1,
        name: "compute_user_heavy",
        entry: compute_task_entry,
    },
    TaskDefinition {
        id: 2,
        name: "syscall_kernel_heavy",
        entry: syscall_task_entry,
    },
];

static mut PCBS: [ProcessControlBlock; TASK_COUNT] = [ProcessControlBlock::empty(); TASK_COUNT];
static mut CURRENT_TASK: usize = NO_TASK;
static mut KERNEL_ACCOUNT_TASK: usize = NO_TASK;
static mut ACCOUNTING_TRACE_EMITTED: usize = 0;
static mut TRAP_ENTRY_ACCOUNT_EVENTS: u64 = 0;
static mut TRAP_EXIT_ACCOUNT_EVENTS: u64 = 0;
static mut TASK_COMPLETE_ACCOUNT_EVENTS: u64 = 0;

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
    initialize_pcbs();
    prepare_all_tasks();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB3 kernel task2 user/kernel completion-time accounting");
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
    println!(
        "[kernel] accounting rule: trap_enter adds to pcb.utime, trap_exit/task_complete adds to pcb.stime"
    );
    println!(
        "[kernel] accounting trace: enabled={}, limit={} event(s)",
        ENABLE_ACCOUNTING_TRACE,
        ACCOUNTING_TRACE_LIMIT
    );

    launch_initial_task(0);

    unsafe { enter_task(pcb_frame_ptr(0), kernel_stack_top()) }
}

pub fn account_trap_enter(frame: &TrapFrame, _mcause: usize, _mtval: usize) {
    let now = read_mtime();
    let current = current_task();
    let delta;
    let utime_total;

    unsafe {
        let pcb = &mut PCBS[current];

        if pcb.mode != TaskMode::User {
            println!(
                "[kernel] trap entered while task {} was not marked as user",
                TASK_DEFS[current].name
            );
            qemu_exit(1);
        }

        delta = now.saturating_sub(pcb.last_timestamp);
        pcb.utime = pcb.utime.saturating_add(delta);
        pcb.last_timestamp = now;
        pcb.mode = TaskMode::Kernel;
        pcb.trap_entries = pcb.trap_entries.saturating_add(1);
        utime_total = pcb.utime;

        KERNEL_ACCOUNT_TASK = current;
        TRAP_ENTRY_ACCOUNT_EVENTS = TRAP_ENTRY_ACCOUNT_EVENTS.saturating_add(1);
    }

    emit_trap_enter_trace(current, frame.mepc, delta, utime_total);
}

pub fn dispatch_trap(frame: &mut TrapFrame, mcause: usize, mtval: usize) {
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

pub fn account_trap_exit(frame: &TrapFrame) {
    let now = read_mtime();
    let charged_task = kernel_account_task();
    let resumed_task = unsafe { CURRENT_TASK };
    let delta = charge_kernel_slice(charged_task, now);
    let stime_total = unsafe { PCBS[charged_task].stime };

    unsafe {
        TRAP_EXIT_ACCOUNT_EVENTS = TRAP_EXIT_ACCOUNT_EVENTS.saturating_add(1);
    }

    if resumed_task != NO_TASK {
        activate_task_for_user(resumed_task, now);
    }

    unsafe {
        KERNEL_ACCOUNT_TASK = NO_TASK;
    }

    emit_trap_exit_trace(charged_task, resumed_task, frame.mepc, delta, stime_total);
}

fn handle_syscall(frame: &mut TrapFrame) {
    match frame.a7 {
        SYS_PROBE => frame.a0 = sys_probe() as usize,
        SYS_FINISH => finish_current_task(frame.a0 as u64, frame),
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            frame.a0 = ENOSYS as usize;
        }
    }
}

fn sys_probe() -> isize {
    let current = kernel_account_task();

    unsafe {
        PCBS[current].syscalls = PCBS[current].syscalls.saturating_add(1);
    }

    let mut acc = unsafe { PCBS[current].kernel_probe_acc }
        .wrapping_add(0x9e37_79b9_7f4a_7c15u64 ^ current as u64);
    let mut index = 0u64;

    while index < KERNEL_PROBE_SPIN {
        acc = acc.rotate_left(7).wrapping_add(index ^ (current as u64 * 17));
        index += 1;
    }

    unsafe {
        PCBS[current].kernel_probe_acc = acc;
    }

    0
}

fn finish_current_task(result: u64, frame: &mut TrapFrame) {
    let current = kernel_account_task();

    unsafe {
        PCBS[current].syscalls = PCBS[current].syscalls.saturating_add(1);
        PCBS[current].result = result;
        PCBS[current].finished = true;
    }

    println!(
        "[kernel] task finished: id={} name={} result={:#018x}",
        TASK_DEFS[current].id,
        TASK_DEFS[current].name,
        result
    );

    if let Some(next) = next_unfinished_task_after(current) {
        println!(
            "[kernel] scheduling next task: id={} name={}",
            TASK_DEFS[next].id,
            TASK_DEFS[next].name
        );

        unsafe {
            CURRENT_TASK = next;
            *frame = PCBS[next].frame;
        }

        return;
    }

    finalize_last_task_and_report()
}

fn finalize_last_task_and_report() -> ! {
    let current = kernel_account_task();
    let now = read_mtime();
    let delta = charge_kernel_slice(current, now);
    let stime_total = unsafe { PCBS[current].stime };

    unsafe {
        TASK_COMPLETE_ACCOUNT_EVENTS = TASK_COMPLETE_ACCOUNT_EVENTS.saturating_add(1);
        CURRENT_TASK = NO_TASK;
        KERNEL_ACCOUNT_TASK = NO_TASK;
    }

    emit_task_complete_trace(current, delta, stime_total);
    print_final_report()
}

fn initialize_pcbs() {
    unsafe {
        PCBS = [ProcessControlBlock::empty(); TASK_COUNT];
        CURRENT_TASK = NO_TASK;
        KERNEL_ACCOUNT_TASK = NO_TASK;
        ACCOUNTING_TRACE_EMITTED = 0;
        TRAP_ENTRY_ACCOUNT_EVENTS = 0;
        TRAP_EXIT_ACCOUNT_EVENTS = 0;
        TASK_COMPLETE_ACCOUNT_EVENTS = 0;
    }
}

fn prepare_all_tasks() {
    unsafe {
        PCBS[0].frame = build_initial_frame(0);
        PCBS[1].frame = build_initial_frame(1);
    }
}

fn launch_initial_task(task_index: usize) {
    let now = read_mtime();
    activate_task_for_user(task_index, now);
    println!(
        "[kernel] launching task: id={} name={}",
        TASK_DEFS[task_index].id,
        TASK_DEFS[task_index].name
    );
}

fn activate_task_for_user(task_index: usize, now: u64) {
    unsafe {
        let pcb = &mut PCBS[task_index];

        if pcb.finished {
            println!(
                "[kernel] attempted to resume finished task {}",
                TASK_DEFS[task_index].name
            );
            qemu_exit(1);
        }

        if !pcb.started {
            pcb.start_tick = now;
            pcb.started = true;
        }

        pcb.last_timestamp = now;
        pcb.mode = TaskMode::User;
        CURRENT_TASK = task_index;
    }
}

fn charge_kernel_slice(task_index: usize, now: u64) -> u64 {
    let delta;

    unsafe {
        let pcb = &mut PCBS[task_index];

        if pcb.mode != TaskMode::Kernel {
            println!(
                "[kernel] attempted to charge kernel time while task {} was not in kernel mode",
                TASK_DEFS[task_index].name
            );
            qemu_exit(1);
        }

        delta = now.saturating_sub(pcb.last_timestamp);
        pcb.stime = pcb.stime.saturating_add(delta);
        pcb.last_timestamp = now;

        if pcb.finished {
            pcb.finish_tick = now;
            pcb.mode = TaskMode::Finished;
        } else {
            pcb.mode = TaskMode::Dormant;
        }
    }

    delta
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
            if !PCBS[candidate].finished {
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
            println!("[kernel] no current task was set");
            qemu_exit(1);
        }

        CURRENT_TASK
    }
}

fn kernel_account_task() -> usize {
    unsafe {
        if KERNEL_ACCOUNT_TASK == NO_TASK {
            println!("[kernel] no kernel accounting owner was set");
            qemu_exit(1);
        }

        KERNEL_ACCOUNT_TASK
    }
}

fn print_final_report() -> ! {
    let compute = unsafe { PCBS[0] };
    let syscall = unsafe { PCBS[1] };
    let total_syscalls = compute.syscalls.saturating_add(syscall.syscalls);
    let trap_entry_updates = unsafe { TRAP_ENTRY_ACCOUNT_EVENTS };
    let trap_exit_updates = unsafe { TRAP_EXIT_ACCOUNT_EVENTS };
    let task_complete_updates = unsafe { TASK_COMPLETE_ACCOUNT_EVENTS };

    let pcb_has_utime_stime =
        compute.utime > 0 && compute.stime > 0 && syscall.utime > 0 && syscall.stime > 0;
    let trap_updates_balanced = trap_entry_updates == total_syscalls
        && trap_exit_updates.saturating_add(task_complete_updates) == trap_entry_updates;
    let compute_user_dominant = compute.utime > compute.stime.saturating_mul(10);
    let syscall_kernel_ratio_rises =
        kernel_ratio_bp(syscall) > kernel_ratio_bp(compute).saturating_add(3_000)
            && syscall.stime > syscall.utime;

    println!("[kernel] final accounting summary begins");
    print_task_stats(0, compute);
    print_task_stats(1, syscall);
    println!(
        "[kernel] accounting events: trap_enter_updates={} trap_exit_updates={} task_complete_updates={}",
        trap_entry_updates,
        trap_exit_updates,
        task_complete_updates
    );
    println!(
        "[kernel] accounting scope: utime=[last_timestamp, trap_enter), stime=[last_timestamp, trap_exit/task_complete)"
    );
    println!(
        "[kernel] acceptance pcb utime/stime maintained: {}",
        pass_fail(pcb_has_utime_stime)
    );
    println!(
        "[kernel] acceptance trap enter/exit timestamp updates balanced: {}",
        pass_fail(trap_updates_balanced)
    );
    println!(
        "[kernel] acceptance compute task utime >> stime: {}",
        pass_fail(compute_user_dominant)
    );
    println!(
        "[kernel] acceptance syscall task kernel ratio increased clearly: {}",
        pass_fail(syscall_kernel_ratio_rises)
    );

    qemu_exit(
        if pcb_has_utime_stime
            && trap_updates_balanced
            && compute_user_dominant
            && syscall_kernel_ratio_rises
        {
            0
        } else {
            1
        },
    )
}

fn print_task_stats(task_index: usize, pcb: ProcessControlBlock) {
    let accounted = pcb.utime.saturating_add(pcb.stime);
    let elapsed = pcb.finish_tick.saturating_sub(pcb.start_tick);
    let gap = abs_diff(accounted, elapsed);
    let user_bp = ratio_bp(pcb.utime, accounted);
    let kernel_bp = ratio_bp(pcb.stime, accounted);

    println!(
        "[kernel] pcb[{}:{}]: utime={} ticks ({} us, {}.{:02}%), stime={} ticks ({} us, {}.{:02}%), total_accounted={} ticks ({} us), elapsed={} ticks ({} us), gap={} ticks, syscalls={}, trap_entries={}, result={:#018x}",
        TASK_DEFS[task_index].id,
        TASK_DEFS[task_index].name,
        pcb.utime,
        ticks_to_us(pcb.utime),
        user_bp / 100,
        user_bp % 100,
        pcb.stime,
        ticks_to_us(pcb.stime),
        kernel_bp / 100,
        kernel_bp % 100,
        accounted,
        ticks_to_us(accounted),
        elapsed,
        ticks_to_us(elapsed),
        gap,
        pcb.syscalls,
        pcb.trap_entries,
        pcb.result
    );
}

fn ratio_bp(part: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        part.saturating_mul(10_000) / total
    }
}

fn kernel_ratio_bp(pcb: ProcessControlBlock) -> u64 {
    ratio_bp(pcb.stime, pcb.utime.saturating_add(pcb.stime))
}

fn ticks_to_us(ticks: u64) -> u64 {
    ticks.saturating_mul(1_000_000) / MTIME_FREQ_HZ
}

fn abs_diff(lhs: u64, rhs: u64) -> u64 {
    if lhs >= rhs {
        lhs - rhs
    } else {
        rhs - lhs
    }
}

fn pass_fail(condition: bool) -> &'static str {
    if condition {
        "PASS"
    } else {
        "FAIL"
    }
}

fn pcb_frame_ptr(task_index: usize) -> *const TrapFrame {
    unsafe { &PCBS[task_index].frame as *const TrapFrame }
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

fn emit_trap_enter_trace(task_index: usize, mepc: usize, delta: u64, utime_total: u64) {
    if acquire_trace_slot() {
        println!(
            "[acct] trap_enter: task={}({}) mepc={:#x} add_utime={} ticks total_utime={} ticks",
            TASK_DEFS[task_index].id,
            TASK_DEFS[task_index].name,
            mepc,
            delta,
            utime_total
        );
    }
}

fn emit_trap_exit_trace(
    charged_task: usize,
    resumed_task: usize,
    next_mepc: usize,
    delta: u64,
    stime_total: u64,
) {
    if acquire_trace_slot() {
        if resumed_task == NO_TASK {
            println!(
                "[acct] trap_exit: charge={}({}) next=none add_stime={} ticks total_stime={} ticks",
                TASK_DEFS[charged_task].id,
                TASK_DEFS[charged_task].name,
                delta,
                stime_total
            );
        } else {
            println!(
                "[acct] trap_exit: charge={}({}) resume={}({}) next_mepc={:#x} add_stime={} ticks total_stime={} ticks",
                TASK_DEFS[charged_task].id,
                TASK_DEFS[charged_task].name,
                TASK_DEFS[resumed_task].id,
                TASK_DEFS[resumed_task].name,
                next_mepc,
                delta,
                stime_total
            );
        }
    }
}

fn emit_task_complete_trace(task_index: usize, delta: u64, stime_total: u64) {
    if acquire_trace_slot() {
        println!(
            "[acct] task_complete: task={}({}) add_stime={} ticks total_stime={} ticks",
            TASK_DEFS[task_index].id,
            TASK_DEFS[task_index].name,
            delta,
            stime_total
        );
    }
}

fn acquire_trace_slot() -> bool {
    if !ENABLE_ACCOUNTING_TRACE {
        return false;
    }

    unsafe {
        if ACCOUNTING_TRACE_EMITTED < ACCOUNTING_TRACE_LIMIT {
            ACCOUNTING_TRACE_EMITTED += 1;
            true
        } else if ACCOUNTING_TRACE_EMITTED == ACCOUNTING_TRACE_LIMIT {
            ACCOUNTING_TRACE_EMITTED += 1;
            println!(
                "[acct] trace limit reached at {} event(s); further accounting logs suppressed",
                ACCOUNTING_TRACE_LIMIT
            );
            false
        } else {
            false
        }
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
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1);
}
