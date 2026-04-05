use crate::{syscall, TaskInfo, INFO_PROBE_CALLS};

pub extern "C" fn info_probe() -> ! {
    let mut info = TaskInfo::empty();
    let mut round = 0u64;

    while round < INFO_PROBE_CALLS {
        let result = syscall::get_taskinfo(&mut info as *mut TaskInfo);
        if result != 0 {
            syscall::exit(1);
        }
        if info.task_id != 3 || info.name() != "info_probe" {
            syscall::exit(2);
        }

        round += 1;
    }

    syscall::exit(0)
}
