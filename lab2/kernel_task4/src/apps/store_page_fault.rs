pub extern "C" fn store_page_fault() -> ! {
    unsafe {
        core::ptr::write_volatile(0 as *mut u64, 0xdead_beef_dead_beefu64);
    }

    loop {
        core::hint::spin_loop();
    }
}
