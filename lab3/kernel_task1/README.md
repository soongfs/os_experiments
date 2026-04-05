# LAB3 内核态 Task1：任务切换过程可视化

## 1. 原始任务说明

### 任务标题

任务切换过程可视化

### 任务要求

1. 在切换路径增加必要日志（可控开关，避免刷屏）；
2. 至少展示：从 task A 切到 task B 的关键点；
3. 输出需可读：包含 task id/name 与切换原因（时间片/阻塞/显式让出等）。

### 验收检查

1. 日志打印在内核核心 `switch_to` 或调度函数附近；
2. 能够清晰看出上下文（A）保存完毕与上下文（B）恢复开始的时机。

## 2. 实验目标与实现思路

本实验在 [lab3/kernel_task1](/root/os_experiments/lab3/kernel_task1) 中实现了一个最小可运行的 RISC-V 裸机 guest 内核，重点不是“复杂调度策略”，而是把日志准确放在任务切换的核心路径上。

实现采用两类切换触发源：

- `yield_demo`：
  - 用户态显式调用 `SYS_YIELD`
  - 用来生成 `explicit_yield` 类型的切换日志
- `timeslice_demo`：
  - 用户态长时间计算
  - 由 `mtime/mtimecmp` 定时器中断触发 `time_slice` 类型的切换日志

日志只在 [main.rs](/root/os_experiments/lab3/kernel_task1/src/main.rs) 的 `switch_to()` 中打印，并且提供两个限流常量：

- `ENABLE_SWITCH_TRACE`
- `SWITCH_TRACE_LIMIT`

默认配置是开启日志，但只保留前 `10` 组切换记录，避免刷屏。

每次实际切换都会打印两行：

1. `save_done`：
   - 表示当前任务 A 的 trap frame 已经保存完毕
2. `restore_begin`：
   - 表示即将把下一个任务 B 的 trap frame 恢复到当前 trap 返回路径

这正对应验收要求里“上下文（A）保存完毕”和“上下文（B）恢复开始”的两个关键时机。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/kernel_task1/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/kernel_task1/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/kernel_task1/linker.ld)：镜像布局、内核栈与两个用户栈。
- [src/boot.S](/root/os_experiments/lab3/kernel_task1/src/boot.S)：启动入口、`enter_task` 和 trap 汇编入口。
- [src/trap.rs](/root/os_experiments/lab3/kernel_task1/src/trap.rs)：`TrapFrame` 定义和 trap 到 Rust 的桥接。
- [src/main.rs](/root/os_experiments/lab3/kernel_task1/src/main.rs)：调度状态机、`switch_to()`、时间片中断、切换日志和最终汇总。
- [src/syscall.rs](/root/os_experiments/lab3/kernel_task1/src/syscall.rs)：用户态 `yield/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab3/kernel_task1/src/console.rs)：内核 UART 输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task1/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task1/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task1/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/switch_trace_objdump.txt](/root/os_experiments/lab3/kernel_task1/artifacts/switch_trace_objdump.txt)：`trap_entry`、`handle_explicit_yield`、`handle_timer_interrupt`、`switch_to` 和 `ecall` 的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/kernel_task1/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/kernel_task1
```

构建：

```bash
cargo build
```

运行一次并保存日志：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task1 > artifacts/run_output.txt
```

再次运行做复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task1 > artifacts/run_output_repeat.txt
```

导出切换路径反汇编：

```bash
cargo objdump --bin lab3_kernel_task1 -- --demangle -d | rg -n -C 5 "lab3_kernel_task1::switch_to|lab3_kernel_task1::handle_timer_interrupt|lab3_kernel_task1::handle_explicit_yield|lab3_kernel_task1::syscall::invoke_syscall3|trap_entry|ecall" > artifacts/switch_trace_objdump.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/switch_trace_objdump.txt
cat artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task1/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 5.2 第一次运行的关键日志

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task1/artifacts/run_output.txt)：

```text
[sched] switch#01 save_done: from id=1 name=yield_demo reason=explicit_yield saved_mepc=0x80002afa
[sched] switch#01 restore_begin: to id=2 name=timeslice_demo reason=explicit_yield next_mepc=0x80002578
[sched] switch#02 save_done: from id=2 name=timeslice_demo reason=time_slice saved_mepc=0x80002578
[sched] switch#02 restore_begin: to id=1 name=yield_demo reason=time_slice next_mepc=0x80002afa
```

这四行已经完整展示了两次关键切换：

1. `yield_demo -> timeslice_demo`
   - 原因：`explicit_yield`
   - 第一行表示 A 已保存完毕
   - 第二行表示 B 即将开始恢复
2. `timeslice_demo -> yield_demo`
   - 原因：`time_slice`
   - 第三行表示 A 已保存完毕
   - 第四行表示 B 即将开始恢复

同一次运行的汇总输出如下：

```text
[kernel] summary: total_switches=83 explicit_yield_switches=3 time_slice_switches=79 task_exit_switches=1 timer_interrupts=79
[kernel] acceptance explicit_yield observed: PASS
[kernel] acceptance time_slice observed: PASS
[kernel] acceptance multiple readable switches captured: PASS
```

### 5.3 第二次运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task1/artifacts/run_output_repeat.txt) 的关键片段与第一次一致，仍然能稳定看到：

```text
[sched] switch#01 save_done: from id=1 name=yield_demo reason=explicit_yield ...
[sched] switch#01 restore_begin: to id=2 name=timeslice_demo reason=explicit_yield ...
[sched] switch#02 save_done: from id=2 name=timeslice_demo reason=time_slice ...
[sched] switch#02 restore_begin: to id=1 name=yield_demo reason=time_slice ...
```

第二次运行汇总仍然是：

```text
[kernel] summary: total_switches=83 explicit_yield_switches=3 time_slice_switches=79 task_exit_switches=1 timer_interrupts=79
```

说明实验行为稳定，可重复复现。

### 5.4 反汇编证据

[artifacts/switch_trace_objdump.txt](/root/os_experiments/lab3/kernel_task1/artifacts/switch_trace_objdump.txt) 中可以直接看到：

```text
00000000800008c0 <trap_entry>:
...
0000000080001b80 <lab3_kernel_task1::handle_explicit_yield::...>:
...
80001c2a: ... <lab3_kernel_task1::switch_to::...>

0000000080001c60 <lab3_kernel_task1::handle_timer_interrupt::...>:
...
80001d7c: ... <lab3_kernel_task1::switch_to::...>

000000008000206e <lab3_kernel_task1::switch_to::...>:
...

0000000080002ad6 <lab3_kernel_task1::syscall::invoke_syscall3::...>:
80002af6: 00000073      ecall
```

这说明：

1. 显式让出路径 `handle_explicit_yield()` 会调用 `switch_to()`；
2. 时间片中断路径 `handle_timer_interrupt()` 也会调用 `switch_to()`；
3. 日志所在的 `switch_to()` 确实处于核心调度路径；
4. 用户态 `yield` 是通过 `ecall` 进入内核的。

## 6. 机制解释

### 6.1 为什么日志要放在 `switch_to()` 而不是 trap 入口

trap 入口只能说明“进入了内核”，但还不能说明：

- 当前任务的上下文何时保存完毕；
- 目标任务的上下文何时开始恢复。

真正同时掌握“from task”和“to task”的位置是在 [main.rs](/root/os_experiments/lab3/kernel_task1/src/main.rs) 的 `switch_to()`：

1. 调用者先把当前 trap frame 写回 `TASKS[from].frame`；
2. `switch_to()` 打印 `save_done`；
3. `switch_to()` 在覆盖当前 trap frame 前打印 `restore_begin`；
4. 最后把 `TASKS[to].frame` 拷回当前 trap frame，`mret` 后恢复 B。

所以把日志放在这里，才能准确对应“保存完毕”和“恢复开始”的两个边界时刻。

### 6.2 可控开关如何避免刷屏

实验使用：

```text
ENABLE_SWITCH_TRACE = true
SWITCH_TRACE_LIMIT = 10
```

因此：

- 想彻底关闭日志时，把 `ENABLE_SWITCH_TRACE` 设为 `false`；
- 想减少日志量时，把 `SWITCH_TRACE_LIMIT` 调小；
- 超过上限后，内核只打印一次：

```text
[sched] switch trace limit reached at 10 record(s); further switches suppressed
```

这满足“可控开关，避免刷屏”的要求。

### 6.3 为什么能同时看到两种切换原因

- `yield_demo` 在用户态显式执行 `SYS_YIELD`，因此内核记录出 `reason=explicit_yield`；
- `timeslice_demo` 长时间计算，超过 `TIME_SLICE_TICKS` 后被 `mtime/mtimecmp` 中断抢占，因此内核记录出 `reason=time_slice`。

这让日志不仅能看到“谁切给谁”，还能看到“为什么切”。

## 7. 验收检查对应关系

1. 日志打印在内核核心 `switch_to` 或调度函数附近：
   - 运行日志由 `switch_to()` 统一打印；
   - [switch_trace_objdump.txt](/root/os_experiments/lab3/kernel_task1/artifacts/switch_trace_objdump.txt) 中可见 `handle_explicit_yield` 和 `handle_timer_interrupt` 都调用了 `switch_to`。
2. 能清晰看出上下文（A）保存完毕与上下文（B）恢复开始：
   - `save_done` 表示 A 已保存；
   - `restore_begin` 表示 B 即将恢复；
   - 日志中同一 `switch#NN` 的两行是成对出现的。
3. 输出可读，包含 task id/name 与切换原因：
   - 例如：
     - `from id=1 name=yield_demo reason=explicit_yield`
     - `to id=2 name=timeslice_demo reason=time_slice`

## 8. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 用户进程。
- 版本信息见 [tool_versions.txt](/root/os_experiments/lab3/kernel_task1/artifacts/tool_versions.txt)：
  - `rustc 1.94.1 (e408947bf 2026-03-25)`
  - `cargo 1.94.1 (29ea6fb6a 2026-03-24)`
  - `riscv64gc-unknown-none-elf (installed)`
  - `QEMU emulator version 10.0.8`
- 本回合没有在第二台原生 Linux 服务器上再次复现。
- 该实验为了可视化简洁，只演示了 `explicit_yield`、`time_slice` 和最终的 `task_exit` 切换路径，没有实现阻塞队列和唤醒路径；但对本任务的验收要求已经足够。
