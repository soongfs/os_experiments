#![no_std]
#![no_main]

mod apps;
mod console;
mod syscall;
mod trap;

use core::arch::asm;
use core::arch::global_asm;
use core::mem::{align_of, size_of};
use core::panic::PanicInfo;
use core::ptr;

global_asm!(include_str!("boot.S"));

const DRAM_START: usize = 0x8000_0000;
const TASK_NAME_LEN: usize = 32;
const APP_COUNT: usize = 3;
const WARMUP_RUNS_PER_APP: usize = 1;
const MEASURED_RUNS_PER_APP: usize = 3;
const TOTAL_RUNS: usize = APP_COUNT * (WARMUP_RUNS_PER_APP + MEASURED_RUNS_PER_APP);

const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;

pub const IO_BURST_WRITES: u64 = 24;
pub const INFO_PROBE_CALLS: u64 = 20;

pub const SYS_WRITE: usize = 0;
pub const SYS_GET_TASKINFO: usize = 1;
pub const SYS_EXIT: usize = 2;

pub const EFAULT: isize = -14;
pub const EINVAL: isize = -22;
pub const ENOSYS: isize = -38;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub task_id: u64,
    pub task_name: [u8; TASK_NAME_LEN],
}

impl TaskInfo {
    pub const fn empty() -> Self {
        Self {
            task_id: 0,
            task_name: [0; TASK_NAME_LEN],
        }
    }

    pub fn name(&self) -> &str {
        bytes_to_str(&self.task_name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunStatus {
    Scheduled,
    Exited,
    Faulted,
}

#[derive(Clone, Copy)]
struct AppDefinition {
    id: u64,
    name: &'static str,
    task_name: [u8; TASK_NAME_LEN],
    entry: extern "C" fn() -> !,
    expectation: &'static str,
}

impl AppDefinition {
    const fn new(
        id: u64,
        name: &'static str,
        task_name: [u8; TASK_NAME_LEN],
        entry: extern "C" fn() -> !,
        expectation: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            task_name,
            entry,
            expectation,
        }
    }
}

#[derive(Clone, Copy)]
struct RunRecord {
    app_index: usize,
    round: usize,
    counted: bool,
    start_mtime: u64,
    end_mtime: u64,
    elapsed_ticks: u64,
    elapsed_us: u64,
    exit_code: i32,
    fault_cause: u64,
    fault_tval: u64,
    status: RunStatus,
}

impl RunRecord {
    const fn scheduled(app_index: usize, round: usize, counted: bool) -> Self {
        Self {
            app_index,
            round,
            counted,
            start_mtime: 0,
            end_mtime: 0,
            elapsed_ticks: 0,
            elapsed_us: 0,
            exit_code: 0,
            fault_cause: 0,
            fault_tval: 0,
            status: RunStatus::Scheduled,
        }
    }
}

#[derive(Clone, Copy)]
struct AppTimingSummary {
    runs: usize,
    successful_runs: usize,
    min_ticks: u64,
    max_ticks: u64,
    total_ticks: u64,
    min_us: u64,
    max_us: u64,
    total_us: u64,
}

impl AppTimingSummary {
    const fn empty() -> Self {
        Self {
            runs: 0,
            successful_runs: 0,
            min_ticks: 0,
            max_ticks: 0,
            total_ticks: 0,
            min_us: 0,
            max_us: 0,
            total_us: 0,
        }
    }

    fn avg_ticks(self) -> u64 {
        if self.successful_runs == 0 {
            0
        } else {
            self.total_ticks / self.successful_runs as u64
        }
    }

    fn avg_us(self) -> u64 {
        if self.successful_runs == 0 {
            0
        } else {
            self.total_us / self.successful_runs as u64
        }
    }

    fn spread_us(self) -> u64 {
        self.max_us.saturating_sub(self.min_us)
    }

    fn spread_basis_points(self) -> u64 {
        let avg_us = self.avg_us();

        if avg_us == 0 {
            0
        } else {
            self.spread_us().saturating_mul(10_000) / avg_us
        }
    }
}

static APPS: [AppDefinition; APP_COUNT] = [
    AppDefinition::new(
        1,
        "io_burst",
        padded_name(b"io_burst"),
        apps::io_burst::io_burst,
        "I/O-heavy: UART writes dominate; timing should include serial output cost",
    ),
    AppDefinition::new(
        2,
        "compute_spin",
        padded_name(b"compute_spin"),
        apps::compute_spin::compute_spin,
        "compute-heavy: expected to be the slowest on average",
    ),
    AppDefinition::new(
        3,
        "info_probe",
        padded_name(b"info_probe"),
        apps::info_probe::info_probe,
        "syscall-heavy but low-output: repeated get_taskinfo probes should remain shorter than compute_spin",
    ),
];

static mut RUNS: [RunRecord; TOTAL_RUNS] = [
    RunRecord::scheduled(0, 0, false),
    RunRecord::scheduled(0, 1, true),
    RunRecord::scheduled(0, 2, true),
    RunRecord::scheduled(0, 3, true),
    RunRecord::scheduled(1, 0, false),
    RunRecord::scheduled(1, 1, true),
    RunRecord::scheduled(1, 2, true),
    RunRecord::scheduled(1, 3, true),
    RunRecord::scheduled(2, 0, false),
    RunRecord::scheduled(2, 1, true),
    RunRecord::scheduled(2, 2, true),
    RunRecord::scheduled(2, 3, true),
];

static mut CURRENT_RUN_INDEX: usize = 0;

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
    println!("[kernel] starting LAB2 kernel task3 completion-time suite");
    println!(
        "[kernel] time source: CLINT mtime @ {:#x}, timebase-frequency={} Hz, 1 tick={} ns",
        MTIME_ADDR, MTIME_FREQ_HZ, MTIME_TICK_NS
    );
    println!(
        "[kernel] timing window: after kernel setup, immediately before mret to U-mode, until first trap back from the app"
    );
    println!(
        "[kernel] each app runs 1 warm-up round plus {} measured rounds; summary excludes warm-up",
        MEASURED_RUNS_PER_APP
    );

    launch_run(0)
}

pub fn handle_syscall(frame: &mut trap::TrapFrame) {
    match frame.a7 {
        SYS_WRITE => {
            frame.a0 = sys_write(frame.a0 as *const u8, frame.a1) as usize;
        }
        SYS_GET_TASKINFO => {
            frame.a0 = sys_get_taskinfo(frame.a0 as *mut TaskInfo) as usize;
        }
        SYS_EXIT => finish_current_run(frame.a0 as i32),
        _ => frame.a0 = ENOSYS as usize,
    }
}

pub fn handle_user_fault(mcause: usize, mepc: usize, mtval: usize) -> ! {
    complete_current_run(
        RunStatus::Faulted,
        0,
        mcause as u64,
        mtval as u64,
        Some(mepc as u64),
    );
    advance_to_next_run()
}

fn launch_run(index: usize) -> ! {
    unsafe {
        CURRENT_RUN_INDEX = index;

        let run = &mut RUNS[index];
        run.start_mtime = 0;
        run.end_mtime = 0;
        run.elapsed_ticks = 0;
        run.elapsed_us = 0;
        run.exit_code = 0;
        run.fault_cause = 0;
        run.fault_tval = 0;
        run.status = RunStatus::Scheduled;

        let app = APPS[run.app_index];
        if run.counted {
            println!(
                "[kernel] launch app={} round={}/{} | expected: {}",
                app.name, run.round, MEASURED_RUNS_PER_APP, app.expectation
            );
        } else {
            println!(
                "[kernel] launch app={} warm-up run | expected: {}",
                app.name, app.expectation
            );
        }

        // Start timing after all kernel bookkeeping and logging for this run are done.
        run.start_mtime = read_mtime();

        enter_user_mode(
            app.entry as *const () as usize,
            ptr::addr_of!(__user_stack_top) as usize,
            ptr::addr_of!(__kernel_stack_top) as usize,
        )
    }
}

fn finish_current_run(code: i32) -> ! {
    complete_current_run(RunStatus::Exited, code, 0, 0, None);
    advance_to_next_run()
}

fn complete_current_run(
    status: RunStatus,
    exit_code: i32,
    fault_cause: u64,
    fault_tval: u64,
    fault_pc: Option<u64>,
) {
    unsafe {
        let run = &mut RUNS[CURRENT_RUN_INDEX];
        run.end_mtime = read_mtime();
        run.elapsed_ticks = run.end_mtime.wrapping_sub(run.start_mtime);
        run.elapsed_us = ticks_to_us(run.elapsed_ticks);
        run.exit_code = exit_code;
        run.fault_cause = fault_cause;
        run.fault_tval = fault_tval;
        run.status = status;

        if let Some(pc) = fault_pc {
            println!(
                "[kernel] fault app={} round={} : mcause={:#x} mepc={:#x} mtval={:#x}",
                current_app().name,
                run.round,
                fault_cause,
                pc,
                fault_tval
            );
        }
    }
}

fn advance_to_next_run() -> ! {
    let finished_index = unsafe { CURRENT_RUN_INDEX };
    print_run_result(finished_index);

    let next_index = finished_index + 1;
    if next_index < TOTAL_RUNS {
        launch_run(next_index)
    } else {
        print_final_report()
    }
}

fn print_run_result(index: usize) {
    let run = unsafe { RUNS[index] };
    let app = APPS[run.app_index];
    let millis = run.elapsed_us / 1_000;
    let millis_frac = run.elapsed_us % 1_000;

    match (run.counted, run.status) {
        (false, RunStatus::Exited) => println!(
            "[kernel] result {} warm-up: status=exit({}) mtime=[{} -> {}] delta={} ticks = {} us = {}.{:03} ms",
            app.name,
            run.exit_code,
            run.start_mtime,
            run.end_mtime,
            run.elapsed_ticks,
            run.elapsed_us,
            millis,
            millis_frac
        ),
        (false, RunStatus::Faulted) => println!(
            "[kernel] result {} warm-up: status=fault(cause={:#x}, mtval={:#x}) mtime=[{} -> {}] delta={} ticks = {} us = {}.{:03} ms",
            app.name,
            run.fault_cause,
            run.fault_tval,
            run.start_mtime,
            run.end_mtime,
            run.elapsed_ticks,
            run.elapsed_us,
            millis,
            millis_frac
        ),
        (false, RunStatus::Scheduled) => {
            println!("[kernel] result {} warm-up: status=scheduled", app.name)
        }
        (true, RunStatus::Exited) => println!(
            "[kernel] result {} round {}: status=exit({}) mtime=[{} -> {}] delta={} ticks = {} us = {}.{:03} ms",
            app.name,
            run.round,
            run.exit_code,
            run.start_mtime,
            run.end_mtime,
            run.elapsed_ticks,
            run.elapsed_us,
            millis,
            millis_frac
        ),
        (true, RunStatus::Faulted) => println!(
            "[kernel] result {} round {}: status=fault(cause={:#x}, mtval={:#x}) mtime=[{} -> {}] delta={} ticks = {} us = {}.{:03} ms",
            app.name,
            run.round,
            run.fault_cause,
            run.fault_tval,
            run.start_mtime,
            run.end_mtime,
            run.elapsed_ticks,
            run.elapsed_us,
            millis,
            millis_frac
        ),
        (true, RunStatus::Scheduled) => {
            println!("[kernel] result {} round {}: status=scheduled", app.name, run.round)
        }
    }
}

fn print_final_report() -> ! {
    println!("[kernel] final timing summary:");

    let io_summary = summarize_app(0);
    let compute_summary = summarize_app(1);
    let info_summary = summarize_app(2);

    print_app_summary(0, io_summary);
    print_app_summary(1, compute_summary);
    print_app_summary(2, info_summary);

    let all_runs_ok = every_run_exited_successfully();
    let all_timings_nonzero = every_run_has_nonzero_timing();
    let repeated_measurements_ok = each_app_has_expected_run_count();
    let compute_is_slowest = compute_summary.avg_us() > io_summary.avg_us()
        && compute_summary.avg_us() > info_summary.avg_us();

    println!(
        "[kernel] check all runs exited successfully: {}",
        pass_fail(all_runs_ok)
    );
    println!(
        "[kernel] check every run captured a non-zero mtime delta: {}",
        pass_fail(all_timings_nonzero)
    );
    println!(
        "[kernel] check each app was measured {} times after warm-up: {}",
        MEASURED_RUNS_PER_APP,
        pass_fail(repeated_measurements_ok)
    );
    println!(
        "[kernel] check compute_spin stays slowest on average: {}",
        pass_fail(compute_is_slowest)
    );

    if all_runs_ok && all_timings_nonzero && repeated_measurements_ok && compute_is_slowest {
        qemu_exit(0)
    } else {
        qemu_exit(1)
    }
}

fn print_app_summary(app_index: usize, summary: AppTimingSummary) {
    let app = APPS[app_index];
    let avg_us = summary.avg_us();
    let avg_ms = avg_us / 1_000;
    let avg_ms_frac = avg_us % 1_000;
    let min_ms = summary.min_us / 1_000;
    let min_ms_frac = summary.min_us % 1_000;
    let max_ms = summary.max_us / 1_000;
    let max_ms_frac = summary.max_us % 1_000;
    let spread_bp = summary.spread_basis_points();

    println!(
        "[kernel] summary {}: measured_runs={} ok={} min={} us ({}.{:03} ms) max={} us ({}.{:03} ms) avg={} ticks / {} us ({}.{:03} ms) spread={} us ({:02}.{:02}%)",
        app.name,
        summary.runs,
        summary.successful_runs,
        summary.min_us,
        min_ms,
        min_ms_frac,
        summary.max_us,
        max_ms,
        max_ms_frac,
        summary.avg_ticks(),
        avg_us,
        avg_ms,
        avg_ms_frac,
        summary.spread_us(),
        spread_bp / 100,
        spread_bp % 100
    );
}

fn summarize_app(app_index: usize) -> AppTimingSummary {
    let mut summary = AppTimingSummary::empty();
    let mut index = 0;

    while index < TOTAL_RUNS {
        let run = unsafe { RUNS[index] };
        if run.app_index == app_index && run.counted {
            summary.runs += 1;

            if run.status == RunStatus::Exited && run.exit_code == 0 {
                summary.successful_runs += 1;
                summary.total_ticks = summary.total_ticks.saturating_add(run.elapsed_ticks);
                summary.total_us = summary.total_us.saturating_add(run.elapsed_us);

                if summary.successful_runs == 1 {
                    summary.min_ticks = run.elapsed_ticks;
                    summary.max_ticks = run.elapsed_ticks;
                    summary.min_us = run.elapsed_us;
                    summary.max_us = run.elapsed_us;
                } else {
                    summary.min_ticks = summary.min_ticks.min(run.elapsed_ticks);
                    summary.max_ticks = summary.max_ticks.max(run.elapsed_ticks);
                    summary.min_us = summary.min_us.min(run.elapsed_us);
                    summary.max_us = summary.max_us.max(run.elapsed_us);
                }
            }
        }

        index += 1;
    }

    summary
}

fn every_run_exited_successfully() -> bool {
    let mut index = 0;

    while index < TOTAL_RUNS {
        let run = unsafe { RUNS[index] };
        if run.status != RunStatus::Exited || run.exit_code != 0 {
            return false;
        }
        index += 1;
    }

    true
}

fn every_run_has_nonzero_timing() -> bool {
    let mut index = 0;

    while index < TOTAL_RUNS {
        let run = unsafe { RUNS[index] };
        if run.start_mtime == 0 || run.end_mtime <= run.start_mtime || run.elapsed_us == 0 {
            return false;
        }
        index += 1;
    }

    true
}

fn each_app_has_expected_run_count() -> bool {
    let mut app_index = 0;

    while app_index < APP_COUNT {
        if summarize_app(app_index).successful_runs != MEASURED_RUNS_PER_APP {
            return false;
        }
        app_index += 1;
    }

    true
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

    let app = current_app();
    *task_info = TaskInfo {
        task_id: app.id,
        task_name: app.task_name,
    };

    0
}

fn current_app() -> AppDefinition {
    let run_index = unsafe { CURRENT_RUN_INDEX };
    let app_index = unsafe { RUNS[run_index].app_index };
    APPS[app_index]
}

fn ticks_to_us(ticks: u64) -> u64 {
    ((ticks as u128) * 1_000_000u128 / MTIME_FREQ_HZ as u128) as u64
}

fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
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
