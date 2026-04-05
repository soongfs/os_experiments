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
const USER_WORKINGSET_BASE: usize = 0x0041_0000;
const USER_WORKINGSET_PAGES: usize = 3;
const USER_WORKINGSET_LIMIT: usize = USER_WORKINGSET_BASE + USER_WORKINGSET_PAGES * PAGE_SIZE;

const PAGE0_VALUE: u64 = 0x1111_2222_3333_4444;
const PAGE2_VALUE: u64 = 0x9999_aaaa_bbbb_cccc;
const STAGE_DONE: u64 = 0xabcd_ef01_2345_6789;
const WORKINGSET_ACCESS_COUNT: usize = 6;

const SATP_MODE_SV39: usize = 8usize << 60;

const PTE_V: usize = 1 << 0;
const PTE_R: usize = 1 << 1;
const PTE_W: usize = 1 << 2;
const PTE_X: usize = 1 << 3;
const PTE_U: usize = 1 << 4;
const PTE_G: usize = 1 << 5;
const PTE_A: usize = 1 << 6;
const PTE_D: usize = 1 << 7;
const PTE_SWAP: usize = 1 << 8;

const SSTATUS_SUM: usize = 1 << 18;

const SYS_EXIT: usize = 1;

const USER_ENV_CALL: usize = 8;
const LOAD_PAGE_FAULT: usize = 13;
const STORE_PAGE_FAULT: usize = 15;

const MEDELEG_MASK: usize = (1 << USER_ENV_CALL) | (1 << LOAD_PAGE_FAULT) | (1 << STORE_PAGE_FAULT);

const RESIDENT_FRAME_COUNT: usize = 2;
const SWAP_SLOT_COUNT: usize = 4;

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
struct FramePool {
    bytes: [u8; PAGE_SIZE * RESIDENT_FRAME_COUNT],
}

impl FramePool {
    const fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE * RESIDENT_FRAME_COUNT],
        }
    }
}

#[repr(align(4096))]
struct SwapArea {
    bytes: [u8; PAGE_SIZE * SWAP_SLOT_COUNT],
}

impl SwapArea {
    const fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE * SWAP_SLOT_COUNT],
        }
    }
}

#[repr(C)]
struct UserSharedData {
    stage_marker: u64,
    page2_hit_readback: u64,
    page0_swapin_readback: u64,
    page0_hit_again: u64,
    exit_code: u64,
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
struct WorkingPage {
    resident: bool,
    frame_index: usize,
    swap_slot: usize,
    has_swap: bool,
}

impl WorkingPage {
    const fn zeroed() -> Self {
        Self {
            resident: false,
            frame_index: 0,
            swap_slot: 0,
            has_swap: false,
        }
    }
}

#[derive(Clone, Copy)]
struct ResidentFrame {
    in_use: bool,
    page_index: usize,
}

impl ResidentFrame {
    const fn empty() -> Self {
        Self {
            in_use: false,
            page_index: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct SwapStats {
    page_faults: usize,
    load_faults: usize,
    store_faults: usize,
    lazy_allocs: usize,
    swap_outs: usize,
    swap_ins: usize,
    clock_scans: usize,
    second_chances: usize,
    accesses: usize,
    hits: usize,
}

impl SwapStats {
    const fn zeroed() -> Self {
        Self {
            page_faults: 0,
            load_faults: 0,
            store_faults: 0,
            lazy_allocs: 0,
            swap_outs: 0,
            swap_ins: 0,
            clock_scans: 0,
            second_chances: 0,
            accesses: 0,
            hits: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct SwapEvidence {
    replacement_triggered: bool,
    evicted_page_index: usize,
    evicted_slot: usize,
    evicted_swap_pte: usize,
    swapin_page_index: usize,
    swapin_slot: usize,
    swapin_restored_pa: usize,
}

impl SwapEvidence {
    const fn zeroed() -> Self {
        Self {
            replacement_triggered: false,
            evicted_page_index: 0,
            evicted_slot: 0,
            evicted_swap_pte: 0,
            swapin_page_index: 0,
            swapin_slot: 0,
            swapin_restored_pa: 0,
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
            if (pte & PTE_SWAP) != 0 { 'S' } else { '-' },
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

static mut RESIDENT_FRAME_POOL: FramePool = FramePool::zeroed();
static mut SWAP_AREA: SwapArea = SwapArea::zeroed();
static mut WORKING_PAGES: [WorkingPage; USER_WORKINGSET_PAGES] =
    [WorkingPage::zeroed(); USER_WORKINGSET_PAGES];
static mut RESIDENT_FRAMES: [ResidentFrame; RESIDENT_FRAME_COUNT] =
    [ResidentFrame::empty(); RESIDENT_FRAME_COUNT];
static mut SWAP_SLOT_IN_USE: [bool; SWAP_SLOT_COUNT] = [false; SWAP_SLOT_COUNT];
static mut CLOCK_HAND: usize = 0;
static mut SWAP_STATS: SwapStats = SwapStats::zeroed();
static mut SWAP_EVIDENCE: SwapEvidence = SwapEvidence::zeroed();

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
    build_page_tables();
    activate_address_space();
    prepare_user_frame();

    println!("[kernel] booted in S-mode with Sv39 enabled");
    println!("[kernel] LAB4 kernel task4 swap in/out with Clock replacement");
    println!(
        "[kernel] policy: working set has {} virtual pages but only {} resident frames; evicted pages are encoded as swap-slot PTEs and restored on demand",
        USER_WORKINGSET_PAGES,
        RESIDENT_FRAME_COUNT
    );
    println!(
        "[kernel] satp(root)={:#x} root_pa={:#x}",
        unsafe { ROOT_SATP },
        root_page_table_pa()
    );
    println!(
        "[kernel] swap backend: {} slots in simulated swap disk area at pa={:#x}",
        SWAP_SLOT_COUNT,
        swap_area_pa()
    );
    print_walk("user_text", USER_TEXT_VA, walk_virtual(USER_TEXT_VA));
    print_walk("user_shared", USER_SHARED_VA, walk_virtual(USER_SHARED_VA));
    print_walk("user_stack", USER_STACK_VA, walk_virtual(USER_STACK_VA));
    print_walk("working_page0_before", USER_WORKINGSET_BASE, walk_virtual(USER_WORKINGSET_BASE));
    print_walk(
        "working_page1_before",
        USER_WORKINGSET_BASE + PAGE_SIZE,
        walk_virtual(USER_WORKINGSET_BASE + PAGE_SIZE),
    );
    print_walk(
        "working_page2_before",
        USER_WORKINGSET_BASE + 2 * PAGE_SIZE,
        walk_virtual(USER_WORKINGSET_BASE + 2 * PAGE_SIZE),
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
        LOAD_PAGE_FAULT | STORE_PAGE_FAULT => handle_user_page_fault(frame, scause, stval),
        _ => {
            println!(
                "[kernel] unexpected supervisor trap: scause={:#x} sepc={:#x} stval={:#x}",
                scause,
                frame.epc,
                stval
            );
            qemu_exit(1);
        }
    }
}

fn handle_user_ecall(frame: &mut TrapFrame) {
    let syscall_nr = frame.a7;
    frame.epc += 4;

    match syscall_nr {
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

fn handle_user_page_fault(frame: &mut TrapFrame, scause: usize, stval: usize) {
    let fault_va = align_down(stval, PAGE_SIZE);
    if !is_workingset_va(fault_va) {
        println!(
            "[kernel] fault outside managed working set: scause={:#x} sepc={:#x} stval={:#x}",
            scause,
            frame.epc,
            stval
        );
        qemu_exit(1);
    }

    let page_index = working_page_index(fault_va);
    let before = walk_virtual(fault_va);

    unsafe {
        SWAP_STATS.page_faults += 1;
        if scause == LOAD_PAGE_FAULT {
            SWAP_STATS.load_faults += 1;
        } else {
            SWAP_STATS.store_faults += 1;
        }
    }

    if pte_is_swapped(before.leaf_pte) {
        swap_in_page(page_index, fault_va, scause, before, frame.epc, stval);
    } else if !pte_is_valid(before.leaf_pte) {
        lazy_allocate_page(page_index, fault_va, scause, before, frame.epc, stval);
    } else {
        println!(
            "[kernel] page fault hit an already-present mapping: va={:#x} pte={:#x}",
            fault_va,
            before.leaf_pte
        );
        qemu_exit(1);
    }
}

fn lazy_allocate_page(
    page_index: usize,
    fault_va: usize,
    scause: usize,
    before: WalkResult,
    sepc: usize,
    stval: usize,
) {
    let frame_index = acquire_frame_for_page(page_index);
    let pa = resident_frame_pa(frame_index);

    unsafe {
        ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
        WORKING_PAGES[page_index].resident = true;
        WORKING_PAGES[page_index].frame_index = frame_index;
        WORKING_PAGES[page_index].has_swap = false;
        RESIDENT_FRAMES[frame_index].in_use = true;
        RESIDENT_FRAMES[frame_index].page_index = page_index;
        SWAP_STATS.lazy_allocs += 1;
    }

    install_working_leaf(fault_va, pa);

    println!(
        "[kernel] lazy map kind={} sepc={:#x} stval={:#x} page_index={} frame={} pa={:#x}",
        fault_kind_name(scause),
        sepc,
        stval,
        page_index,
        frame_index,
        pa
    );
    print_walk_result("fault_before", fault_va, before);
    print_walk("fault_after", fault_va, walk_virtual(fault_va));
}

fn swap_in_page(
    page_index: usize,
    fault_va: usize,
    scause: usize,
    before: WalkResult,
    sepc: usize,
    stval: usize,
) {
    let slot = swap_slot_from_pte(before.leaf_pte);
    let frame_index = acquire_frame_for_page(page_index);
    let pa = resident_frame_pa(frame_index);
    let slot_pa = swap_slot_pa(slot);

    copy_page(pa, slot_pa);

    unsafe {
        WORKING_PAGES[page_index].resident = true;
        WORKING_PAGES[page_index].frame_index = frame_index;
        WORKING_PAGES[page_index].has_swap = false;
        WORKING_PAGES[page_index].swap_slot = slot;
        RESIDENT_FRAMES[frame_index].in_use = true;
        RESIDENT_FRAMES[frame_index].page_index = page_index;
        SWAP_SLOT_IN_USE[slot] = false;
        SWAP_STATS.swap_ins += 1;
        SWAP_EVIDENCE.swapin_page_index = page_index;
        SWAP_EVIDENCE.swapin_slot = slot;
        SWAP_EVIDENCE.swapin_restored_pa = pa;
    }

    install_working_leaf(fault_va, pa);

    println!(
        "[kernel] swap in kind={} sepc={:#x} stval={:#x} page_index={} slot={} restored_pa={:#x} swap_pte={:#x}",
        fault_kind_name(scause),
        sepc,
        stval,
        page_index,
        slot,
        pa,
        before.leaf_pte
    );
    print_walk_result("swap_before", fault_va, before);
    print_walk("swap_after", fault_va, walk_virtual(fault_va));
}

fn acquire_frame_for_page(page_index: usize) -> usize {
    for frame_index in 0..RESIDENT_FRAME_COUNT {
        if !unsafe { RESIDENT_FRAMES[frame_index].in_use } {
            return frame_index;
        }
    }

    unsafe {
        SWAP_EVIDENCE.replacement_triggered = true;
    }
    evict_with_clock(page_index)
}

fn evict_with_clock(incoming_page_index: usize) -> usize {
    loop {
        let frame_index = unsafe { CLOCK_HAND };
        let victim_page_index = unsafe { RESIDENT_FRAMES[frame_index].page_index };
        let victim_va = working_page_va(victim_page_index);
        let pte = unsafe { USER_L0_PAGE_TABLE.entries[vpn0_index(victim_va)] };

        unsafe {
            SWAP_STATS.clock_scans += 1;
        }

        if pte_has_accessed(pte) {
            let cleared = pte & !PTE_A;
            unsafe {
                USER_L0_PAGE_TABLE.entries[vpn0_index(victim_va)] = cleared;
                SWAP_STATS.second_chances += 1;
            }
            sfence_va(victim_va);
            println!(
                "[clock] frame={} victim_page={} va={:#x} second_chance old_pte={:#x} new_pte={:#x}",
                frame_index,
                victim_page_index,
                victim_va,
                pte,
                cleared
            );
            unsafe {
                CLOCK_HAND = (CLOCK_HAND + 1) % RESIDENT_FRAME_COUNT;
            }
            continue;
        }

        let slot = allocate_swap_slot();
        let victim_pa = resident_frame_pa(frame_index);
        copy_page(swap_slot_pa(slot), victim_pa);

        let swap_entry = swap_pte(slot);
        unsafe {
            USER_L0_PAGE_TABLE.entries[vpn0_index(victim_va)] = swap_entry;
            WORKING_PAGES[victim_page_index].resident = false;
            WORKING_PAGES[victim_page_index].has_swap = true;
            WORKING_PAGES[victim_page_index].swap_slot = slot;
            RESIDENT_FRAMES[frame_index].in_use = false;
            SWAP_STATS.swap_outs += 1;
            if SWAP_EVIDENCE.evicted_swap_pte == 0 {
                SWAP_EVIDENCE.evicted_page_index = victim_page_index;
                SWAP_EVIDENCE.evicted_slot = slot;
                SWAP_EVIDENCE.evicted_swap_pte = swap_entry;
            }
            CLOCK_HAND = (CLOCK_HAND + 1) % RESIDENT_FRAME_COUNT;
        }
        sfence_va(victim_va);

        println!(
            "[kernel] swap out trigger incoming_page={} victim_page={} frame={} victim_va={:#x} victim_pa={:#x} slot={} swap_pte={:#x}",
            incoming_page_index,
            victim_page_index,
            frame_index,
            victim_va,
            victim_pa,
            slot,
            swap_entry
        );
        print_walk("victim_after_swap", victim_va, walk_virtual(victim_va));
        return frame_index;
    }
}

fn finish_user_program(exit_code: usize) -> ! {
    let shared = user_shared();
    let page0_walk = walk_virtual(USER_WORKINGSET_BASE);
    let page1_walk = walk_virtual(USER_WORKINGSET_BASE + PAGE_SIZE);
    let page2_walk = walk_virtual(USER_WORKINGSET_BASE + 2 * PAGE_SIZE);

    unsafe {
        SWAP_STATS.accesses = WORKINGSET_ACCESS_COUNT;
        SWAP_STATS.hits = WORKINGSET_ACCESS_COUNT - SWAP_STATS.page_faults;
    }

    println!("[kernel] user program requested exit with code={}", exit_code);
    println!(
        "[kernel] counters: accesses={} hits={} page_faults={} load_faults={} store_faults={} lazy_allocs={} swap_outs={} swap_ins={} clock_scans={} second_chances={}",
        unsafe { SWAP_STATS.accesses },
        unsafe { SWAP_STATS.hits },
        unsafe { SWAP_STATS.page_faults },
        unsafe { SWAP_STATS.load_faults },
        unsafe { SWAP_STATS.store_faults },
        unsafe { SWAP_STATS.lazy_allocs },
        unsafe { SWAP_STATS.swap_outs },
        unsafe { SWAP_STATS.swap_ins },
        unsafe { SWAP_STATS.clock_scans },
        unsafe { SWAP_STATS.second_chances }
    );
    println!(
        "[kernel] user evidence: stage={:#018x} page2_hit={:#018x} page0_swapin={:#018x} page0_hit_again={:#018x}",
        shared.stage_marker,
        shared.page2_hit_readback,
        shared.page0_swapin_readback,
        shared.page0_hit_again
    );
    println!(
        "[kernel] swap evidence: replacement_triggered={} first_evicted_page={} first_evicted_slot={} first_evicted_swap_pte={:#x} swapin_page={} swapin_slot={} swapin_restored_pa={:#x}",
        pass_fail(unsafe { SWAP_EVIDENCE.replacement_triggered }),
        unsafe { SWAP_EVIDENCE.evicted_page_index },
        unsafe { SWAP_EVIDENCE.evicted_slot },
        unsafe { SWAP_EVIDENCE.evicted_swap_pte },
        unsafe { SWAP_EVIDENCE.swapin_page_index },
        unsafe { SWAP_EVIDENCE.swapin_slot },
        unsafe { SWAP_EVIDENCE.swapin_restored_pa }
    );
    print_walk("page0_final", USER_WORKINGSET_BASE, page0_walk);
    print_walk("page1_final", USER_WORKINGSET_BASE + PAGE_SIZE, page1_walk);
    print_walk("page2_final", USER_WORKINGSET_BASE + 2 * PAGE_SIZE, page2_walk);

    let replacement_ok = unsafe { SWAP_EVIDENCE.replacement_triggered }
        && unsafe { SWAP_STATS.swap_outs >= 1 }
        && unsafe { SWAP_STATS.clock_scans >= 1 };
    let swap_pte_ok = pte_is_swapped(unsafe { SWAP_EVIDENCE.evicted_swap_pte })
        && swap_slot_from_pte(unsafe { SWAP_EVIDENCE.evicted_swap_pte }) == unsafe { SWAP_EVIDENCE.evicted_slot }
        && unsafe { SWAP_EVIDENCE.evicted_page_index == 0 };
    let swapin_ok = unsafe { SWAP_STATS.swap_ins >= 1 }
        && unsafe { SWAP_EVIDENCE.swapin_page_index == 0 }
        && unsafe { SWAP_EVIDENCE.swapin_slot == SWAP_EVIDENCE.evicted_slot }
        && pte_is_valid(page0_walk.leaf_pte)
        && !pte_is_swapped(page0_walk.leaf_pte)
        && pte_has_write(page0_walk.leaf_pte)
        && shared.page0_swapin_readback == PAGE0_VALUE
        && shared.page0_hit_again == PAGE0_VALUE
        && shared.page2_hit_readback == PAGE2_VALUE;
    let counts_ok = exit_code == 0
        && shared.stage_marker == STAGE_DONE
        && unsafe { SWAP_STATS.page_faults == 4 }
        && unsafe { SWAP_STATS.load_faults == 1 }
        && unsafe { SWAP_STATS.store_faults == 3 }
        && unsafe { SWAP_STATS.lazy_allocs == 3 }
        && unsafe { SWAP_STATS.swap_outs == 2 }
        && unsafe { SWAP_STATS.swap_ins == 1 }
        && unsafe { SWAP_STATS.hits == 2 };

    println!(
        "[kernel] acceptance memory pressure triggers Clock replacement without crashing: {}",
        pass_fail(replacement_ok)
    );
    println!(
        "[kernel] acceptance swapped-out PTE encodes the correct swap slot: {}",
        pass_fail(swap_pte_ok)
    );
    println!(
        "[kernel] acceptance reaccess faults, swaps page back in, and restores mapping/data: {}",
        pass_fail(swapin_ok && counts_ok)
    );

    qemu_exit(if replacement_ok && swap_pte_ok && swapin_ok && counts_ok {
        0
    } else {
        1
    })
}

fn initialize_runtime() {
    unsafe {
        USER_FRAME = TrapFrame::zeroed();
        CLOCK_HAND = 0;
        SWAP_STATS = SwapStats::zeroed();
        SWAP_EVIDENCE = SwapEvidence::zeroed();
    }

    clear_page_table(ptr::addr_of_mut!(ROOT_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(LOW_L1_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(DEV_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(USER_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(KERNEL_L1_PAGE_TABLE));
    zero_page(ptr::addr_of_mut!(USER_CODE_PAGE));
    zero_page(ptr::addr_of_mut!(USER_SHARED_PAGE));
    zero_page(ptr::addr_of_mut!(USER_STACK_PAGE));
    zero_frame_pool(ptr::addr_of_mut!(RESIDENT_FRAME_POOL));
    zero_swap_area(ptr::addr_of_mut!(SWAP_AREA));

    for page_index in 0..USER_WORKINGSET_PAGES {
        unsafe {
            WORKING_PAGES[page_index] = WorkingPage::zeroed();
        }
    }

    for frame_index in 0..RESIDENT_FRAME_COUNT {
        unsafe {
            RESIDENT_FRAMES[frame_index] = ResidentFrame::empty();
        }
    }

    for slot in 0..SWAP_SLOT_COUNT {
        unsafe {
            SWAP_SLOT_IN_USE[slot] = false;
        }
    }

    let program_bytes = unsafe {
        slice::from_raw_parts(ptr::addr_of!(__user_program_start), user_program_len())
    };
    unsafe {
        ptr::copy_nonoverlapping(
            program_bytes.as_ptr(),
            ptr::addr_of_mut!(USER_CODE_PAGE.bytes).cast::<u8>(),
            program_bytes.len(),
        );
    }

    let shared = user_shared_mut();
    shared.stage_marker = 0;
    shared.page2_hit_readback = 0;
    shared.page0_swapin_readback = 0;
    shared.page0_hit_again = 0;
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

fn install_working_leaf(va: usize, pa: usize) {
    unsafe {
        USER_L0_PAGE_TABLE.entries[vpn0_index(va)] =
            leaf_pte(pa, PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);
    }
    sfence_va(va);
}

fn allocate_swap_slot() -> usize {
    for slot in 0..SWAP_SLOT_COUNT {
        if !unsafe { SWAP_SLOT_IN_USE[slot] } {
            unsafe {
                SWAP_SLOT_IN_USE[slot] = true;
            }
            return slot;
        }
    }
    kernel_fail("swap area exhausted");
}

fn copy_page(dst_pa: usize, src_pa: usize) {
    unsafe {
        ptr::copy_nonoverlapping(src_pa as *const u8, dst_pa as *mut u8, PAGE_SIZE);
    }
}

fn is_workingset_va(va: usize) -> bool {
    va >= USER_WORKINGSET_BASE && va < USER_WORKINGSET_LIMIT
}

fn working_page_index(va: usize) -> usize {
    (va - USER_WORKINGSET_BASE) / PAGE_SIZE
}

fn working_page_va(index: usize) -> usize {
    USER_WORKINGSET_BASE + index * PAGE_SIZE
}

fn resident_frame_pa(index: usize) -> usize {
    resident_frame_pool_pa() + index * PAGE_SIZE
}

fn swap_slot_pa(slot: usize) -> usize {
    swap_area_pa() + slot * PAGE_SIZE
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

fn print_walk(name: &str, va: usize, walk: WalkResult) {
    if pte_is_swapped(walk.leaf_pte) {
        let slot = swap_slot_from_pte(walk.leaf_pte);
        println!(
            "[pt] {} va={:#x} vpn=({},{},{}) level={} root_pte={:#018x} l1_pte={:#018x} l0_pte={:#018x} leaf_pte={:#018x} swap_slot={} swap_pa={:#x} flags={}",
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
            slot,
            swap_slot_pa(slot),
            PteFlags(walk.leaf_pte)
        );
    } else {
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
}

fn print_walk_result(name: &str, va: usize, walk: WalkResult) {
    print_walk(name, va, walk);
}

fn pte_is_valid(pte: usize) -> bool {
    (pte & PTE_V) != 0
}

fn pte_is_leaf(pte: usize) -> bool {
    (pte & (PTE_R | PTE_W | PTE_X)) != 0
}

fn pte_is_swapped(pte: usize) -> bool {
    (pte & PTE_V) == 0 && (pte & PTE_SWAP) != 0
}

fn pte_has_accessed(pte: usize) -> bool {
    (pte & PTE_A) != 0
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

fn swap_pte(slot: usize) -> usize {
    (slot << 10) | PTE_SWAP
}

fn swap_slot_from_pte(pte: usize) -> usize {
    pte >> 10
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

fn zero_frame_pool(pool: *mut FramePool) {
    unsafe {
        ptr::write_bytes((*pool).bytes.as_mut_ptr(), 0, PAGE_SIZE * RESIDENT_FRAME_COUNT);
    }
}

fn zero_swap_area(area: *mut SwapArea) {
    unsafe {
        ptr::write_bytes((*area).bytes.as_mut_ptr(), 0, PAGE_SIZE * SWAP_SLOT_COUNT);
    }
}

fn sfence_va(va: usize) {
    unsafe {
        asm!("sfence.vma {}, zero", in(reg) va, options(nostack, nomem));
    }
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

fn resident_frame_pool_pa() -> usize {
    unsafe { ptr::addr_of_mut!(RESIDENT_FRAME_POOL.bytes) as usize }
}

fn swap_area_pa() -> usize {
    unsafe { ptr::addr_of_mut!(SWAP_AREA.bytes) as usize }
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
    let value = if code == 0 { 0x5555 } else { (code << 16) | 0x3333 };

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
