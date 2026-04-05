# LAB3 用户态 Task1：浮点抢占验证程序

## 1. 原始任务说明

### 任务标题

浮点抢占验证程序

### 任务目标

理解上下文切换对浮点寄存器状态保存/恢复的要求，验证内核对浮点上下文的支持。

### 任务要求

1. 编写浮点密集型程序，持续执行浮点运算并输出校验值；
2. 通过并发运行或与其他任务交替运行，触发抢占与切换；
3. 若浮点上下文保存不正确，校验值应出现异常；需给出判定依据。

### 验收检查

1. 程序运行时强制触发了中断/抢占；
2. 并发运行该程序，最终输出的浮点计算结果/校验和完全正确。

## 2. 实验目标与实现思路

本实验在 [lab3/task1](/root/os_experiments/lab3/task1) 中实现了一个最小可运行的 RISC-V 裸机内核加两个 U-mode 用户任务，运行环境是 QEMU `virt` 机器中的 guest，而不是宿主 Linux 进程。

实现思路分两步：

1. 先分别单独运行 `fp_alpha` 和 `fp_beta`，得到同一份二进制、同一台 QEMU 机器上的“参考校验值”。
2. 再启动定时器中断和轮转调度，让两个任务在浮点循环中被强制抢占；如果 trap 入口没有正确保存/恢复 `f0-f31` 与 `fcsr`，那么并发运行的最终 64 位校验值就会与参考值逐位不一致。

这里没有用“近似相等”判定，而是直接做“按位相等”比较。原因是：

- 浮点工作负载是确定性的同一串递推；
- 参考值与并发值都来自同一个 QEMU guest 中的同一份程序；
- 参考阶段和并发阶段唯一关键差异就是“是否发生抢占和上下文切换”。

因此，只要并发阶段出现校验失配，就可以直接把它作为“浮点上下文泄漏/损坏”的判据。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/task1/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/task1/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/task1/linker.ld)：镜像布局、内核栈与两个用户栈。
- [src/boot.S](/root/os_experiments/lab3/task1/src/boot.S)：启动入口、`enter_task`、trap 汇编入口、浮点压力循环 `fp_stress_loop`。
- [src/trap.rs](/root/os_experiments/lab3/task1/src/trap.rs)：`TrapFrame` 定义与 trap 分发入口。
- [src/main.rs](/root/os_experiments/lab3/task1/src/main.rs)：调度器、`mtime/mtimecmp` 定时器、中断处理、用户任务构造、最终验收输出。
- [src/syscall.rs](/root/os_experiments/lab3/task1/src/syscall.rs)：用户态 `write/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab3/task1/src/console.rs)：内核 UART 输出。
- [src/user_console.rs](/root/os_experiments/lab3/task1/src/user_console.rs)：用户态格式化输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/task1/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/task1/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/task1/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/trap_fp_context_objdump.txt](/root/os_experiments/lab3/task1/artifacts/trap_fp_context_objdump.txt)：trap 和浮点工作负载的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/task1/artifacts/tool_versions.txt)：当前工具链版本。

关键实现点：

1. `TrapFrame` 扩展到 528 字节，除了通用寄存器，还包含 `f0-f31`、`fcsr` 与 `mepc/user_sp`。
2. `trap_entry` 在每次 trap 时先保存整数寄存器，再保存全部浮点寄存器和 `fcsr`，返回前完整恢复。
3. 并发阶段打开 `mie.MTIE` 并持续重编程 `mtimecmp`，时间片固定为 `2500 ticks = 250 us`。
4. 用户态浮点负载 `fp_stress_loop` 长时间把状态留在浮点寄存器里，强制让抢占点落在真实的 FPU 活跃区间。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/task1
```

构建：

```bash
cargo build
```

运行一次并保存日志：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_task1 > artifacts/run_output.txt
```

再次运行做复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_task1 > artifacts/run_output_repeat.txt
```

导出 trap 与浮点上下文保存/恢复的反汇编证据：

```bash
cargo objdump --bin lab3_task1 -- --demangle -d | sed -n '/<enter_task>:/,/^$/p;/<trap_entry>:/,/^$/p;/<fp_stress_loop>:/,/^$/p' > artifacts/trap_fp_context_objdump.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/trap_fp_context_objdump.txt
cat artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/task1/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 5.2 第一次运行摘要

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/task1/artifacts/run_output.txt)：

```text
[kernel] phase=reference task=fp_alpha iterations=220000
[user/fp_alpha] reference checksum=0x7ffec4df42aade3f
[kernel] reference checksum [fp_alpha] = 0x7ffec4df42aade3f
[kernel] phase=reference task=fp_beta iterations=220000
[user/fp_beta] reference checksum=0x801936f547486022
[kernel] reference checksum [fp_beta] = 0x801936f547486022
[kernel] phase=preemptive starting concurrent run
[kernel] expected checksum [fp_alpha] = 0x7ffec4df42aade3f
[kernel] expected checksum [fp_beta] = 0x801936f547486022
[kernel] timer interrupt #1: preempt fp_alpha -> fp_beta at mepc=0x80000678
[kernel] timer interrupt #2: preempt fp_beta -> fp_alpha at mepc=0x80002462
[kernel] timer interrupt #3: preempt fp_alpha -> fp_beta at mepc=0x80000678
[kernel] timer interrupt logging capped after 6 events
[kernel] concurrent checksum [fp_alpha] = 0x7ffec4df42aade3f
[kernel] concurrent checksum [fp_beta] = 0x801936f547486022
[kernel] summary: timer_interrupts=82 forced_switches=82
[kernel] result [fp_alpha]: expected=0x7ffec4df42aade3f observed=0x7ffec4df42aade3f => PASS
[kernel] result [fp_beta]: expected=0x801936f547486022 observed=0x801936f547486022 => PASS
[kernel] acceptance forced timer interrupt occurred: PASS
[kernel] acceptance forced context switch occurred: PASS
[kernel] acceptance concurrent checksums match reference exactly: PASS
```

可以看到：

1. 参考阶段先得到两个任务的基准校验值；
2. 并发阶段真实发生了 82 次定时器中断和 82 次强制切换；
3. 两个任务的并发校验值与参考值逐位一致。

### 5.3 第二次运行摘要

以下关键内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/task1/artifacts/run_output_repeat.txt)：

```text
[kernel] phase=preemptive starting concurrent run
[kernel] expected checksum [fp_alpha] = 0x7ffec4df42aade3f
[kernel] expected checksum [fp_beta] = 0x801936f547486022
[kernel] timer interrupt #1: preempt fp_alpha -> fp_beta at mepc=0x80000678
[kernel] timer interrupt #2: preempt fp_beta -> fp_alpha at mepc=0x80002462
[kernel] timer interrupt #3: preempt fp_alpha -> fp_beta at mepc=0x80000678
[kernel] timer interrupt logging capped after 6 events
[kernel] concurrent checksum [fp_beta] = 0x801936f547486022
[kernel] concurrent checksum [fp_alpha] = 0x7ffec4df42aade3f
[kernel] summary: timer_interrupts=83 forced_switches=83
[kernel] result [fp_alpha]: expected=0x7ffec4df42aade3f observed=0x7ffec4df42aade3f => PASS
[kernel] result [fp_beta]: expected=0x801936f547486022 observed=0x801936f547486022 => PASS
[kernel] acceptance forced timer interrupt occurred: PASS
[kernel] acceptance forced context switch occurred: PASS
[kernel] acceptance concurrent checksums match reference exactly: PASS
```

第二次运行得到 83 次定时器中断和 83 次强制切换，结论与第一次一致，说明结果稳定。

### 5.4 反汇编证据

[artifacts/trap_fp_context_objdump.txt](/root/os_experiments/lab3/task1/artifacts/trap_fp_context_objdump.txt) 中可以直接看到：

```text
00000000800004c0 <trap_entry>:
80000510: a202          fsd ft0, 0x100(sp)
80000520: a2a2          fsd fs0, 0x140(sp)
80000534: ab4a          fsd fs2, 0x190(sp)
80000550: 003022f3      frcsr t0
...
80000570: 00329073      fscsr t0
8000057a: 2012          fld ft0, 0x100(sp)
8000058a: 2416          fld fs0, 0x140(sp)
8000059e: 295a          fld fs2, 0x190(sp)
```

这说明 trap 入口确实在保存/恢复浮点寄存器和 `fcsr`。

同一个反汇编文件里还能看到浮点压力循环：

```text
0000000080000610 <fp_stress_loop>:
80000622: 00053007      fld ft0, 0x0(a0)
80000656: 00063907      fld fs2, 0x0(a2)
80000678: 92a07043      fmadd.d ft0, ft0, fa0, fs2
8000067c: 9ab0f0c3      fmadd.d ft1, ft1, fa1, fs3
...
8000069a: fc029fe3      bnez t0, 0x80000678 <fp_stress_loop+0x68>
```

这说明用户程序在长循环中持续执行浮点指令，满足“浮点密集型程序”的要求。

## 6. 机制解释

### 6.1 抢占是如何被强制触发的

1. 内核通过 `mtime` 读取当前时间，通过 `mtimecmp` 设定下一次定时器中断。
2. 并发阶段开启 `mie.MTIE`，每个时间片重新把 `mtimecmp` 设为 `read_mtime() + 2500`。
3. 当 U-mode 正在执行 `fp_stress_loop` 时，硬件异步触发 Machine Timer Interrupt，进入 `trap_entry`。
4. 内核把当前任务上下文存入 `TASKS[current].frame`，再把另一个可运行任务的上下文覆盖到当前 trap frame，`mret` 后直接切到下一个用户任务。

所以这里不是 cooperative yield，而是真正的“时钟中断驱动的抢占式切换”。

### 6.2 为什么必须保存浮点寄存器

`fp_stress_loop` 在循环体里把 8 路浮点状态放在 `ft0-ft7`，把递推常量放在 `fa0-fa7` 和 `fs2-fs9`，并持续做 `fmadd.d`。  
如果在一个任务被中断后，另一个任务复用了这些寄存器而内核又没有保存/恢复原值，那么前一个任务恢复执行时看到的就不再是自己的浮点状态，最终校验值必然偏离参考值。

### 6.3 判定依据为什么可靠

本实验的判定基于“同一任务两次运行的结果必须逐位相等”：

1. 参考阶段：单任务运行，不发生任务间浮点寄存器竞争；
2. 并发阶段：唯一显著变化是启用了定时器抢占和任务切换；
3. 验证方式：对比 64 位整数化后的校验值，要求 `observed == expected`。

因此：

- 如果浮点上下文保存/恢复正确，则并发结果与参考值完全一致；
- 如果浮点上下文不正确，则至少一个任务会继承到别的任务的 FPU 状态，最终 `observed != expected`，立刻判定失败。

### 6.4 为什么日志里会出现少量文本交错

`uprintln!` 底层走 `write` syscall，而格式化输出可能拆成多次 `write_str`。在并发阶段，两个任务都可能在打印期间再次被抢占，所以 UART 文本偶尔会交错。这是“串口输出非原子”的表现，不是浮点校验失败。最终验收以内核记录并汇总的校验结果为准。

## 7. 验收检查对应关系

1. 程序运行时强制触发了中断/抢占：
   - [artifacts/run_output.txt](/root/os_experiments/lab3/task1/artifacts/run_output.txt) 中有 `timer interrupt #1/#2/#3...`；
   - 第一次运行 `timer_interrupts=82 forced_switches=82`；
   - 第二次运行 `timer_interrupts=83 forced_switches=83`。
2. 并发运行最终结果完全正确：
   - `fp_alpha`：`0x7ffec4df42aade3f == 0x7ffec4df42aade3f`
   - `fp_beta`：`0x801936f547486022 == 0x801936f547486022`
   - 两次运行都打印 `acceptance concurrent checksums match reference exactly: PASS`。
3. 给出了“若保存不正确则会异常”的判定依据：
   - 参考阶段先产生基准校验值；
   - 并发阶段要求逐位匹配；
   - 任一任务失配都直接说明浮点寄存器或 `fcsr` 在上下文切换中泄漏或损坏。

## 8. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 用户进程。
- 工具链版本见 [artifacts/tool_versions.txt](/root/os_experiments/lab3/task1/artifacts/tool_versions.txt)：
  - `rustc 1.94.1 (e408947bf 2026-03-25)`
  - `cargo 1.94.1 (29ea6fb6a 2026-03-24)`
  - `riscv64gc-unknown-none-elf (installed)`
  - `QEMU emulator version 10.0.8`
- 本回合没有在第二台原生 Linux 服务器上再次复现。
- 这是教学化的最小裸机模型，没有分页和真实用户地址空间隔离；但对“定时器抢占 + FPU 上下文保存/恢复”这一实验目标已经足够。
