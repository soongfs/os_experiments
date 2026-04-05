#![no_std]
#![no_main]

mod console;
mod syscall;
mod trap;
mod user_console;

use core::arch::{asm, global_asm};
use core::mem::{align_of, size_of};
use core::panic::PanicInfo;
use core::ptr;

global_asm!(include_str!("boot.S"));

const DRAM_START: usize = 0x8000_0000;
const TASK_NAME_LEN: usize = 32;
const SYSCALL_TABLE_LEN: usize = 3;

pub const SYS_WRITE: usize = 0;
pub const SYS_GET_TASKINFO: usize = 1;
pub const SYS_SHUTDOWN: usize = 2;

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
        let mut len = 0;

        while len < self.task_name.len() && self.task_name[len] != 0 {
            len += 1;
        }

        core::str::from_utf8(&self.task_name[..len]).unwrap_or("<invalid>")
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KernelTask {
    id: u64,
    name: [u8; TASK_NAME_LEN],
}

type SyscallHandler = fn(&mut trap::TrapFrame) -> isize;

const fn padded_name(name: &[u8]) -> [u8; TASK_NAME_LEN] {
    let mut padded = [0; TASK_NAME_LEN];
    let mut index = 0;

    while index < name.len() && index < TASK_NAME_LEN - 1 {
        padded[index] = name[index];
        index += 1;
    }

    padded
}

const CURRENT_TASK: KernelTask = KernelTask {
    id: 1,
    name: padded_name(b"kernel_task1_user"),
};

const SYSCALL_TABLE: [Option<SyscallHandler>; SYSCALL_TABLE_LEN] = [
    Some(sys_write_handler),
    Some(sys_get_taskinfo_handler),
    Some(sys_shutdown_handler),
];

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
    println!(
        "[kernel] current task template: id={} name={}",
        CURRENT_TASK.id,
        task_name_str(&CURRENT_TASK.name)
    );
    println!(
        "[kernel] syscall table ready: write={}, get_taskinfo={}, shutdown={}",
        SYS_WRITE, SYS_GET_TASKINFO, SYS_SHUTDOWN
    );

    unsafe {
        enter_user_mode(
            user_entry as *const () as usize,
            ptr::addr_of!(__user_stack_top) as usize,
            ptr::addr_of!(__kernel_stack_top) as usize,
        )
    }
}

pub fn dispatch_syscall(frame: &mut trap::TrapFrame) {
    let nr = frame.a7;
    let handler = SYSCALL_TABLE.get(nr).copied().flatten();

    let result = match handler {
        Some(handler) => handler(frame),
        None => {
            println!("[kernel] unknown syscall nr={} a0={:#x}", nr, frame.a0);
            ENOSYS
        }
    };

    frame.a0 = result as usize;
}

#[no_mangle]
pub extern "C" fn user_entry() -> ! {
    uprintln!("[user] kernel-side get_taskinfo demo started");

    let mut info = TaskInfo::empty();
    let ok = syscall::get_taskinfo(&mut info as *mut TaskInfo);
    if ok == 0 {
        uprintln!(
            "[user] valid pointer copied task info: id={}, name={}",
            info.task_id,
            info.name()
        );
    } else {
        uprintln!(
            "[user] unexpected failure for valid pointer: {} ({})",
            syscall::describe_error(ok),
            ok
        );
        syscall::shutdown(1);
    }

    let null_result = syscall::get_taskinfo(ptr::null_mut());
    uprintln!(
        "[user] null pointer result: {} ({})",
        syscall::describe_error(null_result),
        null_result
    );

    let misaligned_result = syscall::get_taskinfo(1usize as *mut TaskInfo);
    uprintln!(
        "[user] misaligned pointer result: {} ({})",
        syscall::describe_error(misaligned_result),
        misaligned_result
    );

    let out_of_range_ptr = ptr::addr_of!(__user_stack_top) as *mut TaskInfo;
    let out_of_range_result = syscall::get_taskinfo(out_of_range_ptr);
    uprintln!(
        "[user] past-stack pointer result: {} ({})",
        syscall::describe_error(out_of_range_result),
        out_of_range_result
    );

    syscall::shutdown(0)
}

fn sys_write_handler(frame: &mut trap::TrapFrame) -> isize {
    sys_write(frame.a0 as *const u8, frame.a1)
}

fn sys_get_taskinfo_handler(frame: &mut trap::TrapFrame) -> isize {
    let user_ptr = frame.a0 as *mut TaskInfo;
    println!(
        "[kernel] dispatch syscall nr={} -> sys_get_taskinfo(user_ptr={:#x})",
        frame.a7, user_ptr as usize
    );

    let task_info = TaskInfo {
        task_id: CURRENT_TASK.id,
        task_name: CURRENT_TASK.name,
    };

    match copy_to_user(user_ptr, &task_info) {
        Ok(()) => {
            println!(
                "[kernel] copied task info to user: id={} name={}",
                task_info.task_id,
                task_info.name()
            );
            0
        }
        Err(err) => {
            println!(
                "[kernel] rejected get_taskinfo user pointer {:#x} with {}",
                user_ptr as usize, err
            );
            err
        }
    }
}

fn sys_shutdown_handler(frame: &mut trap::TrapFrame) -> isize {
    let code = frame.a0 as u32;
    println!("[kernel] user requested shutdown with code {}", code);
    qemu_exit(code)
}

fn clear_bss() {
    unsafe {
        let start = ptr::addr_of!(__bss_start) as *mut u8;
        let end = ptr::addr_of!(__bss_end) as usize;
        ptr::write_bytes(start, 0, end - start as usize);
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

fn validated_user_bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Ok(&[]);
    }

    let addr = ptr as usize;
    if addr == 0 {
        return Err(EFAULT);
    }

    let end = match addr.checked_add(len) {
        Some(end) => end,
        None => return Err(EFAULT),
    };

    if addr < DRAM_START || end > user_input_end() {
        return Err(EFAULT);
    }

    unsafe { Ok(core::slice::from_raw_parts(ptr, len)) }
}

fn copy_to_user<T: Copy>(dst: *mut T, src: &T) -> Result<(), isize> {
    validate_user_output_ptr(dst as usize, size_of::<T>(), align_of::<T>())?;

    unsafe {
        ptr::write(dst, *src);
    }

    Ok(())
}

fn validate_user_output_ptr(addr: usize, len: usize, alignment: usize) -> Result<(), isize> {
    if addr == 0 {
        return Err(EFAULT);
    }
    if addr % alignment != 0 {
        return Err(EINVAL);
    }

    let end = match addr.checked_add(len) {
        Some(end) => end,
        None => return Err(EFAULT),
    };

    let bottom = ptr::addr_of!(__user_stack_bottom) as usize;
    let top = ptr::addr_of!(__user_stack_top) as usize;
    if addr < bottom || end > top {
        return Err(EFAULT);
    }

    Ok(())
}

fn user_input_end() -> usize {
    ptr::addr_of!(__image_end) as usize
}

fn task_name_str(name: &[u8; TASK_NAME_LEN]) -> &str {
    let mut len = 0;

    while len < name.len() && name[len] != 0 {
        len += 1;
    }

    core::str::from_utf8(&name[..len]).unwrap_or("<invalid>")
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
