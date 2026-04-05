# LAB3 内核态 Task5：内核任务与抢占式切换

## 1. 原始任务说明

### 任务标题

内核任务与抢占式切换

### 任务目标

理解“内核线程/内核任务”模型，完成可抢占的内核任务调度。

### 任务要求

1. 支持至少 1 类内核任务（例如后台回收/日志刷新/守护任务）；
2. 支持抢占式切换（与用户任务共享调度框架或独立队列均可，但需说明）；
3. 给出验证程序或日志证明其确实在运行且可被抢占。

### 验收检查

1. 存在独立执行于 S 态（内核态）而不关联用户地址空间（无 U 态栈）的 Task；
2. 时钟中断能够成功挂起该内核任务并调度到其他任务。

## 2. 实验目标与实现思路

本实验在 [lab3/kernel_task5](/root/os_experiments/lab3/kernel_task5) 中实现了一个最小 S-mode 内核线程调度器。实验中只有内核任务，没有用户任务，也没有 U-mode 栈或用户地址空间切换。

调度框架采用“独立内核任务队列”：

1. 维护 2 个纯 S-mode 内核任务：
   - `recycler_daemon`
   - `logger_daemon`
2. 每个任务都有自己独立的 kernel stack 和 `TrapFrame`；
3. 调度器在 S-mode trap handler 中按 round-robin 在两个 kernel task 之间切换；
4. 两个任务都只运行在 S 态，不经过 U 态，不绑定用户地址空间。

时钟中断链路采用：

```text
mtime/MTIP -> M-mode timer forwarder -> delegated SSIP -> S-mode scheduler
```

这里没有引入 SBI `set_timer` 或 `Sstc stimecmp`，所以仍由 M-mode 读取 `mtime/mtimecmp` 并转发时钟事件。  
S-mode 最终收到的是 delegated `SSIP`，但日志里明确保留了 `origin=mtime`，这样既能稳定复现中断，又能说明底层来源确实是时钟中断。

切换策略是抢占式的：

1. 当前 kernel task 在 S-mode 正常执行；
2. `mtimecmp` 到期后先进入 M-mode machine trap；
3. M-mode 重新设定下一次时钟并置位 `SSIP`；
4. S-mode 收到 delegated interrupt 后，保存当前 task 的 `TrapFrame`；
5. 调度器选择另一个 runnable kernel task，把它的 `TrapFrame` 覆盖到当前 trap frame；
6. `sret` 返回后直接继续执行另一个 kernel task。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/kernel_task5/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/kernel_task5/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/kernel_task5/linker.ld)：镜像布局、bootstrap kernel stack、两个 kernel task stack、S-mode trap stack 和 M-mode trap stack。
- [src/boot.S](/root/os_experiments/lab3/kernel_task5/src/boot.S)：`enter_supervisor`、`enter_kernel_task`、`machine_trap_entry` 和 `supervisor_trap_entry`。
- [src/trap.rs](/root/os_experiments/lab3/kernel_task5/src/trap.rs)：通用 `TrapFrame` 以及 `mtvec/stvec` 初始化。
- [src/main.rs](/root/os_experiments/lab3/kernel_task5/src/main.rs)：内核任务控制块、timer forwarder、S-mode 调度器、后台任务主体、最终汇总和验收输出。
- [src/console.rs](/root/os_experiments/lab3/kernel_task5/src/console.rs)：UART 输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/kthread_switch_objdump.txt](/root/os_experiments/lab3/kernel_task5/artifacts/kthread_switch_objdump.txt)：`enter_kernel_task`、machine/supervisor trap entry、timer forwarder 与调度路径的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/kernel_task5/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/kernel_task5
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
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task5 > artifacts/run_output.txt
```

第二次运行：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task5 > artifacts/run_output_repeat.txt
```

导出切换路径反汇编：

```bash
cargo objdump --bin lab3_kernel_task5 -- --demangle -d | rg -n -C 5 "enter_supervisor|enter_kernel_task|machine_trap_entry|supervisor_trap_entry|handle_machine_trap|handle_supervisor_trap|csrs mie|csrs sie|csrs mip|csrc sip|sret|mret" > artifacts/kthread_switch_objdump.txt
```

记录工具链：

```bash
{ printf 'rustc: '; rustc --version; printf 'cargo: '; cargo --version; printf 'targets:\n'; rustup target list | grep riscv64gc; printf 'qemu: '; qemu-system-riscv64 --version | head -n 1; } > artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab3_kernel_task5 v0.1.0 (/root/os_experiments/lab3/kernel_task5)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
```

### 5.2 第一次运行结果

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output.txt)：

```text
[kernel] task[0]: id=1 name=recycler_daemon role=background_reclaimer mode=S-only address_space=kernel-only u_stack=none kernel_stack_top=0x8000d930
[kernel] task[1]: id=2 name=logger_daemon role=log_flusher mode=S-only address_space=kernel-only u_stack=none kernel_stack_top=0x80011930
[sched] switch#01 reason=timer_preempt from=1(recycler_daemon) saved_sepc=0x80001a6e -> to=2(logger_daemon) next_sepc=0x80001928
[sched] switch#02 reason=timer_preempt from=2(logger_daemon) saved_sepc=0x80001928 -> to=1(recycler_daemon) next_sepc=0x80001a6e
[kthread] start id=1 name=recycler_daemon mode=S-only stack_top=0x8000d930
[kthread] start id=2 name=logger_daemon mode=S-only stack_top=0x80011930
...
[kernel] summary: machine_timer_forwards=19 supervisor_timer_irqs=18 preempt_switches=18
[kernel] task_summary[recycler_daemon]: started=yes progress=8265246259472653266 switch_ins=10 preemptions=9 kernel_stack_top=0x8000d930
[kernel] task_summary[logger_daemon]: started=yes progress=5854435521184466307 switch_ins=9 preemptions=9 kernel_stack_top=0x80011930
[kernel] acceptance kernel task exists in S-mode without U-stack: PASS
[kernel] acceptance timer interrupt suspended kernel task and scheduled another: PASS
```

从第一次运行可以直接看到：

1. 两个 task 都是 `mode=S-only`、`address_space=kernel-only`、`u_stack=none`；
2. 调度器日志明确给出了 `from -> to` 的 timer preempt 切换；
3. 两个 task 都打印了自己的启动信息；
4. 两个 task 的 `progress` 都大于 `0`，说明它们确实执行过；
5. `switch_ins` 和 `preemptions` 都非零，说明 timer interrupt 确实把当前 kernel task 挂起并切到了另一个 task。

### 5.3 第二次运行结果

以下内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output_repeat.txt)：

```text
[kernel] summary: machine_timer_forwards=19 supervisor_timer_irqs=18 preempt_switches=18
[kernel] task_summary[recycler_daemon]: started=yes progress=18005714526213310642 switch_ins=10 preemptions=9 kernel_stack_top=0x8000d930
[kernel] task_summary[logger_daemon]: started=yes progress=5167716631406653398 switch_ins=9 preemptions=9 kernel_stack_top=0x80011930
[kernel] acceptance kernel task exists in S-mode without U-stack: PASS
[kernel] acceptance timer interrupt suspended kernel task and scheduled another: PASS
```

第二次运行结论一致：

- 仍然发生了 18 次 supervisor 侧抢占切换；
- 两个 kernel task 仍然都启动并推进了各自的后台工作；
- 验收项继续全部通过。

### 5.4 反汇编证据

[artifacts/kthread_switch_objdump.txt](/root/os_experiments/lab3/kernel_task5/artifacts/kthread_switch_objdump.txt) 中可以直接看到：

```text
0000000080000146 <enter_kernel_task>:
...
800001ec: 10200073      sret

00000000800001f0 <machine_trap_entry>:
...
80000244: ... <handle_machine_trap>
...
80000296: 30200073      mret

00000000800002a0 <supervisor_trap_entry>:
...
800002f4: ... <handle_supervisor_trap>
...
80000346: 10200073      sret

0000000080000f68 <handle_machine_trap>:
...

0000000080001074 <handle_supervisor_trap>:
...
```

同一文件里还能看到中断源位和 pending 位的控制：

```text
8000199c: 30452073      csrs mie, a0
800019a4: 10452073      csrs sie, a0
```

这说明：

1. M-mode timer interrupt 被打开；
2. S-mode delegated interrupt 源被打开；
3. `enter_kernel_task` 和 `supervisor_trap_entry` 都通过 `sret` 在 S-mode task 上下文里切换。

## 6. 机制解释

### 6.1 为什么这是“独立执行于 S 态、无 U 栈”的内核任务

本实验的两个 task 都不是用户进程，也不关联用户地址空间：

- 只在 S-mode 运行；
- 不切到 U-mode；
- 没有 U-mode stack；
- 每个 task 只有自己的 kernel stack 和 `TrapFrame`。

初始化日志里直接打印了：

```text
mode=S-only
address_space=kernel-only
u_stack=none
```

这正对应验收点 1。

### 6.2 抢占式切换是如何发生的

任务本身不会主动 `yield`。切换完全由时钟中断驱动：

1. 当前 kernel task 正在 S-mode 执行后台循环；
2. `mtimecmp` 到期，CPU 先进入 M-mode `machine_trap_entry`；
3. `handle_machine_trap()` 重新设定下一次时钟并 forward 一个 delegated `SSIP`；
4. S-mode `supervisor_trap_entry` 进入 `handle_supervisor_trap()`；
5. 调度器把当前 task 的 trap frame 存回 `TASKS[current].frame`；
6. 选择 `next`，把 `TASKS[next].frame` 覆盖到当前 trap frame；
7. `sret` 返回后继续执行另一个 kernel task。

因此这不是 cooperative 切换，而是真正的 preemptive scheduling。

### 6.3 为什么 switch 日志能证明“被挂起后切到其他任务”

日志格式：

```text
[sched] switch#NN reason=timer_preempt from=A saved_sepc=... -> to=B next_sepc=...
```

里面同时包含：

- 切换原因 `reason=timer_preempt`
- 当前被挂起的 task `from=A`
- 目标 task `to=B`
- 保存点 `saved_sepc`
- 恢复点 `next_sepc`

这已经直接给出了“时钟中断挂起当前内核任务并调度到其他任务”的证据。

## 7. 验收检查对应关系

1. 存在独立执行于 S 态而不关联用户地址空间的 Task：
   - [run_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output.txt) 中直接打印 `mode=S-only address_space=kernel-only u_stack=none`；
   - [linker.ld](/root/os_experiments/lab3/kernel_task5/linker.ld) 为每个 task 单独分配了 kernel stack，而没有任何 U-mode stack；
   - [boot.S](/root/os_experiments/lab3/kernel_task5/src/boot.S) 的 `enter_kernel_task` 通过 `sret` 直接进入 S-mode task。
2. 时钟中断能够成功挂起该内核任务并调度到其他任务：
   - [run_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output.txt) 和 [run_output_repeat.txt](/root/os_experiments/lab3/kernel_task5/artifacts/run_output_repeat.txt) 中都存在多条 `reason=timer_preempt from=A -> to=B`；
   - summary 中 `preempt_switches=18`；
   - 两个 task 的 `switch_ins` 和 `preemptions` 都非零。

## 8. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 进程。
- 工具链版本见 [tool_versions.txt](/root/os_experiments/lab3/kernel_task5/artifacts/tool_versions.txt)。
- 为了在最小裸机场景稳定复现，本实验采用 `MTIP -> SSIP` forwarder，而不是 SBI `set_timer` 或 `Sstc stimecmp`。日志中保留了 `origin=mtime` 来说明底层时钟来源。
- 两个后台任务是无限循环任务，实验通过达到预定抢占次数后由内核 summary 主动结束，不依赖任务自行退出。
