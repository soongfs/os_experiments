# LAB3 用户态 Task2：任务切换开销估算

## 1. 原始任务说明

### 任务标题

任务切换开销估算

### 任务目标

建立可重复的测量方法，量化任务切换带来的时间开销。

### 任务要求

1. 编写高频触发切换的程序（例如反复系统调用或让出 CPU）；
2. 统计一定次数下的总耗时，并估算单次切换平均开销；
3. 在实验记录中说明：测量误差来源（计时粒度、缓存效应、调度抖动等）。

### 验收检查

1. 给出明确的单次任务切换耗时估算值（如微秒级）；
2. 报告中对误差来源的分析具有技术合理性。

## 2. 实验目标与实现思路

本实验在 [lab3/task2](/root/os_experiments/lab3/task2) 中实现了一个最小 RISC-V 裸机 guest 内核和两个 U-mode 用户任务，运行环境是 QEMU `virt` 机器里的 guest，而不是宿主 Linux 进程。

测量思路不是直接拿“yield 总时间 / yield 次数”，而是先减去系统调用本身的固定成本：

1. `baseline` 阶段：
   - 两个任务顺序运行；
   - 每个任务执行 `25000` 次 `SYS_NOOP`；
   - 总共 `50000` 次高频 trap，但不发生“每次操作都切到另一个任务”的 ping-pong。
2. `yield` 阶段：
   - 两个任务并发参与；
   - 每个任务执行 `25000` 次 `SYS_YIELD`；
   - 总共 `50000` 次高频 trap，并且每次 `yield` 都切到另一个任务，总开销包含真实任务切换。

核心估算公式是：

```text
单次任务切换开销 ≈ (yield_phase_total - baseline_phase_total) / actual_switches
```

也就是先用 `baseline` 抵消 trap/syscall 的固定进入和返回成本，再把“多出来的时间”除以实际切换次数。

为了让结果更稳，本实验采用：

- `1` 次 warm-up round
- `5` 次 measured rounds

最终同时报告：

- 每轮的单次切换估算值
- `5` 轮测量结果的中位数 `median`
- 算术平均值 `mean`
- 最小/最大值

其中中位数作为主估算值，因为它对单轮调度抖动更稳健。

另外，本实验复用了 LAB3 task1 的完整 trap frame 和 eager 浮点上下文保存/恢复路径，所以这里得到的是“当前实验内核实现”的任务切换开销；即使用户态任务本身不做浮点运算，内核仍会沿当前实现保存/恢复完整上下文。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/task2/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/task2/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/task2/linker.ld)：镜像布局、内核栈与两个用户栈。
- [src/boot.S](/root/os_experiments/lab3/task2/src/boot.S)：启动入口、`enter_task` 与 trap 汇编入口。
- [src/trap.rs](/root/os_experiments/lab3/task2/src/trap.rs)：`TrapFrame` 定义和 trap 到 Rust 的桥接。
- [src/main.rs](/root/os_experiments/lab3/task2/src/main.rs)：round/phase 状态机、`mtime` 计时、`yield` 调度、统计汇总与验收输出。
- [src/syscall.rs](/root/os_experiments/lab3/task2/src/syscall.rs)：用户态 `noop/yield/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab3/task2/src/console.rs)：内核 UART 输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/task2/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/task2/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/task2/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/context_switch_objdump.txt](/root/os_experiments/lab3/task2/artifacts/context_switch_objdump.txt)：`ecall`、trap 入口和 `handle_yield` 的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/task2/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/task2
```

构建：

```bash
cargo build
```

运行一次并保存输出：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_task2 > artifacts/run_output.txt
```

再次运行做复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_task2 > artifacts/run_output_repeat.txt
```

导出 `ecall`/trap/切换路径反汇编：

```bash
cargo objdump --bin lab3_task2 -- --demangle -d | rg -n -C 5 "enter_task|trap_entry|lab3_task2::syscall::invoke_syscall3|lab3_task2::handle_yield|ecall" > artifacts/context_switch_objdump.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/context_switch_objdump.txt
cat artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/task2/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 5.2 第一次完整运行结果

来自 [artifacts/run_output.txt](/root/os_experiments/lab3/task2/artifacts/run_output.txt) 的关键输出：

```text
[kernel] round#1: baseline=47833 us, yield=104308 us, extra=56474 us, switches=50000, switch_estimate=1129 ns (1.129 us)
[kernel] round#2: baseline=51040 us, yield=105872 us, extra=54832 us, switches=50000, switch_estimate=1096 ns (1.096 us)
[kernel] round#3: baseline=47412 us, yield=108461 us, extra=61049 us, switches=50000, switch_estimate=1220 ns (1.220 us)
[kernel] round#4: baseline=48366 us, yield=103546 us, extra=55180 us, switches=50000, switch_estimate=1103 ns (1.103 us)
[kernel] round#5: baseline=47189 us, yield=110435 us, extra=63246 us, switches=50000, switch_estimate=1264 ns (1.264 us)
[kernel] average baseline=48368 us, average yield=106524 us, average extra=58156 us
[kernel] robust median single task-switch overhead = 1129 ns (1.129 us)
[kernel] arithmetic mean switch overhead = 1163 ns (1.163 us), min=1096 ns, max=1264 ns
[kernel] acceptance explicit per-switch estimate available: PASS
[kernel] acceptance operation counts and switch counts are consistent: PASS
[kernel] acceptance measured extra cost stayed positive in every measured round: PASS
```

这次运行给出的主估算值是：

```text
单次任务切换开销 ≈ 1129 ns = 1.129 us
```

### 5.3 第二次完整运行结果

来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/task2/artifacts/run_output_repeat.txt) 的关键输出：

```text
[kernel] round#1: baseline=47989 us, yield=104241 us, extra=56251 us, switches=50000, switch_estimate=1125 ns (1.125 us)
[kernel] round#2: baseline=49616 us, yield=107807 us, extra=58191 us, switches=50000, switch_estimate=1163 ns (1.163 us)
[kernel] round#3: baseline=46948 us, yield=108831 us, extra=61883 us, switches=50000, switch_estimate=1237 ns (1.237 us)
[kernel] round#4: baseline=48710 us, yield=105893 us, extra=57183 us, switches=50000, switch_estimate=1143 ns (1.143 us)
[kernel] round#5: baseline=49844 us, yield=110000 us, extra=60156 us, switches=50000, switch_estimate=1203 ns (1.203 us)
[kernel] average baseline=48621 us, average yield=107355 us, average extra=58733 us
[kernel] robust median single task-switch overhead = 1163 ns (1.163 us)
[kernel] arithmetic mean switch overhead = 1174 ns (1.174 us), min=1125 ns, max=1237 ns
[kernel] acceptance explicit per-switch estimate available: PASS
[kernel] acceptance operation counts and switch counts are consistent: PASS
[kernel] acceptance measured extra cost stayed positive in every measured round: PASS
```

第二次复验给出的主估算值是：

```text
单次任务切换开销 ≈ 1163 ns = 1.163 us
```

综合两次完整运行，可以把当前环境下的单次任务切换开销概括为：

```text
约 1.15 us
```

### 5.4 `ecall` 与切换路径反汇编证据

[artifacts/context_switch_objdump.txt](/root/os_experiments/lab3/task2/artifacts/context_switch_objdump.txt) 中可直接看到：

```text
00000000800031a6 <lab3_task2::syscall::invoke_syscall3::...>:
800031c6: 00000073      ecall

0000000080000aa0 <trap_entry>:
80000aa0: 34011173      csrrw sp, mscratch, sp
80000aa4: df010113      addi  sp, sp, -0x210

00000000800017ee <lab3_task2::handle_yield::...>:
...
80001cf8: afa080e7      jalr  -0x506(ra) <lab3_task2::handle_yield::...>
```

这说明：

1. 用户态高频触发路径确实通过 `ecall` 进入内核；
2. trap 入口保存上下文；
3. `SYS_YIELD` 进入 `handle_yield()`，执行真实的任务切换逻辑。

## 6. 机制解释

### 6.1 为什么要做 `baseline`

如果直接用 `yield_total / switches`，结果会把下面两类成本混在一起：

- trap 进入和返回本身的固定成本
- 真实任务切换的附加成本

所以本实验先测 `baseline`：

- 同样是 `50000` 次高频系统调用；
- 但系统调用返回给当前任务，不在每次调用后切到另一个任务。

然后用 `yield_phase_total - baseline_phase_total` 做差分，尽量把固定 trap 成本扣掉。

### 6.2 `yield` 阶段如何形成高频切换

两个用户任务都执行：

```text
for _ in 0..25000 {
    sys_yield();
}
```

内核在 `handle_yield()` 中：

1. 把当前 trap frame 保存到 `TASKS[current].frame`；
2. 选出另一个 `Runnable` 任务；
3. 把该任务保存的 frame 覆盖回当前 trap frame；
4. `mret` 返回后，CPU 继续执行另一个任务。

由于两个任务的 `yield` 次数相同，实测每轮都得到了：

```text
hot_ops = 50000
switches = 50000
```

这说明每次高频 `yield` 都对应了一次真实任务切换。

### 6.3 为什么结果里报告中位数而不是只报告均值

同一轮内部仍会出现少量抖动，例如第一次运行的 5 个 measured rounds 中，单次切换估算值分布在：

```text
1.096 us ~ 1.264 us
```

均值会被波动更大的轮次拉动，而中位数对单轮异常值更稳。因此本实验把 `median` 作为主结果，把 `mean/min/max` 作为波动范围辅助信息。

## 7. 误差来源分析

### 7.1 计时粒度

本实验用 `mtime` 计时，QEMU `virt` 的 `timebase-frequency = 10,000,000 Hz`，即：

```text
1 tick = 100 ns
```

因此任何单次相位的起止时间都只能量化到 `100 ns` 粒度。  
不过每个 phase 都累计了 `50000` 次高频操作，量化误差被大样本平均后已经很小。

### 7.2 warm-up、缓存和翻译块效应

QEMU TCG 首次执行某段代码时会建立翻译块，内核和用户代码本身也会经历指令/数据路径热身。所以实验显式加入了 `1` 次 warm-up round，并且最终统计只看后面 `5` 次 measured rounds。

### 7.3 调度抖动

虽然 guest 内部的测量逻辑固定，但 QEMU 本身仍运行在宿主 Linux 上，可能受到：

- 宿主调度器抢占
- 其他进程占用 CPU
- TCG 执行节拍波动

这些因素会让某些 measured round 明显偏高。  
这也是本实验选择报告中位数，并保留 `min/max` 范围的原因。

### 7.4 差分模型不是“绝对纯净”的下界

`baseline` 已经尽量抵消了 trap 固定成本，但它仍不等于“零切换”：

- `baseline` 中两个任务是顺序完成的，阶段内仍存在一次从 task0 切到 task1 的收尾 handoff；
- `yield` 阶段和 `baseline` 阶段的控制流、分支预测、缓存占用并不完全相同。

所以这里得到的是“当前实现下的工程估算值”，而不是某个理想化的理论下界。

### 7.5 当前实现包含完整上下文路径

本实验的 trap frame 延续了 LAB3 task1 的完整实现，包含浮点寄存器与 `fcsr` 的保存/恢复路径。  
即使 task2 的用户态工作负载只做整数 `ecall`，估算出的切换开销仍反映了“当前内核 eager-save 完整上下文”的成本，而不是“只保存整数寄存器时的最小成本”。

## 8. 验收检查对应关系

1. 给出明确的单次任务切换耗时估算值：
   - 第一次完整运行：`1129 ns = 1.129 us`
   - 第二次完整运行：`1163 ns = 1.163 us`
   - 综述：当前环境下约 `1.15 us`
2. 误差来源分析具有技术合理性：
   - 已解释 `mtime` 粒度；
   - 已解释 warm-up/缓存/TCG 翻译块效应；
   - 已解释宿主调度抖动；
   - 已解释差分模型与完整上下文实现带来的系统性偏差。

## 9. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 用户进程。
- 版本信息见 [artifacts/tool_versions.txt](/root/os_experiments/lab3/task2/artifacts/tool_versions.txt)：
  - `rustc 1.94.1 (e408947bf 2026-03-25)`
  - `cargo 1.94.1 (29ea6fb6a 2026-03-24)`
  - `riscv64gc-unknown-none-elf (installed)`
  - `QEMU emulator version 10.0.8`
- 本回合没有在第二台原生 Linux 服务器上再次复现。
- 由于 QEMU TCG 和宿主调度器共同参与，文中的绝对时间值不应直接视为真实硬件上的最终结论；但对于“建立可重复测量方法并得到当前实验环境下的开销量级”这一目标，结果已经足够。
