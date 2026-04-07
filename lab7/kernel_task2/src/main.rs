#![no_std]
#![no_main]

mod console;
mod trap;

use core::arch::{asm, global_asm};
use core::fmt;
use core::panic::PanicInfo;
use core::ptr;
use core::slice;

use trap::TrapFrame;

global_asm!(include_str!("boot.S"));

const PAGE_SIZE: usize = 4096;
const PAGE_SHIFT: usize = 12;
const PAGE_TABLE_ENTRIES: usize = 512;
const MEGA_PAGE_SIZE: usize = 1 << 21;

const QEMU_TEST_BASE: usize = 0x0010_0000;
const UART0_ADDR: usize = 0x1000_0000;

const KERNEL_BASE: usize = 0x8000_0000;
const KERNEL_WINDOW_SIZE: usize = 16 * 1024 * 1024;

const USER_TEXT_VA: usize = 0x0040_0000;
const USER_DATA_VA: usize = 0x0040_1000;
const USER_STACK_VA: usize = 0x0040_3000;
const USER_STACK_TOP: usize = 0x0040_4000;
const USER_SIGNAL_FRAME_SIZE: usize = 16;

const SATP_MODE_SV39: usize = 8usize << 60;

const PTE_V: usize = 1 << 0;
const PTE_R: usize = 1 << 1;
const PTE_W: usize = 1 << 2;
const PTE_X: usize = 1 << 3;
const PTE_U: usize = 1 << 4;
const PTE_A: usize = 1 << 6;
const PTE_D: usize = 1 << 7;

const SSTATUS_SUM: usize = 1 << 18;

const SYS_SIGACTION: usize = 1;
const SYS_KILL: usize = 2;
const SYS_YIELD: usize = 3;
const SYS_REPORT: usize = 4;
const SYS_EXIT: usize = 5;
const SYS_SIGRETURN: usize = 6;

const REPORT_HANDLER: usize = 1;
const REPORT_MAIN: usize = 2;

const USER_ENV_CALL: usize = 8;
const LOAD_PAGE_FAULT: usize = 13;
const STORE_PAGE_FAULT: usize = 15;
const MEDELEG_MASK: usize =
    (1 << USER_ENV_CALL) | (1 << LOAD_PAGE_FAULT) | (1 << STORE_PAGE_FAULT);

const MAX_PROCS: usize = 2;
const PID_RECEIVER: usize = 0;
const PID_SENDER: usize = 1;

const PROC_UNUSED: usize = 0;
const PROC_RUNNABLE: usize = 1;
const PROC_EXITED: usize = 2;

const MAX_SIGNALS: usize = 32;
const SIGUSR1: usize = 10;

#[repr(align(4096))]
#[derive(Clone, Copy)]
struct PageTable {
    entries: [usize; PAGE_TABLE_ENTRIES],
}

impl PageTable {
    const fn zeroed() -> Self {
        Self {
            entries: [0; PAGE_TABLE_ENTRIES],
        }
    }
}

#[repr(align(4096))]
#[derive(Clone, Copy)]
struct Page {
    bytes: [u8; PAGE_SIZE],
}

impl Page {
    const fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE],
        }
    }
}

#[derive(Clone, Copy)]
struct WalkResult {
    vpn2: usize,
    vpn1: usize,
    vpn0: usize,
    root_pte: usize,
    l1_pte: usize,
    l0_pte: usize,
    leaf_pte: usize,
}

impl WalkResult {
    const fn zeroed() -> Self {
        Self {
            vpn2: 0,
            vpn1: 0,
            vpn0: 0,
            root_pte: 0,
            l1_pte: 0,
            l0_pte: 0,
            leaf_pte: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct ProcessControl {
    state: usize,
    satp: usize,
    exit_code: usize,
    pending_mask: u32,
    signal_active: bool,
    handlers: [usize; MAX_SIGNALS],
    saved_frame: TrapFrame,
}

impl ProcessControl {
    const fn empty() -> Self {
        Self {
            state: PROC_UNUSED,
            satp: 0,
            exit_code: 0,
            pending_mask: 0,
            signal_active: false,
            handlers: [0; MAX_SIGNALS],
            saved_frame: TrapFrame::zeroed(),
        }
    }
}

#[derive(Clone, Copy)]
struct ProcessReport {
    handler_reports: usize,
    main_reports: usize,
    handler_state: usize,
    main_seen_state: usize,
    final_state: usize,
    last_signal: usize,
    handler_sp: usize,
    handler_ra: usize,
}

impl ProcessReport {
    const fn empty() -> Self {
        Self {
            handler_reports: 0,
            main_reports: 0,
            handler_state: 0,
            main_seen_state: 0,
            final_state: 0,
            last_signal: 0,
            handler_sp: 0,
            handler_ra: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct SignalStats {
    registrations: usize,
    pending_marks: usize,
    dispatches: usize,
    sigreturns: usize,
    saved_epc: usize,
    saved_sp: usize,
    handler_epc: usize,
    trampoline_va: usize,
    stacked_ra: usize,
    stacked_epc: usize,
    restored_epc: usize,
    restored_sp: usize,
}

impl SignalStats {
    const fn zeroed() -> Self {
        Self {
            registrations: 0,
            pending_marks: 0,
            dispatches: 0,
            sigreturns: 0,
            saved_epc: 0,
            saved_sp: 0,
            handler_epc: 0,
            trampoline_va: 0,
            stacked_ra: 0,
            stacked_epc: 0,
            restored_epc: 0,
            restored_sp: 0,
        }
    }
}

struct PteFlags(usize);

impl fmt::Display for PteFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pte = self.0;
        let chars = [
            if (pte & PTE_V) != 0 { 'V' } else { '-' },
            if (pte & PTE_R) != 0 { 'R' } else { '-' },
            if (pte & PTE_W) != 0 { 'W' } else { '-' },
            if (pte & PTE_X) != 0 { 'X' } else { '-' },
            if (pte & PTE_U) != 0 { 'U' } else { '-' },
            if (pte & PTE_A) != 0 { 'A' } else { '-' },
            if (pte & PTE_D) != 0 { 'D' } else { '-' },
        ];
        for ch in chars {
            write!(f, "{ch}")?;
        }
        Ok(())
    }
}

static mut DEV_L0_PAGE_TABLE: PageTable = PageTable::zeroed();
static mut KERNEL_L1_PAGE_TABLE: PageTable = PageTable::zeroed();
static mut PROC_ROOT_TABLES: [PageTable; MAX_PROCS] = [PageTable::zeroed(); MAX_PROCS];
static mut PROC_LOW_L1_TABLES: [PageTable; MAX_PROCS] = [PageTable::zeroed(); MAX_PROCS];
static mut PROC_USER_L0_TABLES: [PageTable; MAX_PROCS] = [PageTable::zeroed(); MAX_PROCS];
static mut USER_CODE_PAGE: Page = Page::zeroed();
static mut USER_DATA_PAGES: [Page; MAX_PROCS] = [Page::zeroed(); MAX_PROCS];
static mut USER_STACK_PAGES: [Page; MAX_PROCS] = [Page::zeroed(); MAX_PROCS];

static mut PROCESS_FRAMES: [TrapFrame; MAX_PROCS] = [TrapFrame::zeroed(); MAX_PROCS];
static mut PROCESS_TABLE: [ProcessControl; MAX_PROCS] = [ProcessControl::empty(); MAX_PROCS];
static mut PROCESS_REPORTS: [ProcessReport; MAX_PROCS] = [ProcessReport::empty(); MAX_PROCS];
static mut CURRENT_PID: usize = PID_RECEIVER;
static mut SIGNAL_STATS: SignalStats = SignalStats::zeroed();

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __supervisor_trap_stack_top: u8;
    static __machine_trap_stack_top: u8;
    static __user_program_start: u8;
    static __user_program_end: u8;
    static __receiver_resume_point: u8;
    static __sigreturn_trampoline: u8;
    static __user_signal_handler: u8;

    fn enter_supervisor(supervisor_entry: usize, supervisor_sp: usize) -> !;
    fn enter_user_task(frame: *const TrapFrame, trap_sp: usize) -> !;
}

#[no_mangle]
pub extern "C" fn start_machine() -> ! {
    clear_bss();
    configure_pmp();
    trap::init_machine_trap_vector();
    delegate_user_exceptions_to_supervisor();

    unsafe {
        asm!(
            "csrw mscratch, {}",
            in(reg) machine_trap_stack_top(),
            options(nostack, nomem)
        );
        enter_supervisor(start_supervisor as *const () as usize, kernel_stack_top())
    }
}

#[no_mangle]
pub extern "C" fn start_supervisor() -> ! {
    trap::init_supervisor_trap_vector();
    clear_sum();
    initialize_runtime();

    println!("[kernel] booted in S-mode with Sv39 enabled");
    println!("[kernel] LAB7 kernel task2 signal delivery and sigreturn");
    println!(
        "[kernel] model: receiver pid=0 registers SIGUSR1 handler, sender pid=1 marks pending signal, kernel rewrites trap frame before returning to user"
    );
    println!(
        "[kernel] expected user symbols: handler={:#x} resume={:#x} trampoline={:#x}",
        expected_handler_va(),
        expected_resume_va(),
        expected_trampoline_va()
    );
    print_walk("receiver_text", PID_RECEIVER, USER_TEXT_VA);
    print_walk("receiver_data", PID_RECEIVER, USER_DATA_VA);
    print_walk("receiver_stack", PID_RECEIVER, USER_STACK_VA);

    unsafe {
        asm!(
            "csrw sscratch, {}",
            in(reg) supervisor_trap_stack_top(),
            options(nostack, nomem)
        );
        CURRENT_PID = PID_RECEIVER;
        activate_process(PID_RECEIVER);
        enter_user_task(
            ptr::addr_of!(PROCESS_FRAMES[PID_RECEIVER]),
            supervisor_trap_stack_top(),
        )
    }
}

#[no_mangle]
pub extern "C" fn handle_machine_trap(frame: &mut TrapFrame) {
    println!(
        "[kernel] unexpected machine trap: mcause={:#x} mepc={:#x} mtval={:#x}",
        read_mcause(),
        frame.epc,
        read_mtval()
    );
    qemu_exit(1);
}

#[no_mangle]
pub extern "C" fn handle_supervisor_trap(frame: &mut TrapFrame) {
    let pid = unsafe { CURRENT_PID };
    let scause = read_scause();
    let stval = read_stval();

    unsafe {
        PROCESS_FRAMES[pid] = *frame;
    }

    let next = match scause {
        USER_ENV_CALL => handle_user_ecall(pid),
        _ => {
            println!(
                "[kernel] unexpected supervisor trap: pid={} scause={:#x} sepc={:#x} stval={:#x}",
                pid, scause, frame.epc, stval
            );
            qemu_exit(1);
        }
    };

    if let Some(next_pid) = next {
        unsafe {
            CURRENT_PID = next_pid;
        }
        dispatch_pending_signal_if_needed(next_pid);
        unsafe {
            activate_process(next_pid);
            *frame = PROCESS_FRAMES[next_pid];
        }
        return;
    }

    finish_experiment();
}

fn initialize_runtime() {
    clear_page_table(ptr::addr_of_mut!(DEV_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(KERNEL_L1_PAGE_TABLE));
    zero_page(ptr::addr_of_mut!(USER_CODE_PAGE));

    for pid in 0..MAX_PROCS {
        clear_page_table(proc_root_table_ptr(pid));
        clear_page_table(proc_low_l1_table_ptr(pid));
        clear_page_table(proc_user_l0_table_ptr(pid));
        zero_page(user_data_page_ptr(pid));
        zero_page(user_stack_page_ptr(pid));
        unsafe {
            PROCESS_FRAMES[pid] = TrapFrame::zeroed();
            PROCESS_TABLE[pid] = ProcessControl::empty();
            PROCESS_REPORTS[pid] = ProcessReport::empty();
        }
    }

    unsafe {
        CURRENT_PID = PID_RECEIVER;
        SIGNAL_STATS = SignalStats::zeroed();
    }

    copy_user_program();
    build_global_kernel_tables();
    build_process(PID_RECEIVER);
    build_process(PID_SENDER);
}

fn copy_user_program() {
    let program_bytes =
        unsafe { slice::from_raw_parts(ptr::addr_of!(__user_program_start), user_program_len()) };

    unsafe {
        ptr::copy_nonoverlapping(
            program_bytes.as_ptr(),
            ptr::addr_of_mut!(USER_CODE_PAGE.bytes).cast::<u8>(),
            program_bytes.len(),
        );
    }
}

fn build_global_kernel_tables() {
    unsafe {
        DEV_L0_PAGE_TABLE.entries[vpn0_index(QEMU_TEST_BASE)] =
            leaf_pte(QEMU_TEST_BASE, PTE_R | PTE_W | PTE_A | PTE_D);

        for entry_index in 0..(KERNEL_WINDOW_SIZE / MEGA_PAGE_SIZE) {
            let pa = KERNEL_BASE + entry_index * MEGA_PAGE_SIZE;
            KERNEL_L1_PAGE_TABLE.entries[entry_index] =
                leaf_pte(pa, PTE_R | PTE_W | PTE_X | PTE_A | PTE_D);
        }
    }
}

fn build_process(pid: usize) {
    clear_page_table(proc_root_table_ptr(pid));
    clear_page_table(proc_low_l1_table_ptr(pid));
    clear_page_table(proc_user_l0_table_ptr(pid));

    unsafe {
        PROC_ROOT_TABLES[pid].entries[vpn2_index(KERNEL_BASE)] = table_pte(kernel_l1_table_pa());
        PROC_ROOT_TABLES[pid].entries[vpn2_index(USER_TEXT_VA)] =
            table_pte(proc_low_l1_table_pa(pid));

        PROC_LOW_L1_TABLES[pid].entries[vpn1_index(QEMU_TEST_BASE)] = table_pte(dev_l0_table_pa());
        PROC_LOW_L1_TABLES[pid].entries[vpn1_index(UART0_ADDR)] =
            leaf_pte(UART0_ADDR, PTE_R | PTE_W | PTE_A | PTE_D);
        PROC_LOW_L1_TABLES[pid].entries[vpn1_index(USER_TEXT_VA)] =
            table_pte(proc_user_l0_table_pa(pid));

        PROC_USER_L0_TABLES[pid].entries[vpn0_index(USER_TEXT_VA)] =
            leaf_pte(user_code_page_pa(), PTE_R | PTE_X | PTE_U | PTE_A);
        PROC_USER_L0_TABLES[pid].entries[vpn0_index(USER_DATA_VA)] =
            leaf_pte(user_data_page_pa(pid), PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);
        PROC_USER_L0_TABLES[pid].entries[vpn0_index(USER_STACK_VA)] =
            leaf_pte(user_stack_page_pa(pid), PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);

        PROCESS_TABLE[pid].state = PROC_RUNNABLE;
        PROCESS_TABLE[pid].satp = satp_for_root(proc_root_table_pa(pid));
        PROCESS_TABLE[pid].exit_code = 0;
        PROCESS_TABLE[pid].pending_mask = 0;
        PROCESS_TABLE[pid].signal_active = false;
        PROCESS_TABLE[pid].handlers = [0; MAX_SIGNALS];
        PROCESS_TABLE[pid].saved_frame = TrapFrame::zeroed();

        PROCESS_FRAMES[pid] = TrapFrame::zeroed();
        PROCESS_FRAMES[pid].epc = USER_TEXT_VA;
        PROCESS_FRAMES[pid].saved_sp = USER_STACK_TOP;
        PROCESS_FRAMES[pid].a0 = pid;
    }
}

fn handle_user_ecall(pid: usize) -> Option<usize> {
    let syscall_nr = unsafe { PROCESS_FRAMES[pid].a7 };

    unsafe {
        PROCESS_FRAMES[pid].epc += 4;
    }

    match syscall_nr {
        SYS_SIGACTION => {
            do_sigaction(pid);
            Some(pid)
        }
        SYS_KILL => {
            do_kill(pid);
            Some(pid)
        }
        SYS_YIELD => Some(other_runnable_or_self(pid)),
        SYS_REPORT => {
            store_process_report(pid);
            Some(pid)
        }
        SYS_EXIT => {
            unsafe {
                PROCESS_TABLE[pid].state = PROC_EXITED;
                PROCESS_TABLE[pid].exit_code = PROCESS_FRAMES[pid].a0;
            }
            next_runnable_after_exit()
        }
        SYS_SIGRETURN => do_sigreturn(pid),
        _ => {
            println!(
                "[kernel] unexpected syscall: pid={} a7={} a0={:#x}",
                pid,
                syscall_nr,
                unsafe { PROCESS_FRAMES[pid].a0 }
            );
            qemu_exit(1);
        }
    }
}

fn do_sigaction(pid: usize) {
    let handler = unsafe { PROCESS_FRAMES[pid].a0 };
    let signum = unsafe { PROCESS_FRAMES[pid].a1 };

    if signum >= MAX_SIGNALS {
        kernel_fail("sigaction signum out of range");
    }

    unsafe {
        PROCESS_TABLE[pid].handlers[signum] = handler;
        PROCESS_FRAMES[pid].a0 = 0;
        SIGNAL_STATS.registrations += 1;
    }

    println!(
        "[kernel] sigaction pid={} signum={} handler={:#x}",
        pid, signum, handler
    );
}

fn do_kill(pid: usize) {
    let target = unsafe { PROCESS_FRAMES[pid].a0 };
    let signum = unsafe { PROCESS_FRAMES[pid].a1 };

    if target >= MAX_PROCS || signum >= MAX_SIGNALS {
        kernel_fail("kill target or signum out of range");
    }

    unsafe {
        PROCESS_TABLE[target].pending_mask |= 1u32 << signum;
        PROCESS_FRAMES[pid].a0 = 0;
        SIGNAL_STATS.pending_marks += 1;
    }

    println!(
        "[kernel] kill sender_pid={} target_pid={} signum={} pending_mask={:#x}",
        pid,
        target,
        signum,
        unsafe { PROCESS_TABLE[target].pending_mask }
    );
}

fn do_sigreturn(pid: usize) -> Option<usize> {
    let (saved_frame, signal_active) = unsafe {
        (
            PROCESS_TABLE[pid].saved_frame,
            PROCESS_TABLE[pid].signal_active,
        )
    };

    if !signal_active {
        kernel_fail("sigreturn without active signal frame");
    }

    let signal_sp = unsafe { PROCESS_FRAMES[pid].saved_sp };
    let stacked_ra = read_user_u64(pid, signal_sp) as usize;
    let stacked_epc = read_user_u64(pid, signal_sp + 8) as usize;

    unsafe {
        PROCESS_TABLE[pid].signal_active = false;
        PROCESS_FRAMES[pid] = saved_frame;
        SIGNAL_STATS.sigreturns += 1;
        SIGNAL_STATS.stacked_ra = stacked_ra;
        SIGNAL_STATS.stacked_epc = stacked_epc;
        SIGNAL_STATS.restored_epc = saved_frame.epc;
        SIGNAL_STATS.restored_sp = saved_frame.saved_sp;
    }

    println!(
        "[kernel] sigreturn pid={} stacked_ra={:#x} stacked_epc={:#x} restore_epc={:#x} restore_sp={:#x}",
        pid,
        stacked_ra,
        stacked_epc,
        saved_frame.epc,
        saved_frame.saved_sp
    );
    Some(pid)
}

fn store_process_report(pid: usize) {
    let phase = unsafe { PROCESS_FRAMES[pid].a0 };

    match phase {
        REPORT_HANDLER => unsafe {
            PROCESS_REPORTS[pid].handler_reports += 1;
            PROCESS_REPORTS[pid].handler_state = PROCESS_FRAMES[pid].a1;
            PROCESS_REPORTS[pid].last_signal = PROCESS_FRAMES[pid].a2;
            PROCESS_REPORTS[pid].handler_sp = PROCESS_FRAMES[pid].a3;
            PROCESS_REPORTS[pid].handler_ra = PROCESS_FRAMES[pid].a4;
            println!(
                "[kernel] handler report pid={} state={} signum={} sp={:#x} ra={:#x}",
                pid,
                PROCESS_REPORTS[pid].handler_state,
                PROCESS_REPORTS[pid].last_signal,
                PROCESS_REPORTS[pid].handler_sp,
                PROCESS_REPORTS[pid].handler_ra
            );
        },
        REPORT_MAIN => unsafe {
            PROCESS_REPORTS[pid].main_reports += 1;
            PROCESS_REPORTS[pid].main_seen_state = PROCESS_FRAMES[pid].a1;
            PROCESS_REPORTS[pid].final_state = PROCESS_FRAMES[pid].a2;
            PROCESS_REPORTS[pid].last_signal = PROCESS_FRAMES[pid].a3;
            println!(
                "[kernel] main report pid={} seen_state={} final_state={} signum={}",
                pid,
                PROCESS_REPORTS[pid].main_seen_state,
                PROCESS_REPORTS[pid].final_state,
                PROCESS_REPORTS[pid].last_signal
            );
        },
        _ => kernel_fail("unknown report phase"),
    }
}

fn dispatch_pending_signal_if_needed(pid: usize) {
    let pending_mask = unsafe { PROCESS_TABLE[pid].pending_mask };
    let signal_active = unsafe { PROCESS_TABLE[pid].signal_active };

    if pending_mask == 0 || signal_active {
        return;
    }

    let signum = pending_mask.trailing_zeros() as usize;
    let handler = unsafe { PROCESS_TABLE[pid].handlers[signum] };

    if handler == 0 {
        kernel_fail("pending signal has no registered handler");
    }

    let saved_frame = unsafe { PROCESS_FRAMES[pid] };
    let new_sp = align_down(saved_frame.saved_sp - USER_SIGNAL_FRAME_SIZE, 16);
    let trampoline = expected_trampoline_va();

    write_user_u64(pid, new_sp, trampoline as u64);
    write_user_u64(pid, new_sp + 8, saved_frame.epc as u64);

    unsafe {
        PROCESS_TABLE[pid].pending_mask &= !(1u32 << signum);
        PROCESS_TABLE[pid].signal_active = true;
        PROCESS_TABLE[pid].saved_frame = saved_frame;

        PROCESS_FRAMES[pid].saved_sp = new_sp;
        PROCESS_FRAMES[pid].ra = trampoline;
        PROCESS_FRAMES[pid].a0 = signum;
        PROCESS_FRAMES[pid].epc = handler;

        SIGNAL_STATS.dispatches += 1;
        SIGNAL_STATS.saved_epc = saved_frame.epc;
        SIGNAL_STATS.saved_sp = saved_frame.saved_sp;
        SIGNAL_STATS.handler_epc = handler;
        SIGNAL_STATS.trampoline_va = trampoline;
    }

    println!(
        "[kernel] dispatch signal pid={} signum={} saved_epc={:#x} saved_sp={:#x} handler_epc={:#x} new_sp={:#x} trampoline={:#x}",
        pid,
        signum,
        saved_frame.epc,
        saved_frame.saved_sp,
        handler,
        new_sp,
        trampoline
    );
}

fn finish_experiment() -> ! {
    let receiver_report = unsafe { PROCESS_REPORTS[PID_RECEIVER] };
    let signal_stats = unsafe { SIGNAL_STATS };
    let receiver_data = read_user_u64(PID_RECEIVER, USER_DATA_VA) as usize;
    let recorded_signal = read_user_u64(PID_RECEIVER, USER_DATA_VA + 8) as usize;

    println!(
        "[kernel] final signal stats: registrations={} pending_marks={} dispatches={} sigreturns={}",
        signal_stats.registrations,
        signal_stats.pending_marks,
        signal_stats.dispatches,
        signal_stats.sigreturns
    );
    println!(
        "[kernel] context rewrite saved_epc={:#x} saved_sp={:#x} handler_epc={:#x} trampoline={:#x} stacked_ra={:#x} stacked_epc={:#x} restored_epc={:#x} restored_sp={:#x}",
        signal_stats.saved_epc,
        signal_stats.saved_sp,
        signal_stats.handler_epc,
        signal_stats.trampoline_va,
        signal_stats.stacked_ra,
        signal_stats.stacked_epc,
        signal_stats.restored_epc,
        signal_stats.restored_sp
    );
    println!(
        "[kernel] receiver report handler_runs={} handler_state={} main_seen_state={} final_state={} last_signal={} data={} recorded_signal={}",
        receiver_report.handler_reports,
        receiver_report.handler_state,
        receiver_report.main_seen_state,
        receiver_report.final_state,
        receiver_report.last_signal,
        receiver_data,
        recorded_signal
    );

    let pending_ok = signal_stats.registrations == 1
        && signal_stats.pending_marks == 1
        && signal_stats.dispatches == 1
        && unsafe { PROCESS_TABLE[PID_RECEIVER].pending_mask } == 0;

    let rewrite_ok = signal_stats.saved_epc == expected_resume_va()
        && signal_stats.handler_epc == expected_handler_va()
        && signal_stats.trampoline_va == expected_trampoline_va()
        && signal_stats.stacked_ra == expected_trampoline_va()
        && signal_stats.stacked_epc == expected_resume_va()
        && signal_stats.restored_epc == expected_resume_va()
        && signal_stats.restored_sp == USER_STACK_TOP;

    let flow_ok = signal_stats.sigreturns == 1
        && unsafe { PROCESS_TABLE[PID_RECEIVER].signal_active } == false
        && unsafe { PROCESS_TABLE[PID_RECEIVER].exit_code } == 0
        && unsafe { PROCESS_TABLE[PID_SENDER].exit_code } == 0
        && receiver_report.handler_reports == 1
        && receiver_report.main_reports == 1
        && receiver_report.handler_state == 2
        && receiver_report.main_seen_state == 2
        && receiver_report.final_state == 3
        && receiver_report.last_signal == SIGUSR1
        && receiver_report.handler_ra == expected_trampoline_va()
        && receiver_data == 3
        && recorded_signal == SIGUSR1;

    println!(
        "[kernel] acceptance pending signal is marked then checked before user return: {}",
        pass_fail(pending_ok)
    );
    println!(
        "[kernel] acceptance kernel rewrites user trap context and restores it via sigreturn: {}",
        pass_fail(rewrite_ok)
    );
    println!(
        "[kernel] acceptance user flow runs normal -> handler -> sigreturn -> normal without crash: {}",
        pass_fail(flow_ok)
    );

    qemu_exit(if pending_ok && rewrite_ok && flow_ok {
        0
    } else {
        1
    })
}

fn other_runnable_or_self(pid: usize) -> usize {
    let other = other_pid(pid);
    if unsafe { PROCESS_TABLE[other].state } == PROC_RUNNABLE {
        other
    } else {
        pid
    }
}

fn next_runnable_after_exit() -> Option<usize> {
    for pid in 0..MAX_PROCS {
        if unsafe { PROCESS_TABLE[pid].state } == PROC_RUNNABLE {
            return Some(pid);
        }
    }
    None
}

fn other_pid(pid: usize) -> usize {
    if pid == PID_RECEIVER {
        PID_SENDER
    } else {
        PID_RECEIVER
    }
}

fn activate_process(pid: usize) {
    let satp = unsafe { PROCESS_TABLE[pid].satp };
    unsafe {
        asm!("csrw satp, {}", in(reg) satp, options(nostack, nomem));
        asm!("sfence.vma zero, zero", options(nostack, nomem));
    }
}

fn read_user_u64(pid: usize, va: usize) -> u64 {
    let walk = walk_proc_virtual(pid, va);
    if !pte_is_valid(walk.leaf_pte) {
        kernel_fail("read_user_u64 on unmapped virtual address");
    }
    let pa = pte_to_pa(walk.leaf_pte) + page_offset(va);
    unsafe { ptr::read_volatile(pa as *const u64) }
}

fn write_user_u64(pid: usize, va: usize, value: u64) {
    let walk = walk_proc_virtual(pid, va);
    if !pte_is_valid(walk.leaf_pte) || !pte_has_write(walk.leaf_pte) {
        kernel_fail("write_user_u64 on unwritable virtual address");
    }
    let pa = pte_to_pa(walk.leaf_pte) + page_offset(va);
    unsafe {
        ptr::write_volatile(pa as *mut u64, value);
    }
}

fn print_walk(label: &str, pid: usize, va: usize) {
    let walk = walk_proc_virtual(pid, va);
    println!(
        "[pt] {} pid={} va={:#x} vpn=({},{},{}) root_pte={:#018x} l1_pte={:#018x} l0_pte={:#018x} leaf_pa={:#x} flags={}",
        label,
        pid,
        va,
        walk.vpn2,
        walk.vpn1,
        walk.vpn0,
        walk.root_pte,
        walk.l1_pte,
        walk.l0_pte,
        pte_to_pa(walk.leaf_pte),
        PteFlags(walk.leaf_pte)
    );
}

fn walk_proc_virtual(pid: usize, va: usize) -> WalkResult {
    let mut result = WalkResult::zeroed();
    let root = proc_root_table_ptr(pid);

    result.vpn2 = vpn2_index(va);
    result.vpn1 = vpn1_index(va);
    result.vpn0 = vpn0_index(va);
    result.root_pte = unsafe { (*root).entries[result.vpn2] };

    if !pte_is_valid(result.root_pte) || pte_is_leaf(result.root_pte) {
        result.leaf_pte = result.root_pte;
        return result;
    }

    let l1 = unsafe { &*(pte_to_pa(result.root_pte) as *const PageTable) };
    result.l1_pte = l1.entries[result.vpn1];

    if !pte_is_valid(result.l1_pte) || pte_is_leaf(result.l1_pte) {
        result.leaf_pte = result.l1_pte;
        return result;
    }

    let l0 = unsafe { &*(pte_to_pa(result.l1_pte) as *const PageTable) };
    result.l0_pte = l0.entries[result.vpn0];
    result.leaf_pte = result.l0_pte;
    result
}

fn pte_is_valid(pte: usize) -> bool {
    (pte & PTE_V) != 0
}

fn pte_is_leaf(pte: usize) -> bool {
    (pte & (PTE_R | PTE_W | PTE_X)) != 0
}

fn pte_has_write(pte: usize) -> bool {
    (pte & PTE_W) != 0
}

fn pte_to_pa(pte: usize) -> usize {
    ((pte >> 10) << PAGE_SHIFT) as usize
}

fn leaf_pte(pa: usize, flags: usize) -> usize {
    ((pa >> PAGE_SHIFT) << 10) | flags | PTE_V
}

fn table_pte(pa: usize) -> usize {
    ((pa >> PAGE_SHIFT) << 10) | PTE_V
}

fn satp_for_root(root_pa: usize) -> usize {
    SATP_MODE_SV39 | (root_pa >> PAGE_SHIFT)
}

fn vpn2_index(va: usize) -> usize {
    (va >> 30) & 0x1ff
}

fn vpn1_index(va: usize) -> usize {
    (va >> 21) & 0x1ff
}

fn vpn0_index(va: usize) -> usize {
    (va >> 12) & 0x1ff
}

fn page_offset(va: usize) -> usize {
    va & (PAGE_SIZE - 1)
}

fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

fn clear_page_table(table: *mut PageTable) {
    for entry_index in 0..PAGE_TABLE_ENTRIES {
        unsafe {
            (*table).entries[entry_index] = 0;
        }
    }
}

fn zero_page(page: *mut Page) {
    unsafe {
        ptr::write_bytes((*page).bytes.as_mut_ptr(), 0, PAGE_SIZE);
    }
}

fn delegate_user_exceptions_to_supervisor() {
    unsafe {
        asm!("csrw medeleg, {}", in(reg) MEDELEG_MASK, options(nostack, nomem));
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

fn clear_sum() {
    unsafe {
        asm!("csrc sstatus, {}", in(reg) SSTATUS_SUM, options(nostack, nomem));
    }
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

fn proc_root_table_ptr(pid: usize) -> *mut PageTable {
    unsafe { ptr::addr_of_mut!(PROC_ROOT_TABLES[pid]) }
}

fn proc_low_l1_table_ptr(pid: usize) -> *mut PageTable {
    unsafe { ptr::addr_of_mut!(PROC_LOW_L1_TABLES[pid]) }
}

fn proc_user_l0_table_ptr(pid: usize) -> *mut PageTable {
    unsafe { ptr::addr_of_mut!(PROC_USER_L0_TABLES[pid]) }
}

fn proc_root_table_pa(pid: usize) -> usize {
    unsafe { ptr::addr_of!(PROC_ROOT_TABLES[pid]) as usize }
}

fn proc_low_l1_table_pa(pid: usize) -> usize {
    unsafe { ptr::addr_of!(PROC_LOW_L1_TABLES[pid]) as usize }
}

fn proc_user_l0_table_pa(pid: usize) -> usize {
    unsafe { ptr::addr_of!(PROC_USER_L0_TABLES[pid]) as usize }
}

fn dev_l0_table_pa() -> usize {
    ptr::addr_of!(DEV_L0_PAGE_TABLE) as usize
}

fn kernel_l1_table_pa() -> usize {
    ptr::addr_of!(KERNEL_L1_PAGE_TABLE) as usize
}

fn user_code_page_pa() -> usize {
    ptr::addr_of!(USER_CODE_PAGE) as usize
}

fn user_data_page_ptr(pid: usize) -> *mut Page {
    unsafe { ptr::addr_of_mut!(USER_DATA_PAGES[pid]) }
}

fn user_stack_page_ptr(pid: usize) -> *mut Page {
    unsafe { ptr::addr_of_mut!(USER_STACK_PAGES[pid]) }
}

fn user_data_page_pa(pid: usize) -> usize {
    unsafe { ptr::addr_of!(USER_DATA_PAGES[pid]) as usize }
}

fn user_stack_page_pa(pid: usize) -> usize {
    unsafe { ptr::addr_of!(USER_STACK_PAGES[pid]) as usize }
}

fn user_program_len() -> usize {
    (ptr::addr_of!(__user_program_end) as usize) - (ptr::addr_of!(__user_program_start) as usize)
}

fn user_symbol_va(symbol: *const u8) -> usize {
    USER_TEXT_VA + (symbol as usize - ptr::addr_of!(__user_program_start) as usize)
}

fn expected_resume_va() -> usize {
    user_symbol_va(ptr::addr_of!(__receiver_resume_point))
}

fn expected_trampoline_va() -> usize {
    user_symbol_va(ptr::addr_of!(__sigreturn_trampoline))
}

fn expected_handler_va() -> usize {
    user_symbol_va(ptr::addr_of!(__user_signal_handler))
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

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
    }
}

fn kernel_fail(message: &str) -> ! {
    println!("[kernel] failure: {}", message);
    qemu_exit(1);
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

fn pass_fail(condition: bool) -> &'static str {
    if condition {
        "PASS"
    } else {
        "FAIL"
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1);
}
