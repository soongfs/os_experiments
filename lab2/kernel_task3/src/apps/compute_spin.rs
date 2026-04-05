use crate::syscall;

const COMPUTE_ROUNDS: u64 = 300_000;

pub extern "C" fn compute_spin() -> ! {
    let mut acc = 0x1234_5678_9abc_def0u64;
    let mut round = 0u64;

    while round < COMPUTE_ROUNDS {
        acc = acc
            .wrapping_mul(6364136223846793005)
            .wrapping_add(round ^ 0x9e37_79b9_7f4a_7c15);
        if acc & 1 == 0 {
            acc ^= 0xa5a5_a5a5_a5a5_a5a5;
        }
        round += 1;
    }

    if acc == 0 {
        let _ = syscall::write(b"unreachable\n");
        syscall::exit(2);
    }

    syscall::exit(0)
}
