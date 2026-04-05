#![no_std]
#![no_main]

mod console;
mod trap;

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const TASK_COUNT: usize = 2;

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIMECMP_OFFSET: usize = 0x4000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIMECMP_ADDR: usize = CLINT_BASE + CLINT_MTIMECMP_OFFSET;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;
const TIME_SLICE_TICKS: u64 = 12_000;

const QEMU_TEST_BASE: usize = 0x0010_0000;

const MIE_MTIE: usize = 1 << 7;
const MIP_SSIP: usize = 1 << 1;
const SIE_SSIE: usize = 1 << 1;
const INTERRUPT_BIT: usize = 1usize << (usize::BITS as usize - 1);
const MACHINE_TIMER_INTERRUPT: usize = INTERRUPT_BIT | 7;
const SUPERVISOR_SOFTWARE_INTERRUPT: usize = INTERRUPT_BIT | 1;

const TARGET_SWITCHES: u64 = 18;
const SWITCH_LOG_LIMIT: u64 = 10;

#[derive(Clone, Copy)]
struct TaskDefinition {
    id: u64,
    name: &'static str,
    role: &'static str,
    entry: extern "C" fn() -> !,
}

#[derive(Clone, Copy)]
struct KernelTask {
    frame: TrapFrame,
    switch_ins: u64,
    preemptions: u64,
}

impl KernelTask {
    const fn empty() -> Self {
        Self {
            frame: TrapFrame::zeroed(),
            switch_ins: 0,
            preemptions: 0,
        }
    }
}

static TASK_DEFS: [TaskDefinition; TASK_COUNT] = [
    TaskDefinition {
        id: 1,
        name: "recycler_daemon",
        role: "background_reclaimer",
        entry: recycler_daemon,
    },
    TaskDefinition {
        id: 2,
        name: "logger_daemon",
        role: "log_flusher",
        entry: logger_daemon,
    },
];

static mut TASKS: [KernelTask; TASK_COUNT] = [KernelTask::empty(); TASK_COUNT];
static mut CURRENT_TASK: usize = 0;
static mut MACHINE_TIMER_FORWARDS: u64 = 0;
static mut SUPERVISOR_IRQS: u64 = 0;
static mut PREEMPT_SWITCHES: u64 = 0;
static mut SWITCH_LOGGED: u64 = 0;

static TASK_STARTED: [AtomicBool; TASK_COUNT] = [AtomicBool::new(false), AtomicBool::new(false)];
static TASK_PROGRESS: [AtomicU64; TASK_COUNT] = [AtomicU64::new(0), AtomicU64::new(0)];
static LAST_MCAUSE: AtomicU64 = AtomicU64::new(0);
static LAST_SCAUSE: AtomicU64 = AtomicU64::new(0);
static LAST_SEPC: AtomicU64 = AtomicU64::new(0);

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __kernel_task0_stack_top: u8;
    static __kernel_task1_stack_top: u8;
    static __supervisor_trap_stack_top: u8;
    static __machine_trap_stack_top: u8;

    fn enter_supervisor(supervisor_entry: usize, supervisor_sp: usize) -> !;
    fn enter_kernel_task(frame: *const TrapFrame, trap_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_machine() -> ! {
    clear_bss();
    configure_pmp();
    trap::init_machine_trap_vector();
    delegate_supervisor_software_interrupt();
    arm_next_timer_interrupt();
    enable_machine_timer_interrupt();

    unsafe {
        asm!(
            "csrw mscratch, {}",
            in(reg) machine_trap_stack_top(),
            options(nostack, nomem)
        );
    }

    unsafe {
        enter_supervisor(start_supervisor as *const () as usize, kernel_stack_top())
    }
}

#[no_mangle]
pub extern "C" fn start_supervisor() -> ! {
    trap::init_supervisor_trap_vector();
    disable_supervisor_interrupts();
    clear_supervisor_software_pending();

    unsafe {
        asm!(
            "csrw sscratch, {}",
            in(reg) supervisor_trap_stack_top(),
            options(nostack, nomem)
        );
    }

    initialize_tasks();
    enable_supervisor_software_source();

    println!("[kernel] booted in S-mode");
    println!("[kernel] LAB3 kernel task5 kernel threads with preemptive switching");
    println!(
        "[kernel] timer source: mtime={:#x}, mtimecmp={:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIMECMP_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] scheduler: independent S-mode kernel-task round-robin queue, no U-mode tasks"
    );
    println!(
        "[kernel] time slice={} ticks ({} us), target_preempt_switches={}",
        TIME_SLICE_TICKS,
        ticks_to_us(TIME_SLICE_TICKS),
        TARGET_SWITCHES
    );

    for task_index in 0..TASK_COUNT {
        println!(
            "[kernel] task[{}]: id={} name={} role={} mode=S-only address_space=kernel-only u_stack=none kernel_stack_top={:#x}",
            task_index,
            TASK_DEFS[task_index].id,
            TASK_DEFS[task_index].name,
            TASK_DEFS[task_index].role,
            task_stack_top(task_index)
        );
    }

    unsafe {
        TASKS[0].switch_ins = 1;
        enter_kernel_task(task_frame_ptr(0), supervisor_trap_stack_top())
    }
}

#[no_mangle]
pub extern "C" fn handle_machine_trap(frame: &mut TrapFrame) {
    let mcause = read_mcause();
    LAST_MCAUSE.store(mcause as u64, Ordering::Relaxed);

    if mcause != MACHINE_TIMER_INTERRUPT {
        println!(
            "[kernel] unexpected machine trap: mcause={:#x} mepc={:#x}",
            mcause, frame.epc
        );
        qemu_exit(1);
    }

    arm_next_timer_interrupt();
    set_supervisor_software_pending();

    unsafe {
        MACHINE_TIMER_FORWARDS += 1;
    }
}

#[no_mangle]
pub extern "C" fn handle_supervisor_trap(frame: &mut TrapFrame) {
    let scause = read_scause();
    LAST_SCAUSE.store(scause as u64, Ordering::Relaxed);
    LAST_SEPC.store(frame.epc as u64, Ordering::Relaxed);

    if scause != SUPERVISOR_SOFTWARE_INTERRUPT {
        println!(
            "[kernel] unexpected supervisor trap: scause={:#x} sepc={:#x} stval={:#x}",
            scause,
            frame.epc,
            read_stval()
        );
        qemu_exit(1);
    }

    clear_supervisor_software_pending();

    let (current, next, saved_sepc, next_sepc, switch_index, should_finish) = unsafe {
        let current = CURRENT_TASK;
        TASKS[current].frame = *frame;
        TASKS[current].preemptions += 1;
        SUPERVISOR_IRQS += 1;

        let next = (current + 1) % TASK_COUNT;
        PREEMPT_SWITCHES += 1;
        let switch_index = PREEMPT_SWITCHES;
        let saved_sepc = TASKS[current].frame.epc;
        let next_sepc = TASKS[next].frame.epc;
        TASKS[next].switch_ins += 1;
        CURRENT_TASK = next;
        *frame = TASKS[next].frame;

        (
            current,
            next,
            saved_sepc,
            next_sepc,
            switch_index,
            PREEMPT_SWITCHES >= TARGET_SWITCHES,
        )
    };

    if switch_index <= SWITCH_LOG_LIMIT {
        println!(
            "[sched] switch#{:02} reason=timer_preempt from={}({}) saved_sepc={:#x} -> to={}({}) next_sepc={:#x}",
            switch_index,
            TASK_DEFS[current].id,
            TASK_DEFS[current].name,
            saved_sepc,
            TASK_DEFS[next].id,
            TASK_DEFS[next].name,
            next_sepc
        );
        unsafe {
            SWITCH_LOGGED = switch_index;
        }
    } else if unsafe { SWITCH_LOGGED } == SWITCH_LOG_LIMIT {
        println!(
            "[sched] switch logging capped after {} timer-preempted context switches",
            SWITCH_LOG_LIMIT
        );
        unsafe {
            SWITCH_LOGGED += 1;
        }
    }

    if should_finish {
        finish_experiment()
    }
}

fn initialize_tasks() {
    unsafe {
        TASKS = [KernelTask::empty(); TASK_COUNT];
        CURRENT_TASK = 0;
        MACHINE_TIMER_FORWARDS = 0;
        SUPERVISOR_IRQS = 0;
        PREEMPT_SWITCHES = 0;
        SWITCH_LOGGED = 0;

        for task_index in 0..TASK_COUNT {
            TASKS[task_index] = build_task(task_index);
        }
    }

    TASK_STARTED[0].store(false, Ordering::Relaxed);
    TASK_STARTED[1].store(false, Ordering::Relaxed);
    TASK_PROGRESS[0].store(0, Ordering::Relaxed);
    TASK_PROGRESS[1].store(0, Ordering::Relaxed);
}

fn build_task(task_index: usize) -> KernelTask {
    let mut frame = TrapFrame::zeroed();
    frame.ra = task_returned as *const () as usize;
    frame.gp = read_gp();
    frame.tp = read_tp();
    frame.saved_sp = task_stack_top(task_index);
    frame.epc = TASK_DEFS[task_index].entry as *const () as usize;

    KernelTask {
        frame,
        switch_ins: 0,
        preemptions: 0,
    }
}

fn finish_experiment() -> ! {
    disable_supervisor_interrupts();

    let started0 = TASK_STARTED[0].load(Ordering::Relaxed);
    let started1 = TASK_STARTED[1].load(Ordering::Relaxed);
    let progress0 = TASK_PROGRESS[0].load(Ordering::Relaxed);
    let progress1 = TASK_PROGRESS[1].load(Ordering::Relaxed);

    let independent_kernel_tasks_ok = started0 && started1 && progress0 > 0 && progress1 > 0;
    let timer_preempt_ok = unsafe { PREEMPT_SWITCHES > 0 && SUPERVISOR_IRQS > 0 };

    println!(
        "[kernel] summary: machine_timer_forwards={} supervisor_timer_irqs={} preempt_switches={}",
        unsafe { MACHINE_TIMER_FORWARDS },
        unsafe { SUPERVISOR_IRQS },
        unsafe { PREEMPT_SWITCHES }
    );

    for task_index in 0..TASK_COUNT {
        let progress = TASK_PROGRESS[task_index].load(Ordering::Relaxed);
        let started = TASK_STARTED[task_index].load(Ordering::Relaxed);
        let task = unsafe { TASKS[task_index] };
        println!(
            "[kernel] task_summary[{}]: started={} progress={} switch_ins={} preemptions={} kernel_stack_top={:#x}",
            TASK_DEFS[task_index].name,
            bool_word(started),
            progress,
            task.switch_ins,
            task.preemptions,
            task_stack_top(task_index)
        );
    }

    println!(
        "[kernel] diagnostics: last_mcause={:#x} last_scause={:#x} last_sepc={:#x}",
        LAST_MCAUSE.load(Ordering::Relaxed),
        LAST_SCAUSE.load(Ordering::Relaxed),
        LAST_SEPC.load(Ordering::Relaxed)
    );
    println!(
        "[kernel] acceptance kernel task exists in S-mode without U-stack: {}",
        pass_fail(independent_kernel_tasks_ok)
    );
    println!(
        "[kernel] acceptance timer interrupt suspended kernel task and scheduled another: {}",
        pass_fail(timer_preempt_ok)
    );

    qemu_exit(if independent_kernel_tasks_ok && timer_preempt_ok {
        0
    } else {
        1
    })
}

#[no_mangle]
pub extern "C" fn recycler_daemon() -> ! {
    TASK_STARTED[0].store(true, Ordering::Relaxed);
    println!(
        "[kthread] start id={} name={} mode=S-only stack_top={:#x}",
        TASK_DEFS[0].id,
        TASK_DEFS[0].name,
        task_stack_top(0)
    );

    let mut acc = 0x1234_5678_9abc_def0u64;
    let mut batch = 0u64;

    loop {
        let mut inner = 0u64;
        while inner < 18_000 {
            acc = acc
                .rotate_left(7)
                .wrapping_add(inner ^ 0x9e37_79b9)
                .wrapping_mul(0x5851_f42d_4c95_7f2d);
            inner += 1;
        }

        batch = batch.wrapping_add(1);
        TASK_PROGRESS[0].store(acc ^ batch, Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "C" fn logger_daemon() -> ! {
    TASK_STARTED[1].store(true, Ordering::Relaxed);
    println!(
        "[kthread] start id={} name={} mode=S-only stack_top={:#x}",
        TASK_DEFS[1].id,
        TASK_DEFS[1].name,
        task_stack_top(1)
    );

    let mut acc = 0x0fed_cba9_8765_4321u64;
    let mut batch = 0u64;

    loop {
        let mut inner = 0u64;
        while inner < 14_000 {
            acc = acc
                .rotate_left(11)
                .wrapping_add(0xd1b5_4a32 ^ inner)
                .wrapping_mul(0x2545_f491_4f6c_dd1d);
            inner += 1;
        }

        batch = batch.wrapping_add(1);
        TASK_PROGRESS[1].store(acc.wrapping_add(batch), Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "C" fn task_returned() -> ! {
    println!("[kernel] kernel task returned unexpectedly");
    qemu_exit(1);
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

fn delegate_supervisor_software_interrupt() {
    unsafe {
        asm!("csrw mideleg, {}", in(reg) MIP_SSIP, options(nostack, nomem));
    }
}

fn enable_machine_timer_interrupt() {
    unsafe {
        asm!("csrs mie, {}", in(reg) MIE_MTIE, options(nostack, nomem));
    }
}

fn enable_supervisor_software_source() {
    unsafe {
        asm!("csrs sie, {}", in(reg) SIE_SSIE, options(nostack, nomem));
    }
}

fn disable_supervisor_interrupts() {
    unsafe {
        asm!("csrc sstatus, {}", in(reg) 0x2usize, options(nostack, nomem));
    }
}

fn set_supervisor_software_pending() {
    unsafe {
        asm!("csrs mip, {}", in(reg) MIP_SSIP, options(nostack, nomem));
    }
}

fn clear_supervisor_software_pending() {
    unsafe {
        asm!("csrc sip, {}", in(reg) MIP_SSIP, options(nostack, nomem));
    }
}

fn arm_next_timer_interrupt() {
    let next = read_mtime().wrapping_add(TIME_SLICE_TICKS);

    unsafe {
        ptr::write_volatile(MTIMECMP_ADDR as *mut u64, next);
    }
}

fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
}

fn read_mcause() -> usize {
    let value: usize;

    unsafe {
        asm!("csrr {}, mcause", out(reg) value, options(nostack, nomem));
    }

    value
}

fn read_scause() -> usize {
    let value: usize;

    unsafe {
        asm!("csrr {}, scause", out(reg) value, options(nostack, nomem));
    }

    value
}

fn read_stval() -> usize {
    let value: usize;

    unsafe {
        asm!("csrr {}, stval", out(reg) value, options(nostack, nomem));
    }

    value
}

fn read_gp() -> usize {
    let value: usize;

    unsafe {
        asm!("mv {}, gp", out(reg) value, options(nostack, nomem, preserves_flags));
    }

    value
}

fn read_tp() -> usize {
    let value: usize;

    unsafe {
        asm!("mv {}, tp", out(reg) value, options(nostack, nomem, preserves_flags));
    }

    value
}

fn ticks_to_us(ticks: u64) -> u64 {
    ticks.saturating_mul(1_000_000) / MTIME_FREQ_HZ
}

fn bool_word(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn pass_fail(condition: bool) -> &'static str {
    if condition {
        "PASS"
    } else {
        "FAIL"
    }
}

fn task_frame_ptr(task_index: usize) -> *const TrapFrame {
    unsafe { &TASKS[task_index].frame as *const TrapFrame }
}

fn kernel_stack_top() -> usize {
    ptr::addr_of!(__kernel_stack_top) as usize
}

fn supervisor_trap_stack_top() -> usize {
    ptr::addr_of!(__supervisor_trap_stack_top) as usize
}

fn machine_trap_stack_top() -> usize {
    ptr::addr_of!(__machine_trap_stack_top) as usize
}

fn task_stack_top(task_index: usize) -> usize {
    match task_index {
        0 => ptr::addr_of!(__kernel_task0_stack_top) as usize,
        1 => ptr::addr_of!(__kernel_task1_stack_top) as usize,
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

pub fn qemu_exit(code: u32) -> ! {
    let value = if code == 0 {
        0x5555
    } else {
        (code << 16) | 0x3333
    };

    unsafe {
        ptr::write_volatile(QEMU_TEST_BASE as *mut u32, value);
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
