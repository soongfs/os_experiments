# LAB2 Task2: 系统调用统计与完成时间统计测试集

## 1. 原始任务说明

### 任务标题

系统调用统计与完成时间统计测试集

### 任务目标

建立可验收的测试集合，用于验证内核统计功能的正确性与鲁棒性。

### 任务要求

1. 编写不少于 3 个特征不同的测试应用（例如：I/O 密集、计算密集、频繁系统调用、触发异常等）；
2. 每个应用需说明“预期触发的系统调用类型/数量趋势”，并在运行后与统计结果对照；
3. 形成一份简短测试报告：列出测试项、预期行为、观测结果、偏差分析。

### 验收检查

1. 提供 3 个独立的测试源码，特征区分明显；
2. 测试报告逻辑自洽，观测结果能够印证测试程序的行为特征（例如 I/O 程序的 `write` 调用次数远高于计算型）。

## 2. 实验目标与实现思路

本实验在 [lab2/task2](/root/os_experiments/lab2/task2) 中构建了一个最小 RISC-V 裸机测试框架。内核从 M-mode 启动，顺序运行 4 个 U-mode 测试应用，并为每个任务记录：

- `total_syscalls`
- `write_calls`
- `get_taskinfo_calls`
- `error_syscalls`
- `elapsed_cycles`
- 完成状态：`exit(code)` 或 `fault(mcause, mtval)`

本次提供的 4 个测试应用如下：

1. `io_burst`：I/O 密集型，反复调用 `write`；
2. `compute_spin`：计算密集型，几乎不触发 syscall，只在末尾 `exit`；
3. `info_flood`：频繁调用 `get_taskinfo`，并在用户态校验统计快照的单调增长；
4. `illegal_trap`：主动执行 U-mode 非法特权指令，验证异常路径是否被内核记录并继续执行后续统计。

后两项分别覆盖“统计正确性”和“异常鲁棒性”，因此这份测试集比“只做 3 个正常退出程序”更适合验收。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/task2/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab2/task2/.cargo/config.toml)：固定 RISC-V 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab2/task2/linker.ld)：镜像装载地址与用户/内核栈布局。
- [src/boot.S](/root/os_experiments/lab2/task2/src/boot.S)：启动入口、trap 保存现场、`enter_user_mode`。
- [src/console.rs](/root/os_experiments/lab2/task2/src/console.rs)：内核串口输出。
- [src/trap.rs](/root/os_experiments/lab2/task2/src/trap.rs)：trap 分发逻辑，区分 syscall 与用户态 fault。
- [src/syscall.rs](/root/os_experiments/lab2/task2/src/syscall.rs)：用户态 syscall 封装。
- [src/main.rs](/root/os_experiments/lab2/task2/src/main.rs)：任务调度、统计记录、`get_taskinfo` 实现、最终报告输出。
- [src/apps/io_burst.rs](/root/os_experiments/lab2/task2/src/apps/io_burst.rs)：I/O 密集测试源码。
- [src/apps/compute_spin.rs](/root/os_experiments/lab2/task2/src/apps/compute_spin.rs)：计算密集测试源码。
- [src/apps/info_flood.rs](/root/os_experiments/lab2/task2/src/apps/info_flood.rs)：频繁 `get_taskinfo` 测试源码。
- [src/apps/illegal_trap.rs](/root/os_experiments/lab2/task2/src/apps/illegal_trap.rs)：异常路径测试源码。
- [artifacts/build_output.txt](/root/os_experiments/lab2/task2/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab2/task2/artifacts/run_output.txt)：第一次完整运行输出。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab2/task2/artifacts/run_output_repeat.txt)：第二次复验输出。

## 4. 统计机制说明

### 4.1 syscall 统计

用户态只通过 `ecall` 进入内核。内核在 [src/main.rs](/root/os_experiments/lab2/task2/src/main.rs) 的 `handle_syscall()` 中按 syscall 类型更新计数：

- `SYS_WRITE`：增加 `total_syscalls` 和 `write_calls`
- `SYS_GET_TASKINFO`：增加 `total_syscalls` 和 `get_taskinfo_calls`
- `SYS_EXIT`：只增加 `total_syscalls`
- 若返回值为负，则额外增加 `error_syscalls`

### 4.2 完成时间统计

内核在启动每个任务前读取 `rdcycle` 保存为 `start_cycle`，任务正常 `exit` 或异常 fault 时再次读取 `rdcycle` 记为 `end_cycle`。最终以 `end_cycle - start_cycle` 作为任务完成时间统计值。

这里记录的是 QEMU 来宾周期数，不是宿主机墙钟时间；因此更适合比较趋势而不是绝对性能。

### 4.3 `get_taskinfo` 的统计快照

`get_taskinfo` 返回的 [TaskInfo](/root/os_experiments/lab2/task2/src/main.rs) 不只包含 `task_id` 与 `task_name`，还包含当前任务截至本次调用时的统计快照：

- `total_syscalls`
- `write_calls`
- `get_taskinfo_calls`
- `error_syscalls`
- `elapsed_cycles`

`info_flood` 会在用户态验证：

1. `task_id` 和 `task_name` 与预期任务匹配；
2. 第 `n` 次调用时，`get_taskinfo_calls == n`；
3. 同一时刻 `total_syscalls == n`；
4. `write_calls == 0`、`error_syscalls == 0`。

这使得测试不仅比较最终统计值，还直接验证了统计在运行过程中的一致性。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/task2
```

构建：

```bash
cargo build
```

运行一次并保存输出：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_task2 > artifacts/run_output.txt
```

再次运行做趋势复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_task2 > artifacts/run_output_repeat.txt
```

查看运行日志：

```bash
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab2/task2/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab2_task2 v0.1.0 (/root/os_experiments/lab2/task2)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.28s
```

### 6.2 第一次运行完整输出

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab2/task2/artifacts/run_output.txt)：

```text
[kernel] booted in M-mode
[kernel] starting LAB2 task2 statistics suite
[kernel] launch task id=1 name=io_burst | expected: write syscall count should dominate; elapsed cycles should stay modest
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
[kernel] result io_burst: status=exit(0) cycles=8415640 total=25 write=24 get_taskinfo=0 error=0
[kernel] launch task id=2 name=compute_spin | expected: almost no syscalls, but elapsed cycles should be the highest
[kernel] result compute_spin: status=exit(0) cycles=20854920 total=1 write=0 get_taskinfo=0 error=0
[kernel] launch task id=3 name=info_flood | expected: get_taskinfo calls should dominate and returned snapshots should grow monotonically
[kernel] result info_flood: status=exit(0) cycles=6339640 total=21 write=0 get_taskinfo=20 error=0
[kernel] launch task id=4 name=illegal_trap | expected: should trigger an illegal-instruction trap and be recorded as faulted
[kernel] task illegal_trap faulted: mcause=0x2 mepc=0x80001ac2 mtval=0x30501073
[kernel] result illegal_trap: status=fault(cause=0x2, mtval=0x30501073) cycles=6296559 total=0 write=0 get_taskinfo=0 error=0
[kernel] final statistics report:
[kernel] result io_burst: status=exit(0) cycles=8415640 total=25 write=24 get_taskinfo=0 error=0
[kernel] result compute_spin: status=exit(0) cycles=20854920 total=1 write=0 get_taskinfo=0 error=0
[kernel] result info_flood: status=exit(0) cycles=6339640 total=21 write=0 get_taskinfo=20 error=0
[kernel] result illegal_trap: status=fault(cause=0x2, mtval=0x30501073) cycles=6296559 total=0 write=0 get_taskinfo=0 error=0
[kernel] check io_burst writes: PASS
[kernel] check compute_spin low-syscall/high-time trend: PASS
[kernel] check info_flood get_taskinfo trend: PASS
[kernel] check illegal_trap robustness path: PASS
[kernel] cross-task trend comparison: PASS
```

### 6.3 第二次运行摘要

第二次运行完整日志见 [artifacts/run_output_repeat.txt](/root/os_experiments/lab2/task2/artifacts/run_output_repeat.txt)。关键统计如下：

```text
[kernel] result io_burst: status=exit(0) cycles=12527879 total=25 write=24 get_taskinfo=0 error=0
[kernel] result compute_spin: status=exit(0) cycles=23599161 total=1 write=0 get_taskinfo=0 error=0
[kernel] result info_flood: status=exit(0) cycles=5121403 total=21 write=0 get_taskinfo=20 error=0
[kernel] result illegal_trap: status=fault(cause=0x2, mtval=0x30501073) cycles=3892320 total=0 write=0 get_taskinfo=0 error=0
[kernel] cross-task trend comparison: PASS
```

可以看到，两次运行中 syscall 计数完全一致，周期数有波动，但“`compute_spin` 最慢、`io_burst` 写调用最多、`info_flood` 的 `get_taskinfo` 调用最多、`illegal_trap` 稳定 fault”这一趋势保持不变。

## 7. 简短测试报告

### 7.1 测试项与预期

| 测试项 | 测试源码 | 预期行为 |
| --- | --- | --- |
| I/O 密集 | [io_burst.rs](/root/os_experiments/lab2/task2/src/apps/io_burst.rs) | `write` 次数明显最高，正常退出，完成时间中等 |
| 计算密集 | [compute_spin.rs](/root/os_experiments/lab2/task2/src/apps/compute_spin.rs) | syscall 极少，仅 `exit`，但完成时间最高 |
| 频繁统计查询 | [info_flood.rs](/root/os_experiments/lab2/task2/src/apps/info_flood.rs) | `get_taskinfo` 次数明显最高，且用户态看到的统计快照单调增长 |
| 异常鲁棒性 | [illegal_trap.rs](/root/os_experiments/lab2/task2/src/apps/illegal_trap.rs) | 不正常退出，应触发 `illegal instruction`，内核记录 fault 并继续完成整个测试集 |

### 7.2 观测结果与对照

| 测试项 | 第一次观测 | 第二次观测 | 结论 |
| --- | --- | --- | --- |
| `io_burst` | `write=24`, `total=25`, `cycles=8415640` | `write=24`, `total=25`, `cycles=12527879` | `write` 调用次数显著高于其他正常任务，符合 I/O 密集特征 |
| `compute_spin` | `write=0`, `get_taskinfo=0`, `total=1`, `cycles=20854920` | `write=0`, `get_taskinfo=0`, `total=1`, `cycles=23599161` | syscall 极少但时间最长，符合计算密集特征 |
| `info_flood` | `get_taskinfo=20`, `total=21`, `cycles=6339640` | `get_taskinfo=20`, `total=21`, `cycles=5121403` | `get_taskinfo` 次数显著最高，且用户态内部校验通过 |
| `illegal_trap` | `fault cause=0x2`, `total=0`, `cycles=6296559` | `fault cause=0x2`, `total=0`, `cycles=3892320` | 异常路径稳定触发并被记录，说明统计框架对 fault 情况可用 |

### 7.3 偏差分析

1. syscall 计数在两次运行中完全一致，没有观测到偏差；
2. 完成时间（`cycles`）在两次运行中有一定浮动，但相对大小关系稳定：
   - `compute_spin` 始终最长；
   - `io_burst` 与 `info_flood` 明显短于 `compute_spin`；
   - `illegal_trap` 很快进入异常处理路径；
3. 由于统计基于 QEMU 来宾周期而非宿主墙钟时间，绝对值不应用于性能结论，只应用于特征比较和验收。

## 8. 验收检查对应关系

1. “提供 3 个独立测试源码”：
   - 实际提供了 4 个独立源码文件：
     - [io_burst.rs](/root/os_experiments/lab2/task2/src/apps/io_burst.rs)
     - [compute_spin.rs](/root/os_experiments/lab2/task2/src/apps/compute_spin.rs)
     - [info_flood.rs](/root/os_experiments/lab2/task2/src/apps/info_flood.rs)
     - [illegal_trap.rs](/root/os_experiments/lab2/task2/src/apps/illegal_trap.rs)
2. “观测结果能够印证行为特征”：
   - `io_burst` 的 `write=24`，而 `compute_spin` 和 `info_flood` 的 `write=0`；
   - `info_flood` 的 `get_taskinfo=20`，而其他任务均为 `0`；
   - `compute_spin` 的 `cycles` 始终最高，印证其计算密集特征；
   - `illegal_trap` 稳定记录 `mcause=0x2`，说明异常路径被统计框架正确覆盖。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境完成，使用：
  - `rustc 1.94.1`
  - `cargo 1.94.1`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器复现。
- 该实验仍是教学化最小内核：没有分页和真实多地址空间，`PMP` 规则也被放宽到足以支撑 U-mode 执行；因此这里重点验证的是“统计逻辑”和“trap/syscall 路径”，而不是完整 OS 隔离模型。
