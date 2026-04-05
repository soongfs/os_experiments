#![no_std]
#![no_main]

mod console;
mod syscall;
mod trap;
mod user_console;

use core::arch::{asm, global_asm};
use core::hint::black_box;
use core::mem::size_of;
use core::panic::PanicInfo;
use core::ptr;

global_asm!(include_str!("boot.S"));

const DRAM_START: usize = 0x8000_0000;
const MAX_FRAMES: usize = 8;

pub const SYS_WRITE: usize = 0;
pub const SYS_EXIT: usize = 1;

pub const EFAULT: isize = -14;
pub const ENOSYS: isize = -38;

#[repr(C)]
#[derive(Clone, Copy)]
struct FrameRecord {
    previous_fp: usize,
    return_address: usize,
}

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    static __kernel_stack_top: u8;
    static __user_stack_bottom: u8;
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
    println!("[kernel] launching stack trace demo in U-mode");

    unsafe {
        enter_user_mode(
            user_entry as *const () as usize,
            ptr::addr_of!(__user_stack_top) as usize,
            ptr::addr_of!(__kernel_stack_top) as usize,
        )
    }
}

pub fn handle_syscall(frame: &mut trap::TrapFrame) {
    let result = match frame.a7 {
        SYS_WRITE => sys_write(frame.a0 as *const u8, frame.a1),
        SYS_EXIT => {
            let code = frame.a0 as u32;
            println!("[kernel] user requested exit with code {}", code);
            qemu_exit(code);
        }
        _ => ENOSYS,
    };

    frame.a0 = result as usize;
}

#[no_mangle]
pub extern "C" fn user_entry() -> ! {
    uprintln!("[user] stack trace demo started");
    trace_root(0x10);
    syscall::exit(0)
}

#[inline(never)]
fn trace_root(seed: usize) {
    let local = seed.wrapping_add(0x11);
    trace_mid(local ^ 0x22);
    black_box(local);
}

#[inline(never)]
fn trace_mid(seed: usize) {
    let local = seed.wrapping_mul(3).wrapping_add(0x33);
    trace_leaf(local ^ 0x44);
    black_box(local);
}

#[inline(never)]
fn trace_leaf(seed: usize) {
    let local = seed.rotate_left(3).wrapping_add(0x55);
    print_stack_trace();
    black_box(local);
}

#[inline(never)]
fn print_stack_trace() {
    let mut fp: usize;
    unsafe {
        asm!("mv {}, s0", out(reg) fp, options(nostack, nomem, preserves_flags));
    }

    let stack_bottom = ptr::addr_of!(__user_stack_bottom) as usize;
    let stack_top = ptr::addr_of!(__user_stack_top) as usize;
    let image_end = ptr::addr_of!(__image_end) as usize;

    uprintln!(
        "[user] frame pointer walk begins: fp={:#x}, stack=[{:#x}, {:#x})",
        fp,
        stack_bottom,
        stack_top
    );

    let mut depth = 0usize;
    let mut printed_frames = 0usize;
    while depth < MAX_FRAMES {
        if !frame_pointer_in_range(fp, stack_bottom, stack_top) {
            break;
        }

        let record = unsafe { read_frame_record(fp) };
        if record.return_address < DRAM_START || record.return_address >= image_end {
            break;
        }

        uprintln!(
            "[user] frame#{:02}: fp={:#x} prev_fp={:#x} ra={:#x}",
            depth,
            fp,
            record.previous_fp,
            record.return_address
        );
        printed_frames += 1;

        if !next_frame_pointer_valid(fp, record.previous_fp, stack_top) {
            break;
        }

        fp = record.previous_fp;
        depth += 1;
    }

    uprintln!(
        "[user] frame pointer walk finished after {} frame(s)",
        printed_frames
    );
}

fn frame_pointer_in_range(fp: usize, stack_bottom: usize, stack_top: usize) -> bool {
    fp >= stack_bottom + (2 * size_of::<usize>()) && fp <= stack_top && fp % size_of::<usize>() == 0
}

fn next_frame_pointer_valid(current_fp: usize, next_fp: usize, stack_top: usize) -> bool {
    next_fp > current_fp && next_fp <= stack_top && next_fp % size_of::<usize>() == 0
}

unsafe fn read_frame_record(fp: usize) -> FrameRecord {
    ptr::read((fp - (2 * size_of::<usize>())) as *const FrameRecord)
}

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
    }
}

fn sys_write(ptr: *const u8, len: usize) -> isize {
    if len == 0 {
        return 0;
    }

    let addr = ptr as usize;
    let end = match addr.checked_add(len) {
        Some(end) => end,
        None => return EFAULT,
    };
    let image_end = ptr::addr_of!(__image_end) as usize;
    if addr == 0 || addr < DRAM_START || end > image_end {
        return EFAULT;
    }

    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    for &byte in bytes {
        console::write_byte(byte);
    }

    len as isize
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

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1)
}
