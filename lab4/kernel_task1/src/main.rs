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
const USER_STACK_VA: usize = 0x0040_2000;
const USER_STACK_TOP: usize = USER_STACK_VA + PAGE_SIZE;
const KERNEL_PROBE_VA: usize = KERNEL_BASE;

const USER_STAGE_BEFORE_PROBE: u64 = 0xfeed_face_0000_0001;

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

const USER_ENV_CALL: usize = 8;
const INSTRUCTION_PAGE_FAULT: usize = 12;
const LOAD_PAGE_FAULT: usize = 13;
const STORE_PAGE_FAULT: usize = 15;

const MEDELEG_MASK: usize = (1 << USER_ENV_CALL)
    | (1 << INSTRUCTION_PAGE_FAULT)
    | (1 << LOAD_PAGE_FAULT)
    | (1 << STORE_PAGE_FAULT);

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

#[repr(C)]
struct UserSharedData {
    seed: u64,
    readback: u64,
    stage_marker: u64,
    unexpected_kernel_value: u64,
    stack_echo: u64,
    unexpected_syscall_code: u64,
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
struct UserEvidence {
    seed: u64,
    readback: u64,
    stage_marker: u64,
    unexpected_kernel_value: u64,
    stack_echo: u64,
    unexpected_syscall_code: u64,
}

impl UserEvidence {
    fn collect() -> Self {
        let shared = user_shared();
        Self {
            seed: shared.seed,
            readback: shared.readback,
            stage_marker: shared.stage_marker,
            unexpected_kernel_value: shared.unexpected_kernel_value,
            stack_echo: shared.stack_echo,
            unexpected_syscall_code: shared.unexpected_syscall_code,
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
static mut USER_DATA_PAGE: Page = Page::zeroed();
static mut USER_STACK_PAGE: Page = Page::zeroed();
static mut USER_FRAME: TrapFrame = TrapFrame::zeroed();
static mut ROOT_SATP: usize = 0;

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

    initialize_user_pages();
    build_single_page_table();
    activate_single_address_space();
    prepare_user_frame();

    println!("[kernel] booted in S-mode with Sv39 enabled");
    println!("[kernel] LAB4 kernel task1 single page-table mechanism");
    println!(
        "[kernel] isolation policy: one shared root page table, kernel leaves keep U=0, user text/data/stack leaves set U=1"
    );
    println!(
        "[kernel] satp(root)={:#x} root_pa={:#x}",
        unsafe { ROOT_SATP },
        root_page_table_pa()
    );
    println!(
        "[kernel] windows: kernel_identity=[{:#x}, {:#x}) user=[{:#x}, {:#x})",
        KERNEL_BASE,
        KERNEL_BASE + KERNEL_WINDOW_SIZE,
        USER_TEXT_VA,
        USER_STACK_TOP
    );
    println!(
        "[kernel] user pages: code_pa={:#x} data_pa={:#x} stack_pa={:#x} copied_program={} bytes",
        user_code_page_pa(),
        user_data_page_pa(),
        user_stack_page_pa(),
        user_program_len()
    );

    print_walk(
        "kernel_probe",
        KERNEL_PROBE_VA,
        walk_virtual(KERNEL_PROBE_VA),
    );
    print_walk("user_text", USER_TEXT_VA, walk_virtual(USER_TEXT_VA));
    print_walk("user_data", USER_DATA_VA, walk_virtual(USER_DATA_VA));
    print_walk("user_stack", USER_STACK_VA, walk_virtual(USER_STACK_VA));

    println!(
        "[kernel] entering U-mode probe at user_text={:#x} with user_sp={:#x}",
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
    let mcause = read_mcause();
    println!(
        "[kernel] unexpected machine trap: mcause={:#x} mepc={:#x} mtval={:#x}",
        mcause,
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
        LOAD_PAGE_FAULT => handle_expected_kernel_access_fault(frame, stval),
        USER_ENV_CALL => handle_unexpected_user_ecall(frame, stval),
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

fn handle_expected_kernel_access_fault(frame: &TrapFrame, stval: usize) -> ! {
    let kernel_walk = walk_virtual(KERNEL_PROBE_VA);
    let user_text_walk = walk_virtual(USER_TEXT_VA);
    let user_data_walk = walk_virtual(USER_DATA_VA);
    let user_stack_walk = walk_virtual(USER_STACK_VA);
    let evidence = UserEvidence::collect();
    let satp = read_satp();

    println!(
        "[kernel] trapped user fault: scause={:#x} sepc={:#x} stval={:#x} satp={:#x}",
        LOAD_PAGE_FAULT, frame.epc, stval, satp
    );
    println!(
        "[kernel] user evidence: seed={:#018x} readback={:#018x} stack_echo={:#018x} stage={:#018x} unexpected_kernel_value={:#018x} unexpected_syscall={:#018x}",
        evidence.seed,
        evidence.readback,
        evidence.stack_echo,
        evidence.stage_marker,
        evidence.unexpected_kernel_value,
        evidence.unexpected_syscall_code
    );

    print_walk("kernel_probe", KERNEL_PROBE_VA, kernel_walk);
    print_walk("user_text", USER_TEXT_VA, user_text_walk);
    print_walk("user_data", USER_DATA_VA, user_data_walk);
    print_walk("user_stack", USER_STACK_VA, user_stack_walk);

    let same_tree_ok = satp == unsafe { ROOT_SATP }
        && kernel_walk.root_pte != 0
        && user_text_walk.root_pte != 0
        && kernel_walk.vpn2 != user_text_walk.vpn2;
    let u_flag_ok = !pte_has_user(kernel_walk.leaf_pte)
        && pte_has_user(user_text_walk.leaf_pte)
        && pte_has_user(user_data_walk.leaf_pte)
        && pte_has_user(user_stack_walk.leaf_pte);
    let fault_ok = stval == KERNEL_PROBE_VA
        && evidence.readback == evidence.seed
        && evidence.stack_echo == evidence.seed
        && evidence.stage_marker == USER_STAGE_BEFORE_PROBE
        && evidence.unexpected_kernel_value == 0
        && evidence.unexpected_syscall_code == 0;

    println!(
        "[kernel] acceptance same multi-level root contains kernel and user mappings: {}",
        pass_fail(same_tree_ok)
    );
    println!(
        "[kernel] acceptance kernel leaves clear U and user leaves set U: {}",
        pass_fail(u_flag_ok)
    );
    println!(
        "[kernel] acceptance user kernel-probe load trapped as delegated load page fault: {}",
        pass_fail(fault_ok)
    );

    qemu_exit(if same_tree_ok && u_flag_ok && fault_ok {
        0
    } else {
        1
    })
}

fn handle_unexpected_user_ecall(frame: &TrapFrame, stval: usize) -> ! {
    let shared = user_shared_mut();
    shared.unexpected_syscall_code = frame.a0 as u64;

    println!(
        "[kernel] unexpected user ecall: a0={:#x} a7={} sepc={:#x} stval={:#x}",
        frame.a0, frame.a7, frame.epc, stval
    );
    println!(
        "[kernel] this means the user probe managed to execute past the forbidden kernel load"
    );
    qemu_exit(1)
}

fn initialize_user_pages() {
    unsafe {
        USER_CODE_PAGE = Page::zeroed();
        USER_DATA_PAGE = Page::zeroed();
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
    shared.seed = 0x1122_3344_5566_7788;
    shared.readback = 0;
    shared.stage_marker = 0;
    shared.unexpected_kernel_value = 0;
    shared.stack_echo = 0;
    shared.unexpected_syscall_code = 0;
}

fn build_single_page_table() {
    clear_page_table(ptr::addr_of_mut!(ROOT_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(LOW_L1_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(DEV_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(USER_L0_PAGE_TABLE));
    clear_page_table(ptr::addr_of_mut!(KERNEL_L1_PAGE_TABLE));

    unsafe {
        ROOT_PAGE_TABLE.entries[vpn2_index(KERNEL_PROBE_VA)] = table_pte(kernel_l1_table_pa());
        ROOT_PAGE_TABLE.entries[vpn2_index(USER_TEXT_VA)] = table_pte(low_l1_table_pa());

        LOW_L1_PAGE_TABLE.entries[vpn1_index(QEMU_TEST_BASE)] = table_pte(dev_l0_table_pa());
        DEV_L0_PAGE_TABLE.entries[vpn0_index(QEMU_TEST_BASE)] =
            leaf_pte(QEMU_TEST_BASE, PTE_R | PTE_W | PTE_A | PTE_D);

        LOW_L1_PAGE_TABLE.entries[vpn1_index(UART0_ADDR)] =
            leaf_pte(UART0_ADDR, PTE_R | PTE_W | PTE_A | PTE_D);

        LOW_L1_PAGE_TABLE.entries[vpn1_index(USER_TEXT_VA)] = table_pte(user_l0_table_pa());
        USER_L0_PAGE_TABLE.entries[vpn0_index(USER_TEXT_VA)] =
            leaf_pte(user_code_page_pa(), PTE_R | PTE_X | PTE_U | PTE_A);
        USER_L0_PAGE_TABLE.entries[vpn0_index(USER_DATA_VA)] =
            leaf_pte(user_data_page_pa(), PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);
        USER_L0_PAGE_TABLE.entries[vpn0_index(USER_STACK_VA)] =
            leaf_pte(user_stack_page_pa(), PTE_R | PTE_W | PTE_U | PTE_A | PTE_D);

        for entry_index in 0..(KERNEL_WINDOW_SIZE / MEGA_PAGE_SIZE) {
            let pa = KERNEL_BASE + entry_index * MEGA_PAGE_SIZE;
            KERNEL_L1_PAGE_TABLE.entries[entry_index] =
                leaf_pte(pa, PTE_R | PTE_W | PTE_X | PTE_A | PTE_D);
        }
    }
}

fn activate_single_address_space() {
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

fn user_data_page_pa() -> usize {
    ptr::addr_of!(USER_DATA_PAGE) as usize
}

fn user_stack_page_pa() -> usize {
    ptr::addr_of!(USER_STACK_PAGE) as usize
}

fn user_shared() -> &'static UserSharedData {
    unsafe { &*ptr::addr_of!(USER_DATA_PAGE.bytes).cast::<UserSharedData>() }
}

fn user_shared_mut() -> &'static mut UserSharedData {
    unsafe { &mut *ptr::addr_of_mut!(USER_DATA_PAGE.bytes).cast::<UserSharedData>() }
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
