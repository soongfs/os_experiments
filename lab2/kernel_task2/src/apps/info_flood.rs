use crate::{syscall, TaskInfo, INFO_FLOOD_CALLS};

pub extern "C" fn info_flood() -> ! {
    let mut info = TaskInfo::empty();
    let mut round = 0u64;

    while round < INFO_FLOOD_CALLS {
        let result = syscall::get_taskinfo(&mut info as *mut TaskInfo);
        if result != 0 {
            syscall::exit(1);
        }
        if info.task_id != 3 || info.name() != "info_flood" {
            syscall::exit(2);
        }
        if info.get_taskinfo_calls != round + 1 {
            syscall::exit(3);
        }
        if info.total_syscalls != round + 1 {
            syscall::exit(4);
        }
        if info.write_calls != 0 || info.error_syscalls != 0 {
            syscall::exit(5);
        }

        round += 1;
    }

    syscall::exit(0)
}
