use core::arch::asm;

use crate::{handle_syscall, println, qemu_exit};

#[repr(C)]
pub struct TrapFrame {
    pub ra: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
    pub user_sp: usize,
    pub mepc: usize,
}

extern "C" {
    fn trap_entry();
}

pub fn init_trap_vector() {
    unsafe {
        asm!(
            "csrw mtvec, {}",
            in(reg) trap_entry as *const () as usize,
            options(nostack, nomem)
        );
    }
}

#[no_mangle]
pub extern "C" fn handle_trap(frame: &mut TrapFrame) {
    let mcause = read_mcause();

    match mcause {
        8 => {
            frame.mepc = frame.mepc.wrapping_add(4);
            handle_syscall(frame);
        }
        _ => {
            println!(
                "[kernel] unexpected trap: mcause={:#x} mepc={:#x} mtval={:#x}",
                mcause,
                frame.mepc,
                read_mtval()
            );
            qemu_exit(1);
        }
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
