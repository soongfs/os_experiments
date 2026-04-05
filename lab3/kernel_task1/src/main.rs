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
const CLINT_MTIMECMP_OFFSET: usize = 0x4000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIMECMP_ADDR: usize = CLINT_BASE + CLINT_MTIMECMP_OFFSET;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;
const TIME_SLICE_TICKS: u64 = 18_000;

const MIE_MTIE: usize = 1 << 7;
const INTERRUPT_BIT: usize = 1usize << (usize::BITS - 1);
const USER_ENV_CALL: usize = 8;
const MACHINE_TIMER_INTERRUPT: usize = INTERRUPT_BIT | 7;

const ENABLE_SWITCH_TRACE: bool = true;
const SWITCH_TRACE_LIMIT: usize = 10;

const YIELD_TASK_ROUNDS: usize = 3;
const YIELD_TASK_SPIN: u64 = 45_000;
const TIMESLICE_TASK_PHASES: usize = 3;
const TIMESLICE_TASK_SPIN: u64 = 1_250_000;

pub const SYS_YIELD: usize = 0;
pub const SYS_FINISH: usize = 1;
pub const ENOSYS: isize = -38;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Runnable,
    Finished,
}

#[derive(Clone, Copy)]
struct TaskDefinition {
    id: u64,
    name: &'static str,
    entry: extern "C" fn() -> !,
}

#[derive(Clone, Copy)]
struct TaskControlBlock {
    state: TaskState,
    frame: TrapFrame,
    exit_code: u64,
}

impl TaskControlBlock {
    const fn empty() -> Self {
        Self {
            state: TaskState::Runnable,
            frame: TrapFrame::zeroed(),
            exit_code: 0,
        }
    }
}

#[derive(Clone, Copy)]
enum SwitchReason {
    Boot,
    ExplicitYield,
    TimeSlice,
    TaskExit,
}

impl SwitchReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::ExplicitYield => "explicit_yield",
            Self::TimeSlice => "time_slice",
            Self::TaskExit => "task_exit",
        }
    }
}

static TASK_DEFS: [TaskDefinition; TASK_COUNT] = [
    TaskDefinition {
        id: 1,
        name: "yield_demo",
        entry: yield_demo_entry,
    },
    TaskDefinition {
        id: 2,
        name: "timeslice_demo",
        entry: timeslice_demo_entry,
    },
];

static mut TASKS: [TaskControlBlock; TASK_COUNT] = [TaskControlBlock::empty(); TASK_COUNT];
static mut CURRENT_TASK: usize = NO_TASK;
static mut TIMER_INTERRUPT_COUNT: usize = 0;
static mut EXPLICIT_YIELD_SWITCHES: usize = 0;
static mut TIME_SLICE_SWITCHES: usize = 0;
static mut TASK_EXIT_SWITCHES: usize = 0;
static mut TOTAL_SWITCHES: usize = 0;
static mut SWITCH_TRACE_EMITTED: usize = 0;

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
    initialize_tasks();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB3 kernel task1 task-switch visualization");
    println!(
        "[kernel] timer source: mtime={:#x}, mtimecmp={:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIMECMP_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] switch trace: enabled={}, limit={} record(s)",
        ENABLE_SWITCH_TRACE,
        SWITCH_TRACE_LIMIT
    );
    println!(
        "[kernel] workloads: {} rounds of explicit yield, {} long phases for timer preemption",
        YIELD_TASK_ROUNDS,
        TIMESLICE_TASK_PHASES
    );

    enable_timer_interrupts();
    arm_next_timer_interrupt();
    log_initial_restore(0);

    unsafe {
        CURRENT_TASK = 0;
        enter_task(task_frame_ptr(0), kernel_stack_top());
    }
}

pub fn dispatch_trap(frame: &mut TrapFrame, mcause: usize, mtval: usize) {
    match mcause {
        USER_ENV_CALL => {
            frame.mepc = frame.mepc.wrapping_add(4);
            handle_syscall(frame);
        }
        MACHINE_TIMER_INTERRUPT => handle_timer_interrupt(frame),
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
        SYS_YIELD => handle_explicit_yield(frame),
        SYS_FINISH => handle_finish(frame.a0 as u64, frame),
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            frame.a0 = ENOSYS as usize;
        }
    }
}

fn handle_explicit_yield(frame: &mut TrapFrame) {
    let current = current_task();
    save_current_frame(current, frame);

    if let Some(next) = next_runnable_excluding(current) {
        unsafe {
            EXPLICIT_YIELD_SWITCHES += 1;
        }
        switch_to(frame, current, next, SwitchReason::ExplicitYield);
    }
}

fn handle_timer_interrupt(frame: &mut TrapFrame) {
    arm_next_timer_interrupt();

    unsafe {
        TIMER_INTERRUPT_COUNT += 1;
    }

    let current = current_task();
    save_current_frame(current, frame);

    if let Some(next) = next_runnable_excluding(current) {
        unsafe {
            TIME_SLICE_SWITCHES += 1;
        }
        switch_to(frame, current, next, SwitchReason::TimeSlice);
    }
}

fn handle_finish(code: u64, frame: &mut TrapFrame) {
    let current = current_task();

    unsafe {
        TASKS[current].state = TaskState::Finished;
        TASKS[current].exit_code = code;
    }

    println!(
        "[kernel] task exit: id={} name={} code={:#x}",
        TASK_DEFS[current].id,
        TASK_DEFS[current].name,
        code
    );

    if let Some(next) = next_runnable_excluding(current) {
        unsafe {
            TASK_EXIT_SWITCHES += 1;
        }
        switch_to(frame, current, next, SwitchReason::TaskExit);
    } else {
        finish_experiment();
    }
}

fn switch_to(frame: &mut TrapFrame, from: usize, to: usize, reason: SwitchReason) {
    let from_id = TASK_DEFS[from].id;
    let from_name = TASK_DEFS[from].name;
    let from_mepc = unsafe { TASKS[from].frame.mepc };
    let to_id = TASK_DEFS[to].id;
    let to_name = TASK_DEFS[to].name;
    let to_mepc = unsafe { TASKS[to].frame.mepc };

    let switch_index = unsafe {
        TOTAL_SWITCHES += 1;
        TOTAL_SWITCHES
    };

    emit_switch_trace(
        switch_index,
        reason,
        from_id,
        from_name,
        from_mepc,
        to_id,
        to_name,
        to_mepc,
    );

    unsafe {
        CURRENT_TASK = to;
        *frame = TASKS[to].frame;
    }
}

fn emit_switch_trace(
    switch_index: usize,
    reason: SwitchReason,
    from_id: u64,
    from_name: &str,
    from_mepc: usize,
    to_id: u64,
    to_name: &str,
    to_mepc: usize,
) {
    if !ENABLE_SWITCH_TRACE {
        return;
    }

    let should_emit = unsafe { SWITCH_TRACE_EMITTED < SWITCH_TRACE_LIMIT };
    if !should_emit {
        return;
    }

    println!(
        "[sched] switch#{:02} save_done: from id={} name={} reason={} saved_mepc={:#x}",
        switch_index,
        from_id,
        from_name,
        reason.as_str(),
        from_mepc
    );
    println!(
        "[sched] switch#{:02} restore_begin: to id={} name={} reason={} next_mepc={:#x}",
        switch_index,
        to_id,
        to_name,
        reason.as_str(),
        to_mepc
    );

    unsafe {
        SWITCH_TRACE_EMITTED += 1;
        if SWITCH_TRACE_EMITTED == SWITCH_TRACE_LIMIT {
            println!(
                "[sched] switch trace limit reached at {} record(s); further switches suppressed",
                SWITCH_TRACE_LIMIT
            );
        }
    }
}

fn log_initial_restore(task_index: usize) {
    if !ENABLE_SWITCH_TRACE {
        return;
    }

    println!(
        "[sched] boot restore_begin: to id={} name={} reason={} next_mepc={:#x}",
        TASK_DEFS[task_index].id,
        TASK_DEFS[task_index].name,
        SwitchReason::Boot.as_str(),
        unsafe { TASKS[task_index].frame.mepc }
    );
}

fn initialize_tasks() {
    unsafe {
        TASKS[0].state = TaskState::Runnable;
        TASKS[0].frame = build_initial_frame(0);
        TASKS[0].exit_code = 0;

        TASKS[1].state = TaskState::Runnable;
        TASKS[1].frame = build_initial_frame(1);
        TASKS[1].exit_code = 0;

        CURRENT_TASK = NO_TASK;
        TIMER_INTERRUPT_COUNT = 0;
        EXPLICIT_YIELD_SWITCHES = 0;
        TIME_SLICE_SWITCHES = 0;
        TASK_EXIT_SWITCHES = 0;
        TOTAL_SWITCHES = 0;
        SWITCH_TRACE_EMITTED = 0;
    }
}

fn build_initial_frame(task_index: usize) -> TrapFrame {
    let mut frame = TrapFrame::zeroed();
    frame.gp = read_gp();
    frame.tp = read_tp();
    frame.user_sp = user_stack_top(task_index);
    frame.mepc = TASK_DEFS[task_index].entry as *const () as usize;
    frame
}

fn save_current_frame(current: usize, frame: &TrapFrame) {
    unsafe {
        TASKS[current].frame = *frame;
    }
}

fn next_runnable_excluding(current: usize) -> Option<usize> {
    for offset in 1..=TASK_COUNT {
        let candidate = (current + offset) % TASK_COUNT;
        let runnable = unsafe { TASKS[candidate].state == TaskState::Runnable };
        if runnable {
            return Some(candidate);
        }
    }

    None
}

fn current_task() -> usize {
    unsafe {
        if CURRENT_TASK == NO_TASK {
            println!("[kernel] no current task set");
            qemu_exit(1);
        }

        CURRENT_TASK
    }
}

fn finish_experiment() -> ! {
    disable_timer_interrupts();
    disarm_timer_interrupt();

    let total_switches = unsafe { TOTAL_SWITCHES };
    let explicit_yields = unsafe { EXPLICIT_YIELD_SWITCHES };
    let time_slices = unsafe { TIME_SLICE_SWITCHES };
    let exit_switches = unsafe { TASK_EXIT_SWITCHES };
    let timer_interrupts = unsafe { TIMER_INTERRUPT_COUNT };

    println!("[kernel] switch summary begins");
    println!(
        "[kernel] summary: total_switches={} explicit_yield_switches={} time_slice_switches={} task_exit_switches={} timer_interrupts={}",
        total_switches,
        explicit_yields,
        time_slices,
        exit_switches,
        timer_interrupts
    );
    for index in 0..TASK_COUNT {
        let exit_code = unsafe { TASKS[index].exit_code };
        println!(
            "[kernel] task result: id={} name={} exit_code={:#x}",
            TASK_DEFS[index].id,
            TASK_DEFS[index].name,
            exit_code
        );
    }
    println!(
        "[kernel] acceptance: switch logs came from scheduler/switch_to path with save_done + restore_begin markers"
    );

    let pass = explicit_yields > 0 && time_slices > 0 && total_switches >= 3;
    println!(
        "[kernel] acceptance explicit_yield observed: {}",
        pass_fail(explicit_yields > 0)
    );
    println!(
        "[kernel] acceptance time_slice observed: {}",
        pass_fail(time_slices > 0)
    );
    println!(
        "[kernel] acceptance multiple readable switches captured: {}",
        pass_fail(total_switches >= 3)
    );

    qemu_exit(if pass { 0 } else { 1 })
}

#[no_mangle]
pub extern "C" fn yield_demo_entry() -> ! {
    let mut round = 0usize;
    let mut acc = 0x1234_5678_9abc_def0u64;

    while round < YIELD_TASK_ROUNDS {
        syscall::yield_now();
        acc = busy_mix(acc, YIELD_TASK_SPIN);
        round += 1;
    }

    syscall::finish(acc)
}

#[no_mangle]
pub extern "C" fn timeslice_demo_entry() -> ! {
    let mut phase = 0usize;
    let mut acc = 0xfedc_ba98_7654_3210u64;

    while phase < TIMESLICE_TASK_PHASES {
        acc = busy_mix(acc, TIMESLICE_TASK_SPIN);
        phase += 1;
    }

    syscall::finish(acc)
}

#[inline(never)]
fn busy_mix(mut acc: u64, iterations: u64) -> u64 {
    let mut index = 0u64;

    while index < iterations {
        acc = acc
            .rotate_left(5)
            .wrapping_add(index ^ 0x9e37_79b9_7f4a_7c15)
            .wrapping_mul(0x5851_f42d_4c95_7f2d);
        index += 1;
    }

    black_box(acc)
}

fn kernel_stack_top() -> usize {
    ptr::addr_of!(__kernel_stack_top) as usize
}

fn task_frame_ptr(task_index: usize) -> *const TrapFrame {
    unsafe { &TASKS[task_index].frame as *const TrapFrame }
}

fn user_stack_top(task_index: usize) -> usize {
    match task_index {
        0 => ptr::addr_of!(__user_task0_stack_top) as usize,
        1 => ptr::addr_of!(__user_task1_stack_top) as usize,
        _ => qemu_exit(1),
    }
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

fn enable_timer_interrupts() {
    unsafe {
        asm!("csrs mie, {}", in(reg) MIE_MTIE, options(nostack, nomem));
    }
}

fn disable_timer_interrupts() {
    unsafe {
        asm!("csrc mie, {}", in(reg) MIE_MTIE, options(nostack, nomem));
    }
}

fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
}

fn arm_next_timer_interrupt() {
    let deadline = read_mtime().wrapping_add(TIME_SLICE_TICKS);
    unsafe {
        ptr::write_volatile(MTIMECMP_ADDR as *mut u64, deadline);
    }
}

fn disarm_timer_interrupt() {
    unsafe {
        ptr::write_volatile(MTIMECMP_ADDR as *mut u64, u64::MAX);
    }
}

fn pass_fail(condition: bool) -> &'static str {
    if condition {
        "PASS"
    } else {
        "FAIL"
    }
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
