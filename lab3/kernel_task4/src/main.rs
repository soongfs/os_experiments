#![no_std]
#![no_main]

mod console;
mod trap;

use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIMECMP_OFFSET: usize = 0x4000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIMECMP_ADDR: usize = CLINT_BASE + CLINT_MTIMECMP_OFFSET;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;

const TIMER_INTERVAL_TICKS: u64 = 16_000;
const CRITICAL_HOLD_TICKS: u64 = 24_000;
const TARGET_KERNEL_INTERRUPTS: u64 = 6;
const SAFE_LOG_LIMIT: u64 = 4;
const IRQ_LOG_LIMIT: u64 = 6;

const QEMU_TEST_BASE: usize = 0x0010_0000;

const MIE_MTIE: usize = 1 << 7;
const MIP_SSIP: usize = 1 << 1;
const SIE_SSIE: usize = 1 << 1;
const SSTATUS_SIE: usize = 1 << 1;
const INTERRUPT_BIT: usize = 1usize << (usize::BITS as usize - 1);
const MACHINE_TIMER_INTERRUPT: usize = INTERRUPT_BIT | 7;
const SUPERVISOR_SOFTWARE_INTERRUPT: usize = INTERRUPT_BIT | 1;

#[derive(Clone, Copy)]
struct SharedState {
    critical_sections: u64,
    pending_observed_during_critical: u64,
    handler_path_observed: u64,
    critical_updates: u64,
}

impl SharedState {
    const fn empty() -> Self {
        Self {
            critical_sections: 0,
            pending_observed_during_critical: 0,
            handler_path_observed: 0,
            critical_updates: 0,
        }
    }
}

struct InterruptMutex<T> {
    locked: UnsafeCell<bool>,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for InterruptMutex<T> {}

impl<T> InterruptMutex<T> {
    const fn new(value: T) -> Self {
        Self {
            locked: UnsafeCell::new(false),
            value: UnsafeCell::new(value),
        }
    }

    fn lock(&self) -> InterruptMutexGuard<'_, T> {
        let interrupt_guard = InterruptGuard::new();

        unsafe {
            if *self.locked.get() {
                println!("[kernel] interrupt mutex re-entry detected");
                qemu_exit(1);
            }

            *self.locked.get() = true;
        }

        LOCK_ACQUISITIONS.fetch_add(1, Ordering::Relaxed);

        InterruptMutexGuard {
            mutex: self,
            interrupt_guard,
        }
    }
}

struct InterruptMutexGuard<'a, T> {
    mutex: &'a InterruptMutex<T>,
    interrupt_guard: InterruptGuard,
}

impl<T> Deref for InterruptMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for InterruptMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for InterruptMutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            *self.mutex.locked.get() = false;
        }
        let _ = &self.interrupt_guard;
    }
}

struct InterruptGuard {
    previous_sie: bool,
}

impl InterruptGuard {
    fn new() -> Self {
        let previous_sie = supervisor_interrupts_enabled();
        disable_supervisor_interrupts();
        Self { previous_sie }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.previous_sie {
            enable_supervisor_interrupts();
        }
    }
}

static MACHINE_TIMER_FORWARDS: AtomicU64 = AtomicU64::new(0);
static SUPERVISOR_TIMER_COUNT: AtomicU64 = AtomicU64::new(0);
static LOCK_ACQUISITIONS: AtomicU64 = AtomicU64::new(0);
static SAFE_INTERVAL_SIE_OPENED: AtomicBool = AtomicBool::new(false);
static CRITICAL_SECTION_SIE_CLOSED: AtomicBool = AtomicBool::new(false);
static KERNEL_SURVIVED_TIMER_INTERRUPTS: AtomicBool = AtomicBool::new(false);
static LAST_MCAUSE: AtomicU64 = AtomicU64::new(0);
static LAST_SCAUSE: AtomicU64 = AtomicU64::new(0);
static LAST_SEPC: AtomicU64 = AtomicU64::new(0);

static SHARED_STATE: InterruptMutex<SharedState> = InterruptMutex::new(SharedState::empty());

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __supervisor_trap_stack_top: u8;
    static __machine_trap_stack_top: u8;

    fn enter_supervisor(supervisor_entry: usize, supervisor_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_machine() -> ! {
    clear_bss();
    configure_pmp();
    trap::init_machine_trap_vector();
    delegate_supervisor_timer_interrupt();
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
    clear_supervisor_timer_pending();

    unsafe {
        asm!(
            "csrw sscratch, {}",
            in(reg) supervisor_trap_stack_top(),
            options(nostack, nomem)
        );
    }

    println!("[kernel] booted in S-mode");
    println!("[kernel] LAB3 kernel task4 in-kernel interrupt response");
    println!(
        "[kernel] timer source: mtime={:#x}, mtimecmp={:#x}, frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIMECMP_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] policy: sstatus.SIE=1 in safe intervals, sstatus.SIE=0 while holding interrupt mutex"
    );
    println!(
        "[kernel] delivery path: machine_timer_forwarder(MTIP) -> delegated supervisor_software(SSIP) -> S-mode handler"
    );
    println!(
        "[kernel] timer interval={} ticks ({} us), critical_hold={} ticks ({} us)",
        TIMER_INTERVAL_TICKS,
        ticks_to_us(TIMER_INTERVAL_TICKS),
        CRITICAL_HOLD_TICKS,
        ticks_to_us(CRITICAL_HOLD_TICKS)
    );

    clear_supervisor_timer_pending();
    enable_supervisor_timer_source();

    run_kernel_experiment()
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
    set_supervisor_timer_pending();
    MACHINE_TIMER_FORWARDS.fetch_add(1, Ordering::Relaxed);
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

    clear_supervisor_timer_pending();
    let irq_index = SUPERVISOR_TIMER_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    KERNEL_SURVIVED_TIMER_INTERRUPTS.store(true, Ordering::Relaxed);

    {
        let mut stats = SHARED_STATE.lock();
        stats.handler_path_observed += 1;
    }

    if irq_index <= IRQ_LOG_LIMIT {
        println!(
            "[kernel] irq#{} source=forwarded_timer(origin=mtime, delivered=ssip) scause={:#x} sepc={:#x} path=machine_timer_forwarder->supervisor_trap->timer_handler",
            irq_index,
            scause,
            frame.epc
        );
    }
}

fn run_kernel_experiment() -> ! {
    let mut round = 0u64;

    while SUPERVISOR_TIMER_COUNT.load(Ordering::Relaxed) < TARGET_KERNEL_INTERRUPTS {
        run_safe_interval(round);
        run_critical_section(round);
        round += 1;
    }

    finish_experiment()
}

fn run_safe_interval(round: u64) {
    let before = SUPERVISOR_TIMER_COUNT.load(Ordering::Relaxed);

    if round < SAFE_LOG_LIMIT {
        println!(
            "[kernel] safe interval #{}: enabling sstatus.SIE for forwarded_timer_irq",
            round + 1
        );
    }

    enable_supervisor_interrupts();
    let sie_open = supervisor_interrupts_enabled();
    SAFE_INTERVAL_SIE_OPENED.store(sie_open, Ordering::Relaxed);

    while SUPERVISOR_TIMER_COUNT.load(Ordering::Relaxed) == before {
        unsafe {
            asm!("wfi", options(nomem, nostack));
        }
    }

    disable_supervisor_interrupts();
}

fn run_critical_section(round: u64) {
    let critical_index;

    {
        let mut state = SHARED_STATE.lock();
        state.critical_sections += 1;
        critical_index = state.critical_sections;

        let sie_closed = !supervisor_interrupts_enabled();
        CRITICAL_SECTION_SIE_CLOSED.store(sie_closed, Ordering::Relaxed);

        if round < SAFE_LOG_LIMIT {
            println!(
                "[kernel] critical section #{} enter: sstatus.SIE={} lock=held",
                critical_index,
                bool_word(!sie_closed)
            );
        }

        let start = read_mtime();
        while read_mtime().wrapping_sub(start) < CRITICAL_HOLD_TICKS {
            state.critical_updates = state.critical_updates.wrapping_add(1);
        }

        let pending = supervisor_timer_pending();
        if pending {
            state.pending_observed_during_critical += 1;
        }

        if round < SAFE_LOG_LIMIT {
            println!(
                "[kernel] critical section #{} hold: sstatus.SIE={} pending_forwarded_timer_irq={}",
                critical_index,
                bool_word(supervisor_interrupts_enabled()),
                bool_word(pending)
            );
        }
    }

    if round < SAFE_LOG_LIMIT {
        println!(
            "[kernel] critical section #{} exit: sstatus.SIE_after_guard={}",
            critical_index,
            bool_word(supervisor_interrupts_enabled())
        );
    }

    disable_supervisor_interrupts();
}

fn finish_experiment() -> ! {
    disable_supervisor_interrupts();

    let state = {
        let guard = SHARED_STATE.lock();
        *guard
    };
    let machine_forwards = MACHINE_TIMER_FORWARDS.load(Ordering::Relaxed);
    let supervisor_irqs = SUPERVISOR_TIMER_COUNT.load(Ordering::Relaxed);
    let lock_acquisitions = LOCK_ACQUISITIONS.load(Ordering::Relaxed);
    let safe_interval_ok = SAFE_INTERVAL_SIE_OPENED.load(Ordering::Relaxed);
    let survived_ok = KERNEL_SURVIVED_TIMER_INTERRUPTS.load(Ordering::Relaxed)
        && machine_forwards > 0
        && supervisor_irqs > 0;
    let critical_ok = CRITICAL_SECTION_SIE_CLOSED.load(Ordering::Relaxed)
        && state.pending_observed_during_critical > 0
        && lock_acquisitions >= state.critical_sections + state.handler_path_observed;

    println!(
        "[kernel] summary: machine_timer_forwards={} supervisor_timer_irqs={} critical_sections={} lock_acquisitions={} pending_during_critical={} critical_updates={}",
        machine_forwards,
        supervisor_irqs,
        state.critical_sections,
        lock_acquisitions,
        state.pending_observed_during_critical,
        state.critical_updates
    );
    println!(
        "[kernel] diagnostics: last_mcause={:#x} last_scause={:#x} last_sepc={:#x}",
        LAST_MCAUSE.load(Ordering::Relaxed),
        LAST_SCAUSE.load(Ordering::Relaxed),
        LAST_SEPC.load(Ordering::Relaxed)
    );
    println!(
        "[kernel] acceptance sstatus.SIE opened in safe interval: {}",
        pass_fail(safe_interval_ok)
    );
    println!(
        "[kernel] acceptance kernel-space timer interrupt handled without crash: {}",
        pass_fail(survived_ok)
    );
    println!(
        "[kernel] acceptance locks and critical sections used interrupt-off protection: {}",
        pass_fail(critical_ok)
    );

    qemu_exit(if safe_interval_ok && survived_ok && critical_ok {
        0
    } else {
        1
    })
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

fn delegate_supervisor_timer_interrupt() {
    unsafe {
        asm!("csrw mideleg, {}", in(reg) MIP_SSIP, options(nostack, nomem));
    }
}

fn enable_machine_timer_interrupt() {
    unsafe {
        asm!("csrs mie, {}", in(reg) MIE_MTIE, options(nostack, nomem));
    }
}

fn enable_supervisor_timer_source() {
    unsafe {
        asm!("csrs sie, {}", in(reg) SIE_SSIE, options(nostack, nomem));
    }
}

fn enable_supervisor_interrupts() {
    unsafe {
        asm!("csrs sstatus, {}", in(reg) SSTATUS_SIE, options(nostack, nomem));
    }
}

fn disable_supervisor_interrupts() {
    unsafe {
        asm!("csrc sstatus, {}", in(reg) SSTATUS_SIE, options(nostack, nomem));
    }
}

fn supervisor_interrupts_enabled() -> bool {
    let sstatus: usize;

    unsafe {
        asm!("csrr {}, sstatus", out(reg) sstatus, options(nostack, nomem));
    }

    (sstatus & SSTATUS_SIE) != 0
}

fn set_supervisor_timer_pending() {
    unsafe {
        asm!("csrs mip, {}", in(reg) MIP_SSIP, options(nostack, nomem));
    }
}

fn clear_supervisor_timer_pending() {
    unsafe {
        asm!("csrc sip, {}", in(reg) MIP_SSIP, options(nostack, nomem));
    }
}

fn supervisor_timer_pending() -> bool {
    let sip: usize;

    unsafe {
        asm!("csrr {}, sip", out(reg) sip, options(nostack, nomem));
    }

    (sip & MIP_SSIP) != 0
}

fn arm_next_timer_interrupt() {
    let next = read_mtime().wrapping_add(TIMER_INTERVAL_TICKS);

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

fn ticks_to_us(ticks: u64) -> u64 {
    ticks.saturating_mul(1_000_000) / MTIME_FREQ_HZ
}

fn bool_word(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
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

fn kernel_stack_top() -> usize {
    ptr::addr_of!(__kernel_stack_top) as usize
}

fn supervisor_trap_stack_top() -> usize {
    ptr::addr_of!(__supervisor_trap_stack_top) as usize
}

fn machine_trap_stack_top() -> usize {
    ptr::addr_of!(__machine_trap_stack_top) as usize
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
