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
const USER_STACK_TOP: usize = 0x0040_4000;

const INITIAL_VALUE: u64 = 0x1111_2222_3333_4444;
const CHILD_WRITE_VALUE: u64 = 0xc0ff_ee00_0000_0001;
const PARENT_WRITE_VALUE: u64 = 0xa11c_e000_0000_0002;

const SATP_MODE_SV39: usize = 8usize << 60;

const PTE_V: usize = 1 << 0;
const PTE_R: usize = 1 << 1;
const PTE_W: usize = 1 << 2;
const PTE_X: usize = 1 << 3;
const PTE_U: usize = 1 << 4;
const PTE_G: usize = 1 << 5;
const PTE_A: usize = 1 << 6;
const PTE_D: usize = 1 << 7;
const PTE_COW: usize = 1 << 8;

const SSTATUS_SUM: usize = 1 << 18;

const SYS_FORK: usize = 1;
const SYS_YIELD: usize = 2;
const SYS_REPORT: usize = 3;
const SYS_EXIT: usize = 4;

const USER_ENV_CALL: usize = 8;
const LOAD_PAGE_FAULT: usize = 13;
const STORE_PAGE_FAULT: usize = 15;

const MEDELEG_MASK: usize = (1 << USER_ENV_CALL) | (1 << LOAD_PAGE_FAULT) | (1 << STORE_PAGE_FAULT);

const MAX_PROCS: usize = 2;
const PID_PARENT: usize = 0;
const PID_CHILD: usize = 1;

const PROC_UNUSED: usize = 0;
const PROC_RUNNABLE: usize = 1;
const PROC_EXITED: usize = 2;

const MAX_DATA_PAGES: usize = 4;

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

#[repr(align(4096))]
struct DataPagePool {
    bytes: [u8; PAGE_SIZE * MAX_DATA_PAGES],
}

impl DataPagePool {
    const fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE * MAX_DATA_PAGES],
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
    leaf_level: usize,
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
            leaf_level: usize::MAX,
            leaf_pte: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct ProcessControl {
    state: usize,
    satp: usize,
    exit_code: usize,
}

impl ProcessControl {
    const fn empty() -> Self {
        Self {
            state: PROC_UNUSED,
            satp: 0,
            exit_code: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct ProcessReport {
    reported: bool,
    first_read: u64,
    second_read: u64,
    third_read: u64,
}

impl ProcessReport {
    const fn empty() -> Self {
        Self {
            reported: false,
            first_read: 0,
            second_read: 0,
            third_read: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct ForkStats {
    calls: usize,
    shared_pa: usize,
    alloc_count_before: usize,
    alloc_count_after: usize,
    refcount_before: usize,
    refcount_after: usize,
    parent_pte_after: usize,
    child_pte_after: usize,
}

impl ForkStats {
    const fn zeroed() -> Self {
        Self {
            calls: 0,
            shared_pa: 0,
            alloc_count_before: 0,
            alloc_count_after: 0,
            refcount_before: 0,
            refcount_after: 0,
            parent_pte_after: 0,
            child_pte_after: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct CowStats {
    store_faults: usize,
    copy_faults: usize,
    reuse_faults: usize,
    child_refcount_before: usize,
    child_old_pa: usize,
    child_new_pa: usize,
    parent_refcount_before: usize,
    parent_old_pa: usize,
    parent_new_pa: usize,
}

impl CowStats {
    const fn zeroed() -> Self {
        Self {
            store_faults: 0,
            copy_faults: 0,
            reuse_faults: 0,
            child_refcount_before: 0,
            child_old_pa: 0,
            child_new_pa: 0,
            parent_refcount_before: 0,
            parent_old_pa: 0,
            parent_new_pa: 0,
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
            if (pte & PTE_G) != 0 { 'G' } else { '-' },
            if (pte & PTE_A) != 0 { 'A' } else { '-' },
            if (pte & PTE_D) != 0 { 'D' } else { '-' },
            if (pte & PTE_COW) != 0 { 'C' } else { '-' },
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
static mut DATA_PAGE_POOL: DataPagePool = DataPagePool::zeroed();

static mut PROCESS_FRAMES: [TrapFrame; MAX_PROCS] = [TrapFrame::zeroed(); MAX_PROCS];
static mut PROCESS_TABLE: [ProcessControl; MAX_PROCS] = [ProcessControl::empty(); MAX_PROCS];
static mut PROCESS_REPORTS: [ProcessReport; MAX_PROCS] = [ProcessReport::empty(); MAX_PROCS];
static mut PAGE_REFCOUNTS: [usize; MAX_DATA_PAGES] = [0; MAX_DATA_PAGES];
static mut NEXT_FREE_DATA_PAGE: usize = 0;
static mut DATA_PAGE_ALLOC_COUNT: usize = 0;
static mut CURRENT_PID: usize = PID_PARENT;
static mut FORK_STATS: ForkStats = ForkStats::zeroed();
static mut COW_STATS: CowStats = CowStats::zeroed();

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __supervisor_trap_stack_top: u8;
    static __machine_trap_stack_top: u8;
    static __user_program_start: u8;
    static __user_program_end: u8;

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
    println!("[kernel] LAB4 kernel task3 copy-on-write fork");
    println!(
        "[kernel] policy: fork shares one writable anonymous user page by clearing W and setting COW; store page fault resolves copy or write-enable"
    );
    println!(
        "[kernel] parent satp={:#x} root_pa={:#x}",
        unsafe { PROCESS_TABLE[PID_PARENT].satp },
        proc_root_table_pa(PID_PARENT)
    );
    println!(
        "[kernel] initial anonymous page pa={:#x} refcount={} value={:#018x}",
        walk_proc_virtual(PID_PARENT, USER_DATA_VA).leaf_pte_to_pa(),
        data_page_refcount(walk_proc_virtual(PID_PARENT, USER_DATA_VA).leaf_pte_to_pa()),
        unsafe { *(walk_proc_virtual(PID_PARENT, USER_DATA_VA).leaf_pte_to_pa() as *const u64) }
    );
    print_walk("parent_data_before_fork", PID_PARENT, USER_DATA_VA);

    unsafe {
        asm!(
            "csrw sscratch, {}",
            in(reg) supervisor_trap_stack_top(),
            options(nostack, nomem)
        );
        CURRENT_PID = PID_PARENT;
        activate_process(PID_PARENT);
        enter_user_task(
            ptr::addr_of!(PROCESS_FRAMES[PID_PARENT]),
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
        STORE_PAGE_FAULT => handle_cow_store_fault(pid, stval),
        LOAD_PAGE_FAULT => {
            println!(
                "[kernel] unexpected load page fault: pid={} sepc={:#x} stval={:#x}",
                pid, frame.epc, stval
            );
            qemu_exit(1);
        }
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
            activate_process(next_pid);
            *frame = PROCESS_FRAMES[next_pid];
        }
        return;
    }

    finish_experiment();
}

fn initialize_runtime() {
    unsafe {
        NEXT_FREE_DATA_PAGE = 0;
        DATA_PAGE_ALLOC_COUNT = 0;
        CURRENT_PID = PID_PARENT;
        FORK_STATS = ForkStats::zeroed();
        COW_STATS = CowStats::zeroed();
    }

    clear_page_table(ptr::addr_of_mut!(DEV_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(KERNEL_L1_PAGE_TABLE));
    zero_page(ptr::addr_of_mut!(USER_CODE_PAGE));
    zero_data_page_pool();

    for pid in 0..MAX_PROCS {
        clear_page_table(proc_root_table_ptr(pid));
        clear_page_table(proc_low_l1_table_ptr(pid));
        clear_page_table(proc_user_l0_table_ptr(pid));
        unsafe {
            PROCESS_FRAMES[pid] = TrapFrame::zeroed();
            PROCESS_TABLE[pid] = ProcessControl::empty();
            PROCESS_REPORTS[pid] = ProcessReport::empty();
        }
    }

    for index in 0..MAX_DATA_PAGES {
        unsafe {
            PAGE_REFCOUNTS[index] = 0;
        }
    }

    copy_user_program();
    build_global_kernel_tables();

    let initial_data_pa = allocate_data_page();
    unsafe {
        *(initial_data_pa as *mut u64) = INITIAL_VALUE;
    }
    build_process(PID_PARENT, make_writable_data_pte(initial_data_pa));

    unsafe {
        PROCESS_TABLE[PID_PARENT].state = PROC_RUNNABLE;
        PROCESS_FRAMES[PID_PARENT] = TrapFrame::zeroed();
        PROCESS_FRAMES[PID_PARENT].epc = USER_TEXT_VA;
        PROCESS_FRAMES[PID_PARENT].saved_sp = USER_STACK_TOP;
    }
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
    clear_page_table(ptr::addr_of_mut!(DEV_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(KERNEL_L1_PAGE_TABLE));

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

fn build_process(pid: usize, data_pte: usize) {
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
        PROC_USER_L0_TABLES[pid].entries[vpn0_index(USER_DATA_VA)] = data_pte;

        PROCESS_TABLE[pid].satp = satp_for_root(proc_root_table_pa(pid));
        PROCESS_TABLE[pid].state = PROC_RUNNABLE;
        PROCESS_TABLE[pid].exit_code = 0;
        PROCESS_REPORTS[pid] = ProcessReport::empty();
    }
}

fn handle_user_ecall(pid: usize) -> Option<usize> {
    let syscall_nr = unsafe { PROCESS_FRAMES[pid].a7 };

    unsafe {
        PROCESS_FRAMES[pid].epc += 4;
    }

    match syscall_nr {
        SYS_FORK => {
            do_fork();
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

fn do_fork() {
    let parent_data_pte =
        unsafe { PROC_USER_L0_TABLES[PID_PARENT].entries[vpn0_index(USER_DATA_VA)] };
    let shared_pa = pte_to_pa(parent_data_pte);
    let alloc_before = unsafe { DATA_PAGE_ALLOC_COUNT };
    let ref_before = data_page_refcount(shared_pa);

    unsafe {
        PROC_ROOT_TABLES[PID_CHILD] = PROC_ROOT_TABLES[PID_PARENT];
        PROC_LOW_L1_TABLES[PID_CHILD] = PROC_LOW_L1_TABLES[PID_PARENT];
        PROC_USER_L0_TABLES[PID_CHILD] = PROC_USER_L0_TABLES[PID_PARENT];
    }

    unsafe {
        PROC_ROOT_TABLES[PID_CHILD].entries[vpn2_index(USER_TEXT_VA)] =
            table_pte(proc_low_l1_table_pa(PID_CHILD));
        PROC_LOW_L1_TABLES[PID_CHILD].entries[vpn1_index(USER_TEXT_VA)] =
            table_pte(proc_user_l0_table_pa(PID_CHILD));
    }

    let cow_pte = make_cow_pte(shared_pa);
    install_data_pte(PID_PARENT, cow_pte);
    install_data_pte(PID_CHILD, cow_pte);
    increment_data_page_refcount(shared_pa);

    unsafe {
        PROCESS_TABLE[PID_CHILD].state = PROC_RUNNABLE;
        PROCESS_TABLE[PID_CHILD].satp = satp_for_root(proc_root_table_pa(PID_CHILD));
        PROCESS_TABLE[PID_CHILD].exit_code = 0;
        PROCESS_REPORTS[PID_CHILD] = ProcessReport::empty();

        PROCESS_FRAMES[PID_CHILD] = PROCESS_FRAMES[PID_PARENT];
        PROCESS_FRAMES[PID_CHILD].a0 = 0;
        PROCESS_FRAMES[PID_PARENT].a0 = PID_CHILD;

        FORK_STATS.calls += 1;
        FORK_STATS.shared_pa = shared_pa;
        FORK_STATS.alloc_count_before = alloc_before;
        FORK_STATS.alloc_count_after = DATA_PAGE_ALLOC_COUNT;
        FORK_STATS.refcount_before = ref_before;
        FORK_STATS.refcount_after = data_page_refcount(shared_pa);
        FORK_STATS.parent_pte_after =
            PROC_USER_L0_TABLES[PID_PARENT].entries[vpn0_index(USER_DATA_VA)];
        FORK_STATS.child_pte_after =
            PROC_USER_L0_TABLES[PID_CHILD].entries[vpn0_index(USER_DATA_VA)];
    }

    println!(
        "[kernel] fork complete: alloc_before={} alloc_after={} shared_pa={:#x} ref_before={} ref_after={}",
        alloc_before,
        unsafe { DATA_PAGE_ALLOC_COUNT },
        shared_pa,
        ref_before,
        data_page_refcount(shared_pa)
    );
    print_walk("parent_after_fork", PID_PARENT, USER_DATA_VA);
    print_walk("child_after_fork", PID_CHILD, USER_DATA_VA);
}

fn store_process_report(pid: usize) {
    unsafe {
        PROCESS_REPORTS[pid].reported = true;
        PROCESS_REPORTS[pid].first_read = PROCESS_FRAMES[pid].a1 as u64;
        PROCESS_REPORTS[pid].second_read = PROCESS_FRAMES[pid].a2 as u64;
        PROCESS_REPORTS[pid].third_read = PROCESS_FRAMES[pid].a3 as u64;
    }

    println!(
        "[kernel] report pid={} first={:#018x} second={:#018x} third={:#018x}",
        pid,
        unsafe { PROCESS_REPORTS[pid].first_read },
        unsafe { PROCESS_REPORTS[pid].second_read },
        unsafe { PROCESS_REPORTS[pid].third_read }
    );
}

fn handle_cow_store_fault(pid: usize, stval: usize) -> Option<usize> {
    let fault_va = align_down(stval, PAGE_SIZE);
    if fault_va != USER_DATA_VA {
        println!(
            "[kernel] unexpected store page fault: pid={} sepc={:#x} stval={:#x}",
            pid,
            unsafe { PROCESS_FRAMES[pid].epc },
            stval
        );
        qemu_exit(1);
    }

    let before = walk_proc_virtual(pid, USER_DATA_VA);
    if !pte_is_valid(before.leaf_pte) || !pte_has_cow(before.leaf_pte) {
        println!(
            "[kernel] store fault on non-COW page: pid={} pte={:#x}",
            pid, before.leaf_pte
        );
        qemu_exit(1);
    }

    let old_pa = pte_to_pa(before.leaf_pte);
    let refcount_before = data_page_refcount(old_pa);

    unsafe {
        COW_STATS.store_faults += 1;
    }

    let new_pa = if refcount_before > 1 {
        let fresh_pa = allocate_data_page();
        copy_page(fresh_pa, old_pa);
        decrement_data_page_refcount(old_pa);
        install_data_pte(pid, make_writable_data_pte(fresh_pa));

        unsafe {
            COW_STATS.copy_faults += 1;
            if pid == PID_CHILD {
                COW_STATS.child_refcount_before = refcount_before;
                COW_STATS.child_old_pa = old_pa;
                COW_STATS.child_new_pa = fresh_pa;
            }
        }

        println!(
            "[kernel] cow store fault pid={} action=copy refcount_before={} old_pa={:#x} new_pa={:#x}",
            pid, refcount_before, old_pa, fresh_pa
        );
        fresh_pa
    } else {
        install_data_pte(pid, make_writable_data_pte(old_pa));

        unsafe {
            COW_STATS.reuse_faults += 1;
            if pid == PID_PARENT {
                COW_STATS.parent_refcount_before = refcount_before;
                COW_STATS.parent_old_pa = old_pa;
                COW_STATS.parent_new_pa = old_pa;
            }
        }

        println!(
            "[kernel] cow store fault pid={} action=reuse refcount_before={} pa={:#x}",
            pid, refcount_before, old_pa
        );
        old_pa
    };

    let after = walk_proc_virtual(pid, USER_DATA_VA);
    println!(
        "[kernel] cow fault result pid={} sepc={:#x} stval={:#x} old_pa={:#x} new_pa={:#x}",
        pid,
        unsafe { PROCESS_FRAMES[pid].epc },
        stval,
        old_pa,
        new_pa
    );
    print_walk_result("cow_fault_before", pid, USER_DATA_VA, before);
    print_walk_result("cow_fault_after", pid, USER_DATA_VA, after);
    Some(pid)
}

fn finish_experiment() -> ! {
    let parent_walk = walk_proc_virtual(PID_PARENT, USER_DATA_VA);
    let child_walk = walk_proc_virtual(PID_CHILD, USER_DATA_VA);
    let parent_pa = pte_to_pa(parent_walk.leaf_pte);
    let child_pa = pte_to_pa(child_walk.leaf_pte);
    let parent_value = unsafe { *(parent_pa as *const u64) };
    let child_value = unsafe { *(child_pa as *const u64) };
    let parent_report = unsafe { PROCESS_REPORTS[PID_PARENT] };
    let child_report = unsafe { PROCESS_REPORTS[PID_CHILD] };
    let fork_stats = unsafe { FORK_STATS };
    let cow_stats = unsafe { COW_STATS };

    println!(
        "[kernel] final fork stats: calls={} alloc_before={} alloc_after={} shared_pa={:#x} ref_before={} ref_after={}",
        fork_stats.calls,
        fork_stats.alloc_count_before,
        fork_stats.alloc_count_after,
        fork_stats.shared_pa,
        fork_stats.refcount_before,
        fork_stats.refcount_after
    );
    println!(
        "[kernel] final cow stats: store_faults={} copy_faults={} reuse_faults={} child_ref_before={} child_old_pa={:#x} child_new_pa={:#x} parent_ref_before={} parent_old_pa={:#x} parent_new_pa={:#x}",
        cow_stats.store_faults,
        cow_stats.copy_faults,
        cow_stats.reuse_faults,
        cow_stats.child_refcount_before,
        cow_stats.child_old_pa,
        cow_stats.child_new_pa,
        cow_stats.parent_refcount_before,
        cow_stats.parent_old_pa,
        cow_stats.parent_new_pa
    );
    println!(
        "[kernel] reports parent(first={:#018x} after_child={:#018x} after_parent={:#018x}) child(first={:#018x} after_child={:#018x})",
        parent_report.first_read,
        parent_report.second_read,
        parent_report.third_read,
        child_report.first_read,
        child_report.second_read
    );
    println!(
        "[kernel] final values parent={:#018x} child={:#018x} parent_refcount={} child_refcount={}",
        parent_value,
        child_value,
        data_page_refcount(parent_pa),
        data_page_refcount(child_pa)
    );
    print_walk_result("parent_final", PID_PARENT, USER_DATA_VA, parent_walk);
    print_walk_result("child_final", PID_CHILD, USER_DATA_VA, child_walk);

    let fork_ok = fork_stats.calls == 1
        && fork_stats.alloc_count_after == fork_stats.alloc_count_before
        && fork_stats.refcount_before == 1
        && fork_stats.refcount_after == 2
        && pte_to_pa(fork_stats.parent_pte_after) == fork_stats.shared_pa
        && pte_to_pa(fork_stats.child_pte_after) == fork_stats.shared_pa
        && !pte_has_write(fork_stats.parent_pte_after)
        && !pte_has_write(fork_stats.child_pte_after)
        && pte_has_cow(fork_stats.parent_pte_after)
        && pte_has_cow(fork_stats.child_pte_after);

    let cow_ok = cow_stats.store_faults == 2
        && cow_stats.copy_faults == 1
        && cow_stats.reuse_faults == 1
        && cow_stats.child_refcount_before == 2
        && cow_stats.child_old_pa != 0
        && cow_stats.child_old_pa != cow_stats.child_new_pa
        && cow_stats.parent_refcount_before == 1
        && cow_stats.parent_old_pa == cow_stats.parent_new_pa
        && pte_has_write(parent_walk.leaf_pte)
        && pte_has_write(child_walk.leaf_pte)
        && !pte_has_cow(parent_walk.leaf_pte)
        && !pte_has_cow(child_walk.leaf_pte);

    let semantics_ok = unsafe { PROCESS_TABLE[PID_PARENT].exit_code == 0 }
        && unsafe { PROCESS_TABLE[PID_CHILD].exit_code == 0 }
        && parent_report.reported
        && child_report.reported
        && parent_report.first_read == INITIAL_VALUE
        && child_report.first_read == INITIAL_VALUE
        && parent_report.second_read == INITIAL_VALUE
        && child_report.second_read == CHILD_WRITE_VALUE
        && parent_report.third_read == PARENT_WRITE_VALUE
        && parent_value == PARENT_WRITE_VALUE
        && child_value == CHILD_WRITE_VALUE
        && parent_pa != child_pa
        && data_page_refcount(parent_pa) == 1
        && data_page_refcount(child_pa) == 1;

    println!(
        "[kernel] acceptance fork shares page, clears W, and only bumps refcount: {}",
        pass_fail(fork_ok)
    );
    println!(
        "[kernel] acceptance store page fault with refcount>1 copies page and remaps writable: {}",
        pass_fail(cow_ok)
    );
    println!(
        "[kernel] acceptance parent/child writes are isolated both ways: {}",
        pass_fail(semantics_ok)
    );

    qemu_exit(if fork_ok && cow_ok && semantics_ok {
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
    if pid == PID_PARENT {
        PID_CHILD
    } else {
        PID_PARENT
    }
}

fn allocate_data_page() -> usize {
    let index = unsafe { NEXT_FREE_DATA_PAGE };
    if index >= MAX_DATA_PAGES {
        kernel_fail("data page pool exhausted");
    }

    let pa = data_page_pool_pa() + index * PAGE_SIZE;
    unsafe {
        ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
        PAGE_REFCOUNTS[index] = 1;
        NEXT_FREE_DATA_PAGE += 1;
        DATA_PAGE_ALLOC_COUNT += 1;
    }
    pa
}

fn copy_page(dst_pa: usize, src_pa: usize) {
    unsafe {
        ptr::copy_nonoverlapping(src_pa as *const u8, dst_pa as *mut u8, PAGE_SIZE);
    }
}

fn data_page_refcount(pa: usize) -> usize {
    unsafe { PAGE_REFCOUNTS[data_page_index(pa)] }
}

fn increment_data_page_refcount(pa: usize) {
    let index = data_page_index(pa);
    unsafe {
        PAGE_REFCOUNTS[index] += 1;
    }
}

fn decrement_data_page_refcount(pa: usize) {
    let index = data_page_index(pa);
    unsafe {
        if PAGE_REFCOUNTS[index] == 0 {
            kernel_fail("decrement on zero refcount");
        }
        PAGE_REFCOUNTS[index] -= 1;
    }
}

fn data_page_index(pa: usize) -> usize {
    let base = data_page_pool_pa();
    if pa < base || pa >= base + PAGE_SIZE * MAX_DATA_PAGES || (pa - base) % PAGE_SIZE != 0 {
        kernel_fail("physical page is outside the COW data page pool");
    }
    (pa - base) / PAGE_SIZE
}

fn install_data_pte(pid: usize, pte: usize) {
    unsafe {
        PROC_USER_L0_TABLES[pid].entries[vpn0_index(USER_DATA_VA)] = pte;
        asm!(
            "sfence.vma {}, zero",
            in(reg) USER_DATA_VA,
            options(nostack, nomem)
        );
    }
}

fn make_writable_data_pte(pa: usize) -> usize {
    leaf_pte(pa, PTE_R | PTE_W | PTE_U | PTE_A | PTE_D)
}

fn make_cow_pte(pa: usize) -> usize {
    leaf_pte(pa, PTE_R | PTE_U | PTE_A | PTE_COW)
}

fn activate_process(pid: usize) {
    let satp = unsafe { PROCESS_TABLE[pid].satp };
    unsafe {
        asm!("csrw satp, {}", in(reg) satp, options(nostack, nomem));
        asm!("sfence.vma zero, zero", options(nostack, nomem));
    }
}

fn print_walk(label: &str, pid: usize, va: usize) {
    print_walk_result(label, pid, va, walk_proc_virtual(pid, va));
}

fn print_walk_result(label: &str, pid: usize, va: usize, walk: WalkResult) {
    println!(
        "[pt] {} pid={} va={:#x} vpn=({},{},{}) level={} root_pte={:#018x} l1_pte={:#018x} l0_pte={:#018x} leaf_pte={:#018x} leaf_pa={:#x} flags={}",
        label,
        pid,
        va,
        walk.vpn2,
        walk.vpn1,
        walk.vpn0,
        leaf_level_name(walk.leaf_level),
        walk.root_pte,
        walk.l1_pte,
        walk.l0_pte,
        walk.leaf_pte,
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
        result.leaf_level = 2;
        result.leaf_pte = result.root_pte;
        return result;
    }

    let l1 = unsafe { &*(pte_to_pa(result.root_pte) as *const PageTable) };
    result.l1_pte = l1.entries[result.vpn1];

    if !pte_is_valid(result.l1_pte) || pte_is_leaf(result.l1_pte) {
        result.leaf_level = 1;
        result.leaf_pte = result.l1_pte;
        return result;
    }

    let l0 = unsafe { &*(pte_to_pa(result.l1_pte) as *const PageTable) };
    result.l0_pte = l0.entries[result.vpn0];
    result.leaf_level = 0;
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

fn pte_has_cow(pte: usize) -> bool {
    (pte & PTE_COW) != 0
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

fn zero_data_page_pool() {
    unsafe {
        ptr::write_bytes(
            ptr::addr_of_mut!(DATA_PAGE_POOL.bytes).cast::<u8>(),
            0,
            PAGE_SIZE * MAX_DATA_PAGES,
        );
    }
}

fn leaf_level_name(level: usize) -> &'static str {
    match level {
        0 => "L0-4K",
        1 => "L1-2M",
        2 => "L2-1G",
        _ => "invalid",
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

fn data_page_pool_pa() -> usize {
    unsafe { ptr::addr_of_mut!(DATA_PAGE_POOL.bytes) as usize }
}

fn user_program_len() -> usize {
    (ptr::addr_of!(__user_program_end) as usize) - (ptr::addr_of!(__user_program_start) as usize)
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

trait WalkPaExt {
    fn leaf_pte_to_pa(self) -> usize;
}

impl WalkPaExt for WalkResult {
    fn leaf_pte_to_pa(self) -> usize {
        pte_to_pa(self.leaf_pte)
    }
}
