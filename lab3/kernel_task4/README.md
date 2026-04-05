# LAB3 内核态 Task4：内核态中断响应

## 1. 原始任务说明

### 任务标题

内核态中断响应

### 任务目标

掌握中断响应流程，在内核态可重入与不可重入区间之间做正确处理。

### 任务要求

1. 支持在内核态响应外设或时钟中断；
2. 明确中断屏蔽/开中断的策略，避免破坏临界区；
3. 输出必要诊断信息（中断号/来源/处理路径）。

### 验收检查

1. `sstatus.SIE` 位在安全区间被打开；
2. 内核空间发生时钟中断时不会导致系统崩溃；
3. 锁与临界区使用了关中断保护。

## 2. 实验目标与实现思路

本实验在 [lab3/kernel_task4](/root/os_experiments/lab3/kernel_task4) 中实现了一个最小 S-mode 内核。内核本身持续在 S-mode 里运行，不切到用户态；实验目标是验证“当 CPU 正在执行内核代码时，时钟事件到来后系统仍能安全响应”，并且临界区不会被中断破坏。

实现采用两级路径：

1. `mtime/mtimecmp` 仍由 M-mode 管理，Machine Timer Interrupt (`MTIP`) 到来时先进入 `machine_trap_entry`；
2. M-mode handler 重编程下一次 `mtimecmp`，再把时钟事件转发成一个可由 S-mode 清除 pending 的 `SSIP`；
3. 因为 `mideleg` 把 `SSIP` 委托给了 S-mode，且 S-mode 打开了 `sie.SSIE`，所以当 `sstatus.SIE=1` 时，S-mode 内核会在安全区间收到中断；
4. S-mode handler 输出 `scause`、`sepc` 和处理路径日志，并通过一个带关中断保护的 `InterruptMutex` 更新共享状态。

这里之所以没有直接把时钟事件交给 `STIP`，是因为当前这个 bare-metal 实验环境没有引入 SBI `set_timer` 或 `Sstc stimecmp` 支持。  
因此实验采用的是：

```text
mtime/MTIP -> M-mode forwarder -> delegated SSIP -> S-mode kernel handler
```

时钟源依然是 `mtime`，只是 S-mode 的可清 pending 中断类型选成了 `SSIP`，这样更适合在最小裸机场景里稳定复现“内核态时钟响应 + 关中断临界区”。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/kernel_task4/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/kernel_task4/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/kernel_task4/linker.ld)：镜像布局、S-mode 内核栈、S-mode trap 栈和 M-mode trap 栈。
- [src/boot.S](/root/os_experiments/lab3/kernel_task4/src/boot.S)：`enter_supervisor`、`machine_trap_entry` 和 `supervisor_trap_entry`。
- [src/trap.rs](/root/os_experiments/lab3/kernel_task4/src/trap.rs)：通用 `TrapFrame` 以及 `mtvec/stvec` 初始化。
- [src/main.rs](/root/os_experiments/lab3/kernel_task4/src/main.rs)：M-mode timer forwarder、S-mode 中断处理、`InterruptGuard`、`InterruptMutex`、safe interval/critical section 实验循环和最终验收输出。
- [src/console.rs](/root/os_experiments/lab3/kernel_task4/src/console.rs)：内核 UART 输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task4/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task4/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task4/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/interrupt_objdump.txt](/root/os_experiments/lab3/kernel_task4/artifacts/interrupt_objdump.txt)：`enter_supervisor`、machine/supervisor trap entry、`csrs/csrc sstatus`、`csrs mie/sie`、`csrs mip/csrc sip` 等反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/kernel_task4/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/kernel_task4
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
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task4 > artifacts/run_output.txt
```

第二次运行：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task4 > artifacts/run_output_repeat.txt
```

导出中断相关反汇编：

```bash
cargo objdump --bin lab3_kernel_task4 -- --demangle -d | rg -n -C 4 "enable_supervisor_interrupts|disable_supervisor_interrupts|enable_machine_timer_interrupt|enable_supervisor_timer_source|set_supervisor_timer_pending|clear_supervisor_timer_pending|enter_supervisor|machine_trap_entry|supervisor_trap_entry|handle_machine_trap|handle_supervisor_trap|csrs\\s+sstatus|csrc\\s+sstatus|csrs\\s+mie|csrs\\s+sie|csrs\\s+mip|csrc\\s+sip" > artifacts/interrupt_objdump.txt
```

记录工具链：

```bash
{ printf 'rustc: '; rustc --version; printf 'cargo: '; cargo --version; printf 'targets:\n'; rustup target list | grep riscv64gc; printf 'qemu: '; qemu-system-riscv64 --version | head -n 1; } > artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task4/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab3_kernel_task4 v0.1.0 (/root/os_experiments/lab3/kernel_task4)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.22s
```

### 5.2 第一次运行结果

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task4/artifacts/run_output.txt)：

```text
[kernel] policy: sstatus.SIE=1 in safe intervals, sstatus.SIE=0 while holding interrupt mutex
[kernel] delivery path: machine_timer_forwarder(MTIP) -> delegated supervisor_software(SSIP) -> S-mode handler
[kernel] safe interval #1: enabling sstatus.SIE for forwarded_timer_irq
[kernel] irq#1 source=forwarded_timer(origin=mtime, delivered=ssip) scause=0x8000000000000001 sepc=0x800015ee path=machine_timer_forwarder->supervisor_trap->timer_handler
[kernel] critical section #1 enter: sstatus.SIE=0 lock=held
[kernel] critical section #1 hold: sstatus.SIE=0 pending_forwarded_timer_irq=1
...
[kernel] summary: machine_timer_forwards=12 supervisor_timer_irqs=6 critical_sections=5 lock_acquisitions=12 pending_during_critical=5 critical_updates=32655
[kernel] diagnostics: last_mcause=0x8000000000000007 last_scause=0x8000000000000001 last_sepc=0x8000196a
[kernel] acceptance sstatus.SIE opened in safe interval: PASS
[kernel] acceptance kernel-space timer interrupt handled without crash: PASS
[kernel] acceptance locks and critical sections used interrupt-off protection: PASS
```

从第一次运行可以直接读出：

1. S-mode safe interval 中明确执行了“打开 `sstatus.SIE` 等待中断”；
2. 中断日志包含了：
   - 中断号 `irq#N`
   - 来源 `origin=mtime`
   - 实际 S-mode 交付类型 `delivered=ssip`
   - `scause`
   - `sepc`
   - 处理路径 `machine_timer_forwarder->supervisor_trap->timer_handler`
3. 临界区内部 `sstatus.SIE=0`，而且 `pending_forwarded_timer_irq=1`，说明时钟事件在关中断期间被延后，没有闯入临界区；
4. 最终所有验收项都是 `PASS`。

### 5.3 第二次运行结果

以下内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task4/artifacts/run_output_repeat.txt)：

```text
[kernel] summary: machine_timer_forwards=14 supervisor_timer_irqs=6 critical_sections=6 lock_acquisitions=13 pending_during_critical=6 critical_updates=32502
[kernel] diagnostics: last_mcause=0x8000000000000007 last_scause=0x8000000000000001 last_sepc=0x8000196a
[kernel] acceptance sstatus.SIE opened in safe interval: PASS
[kernel] acceptance kernel-space timer interrupt handled without crash: PASS
[kernel] acceptance locks and critical sections used interrupt-off protection: PASS
```

第二次运行结论一致：

- 机器侧 forward 次数和 S-mode 收到的 IRQ 次数都大于 0；
- 每次 critical section 里都能看到 pending IRQ 被压住；
- 系统没有崩溃，也没有锁重入或临界区破坏。

### 5.4 反汇编证据

[artifacts/interrupt_objdump.txt](/root/os_experiments/lab3/kernel_task4/artifacts/interrupt_objdump.txt) 中可直接看到：

```text
0000000080000f30 <machine_trap_entry>:
...
80000f84: ... <handle_machine_trap>

0000000080000fe0 <supervisor_trap_entry>:
...
80001034: ... <handle_supervisor_trap>

0000000080001964 <lab3_kernel_task4::enable_supervisor_interrupts::...>:
80001966: 10052073      csrs sstatus, a0

0000000080001974 <lab3_kernel_task4::disable_supervisor_interrupts::...>:
80001976: 10053073      csrc sstatus, a0

000000008000196c <lab3_kernel_task4::set_supervisor_timer_pending::...>:
8000196e: 34452073      csrs mip, a0

0000000080001990 <lab3_kernel_task4::clear_supervisor_timer_pending::...>:
80001992: 14453073      csrc sip, a0

0000000080001998 <lab3_kernel_task4::enable_machine_timer_interrupt::...>:
8000199c: 30452073      csrs mie, a0

00000000800019a2 <lab3_kernel_task4::enable_supervisor_timer_source::...>:
800019a4: 10452073      csrs sie, a0
```

这正对应实验设计里的关键点：

1. `machine_trap_entry` 和 `supervisor_trap_entry` 都存在；
2. `sstatus.SIE` 在 safe interval/critical section 切换时确实用 `csrs/csrc sstatus` 控制；
3. M-mode 用 `csrs mip` forward 一个 S-mode 可见 pending；
4. S-mode 用 `csrc sip` 清除 delegated pending；
5. `mie` 和 `sie` 的源位分别被打开。

## 6. 机制解释

### 6.1 为什么 safe interval 要显式开 `sstatus.SIE`

内核主循环分成两类区间：

- safe interval：
  - 显式执行 `enable_supervisor_interrupts()`；
  - 允许 S-mode 在执行内核代码时被打断；
  - 实验里用 `wfi` 等待下一次 forwarded timer IRQ。
- critical section：
  - 通过 `InterruptGuard` 先执行 `csrc sstatus, SIE`；
  - 再进入 `InterruptMutex`；
  - 共享状态修改完成后退出 guard，恢复到进入临界区前的 SIE 状态。

这里的关键不是“永远开中断”，而是“只在可重入的安全区打开中断”。

### 6.2 为什么临界区里 `SIE_after_guard=0`

日志中的：

```text
sstatus.SIE_after_guard=0
```

不是恢复失败，而是本实验的主循环故意把“开中断”集中到下一个 safe interval 做。  
`InterruptGuard` 的语义是“恢复进入 guard 前的 SIE 状态”，而不是“无条件开中断”。  
在本实验里，进入 critical section 之前，上一轮 safe interval 已经结束并主动重新关中断，所以 guard 退出后的状态仍然是 `0`，下一轮 safe interval 再显式打开。

### 6.3 为什么 `pending_forwarded_timer_irq=1` 是好事

临界区里看到：

```text
pending_forwarded_timer_irq=1
```

表示底层时钟事件已经到了，但因为 `SIE=0`，S-mode handler 没有立刻打断当前临界区。  
这正是“关中断保护临界区”的核心现象：

- 时钟源没有丢；
- 事件被延后；
- 离开 critical section 后，safe interval 重新开中断，事件再被处理。

### 6.4 锁为什么要和关中断绑定

当前实验是单核，但中断 handler 和主循环都会访问共享状态。  
如果主循环拿锁时不先关中断，那么：

1. 主循环可能在持锁期间被 timer IRQ 打断；
2. S-mode handler 再次尝试拿同一把锁；
3. 就会出现重入或临界区破坏。

所以 [main.rs](/root/os_experiments/lab3/kernel_task4/src/main.rs) 里的 `InterruptMutex` 把“关中断 + 上锁”绑定在一起，这是本实验最核心的保护策略。

## 7. 验收检查对应关系

1. `sstatus.SIE` 位在安全区间被打开：
   - 日志中有 `safe interval #N: enabling sstatus.SIE for forwarded_timer_irq`；
   - 反汇编中有 `csrs sstatus` 和 `csrc sstatus`。
2. 内核空间发生时钟中断时不会导致系统崩溃：
   - 运行环境始终停留在 S-mode 内核；
   - `origin=mtime` 的 timer 事件被反复 forward 并处理；
   - 两次运行都正常打印 summary 并全部 `PASS`。
3. 锁与临界区使用了关中断保护：
   - `InterruptGuard` 在拿锁前先关 `SIE`；
   - 临界区里日志明确打印 `sstatus.SIE=0 lock=held`；
   - 同时还能观察到 `pending_forwarded_timer_irq=1`，说明中断没有破坏临界区而是被延后。

## 8. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 进程。
- 工具链版本见 [tool_versions.txt](/root/os_experiments/lab3/kernel_task4/artifacts/tool_versions.txt)。
- 当前实现用的是 `MTIP -> SSIP` forwarder，而不是 SBI `set_timer` 或 `Sstc stimecmp`。这使得实验更适合最小裸机环境，但 S-mode 日志中的 interrupt type 显示为 `SSIP`，README 已明确说明其时钟来源仍是 `mtime`。
- UART 输出不是原子的，因此中断日志和普通日志偶尔会有文本交错；最终验收以内核 summary 和 PASS 结果为准。
