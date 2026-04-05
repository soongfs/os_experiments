# LAB3 用户态 Task3：用户态/内核态时间统计验证

## 1. 原始任务说明

### 任务标题

用户态/内核态时间统计验证

### 任务目标

理解 CPU 时间记账的基本概念，验证内核对用户态/内核态时间的区分统计。

### 任务要求

1. 编写长时间后台任务（用户态计算密集）；
2. 编写系统调用密集任务（内核态时间占比更高）；
3. 对比两类任务的时间统计结果，说明差异原因。

### 验收检查

1. 任务 1（计算型）的用户态时间远大于内核态时间；
2. 任务 2（Syscall 型）的内核态时间占比明显上升；
3. 提供相关统计的终端输出。

## 2. 实验目标与实现思路

本实验在 [lab3/task3](/root/os_experiments/lab3/task3) 中实现了一个最小 RISC-V 裸机 guest 内核和两个顺序运行的 U-mode 用户任务。运行环境是 QEMU `virt` 机器中的 guest，而不是宿主 Linux 进程。

实验的核心不是调度器，而是“在用户态和内核态边界处做时间记账”：

1. 当 CPU 通过 `ecall` 从 U-mode 进入 trap 时，内核读取 `mtime`，把 `now - last_tick` 累加到当前任务的 `user_ticks`；
2. trap 处理期间，CPU 被认为处在 kernel slice；
3. 在返回 U-mode 前，内核再次读取 `mtime`，把这段时间累加到当前任务的 `kernel_ticks`。

这样即使没有启用周期性时钟中断，也能在每次“用户态 -> 内核态 -> 用户态”边界上准确区分时间归属。

两个 workload 设计如下：

- `compute_background`：
  - 仅在用户态执行长时间整数计算；
  - 中途不做系统调用，只在结束时执行一次 `finish`；
  - 预期绝大部分时间都被记为用户态时间。
- `syscall_probe`：
  - 执行 `60000` 次 `SYS_PROBE`；
  - 每次调用都会通过 `ecall` 进入内核；
  - 内核还会在 `sys_probe()` 内做一小段固定整数运算 `kernel_probe_spin=48`，放大内核停留时间；
  - 预期内核态时间占比明显上升。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/task3/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/task3/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/task3/linker.ld)：镜像布局、内核栈与两个用户栈。
- [src/boot.S](/root/os_experiments/lab3/task3/src/boot.S)：启动入口、`enter_task` 与 trap 汇编入口。
- [src/trap.rs](/root/os_experiments/lab3/task3/src/trap.rs)：`TrapFrame` 定义和 trap 到 Rust 的桥接。
- [src/main.rs](/root/os_experiments/lab3/task3/src/main.rs)：`mtime` 读取、user/kernel 时间记账、任务切换、最终统计和验收输出。
- [src/syscall.rs](/root/os_experiments/lab3/task3/src/syscall.rs)：用户态 `probe/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab3/task3/src/console.rs)：内核 UART 输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/task3/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/task3/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/task3/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/accounting_objdump.txt](/root/os_experiments/lab3/task3/artifacts/accounting_objdump.txt)：`ecall`、trap 入口和 `sys_probe` 反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/task3/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/task3
```

构建：

```bash
cargo build
```

运行一次并保存统计输出：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_task3 > artifacts/run_output.txt
```

再次运行做复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_task3 > artifacts/run_output_repeat.txt
```

导出 `ecall` 和记账路径的反汇编证据：

```bash
cargo objdump --bin lab3_task3 -- --demangle -d | rg -n -C 5 "enter_task|trap_entry|lab3_task3::syscall::invoke_syscall3|lab3_task3::sys_probe|ecall" > artifacts/accounting_objdump.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/accounting_objdump.txt
cat artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/task3/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
```

### 5.2 第一次完整运行结果

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/task3/artifacts/run_output.txt)：

```text
[kernel] stats[compute_background]: user=1392767 ticks (139276 us, 99.87%), kernel=1693 ticks (169 us, 0.12%), total=1394460 ticks (139446 us), syscalls=1, result=0x886bdb03e6f7f8c8
[kernel] stats[syscall_probe]: user=959524 ticks (95952 us, 39.29%), kernel=1482342 ticks (148234 us, 60.70%), total=2441866 ticks (244186 us), syscalls=60001, result=0x000000000000ea60
[kernel] acceptance task1 compute user_time >> kernel_time: PASS
[kernel] acceptance task2 syscall kernel ratio increased clearly: PASS
```

从第一次运行可直接看出：

- 计算型任务：
  - 用户态 `99.87%`
  - 内核态 `0.12%`
- Syscall 型任务：
  - 用户态 `39.29%`
  - 内核态 `60.70%`

也就是说，第二类任务的内核态占比不仅“上升”，而且已经高于用户态占比。

### 5.3 第二次完整运行结果

以下内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/task3/artifacts/run_output_repeat.txt)：

```text
[kernel] stats[compute_background]: user=1335302 ticks (133530 us, 99.83%), kernel=2204 ticks (220 us, 0.16%), total=1337506 ticks (133750 us), syscalls=1, result=0x886bdb03e6f7f8c8
[kernel] stats[syscall_probe]: user=952988 ticks (95298 us, 39.29%), kernel=1472377 ticks (147237 us, 60.70%), total=2425365 ticks (242536 us), syscalls=60001, result=0x000000000000ea60
[kernel] acceptance task1 compute user_time >> kernel_time: PASS
[kernel] acceptance task2 syscall kernel ratio increased clearly: PASS
```

第二次运行的比例与第一次几乎一致：

- `compute_background` 仍然约 `99.8%` 在用户态；
- `syscall_probe` 仍然约 `60.7%` 在内核态。

这说明实验结果可重复，满足“提供相关统计终端输出”的验收要求。

### 5.4 反汇编证据

[artifacts/accounting_objdump.txt](/root/os_experiments/lab3/task3/artifacts/accounting_objdump.txt) 中可以直接看到：

```text
00000000800008cc <lab3_task3::syscall::invoke_syscall3::...>:
800008ec: 00000073      ecall

0000000080000cc0 <trap_entry>:
80000cc0: 34011173      csrrw sp, mscratch, sp
80000cc4: df010113      addi  sp, sp, -0x210

00000000800021d0 <lab3_task3::sys_probe::...>:
...
```

这证明：

1. 用户态 `probe()` 确实通过 `ecall` 进入内核；
2. trap 入口会保存上下文；
3. `sys_probe()` 在内核态真实执行，因而能够增加内核态时间占比。

## 6. 机制解释

### 6.1 记账边界在哪里

本实验只有一种进入内核的路径：`ecall`。

当用户任务执行 syscall wrapper 时：

1. `invoke_syscall3()` 把 syscall 号放入 `a7`，执行 `ecall`；
2. 硬件跳转到 `trap_entry`；
3. Rust 侧 `dispatch_trap()` 第一件事就是读取 `mtime`，把这段时间记入当前任务 `user_ticks`；
4. syscall 处理完成后，内核再次读取 `mtime`，把中间这段时间记入 `kernel_ticks`；
5. 返回 U-mode 前，把 `LAST_ACCOUNT_TICK` 更新为新的用户态起点。

因此 user/kernel 时间的分界点不是“函数调用边界”，而是“特权级切换边界”。

### 6.2 为什么计算型任务几乎全是用户态时间

`compute_background` 在用户态做 `4000000` 次整数递推，只在最后执行一次 `finish`。

所以：

- 大部分时间都花在 U-mode 循环里；
- 内核只在最后一次 `finish` syscall 中短暂停留；
- 最终统计自然表现为 `user >> kernel`。

第一次运行里：

```text
user = 139276 us
kernel = 169 us
```

这已经远大于“10 倍关系”，而是接近三个数量级。

### 6.3 为什么 syscall 型任务的内核时间占比会明显上升

`syscall_probe` 每次循环都执行一次 `SYS_PROBE`，总共 `60000` 次，并且内核 `sys_probe()` 还会做一小段固定整数计算。

这意味着：

- 用户态只负责执行短小的 wrapper 和循环控制；
- 每次操作都需要进入 M-mode；
- trap 保存/恢复、syscall 分发和 `sys_probe()` 自身开销都被计入 `kernel_ticks`。

于是最终表现为：

```text
user ≈ 39.29%
kernel ≈ 60.70%
```

这正是“系统调用密集任务的内核态时间占比更高”的预期现象。

## 7. 对比分析

### 7.1 两类任务的时间分布差异

第一次运行中：

- `compute_background`：
  - 用户态 `139276 us`
  - 内核态 `169 us`
  - 内核态占比仅 `0.12%`
- `syscall_probe`：
  - 用户态 `95952 us`
  - 内核态 `148234 us`
  - 内核态占比 `60.70%`

直接对比可见：

- 第一类任务的时间几乎都消耗在用户态计算；
- 第二类任务的大头开销来自频繁陷入内核和内核处理逻辑。

### 7.2 差异原因

差异的根因是“陷入内核的频率”不同：

- 计算型任务：
  - 只有最后一次 `finish` 会触发 trap；
  - 几乎一直停留在 U-mode。
- Syscall 型任务：
  - 每次 `probe()` 都触发 trap；
  - 大量时间花在 trap 入口、syscall 分发和 `sys_probe()` 本身。

因此即便两类任务都在“做工作”，CPU 时间的归属仍然会因为特权级切换频率不同而明显改变。

## 8. 验收检查对应关系

1. 任务 1（计算型）的用户态时间远大于内核态时间：
   - 第一次运行：`99.87%` vs `0.12%`
   - 第二次运行：`99.83%` vs `0.16%`
2. 任务 2（Syscall 型）的内核态时间占比明显上升：
   - 两次运行都为 `kernel ≈ 60.70%`
   - 明显高于计算型任务的 `0.12% ~ 0.16%`
3. 提供了相关统计终端输出：
   - 见 [run_output.txt](/root/os_experiments/lab3/task3/artifacts/run_output.txt)
   - 见 [run_output_repeat.txt](/root/os_experiments/lab3/task3/artifacts/run_output_repeat.txt)

## 9. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 用户进程。
- 版本信息见 [tool_versions.txt](/root/os_experiments/lab3/task3/artifacts/tool_versions.txt)：
  - `rustc 1.94.1 (e408947bf 2026-03-25)`
  - `cargo 1.94.1 (29ea6fb6a 2026-03-24)`
  - `riscv64gc-unknown-none-elf (installed)`
  - `QEMU emulator version 10.0.8`
- 本回合没有在第二台原生 Linux 服务器上再次复现。
- 实验中的绝对时间值依赖 QEMU TCG 和宿主调度器，但“计算型几乎全在用户态、syscall 型内核占比显著上升”这一结论在两次复验中保持一致。
