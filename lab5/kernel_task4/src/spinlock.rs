use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct SpinLock<T> {
    locked: AtomicBool,
    acquisitions: AtomicU64,
    contention_spins: AtomicU64,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            acquisitions: AtomicU64::new(0),
            contention_spins: AtomicU64::new(0),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let mut spins = 0u64;

        loop {
            if self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                self.acquisitions.fetch_add(1, Ordering::Relaxed);
                if spins != 0 {
                    self.contention_spins.fetch_add(spins, Ordering::Relaxed);
                }
                return SpinLockGuard { lock: self };
            }

            while self.locked.load(Ordering::Relaxed) {
                spins = spins.wrapping_add(1);
                spin_loop();
            }

            spins = spins.wrapping_add(1);
        }
    }

    pub fn acquisitions(&self) -> u64 {
        self.acquisitions.load(Ordering::Relaxed)
    }

    pub fn contention_spins(&self) -> u64 {
        self.contention_spins.load(Ordering::Relaxed)
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}
