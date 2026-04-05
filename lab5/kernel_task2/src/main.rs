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

const TASK_COUNT: usize = 3;
const PHASE_COUNT: usize = 2;
const PRIORITY_LEVELS: usize = 4;
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

const ENABLE_SCHED_TRACE: bool = true;
const SCHED_TRACE_LIMIT: usize = 18;

const INTERACTIVE_ROUNDS: usize = 10;
const INTERACTIVE_SPIN: u64 = 10_000;
const COMPUTE_SHORT_SPIN: u64 = 600_000;
const COMPUTE_LONG_SPIN: u64 = 1_200_000;

pub const SYS_YIELD: usize = 0;
pub const SYS_FINISH: usize = 1;
pub const ENOSYS: isize = -38;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Runnable,
    Finished,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskClass {
    Interactive,
    Compute,
}

impl TaskClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Compute => "compute",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SchedulerPolicy {
    RoundRobin,
    StaticPriority,
}

impl SchedulerPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::RoundRobin => "round_robin",
            Self::StaticPriority => "static_priority",
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

#[derive(Clone, Copy)]
struct TaskDefinition {
    id: u64,
    name: &'static str,
    class: TaskClass,
    priority: usize,
    entry: extern "C" fn() -> !,
}

#[derive(Clone, Copy)]
struct TaskControlBlock {
    state: TaskState,
    frame: TrapFrame,
    exit_code: u64,
    switch_ins: u64,
    runtime_ticks: u64,
    last_resume_tick: u64,
    finish_tick: u64,
    finish_order: u64,
    explicit_yields: u64,
    time_slice_preemptions: u64,
}

impl TaskControlBlock {
    const fn empty() -> Self {
        Self {
            state: TaskState::Runnable,
            frame: TrapFrame::zeroed(),
            exit_code: 0,
            switch_ins: 0,
            runtime_ticks: 0,
            last_resume_tick: 0,
            finish_tick: 0,
            finish_order: 0,
            explicit_yields: 0,
            time_slice_preemptions: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct PhaseDefinition {
    name: &'static str,
    policy: SchedulerPolicy,
}

#[derive(Clone, Copy)]
struct TaskPhaseStats {
    pid: u64,
    priority: u64,
    switch_ins: u64,
    runtime_ticks: u64,
    finish_tick: u64,
    finish_order: u64,
    explicit_yields: u64,
    time_slice_preemptions: u64,
    exit_code: u64,
}

impl TaskPhaseStats {
    const fn empty() -> Self {
        Self {
            pid: 0,
            priority: 0,
            switch_ins: 0,
            runtime_ticks: 0,
            finish_tick: 0,
            finish_order: 0,
            explicit_yields: 0,
            time_slice_preemptions: 0,
            exit_code: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct PhaseSummary {
    total_switches: u64,
    explicit_yield_switches: u64,
    time_slice_switches: u64,
    task_exit_switches: u64,
    timer_interrupts: u64,
    task_stats: [TaskPhaseStats; TASK_COUNT],
}

impl PhaseSummary {
    const fn empty() -> Self {
        Self {
            total_switches: 0,
            explicit_yield_switches: 0,
            time_slice_switches: 0,
            task_exit_switches: 0,
            timer_interrupts: 0,
            task_stats: [TaskPhaseStats::empty(); TASK_COUNT],
        }
    }
}

static TASK_DEFS: [TaskDefinition; TASK_COUNT] = [
    TaskDefinition {
        id: 1,
        name: "interactive_proc",
        class: TaskClass::Interactive,
        priority: 0,
        entry: interactive_proc_entry,
    },
    TaskDefinition {
        id: 2,
        name: "compute_short",
        class: TaskClass::Compute,
        priority: 2,
        entry: compute_short_entry,
    },
    TaskDefinition {
        id: 3,
        name: "compute_long",
        class: TaskClass::Compute,
        priority: 2,
        entry: compute_long_entry,
    },
];

static PHASE_DEFS: [PhaseDefinition; PHASE_COUNT] = [
    PhaseDefinition {
        name: "rr_baseline",
        policy: SchedulerPolicy::RoundRobin,
    },
    PhaseDefinition {
        name: "priority_compare",
        policy: SchedulerPolicy::StaticPriority,
    },
];

static mut TASKS: [TaskControlBlock; TASK_COUNT] = [TaskControlBlock::empty(); TASK_COUNT];
static mut CURRENT_TASK: usize = NO_TASK;
static mut CURRENT_PHASE: usize = 0;
static mut PHASE_START_TICK: u64 = 0;
static mut FINISHED_TASKS: u64 = 0;
static mut TIMER_INTERRUPT_COUNT: u64 = 0;
static mut EXPLICIT_YIELD_SWITCHES: u64 = 0;
static mut TIME_SLICE_SWITCHES: u64 = 0;
static mut TASK_EXIT_SWITCHES: u64 = 0;
static mut TOTAL_SWITCHES: u64 = 0;
static mut SWITCH_TRACE_EMITTED: usize = 0;
static mut PHASE_SUMMARIES: [PhaseSummary; PHASE_COUNT] = [PhaseSummary::empty(); PHASE_COUNT];
static mut PRIORITY_LAST_PICK: [usize; PRIORITY_LEVELS] = [NO_TASK; PRIORITY_LEVELS];

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __user_task0_stack_top: u8;
    static __user_task1_stack_top: u8;
    static __user_task2_stack_top: u8;

    fn enter_task(frame: *const TrapFrame, kernel_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_kernel() -> ! {
    clear_bss();
    trap::init_trap_vector();
    configure_pmp();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB5 kernel task2 non-RR scheduler experiment");
    println!(
        "[kernel] timer source: mtime={:#x}, mtimecmp={:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIMECMP_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] policies: phase0={}, phase1={}",
        PHASE_DEFS[0].policy.as_str(),
        PHASE_DEFS[1].policy.as_str()
    );
    println!(
        "[kernel] trace control: enabled={}, limit={} record(s) per phase",
        ENABLE_SCHED_TRACE,
        SCHED_TRACE_LIMIT
    );
    for index in 0..TASK_COUNT {
        println!(
            "[kernel] task_def[{}]: pid={} name={} class={} priority={} role={}",
            index,
            TASK_DEFS[index].id,
            TASK_DEFS[index].name,
            TASK_DEFS[index].class.as_str(),
            TASK_DEFS[index].priority,
            task_role(index)
        );
    }

    enable_timer_interrupts();
    start_phase(0)
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
    let trap_tick = read_mtime();

    charge_runtime(current, trap_tick);
    save_current_frame(current, frame);
    unsafe {
        TASKS[current].explicit_yields += 1;
    }

    if let Some(next) = choose_next_task(current, SwitchReason::ExplicitYield) {
        if next != current {
            unsafe {
                EXPLICIT_YIELD_SWITCHES += 1;
            }
            switch_to(frame, current, next, SwitchReason::ExplicitYield, trap_tick);
            return;
        }
    }

    resume_same_task(current, trap_tick);
}

fn handle_timer_interrupt(frame: &mut TrapFrame) {
    let trap_tick = read_mtime();
    arm_next_timer_interrupt_from(trap_tick);

    unsafe {
        TIMER_INTERRUPT_COUNT += 1;
    }

    let current = current_task();
    charge_runtime(current, trap_tick);
    save_current_frame(current, frame);

    if let Some(next) = choose_next_task(current, SwitchReason::TimeSlice) {
        if next != current {
            unsafe {
                TIME_SLICE_SWITCHES += 1;
                TASKS[current].time_slice_preemptions += 1;
            }
            switch_to(frame, current, next, SwitchReason::TimeSlice, trap_tick);
            return;
        }
    }

    resume_same_task(current, trap_tick);
}

fn handle_finish(code: u64, frame: &mut TrapFrame) {
    let current = current_task();
    let trap_tick = read_mtime();

    charge_runtime(current, trap_tick);
    save_current_frame(current, frame);

    unsafe {
        TASKS[current].state = TaskState::Finished;
        TASKS[current].exit_code = code;
        TASKS[current].finish_tick = trap_tick.wrapping_sub(PHASE_START_TICK);
        FINISHED_TASKS += 1;
        TASKS[current].finish_order = FINISHED_TASKS;
    }

    println!(
        "[kernel][{}] process exit: pid={} name={} finish_tick={} runtime_ticks={} code={:#x}",
        current_phase_name(),
        TASK_DEFS[current].id,
        TASK_DEFS[current].name,
        unsafe { TASKS[current].finish_tick },
        unsafe { TASKS[current].runtime_ticks },
        code
    );

    if let Some(next) = choose_next_task(current, SwitchReason::TaskExit) {
        unsafe {
            TASK_EXIT_SWITCHES += 1;
        }
        switch_to(frame, current, next, SwitchReason::TaskExit, trap_tick);
    } else {
        complete_phase_or_finish()
    }
}

fn switch_to(frame: &mut TrapFrame, from: usize, to: usize, reason: SwitchReason, now: u64) {
    let switch_index = unsafe {
        TOTAL_SWITCHES += 1;
        TOTAL_SWITCHES
    };

    emit_switch_trace(
        switch_index,
        current_policy(),
        reason,
        from,
        to,
        unsafe { TASKS[from].runtime_ticks },
        unsafe { TASKS[to].runtime_ticks },
    );

    unsafe {
        CURRENT_TASK = to;
        TASKS[to].switch_ins += 1;
        TASKS[to].last_resume_tick = now;
        if current_policy() == SchedulerPolicy::StaticPriority {
            PRIORITY_LAST_PICK[TASK_DEFS[to].priority] = to;
        }
        *frame = TASKS[to].frame;
    }
}

fn resume_same_task(task_index: usize, now: u64) {
    unsafe {
        TASKS[task_index].last_resume_tick = now;
    }
}

fn emit_switch_trace(
    switch_index: u64,
    policy: SchedulerPolicy,
    reason: SwitchReason,
    from: usize,
    to: usize,
    from_runtime: u64,
    to_runtime: u64,
) {
    if !ENABLE_SCHED_TRACE {
        return;
    }

    let should_emit = unsafe { SWITCH_TRACE_EMITTED < SCHED_TRACE_LIMIT };
    if !should_emit {
        return;
    }

    println!(
        "[sched][{}] switch#{:02} reason={} from pid={}({}) runtime={} -> to pid={}({}) runtime={} priority={}->{}",
        policy.as_str(),
        switch_index,
        reason.as_str(),
        TASK_DEFS[from].id,
        TASK_DEFS[from].name,
        from_runtime,
        TASK_DEFS[to].id,
        TASK_DEFS[to].name,
        to_runtime,
        TASK_DEFS[from].priority,
        TASK_DEFS[to].priority
    );

    unsafe {
        SWITCH_TRACE_EMITTED += 1;
        if SWITCH_TRACE_EMITTED == SCHED_TRACE_LIMIT {
            println!(
                "[sched][{}] trace limit reached at {} record(s); further switches suppressed",
                policy.as_str(),
                SCHED_TRACE_LIMIT
            );
        }
    }
}

fn log_initial_restore(task_index: usize) {
    if !ENABLE_SCHED_TRACE {
        return;
    }

    println!(
        "[sched][{}] boot restore_begin: to pid={} name={} class={} priority={} reason={} next_mepc={:#x}",
        current_policy().as_str(),
        TASK_DEFS[task_index].id,
        TASK_DEFS[task_index].name,
        TASK_DEFS[task_index].class.as_str(),
        TASK_DEFS[task_index].priority,
        SwitchReason::Boot.as_str(),
        unsafe { TASKS[task_index].frame.mepc }
    );
}

fn start_phase(phase_index: usize) -> ! {
    let first_task;
    let now;

    unsafe {
        CURRENT_PHASE = phase_index;
    }
    reset_phase_state();
    initialize_tasks_for_phase();

    println!(
        "[kernel] phase begin: index={} name={} policy={}",
        phase_index,
        current_phase_name(),
        current_policy().as_str()
    );

    now = read_mtime();
    unsafe {
        PHASE_START_TICK = now;
    }
    arm_next_timer_interrupt_from(now);

    first_task = choose_initial_task();
    unsafe {
        CURRENT_TASK = first_task;
        TASKS[first_task].switch_ins = 1;
        TASKS[first_task].last_resume_tick = now;
        if current_policy() == SchedulerPolicy::StaticPriority {
            PRIORITY_LAST_PICK[TASK_DEFS[first_task].priority] = first_task;
        }
    }

    log_initial_restore(first_task);

    unsafe {
        enter_task(task_frame_ptr(first_task), kernel_stack_top());
    }
}

fn complete_phase_or_finish() -> ! {
    let phase_index = unsafe { CURRENT_PHASE };

    disable_timer_interrupts();
    disarm_timer_interrupt();
    save_phase_summary(phase_index);
    print_phase_summary(phase_index);

    if phase_index + 1 < PHASE_COUNT {
        println!(
            "[kernel] phase transition: {} -> {}",
            PHASE_DEFS[phase_index].name,
            PHASE_DEFS[phase_index + 1].name
        );
        enable_timer_interrupts();
        start_phase(phase_index + 1);
    }

    print_cross_phase_comparison_and_exit()
}

fn save_phase_summary(phase_index: usize) {
    let mut summary = PhaseSummary::empty();

    unsafe {
        summary.total_switches = TOTAL_SWITCHES;
        summary.explicit_yield_switches = EXPLICIT_YIELD_SWITCHES;
        summary.time_slice_switches = TIME_SLICE_SWITCHES;
        summary.task_exit_switches = TASK_EXIT_SWITCHES;
        summary.timer_interrupts = TIMER_INTERRUPT_COUNT;

        for index in 0..TASK_COUNT {
            summary.task_stats[index] = TaskPhaseStats {
                pid: TASK_DEFS[index].id,
                priority: TASK_DEFS[index].priority as u64,
                switch_ins: TASKS[index].switch_ins,
                runtime_ticks: TASKS[index].runtime_ticks,
                finish_tick: TASKS[index].finish_tick,
                finish_order: TASKS[index].finish_order,
                explicit_yields: TASKS[index].explicit_yields,
                time_slice_preemptions: TASKS[index].time_slice_preemptions,
                exit_code: TASKS[index].exit_code,
            };
        }

        PHASE_SUMMARIES[phase_index] = summary;
    }
}

fn print_phase_summary(phase_index: usize) {
    let summary = unsafe { PHASE_SUMMARIES[phase_index] };
    let phase = PHASE_DEFS[phase_index];

    println!(
        "[kernel][{}] summary: policy={} total_switches={} explicit_yield_switches={} time_slice_switches={} task_exit_switches={} timer_interrupts={}",
        phase.name,
        phase.policy.as_str(),
        summary.total_switches,
        summary.explicit_yield_switches,
        summary.time_slice_switches,
        summary.task_exit_switches,
        summary.timer_interrupts
    );

    for index in 0..TASK_COUNT {
        let stats = summary.task_stats[index];
        println!(
            "[kernel][{}] proc pid={} name={} class={} priority={} finish_order={} finish_tick={} runtime_ticks={} switch_ins={} explicit_yields={} time_slice_preemptions={} exit_code={:#x}",
            phase.name,
            stats.pid,
            TASK_DEFS[index].name,
            TASK_DEFS[index].class.as_str(),
            stats.priority,
            stats.finish_order,
            stats.finish_tick,
            stats.runtime_ticks,
            stats.switch_ins,
            stats.explicit_yields,
            stats.time_slice_preemptions,
            stats.exit_code
        );
    }
}

fn print_cross_phase_comparison_and_exit() -> ! {
    let rr = unsafe { PHASE_SUMMARIES[0] };
    let priority = unsafe { PHASE_SUMMARIES[1] };
    let rr_interactive = rr.task_stats[0];
    let prio_interactive = priority.task_stats[0];
    let rr_compute_long = rr.task_stats[2];
    let prio_compute_long = priority.task_stats[2];
    let rr_service_share_tenths =
        service_share_tenths(rr_interactive.runtime_ticks, rr_interactive.finish_tick);
    let prio_service_share_tenths =
        service_share_tenths(prio_interactive.runtime_ticks, prio_interactive.finish_tick);

    let high_priority_favored = prio_service_share_tenths > rr_service_share_tenths
        && prio_interactive.finish_tick < rr_interactive.finish_tick;
    let timing_diff_visible =
        prio_interactive.finish_tick.saturating_mul(10) < rr_interactive.finish_tick.saturating_mul(8);
    let low_priority_completed =
        priority.task_stats[1].finish_order > 0 && priority.task_stats[2].finish_order > 0;

    println!(
        "[kernel] comparison: interactive_finish_tick rr={} priority={} interactive_service_share={}%.{} rr_runtime={} priority_runtime={}",
        rr_interactive.finish_tick,
        prio_interactive.finish_tick,
        rr_service_share_tenths / 10,
        rr_service_share_tenths % 10,
        rr_interactive.runtime_ticks,
        prio_interactive.runtime_ticks
    );
    println!(
        "[kernel] comparison: interactive_service_share_priority={}%.{} priority_switch_ins={} rr_switch_ins={}",
        prio_service_share_tenths / 10,
        prio_service_share_tenths % 10,
        prio_interactive.switch_ins,
        rr_interactive.switch_ins
    );
    println!(
        "[kernel] comparison: compute_long_finish_tick rr={} priority={}",
        rr_compute_long.finish_tick,
        prio_compute_long.finish_tick
    );
    println!(
        "[kernel] acceptance high-priority interactive task favored under static_priority: {}",
        pass_fail(high_priority_favored)
    );
    println!(
        "[kernel] acceptance rr vs static_priority show clearly different completion timing: {}",
        pass_fail(timing_diff_visible)
    );
    println!(
        "[kernel] acceptance low-priority compute tasks still completed in finite run: {}",
        pass_fail(low_priority_completed)
    );

    qemu_exit(if high_priority_favored && timing_diff_visible && low_priority_completed {
        0
    } else {
        1
    })
}

fn service_share_tenths(runtime_ticks: u64, finish_tick: u64) -> u64 {
    if finish_tick == 0 {
        0
    } else {
        ((runtime_ticks as u128) * 1000 / (finish_tick as u128)) as u64
    }
}

fn reset_phase_state() {
    unsafe {
        CURRENT_TASK = NO_TASK;
        PHASE_START_TICK = 0;
        FINISHED_TASKS = 0;
        TIMER_INTERRUPT_COUNT = 0;
        EXPLICIT_YIELD_SWITCHES = 0;
        TIME_SLICE_SWITCHES = 0;
        TASK_EXIT_SWITCHES = 0;
        TOTAL_SWITCHES = 0;
        SWITCH_TRACE_EMITTED = 0;
        PRIORITY_LAST_PICK = [NO_TASK; PRIORITY_LEVELS];
    }
}

fn initialize_tasks_for_phase() {
    unsafe {
        for index in 0..TASK_COUNT {
            TASKS[index] = TaskControlBlock::empty();
            TASKS[index].state = TaskState::Runnable;
            TASKS[index].frame = build_initial_frame(index);
        }
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

fn choose_initial_task() -> usize {
    match current_policy() {
        SchedulerPolicy::RoundRobin => 0,
        SchedulerPolicy::StaticPriority => choose_initial_priority_task(),
    }
}

fn choose_initial_priority_task() -> usize {
    let mut best_task = 0usize;
    let mut best_priority = usize::MAX;

    for index in 0..TASK_COUNT {
        if TASK_DEFS[index].priority < best_priority {
            best_priority = TASK_DEFS[index].priority;
            best_task = index;
        }
    }

    best_task
}

fn choose_next_task(current: usize, reason: SwitchReason) -> Option<usize> {
    match current_policy() {
        SchedulerPolicy::RoundRobin => next_runnable_rr(current),
        SchedulerPolicy::StaticPriority => next_runnable_priority(current, reason),
    }
}

fn next_runnable_rr(current: usize) -> Option<usize> {
    for offset in 1..=TASK_COUNT {
        let candidate = (current + offset) % TASK_COUNT;
        if unsafe { TASKS[candidate].state == TaskState::Runnable } {
            return Some(candidate);
        }
    }

    None
}

fn next_runnable_priority(current: usize, reason: SwitchReason) -> Option<usize> {
    let exclude_current = matches!(reason, SwitchReason::ExplicitYield | SwitchReason::TaskExit);
    let mut best_priority = usize::MAX;

    for index in 0..TASK_COUNT {
        if exclude_current && index == current {
            continue;
        }
        if unsafe { TASKS[index].state != TaskState::Runnable } {
            continue;
        }
        if TASK_DEFS[index].priority < best_priority {
            best_priority = TASK_DEFS[index].priority;
        }
    }

    if best_priority == usize::MAX {
        return None;
    }

    let start = unsafe {
        let cursor = PRIORITY_LAST_PICK[best_priority];
        if cursor == NO_TASK {
            0
        } else {
            (cursor + 1) % TASK_COUNT
        }
    };

    for step in 0..TASK_COUNT {
        let candidate = (start + step) % TASK_COUNT;
        if exclude_current && candidate == current {
            continue;
        }
        if unsafe { TASKS[candidate].state != TaskState::Runnable } {
            continue;
        }
        if TASK_DEFS[candidate].priority == best_priority {
            return Some(candidate);
        }
    }

    None
}

fn charge_runtime(task_index: usize, now: u64) {
    unsafe {
        let last = TASKS[task_index].last_resume_tick;
        if last != 0 && now >= last {
            TASKS[task_index].runtime_ticks =
                TASKS[task_index].runtime_ticks.wrapping_add(now.wrapping_sub(last));
        }
        TASKS[task_index].last_resume_tick = 0;
    }
}

fn save_current_frame(current: usize, frame: &TrapFrame) {
    unsafe {
        TASKS[current].frame = *frame;
    }
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

fn current_phase_name() -> &'static str {
    unsafe { PHASE_DEFS[CURRENT_PHASE].name }
}

fn current_policy() -> SchedulerPolicy {
    unsafe { PHASE_DEFS[CURRENT_PHASE].policy }
}

#[no_mangle]
pub extern "C" fn interactive_proc_entry() -> ! {
    let mut round = 0usize;
    let mut acc = 0x1357_9bdf_2468_ace0u64;

    while round < INTERACTIVE_ROUNDS {
        acc = busy_mix(acc, INTERACTIVE_SPIN);
        syscall::yield_now();
        round += 1;
    }

    syscall::finish(acc)
}

#[no_mangle]
pub extern "C" fn compute_short_entry() -> ! {
    let acc = busy_mix(0x2222_3333_4444_5555u64, COMPUTE_SHORT_SPIN);
    syscall::finish(acc)
}

#[no_mangle]
pub extern "C" fn compute_long_entry() -> ! {
    let acc = busy_mix(0x7777_8888_9999_aaaau64, COMPUTE_LONG_SPIN);
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

fn task_role(task_index: usize) -> &'static str {
    match task_index {
        0 => "yield-heavy short-burst interactive workload",
        1 => "cpu-bound short compute workload",
        2 => "cpu-bound long compute workload",
        _ => "unknown",
    }
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
        2 => ptr::addr_of!(__user_task2_stack_top) as usize,
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

fn arm_next_timer_interrupt_from(now: u64) {
    let deadline = now.wrapping_add(TIME_SLICE_TICKS);
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
