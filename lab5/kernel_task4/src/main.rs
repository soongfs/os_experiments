#![no_std]
#![no_main]

mod console;
mod spinlock;

use core::arch::{asm, global_asm};
use core::hint::{black_box, spin_loop};
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use spinlock::SpinLock;

global_asm!(include_str!("boot.S"));

const EXPECTED_HARTS: usize = 4;
const NO_HART: usize = usize::MAX;

const BOOT_STACK_PER_HART: usize = 16 * 1024;
const INTERRUPT_STACK_PER_HART: usize = 8 * 1024;

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIMECMP_OFFSET: usize = 0x4000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;

const TIMER_INTERVAL_TICKS: u64 = 2_500;
const TARGET_TIMER_IRQS: u64 = 10;
const TARGET_IPI_RECEIVED: u64 = 4;
const TARGET_WORK_UNITS: u64 = 12;
const IPI_SEND_PERIOD: u64 = 2;
const IPI_SEND_LIMIT: u64 = 6;

const CRITICAL_HOLD_TICKS: u64 = 4_500;
const NORMAL_WORK_ITERS: u64 = 8_000;

const MIE_MSIE: usize = 1 << 3;
const MIE_MTIE: usize = 1 << 7;
const MSTATUS_MIE: usize = 1 << 3;
const MSTATUS_FS_DIRTY: usize = 0x6000;
const INTERRUPT_BIT: usize = 1usize << (usize::BITS as usize - 1);
const MACHINE_SOFTWARE_INTERRUPT: usize = INTERRUPT_BIT | 3;
const MACHINE_TIMER_INTERRUPT: usize = INTERRUPT_BIT | 7;

const QEMU_TEST_BASE: usize = 0x0010_0000;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TrapFrame {
    pub ra: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
    pub saved_sp: usize,
    pub mepc: usize,
    pub f: [u64; 32],
    pub fcsr: usize,
    pub reserved: usize,
}

#[derive(Clone, Copy)]
struct SharedKernelState {
    critical_sections: u64,
    shared_checksum: u64,
    last_owner: u64,
}

impl SharedKernelState {
    const fn empty() -> Self {
        Self {
            critical_sections: 0,
            shared_checksum: 0,
            last_owner: 0,
        }
    }
}

static SHARED_STATE: SpinLock<SharedKernelState> = SpinLock::new(SharedKernelState::empty());

static READY_HARTS: AtomicUsize = AtomicUsize::new(0);
static BOOTED_MASK: AtomicUsize = AtomicUsize::new(0);
static START_WORK: AtomicBool = AtomicBool::new(false);
static FINISHED_HARTS: AtomicUsize = AtomicUsize::new(0);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

static HART_INIT_DONE: [AtomicBool; EXPECTED_HARTS] =
    [const { AtomicBool::new(false) }; EXPECTED_HARTS];
static WORK_UNITS_DONE: [AtomicU64; EXPECTED_HARTS] =
    [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static TIMER_IRQS: [AtomicU64; EXPECTED_HARTS] = [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static IPI_IRQS: [AtomicU64; EXPECTED_HARTS] = [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static IPI_SENT: [AtomicU64; EXPECTED_HARTS] = [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static INTERRUPTS_WHILE_LOCK_HELD: [AtomicU64; EXPECTED_HARTS] =
    [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static LAST_MEPC: [AtomicU64; EXPECTED_HARTS] = [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static HART_IN_CRITICAL: [AtomicBool; EXPECTED_HARTS] =
    [const { AtomicBool::new(false) }; EXPECTED_HARTS];
static MIN_INTERRUPT_SP: [AtomicUsize; EXPECTED_HARTS] =
    [const { AtomicUsize::new(usize::MAX) }; EXPECTED_HARTS];
static LAST_INTERRUPT_SP: [AtomicUsize; EXPECTED_HARTS] =
    [const { AtomicUsize::new(0) }; EXPECTED_HARTS];

static ACTIVE_LOCK_HOLDER: AtomicUsize = AtomicUsize::new(NO_HART);
static LOCK_STATE_VIOLATIONS: AtomicU64 = AtomicU64::new(0);

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __interrupt_stack_top: u8;
    static mut __boot_release_flag: u64;
    fn trap_entry();
}

#[no_mangle]
pub extern "C" fn start_primary() -> ! {
    clear_bss();
    initialize_global_state();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB5 kernel task4 kernel-context interrupt handling on SMP");
    println!(
        "[kernel] environment: harts={} boot_stack_per_hart={} interrupt_stack_per_hart={}",
        EXPECTED_HARTS,
        BOOT_STACK_PER_HART,
        INTERRUPT_STACK_PER_HART
    );
    println!(
        "[kernel] interrupt policy: timer=MTIP ipi=MSIP, handlers run on dedicated interrupt stacks and never take the shared spinlock"
    );
    println!(
        "[kernel] timer interval={} ticks ({} us), critical_hold={} ticks ({} us), target_timer_irqs_per_hart={}, target_ipi_receives_per_hart={}",
        TIMER_INTERVAL_TICKS,
        ticks_to_us(TIMER_INTERVAL_TICKS),
        CRITICAL_HOLD_TICKS,
        ticks_to_us(CRITICAL_HOLD_TICKS),
        TARGET_TIMER_IRQS,
        TARGET_IPI_RECEIVED
    );

    configure_pmp();
    release_secondary_harts();
    hart_main(0, true)
}

#[no_mangle]
pub extern "C" fn start_secondary(hart_id: usize) -> ! {
    if hart_id >= EXPECTED_HARTS {
        loop {
            spin_loop();
        }
    }

    configure_pmp();
    hart_main(hart_id, false)
}

fn hart_main(hart_id: usize, is_primary: bool) -> ! {
    init_trap_vector();
    install_interrupt_stack(hart_id);
    clear_msip(hart_id);
    enable_fp_context();

    let ready = mark_hart_initialized(hart_id);
    println!(
        "[hart{}] init complete: role={} normal_sp={:#x} interrupt_stack=[{:#x}, {:#x}) ready_harts={}/{}",
        hart_id,
        if is_primary { "primary" } else { "secondary" },
        read_sp(),
        interrupt_stack_bottom(hart_id),
        interrupt_stack_top(hart_id),
        ready,
        EXPECTED_HARTS
    );

    if is_primary {
        while READY_HARTS.load(Ordering::Acquire) < EXPECTED_HARTS {
            spin_loop();
        }
        START_WORK.store(true, Ordering::Release);
        println!(
            "[hart{}] start barrier released: ready_harts={} booted_mask={:#x}",
            hart_id,
            READY_HARTS.load(Ordering::Acquire),
            BOOTED_MASK.load(Ordering::Acquire)
        );
    } else {
        while !START_WORK.load(Ordering::Acquire) {
            spin_loop();
        }
        println!("[hart{}] start barrier observed", hart_id);
    }

    arm_next_timer_interrupt(hart_id, read_mtime());
    enable_machine_interrupts();
    worker_loop(hart_id)
}

fn worker_loop(hart_id: usize) -> ! {
    let mut work_units = 0u64;
    let mut local = 0x1234_5678_9abc_def0u64 ^ ((hart_id as u64) << 40);

    while !local_done(hart_id, work_units) {
        with_shared_state_lock(hart_id, |state| {
            state.critical_sections += 1;
            state.last_owner = hart_id as u64;
            local = busy_mix(local ^ state.shared_checksum ^ state.critical_sections, 512);
            hold_lock_until(read_mtime().wrapping_add(CRITICAL_HOLD_TICKS));
            state.shared_checksum ^= local.rotate_left((hart_id as u32) + 1);
        });

        local = busy_mix(local ^ work_units, NORMAL_WORK_ITERS);
        work_units += 1;
        WORK_UNITS_DONE[hart_id].store(work_units, Ordering::Release);

        if work_units % 6 == 0 {
            println!(
                "[hart{}] progress: work_units={} timer_irqs={} ipi_irqs={} interrupts_while_lock_held={}",
                hart_id,
                work_units,
                TIMER_IRQS[hart_id].load(Ordering::Acquire),
                IPI_IRQS[hart_id].load(Ordering::Acquire),
                INTERRUPTS_WHILE_LOCK_HELD[hart_id].load(Ordering::Acquire)
            );
        }
    }

    disable_machine_interrupts();

    let finished = FINISHED_HARTS.fetch_add(1, Ordering::AcqRel) + 1;
    println!(
        "[hart{}] worker complete: work_units={} timer_irqs={} ipi_irqs={} interrupts_while_lock_held={} finished_harts={}/{}",
        hart_id,
        work_units,
        TIMER_IRQS[hart_id].load(Ordering::Acquire),
        IPI_IRQS[hart_id].load(Ordering::Acquire),
        INTERRUPTS_WHILE_LOCK_HELD[hart_id].load(Ordering::Acquire),
        finished,
        EXPECTED_HARTS
    );

    if finished == EXPECTED_HARTS {
        SHUTDOWN.store(true, Ordering::Release);
        print_summary_and_exit()
    }

    while !SHUTDOWN.load(Ordering::Acquire) {
        spin_loop();
    }

    loop {
        spin_loop();
    }
}

fn local_done(hart_id: usize, work_units: u64) -> bool {
    work_units >= TARGET_WORK_UNITS
        && TIMER_IRQS[hart_id].load(Ordering::Acquire) >= TARGET_TIMER_IRQS
        && IPI_IRQS[hart_id].load(Ordering::Acquire) >= TARGET_IPI_RECEIVED
}

fn with_shared_state_lock<F>(hart_id: usize, f: F)
where
    F: FnOnce(&mut SharedKernelState),
{
    let mut state = SHARED_STATE.lock();
    let previous_holder = ACTIVE_LOCK_HOLDER.swap(hart_id, Ordering::AcqRel);
    if previous_holder != NO_HART {
        LOCK_STATE_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    }

    HART_IN_CRITICAL[hart_id].store(true, Ordering::Release);
    f(&mut state);
    HART_IN_CRITICAL[hart_id].store(false, Ordering::Release);

    let observed_holder = ACTIVE_LOCK_HOLDER.swap(NO_HART, Ordering::AcqRel);
    if observed_holder != hart_id {
        LOCK_STATE_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

fn print_summary_and_exit() -> ! {
    let shared = {
        let state = SHARED_STATE.lock();
        *state
    };

    println!(
        "[kernel] summary: ready_harts={} booted_mask={:#x} finished_harts={} shared_critical_sections={} shared_checksum={:#x}",
        READY_HARTS.load(Ordering::Acquire),
        BOOTED_MASK.load(Ordering::Acquire),
        FINISHED_HARTS.load(Ordering::Acquire),
        shared.critical_sections,
        shared.shared_checksum
    );
    println!(
        "[kernel] shared_lock: acquisitions={} contention_spins={} lock_state_violations={}",
        SHARED_STATE.acquisitions(),
        SHARED_STATE.contention_spins(),
        LOCK_STATE_VIOLATIONS.load(Ordering::Acquire)
    );

    let mut interrupt_stack_safe = true;
    let mut delivered_on_all_harts = true;
    let mut lock_boundary_safe = LOCK_STATE_VIOLATIONS.load(Ordering::Acquire) == 0;
    let mut lock_edge_observed = false;

    for hart_id in 0..EXPECTED_HARTS {
        let timer_irqs = TIMER_IRQS[hart_id].load(Ordering::Acquire);
        let ipi_irqs = IPI_IRQS[hart_id].load(Ordering::Acquire);
        let ipi_sent = IPI_SENT[hart_id].load(Ordering::Acquire);
        let interrupts_while_locked =
            INTERRUPTS_WHILE_LOCK_HELD[hart_id].load(Ordering::Acquire);
        let min_sp = MIN_INTERRUPT_SP[hart_id].load(Ordering::Acquire);
        let last_sp = LAST_INTERRUPT_SP[hart_id].load(Ordering::Acquire);
        let stack_bottom = interrupt_stack_bottom(hart_id);
        let stack_top = interrupt_stack_top(hart_id);

        delivered_on_all_harts &= timer_irqs >= TARGET_TIMER_IRQS && ipi_irqs >= TARGET_IPI_RECEIVED;
        lock_edge_observed |= interrupts_while_locked > 0;
        interrupt_stack_safe &= min_sp >= stack_bottom && min_sp < stack_top;
        lock_boundary_safe &= !HART_IN_CRITICAL[hart_id].load(Ordering::Acquire);

        println!(
            "[kernel] hart[{}]: init_done={} work_units={} timer_irqs={} ipi_sent={} ipi_irqs={} interrupts_while_lock_held={} last_mepc={:#x}",
            hart_id,
            bool_to_u64(HART_INIT_DONE[hart_id].load(Ordering::Acquire)),
            WORK_UNITS_DONE[hart_id].load(Ordering::Acquire),
            timer_irqs,
            ipi_sent,
            ipi_irqs,
            interrupts_while_locked,
            LAST_MEPC[hart_id].load(Ordering::Acquire)
        );
        println!(
            "[kernel] hart[{}]: interrupt_stack=[{:#x}, {:#x}) min_sp={:#x} last_sp={:#x}",
            hart_id,
            stack_bottom,
            stack_top,
            min_sp,
            last_sp
        );
    }

    let interrupts_stable = READY_HARTS.load(Ordering::Acquire) == EXPECTED_HARTS
        && FINISHED_HARTS.load(Ordering::Acquire) == EXPECTED_HARTS
        && delivered_on_all_harts;
    let stack_and_lock_safe = interrupt_stack_safe && lock_boundary_safe && lock_edge_observed;

    println!(
        "[kernel] acceptance each hart handled timer interrupts and IPIs: {}",
        pass_fail(interrupts_stable)
    );
    println!(
        "[kernel] acceptance interrupt handlers preserved dedicated stacks and lock state: {}",
        pass_fail(stack_and_lock_safe)
    );

    qemu_exit(if interrupts_stable && stack_and_lock_safe {
        0
    } else {
        1
    })
}

fn initialize_global_state() {
    READY_HARTS.store(0, Ordering::Relaxed);
    BOOTED_MASK.store(0, Ordering::Relaxed);
    START_WORK.store(false, Ordering::Relaxed);
    FINISHED_HARTS.store(0, Ordering::Relaxed);
    SHUTDOWN.store(false, Ordering::Relaxed);
    ACTIVE_LOCK_HOLDER.store(NO_HART, Ordering::Relaxed);
    LOCK_STATE_VIOLATIONS.store(0, Ordering::Relaxed);

    for hart_id in 0..EXPECTED_HARTS {
        HART_INIT_DONE[hart_id].store(false, Ordering::Relaxed);
        WORK_UNITS_DONE[hart_id].store(0, Ordering::Relaxed);
        TIMER_IRQS[hart_id].store(0, Ordering::Relaxed);
        IPI_IRQS[hart_id].store(0, Ordering::Relaxed);
        IPI_SENT[hart_id].store(0, Ordering::Relaxed);
        INTERRUPTS_WHILE_LOCK_HELD[hart_id].store(0, Ordering::Relaxed);
        LAST_MEPC[hart_id].store(0, Ordering::Relaxed);
        HART_IN_CRITICAL[hart_id].store(false, Ordering::Relaxed);
        MIN_INTERRUPT_SP[hart_id].store(usize::MAX, Ordering::Relaxed);
        LAST_INTERRUPT_SP[hart_id].store(0, Ordering::Relaxed);
    }

    {
        let mut state = SHARED_STATE.lock();
        *state = SharedKernelState::empty();
    }
}

fn mark_hart_initialized(hart_id: usize) -> usize {
    HART_INIT_DONE[hart_id].store(true, Ordering::Release);
    BOOTED_MASK.fetch_or(1usize << hart_id, Ordering::AcqRel);
    READY_HARTS.fetch_add(1, Ordering::AcqRel) + 1
}

fn release_secondary_harts() {
    unsafe {
        ptr::write_volatile(ptr::addr_of_mut!(__boot_release_flag), 1);
    }
}

fn init_trap_vector() {
    unsafe {
        asm!(
            "csrw mtvec, {}",
            in(reg) trap_entry as *const () as usize,
            options(nostack, nomem)
        );
    }
}

fn install_interrupt_stack(hart_id: usize) {
    unsafe {
        asm!(
            "csrw mscratch, {}",
            in(reg) interrupt_stack_top(hart_id),
            options(nostack, nomem)
        );
    }
}

fn enable_machine_interrupts() {
    unsafe {
        asm!(
            "csrs mie, {}",
            in(reg) MIE_MTIE | MIE_MSIE,
            options(nostack, nomem)
        );
        asm!(
            "csrs mstatus, {}",
            in(reg) MSTATUS_MIE,
            options(nostack, nomem)
        );
    }
}

fn enable_fp_context() {
    unsafe {
        asm!(
            "csrs mstatus, {}",
            in(reg) MSTATUS_FS_DIRTY,
            options(nostack, nomem)
        );
    }
}

fn disable_machine_interrupts() {
    unsafe {
        asm!(
            "csrc mstatus, {}",
            in(reg) MSTATUS_MIE,
            options(nostack, nomem)
        );
        asm!(
            "csrc mie, {}",
            in(reg) MIE_MTIE | MIE_MSIE,
            options(nostack, nomem)
        );
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

#[no_mangle]
pub extern "C" fn rust_handle_trap(frame: &mut TrapFrame) {
    let hart_id = read_mhartid();
    let mcause = read_mcause();

    LAST_MEPC[hart_id].store(frame.mepc as u64, Ordering::Relaxed);
    record_interrupt_stack(hart_id, frame as *const TrapFrame as usize);

    if HART_IN_CRITICAL[hart_id].load(Ordering::Acquire) {
        INTERRUPTS_WHILE_LOCK_HELD[hart_id].fetch_add(1, Ordering::Relaxed);
        if ACTIVE_LOCK_HOLDER.load(Ordering::Acquire) != hart_id {
            LOCK_STATE_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    match mcause {
        MACHINE_TIMER_INTERRUPT => handle_timer_interrupt(hart_id),
        MACHINE_SOFTWARE_INTERRUPT => handle_machine_software_interrupt(hart_id),
        _ => {
            println!(
                "[kernel] unexpected trap on hart{}: mcause={:#x} mepc={:#x} mtval={:#x}",
                hart_id,
                mcause,
                frame.mepc,
                read_mtval()
            );
            qemu_exit(1);
        }
    }
}

fn handle_timer_interrupt(hart_id: usize) {
    let now = read_mtime();
    arm_next_timer_interrupt(hart_id, now);

    let count = TIMER_IRQS[hart_id].fetch_add(1, Ordering::AcqRel) + 1;
    if count % IPI_SEND_PERIOD == 0 {
        let already_sent = IPI_SENT[hart_id].load(Ordering::Acquire);
        if already_sent < IPI_SEND_LIMIT {
            let next_hart = (hart_id + 1) % EXPECTED_HARTS;
            send_ipi(next_hart);
            IPI_SENT[hart_id].fetch_add(1, Ordering::AcqRel);
        }
    }
}

fn handle_machine_software_interrupt(hart_id: usize) {
    clear_msip(hart_id);
    IPI_IRQS[hart_id].fetch_add(1, Ordering::AcqRel);
}

fn send_ipi(target_hart: usize) {
    unsafe {
        ptr::write_volatile(msip_addr(target_hart) as *mut u32, 1);
    }
}

fn clear_msip(hart_id: usize) {
    unsafe {
        ptr::write_volatile(msip_addr(hart_id) as *mut u32, 0);
    }
}

fn arm_next_timer_interrupt(hart_id: usize, now: u64) {
    unsafe {
        ptr::write_volatile(mtimecmp_addr(hart_id) as *mut u64, now.wrapping_add(TIMER_INTERVAL_TICKS));
    }
}

fn msip_addr(hart_id: usize) -> usize {
    CLINT_BASE + hart_id * 4
}

fn mtimecmp_addr(hart_id: usize) -> usize {
    CLINT_BASE + CLINT_MTIMECMP_OFFSET + hart_id * 8
}

fn record_interrupt_stack(hart_id: usize, frame_sp: usize) {
    LAST_INTERRUPT_SP[hart_id].store(frame_sp, Ordering::Relaxed);
    update_min_sp(&MIN_INTERRUPT_SP[hart_id], frame_sp);
}

fn update_min_sp(slot: &AtomicUsize, candidate: usize) {
    let mut current = slot.load(Ordering::Acquire);

    while candidate < current {
        match slot.compare_exchange_weak(current, candidate, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn hold_lock_until(deadline: u64) {
    while read_mtime() < deadline {
        spin_loop();
    }
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

fn interrupt_stack_top(hart_id: usize) -> usize {
    (ptr::addr_of!(__interrupt_stack_top) as usize)
        .wrapping_sub(hart_id * INTERRUPT_STACK_PER_HART)
}

fn interrupt_stack_bottom(hart_id: usize) -> usize {
    interrupt_stack_top(hart_id).wrapping_sub(INTERRUPT_STACK_PER_HART)
}

fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
}

fn read_sp() -> usize {
    let value: usize;
    unsafe {
        asm!("mv {}, sp", out(reg) value, options(nostack, nomem, preserves_flags));
    }
    value
}

fn read_mhartid() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mhartid", out(reg) value, options(nostack, nomem));
    }
    value
}

fn read_mcause() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mcause", out(reg) value, options(nostack, nomem));
    }
    value
}

fn read_mtval() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mtval", out(reg) value, options(nostack, nomem));
    }
    value
}

fn ticks_to_us(ticks: u64) -> u64 {
    ticks.saturating_mul(MTIME_TICK_NS) / 1_000
}

fn pass_fail(condition: bool) -> &'static str {
    if condition {
        "PASS"
    } else {
        "FAIL"
    }
}

fn bool_to_u64(value: bool) -> u64 {
    if value {
        1
    } else {
        0
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
    let value = if code == 0 { 0x5555 } else { (code << 16) | 0x3333 };

    unsafe {
        ptr::write_volatile(QEMU_TEST_BASE as *mut u32, value);
    }

    loop {
        spin_loop();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    println!("[kernel] panic on hart{}: {}", read_mhartid(), info);
    qemu_exit(1)
}
