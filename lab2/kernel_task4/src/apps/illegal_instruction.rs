use core::arch::asm;

pub extern "C" fn illegal_instruction() -> ! {
    unsafe {
        asm!("csrw sstatus, zero", options(nostack));
    }

    loop {
        core::hint::spin_loop();
    }
}
