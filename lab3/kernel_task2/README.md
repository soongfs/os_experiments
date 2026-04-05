# LAB3 内核态 Task2：用户态/内核态完成时间统计

## 1. 原始任务说明

### 任务标题

用户态/内核态完成时间统计

### 任务目标

在完成时间的基础上细分记账来源，理解特权级切换与计时的关系。

### 任务要求

1. 分别统计用户态与内核态执行时间；
2. 给出统计口径（在何处开始/停止计数）；
3. 对用户态计算密集与系统调用密集任务，应能体现时间占比差异。

### 验收检查

1. 进程控制块维护了 `utime` 和 `stime` 计数；
2. Trap 进入和退出时准确进行了时间戳更新与累加。

## 2. 实验目标与实现思路

本实验在 [lab3/kernel_task2](/root/os_experiments/lab3/kernel_task2) 中实现了一个最小 RISC-V 裸机 guest 内核，并把时间记账点放到真正的 trap 进入和 trap 退出边界上，而不是只在 syscall 逻辑内部做近似统计。

实现思路如下：

1. 每个任务维护一个 `ProcessControlBlock`：
   - `utime`：累计用户态时间；
   - `stime`：累计内核态时间；
   - `last_timestamp`：上一次记账边界时间戳；
   - `mode`：`Dormant/User/Kernel/Finished`；
   - 以及 `start_tick`、`finish_tick`、`syscalls`、`trap_entries` 等辅助字段。
2. trap 汇编入口 [boot.S](/root/os_experiments/lab3/kernel_task2/src/boot.S) 在保存完上下文后，按顺序调用：
   - `rust_account_trap_enter(frame)`
   - `rust_handle_trap(frame)`
   - `rust_account_trap_exit(frame)`
3. 记账口径：
   - `utime`：在 `trap_enter` 读取 `mtime`，把 `[last_timestamp, trap_enter)` 累加到当前 PCB 的 `utime`；
   - `stime`：在 `trap_exit` 读取 `mtime`，把 `[last_timestamp, trap_exit)` 累加到当前 PCB 的 `stime`；
   - 对最后一个 `finish` 任务，因为不会再执行 `trap_exit -> mret`，单独走 `task_complete` 收口，把最后一段 kernel slice 累加到 `stime`。
4. 两类 workload：
   - `compute_user_heavy`：仅做长时间用户态整数计算，最后只执行一次 `finish`；
   - `syscall_kernel_heavy`：重复执行 `60000` 次 `SYS_PROBE`，每次都通过 `ecall` 进入内核，并在内核中做固定 `kernel_probe_spin=48` 次运算，放大 `stime` 占比。

为了便于验证 trap 边界，本实验还加入了可控限流的 accounting trace：

- `ENABLE_ACCOUNTING_TRACE`
- `ACCOUNTING_TRACE_LIMIT`

默认只输出前 `8` 条 enter/exit 记账事件，避免刷屏。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/kernel_task2/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/kernel_task2/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/kernel_task2/linker.ld)：镜像布局、内核栈与两个用户栈。
- [src/boot.S](/root/os_experiments/lab3/kernel_task2/src/boot.S)：启动入口、`enter_task`、trap 汇编入口和 `rust_account_trap_enter/rust_handle_trap/rust_account_trap_exit` 调用顺序。
- [src/trap.rs](/root/os_experiments/lab3/kernel_task2/src/trap.rs)：`TrapFrame` 定义、trap 向量初始化与 Rust trap/accounting hook 桥接。
- [src/main.rs](/root/os_experiments/lab3/kernel_task2/src/main.rs)：PCB、`utime/stime` 记账、workload、统计汇总和验收判断。
- [src/syscall.rs](/root/os_experiments/lab3/kernel_task2/src/syscall.rs)：用户态 `probe/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab3/kernel_task2/src/console.rs)：内核 UART 输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task2/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task2/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task2/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/accounting_objdump.txt](/root/os_experiments/lab3/kernel_task2/artifacts/accounting_objdump.txt)：`ecall`、`trap_entry`、`rust_account_trap_enter/exit` 和对应 Rust 记账函数的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/kernel_task2/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/kernel_task2
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
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task2 > artifacts/run_output.txt
```

第二次运行：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task2 > artifacts/run_output_repeat.txt
```

导出反汇编证据：

```bash
cargo objdump --bin lab3_kernel_task2 -- --demangle -d | rg -n -C 5 "rust_account_trap_enter|rust_account_trap_exit|trap_entry|lab3_kernel_task2::account_trap_enter|lab3_kernel_task2::account_trap_exit|lab3_kernel_task2::syscall::invoke_syscall3|ecall" > artifacts/accounting_objdump.txt
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
cat artifacts/accounting_objdump.txt
cat artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task2/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 5.2 第一次运行结果

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task2/artifacts/run_output.txt)：

```text
[acct] trap_enter: task=1(compute_user_heavy) mepc=0x8000004c add_utime=1448489 ticks total_utime=1448489 ticks
[acct] trap_exit: charge=1(compute_user_heavy) resume=2(syscall_kernel_heavy) next_mepc=0x80002b5c add_stime=18849 ticks total_stime=18849 ticks
...
[kernel] pcb[1:compute_user_heavy]: utime=1448489 ticks (144848 us, 98.71%), stime=18849 ticks (1884 us, 1.28%), total_accounted=1467338 ticks (146733 us), elapsed=1467338 ticks (146733 us), gap=0 ticks, syscalls=1, trap_entries=1, result=0x886bdb03e6f7f8c8
[kernel] pcb[2:syscall_kernel_heavy]: utime=1102652 ticks (110265 us, 38.47%), stime=1762898 ticks (176289 us, 61.52%), total_accounted=2865550 ticks (286555 us), elapsed=2865550 ticks (286555 us), gap=0 ticks, syscalls=60001, trap_entries=60001, result=0x000000000000ea60
[kernel] accounting events: trap_enter_updates=60002 trap_exit_updates=60001 task_complete_updates=1
[kernel] acceptance pcb utime/stime maintained: PASS
[kernel] acceptance trap enter/exit timestamp updates balanced: PASS
[kernel] acceptance compute task utime >> stime: PASS
[kernel] acceptance syscall task kernel ratio increased clearly: PASS
```

从第一次运行可以直接读出：

- `compute_user_heavy`：
  - `utime = 98.71%`
  - `stime = 1.28%`
- `syscall_kernel_heavy`：
  - `utime = 38.47%`
  - `stime = 61.52%`

也就是说，计算密集任务几乎都落在用户态时间中，而 syscall 密集任务的内核态时间已经明显高于用户态时间。

另一个重要结果是：

```text
total_accounted == elapsed
gap = 0 ticks
```

说明本实验定义下的完成时间，已经被 `utime + stime` 完整分解。

### 5.3 第二次运行结果

以下内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task2/artifacts/run_output_repeat.txt)：

```text
[kernel] pcb[1:compute_user_heavy]: utime=1470915 ticks (147091 us, 98.75%), stime=18501 ticks (1850 us, 1.24%), total_accounted=1489416 ticks (148941 us), elapsed=1489416 ticks (148941 us), gap=0 ticks, syscalls=1, trap_entries=1, result=0x886bdb03e6f7f8c8
[kernel] pcb[2:syscall_kernel_heavy]: utime=1064079 ticks (106407 us, 37.45%), stime=1776688 ticks (177668 us, 62.54%), total_accounted=2840767 ticks (284076 us), elapsed=2840767 ticks (284076 us), gap=0 ticks, syscalls=60001, trap_entries=60001, result=0x000000000000ea60
[kernel] acceptance pcb utime/stime maintained: PASS
[kernel] acceptance trap enter/exit timestamp updates balanced: PASS
[kernel] acceptance compute task utime >> stime: PASS
[kernel] acceptance syscall task kernel ratio increased clearly: PASS
```

第二次运行比例和第一次一致：

- 计算型任务仍然约 `98.7%` 在用户态；
- syscall 型任务仍然约 `62%` 在内核态；
- `gap` 仍然是 `0 ticks`。

这说明实验结果稳定、可重复。

### 5.4 反汇编证据

[artifacts/accounting_objdump.txt](/root/os_experiments/lab3/kernel_task2/artifacts/accounting_objdump.txt) 中可以直接看到：

```text
000000008000002c <lab3_kernel_task2::syscall::invoke_syscall3::...>:
8000004c: 00000073      ecall

00000000800001e0 <trap_entry>:
...
80000282: ... <rust_account_trap_enter>
80000288: ... <rust_handle_trap>
80000296: ... <rust_account_trap_exit>

0000000080000b6a <lab3_kernel_task2::account_trap_exit::...>:
...

0000000080000d6a <lab3_kernel_task2::account_trap_enter::...>:
...
```

这证明：

1. 用户态 syscall 确实通过 `ecall` 进入内核；
2. `trap_entry` 在恢复前会先执行 `rust_account_trap_enter -> rust_handle_trap -> rust_account_trap_exit`；
3. `utime/stime` 的累加确实挂在 trap 进入和 trap 退出边界上。

## 6. 机制解释

### 6.1 PCB 中的 `utime/stime` 如何维护

每个 PCB 都维护：

- `utime`
- `stime`
- `last_timestamp`
- `mode`

其含义是：

- 当 `mode=User` 时，`last_timestamp` 表示最近一次进入用户态的时刻；
- 当 trap 发生时，`account_trap_enter()` 读取当前 `mtime`，把这段用户态切片累加到 `utime`；
- 随后把 `mode` 改成 `Kernel`，并把 `last_timestamp` 更新为 trap 进入时刻；
- 当即将恢复用户态时，`account_trap_exit()` 再读取一次 `mtime`，把这段内核态切片累加到 `stime`。

因此，`utime/stime` 不是按“函数语义”估算，而是按“特权级边界”精确切分。

### 6.2 统计口径的开始和停止位置

本实验采用的统计口径如下：

1. 开始计数：
   - 当内核决定首次把任务恢复到 U-mode 前，设置 `start_tick=last_timestamp=now`。
2. `utime` 停止位置：
   - trap 进入时，读取 `mtime`；
   - 把 `[last_timestamp, trap_enter)` 记入 `utime`。
3. `stime` 停止位置：
   - trap 即将退出到 U-mode 时，读取 `mtime`；
   - 把 `[last_timestamp, trap_exit)` 记入 `stime`。
4. 最后一个结束任务：
   - 因为不会再执行 `trap_exit -> mret`，所以在 `finalize_last_task_and_report()` 中读取 `mtime`；
   - 把最后一段 kernel slice 计入 `stime`，同时记录 `finish_tick`。

所以最终：

```text
elapsed = finish_tick - start_tick
accounted = utime + stime
```

本次两轮运行里都得到了 `gap=0`，说明这套口径自洽。

### 6.3 为什么两类任务的时间占比差异明显

- `compute_user_heavy` 只在结束时执行一次 `finish` syscall：
  - 几乎所有时间都在 U-mode 计算循环中；
  - 只有最后一次进入内核收尾；
  - 所以 `utime >> stime`。
- `syscall_kernel_heavy` 会执行 `60000` 次 `SYS_PROBE`：
  - 每次都会发生 `U -> M trap -> U`；
  - 并且内核 `sys_probe()` 还做了固定长度整数运算；
  - 因而 `stime` 占比显著上升，且高于 `utime`。

## 7. 验收检查对应关系

1. 进程控制块维护了 `utime` 和 `stime`：
   - [main.rs](/root/os_experiments/lab3/kernel_task2/src/main.rs) 中的 `ProcessControlBlock` 直接包含 `utime`、`stime`、`last_timestamp` 和 `mode`；
   - 运行输出也直接打印了每个 PCB 的 `utime/stime`。
2. Trap 进入和退出时准确进行了时间戳更新与累加：
   - [boot.S](/root/os_experiments/lab3/kernel_task2/src/boot.S) 中，`trap_entry` 在恢复前依次调用 `rust_account_trap_enter`、`rust_handle_trap`、`rust_account_trap_exit`；
   - [trap.rs](/root/os_experiments/lab3/kernel_task2/src/trap.rs) 把这两个 hook 接到 Rust；
   - [run_output.txt](/root/os_experiments/lab3/kernel_task2/artifacts/run_output.txt) 中可以直接看到 `trap_enter` 和 `trap_exit` 的记账日志；
   - `trap_enter_updates=60002`、`trap_exit_updates=60001`、`task_complete_updates=1` 与总 syscall 数严格平衡，说明每次进入内核都得到了对应的时间归属更新。
3. 计算密集和 syscall 密集任务体现了时间占比差异：
   - 第一次运行约 `98.71% vs 1.28%`；
   - 第一次运行约 `38.47% vs 61.52%`；
   - 第二次运行趋势一致。

## 8. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 进程。
- 工具链版本见 [tool_versions.txt](/root/os_experiments/lab3/kernel_task2/artifacts/tool_versions.txt)。
- 当前实验只覆盖 `ecall` 导致的 `U -> M -> U` trap 路径，没有引入时间片中断或阻塞唤醒。
- `mtime` 粒度为 `100 ns`，极短路径仍会受计时粒度影响；但两轮运行中比例和 `gap` 都稳定。
