# LAB5 内核态 Task1：进程切换过程显示

## 1. 原始任务说明

### 任务标题

进程切换过程显示

### 任务目标

掌握进程上下文切换关键路径与数据结构。

### 任务要求

1. 在切换路径输出必要日志（可开关）；
2. 日志需包含：切换前/后进程标识与触发原因；
3. 输出不能显著干扰系统稳定性。

### 验收检查

1. 内核能按 PID 输出切换轨迹；
2. 可以清楚区分“主动让出（yield）”、“时间片耗尽”等不同切换原因。

## 2. Acceptance -> Evidence 清单

- 切换日志确实位于核心调度路径，而不是外围演示代码。
  证据：`handle_explicit_yield()` 和 `handle_timer_interrupt()` 都调用 `switch_to()`；[artifacts/process_switch_objdump.txt](/root/os_experiments/lab5/kernel_task1/artifacts/process_switch_objdump.txt) 中可见两条路径都跳转到 `lab5_kernel_task1::switch_to`。
- 日志按 PID 输出切换前/后的进程标识。
  证据：[artifacts/run_output.txt](/root/os_experiments/lab5/kernel_task1/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/kernel_task1/artifacts/run_output_repeat.txt) 中多次出现 `from pid=1` / `to pid=2`。
- 日志能区分 `explicit_yield` 与 `time_slice` 两类触发原因。
  证据：同一份运行日志中同时出现 `reason=explicit_yield` 和 `reason=time_slice`。
- 日志不会无限刷屏，且可控开关存在。
  证据：源码中的 `ENABLE_PROCESS_SWITCH_TRACE` 与 `PROCESS_SWITCH_TRACE_LIMIT`；运行日志里只出现一次 `switch trace limit reached` 提示，后续切换不再继续打印。
- 验收项 1 和 2 都有明确 PASS。
  证据：运行末尾出现 `[kernel] acceptance yield reason observed: PASS`、`[kernel] acceptance time_slice reason observed: PASS` 和 `[kernel] acceptance pid-based readable switch trace captured: PASS`。

## 3. 实验目标与实现思路

本实验在 [lab5/kernel_task1](/root/os_experiments/lab5/kernel_task1) 中实现为 QEMU `virt` 机器上的 RISC-V 裸机 guest 内核，不是宿主 Linux 进程。

实现直接复用了 [lab3/kernel_task1](/root/os_experiments/lab3/kernel_task1) 的稳定最小内核骨架，因为它已经具备：

- `trap_entry -> rust_handle_trap -> scheduler` 的完整切换路径；
- 用户态显式 `yield` 和定时器抢占两种切换触发源；
- 可重复的 `TrapFrame` 保存/恢复流程。

在这个骨架上，本题只做最小但关键的改动：

1. 把对外日志语义从“task”收敛成“process / PID”；
2. 统一在 `switch_to()` 内打印 `save_done` / `restore_begin` 两个关键边界；
3. 为每次切换带上 `pid`、`name`、`reason` 和 `mepc`；
4. 通过开关和限流控制日志量，避免显著扰动系统。

实验中的两个被调度实体是：

- `pid=1 name=yield_proc`
  - 用户态主动执行 `SYS_YIELD`
- `pid=2 name=timeslice_proc`
  - 长时间计算，依靠 `mtime/mtimecmp` 定时器中断触发抢占

这样可以稳定生成两类不同原因的切换轨迹。

## 4. 关键数据结构与路径

- [src/main.rs](/root/os_experiments/lab5/kernel_task1/src/main.rs)
  - `TaskControlBlock`：保存每个进程的 `TrapFrame`、运行状态和退出码；
  - `TASK_DEFS`：定义 `pid/name/entry`；
  - `CURRENT_TASK`：记录当前正在运行的进程槽位；
  - `SwitchReason`：记录 `boot / explicit_yield / time_slice / task_exit`；
  - `switch_to()`：真正执行“保存 A、选择 B、恢复 B”并打印日志的位置。
- [src/trap.rs](/root/os_experiments/lab5/kernel_task1/src/trap.rs)
  - `TrapFrame`：保存通用寄存器、用户栈、`mepc` 和浮点寄存器，是进程上下文切换的核心数据结构。
- [src/boot.S](/root/os_experiments/lab5/kernel_task1/src/boot.S)
  - `trap_entry`：汇编入口，负责把当前用户态上下文压成 `TrapFrame`；
  - `enter_task`：把目标 `TrapFrame` 恢复回机器寄存器并 `mret`。
- [src/syscall.rs](/root/os_experiments/lab5/kernel_task1/src/syscall.rs)
  - 用户态 `yield_now()` / `finish()` 的 `ecall` 封装。

## 5. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab5/kernel_task1/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab5/kernel_task1/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标和链接脚本。
- [Cargo.lock](/root/os_experiments/lab5/kernel_task1/Cargo.lock)：Cargo 锁文件。
- [linker.ld](/root/os_experiments/lab5/kernel_task1/linker.ld)：镜像布局、内核栈和两个用户栈。
- [src/boot.S](/root/os_experiments/lab5/kernel_task1/src/boot.S)：启动入口、trap 汇编入口、上下文保存/恢复。
- [src/trap.rs](/root/os_experiments/lab5/kernel_task1/src/trap.rs)：`TrapFrame` 和 trap 向量初始化。
- [src/syscall.rs](/root/os_experiments/lab5/kernel_task1/src/syscall.rs)：用户态 `yield/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab5/kernel_task1/src/console.rs)：UART 输出。
- [src/main.rs](/root/os_experiments/lab5/kernel_task1/src/main.rs)：调度状态机、切换日志、定时器中断处理和验收输出。
- [artifacts/build_output.txt](/root/os_experiments/lab5/kernel_task1/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab5/kernel_task1/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/kernel_task1/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/process_switch_objdump.txt](/root/os_experiments/lab5/kernel_task1/artifacts/process_switch_objdump.txt)：`trap_entry`、`handle_explicit_yield`、`handle_timer_interrupt`、`switch_to` 和 `ecall` 的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab5/kernel_task1/artifacts/tool_versions.txt)：工具链与 QEMU 版本。

## 6. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab5/kernel_task1
```

构建：

```bash
cargo build
```

保存构建输出：

```bash
cargo build > artifacts/build_output.txt 2>&1
```

第一次运行：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task1 > artifacts/run_output.txt 2>&1
```

第二次运行：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task1 > artifacts/run_output_repeat.txt 2>&1
```

导出调度路径反汇编：

```bash
cargo objdump --bin lab5_kernel_task1 -- --demangle -d | rg -n -C 5 "lab5_kernel_task1::switch_to|lab5_kernel_task1::handle_timer_interrupt|lab5_kernel_task1::handle_explicit_yield|lab5_kernel_task1::syscall::invoke_syscall3|trap_entry|ecall" > artifacts/process_switch_objdump.txt
```

记录工具链：

```bash
{ printf 'rustc: '; rustc --version; printf 'cargo: '; cargo --version; printf 'targets:\n'; rustup target list | grep riscv64gc; printf 'qemu: '; qemu-system-riscv64 --version | head -n 1; } > artifacts/tool_versions.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/process_switch_objdump.txt
cat artifacts/tool_versions.txt
```

## 7. 本次实际运行结果

### 7.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab5/kernel_task1/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab5_kernel_task1 v0.1.0 (/root/os_experiments/lab5/kernel_task1)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s
```

### 7.2 第一次运行的关键日志

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab5/kernel_task1/artifacts/run_output.txt)：

```text
[sched] boot restore_begin: to pid=1 name=yield_proc reason=boot next_mepc=0x800026fc
[sched] switch#01 save_done: from pid=1 name=yield_proc reason=explicit_yield saved_mepc=0x80000522
[sched] switch#01 restore_begin: to pid=2 name=timeslice_proc reason=explicit_yield next_mepc=0x8000269a
[sched] switch#02 save_done: from pid=2 name=timeslice_proc reason=time_slice saved_mepc=0x8000269a
[sched] switch#02 restore_begin: to pid=1 name=yield_proc reason=time_slice next_mepc=0x80000522
...
[sched] switch trace limit reached at 12 record(s); further switches suppressed
[kernel] summary: total_switches=88 explicit_yield_switches=3 time_slice_switches=84 process_exit_switches=1 timer_interrupts=84
[kernel] acceptance yield reason observed: PASS
[kernel] acceptance time_slice reason observed: PASS
[kernel] acceptance pid-based readable switch trace captured: PASS
```

从第一次运行可以直接读出：

1. 切换日志按 `pid=` 输出，满足“按 PID 输出轨迹”；
2. 第一类切换原因是 `explicit_yield`；
3. 第二类切换原因是 `time_slice`；
4. 日志在 `12` 组后自动抑制，没有无限刷屏。

### 7.3 第二次运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab5/kernel_task1/artifacts/run_output_repeat.txt) 的关键片段与第一次一致：

```text
[sched] switch#01 save_done: from pid=1 name=yield_proc reason=explicit_yield ...
[sched] switch#01 restore_begin: to pid=2 name=timeslice_proc reason=explicit_yield ...
[sched] switch#02 save_done: from pid=2 name=timeslice_proc reason=time_slice ...
[sched] switch#02 restore_begin: to pid=1 name=yield_proc reason=time_slice ...
...
[kernel] summary: total_switches=88 explicit_yield_switches=3 time_slice_switches=84 process_exit_switches=1 timer_interrupts=84
```

第二次运行再次得到：

```text
[kernel] acceptance yield reason observed: PASS
[kernel] acceptance time_slice reason observed: PASS
[kernel] acceptance pid-based readable switch trace captured: PASS
```

这说明实验结果稳定、可重复。

### 7.4 反汇编证据

[artifacts/process_switch_objdump.txt](/root/os_experiments/lab5/kernel_task1/artifacts/process_switch_objdump.txt) 中可以直接看到：

```text
00000000800004fe <lab5_kernel_task1::syscall::invoke_syscall3...>:
8000051e: 00000073      ecall

00000000800009e0 <trap_entry>:
...

0000000080001ca2 <lab5_kernel_task1::handle_explicit_yield...>:
...
80001d4c: ... <lab5_kernel_task1::switch_to...>

0000000080001d82 <lab5_kernel_task1::handle_timer_interrupt...>:
...
80001e9e: ... <lab5_kernel_task1::switch_to...>

0000000080002190 <lab5_kernel_task1::switch_to...>:
...
```

这证明：

1. 用户态 `yield` 通过 `ecall` 进入内核；
2. trap 入口真实存在且位于上下文保存/恢复路径上；
3. 显式让出和时间片中断两条路径都会调用 `switch_to()`；
4. 日志位置确实靠近核心切换路径，而不是外围打印代码。

## 8. 机制解释

### 8.1 为什么日志要放在 `switch_to()`

只在 trap 入口打印，最多只能证明“进了内核”；但题目真正要看的是：

- 当前进程 A 何时已经保存完毕；
- 下一个进程 B 何时开始恢复。

这两个边界只有在 `switch_to()` 里同时可见：

1. 调用者先把当前 `TrapFrame` 写回 `TASKS[from].frame`；
2. `switch_to()` 打印 `save_done: from pid=...`；
3. `switch_to()` 打印 `restore_begin: to pid=...`；
4. 最后把 `TASKS[to].frame` 拷回当前 trap frame，`mret` 后恢复 B。

因此把日志统一放在 `switch_to()`，才能稳定得到“前/后进程标识 + 原因 + 关键边界”的完整切换轨迹。

### 8.2 关键数据结构如何参与上下文切换

本实验的上下文切换核心数据结构有三层：

- `TrapFrame`
  - 保存通用寄存器、用户栈、`mepc` 和浮点上下文，是“进程现场”的完整镜像；
- `TaskControlBlock`
  - 为每个进程保存一个 `TrapFrame`、运行状态和退出码；
- `CURRENT_TASK`
  - 指向当前正在运行的进程槽位。

切换过程可以概括成：

1. trap 发生时，`trap_entry` 在内核栈上保存当前寄存器；
2. Rust 侧把这份 trap frame 存回当前 PCB；
3. 调度器选择下一个可运行 PCB；
4. `switch_to()` 把目标 PCB 中保存的 trap frame 恢复到当前 trap frame；
5. `mret` 返回用户态，目标进程继续执行。

### 8.3 为什么输出不会显著干扰系统稳定性

本题明确要求“输出不能显著干扰系统稳定性”，所以实验没有无界打印，而是用了两级控制：

- `ENABLE_PROCESS_SWITCH_TRACE`
  - 总开关，可以彻底关闭切换日志；
- `PROCESS_SWITCH_TRACE_LIMIT = 12`
  - 只保留前 `12` 组切换记录，之后只打印一次抑制提示。

这意味着：

- 能清楚展示早期关键切换轨迹；
- 不会因为 timer interrupt 高频触发而把串口打爆；
- 后续系统仍能稳定完成所有进程并输出总结统计。

## 9. 验收检查映射

- [x] 内核能按 PID 输出切换轨迹。
  证据：运行日志中多次出现 `from pid=1`、`to pid=2`。
- [x] 可以清楚区分“主动让出（yield）”、“时间片耗尽”等不同切换原因。
  证据：同一份日志中同时出现 `reason=explicit_yield` 和 `reason=time_slice`，且末尾两条验收输出都为 `PASS`。

## 10. 环境说明、限制与未决事项

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 用户进程。
- 当前实现是教学内核里的最小双进程切换模型，不是完整 Unix 进程管理器。
- 日志限流保证了稳定性，但也意味着只保留早期 `12` 组切换明细；后续切换通过 summary 计数体现。
- 工具链与 QEMU 版本见 [artifacts/tool_versions.txt](/root/os_experiments/lab5/kernel_task1/artifacts/tool_versions.txt)。
