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
const USER_SHARED_VA: usize = 0x0040_1000;
const USER_STACK_VA: usize = 0x0040_2000;
const USER_STACK_TOP: usize = USER_STACK_VA + PAGE_SIZE;
const USER_HEAP_BASE: usize = 0x0041_0000;
const USER_HEAP_LIMIT: usize = 0x0042_0000;
const USER_MMAP_BASE: usize = 0x0042_0000;
const USER_MMAP_LIMIT: usize = 0x0044_0000;

const STAGE_DONE: u64 = 0x5555_6666_7777_8888;
const HEAP_PAGE0_VALUE: u64 = 0x1111_2222_3333_4444;
const HEAP_PAGE1_VALUE: u64 = 0x5555_6666_7777_8888;
const MMAP_PAGE0_VALUE: u64 = 0x9999_aaaa_bbbb_cccc;
const MMAP_PAGE1_VALUE: u64 = 0xdddd_eeee_ffff_0001;

const SATP_MODE_SV39: usize = 8usize << 60;

const PTE_V: usize = 1 << 0;
const PTE_R: usize = 1 << 1;
const PTE_W: usize = 1 << 2;
const PTE_X: usize = 1 << 3;
const PTE_U: usize = 1 << 4;
const PTE_G: usize = 1 << 5;
const PTE_A: usize = 1 << 6;
const PTE_D: usize = 1 << 7;

const SSTATUS_SUM: usize = 1 << 18;

const SYS_SBRK: usize = 1;
const SYS_MMAP: usize = 2;
const SYS_EXIT: usize = 3;

const USER_ENV_CALL: usize = 8;
const LOAD_PAGE_FAULT: usize = 13;
const STORE_PAGE_FAULT: usize = 15;

const MEDELEG_MASK: usize = (1 << USER_ENV_CALL) | (1 << LOAD_PAGE_FAULT) | (1 << STORE_PAGE_FAULT);

const VMA_READ: usize = 1 << 0;
const VMA_WRITE: usize = 1 << 1;

const VMA_KIND_HEAP: usize = 1;
const VMA_KIND_MMAP: usize = 2;

const MAX_VMAS: usize = 4;
const MAX_LAZY_PAGES: usize = 16;

#[repr(align(4096))]
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
struct LazyPagePool {
    bytes: [u8; PAGE_SIZE * MAX_LAZY_PAGES],
}

impl LazyPagePool {
    const fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE * MAX_LAZY_PAGES],
        }
    }
}

#[repr(C)]
struct UserSharedData {
    stage_marker: u64,
    heap_base: u64,
    mmap_base: u64,
    heap_page0_readback: u64,
    heap_page1_readback: u64,
    mmap_page0_initial: u64,
    mmap_page0_readback: u64,
    mmap_page1_initial: u64,
    mmap_page1_readback: u64,
    exit_code: u64,
}

#[derive(Clone, Copy)]
struct Vma {
    active: bool,
    start: usize,
    end: usize,
    flags: usize,
    kind: usize,
    fault_count: usize,
    mapped_pages: usize,
}

impl Vma {
    const fn empty() -> Self {
        Self {
            active: false,
            start: 0,
            end: 0,
            flags: 0,
            kind: 0,
            fault_count: 0,
            mapped_pages: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct LazyStats {
    reserve_count: usize,
    sbrk_calls: usize,
    mmap_calls: usize,
    page_fault_count: usize,
    load_fault_count: usize,
    store_fault_count: usize,
    page_alloc_count: usize,
    map_install_count: usize,
}

impl LazyStats {
    const fn zeroed() -> Self {
        Self {
            reserve_count: 0,
            sbrk_calls: 0,
            mmap_calls: 0,
            page_fault_count: 0,
            load_fault_count: 0,
            store_fault_count: 0,
            page_alloc_count: 0,
            map_install_count: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct LazyEvidence {
    heap_unmapped_after_sbrk: bool,
    mmap_unmapped_after_mmap: bool,
    first_fault_seen: bool,
    alloc_count_before_first_fault: usize,
    lazy_map_success_count: usize,
}

impl LazyEvidence {
    const fn zeroed() -> Self {
        Self {
            heap_unmapped_after_sbrk: false,
            mmap_unmapped_after_mmap: false,
            first_fault_seen: false,
            alloc_count_before_first_fault: 0,
            lazy_map_success_count: 0,
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
        ];

        for ch in chars {
            write!(f, "{ch}")?;
        }

        Ok(())
    }
}

static mut ROOT_PAGE_TABLE: PageTable = PageTable::zeroed();
static mut LOW_L1_PAGE_TABLE: PageTable = PageTable::zeroed();
static mut DEV_L0_PAGE_TABLE: PageTable = PageTable::zeroed();
static mut USER_L0_PAGE_TABLE: PageTable = PageTable::zeroed();
static mut KERNEL_L1_PAGE_TABLE: PageTable = PageTable::zeroed();

static mut USER_CODE_PAGE: Page = Page::zeroed();
static mut USER_SHARED_PAGE: Page = Page::zeroed();
static mut USER_STACK_PAGE: Page = Page::zeroed();
static mut USER_FRAME: TrapFrame = TrapFrame::zeroed();
static mut ROOT_SATP: usize = 0;

static mut LAZY_PAGE_POOL: LazyPagePool = LazyPagePool::zeroed();
static mut NEXT_FREE_LAZY_PAGE: usize = 0;
static mut VMAS: [Vma; MAX_VMAS] = [Vma::empty(); MAX_VMAS];
static mut LAZY_STATS: LazyStats = LazyStats::zeroed();
static mut LAZY_EVIDENCE: LazyEvidence = LazyEvidence::zeroed();
static mut HEAP_BREAK: usize = USER_HEAP_BASE;
static mut NEXT_MMAP_BASE: usize = USER_MMAP_BASE;

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

    initialize_lazy_runtime();
    initialize_user_pages();
    build_page_tables();
    activate_address_space();
    prepare_user_frame();

    println!("[kernel] booted in S-mode with Sv39 enabled");
    println!("[kernel] LAB4 kernel task2 lazy anonymous paging");
    println!(
        "[kernel] policy: sys_sbrk/sys_mmap reserve VMA only; load/store page fault allocates physical page and installs user leaf PTE"
    );
    println!(
        "[kernel] satp(root)={:#x} root_pa={:#x}",
        unsafe { ROOT_SATP },
        root_page_table_pa()
    );
    println!(
        "[kernel] eager user mappings: text={:#x} shared={:#x} stack={:#x}; lazy heap_base={:#x} mmap_base={:#x}",
        USER_TEXT_VA,
        USER_SHARED_VA,
        USER_STACK_VA,
        USER_HEAP_BASE,
        USER_MMAP_BASE
    );

    print_walk("user_text", USER_TEXT_VA, walk_virtual(USER_TEXT_VA));
    print_walk("user_shared", USER_SHARED_VA, walk_virtual(USER_SHARED_VA));
    print_walk("user_stack", USER_STACK_VA, walk_virtual(USER_STACK_VA));
    print_walk(
        "heap_before_reserve",
        USER_HEAP_BASE,
        walk_virtual(USER_HEAP_BASE),
    );
    print_walk(
        "mmap_before_reserve",
        USER_MMAP_BASE,
        walk_virtual(USER_MMAP_BASE),
    );

    println!(
        "[kernel] entering U-mode lazy-paging probe at user_text={:#x} user_sp={:#x}",
        USER_TEXT_VA, USER_STACK_TOP
    );

    unsafe {
        asm!(
            "csrw sscratch, {}",
            in(reg) supervisor_trap_stack_top(),
            options(nostack, nomem)
        );
        enter_user_task(ptr::addr_of!(USER_FRAME), supervisor_trap_stack_top())
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
    let scause = read_scause();
    let stval = read_stval();

    match scause {
        USER_ENV_CALL => handle_user_ecall(frame),
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT => handle_lazy_page_fault(frame, scause, stval),
        _ => {
            println!(
                "[kernel] unexpected supervisor trap: scause={:#x} sepc={:#x} stval={:#x} satp={:#x}",
                scause,
                frame.epc,
                stval,
                read_satp()
            );
            qemu_exit(1);
        }
    }
}

fn handle_user_ecall(frame: &mut TrapFrame) {
    let syscall_nr = frame.a7;
    frame.epc += 4;

    match syscall_nr {
        SYS_SBRK => {
            let old_break = sys_sbrk(frame.a0);
            frame.a0 = old_break;
        }
        SYS_MMAP => {
            let base = sys_mmap(frame.a0);
            frame.a0 = base;
        }
        SYS_EXIT => finish_user_program(frame.a0),
        _ => {
            println!(
                "[kernel] unexpected syscall a7={} a0={:#x} sepc={:#x}",
                frame.a7,
                frame.a0,
                frame.epc - 4
            );
            qemu_exit(1);
        }
    }
}

fn sys_sbrk(length: usize) -> usize {
    let grow_len = align_up(length, PAGE_SIZE);
    let old_break = unsafe { HEAP_BREAK };
    let new_break = old_break
        .checked_add(grow_len)
        .unwrap_or_else(|| kernel_fail("heap break overflow"));

    if new_break > USER_HEAP_LIMIT {
        kernel_fail("heap reservation exceeds limit");
    }

    unsafe {
        HEAP_BREAK = new_break;
        LAZY_STATS.reserve_count += 1;
        LAZY_STATS.sbrk_calls += 1;
    }

    upsert_vma(
        VMA_KIND_HEAP,
        USER_HEAP_BASE,
        new_break,
        VMA_READ | VMA_WRITE,
    );

    let walk = walk_virtual(old_break);
    unsafe {
        LAZY_EVIDENCE.heap_unmapped_after_sbrk = !pte_is_valid(walk.leaf_pte);
    }

    println!(
        "[kernel] sys_sbrk len={} -> base={:#x} new_break={:#x} alloc_count={} still_unmapped_after_reserve={}",
        grow_len,
        old_break,
        new_break,
        unsafe { LAZY_STATS.page_alloc_count },
        pass_fail(!pte_is_valid(walk.leaf_pte))
    );
    print_walk("heap_reserved", old_break, walk);
    print_vmas();
    old_break
}

fn sys_mmap(length: usize) -> usize {
    let map_len = align_up(length, PAGE_SIZE);
    let base = unsafe { NEXT_MMAP_BASE };
    let end = base
        .checked_add(map_len)
        .unwrap_or_else(|| kernel_fail("mmap reservation overflow"));

    if end > USER_MMAP_LIMIT {
        kernel_fail("mmap reservation exceeds limit");
    }

    unsafe {
        NEXT_MMAP_BASE = end;
        LAZY_STATS.reserve_count += 1;
        LAZY_STATS.mmap_calls += 1;
    }

    insert_vma(base, end, VMA_READ | VMA_WRITE, VMA_KIND_MMAP);

    let walk = walk_virtual(base);
    unsafe {
        LAZY_EVIDENCE.mmap_unmapped_after_mmap = !pte_is_valid(walk.leaf_pte);
    }

    println!(
        "[kernel] sys_mmap len={} -> base={:#x} end={:#x} alloc_count={} still_unmapped_after_reserve={}",
        map_len,
        base,
        end,
        unsafe { LAZY_STATS.page_alloc_count },
        pass_fail(!pte_is_valid(walk.leaf_pte))
    );
    print_walk("mmap_reserved", base, walk);
    print_vmas();
    base
}

fn handle_lazy_page_fault(frame: &mut TrapFrame, scause: usize, stval: usize) {
    let fault_va = align_down(stval, PAGE_SIZE);
    let before = walk_virtual(fault_va);

    unsafe {
        LAZY_STATS.page_fault_count += 1;
        if scause == LOAD_PAGE_FAULT {
            LAZY_STATS.load_fault_count += 1;
        } else {
            LAZY_STATS.store_fault_count += 1;
        }

        if !LAZY_EVIDENCE.first_fault_seen {
            LAZY_EVIDENCE.first_fault_seen = true;
            LAZY_EVIDENCE.alloc_count_before_first_fault = LAZY_STATS.page_alloc_count;
        }
    }

    let vma_index = find_vma_index(fault_va)
        .unwrap_or_else(|| kernel_fail("page fault is outside all lazy VMAs"));

    if pte_is_valid(before.leaf_pte) {
        kernel_fail("page fault hit an already-mapped leaf");
    }

    let pa = allocate_lazy_page();
    let flags = user_leaf_flags_for_vma(unsafe { VMAS[vma_index].flags });
    install_user_leaf(fault_va, pa, flags);

    unsafe {
        VMAS[vma_index].fault_count += 1;
        VMAS[vma_index].mapped_pages += 1;
    }

    let after = walk_virtual(fault_va);

    if pte_is_valid(after.leaf_pte)
        && pte_to_pa(after.leaf_pte) == pa
        && pte_has_user(after.leaf_pte)
    {
        unsafe {
            LAZY_EVIDENCE.lazy_map_success_count += 1;
        }
    }

    println!(
        "[kernel] lazy fault kind={} sepc={:#x} stval={:#x} page={:#x} vma={} before_present={} alloc_pa={:#x}",
        fault_kind_name(scause),
        frame.epc,
        stval,
        fault_va,
        vma_kind_name(unsafe { VMAS[vma_index].kind }),
        yes_no(pte_is_valid(before.leaf_pte)),
        pa
    );
    print_walk("fault_before", fault_va, before);
    print_walk("fault_after", fault_va, after);
}

fn finish_user_program(exit_code: usize) -> ! {
    let shared = user_shared();
    let heap_page0_walk = walk_virtual(USER_HEAP_BASE);
    let heap_page1_walk = walk_virtual(USER_HEAP_BASE + PAGE_SIZE);
    let mmap_page0_walk = walk_virtual(USER_MMAP_BASE);
    let mmap_page1_walk = walk_virtual(USER_MMAP_BASE + PAGE_SIZE);
    let stats = unsafe { LAZY_STATS };
    let evidence = unsafe { LAZY_EVIDENCE };

    println!(
        "[kernel] user program requested exit with code={}",
        exit_code
    );
    println!(
        "[kernel] counters: reserves={} sbrk_calls={} mmap_calls={} page_faults={} load_faults={} store_faults={} allocs={} map_installs={}",
        stats.reserve_count,
        stats.sbrk_calls,
        stats.mmap_calls,
        stats.page_fault_count,
        stats.load_fault_count,
        stats.store_fault_count,
        stats.page_alloc_count,
        stats.map_install_count
    );
    println!(
        "[kernel] user evidence: stage={:#018x} heap_base={:#x} mmap_base={:#x} heap0={:#018x} heap1={:#018x} mmap0_initial={:#018x} mmap0={:#018x} mmap1_initial={:#018x} mmap1={:#018x}",
        shared.stage_marker,
        shared.heap_base,
        shared.mmap_base,
        shared.heap_page0_readback,
        shared.heap_page1_readback,
        shared.mmap_page0_initial,
        shared.mmap_page0_readback,
        shared.mmap_page1_initial,
        shared.mmap_page1_readback
    );

    print_vmas();
    print_walk("heap_page0", USER_HEAP_BASE, heap_page0_walk);
    print_walk("heap_page1", USER_HEAP_BASE + PAGE_SIZE, heap_page1_walk);
    print_walk("mmap_page0", USER_MMAP_BASE, mmap_page0_walk);
    print_walk("mmap_page1", USER_MMAP_BASE + PAGE_SIZE, mmap_page1_walk);

    let reservation_ok = evidence.heap_unmapped_after_sbrk
        && evidence.mmap_unmapped_after_mmap
        && evidence.first_fault_seen
        && evidence.alloc_count_before_first_fault == 0;
    let fault_path_ok = stats.page_fault_count == 4
        && stats.load_fault_count == 2
        && stats.store_fault_count == 2
        && stats.page_alloc_count == 4
        && stats.map_install_count == 4
        && evidence.lazy_map_success_count == 4;
    let mapped_ok = pte_is_valid(heap_page0_walk.leaf_pte)
        && pte_is_valid(heap_page1_walk.leaf_pte)
        && pte_is_valid(mmap_page0_walk.leaf_pte)
        && pte_is_valid(mmap_page1_walk.leaf_pte)
        && pte_has_user(heap_page0_walk.leaf_pte)
        && pte_has_user(heap_page1_walk.leaf_pte)
        && pte_has_user(mmap_page0_walk.leaf_pte)
        && pte_has_user(mmap_page1_walk.leaf_pte);
    let user_ok = exit_code == 0
        && shared.stage_marker == STAGE_DONE
        && shared.heap_base == USER_HEAP_BASE as u64
        && shared.mmap_base == USER_MMAP_BASE as u64
        && shared.heap_page0_readback == HEAP_PAGE0_VALUE
        && shared.heap_page1_readback == HEAP_PAGE1_VALUE
        && shared.mmap_page0_initial == 0
        && shared.mmap_page0_readback == MMAP_PAGE0_VALUE
        && shared.mmap_page1_initial == 0
        && shared.mmap_page1_readback == MMAP_PAGE1_VALUE;

    println!(
        "[kernel] acceptance sys_sbrk/sys_mmap reserve VMA without immediate physical allocation: {}",
        pass_fail(reservation_ok)
    );
    println!(
        "[kernel] acceptance load/store page fault allocates page and installs user PTE on demand: {}",
        pass_fail(fault_path_ok && mapped_ok)
    );
    println!(
        "[kernel] acceptance user-side heap/mmap accesses observe expected zero-fill and readback values: {}",
        pass_fail(user_ok)
    );

    qemu_exit(if reservation_ok && fault_path_ok && mapped_ok && user_ok {
        0
    } else {
        1
    })
}

fn initialize_lazy_runtime() {
    unsafe {
        NEXT_FREE_LAZY_PAGE = 0;
        VMAS = [Vma::empty(); MAX_VMAS];
        LAZY_STATS = LazyStats::zeroed();
        LAZY_EVIDENCE = LazyEvidence::zeroed();
        HEAP_BREAK = USER_HEAP_BASE;
        NEXT_MMAP_BASE = USER_MMAP_BASE;
        ptr::write_bytes(
            ptr::addr_of_mut!(LAZY_PAGE_POOL.bytes).cast::<u8>(),
            0,
            PAGE_SIZE * MAX_LAZY_PAGES,
        );
    }
}

fn initialize_user_pages() {
    unsafe {
        USER_CODE_PAGE = Page::zeroed();
        USER_SHARED_PAGE = Page::zeroed();
        USER_STACK_PAGE = Page::zeroed();
    }

    let program_bytes =
        unsafe { slice::from_raw_parts(ptr::addr_of!(__user_program_start), user_program_len()) };

    unsafe {
        ptr::copy_nonoverlapping(
            program_bytes.as_ptr(),
            ptr::addr_of_mut!(USER_CODE_PAGE.bytes).cast::<u8>(),
            program_bytes.len(),
        );
    }

    let shared = user_shared_mut();
    shared.stage_marker = 0;
    shared.heap_base = 0;
    shared.mmap_base = 0;
    shared.heap_page0_readback = 0;
    shared.heap_page1_readback = 0;
    shared.mmap_page0_initial = 0;
    shared.mmap_page0_readback = 0;
    shared.mmap_page1_initial = 0;
    shared.mmap_page1_readback = 0;
    shared.exit_code = 0;
}

fn build_page_tables() {
    clear_page_table(ptr::addr_of_mut!(ROOT_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(LOW_L1_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(DEV_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(USER_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(KERNEL_L1_PAGE_TABLE));

    unsafe {
        ROOT_PAGE_TABLE.entries[vpn2_index(KERNEL_BASE)] = table_pte(kernel_l1_table_pa());
        ROOT_PAGE_TABLE.entries[vpn2_index(USER_TEXT_VA)] = table_pte(low_l1_table_pa());

        LOW_L1_PAGE_TABLE.entries[vpn1_index(QEMU_TEST_BASE)] = table_pte(dev_l0_table_pa());
        DEV_L0_PAGE_TABLE.entries[vpn0_index(QEMU_TEST_BASE)] =
            leaf_pte(QEMU_TEST_BASE, PTE_R | PTE_W | PTE_A | PTE_D);

        LOW_L1_PAGE_TABLE.entries[vpn1_index(UART0_ADDR)] =
            leaf_pte(UART0_ADDR, PTE_R | PTE_W | PTE_A | PTE_D);

        LOW_L1_PAGE_TABLE.entries[vpn1_index(USER_TEXT_VA)] = table_pte(user_l0_table_pa());
        USER_L0_PAGE_TABLE.entries[vpn0_index(USER_TEXT_VA)] =
            leaf_pte(user_code_page_pa(), PTE_R | PTE_X | PTE_U | PTE_A);
        USER_L0_PAGE_TABLE.entries[vpn0_index(USER_SHARED_VA)] =
            leaf_pte(user_shared_page_pa(), PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);
        USER_L0_PAGE_TABLE.entries[vpn0_index(USER_STACK_VA)] =
            leaf_pte(user_stack_page_pa(), PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);

        for entry_index in 0..(KERNEL_WINDOW_SIZE / MEGA_PAGE_SIZE) {
            let pa = KERNEL_BASE + entry_index * MEGA_PAGE_SIZE;
            KERNEL_L1_PAGE_TABLE.entries[entry_index] =
                leaf_pte(pa, PTE_R | PTE_W | PTE_X | PTE_A | PTE_D);
        }
    }
}

fn activate_address_space() {
    let satp = SATP_MODE_SV39 | (root_page_table_pa() >> PAGE_SHIFT);

    unsafe {
        ROOT_SATP = satp;
        asm!("csrw satp, {}", in(reg) satp, options(nostack, nomem));
        asm!("sfence.vma zero, zero", options(nostack, nomem));
        asm!("fence.i", options(nostack, nomem));
    }
}

fn prepare_user_frame() {
    unsafe {
        USER_FRAME = TrapFrame::zeroed();
        USER_FRAME.epc = USER_TEXT_VA;
        USER_FRAME.saved_sp = USER_STACK_TOP;
    }
}

fn upsert_vma(kind: usize, start: usize, end: usize, flags: usize) {
    for index in 0..MAX_VMAS {
        unsafe {
            if VMAS[index].active && VMAS[index].kind == kind {
                VMAS[index].start = start;
                VMAS[index].end = end;
                VMAS[index].flags = flags;
                return;
            }
        }
    }

    insert_vma(start, end, flags, kind);
}

fn insert_vma(start: usize, end: usize, flags: usize, kind: usize) {
    for index in 0..MAX_VMAS {
        unsafe {
            if !VMAS[index].active {
                VMAS[index] = Vma {
                    active: true,
                    start,
                    end,
                    flags,
                    kind,
                    fault_count: 0,
                    mapped_pages: 0,
                };
                return;
            }
        }
    }

    kernel_fail("out of VMA slots");
}

fn find_vma_index(va: usize) -> Option<usize> {
    for index in 0..MAX_VMAS {
        let vma = unsafe { VMAS[index] };
        if vma.active && va >= vma.start && va < vma.end {
            return Some(index);
        }
    }
    None
}

fn allocate_lazy_page() -> usize {
    let page_index = unsafe { NEXT_FREE_LAZY_PAGE };
    if page_index >= MAX_LAZY_PAGES {
        kernel_fail("lazy page pool exhausted");
    }

    let pa = lazy_page_pool_pa() + page_index * PAGE_SIZE;

    unsafe {
        ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
        NEXT_FREE_LAZY_PAGE += 1;
        LAZY_STATS.page_alloc_count += 1;
    }

    pa
}

fn install_user_leaf(va: usize, pa: usize, flags: usize) {
    let entry = unsafe { &mut (*ptr::addr_of_mut!(USER_L0_PAGE_TABLE)).entries[vpn0_index(va)] };

    if pte_is_valid(*entry) {
        kernel_fail("attempted to install an already-present lazy PTE");
    }

    *entry = leaf_pte(pa, flags);

    unsafe {
        LAZY_STATS.map_install_count += 1;
        asm!("sfence.vma {}, zero", in(reg) va, options(nostack, nomem));
    }
}

fn user_leaf_flags_for_vma(vma_flags: usize) -> usize {
    let mut flags = PTE_U | PTE_A;
    if (vma_flags & VMA_READ) != 0 {
        flags |= PTE_R;
    }
    if (vma_flags & VMA_WRITE) != 0 {
        flags |= PTE_W | PTE_D;
    }
    flags
}

fn print_vmas() {
    for index in 0..MAX_VMAS {
        let vma = unsafe { VMAS[index] };
        if vma.active {
            println!(
                "[vma] slot={} kind={} range=[{:#x}, {:#x}) flags={} faults={} mapped_pages={}",
                index,
                vma_kind_name(vma.kind),
                vma.start,
                vma.end,
                vma_flags_name(vma.flags),
                vma.fault_count,
                vma.mapped_pages
            );
        }
    }
}

fn print_walk(name: &str, va: usize, walk: WalkResult) {
    println!(
        "[pt] {} va={:#x} vpn=({},{},{}) level={} root_pte={:#018x} l1_pte={:#018x} l0_pte={:#018x} leaf_pte={:#018x} leaf_pa={:#x} flags={}",
        name,
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

fn walk_virtual(va: usize) -> WalkResult {
    let mut result = WalkResult::zeroed();
    let root = ptr::addr_of!(ROOT_PAGE_TABLE);

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

fn pte_has_user(pte: usize) -> bool {
    (pte & PTE_U) != 0
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

fn vpn2_index(va: usize) -> usize {
    (va >> 30) & 0x1ff
}

fn vpn1_index(va: usize) -> usize {
    (va >> 21) & 0x1ff
}

fn vpn0_index(va: usize) -> usize {
    (va >> 12) & 0x1ff
}

fn clear_page_table(table: *mut PageTable) {
    for entry_index in 0..PAGE_TABLE_ENTRIES {
        unsafe {
            (*table).entries[entry_index] = 0;
        }
    }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

fn clear_sum() {
    unsafe {
        asm!("csrc sstatus, {}", in(reg) SSTATUS_SUM, options(nostack, nomem));
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

fn fault_kind_name(scause: usize) -> &'static str {
    match scause {
        LOAD_PAGE_FAULT => "load",
        STORE_PAGE_FAULT => "store",
        _ => "unknown",
    }
}

fn vma_kind_name(kind: usize) -> &'static str {
    match kind {
        VMA_KIND_HEAP => "heap",
        VMA_KIND_MMAP => "mmap",
        _ => "unknown",
    }
}

fn vma_flags_name(flags: usize) -> &'static str {
    if flags == (VMA_READ | VMA_WRITE) {
        "rw"
    } else if flags == VMA_READ {
        "r"
    } else if flags == VMA_WRITE {
        "w"
    } else {
        "-"
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

fn read_satp() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, satp", out(reg) value, options(nostack, nomem));
    }
    value
}

fn root_page_table_pa() -> usize {
    ptr::addr_of!(ROOT_PAGE_TABLE) as usize
}

fn low_l1_table_pa() -> usize {
    ptr::addr_of!(LOW_L1_PAGE_TABLE) as usize
}

fn dev_l0_table_pa() -> usize {
    ptr::addr_of!(DEV_L0_PAGE_TABLE) as usize
}

fn user_l0_table_pa() -> usize {
    ptr::addr_of!(USER_L0_PAGE_TABLE) as usize
}

fn kernel_l1_table_pa() -> usize {
    ptr::addr_of!(KERNEL_L1_PAGE_TABLE) as usize
}

fn user_code_page_pa() -> usize {
    ptr::addr_of!(USER_CODE_PAGE) as usize
}

fn user_shared_page_pa() -> usize {
    ptr::addr_of!(USER_SHARED_PAGE) as usize
}

fn user_stack_page_pa() -> usize {
    ptr::addr_of!(USER_STACK_PAGE) as usize
}

fn lazy_page_pool_pa() -> usize {
    unsafe { ptr::addr_of_mut!(LAZY_PAGE_POOL.bytes) as usize }
}

fn user_shared() -> &'static UserSharedData {
    unsafe { &*ptr::addr_of!(USER_SHARED_PAGE.bytes).cast::<UserSharedData>() }
}

fn user_shared_mut() -> &'static mut UserSharedData {
    unsafe { &mut *ptr::addr_of_mut!(USER_SHARED_PAGE.bytes).cast::<UserSharedData>() }
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

fn yes_no(condition: bool) -> &'static str {
    if condition {
        "YES"
    } else {
        "NO"
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1);
}
