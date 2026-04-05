# LAB2 内核态 Task2: 系统调用编号与次数统计

## 1. 原始任务说明

### 任务标题

系统调用编号与次数统计

### 任务目标

理解内核观测点（instrumentation）的设计方式，掌握低开销统计结构的维护策略。

### 任务要求

1. 统计每个应用执行期间触发的系统调用编号与次数；
2. 至少支持对多应用依次执行的统计隔离（每个应用分别统计或可区分）；
3. 输出结果需可复核：与用户态测试集的行为趋势一致。

### 验收检查

1. 进程控制块（PCB/TCB）中新增了统计数据结构；
2. 统计数值不受任务切换或内核自身初始化的干扰；
3. 任务退出时能在内核日志中打印正确的系统调用直方图。

## 2. 实验目标与实现思路

本任务单独放在 [lab2/kernel_task2](/root/os_experiments/lab2/kernel_task2) 中，重点展示内核侧 syscall instrumentation，而不是用户态测试框架本身。为了让“统计趋势”可以复核，我直接复用了 [lab2/task2](/root/os_experiments/lab2/task2) 中已经验证过的 4 个 workload：

1. `io_burst`：I/O 密集，反复触发 `write`；
2. `compute_spin`：计算密集，几乎只有 `exit`；
3. `info_flood`：频繁触发 `get_taskinfo`；
4. `illegal_trap`：直接触发非法指令异常，不经过 `ecall`。

内核为每个任务维护一个 `TaskControlBlock`，其中内嵌 `SyscallStats`：

- `total_syscalls`
- `histogram: [u64; 3]`
- `error_syscalls`
- `unknown_syscalls`

统计点只放在 U-mode `ecall` 已被 trap 层识别之后的 `handle_syscall()` 中，因此：

- 内核启动打印不会被统计；
- 任务切换逻辑不会被统计；
- 非 syscall 异常不会误记入直方图；
- 每次启动新任务时都会重置当前 TCB 的统计字段，从而保证任务之间隔离。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/kernel_task2/Cargo.toml)：Rust 裸机工程配置，包名为 `lab2_kernel_task2`。
- [.cargo/config.toml](/root/os_experiments/lab2/kernel_task2/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab2/kernel_task2/linker.ld)：镜像地址、内核栈、用户栈布局。
- [src/boot.S](/root/os_experiments/lab2/kernel_task2/src/boot.S)：启动入口、trap 现场保存与 `enter_user_mode`。
- [src/trap.rs](/root/os_experiments/lab2/kernel_task2/src/trap.rs)：trap 分发逻辑，区分 `ecall` 与用户态 fault。
- [src/main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs)：TCB、syscall 统计结构、dispatcher、用户指针检查、任务切换与最终报告。
- [src/syscall.rs](/root/os_experiments/lab2/kernel_task2/src/syscall.rs)：用户态 `write/get_taskinfo/exit` 封装。
- [src/console.rs](/root/os_experiments/lab2/kernel_task2/src/console.rs)：内核串口输出。
- [src/apps/io_burst.rs](/root/os_experiments/lab2/kernel_task2/src/apps/io_burst.rs)：I/O 密集 workload。
- [src/apps/compute_spin.rs](/root/os_experiments/lab2/kernel_task2/src/apps/compute_spin.rs)：计算密集 workload。
- [src/apps/info_flood.rs](/root/os_experiments/lab2/kernel_task2/src/apps/info_flood.rs)：频繁 `get_taskinfo` workload。
- [src/apps/illegal_trap.rs](/root/os_experiments/lab2/kernel_task2/src/apps/illegal_trap.rs)：异常路径 workload。
- [artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/build_output.txt)：最近一次构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output.txt)：第一次完整 QEMU 运行输出。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output_repeat.txt)：第二次复验输出。

## 4. 内核统计机制

### 4.1 TCB 中新增统计结构

[src/main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs) 中定义了：

```rust
struct SyscallStats {
    total_syscalls: u64,
    histogram: [u64; SYSCALL_HIST_LEN],
    error_syscalls: u64,
    unknown_syscalls: u64,
}

struct TaskControlBlock {
    id: u64,
    name: &'static str,
    task_name: [u8; TASK_NAME_LEN],
    entry: extern "C" fn() -> !,
    expected_profile: &'static str,
    stats: SyscallStats,
    start_cycle: u64,
    end_cycle: u64,
    exit_code: i32,
    fault_cause: u64,
    fault_tval: u64,
    status: TaskStatus,
}
```

这对应验收要求中的“PCB/TCB 中新增统计数据结构”。

### 4.2 为什么统计不受内核初始化和任务切换干扰

syscall 统计入口在 `handle_syscall()`：

```rust
pub fn handle_syscall(frame: &mut trap::TrapFrame) {
    match frame.a7 {
        SYS_WRITE => {
            record_syscall_number(SYS_WRITE);
            ...
        }
        SYS_GET_TASKINFO => {
            record_syscall_number(SYS_GET_TASKINFO);
            ...
        }
        SYS_EXIT => {
            record_syscall_number(SYS_EXIT);
            finish_current_task(frame.a0 as i32)
        }
        nr => {
            record_syscall_number(nr);
            record_syscall_error();
            frame.a0 = ENOSYS as usize;
        }
    }
}
```

只有用户态执行 `ecall`，trap 层识别为 syscall 后才会进入这段逻辑。因此：

1. 内核启动过程里的打印不会增加计数；
2. `launch_task()`、`advance_to_next_task()` 这些调度代码不会增加计数；
3. `illegal_trap` 触发的是非法指令 fault，不会走 `handle_syscall()`，所以它的直方图保持全 0。

这正是“观测点放在正确层级”的关键。

### 4.3 多应用统计隔离

每次启动任务时，`launch_task()` 会显式清空当前 TCB 的统计域：

```rust
task.stats = SyscallStats::empty();
task.start_cycle = read_cycle();
task.end_cycle = 0;
task.exit_code = 0;
task.fault_cause = 0;
task.fault_tval = 0;
task.status = TaskStatus::Ready;
```

因此每个 workload 只看到自己的 syscall 计数。前一个任务的 `write` 或 `get_taskinfo` 不会“串”到下一个任务。

### 4.4 任务退出时打印 syscall 直方图

任务正常 `exit` 或 fault 后都会调用 `print_task_result()`，然后进一步输出：

```rust
fn print_task_histogram(task: TaskControlBlock) {
    println!("[kernel] syscall histogram for {}:", task.name);
    ...
}
```

直方图固定打印 3 个已知 syscall 编号和一个 `unknown` 桶：

- `nr=0 (write)`
- `nr=1 (get_taskinfo)`
- `nr=2 (exit)`
- `unknown`

这样既满足“按编号统计”，也便于后续扩展更多 syscall 号。

## 5. 与用户态测试集的对应关系

本实验直接复用了用户态 [lab2/task2](/root/os_experiments/lab2/task2) 的 workload 设计，因此可以对照其行为趋势：

| workload | 预期 syscall 趋势 | 内核侧应观察到的直方图特征 |
| --- | --- | --- |
| `io_burst` | `write` 明显最多 | `nr=0` 桶远高于其他桶，`nr=2` 恰为 1 |
| `compute_spin` | 几乎不 syscall，只在结束时 `exit` | 只有 `nr=2` 为 1，其余为 0 |
| `info_flood` | `get_taskinfo` 明显最多 | `nr=1` 桶远高于其他桶，`nr=2` 恰为 1 |
| `illegal_trap` | 直接 fault，不触发 `ecall` | 所有 syscall 桶都保持 0 |

其中 [src/apps/info_flood.rs](/root/os_experiments/lab2/kernel_task2/src/apps/info_flood.rs) 还会在用户态校验 `get_taskinfo_calls` 与 `total_syscalls` 是否随调用次数单调增长，这能反向证明内核统计快照在执行期间也是一致的。

## 6. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/kernel_task2
```

构建：

```bash
cargo build
```

运行一次并保存输出：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task2 > artifacts/run_output.txt
```

再次运行做复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task2 > artifacts/run_output_repeat.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
```

## 7. 本次实际运行结果

### 7.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 7.2 第一次运行完整输出

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output.txt)：

```text
[kernel] booted in M-mode
[kernel] starting LAB2 kernel task2 syscall histogram suite
[kernel] tracked syscall numbers: nr=0(write), nr=1(get_taskinfo), nr=2(exit)
[kernel] launch task id=1 name=io_burst | expected: write bucket should dominate; exit bucket should be exactly 1
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
io
[kernel] result io_burst: status=exit(0) cycles=4452240 total=25 errors=0 unknown=0
[kernel] syscall histogram for io_burst:
[kernel]   nr=0 (write) -> 24
[kernel]   nr=1 (get_taskinfo) -> 0
[kernel]   nr=2 (exit) -> 1
[kernel]   unknown -> 0
[kernel] launch task id=2 name=compute_spin | expected: almost only exit syscall; elapsed cycles should be the largest
[kernel] result compute_spin: status=exit(0) cycles=13410760 total=1 errors=0 unknown=0
[kernel] syscall histogram for compute_spin:
[kernel]   nr=0 (write) -> 0
[kernel]   nr=1 (get_taskinfo) -> 0
[kernel]   nr=2 (exit) -> 1
[kernel]   unknown -> 0
[kernel] launch task id=3 name=info_flood | expected: get_taskinfo bucket should dominate; exit bucket should be exactly 1
[kernel] result info_flood: status=exit(0) cycles=3303280 total=21 errors=0 unknown=0
[kernel] syscall histogram for info_flood:
[kernel]   nr=0 (write) -> 0
[kernel]   nr=1 (get_taskinfo) -> 20
[kernel]   nr=2 (exit) -> 1
[kernel]   unknown -> 0
[kernel] launch task id=4 name=illegal_trap | expected: should fault before any syscall, keeping every bucket at 0
[kernel] task illegal_trap faulted: mcause=0x2 mepc=0x80002202 mtval=0x30501073
[kernel] result illegal_trap: status=fault(cause=0x2, mtval=0x30501073) cycles=1046720 total=0 errors=0 unknown=0
[kernel] syscall histogram for illegal_trap:
[kernel]   nr=0 (write) -> 0
[kernel]   nr=1 (get_taskinfo) -> 0
[kernel]   nr=2 (exit) -> 0
[kernel]   unknown -> 0
[kernel] final per-task summary:
[kernel] result io_burst: status=exit(0) cycles=4452240 total=25 errors=0 unknown=0
[kernel] syscall histogram for io_burst:
[kernel]   nr=0 (write) -> 24
[kernel]   nr=1 (get_taskinfo) -> 0
[kernel]   nr=2 (exit) -> 1
[kernel]   unknown -> 0
[kernel] result compute_spin: status=exit(0) cycles=13410760 total=1 errors=0 unknown=0
[kernel] syscall histogram for compute_spin:
[kernel]   nr=0 (write) -> 0
[kernel]   nr=1 (get_taskinfo) -> 0
[kernel]   nr=2 (exit) -> 1
[kernel]   unknown -> 0
[kernel] result info_flood: status=exit(0) cycles=3303280 total=21 errors=0 unknown=0
[kernel] syscall histogram for info_flood:
[kernel]   nr=0 (write) -> 0
[kernel]   nr=1 (get_taskinfo) -> 20
[kernel]   nr=2 (exit) -> 1
[kernel]   unknown -> 0
[kernel] result illegal_trap: status=fault(cause=0x2, mtval=0x30501073) cycles=1046720 total=0 errors=0 unknown=0
[kernel] syscall histogram for illegal_trap:
[kernel]   nr=0 (write) -> 0
[kernel]   nr=1 (get_taskinfo) -> 0
[kernel]   nr=2 (exit) -> 0
[kernel]   unknown -> 0
[kernel] check io_burst write bucket dominance: PASS
[kernel] check compute_spin exit-only histogram: PASS
[kernel] check info_flood get_taskinfo bucket dominance: PASS
[kernel] check illegal_trap keeps histogram clean: PASS
[kernel] check per-task isolation across launches: PASS
[kernel] cross-check with user-side workload trends: PASS
```

### 7.3 第二次运行摘要

[artifacts/run_output_repeat.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output_repeat.txt) 中的关键统计与第一次一致：

| workload | `nr=0 write` | `nr=1 get_taskinfo` | `nr=2 exit` | `unknown` | 结论 |
| --- | --- | --- | --- | --- | --- |
| `io_burst` | 24 | 0 | 1 | 0 | `write` 桶稳定占优 |
| `compute_spin` | 0 | 0 | 1 | 0 | 只有 `exit` |
| `info_flood` | 0 | 20 | 1 | 0 | `get_taskinfo` 桶稳定占优 |
| `illegal_trap` | 0 | 0 | 0 | 0 | fault 前没有 syscall |

两次运行的 `cycles` 会有浮动，但 syscall 直方图完全一致，说明统计逻辑稳定，且和 workload 的程序特征一致。

## 8. 简短测试报告

### 8.1 测试项、预期行为、观测结果

| 测试项 | 预期行为 | 观测结果 | 结论 |
| --- | --- | --- | --- |
| `io_burst` | `write` 调用显著高于其他 syscall | `nr=0 -> 24`, `nr=2 -> 1` | 符合 I/O 密集特征 |
| `compute_spin` | 几乎不发生 syscall，仅末尾 `exit` | `nr=2 -> 1`，其余全 0 | 符合计算密集特征 |
| `info_flood` | `get_taskinfo` 调用显著高于其他 syscall | `nr=1 -> 20`, `nr=2 -> 1` | 符合频繁系统调用特征 |
| `illegal_trap` | 不走 syscall 路径，直接 fault | 全部桶为 0，记录 `mcause=0x2` | 证明 fault 不会污染 syscall 统计 |

### 8.2 偏差分析

1. 两次运行中 syscall 计数没有偏差，直方图完全一致；
2. 周期数存在波动，但只影响运行时间，不影响 syscall 分布；
3. `illegal_trap` 没有进入 `exit`，因此 `nr=2` 为 0，这是预期行为而不是统计缺失；
4. `info_flood` 的 `get_taskinfo` 计数为 20 而不是 21，因为最后一次 `exit` 单独落在 `nr=2` 桶中。

## 9. 验收检查对应关系

1. PCB/TCB 中新增统计结构：
   - [src/main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs) 中的 `TaskControlBlock.stats: SyscallStats` 即为内核侧统计数据结构。
2. 统计不受任务切换或内核初始化干扰：
   - 统计点只在 `handle_syscall()` 中；
   - `illegal_trap` 的直方图保持全 0，证明非 syscall trap 未污染统计；
   - 各任务之间的桶值互不串扰，证明切换逻辑未污染统计。
3. 任务退出时打印正确的 syscall 直方图：
   - `io_burst` 输出 `write=24, get_taskinfo=0, exit=1`；
   - `compute_spin` 输出 `write=0, get_taskinfo=0, exit=1`；
   - `info_flood` 输出 `write=0, get_taskinfo=20, exit=1`；
   - `illegal_trap` 输出全部为 0，并单独记录 fault。

## 10. 环境说明与限制

- 本次实验在当前 Linux 环境完成，使用：
  - `rustc 1.94.1`
  - `cargo 1.94.1`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器复现。
- 本实验是教学化最小裸机内核，没有真实 MMU、多进程地址空间与抢占式调度；
- 因此这里的“任务切换隔离”是指顺序运行多个 workload 时的内核统计隔离，而不是完整操作系统中的进程隔离语义。
