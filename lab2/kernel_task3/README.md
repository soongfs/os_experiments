# LAB2 内核态 Task3: 完成时间统计

## 1. 原始任务说明

### 任务标题

完成时间统计

### 任务目标

理解时间源（timer）与内核记账（accounting）的关系，掌握测量粒度与误差来源。

### 任务要求

1. 统计每个应用完成时间；
2. 说明所使用时间源（时钟中断计数/时间寄存器等）与单位；
3. 结果需可对比：对同一应用多次运行，统计应在合理波动范围内。

### 验收检查

1. 使用 `mtime` 等硬件寄存器进行精确计时，且单位换算正确（如毫秒/微秒）；
2. 计时区间合理，排除了内核加载该应用本身的开销。

## 2. 实验目标与实现思路

本任务单独放在 [lab2/kernel_task3](/root/os_experiments/lab2/kernel_task3) 中，重点展示内核侧时间记账，而不是 syscall 统计本身。为了让结果可对比，我选了 3 个 workload，并让每个 workload 采用：

- 1 次 warm-up
- 3 次正式测量

这样做有两个目的：

1. 首次执行常会带有镜像首次触发、QEMU 冷启动、串口输出首轮路径等一次性成本；
2. 验收关心的是“同一应用多次运行时，统计是否稳定”，因此最终汇总只统计 warm-up 之后的 3 次正式测量。

本实验的 3 个 workload 如下：

1. `io_burst`：I/O 密集，连续执行 24 次 `write("io\n")`；
2. `compute_spin`：计算密集，做 300000 轮整数运算，只在末尾 `exit`；
3. `info_probe`：系统调用密集，连续执行 20 次 `get_taskinfo`。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/kernel_task3/Cargo.toml)：Rust 裸机工程配置，包名为 `lab2_kernel_task3`。
- [.cargo/config.toml](/root/os_experiments/lab2/kernel_task3/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab2/kernel_task3/linker.ld)：镜像、内核栈、用户栈布局。
- [src/boot.S](/root/os_experiments/lab2/kernel_task3/src/boot.S)：启动入口、trap 保存现场与 `enter_user_mode`。
- [src/trap.rs](/root/os_experiments/lab2/kernel_task3/src/trap.rs)：trap 分发逻辑，识别 `ecall` 和 fault。
- [src/main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs)：`mtime` 读取、时间换算、运行记录、用户指针检查、最终报告。
- [src/syscall.rs](/root/os_experiments/lab2/kernel_task3/src/syscall.rs)：用户态 `write/get_taskinfo/exit` 封装。
- [src/console.rs](/root/os_experiments/lab2/kernel_task3/src/console.rs)：内核串口输出。
- [src/apps/io_burst.rs](/root/os_experiments/lab2/kernel_task3/src/apps/io_burst.rs)：I/O 密集 workload。
- [src/apps/compute_spin.rs](/root/os_experiments/lab2/kernel_task3/src/apps/compute_spin.rs)：计算密集 workload。
- [src/apps/info_probe.rs](/root/os_experiments/lab2/kernel_task3/src/apps/info_probe.rs)：高频 `get_taskinfo` workload。
- [artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/build_output.txt)：最近一次构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output.txt)：第一次完整运行输出。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output_repeat.txt)：第二次完整运行输出。
- [artifacts/qemu_timebase_probe.txt](/root/os_experiments/lab2/kernel_task3/artifacts/qemu_timebase_probe.txt)：QEMU `virt` 设备树中的 `clint` 与 `timebase-frequency` 证据。

## 4. 时间源与计时机制

### 4.1 使用的时间源

[src/main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs) 中定义了：

```rust
const CLINT_BASE: usize = 0x0200_0000;
const CLINT_MTIME_OFFSET: usize = 0xBFF8;
const MTIME_ADDR: usize = CLINT_BASE + CLINT_MTIME_OFFSET;
const MTIME_FREQ_HZ: u64 = 10_000_000;
const MTIME_TICK_NS: u64 = 1_000_000_000 / MTIME_FREQ_HZ;
```

内核通过：

```rust
fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile(MTIME_ADDR as *const u64) }
}
```

直接读取 `mtime` 硬件寄存器。

时间源确认来自 [artifacts/qemu_timebase_probe.txt](/root/os_experiments/lab2/kernel_task3/artifacts/qemu_timebase_probe.txt)：

- 设备树字符串中存在 `clint@2000000`、`sifive,clint0`、`timebase-frequency`
- DTB 偏移 `0x260` 处出现 `00 98 96 80`，即十六进制 `0x00989680 = 10,000,000`
- DTB 偏移 `0xe70` 一段能看到 `reg = <0x00000000 0x02000000 0x00000000 0x00010000>`，对应 `clint@0x02000000`

在 `sifive,clint0` 布局下，`mtime` 位于 `0xBFF8` 偏移，因此绝对地址为 `0x0200_BFF8`。  
由 `10,000,000 Hz` 可得：

- `1 tick = 1 / 10,000,000 s = 100 ns`
- `10 ticks = 1 us`
- `10,000 ticks = 1 ms`

### 4.2 单位换算

本实验同时保留：

- 原始 `mtime` 起止值
- `delta_ticks`
- `delta_us`
- 格式化后的 `ms`

核心换算公式是：

```rust
elapsed_ticks = end_mtime - start_mtime
elapsed_us = elapsed_ticks * 1_000_000 / MTIME_FREQ_HZ
```

日志中会直接输出：

```text
delta=31418 ticks = 3141 us = 3.141 ms
```

这样既能看到原始计数，也能直接对照微秒/毫秒结果。

### 4.3 计时区间如何排除内核加载开销

本实验没有文件系统和动态装载器，所有 workload 都在同一裸机镜像中静态链接。因此“加载开销”主要体现在：

- 内核为下一次运行做 bookkeeping
- 打印 launch 日志
- 设置 `CURRENT_RUN_INDEX`
- 准备用户栈与内核栈

我把 `start_mtime = read_mtime()` 放在 `launch_run()` 中最后一次 `println!` 之后、`enter_user_mode(...); mret` 之前：

```rust
println!("[kernel] launch app=...");
run.start_mtime = read_mtime();
enter_user_mode(...);
```

结束时则在用户态 `exit` 或 fault 刚 trap 回内核的第一时间读取 `end_mtime`：

```rust
run.end_mtime = read_mtime();
run.elapsed_ticks = run.end_mtime - run.start_mtime;
```

因此计时窗口覆盖的是“用户代码实际执行时间 + syscall/fault trap 往返时间”，而不包含：

- 内核前置打印与准备
- 后续结果汇总打印
- 整个测试框架的收尾逻辑

这满足了“排除内核加载该应用本身开销”的验收要求。

### 4.4 为什么加入 warm-up

实际运行表明每个 workload 的首轮常明显慢于后续轮次，典型原因包括：

- QEMU 首次触发路径带来的冷启动效应
- 指令和数据第一次被触碰时的额外开销
- 串口 I/O 的首轮初始化路径

因此每个应用先跑 1 次 warm-up，只把后续 3 次计入最终统计。  
这样既保留了真实首轮现象，又能让“同一应用多次运行的波动范围”更有解释力。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/kernel_task3
```

构建：

```bash
cargo build
```

运行一次并保存输出：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task3 > artifacts/run_output.txt
```

再次运行做复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task3 > artifacts/run_output_repeat.txt
```

导出 QEMU `virt` 设备树并保存时间源证据：

```bash
qemu-system-riscv64 -machine virt,dumpdtb=/tmp/kernel_task3_qemu_virt.dtb -display none -S
strings -a /tmp/kernel_task3_qemu_virt.dtb | rg 'clint|timebase-frequency'
hexdump -C /tmp/kernel_task3_qemu_virt.dtb
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/qemu_timebase_probe.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 6.2 第一次运行摘要

以下摘要来自 [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output.txt)：

```text
[kernel] time source: CLINT mtime @ 0x200bff8, timebase-frequency=10000000 Hz, 1 tick=100 ns
[kernel] each app runs 1 warm-up round plus 3 measured rounds; summary excludes warm-up
[kernel] summary io_burst: measured_runs=3 ok=3 min=204 us (0.204 ms) max=208 us (0.208 ms) avg=2063 ticks / 206 us (0.206 ms) spread=4 us (01.94%)
[kernel] summary compute_spin: measured_runs=3 ok=3 min=3123 us (3.123 ms) max=3345 us (3.345 ms) avg=32036 ticks / 3203 us (3.203 ms) spread=222 us (06.93%)
[kernel] summary info_probe: measured_runs=3 ok=3 min=34 us (0.034 ms) max=40 us (0.040 ms) avg=372 ticks / 36 us (0.036 ms) spread=6 us (16.66%)
[kernel] check all runs exited successfully: PASS
[kernel] check every run captured a non-zero mtime delta: PASS
[kernel] check each app was measured 3 times after warm-up: PASS
[kernel] check compute_spin stays slowest on average: PASS
```

### 6.3 第二次运行摘要

以下摘要来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output_repeat.txt)：

```text
[kernel] summary io_burst: measured_runs=3 ok=3 min=209 us (0.209 ms) max=226 us (0.226 ms) avg=2157 ticks / 215 us (0.215 ms) spread=17 us (07.90%)
[kernel] summary compute_spin: measured_runs=3 ok=3 min=3122 us (3.122 ms) max=3198 us (3.198 ms) avg=31552 ticks / 3154 us (3.154 ms) spread=76 us (02.40%)
[kernel] summary info_probe: measured_runs=3 ok=3 min=36 us (0.036 ms) max=40 us (0.040 ms) avg=385 ticks / 38 us (0.038 ms) spread=4 us (10.52%)
[kernel] check all runs exited successfully: PASS
[kernel] check every run captured a non-zero mtime delta: PASS
[kernel] check each app was measured 3 times after warm-up: PASS
[kernel] check compute_spin stays slowest on average: PASS
```

## 7. 对比分析

### 7.1 同一应用的多次运行波动

| 应用 | 第一次运行平均值 | 第二次运行平均值 | 跨次运行差异 | 结论 |
| --- | --- | --- | --- | --- |
| `io_burst` | `206 us` | `215 us` | `9 us`，约 `4.37%` | 波动较小 |
| `compute_spin` | `3203 us` | `3154 us` | `49 us`，约 `1.53%` | 非常稳定 |
| `info_probe` | `36 us` | `38 us` | `2 us`，约 `5.56%` | 波动较小 |

### 7.2 同一轮内部的离散程度

| 应用 | 第一次运行 spread | 第二次运行 spread | 解释 |
| --- | --- | --- | --- |
| `io_burst` | `4 us (1.94%)` | `17 us (7.90%)` | 串口 I/O 对宿主调度有轻微敏感性，但仍在很小范围内 |
| `compute_spin` | `222 us (6.93%)` | `76 us (2.40%)` | 纯计算最稳定，第一次运行中出现一次轻微宿主机调度抖动 |
| `info_probe` | `6 us (16.66%)` | `4 us (10.52%)` | 绝对值极小，百分比看起来偏大但实际只差几个微秒 |

对 `info_probe` 来说，绝对时间只有几十微秒，因此即便相差 `4~6 us`，换算成百分比也会显得较大；这属于“基数很小导致百分比放大”，不代表计时失真。

### 7.3 不同 workload 的相对关系

两次完整运行都满足：

- `compute_spin` 平均耗时最高，说明计算密集型 workload 明显更慢；
- `io_burst` 次之，说明串口输出开销可见但远小于长时间计算；
- `info_probe` 最短，说明仅做少量结构化 syscall 往返时耗时很低。

这与 workload 的设计特征一致。

## 8. 验收检查对应关系

1. 使用 `mtime` 等硬件寄存器计时，且单位换算正确：
   - [src/main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs) 直接读取 `0x0200bff8`；
   - [artifacts/qemu_timebase_probe.txt](/root/os_experiments/lab2/kernel_task3/artifacts/qemu_timebase_probe.txt) 给出 `timebase-frequency = 10,000,000 Hz` 的本机证据；
   - 日志同时打印原始 `ticks`、`us` 与 `ms`。
2. 计时区间合理，排除了内核加载开销：
   - `start_mtime` 放在 `launch` 打印之后、`mret` 之前；
   - `end_mtime` 放在 trap 回内核后的第一时间；
   - 最终报告显式说明 warm-up 与汇总统计分离。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境完成，使用：
  - `rustc 1.94.1`
  - `cargo 1.94.1`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器复现。
- 本实验运行在教学化最小裸机环境中，没有分页、动态装载器和抢占式调度；
- 因此这里的“排除应用加载开销”是指：排除内核为切换到该 workload 所做的前置 bookkeeping 和日志输出，而不是完整现代 OS 中的 ELF 装载成本。
