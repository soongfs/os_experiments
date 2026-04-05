#![no_std]
#![no_main]

mod console;
mod syscall;
mod trap;

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr;
use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const TASK_COUNT: usize = 2;
const WARMUP_ROUNDS: usize = 1;
const MEASURED_ROUNDS: usize = 5;
const TOTAL_ROUNDS: usize = WARMUP_ROUNDS + MEASURED_ROUNDS;
const OPS_PER_TASK: usize = 25_000;

const MODE_NOOP: usize = 0;
const MODE_YIELD: usize = 1;

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;

const USER_ENV_CALL: usize = 8;

pub const SYS_NOOP: usize = 0;
pub const SYS_YIELD: usize = 1;
pub const SYS_FINISH: usize = 2;
pub const ENOSYS: isize = -38;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Empty,
    Runnable,
    Finished,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Baseline { round: usize },
    Yield { round: usize },
}

#[derive(Clone, Copy)]
struct TaskControlBlock {
    state: TaskState,
    frame: TrapFrame,
}

impl TaskControlBlock {
    const fn empty() -> Self {
        Self {
            state: TaskState::Empty,
            frame: TrapFrame::zeroed(),
        }
    }
}

#[derive(Clone, Copy)]
struct RoundRecord {
    baseline_ticks: u64,
    baseline_ops: u64,
    yield_ticks: u64,
    yield_ops: u64,
    yield_switches: u64,
}

impl RoundRecord {
    const fn empty() -> Self {
        Self {
            baseline_ticks: 0,
            baseline_ops: 0,
            yield_ticks: 0,
            yield_ops: 0,
            yield_switches: 0,
        }
    }

    fn extra_ticks(self) -> u64 {
        self.yield_ticks.saturating_sub(self.baseline_ticks)
    }
}

#[derive(Clone, Copy)]
struct Summary {
    rounds: usize,
    total_baseline_ticks: u64,
    total_yield_ticks: u64,
    total_extra_ticks: u64,
    total_switches: u64,
    min_switch_ns: u64,
    max_switch_ns: u64,
}

impl Summary {
    const fn empty() -> Self {
        Self {
            rounds: 0,
            total_baseline_ticks: 0,
            total_yield_ticks: 0,
            total_extra_ticks: 0,
            total_switches: 0,
            min_switch_ns: 0,
            max_switch_ns: 0,
        }
    }

    fn avg_switch_ns(self) -> u64 {
        if self.total_switches == 0 {
            0
        } else {
            ticks_to_ns(self.total_extra_ticks) / self.total_switches
        }
    }
}

static mut TASKS: [TaskControlBlock; TASK_COUNT] = [TaskControlBlock::empty(); TASK_COUNT];
static mut ROUND_RECORDS: [RoundRecord; TOTAL_ROUNDS] = [RoundRecord::empty(); TOTAL_ROUNDS];
static mut CURRENT_PHASE: Phase = Phase::Baseline { round: 0 };
static mut CURRENT_TASK: usize = 0;
static mut PHASE_START_TICKS: u64 = 0;
static mut PHASE_HOT_OPS: u64 = 0;
static mut PHASE_SWITCHES: u64 = 0;

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
        ROUND_RECORDS = [RoundRecord::empty(); TOTAL_ROUNDS];
    }

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB3 task2 task-switch overhead estimator");
    println!(
        "[kernel] time source: mtime @ {:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] methodology: sequential noop-syscall baseline versus two-task ping-pong yield benchmark"
    );
    println!(
        "[kernel] rounds: {} warm-up + {} measured, ops/task/phase={}",
        WARMUP_ROUNDS,
        MEASURED_ROUNDS,
        OPS_PER_TASK
    );

    start_baseline_round(0);

    unsafe {
        enter_task(task_frame_ptr(CURRENT_TASK), kernel_stack_top());
    }
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

fn handle_syscall(frame: &mut TrapFrame) {
    match frame.a7 {
        SYS_NOOP => {
            unsafe {
                PHASE_HOT_OPS += 1;
            }
            frame.a0 = 0;
        }
        SYS_YIELD => handle_yield(frame),
        SYS_FINISH => handle_finish(frame),
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            frame.a0 = ENOSYS as usize;
        }
    }
}

fn handle_yield(frame: &mut TrapFrame) {
    unsafe {
        PHASE_HOT_OPS += 1;

        let current = CURRENT_TASK;
        TASKS[current].frame = *frame;

        let Some(next) = next_runnable_after(current) else {
            println!("[kernel] yield with no runnable peer");
            qemu_exit(1);
        };

        if next != current {
            PHASE_SWITCHES += 1;
        }

        CURRENT_TASK = next;
        *frame = TASKS[next].frame;
    }
}

fn handle_finish(frame: &mut TrapFrame) {
    unsafe {
        TASKS[CURRENT_TASK].state = TaskState::Finished;
    }

    if let Some(next) = unsafe { next_runnable_after(CURRENT_TASK) } {
        unsafe {
            CURRENT_TASK = next;
            *frame = TASKS[next].frame;
        }
        return;
    }

    finish_phase(frame);
}

fn finish_phase(frame: &mut TrapFrame) {
    let end_ticks = read_mtime();

    let (phase, hot_ops, switches, elapsed_ticks) = unsafe {
        (
            CURRENT_PHASE,
            PHASE_HOT_OPS,
            PHASE_SWITCHES,
            end_ticks.saturating_sub(PHASE_START_TICKS),
        )
    };

    match phase {
        Phase::Baseline { round } => {
            unsafe {
                ROUND_RECORDS[round].baseline_ticks = elapsed_ticks;
                ROUND_RECORDS[round].baseline_ops = hot_ops;
            }

            println!(
                "[kernel] {} baseline done: elapsed={} ticks ({} us), hot_ops={}, avg={} ns/op",
                round_label(round),
                elapsed_ticks,
                ticks_to_us(elapsed_ticks),
                hot_ops,
                average_ns(elapsed_ticks, hot_ops)
            );

            start_yield_round(round);

            unsafe {
                *frame = TASKS[CURRENT_TASK].frame;
            }
        }
        Phase::Yield { round } => {
            unsafe {
                ROUND_RECORDS[round].yield_ticks = elapsed_ticks;
                ROUND_RECORDS[round].yield_ops = hot_ops;
                ROUND_RECORDS[round].yield_switches = switches;
            }

            let record = unsafe { ROUND_RECORDS[round] };
            let extra_ticks = record.extra_ticks();
            let per_switch_ns = average_ns(extra_ticks, record.yield_switches);

            println!(
                "[kernel] {} yield done: elapsed={} ticks ({} us), hot_ops={}, switches={}, extra={} ticks ({} us), switch_estimate={} ns ({}.{:03} us)",
                round_label(round),
                elapsed_ticks,
                ticks_to_us(elapsed_ticks),
                hot_ops,
                switches,
                extra_ticks,
                ticks_to_us(extra_ticks),
                per_switch_ns,
                per_switch_ns / 1_000,
                per_switch_ns % 1_000
            );

            if round + 1 < TOTAL_ROUNDS {
                start_baseline_round(round + 1);

                unsafe {
                    *frame = TASKS[CURRENT_TASK].frame;
                }
            } else {
                finish_experiment();
            }
        }
    }
}

fn start_baseline_round(round: usize) {
    unsafe {
        CURRENT_PHASE = Phase::Baseline { round };
    }
    start_phase(round, MODE_NOOP);
    println!(
        "[kernel] {} starting sequential noop baseline",
        round_label(round)
    );
}

fn start_yield_round(round: usize) {
    unsafe {
        CURRENT_PHASE = Phase::Yield { round };
    }
    start_phase(round, MODE_YIELD);
    println!(
        "[kernel] {} starting ping-pong yield benchmark",
        round_label(round)
    );
}

fn start_phase(round: usize, mode: usize) {
    unsafe {
        TASKS = [TaskControlBlock::empty(); TASK_COUNT];
        CURRENT_TASK = 0;
        PHASE_HOT_OPS = 0;
        PHASE_SWITCHES = 0;

        for slot in 0..TASK_COUNT {
            TASKS[slot] = build_task(slot, mode, OPS_PER_TASK);
        }

        PHASE_START_TICKS = read_mtime();
    }

    let _ = round;
}

#[no_mangle]
pub extern "C" fn user_task_entry(_task_slot: usize, mode: usize, iterations: usize) -> ! {
    let mut remaining = iterations;

    while remaining != 0 {
        match mode {
            MODE_NOOP => syscall::noop(),
            MODE_YIELD => syscall::yield_now(),
            _ => break,
        }

        remaining -= 1;
    }

    syscall::finish()
}

fn build_task(slot: usize, mode: usize, iterations: usize) -> TaskControlBlock {
    let mut frame = TrapFrame::zeroed();
    frame.gp = read_gp();
    frame.tp = read_tp();
    frame.user_sp = user_stack_top(slot);
    frame.mepc = user_task_entry as *const () as usize;
    frame.a0 = slot;
    frame.a1 = mode;
    frame.a2 = iterations;

    TaskControlBlock {
        state: TaskState::Runnable,
        frame,
    }
}

fn next_runnable_after(current: usize) -> Option<usize> {
    unsafe {
        for offset in 1..=TASK_COUNT {
            let candidate = (current + offset) % TASK_COUNT;
            if TASKS[candidate].state == TaskState::Runnable {
                return Some(candidate);
            }
        }
    }

    None
}

fn finish_experiment() -> ! {
    let mut summary = Summary::empty();
    let mut positive_estimate = true;
    let mut switch_count_ok = true;
    let expected_ops = (TASK_COUNT * OPS_PER_TASK) as u64;
    let mut switch_ns_samples = [0u64; MEASURED_ROUNDS];

    println!("[kernel] measured-round summary begins");

    for round in WARMUP_ROUNDS..TOTAL_ROUNDS {
        let record = unsafe { ROUND_RECORDS[round] };
        let extra_ticks = record.extra_ticks();
        let per_switch_ns = average_ns(extra_ticks, record.yield_switches);
        let sample_index = round - WARMUP_ROUNDS;

        positive_estimate &= extra_ticks > 0;
        switch_count_ok &= record.baseline_ops == expected_ops
            && record.yield_ops == expected_ops
            && record.yield_switches == expected_ops;
        switch_ns_samples[sample_index] = per_switch_ns;

        println!(
            "[kernel] {}: baseline={} us, yield={} us, extra={} us, switches={}, switch_estimate={} ns ({}.{:03} us)",
            round_label(round),
            ticks_to_us(record.baseline_ticks),
            ticks_to_us(record.yield_ticks),
            ticks_to_us(extra_ticks),
            record.yield_switches,
            per_switch_ns,
            per_switch_ns / 1_000,
            per_switch_ns % 1_000
        );

        summary.rounds += 1;
        summary.total_baseline_ticks += record.baseline_ticks;
        summary.total_yield_ticks += record.yield_ticks;
        summary.total_extra_ticks += extra_ticks;
        summary.total_switches += record.yield_switches;

        if summary.min_switch_ns == 0 || per_switch_ns < summary.min_switch_ns {
            summary.min_switch_ns = per_switch_ns;
        }
        if per_switch_ns > summary.max_switch_ns {
            summary.max_switch_ns = per_switch_ns;
        }
    }

    let avg_switch_ns = summary.avg_switch_ns();
    sort_u64_slice(&mut switch_ns_samples);
    let median_switch_ns = switch_ns_samples[MEASURED_ROUNDS / 2];

    println!(
        "[kernel] average baseline={} us, average yield={} us, average extra={} us",
        ticks_to_us(summary.total_baseline_ticks / summary.rounds as u64),
        ticks_to_us(summary.total_yield_ticks / summary.rounds as u64),
        ticks_to_us(summary.total_extra_ticks / summary.rounds as u64)
    );
    println!(
        "[kernel] robust median single task-switch overhead = {} ns ({}.{:03} us)",
        median_switch_ns,
        median_switch_ns / 1_000,
        median_switch_ns % 1_000
    );
    println!(
        "[kernel] arithmetic mean switch overhead = {} ns ({}.{:03} us), min={} ns, max={} ns",
        avg_switch_ns,
        avg_switch_ns / 1_000,
        avg_switch_ns % 1_000,
        summary.min_switch_ns,
        summary.max_switch_ns
    );
    println!(
        "[kernel] formula: (yield_total - baseline_total) / actual_switches"
    );
    println!(
        "[kernel] acceptance explicit per-switch estimate available: {}",
        pass_fail(median_switch_ns > 0)
    );
    println!(
        "[kernel] acceptance operation counts and switch counts are consistent: {}",
        pass_fail(switch_count_ok)
    );
    println!(
        "[kernel] acceptance measured extra cost stayed positive in every measured round: {}",
        pass_fail(positive_estimate)
    );

    qemu_exit(if median_switch_ns > 0 && switch_count_ok && positive_estimate {
        0
    } else {
        1
    })
}

fn task_frame_ptr(slot: usize) -> *const TrapFrame {
    unsafe { &TASKS[slot].frame as *const TrapFrame }
}

fn kernel_stack_top() -> usize {
    ptr::addr_of!(__kernel_stack_top) as usize
}

fn user_stack_top(slot: usize) -> usize {
    match slot {
        0 => ptr::addr_of!(__user_task0_stack_top) as usize,
        1 => ptr::addr_of!(__user_task1_stack_top) as usize,
        _ => qemu_exit(1),
    }
}

fn round_label(round: usize) -> &'static str {
    match round {
        0 => "round#0(warm-up)",
        1 => "round#1",
        2 => "round#2",
        3 => "round#3",
        4 => "round#4",
        5 => "round#5",
        _ => "round#?",
    }
}

fn average_ns(ticks: u64, count: u64) -> u64 {
    if count == 0 {
        0
    } else {
        ticks_to_ns(ticks) / count
    }
}

fn ticks_to_ns(ticks: u64) -> u64 {
    ticks.saturating_mul(MTIME_TICK_NS)
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

fn sort_u64_slice(values: &mut [u64]) {
    let len = values.len();
    let mut i = 1;

    while i < len {
        let key = values[i];
        let mut j = i;

        while j > 0 && values[j - 1] > key {
            values[j] = values[j - 1];
            j -= 1;
        }

        values[j] = key;
        i += 1;
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
