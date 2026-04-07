use core::arch::asm;

#[repr(C)]
#[derive(Clone, Copy)]
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
    pub saved_sp: usize,
    pub epc: usize,
}

impl TrapFrame {
    pub const fn zeroed() -> Self {
        Self {
            ra: 0,
            gp: 0,
            tp: 0,
            t0: 0,
            t1: 0,
            t2: 0,
            s0: 0,
            s1: 0,
            a0: 0,
            a1: 0,
            a2: 0,
            a3: 0,
            a4: 0,
            a5: 0,
            a6: 0,
            a7: 0,
            s2: 0,
            s3: 0,
            s4: 0,
            s5: 0,
            s6: 0,
            s7: 0,
            s8: 0,
            s9: 0,
            s10: 0,
            s11: 0,
            t3: 0,
            t4: 0,
            t5: 0,
            t6: 0,
            saved_sp: 0,
            epc: 0,
        }
    }
}

extern "C" {
    fn machine_trap_entry();
    fn supervisor_trap_entry();
}

pub fn init_machine_trap_vector() {
    unsafe {
        asm!(
            "csrw mtvec, {}",
            in(reg) machine_trap_entry as *const () as usize,
            options(nostack, nomem)
        );
    }
}

pub fn init_supervisor_trap_vector() {
    unsafe {
        asm!(
            "csrw stvec, {}",
            in(reg) supervisor_trap_entry as *const () as usize,
            options(nostack, nomem)
        );
    }
}
