use core::arch::asm;

pub extern "C" fn illegal_trap() -> ! {
    unsafe {
        asm!("csrw mtvec, zero", options(nostack));
    }

    loop {
        core::hint::spin_loop();
    }
}
