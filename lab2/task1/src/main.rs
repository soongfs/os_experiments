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

const fn padded_task_name(name: &[u8]) -> [u8; TASK_NAME_LEN] {
    let mut padded = [0; TASK_NAME_LEN];
    let mut index = 0;

    while index < name.len() && index < TASK_NAME_LEN - 1 {
        padded[index] = name[index];
        index += 1;
    }

    padded
}

const CURRENT_TASK: TaskInfo = TaskInfo {
    task_id: 1,
    task_name: padded_task_name(b"lab2_task1_user"),
};

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
    println!(
        "[kernel] launching user task id={} name={}",
        CURRENT_TASK.task_id,
        CURRENT_TASK.name()
    );

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
        SYS_GET_TASKINFO => {
            let result = sys_get_taskinfo(frame.a0 as *mut TaskInfo);

            if result == 0 {
                println!(
                    "[kernel] get_taskinfo -> id={} name={}",
                    CURRENT_TASK.task_id,
                    CURRENT_TASK.name()
                );
            } else {
                println!(
                    "[kernel] get_taskinfo rejected user pointer {:#x} with {}",
                    frame.a0, result
                );
            }

            result
        }
        SYS_SHUTDOWN => {
            let code = frame.a0 as u32;
            println!("[kernel] user requested shutdown with code {}", code);
            qemu_exit(code);
        }
        _ => {
            println!("[kernel] unsupported syscall {}", frame.a7);
            ENOSYS
        }
    };

    frame.a0 = result as usize;
}

#[no_mangle]
pub extern "C" fn user_entry() -> ! {
    uprintln!("[user] task started in U-mode");

    let mut info = TaskInfo::empty();
    let ok = syscall::get_taskinfo(&mut info as *mut TaskInfo);
    if ok == 0 {
        uprintln!(
            "[user] get_taskinfo success: id={}, name={}",
            info.task_id,
            info.name()
        );
    } else {
        uprintln!(
            "[user] get_taskinfo failed: {} ({})",
            syscall::describe_error(ok),
            ok
        );
    }

    let bad = syscall::get_taskinfo(ptr::null_mut());
    if bad == 0 {
        uprintln!("[user] unexpected success for null pointer");
        syscall::shutdown(1);
    } else {
        uprintln!(
            "[user] null pointer call rejected: {} ({})",
            syscall::describe_error(bad),
            bad
        );
    }

    uprintln!("[user] task finished cleanly");
    syscall::shutdown(0)
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

fn sys_get_taskinfo(ptr: *mut TaskInfo) -> isize {
    let task_info = match validated_user_mut::<TaskInfo>(ptr) {
        Ok(task_info) => task_info,
        Err(err) => return err,
    };

    *task_info = CURRENT_TASK;
    0
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

fn configure_pmp() {
    unsafe {
        asm!("csrw pmpaddr0, {}", in(reg) usize::MAX >> 2, options(nostack, nomem));
        asm!("csrw pmpcfg0, {}", in(reg) 0x1fusize, options(nostack, nomem));
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    println!("[kernel] panic: {}", info);
    qemu_exit(1)
}
