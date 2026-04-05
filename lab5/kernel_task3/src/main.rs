#![no_std]
#![no_main]

mod console;
mod spinlock;

use core::arch::{asm, global_asm};
use core::hint::{black_box, spin_loop};
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{fence, AtomicBool, AtomicU64, AtomicUsize, Ordering};

use spinlock::SpinLock;

global_asm!(include_str!("boot.S"));

const EXPECTED_HARTS: usize = 4;
const JOB_COUNT: usize = 8;
const HEAP_SIZE: usize = 64 * 1024;
const ALLOC_ALIGN: usize = 64;
const LOCK_HOLD_SPINS: u32 = 128;
const QEMU_TEST_BASE: usize = 0x0010_0000;

#[derive(Clone, Copy)]
enum JobClass {
    Interactive,
    Compute,
}

impl JobClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Compute => "compute",
        }
    }
}

#[derive(Clone, Copy)]
struct JobDefinition {
    id: u64,
    name: &'static str,
    class: JobClass,
    alloc_bytes: usize,
    bursts: usize,
    iterations: u64,
}

#[derive(Clone, Copy)]
struct Allocation {
    offset: usize,
    size: usize,
}

struct RunQueue {
    entries: [usize; JOB_COUNT],
    next: usize,
    pop_count: u64,
}

impl RunQueue {
    const fn empty() -> Self {
        Self {
            entries: [0; JOB_COUNT],
            next: 0,
            pop_count: 0,
        }
    }

    fn reset(&mut self) {
        let mut index = 0usize;

        while index < JOB_COUNT {
            self.entries[index] = index;
            index += 1;
        }

        self.next = 0;
        self.pop_count = 0;
    }

    fn pop_next(&mut self) -> Option<usize> {
        if self.next >= JOB_COUNT {
            None
        } else {
            let job_index = self.entries[self.next];
            self.next += 1;
            self.pop_count += 1;
            Some(job_index)
        }
    }

    fn remaining(&self) -> usize {
        JOB_COUNT.saturating_sub(self.next)
    }
}

struct BumpAllocator {
    next_offset: usize,
    high_water: usize,
    allocations: u64,
}

impl BumpAllocator {
    const fn empty() -> Self {
        Self {
            next_offset: 0,
            high_water: 0,
            allocations: 0,
        }
    }

    fn reset(&mut self) {
        self.next_offset = 0;
        self.high_water = 0;
        self.allocations = 0;
    }

    fn allocate(&mut self, size: usize, align: usize) -> Option<Allocation> {
        let mask = align.wrapping_sub(1);
        let start = self.next_offset.wrapping_add(mask) & !mask;
        let end = start.checked_add(size)?;

        if end > HEAP_SIZE {
            return None;
        }

        self.next_offset = end;
        if end > self.high_water {
            self.high_water = end;
        }
        self.allocations += 1;

        Some(Allocation { offset: start, size })
    }
}

static JOB_DEFS: [JobDefinition; JOB_COUNT] = [
    JobDefinition {
        id: 1,
        name: "tty_echo",
        class: JobClass::Interactive,
        alloc_bytes: 512,
        bursts: 6,
        iterations: 28_000,
    },
    JobDefinition {
        id: 2,
        name: "batch_crc_a",
        class: JobClass::Compute,
        alloc_bytes: 4096,
        bursts: 1,
        iterations: 820_000,
    },
    JobDefinition {
        id: 3,
        name: "ui_refresh",
        class: JobClass::Interactive,
        alloc_bytes: 768,
        bursts: 5,
        iterations: 34_000,
    },
    JobDefinition {
        id: 4,
        name: "batch_crc_b",
        class: JobClass::Compute,
        alloc_bytes: 3072,
        bursts: 1,
        iterations: 760_000,
    },
    JobDefinition {
        id: 5,
        name: "pipe_shell",
        class: JobClass::Interactive,
        alloc_bytes: 640,
        bursts: 7,
        iterations: 26_000,
    },
    JobDefinition {
        id: 6,
        name: "matrix_mul",
        class: JobClass::Compute,
        alloc_bytes: 4096,
        bursts: 1,
        iterations: 900_000,
    },
    JobDefinition {
        id: 7,
        name: "input_mux",
        class: JobClass::Interactive,
        alloc_bytes: 896,
        bursts: 6,
        iterations: 30_000,
    },
    JobDefinition {
        id: 8,
        name: "log_compact",
        class: JobClass::Compute,
        alloc_bytes: 3584,
        bursts: 1,
        iterations: 880_000,
    },
];

static RUN_QUEUE: SpinLock<RunQueue> = SpinLock::new(RunQueue::empty());
static ALLOCATOR: SpinLock<BumpAllocator> = SpinLock::new(BumpAllocator::empty());

static READY_HARTS: AtomicUsize = AtomicUsize::new(0);
static BOOTED_MASK: AtomicUsize = AtomicUsize::new(0);
static START_SCHEDULING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static COMPLETED_JOBS: AtomicUsize = AtomicUsize::new(0);
static FINISHED_HARTS: AtomicUsize = AtomicUsize::new(0);
static RUNNING_JOBS: AtomicU64 = AtomicU64::new(0);
static MAX_PARALLEL_JOBS: AtomicU64 = AtomicU64::new(0);

static HART_INIT_DONE: [AtomicBool; EXPECTED_HARTS] =
    [const { AtomicBool::new(false) }; EXPECTED_HARTS];
static HART_JOBS_DONE: [AtomicU64; EXPECTED_HARTS] =
    [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static HART_ALLOC_BYTES: [AtomicU64; EXPECTED_HARTS] =
    [const { AtomicU64::new(0) }; EXPECTED_HARTS];
static JOB_COMPLETION_HART: [AtomicUsize; JOB_COUNT] =
    [const { AtomicUsize::new(usize::MAX) }; JOB_COUNT];
static JOB_CHECKSUMS: [AtomicU64; JOB_COUNT] = [const { AtomicU64::new(0) }; JOB_COUNT];

static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static mut __boot_release_flag: u64;
}

#[no_mangle]
pub extern "C" fn start_primary() -> ! {
    clear_bss();
    initialize_global_state();
    configure_pmp();

    println!("[kernel] booted in M-mode");
    println!("[kernel] LAB5 kernel task3 SMP work-queue scheduler");
    println!(
        "[kernel] qemu expected_harts={} job_count={} heap_size={} bytes alloc_align={} bytes",
        EXPECTED_HARTS,
        JOB_COUNT,
        HEAP_SIZE,
        ALLOC_ALIGN
    );
    println!(
        "[kernel] synchronization: run_queue and bump_allocator are protected by spinlocks"
    );

    for index in 0..JOB_COUNT {
        let job = JOB_DEFS[index];
        println!(
            "[kernel] job_def[{}]: id={} name={} class={} alloc_bytes={} bursts={} iterations={}",
            index,
            job.id,
            job.name,
            job.class.as_str(),
            job.alloc_bytes,
            job.bursts,
            job.iterations
        );
    }

    release_secondary_harts();
    hart_main(0, true)
}

#[no_mangle]
pub extern "C" fn start_secondary(hart_id: usize) -> ! {
    if hart_id >= EXPECTED_HARTS {
        println!(
            "[hart{}] parked: launched beyond configured expected_harts={}",
            hart_id, EXPECTED_HARTS
        );
        loop {
            spin_loop();
        }
    }

    configure_pmp();
    hart_main(hart_id, false)
}

fn hart_main(hart_id: usize, is_primary: bool) -> ! {
    let stack_pointer = read_sp();
    let ready_harts = mark_hart_initialized(hart_id);

    println!(
        "[hart{}] init complete: role={} sp={:#x} ready_harts={}/{}",
        hart_id,
        if is_primary { "primary" } else { "secondary" },
        stack_pointer,
        ready_harts,
        EXPECTED_HARTS
    );

    if is_primary {
        while READY_HARTS.load(Ordering::Acquire) < EXPECTED_HARTS {
            spin_loop();
        }

        START_SCHEDULING.store(true, Ordering::Release);
        println!(
            "[hart{}] start barrier released: ready_harts={} booted_mask={:#x}",
            hart_id,
            READY_HARTS.load(Ordering::Acquire),
            BOOTED_MASK.load(Ordering::Acquire)
        );
    } else {
        while !START_SCHEDULING.load(Ordering::Acquire) {
            spin_loop();
        }

        println!("[hart{}] start barrier observed", hart_id);
    }

    scheduler_loop(hart_id)
}

fn scheduler_loop(hart_id: usize) -> ! {
    loop {
        if let Some(job_index) = pop_next_job(hart_id) {
            run_job(hart_id, job_index);
            continue;
        }

        if COMPLETED_JOBS.load(Ordering::Acquire) == JOB_COUNT {
            break;
        }

        spin_loop();
    }

    let finished_harts = FINISHED_HARTS.fetch_add(1, Ordering::AcqRel) + 1;
    println!(
        "[hart{}] scheduler drained: jobs_done={} finished_harts={}/{}",
        hart_id,
        HART_JOBS_DONE[hart_id].load(Ordering::Relaxed),
        finished_harts,
        EXPECTED_HARTS
    );

    if finished_harts == EXPECTED_HARTS {
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

fn pop_next_job(hart_id: usize) -> Option<usize> {
    let (job_index, remaining) = {
        let mut queue = RUN_QUEUE.lock();
        hold_lock_window();
        let job_index = queue.pop_next();
        let remaining = queue.remaining();
        (job_index, remaining)
    };

    if let Some(index) = job_index {
        let job = JOB_DEFS[index];
        println!(
            "[hart{}] schedule pick: job={} name={} class={} queue_remaining={}",
            hart_id,
            job.id,
            job.name,
            job.class.as_str(),
            remaining
        );
    }

    job_index
}

fn run_job(hart_id: usize, job_index: usize) {
    let job = JOB_DEFS[job_index];
    let running_now = RUNNING_JOBS.fetch_add(1, Ordering::AcqRel) + 1;
    update_max_parallel_jobs(running_now);

    let allocation = allocate_for_job(hart_id, job_index, job.alloc_bytes);

    println!(
        "[hart{}] job start: id={} name={} class={} alloc_offset={} alloc_bytes={} running_jobs={}",
        hart_id,
        job.id,
        job.name,
        job.class.as_str(),
        allocation.offset,
        allocation.size,
        running_now
    );

    let checksum = execute_job(hart_id, job, allocation);
    let completed_jobs = COMPLETED_JOBS.fetch_add(1, Ordering::AcqRel) + 1;
    let running_after = RUNNING_JOBS.fetch_sub(1, Ordering::AcqRel) - 1;

    JOB_CHECKSUMS[job_index].store(checksum, Ordering::Release);
    JOB_COMPLETION_HART[job_index].store(hart_id, Ordering::Release);
    HART_JOBS_DONE[hart_id].fetch_add(1, Ordering::Relaxed);
    HART_ALLOC_BYTES[hart_id].fetch_add(job.alloc_bytes as u64, Ordering::Relaxed);

    println!(
        "[hart{}] job done: id={} checksum={:#x} completed_jobs={}/{} running_after={}",
        hart_id,
        job.id,
        checksum,
        completed_jobs,
        JOB_COUNT,
        running_after
    );
}

fn allocate_for_job(hart_id: usize, job_index: usize, size: usize) -> Allocation {
    let (result, high_water) = {
        let mut allocator = ALLOCATOR.lock();
        hold_lock_window();
        let result = allocator.allocate(size, ALLOC_ALIGN);
        let high_water = allocator.high_water;
        (result, high_water)
    };

    if let Some(allocation) = result {
        println!(
            "[hart{}] alloc: job={} bytes={} offset={} high_water={}",
            hart_id,
            JOB_DEFS[job_index].id,
            size,
            allocation.offset,
            high_water
        );
        allocation
    } else {
        println!(
            "[hart{}] allocator exhausted: job={} requested={} heap_size={}",
            hart_id,
            JOB_DEFS[job_index].id,
            size,
            HEAP_SIZE
        );
        qemu_exit(1)
    }
}

fn execute_job(hart_id: usize, job: JobDefinition, allocation: Allocation) -> u64 {
    let mut burst = 0usize;
    let mut acc = 0x9e37_79b9_7f4a_7c15u64
        ^ job.id
        ^ ((hart_id as u64) << 32)
        ^ (allocation.offset as u64);

    while burst < job.bursts {
        acc = busy_mix(acc ^ (burst as u64), job.iterations);
        acc ^= touch_heap(allocation, job.id, hart_id, burst);
        burst += 1;
    }

    black_box(acc)
}

fn touch_heap(allocation: Allocation, job_id: u64, hart_id: usize, burst: usize) -> u64 {
    let base = unsafe { (ptr::addr_of_mut!(HEAP) as *mut u8).add(allocation.offset) };
    let mut index = 0usize;
    let mut checksum = 0u64;
    let seed = (job_id as u8)
        .wrapping_add((hart_id as u8).wrapping_mul(7))
        .wrapping_add((burst as u8).wrapping_mul(13));

    while index < allocation.size {
        let value = seed.wrapping_add(index as u8).rotate_left((hart_id & 7) as u32);

        unsafe {
            ptr::write_volatile(base.add(index), value);
        }

        checksum = checksum.rotate_left(3).wrapping_add(value as u64);
        index += 1;
    }

    let mut verify_index = 0usize;
    while verify_index < allocation.size {
        let value = unsafe { ptr::read_volatile(base.add(verify_index)) };
        checksum ^= (value as u64) << (verify_index & 7);
        verify_index += (allocation.size / 8).max(1);
    }

    checksum
}

#[inline(never)]
fn busy_mix(mut acc: u64, iterations: u64) -> u64 {
    let mut index = 0u64;

    while index < iterations {
        acc = acc
            .rotate_left(7)
            .wrapping_add(index ^ 0x5851_f42d_4c95_7f2d)
            .wrapping_mul(0x1405_7b7e_f767_814f);
        index += 1;
    }

    black_box(acc)
}

fn print_summary_and_exit() -> ! {
    let booted_mask = BOOTED_MASK.load(Ordering::Acquire);
    let ready_harts = READY_HARTS.load(Ordering::Acquire);
    let completed_jobs = COMPLETED_JOBS.load(Ordering::Acquire);
    let finished_harts = FINISHED_HARTS.load(Ordering::Acquire);
    let max_parallel_jobs = MAX_PARALLEL_JOBS.load(Ordering::Acquire);

    let (queue_pop_count, queue_remaining) = {
        let queue = RUN_QUEUE.lock();
        (queue.pop_count, queue.remaining())
    };

    let (allocator_allocations, allocator_high_water) = {
        let allocator = ALLOCATOR.lock();
        (allocator.allocations, allocator.high_water)
    };

    println!(
        "[kernel] summary: ready_harts={} booted_mask={:#x} finished_harts={} completed_jobs={}/{} max_parallel_jobs={}",
        ready_harts,
        booted_mask,
        finished_harts,
        completed_jobs,
        JOB_COUNT,
        max_parallel_jobs
    );
    println!(
        "[kernel] run_queue_lock: acquisitions={} contention_spins={} pop_count={} remaining={}",
        RUN_QUEUE.acquisitions(),
        RUN_QUEUE.contention_spins(),
        queue_pop_count,
        queue_remaining
    );
    println!(
        "[kernel] allocator_lock: acquisitions={} contention_spins={} allocations={} high_water={} bytes",
        ALLOCATOR.acquisitions(),
        ALLOCATOR.contention_spins(),
        allocator_allocations,
        allocator_high_water
    );

    for hart_id in 0..EXPECTED_HARTS {
        println!(
            "[kernel] hart[{}]: init_done={} jobs_done={} alloc_bytes={}",
            hart_id,
            bool_to_u64(HART_INIT_DONE[hart_id].load(Ordering::Acquire)),
            HART_JOBS_DONE[hart_id].load(Ordering::Relaxed),
            HART_ALLOC_BYTES[hart_id].load(Ordering::Relaxed)
        );
    }

    for job_index in 0..JOB_COUNT {
        let hart_id = JOB_COMPLETION_HART[job_index].load(Ordering::Acquire);
        println!(
            "[kernel] job[{}]: id={} class={} completed_by_hart={} checksum={:#x}",
            job_index,
            JOB_DEFS[job_index].id,
            JOB_DEFS[job_index].class.as_str(),
            hart_id,
            JOB_CHECKSUMS[job_index].load(Ordering::Acquire)
        );
    }

    let all_harts_awakened = ready_harts == EXPECTED_HARTS && booted_mask == expected_hart_mask();
    let locks_protected = queue_pop_count == JOB_COUNT as u64
        && allocator_allocations == JOB_COUNT as u64
        && RUN_QUEUE.acquisitions() >= JOB_COUNT as u64
        && ALLOCATOR.acquisitions() >= JOB_COUNT as u64;
    let stable_under_smp =
        completed_jobs == JOB_COUNT && finished_harts == EXPECTED_HARTS && max_parallel_jobs >= 2;

    println!(
        "[kernel] acceptance all configured harts completed independent initialization: {}",
        pass_fail(all_harts_awakened)
    );
    println!(
        "[kernel] acceptance global run queue and allocator were protected by spinlocks: {}",
        pass_fail(locks_protected)
    );
    println!(
        "[kernel] acceptance -smp {} run completed without deadlock or crash: {}",
        EXPECTED_HARTS,
        pass_fail(stable_under_smp)
    );

    qemu_exit(if all_harts_awakened && locks_protected && stable_under_smp {
        0
    } else {
        1
    })
}

fn initialize_global_state() {
    {
        let mut queue = RUN_QUEUE.lock();
        queue.reset();
    }

    {
        let mut allocator = ALLOCATOR.lock();
        allocator.reset();
    }

    SHUTDOWN.store(false, Ordering::Relaxed);
    START_SCHEDULING.store(false, Ordering::Relaxed);
    READY_HARTS.store(0, Ordering::Relaxed);
    BOOTED_MASK.store(0, Ordering::Relaxed);
    COMPLETED_JOBS.store(0, Ordering::Relaxed);
    FINISHED_HARTS.store(0, Ordering::Relaxed);
    RUNNING_JOBS.store(0, Ordering::Relaxed);
    MAX_PARALLEL_JOBS.store(0, Ordering::Relaxed);
}

fn mark_hart_initialized(hart_id: usize) -> usize {
    HART_INIT_DONE[hart_id].store(true, Ordering::Release);
    BOOTED_MASK.fetch_or(1usize << hart_id, Ordering::AcqRel);
    READY_HARTS.fetch_add(1, Ordering::AcqRel) + 1
}

fn release_secondary_harts() {
    fence(Ordering::Release);
    unsafe {
        ptr::write_volatile(ptr::addr_of_mut!(__boot_release_flag), 1);
    }
}

fn hold_lock_window() {
    let mut remaining = LOCK_HOLD_SPINS;

    while remaining != 0 {
        spin_loop();
        remaining -= 1;
    }
}

fn update_max_parallel_jobs(candidate: u64) {
    let mut observed = MAX_PARALLEL_JOBS.load(Ordering::Acquire);

    while candidate > observed {
        match MAX_PARALLEL_JOBS.compare_exchange_weak(
            observed,
            candidate,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => observed = current,
        }
    }
}

fn expected_hart_mask() -> usize {
    (1usize << EXPECTED_HARTS) - 1
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
