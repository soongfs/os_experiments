#![no_std]
#![no_main]

mod console;
mod syscall;
mod trap;
mod user_console;

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr;
use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const DRAM_START: usize = 0x8000_0000;
const TASK_COUNT: usize = 2;
const FP_LANES: usize = 8;
const PHASE_REFERENCE: usize = 0;
const PHASE_PREEMPTIVE: usize = 1;
const TIMER_LOG_LIMIT: usize = 6;

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIMECMP_OFFSET: usize = 0x4000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIMECMP_ADDR: usize = CLINT_BASE + CLINT_MTIMECMP_OFFSET;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;
const TIME_SLICE_TICKS: u64 = 2_500;

const MIE_MTIE: usize = 1 << 7;
const INTERRUPT_BIT: usize = 1usize << (usize::BITS - 1);
const USER_ENV_CALL: usize = 8;
const MACHINE_TIMER_INTERRUPT: usize = INTERRUPT_BIT | 7;

pub const SYS_WRITE: usize = 0;
pub const SYS_FINISH: usize = 1;

pub const EFAULT: isize = -14;
pub const ENOSYS: isize = -38;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Empty,
    Runnable,
    Finished,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Reference(usize),
    Preemptive,
}

#[derive(Clone, Copy)]
struct TaskControlBlock {
    state: TaskState,
    frame: TrapFrame,
    last_checksum: u64,
}

impl TaskControlBlock {
    const fn empty() -> Self {
        Self {
            state: TaskState::Empty,
            frame: TrapFrame::zeroed(),
            last_checksum: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Checkpoint {
    reference: u64,
    concurrent: u64,
    have_reference: bool,
    have_concurrent: bool,
}

impl Checkpoint {
    const fn empty() -> Self {
        Self {
            reference: 0,
            concurrent: 0,
            have_reference: false,
            have_concurrent: false,
        }
    }
}

struct UserWorkload {
    name: &'static str,
    iterations: usize,
    init: [f64; FP_LANES],
    mul: [f64; FP_LANES],
    add: [f64; FP_LANES],
}

static USER_WORKLOADS: [UserWorkload; TASK_COUNT] = [
    UserWorkload {
        name: "fp_alpha",
        iterations: 220_000,
        init: [0.75, 1.125, 0.5, 1.875, 0.625, 1.375, 0.9375, 1.5625],
        mul: [
            0.999_991,
            0.999_989,
            0.999_987,
            0.999_985,
            0.999_983,
            0.999_981,
            0.999_979,
            0.999_977,
        ],
        add: [
            0.000_031_25,
            0.000_021_484_375,
            0.000_015_258_789_062_5,
            0.000_045_776_367_187_5,
            0.000_018_310_546_875,
            0.000_026_702_880_859_375,
            0.000_011_444_091_796_875,
            0.000_039_100_646_972_656_25,
        ],
    },
    UserWorkload {
        name: "fp_beta",
        iterations: 220_000,
        init: [1.28125, 0.84375, 1.65625, 0.71875, 1.46875, 0.59375, 1.09375, 0.96875],
        mul: [
            0.999_973,
            0.999_971,
            0.999_969,
            0.999_967,
            0.999_965,
            0.999_963,
            0.999_961,
            0.999_959,
        ],
        add: [
            0.000_028_610_229_492_187_5,
            0.000_019_073_486_328_125,
            0.000_041_961_669_921_875,
            0.000_013_351_440_429_687_5,
            0.000_034_332_275_390_625,
            0.000_017_166_137_695_312_5,
            0.000_022_888_183_593_75,
            0.000_030_517_578_125,
        ],
    },
];

static mut TASKS: [TaskControlBlock; TASK_COUNT] = [TaskControlBlock::empty(); TASK_COUNT];
static mut RESULTS: [Checkpoint; TASK_COUNT] = [Checkpoint::empty(); TASK_COUNT];
static mut CURRENT_PHASE: Phase = Phase::Reference(0);
static mut CURRENT_TASK: usize = 0;
static mut TIMER_INTERRUPT_COUNT: usize = 0;
static mut FORCED_SWITCH_COUNT: usize = 0;
static mut TIMER_LOGGED: usize = 0;

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __user_task0_stack_top: u8;
    static __user_task1_stack_top: u8;
    static __image_end: u8;

    fn enter_task(frame: *const TrapFrame, kernel_sp: usize) -> !;
    fn fp_stress_loop(init: *const f64, mul: *const f64, add: *const f64, iterations: usize)
        -> u64;
}

#[no_mangle]
pub extern "C" fn start_kernel() -> ! {
    clear_bss();
    trap::init_trap_vector();
    configure_pmp();

    unsafe {
        RESULTS = [Checkpoint::empty(); TASK_COUNT];
    }

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB3 task1 floating-point preemption verifier");
    println!(
        "[kernel] timer source: mtime={:#x}, mtimecmp={:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIMECMP_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] plan: run two single-task references, then rerun both under timer-driven preemption"
    );
    println!(
        "[kernel] concurrent time slice: {} ticks ({} us)",
        TIME_SLICE_TICKS,
        ticks_to_us(TIME_SLICE_TICKS)
    );

    start_reference_phase(0);

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
        SYS_WRITE => {
            frame.a0 = sys_write(frame.a0 as *const u8, frame.a1) as usize;
        }
        SYS_FINISH => handle_finish(frame, frame.a0 as u64),
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            frame.a0 = ENOSYS as usize;
        }
    }
}

fn handle_timer_interrupt(frame: &mut TrapFrame) {
    arm_next_timer_interrupt();

    unsafe {
        TIMER_INTERRUPT_COUNT += 1;

        let current = CURRENT_TASK;
        TASKS[current].frame = *frame;

        let Some(next) = next_runnable_after(current) else {
            println!("[kernel] timer interrupt with no runnable task");
            qemu_exit(1);
        };

        if next != current {
            FORCED_SWITCH_COUNT += 1;

            if TIMER_LOGGED < TIMER_LOG_LIMIT {
                let interrupt_count = TIMER_INTERRUPT_COUNT;

                println!(
                    "[kernel] timer interrupt #{}: preempt {} -> {} at mepc={:#x}",
                    interrupt_count,
                    USER_WORKLOADS[current].name,
                    USER_WORKLOADS[next].name,
                    frame.mepc
                );
                TIMER_LOGGED += 1;

                if TIMER_LOGGED == TIMER_LOG_LIMIT {
                    println!(
                        "[kernel] timer interrupt logging capped after {} events",
                        TIMER_LOG_LIMIT
                    );
                }
            }
        }

        CURRENT_TASK = next;
        *frame = TASKS[next].frame;
    }
}

fn handle_finish(frame: &mut TrapFrame, checksum: u64) {
    unsafe {
        let current = CURRENT_TASK;
        TASKS[current].state = TaskState::Finished;
        TASKS[current].last_checksum = checksum;

        match CURRENT_PHASE {
            Phase::Reference(slot) => {
                RESULTS[slot].reference = checksum;
                RESULTS[slot].have_reference = true;

                println!(
                    "[kernel] reference checksum [{}] = {:#018x}",
                    USER_WORKLOADS[slot].name,
                    checksum
                );

                if slot + 1 < TASK_COUNT {
                    start_reference_phase(slot + 1);
                    *frame = TASKS[CURRENT_TASK].frame;
                } else {
                    start_preemptive_phase();
                    *frame = TASKS[CURRENT_TASK].frame;
                }
            }
            Phase::Preemptive => {
                RESULTS[current].concurrent = checksum;
                RESULTS[current].have_concurrent = true;

                println!(
                    "[kernel] concurrent checksum [{}] = {:#018x}",
                    USER_WORKLOADS[current].name,
                    checksum
                );

                if let Some(next) = next_runnable_after(current) {
                    CURRENT_TASK = next;
                    arm_next_timer_interrupt();
                    *frame = TASKS[next].frame;
                } else {
                    finish_experiment();
                }
            }
        }
    }
}

fn start_reference_phase(slot: usize) {
    disable_timer_interrupts();
    disarm_timer_interrupt();

    unsafe {
        CURRENT_PHASE = Phase::Reference(slot);
        CURRENT_TASK = slot;
        TASKS = [TaskControlBlock::empty(); TASK_COUNT];
        TASKS[slot] = build_task(slot, PHASE_REFERENCE, 0);
    }

    println!(
        "[kernel] phase=reference task={} iterations={}",
        USER_WORKLOADS[slot].name,
        USER_WORKLOADS[slot].iterations
    );
}

fn start_preemptive_phase() {
    disable_timer_interrupts();
    disarm_timer_interrupt();

    unsafe {
        CURRENT_PHASE = Phase::Preemptive;
        CURRENT_TASK = 0;
        TASKS = [TaskControlBlock::empty(); TASK_COUNT];

        for slot in 0..TASK_COUNT {
            TASKS[slot] = build_task(slot, PHASE_PREEMPTIVE, RESULTS[slot].reference);
        }

        TIMER_INTERRUPT_COUNT = 0;
        FORCED_SWITCH_COUNT = 0;
        TIMER_LOGGED = 0;
    }

    println!("[kernel] phase=preemptive starting concurrent run");
    for slot in 0..TASK_COUNT {
        unsafe {
            println!(
                "[kernel] expected checksum [{}] = {:#018x}",
                USER_WORKLOADS[slot].name,
                RESULTS[slot].reference
            );
        }
    }

    enable_timer_interrupts();
    arm_next_timer_interrupt();
}

fn finish_experiment() -> ! {
    disable_timer_interrupts();
    disarm_timer_interrupt();

    let mut all_match = true;

    println!(
        "[kernel] summary: timer_interrupts={} forced_switches={}",
        unsafe { TIMER_INTERRUPT_COUNT },
        unsafe { FORCED_SWITCH_COUNT }
    );

    for slot in 0..TASK_COUNT {
        let result = unsafe { RESULTS[slot] };
        let matched = result.have_reference
            && result.have_concurrent
            && result.reference == result.concurrent;
        all_match &= matched;

        println!(
            "[kernel] result [{}]: expected={:#018x} observed={:#018x} => {}",
            USER_WORKLOADS[slot].name,
            result.reference,
            result.concurrent,
            pass_fail(matched)
        );
    }

    let interrupts_ok = unsafe { TIMER_INTERRUPT_COUNT > 0 };
    let switches_ok = unsafe { FORCED_SWITCH_COUNT > 0 };

    println!(
        "[kernel] acceptance forced timer interrupt occurred: {}",
        pass_fail(interrupts_ok)
    );
    println!(
        "[kernel] acceptance forced context switch occurred: {}",
        pass_fail(switches_ok)
    );
    println!(
        "[kernel] acceptance concurrent checksums match reference exactly: {}",
        pass_fail(all_match)
    );
    println!(
        "[kernel] criterion: any exact-bit checksum mismatch means FP register or fcsr state leaked across preemption"
    );

    qemu_exit(if interrupts_ok && switches_ok && all_match {
        0
    } else {
        1
    })
}

#[no_mangle]
pub extern "C" fn user_task_entry(task_slot: usize, phase: usize, expected_checksum: usize) -> ! {
    let workload = &USER_WORKLOADS[task_slot];
    let expected_checksum = expected_checksum as u64;

    uprintln!(
        "[user/{}] {} run start: iterations={}",
        workload.name,
        phase_label(phase),
        workload.iterations
    );

    let checksum = unsafe {
        fp_stress_loop(
            workload.init.as_ptr(),
            workload.mul.as_ptr(),
            workload.add.as_ptr(),
            workload.iterations,
        )
    };

    if phase == PHASE_REFERENCE {
        uprintln!(
            "[user/{}] reference checksum={:#018x}",
            workload.name,
            checksum
        );
    } else {
        let matched = checksum == expected_checksum;
        uprintln!(
            "[user/{}] preemptive checksum={:#018x}, expected={:#018x}, status={}",
            workload.name,
            checksum,
            expected_checksum,
            if matched { "match" } else { "mismatch" }
        );
    }

    syscall::finish(checksum)
}

fn build_task(slot: usize, phase: usize, expected_checksum: u64) -> TaskControlBlock {
    let mut frame = TrapFrame::zeroed();
    frame.gp = read_gp();
    frame.tp = read_tp();
    frame.user_sp = user_stack_top(slot);
    frame.mepc = user_task_entry as *const () as usize;
    frame.a0 = slot;
    frame.a1 = phase;
    frame.a2 = expected_checksum as usize;

    TaskControlBlock {
        state: TaskState::Runnable,
        frame,
        last_checksum: 0,
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

fn phase_label(phase: usize) -> &'static str {
    match phase {
        PHASE_REFERENCE => "reference",
        PHASE_PREEMPTIVE => "preemptive",
        _ => "unknown",
    }
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

fn validated_user_bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Ok(&[]);
    }

    let addr = ptr as usize;
    let end = match addr.checked_add(len) {
        Some(end) => end,
        None => return Err(EFAULT),
    };

    if addr == 0 || addr < DRAM_START || end > user_memory_end() {
        return Err(EFAULT);
    }

    Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
}

fn user_memory_end() -> usize {
    ptr::addr_of!(__image_end) as usize
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
