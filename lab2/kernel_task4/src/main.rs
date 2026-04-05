#![no_std]
#![no_main]

mod apps;
mod console;
mod syscall;
mod trap;

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr;

use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const PHYS_BASE: usize = 0x8000_0000;
const USER_BASE: usize = 0x4000_0000;
const UART_BASE: usize = 0x1000_0000;
const QEMU_TEST_BASE: usize = 0x0010_0000;

const APP_COUNT: usize = 5;

const SYS_WRITE: usize = 0;
const SYS_EXIT: usize = 1;

const EFAULT: isize = -14;
const ENOSYS: isize = -38;

const SSTATUS_SUM: usize = 1 << 18;

const EXC_ILLEGAL_INSTRUCTION: usize = 2;
const EXC_LOAD_PAGE_FAULT: usize = 13;
const EXC_STORE_PAGE_FAULT: usize = 15;
const EXC_INST_PAGE_FAULT: usize = 12;
const EXC_ECALL_FROM_U: usize = 8;

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_G: u64 = 1 << 5;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

const SATP_MODE_SV39: usize = 8usize << 60;

#[repr(C, align(4096))]
struct PageTable([u64; 512]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunStatus {
    Pending,
    Exited,
    Faulted,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpectedOutcome {
    Exit(i32),
    Fault(usize),
}

#[derive(Clone, Copy)]
struct AppDefinition {
    name: &'static str,
    entry: extern "C" fn() -> !,
    expectation: &'static str,
    expected_outcome: ExpectedOutcome,
}

impl AppDefinition {
    const fn new(
        name: &'static str,
        entry: extern "C" fn() -> !,
        expectation: &'static str,
        expected_outcome: ExpectedOutcome,
    ) -> Self {
        Self {
            name,
            entry,
            expectation,
            expected_outcome,
        }
    }
}

#[derive(Clone, Copy)]
struct RunRecord {
    status: RunStatus,
    exit_code: i32,
    scause: usize,
    sepc: usize,
    stval: usize,
    instruction: u32,
    instruction_valid: bool,
    ra: usize,
    sp: usize,
    gp: usize,
    s0: usize,
    a0: usize,
    a1: usize,
    a7: usize,
}

impl RunRecord {
    const fn empty() -> Self {
        Self {
            status: RunStatus::Pending,
            exit_code: 0,
            scause: 0,
            sepc: 0,
            stval: 0,
            instruction: 0,
            instruction_valid: false,
            ra: 0,
            sp: 0,
            gp: 0,
            s0: 0,
            a0: 0,
            a1: 0,
            a7: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct ExceptionStats {
    illegal_instruction: u64,
    store_page_fault: u64,
    load_page_fault: u64,
    instruction_page_fault: u64,
    other_faults: u64,
    clean_exits: u64,
}

impl ExceptionStats {
    const fn empty() -> Self {
        Self {
            illegal_instruction: 0,
            store_page_fault: 0,
            load_page_fault: 0,
            instruction_page_fault: 0,
            other_faults: 0,
            clean_exits: 0,
        }
    }
}

static APPS: [AppDefinition; APP_COUNT] = [
    AppDefinition::new(
        "healthy_before_faults",
        apps::healthy_before_faults::healthy_before_faults,
        "normal write+exit path should succeed",
        ExpectedOutcome::Exit(0),
    ),
    AppDefinition::new(
        "illegal_instruction",
        apps::illegal_instruction::illegal_instruction,
        "U-mode writes sstatus CSR and should raise Illegal Instruction",
        ExpectedOutcome::Fault(EXC_ILLEGAL_INSTRUCTION),
    ),
    AppDefinition::new(
        "healthy_after_illegal",
        apps::healthy_after_illegal::healthy_after_illegal,
        "system should continue after illegal instruction fault",
        ExpectedOutcome::Exit(0),
    ),
    AppDefinition::new(
        "store_page_fault",
        apps::store_page_fault::store_page_fault,
        "U-mode store to unmapped address 0x0 should raise Store/AMO Page Fault",
        ExpectedOutcome::Fault(EXC_STORE_PAGE_FAULT),
    ),
    AppDefinition::new(
        "healthy_after_store_fault",
        apps::healthy_after_store_fault::healthy_after_store_fault,
        "system should continue after store page fault",
        ExpectedOutcome::Exit(0),
    ),
];

static mut RUNS: [RunRecord; APP_COUNT] = [RunRecord::empty(); APP_COUNT];
static mut CURRENT_APP_INDEX: usize = 0;
static mut EXCEPTION_STATS: ExceptionStats = ExceptionStats::empty();

#[allow(clippy::declare_interior_mutable_const)]
const EMPTY_PAGE_TABLE: PageTable = PageTable([0; 512]);

static mut ROOT_PAGE_TABLE: PageTable = EMPTY_PAGE_TABLE;
static mut LOW_L1_PAGE_TABLE: PageTable = EMPTY_PAGE_TABLE;
static mut LOW_L0_PAGE_TABLE: PageTable = EMPTY_PAGE_TABLE;

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __user_stack_top: u8;
    static __image_end: u8;

    fn enter_supervisor(supervisor_entry: usize) -> !;
    fn enter_user_mode(user_entry: usize, user_sp: usize, kernel_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_machine() -> ! {
    clear_bss();
    configure_pmp();
    delegate_user_traps_to_supervisor();
    unsafe { enter_supervisor(start_supervisor as *const () as usize) }
}

#[no_mangle]
pub extern "C" fn start_supervisor() -> ! {
    setup_page_tables();
    activate_paging();
    trap::init_trap_vector();
    enable_sum();

    println!("[kernel] booted in S-mode");
    println!("[kernel] starting LAB2 kernel task4 exception diagnostics suite");
    println!(
        "[kernel] trap CSRs: scause/sepc/stval will be logged from supervisor trap context"
    );
    println!(
        "[kernel] address layout: kernel identity @ {:#x}, user alias @ {:#x}",
        PHYS_BASE, USER_BASE
    );
    println!(
        "[kernel] exception summary target: Illegal Instruction + Store/AMO Page Fault, then continue running later tasks"
    );

    launch_app(0)
}

pub fn handle_syscall(frame: &mut TrapFrame) {
    match frame.a7 {
        SYS_WRITE => frame.a0 = sys_write(frame.a0 as *const u8, frame.a1) as usize,
        SYS_EXIT => finish_current_app(frame.a0 as i32),
        _ => frame.a0 = ENOSYS as usize,
    }
}

pub fn handle_user_exception(frame: &TrapFrame, scause: usize, stval: usize) -> ! {
    let app = current_app();
    let interrupt = scause >> (usize::BITS as usize - 1);
    let cause_code = scause & !(1usize << (usize::BITS as usize - 1));
    let instruction = read_user_instruction(frame.sepc);

    unsafe {
        let record = &mut RUNS[CURRENT_APP_INDEX];
        record.status = RunStatus::Faulted;
        record.scause = scause;
        record.sepc = frame.sepc;
        record.stval = stval;
        record.instruction = instruction.unwrap_or(0);
        record.instruction_valid = instruction.is_some();
        record.ra = frame.ra;
        record.sp = frame.user_sp;
        record.gp = frame.gp;
        record.s0 = frame.s0;
        record.a0 = frame.a0;
        record.a1 = frame.a1;
        record.a7 = frame.a7;
    }

    update_exception_stats(cause_code);

    println!(
        "[kernel] exception app={} action=kill-and-continue",
        app.name
    );
    println!(
        "[kernel]   scause={:#x} interrupt={} code={} type={}",
        scause,
        interrupt,
        cause_code,
        exception_name(cause_code)
    );
    println!(
        "[kernel]   sepc={:#x} stval={:#x}",
        frame.sepc,
        stval
    );
    match instruction {
        Some(word) => println!("[kernel]   instruction=0x{:08x}", word),
        None => println!("[kernel]   instruction=<unavailable>"),
    }
    println!(
        "[kernel]   regs ra={:#x} sp={:#x} gp={:#x} s0={:#x} a0={:#x} a1={:#x} a7={:#x}",
        frame.ra,
        frame.user_sp,
        frame.gp,
        frame.s0,
        frame.a0,
        frame.a1,
        frame.a7
    );

    advance_to_next_app()
}

fn launch_app(index: usize) -> ! {
    unsafe {
        CURRENT_APP_INDEX = index;
        RUNS[index] = RunRecord::empty();
    }

    let app = current_app();
    println!(
        "[kernel] launch app={} | expected={}",
        app.name, app.expectation
    );

    let user_entry = user_alias(app.entry as usize);
    let user_sp = user_alias(ptr::addr_of!(__user_stack_top) as usize);
    let kernel_sp = ptr::addr_of!(__kernel_stack_top) as usize;

    unsafe { enter_user_mode(user_entry, user_sp, kernel_sp) }
}

fn finish_current_app(code: i32) -> ! {
    unsafe {
        let record = &mut RUNS[CURRENT_APP_INDEX];
        record.status = RunStatus::Exited;
        record.exit_code = code;
        EXCEPTION_STATS.clean_exits += 1;
    }

    advance_to_next_app()
}

fn advance_to_next_app() -> ! {
    let finished_index = unsafe { CURRENT_APP_INDEX };
    print_app_result(finished_index);

    let next_index = finished_index + 1;
    if next_index < APP_COUNT {
        launch_app(next_index)
    } else {
        print_final_report()
    }
}

fn print_app_result(index: usize) {
    let app = APPS[index];
    let record = unsafe { RUNS[index] };

    match record.status {
        RunStatus::Exited => println!(
            "[kernel] result app={} status=exit({})",
            app.name, record.exit_code
        ),
        RunStatus::Faulted => println!(
            "[kernel] result app={} status=fault(type={}, scause={:#x}, sepc={:#x}, stval={:#x})",
            app.name,
            exception_name(record.scause & !(1usize << (usize::BITS as usize - 1))),
            record.scause,
            record.sepc,
            record.stval
        ),
        RunStatus::Pending => println!(
            "[kernel] result app={} status=pending",
            app.name
        ),
    }
}

fn print_final_report() -> ! {
    println!("[kernel] final exception summary:");
    for index in 0..APP_COUNT {
        print_app_result(index);
    }

    let stats = unsafe { EXCEPTION_STATS };
    println!(
        "[kernel] exception_stats illegal_instruction={} store_page_fault={} load_page_fault={} instruction_page_fault={} other_faults={} clean_exits={}",
        stats.illegal_instruction,
        stats.store_page_fault,
        stats.load_page_fault,
        stats.instruction_page_fault,
        stats.other_faults,
        stats.clean_exits
    );

    let healthy_before_ok = matches_expected(0);
    let illegal_ok = matches_expected(1);
    let healthy_after_illegal_ok = matches_expected(2);
    let store_fault_ok = matches_expected(3);
    let healthy_after_store_ok = matches_expected(4);
    let stats_ok = stats.illegal_instruction == 1
        && stats.store_page_fault == 1
        && stats.other_faults == 0
        && stats.clean_exits == 3;

    println!(
        "[kernel] check healthy task before faults exits cleanly: {}",
        pass_fail(healthy_before_ok)
    );
    println!(
        "[kernel] check illegal instruction fault captured with scause/sepc/stval: {}",
        pass_fail(illegal_ok)
    );
    println!(
        "[kernel] check system continues after illegal instruction: {}",
        pass_fail(healthy_after_illegal_ok)
    );
    println!(
        "[kernel] check store page fault captured with scause/sepc/stval: {}",
        pass_fail(store_fault_ok)
    );
    println!(
        "[kernel] check system continues after store page fault: {}",
        pass_fail(healthy_after_store_ok)
    );
    println!(
        "[kernel] check exception counters are consistent: {}",
        pass_fail(stats_ok)
    );

    if healthy_before_ok
        && illegal_ok
        && healthy_after_illegal_ok
        && store_fault_ok
        && healthy_after_store_ok
        && stats_ok
    {
        qemu_exit(0)
    } else {
        qemu_exit(1)
    }
}

fn matches_expected(index: usize) -> bool {
    let app = APPS[index];
    let record = unsafe { RUNS[index] };

    match app.expected_outcome {
        ExpectedOutcome::Exit(code) => record.status == RunStatus::Exited && record.exit_code == code,
        ExpectedOutcome::Fault(expected_cause) => {
            let actual_cause = record.scause & !(1usize << (usize::BITS as usize - 1));
            record.status == RunStatus::Faulted
                && actual_cause == expected_cause
                && record.sepc != 0
        }
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

fn update_exception_stats(cause_code: usize) {
    unsafe {
        match cause_code {
            EXC_ILLEGAL_INSTRUCTION => EXCEPTION_STATS.illegal_instruction += 1,
            EXC_STORE_PAGE_FAULT => EXCEPTION_STATS.store_page_fault += 1,
            EXC_LOAD_PAGE_FAULT => EXCEPTION_STATS.load_page_fault += 1,
            EXC_INST_PAGE_FAULT => EXCEPTION_STATS.instruction_page_fault += 1,
            _ => EXCEPTION_STATS.other_faults += 1,
        }
    }
}

fn read_user_instruction(addr: usize) -> Option<u32> {
    if addr < USER_BASE || addr + 2 > user_memory_end() || addr & 1 != 0 {
        return None;
    }

    unsafe {
        let low = ptr::read_volatile(addr as *const u16) as u32;
        if low & 0b11 != 0b11 {
            Some(low)
        } else if addr + 4 <= user_memory_end() {
            let high = ptr::read_volatile((addr + 2) as *const u16) as u32;
            Some(low | (high << 16))
        } else {
            None
        }
    }
}

fn current_app() -> AppDefinition {
    unsafe { APPS[CURRENT_APP_INDEX] }
}

fn validated_user_bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Ok(&[]);
    }

    let addr = ptr as usize;
    if !user_range_valid(addr, len) {
        return Err(EFAULT);
    }

    unsafe { Ok(core::slice::from_raw_parts(ptr, len)) }
}

fn user_range_valid(addr: usize, len: usize) -> bool {
    if len == 0 {
        return true;
    }

    let end = match addr.checked_add(len) {
        Some(end) => end,
        None => return false,
    };

    addr >= USER_BASE && end <= user_memory_end()
}

fn user_memory_end() -> usize {
    user_alias(ptr::addr_of!(__user_stack_top) as usize)
}

fn user_alias(phys_addr: usize) -> usize {
    USER_BASE + (phys_addr - PHYS_BASE)
}

fn setup_page_tables() {
    unsafe {
        ROOT_PAGE_TABLE = EMPTY_PAGE_TABLE;
        LOW_L1_PAGE_TABLE = EMPTY_PAGE_TABLE;
        LOW_L0_PAGE_TABLE = EMPTY_PAGE_TABLE;

        ROOT_PAGE_TABLE.0[(PHYS_BASE >> 30) & 0x1ff] =
            leaf_pte(PHYS_BASE, PTE_R | PTE_W | PTE_X | PTE_G);
        ROOT_PAGE_TABLE.0[(USER_BASE >> 30) & 0x1ff] =
            leaf_pte(PHYS_BASE, PTE_R | PTE_W | PTE_X | PTE_U);

        ROOT_PAGE_TABLE.0[0] = table_pte(ptr::addr_of!(LOW_L1_PAGE_TABLE) as usize);
        LOW_L1_PAGE_TABLE.0[(UART_BASE >> 21) & 0x1ff] =
            leaf_pte(UART_BASE, PTE_R | PTE_W | PTE_G);
        LOW_L1_PAGE_TABLE.0[0] = table_pte(ptr::addr_of!(LOW_L0_PAGE_TABLE) as usize);
        LOW_L0_PAGE_TABLE.0[(QEMU_TEST_BASE >> 12) & 0x1ff] =
            leaf_pte(QEMU_TEST_BASE, PTE_R | PTE_W | PTE_G);
    }
}

fn activate_paging() {
    let root_ppn = (ptr::addr_of!(ROOT_PAGE_TABLE) as usize) >> 12;
    let satp = SATP_MODE_SV39 | root_ppn;

    unsafe {
        asm!(
            "sfence.vma zero, zero",
            "csrw satp, {}",
            "sfence.vma zero, zero",
            in(reg) satp,
            options(nostack)
        );
    }
}

fn enable_sum() {
    let mut sstatus: usize;

    unsafe {
        asm!("csrr {}, sstatus", out(reg) sstatus, options(nostack, nomem));
        sstatus |= SSTATUS_SUM;
        asm!("csrw sstatus, {}", in(reg) sstatus, options(nostack, nomem));
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

fn delegate_user_traps_to_supervisor() {
    let medeleg = (1usize << EXC_ILLEGAL_INSTRUCTION)
        | (1usize << EXC_INST_PAGE_FAULT)
        | (1usize << EXC_LOAD_PAGE_FAULT)
        | (1usize << EXC_STORE_PAGE_FAULT)
        | (1usize << EXC_ECALL_FROM_U);

    unsafe {
        asm!("csrw medeleg, {}", in(reg) medeleg, options(nostack, nomem));
        asm!("csrw mideleg, zero", options(nostack, nomem));
    }
}

fn table_pte(next_table: usize) -> u64 {
    (((next_table as u64) >> 12) << 10) | PTE_V
}

fn leaf_pte(phys_addr: usize, flags: u64) -> u64 {
    (((phys_addr as u64) >> 12) << 10) | flags | PTE_V | PTE_A | PTE_D
}

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
    }
}

fn exception_name(code: usize) -> &'static str {
    match code {
        EXC_ILLEGAL_INSTRUCTION => "Illegal Instruction",
        EXC_LOAD_PAGE_FAULT => "Load Page Fault",
        EXC_STORE_PAGE_FAULT => "Store/AMO Page Fault",
        EXC_INST_PAGE_FAULT => "Instruction Page Fault",
        EXC_ECALL_FROM_U => "Environment Call from U-mode",
        _ => "Other Exception",
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
        core::hint::spin_loop();
    }
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
