# LAB7 内核态 task2：signal 机制实现

## 原始任务

> 完成LAB7 内核态task2：signal 机制实现
> 目标：实现信号投递、处理与返回的完整闭环，正确修改并恢复用户态上下文。
> 要求：
> 1. 支持信号注册、投递与处理；
> 2. 在处理完信号后正确返回用户态原执行流；
> 3. 提供验证：信号触发后程序继续运行且状态正确；
> 4. 在实验记录中说明：用户态 trap 上下文如何被临时改写与恢复。
> 验收检查：
> 1. 内核为进程标记 Pending 信号并在即将返回用户态时进行分发检查；
> 2. 能主动“压栈”并篡改返回地址 sepc 指向用户态 handler，同时布置 sigreturn 返回跳板；
> 3. 用户程序能走完“正常执行 -> 中断 -> 跑 Handler -> sigreturn -> 恢复正常执行”的流程且不引发崩溃。

## 实验环境与方案

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现采用最小化两进程模型：

- `receiver(pid=0)`：注册 `SIGUSR1` 的用户态 handler，然后执行一次 `yield`
- `sender(pid=1)`：通过 `kill` 型系统调用向 `receiver` 投递 `SIGUSR1`
- 内核在下一次“即将返回用户态”时检查 `receiver.pending_mask`
- 若有待处理信号，则把当前用户 TrapFrame 保存为 `saved_frame`，改写新的返回上下文，让用户先去跑 handler
- handler `ret` 到用户态 `sigreturn trampoline`，随后通过 `SYS_SIGRETURN` 恢复原 TrapFrame

这是一个教学用最小模型，刻意没有实现：

- 多信号优先级、屏蔽字与默认动作
- 嵌套 signal
- 用户态 libc 风格信号栈

但它完整覆盖了本任务要求的闭环：

- `sigaction/register`
- `pending` 标记
- 返回用户态前分发
- 用户 handler 执行
- `sigreturn` 恢复 `sepc/sp/ra`
- 原执行流继续运行并产生正确状态

## 文件列表

- `src/main.rs`：页表、两进程调度、`sigaction/kill/yield/report/exit/sigreturn` syscall、pending 检查、TrapFrame 改写与恢复、验收日志。
- `src/boot.S`：M/S/U 切换、trap 入口保存恢复、用户态 receiver/sender/handler/trampoline 程序。
- `src/trap.rs`：TrapFrame 定义与 trap vector 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局与 kernel/trap 栈。
- `artifacts/build_output.txt`：构建日志。
- `artifacts/run_output.txt`：第一次完整运行日志。
- `artifacts/run_output_repeat.txt`：第二次完整运行日志。
- `artifacts/signal_kernel_objdump.txt`：反汇编证据。
- `artifacts/signal_kernel_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链与 QEMU 版本。

## 关键机制说明

### 1. Pending 信号的标记与返回前检查

`sender` 进程执行 `SYS_KILL` 后，内核只做一件事：

- 给 `receiver.pending_mask` 置位 `1 << SIGUSR1`

此时并不立刻跳入 handler，而是等到调度器准备再次返回 `receiver` 用户态时，在 `handle_supervisor_trap()` 的末尾调用 `dispatch_pending_signal_if_needed()`：

1. 先看 `pending_mask != 0`
2. 再看当前没有 `signal_active`
3. 然后取出注册好的 handler 地址
4. 保存当前 `PROCESS_FRAMES[pid]` 到 `saved_frame`
5. 改写新的返回上下文

这正对应验收要求中的“Pending 标记并在即将返回用户态时检查”。

### 2. 用户态 Trap 上下文如何被临时改写

分发 signal 时，内核做了三类修改：

1. 保存原上下文  
   `saved_frame = PROCESS_FRAMES[pid]`，其中包含原 `sepc`、`sp`、通用寄存器等。

2. 主动“压栈”构造用户态 signal frame  
   内核把用户栈从 `0x404000` 下移到 `0x403ff0`，并写入两项内容：
   - `stack[0] = trampoline_va`
   - `stack[8] = saved_epc`

3. 篡改即将返回的 TrapFrame  
   - `epc = handler_va`
   - `ra = trampoline_va`
   - `a0 = signum`
   - `saved_sp = new_sp`

因此，下一次 `sret` 返回用户态时，CPU 不会回到原程序，而是先进入用户 handler；handler `ret` 时会跳到 trampoline；trampoline 发起 `SYS_SIGRETURN`，再由内核把 `saved_frame` 整体恢复。

### 3. sigreturn 如何恢复原执行流

`SYS_SIGRETURN` 被触发后：

1. 内核读取当前 signal frame 栈内容，记录压栈证据
2. 清除 `signal_active`
3. 用 `saved_frame` 覆盖 `PROCESS_FRAMES[pid]`
4. 恢复后的 `epc` 重新指向 signal 前的用户地址

本实验里，恢复后的 `epc` 是 `0x400022`，即 `yield` 返回后的 `__receiver_resume_point`。因此用户程序随后继续执行“读取状态 -> 再自增一次 -> report -> exit”，证明执行流回到了中断前应有的位置。

## 构建、运行与复现

在任务目录 `lab7/kernel_task2/` 下执行：

```bash
cargo build
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab7_kernel_task2
cargo objdump --bin lab7_kernel_task2 -- --demangle -d > artifacts/signal_kernel_objdump.txt
cargo nm --bin lab7_kernel_task2 -- --demangle > artifacts/signal_kernel_nm.txt
```

本次归档时实际保存的产物：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/signal_kernel_objdump.txt`
- `artifacts/signal_kernel_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] sigaction pid=0 signum=10 handler=0x40005e
[kernel] kill sender_pid=1 target_pid=0 signum=10 pending_mask=0x400
[kernel] dispatch signal pid=0 signum=10 saved_epc=0x400022 saved_sp=0x404000 handler_epc=0x40005e new_sp=0x403ff0 trampoline=0x40007c
[kernel] handler report pid=0 state=2 signum=10 sp=0x403ff0 ra=0x40007c
[kernel] sigreturn pid=0 stacked_ra=0x40007c stacked_epc=0x400022 restore_epc=0x400022 restore_sp=0x404000
[kernel] main report pid=0 seen_state=2 final_state=3 signum=10
[kernel] acceptance pending signal is marked then checked before user return: PASS
[kernel] acceptance kernel rewrites user trap context and restores it via sigreturn: PASS
[kernel] acceptance user flow runs normal -> handler -> sigreturn -> normal without crash: PASS
```

从这段日志可以直接读出完整链路：

- `sigaction` 已登记 `SIGUSR1 -> 0x40005e`
- `kill` 只设置 `pending_mask=0x400`
- 返回用户态前，内核把原 `epc=0x400022` 和 `sp=0x404000` 保存起来
- 用户 handler 运行时的 `sp=0x403ff0`，`ra=0x40007c`，说明已切到 signal frame 并布置 trampoline
- `sigreturn` 后恢复 `epc=0x400022 sp=0x404000`
- 主流程继续运行，把状态从 handler 后的 `2` 推进到最终 `3`

### 重复运行

`artifacts/run_output_repeat.txt` 与第一次运行一致：

- `pending_mask` 仍然是 `0x400`
- `saved_epc` / `stacked_epc` / `restored_epc` 仍然都是 `0x400022`
- 三条 acceptance 继续全部为 `PASS`

说明该 signal 闭环是稳定可复现的。

### 符号与反汇编证据

`artifacts/signal_kernel_nm.txt` 中的关键符号：

```text
0000000080000f32 T __receiver_resume_point
0000000080000f6e T __user_signal_handler
0000000080000f8c T __sigreturn_trampoline
00000000800014d4 t lab7_kernel_task2::do_sigreturn::...
000000008000442e t lab7_kernel_task2::dispatch_pending_signal_if_needed::...
```

`artifacts/signal_kernel_objdump.txt` 中能看到用户态恢复点、handler 和 trampoline：

```text
0000000080000f32 <__receiver_resume_point>:
80000f4c: 00000073      ecall
80000f54: 00000073      ecall

0000000080000f6e <__user_signal_handler>:
80000f84: 4891          li a7, 0x4
80000f86: 00000073      ecall
80000f8a: 8082          ret

0000000080000f8c <__sigreturn_trampoline>:
80000f8c: 4899          li a7, 0x6
80000f8e: 00000073      ecall
```

这说明：

- `__receiver_resume_point` 是一个独立的用户态恢复点
- handler 在用户态运行，并最终执行 `ret`
- `ret` 的目标是 trampoline
- trampoline 通过 `SYS_SIGRETURN` 进入内核完成恢复

## 验收检查映射

### 1. Pending 信号标记并在返回用户态前分发检查

证据：

- `run_output.txt` 中的  
  `kill ... pending_mask=0x400`
- 随后的  
  `dispatch signal pid=0 ...`
- 最终 acceptance  
  `pending signal is marked then checked before user return: PASS`

结论：

- 已满足。

### 2. 压栈、篡改 sepc、布置 sigreturn 返回跳板

证据：

- `dispatch signal ... saved_epc=0x400022 saved_sp=0x404000 ... new_sp=0x403ff0 trampoline=0x40007c`
- `handler report ... sp=0x403ff0 ra=0x40007c`
- `sigreturn ... stacked_ra=0x40007c stacked_epc=0x400022 restore_epc=0x400022`
- `signal_kernel_nm.txt` / `signal_kernel_objdump.txt` 中的 `__sigreturn_trampoline`

结论：

- 已满足。

### 3. 用户程序完成“正常执行 -> 中断 -> 跑 Handler -> sigreturn -> 恢复正常执行”

证据：

- handler 阶段：
  `handler report pid=0 state=2 signum=10`
- 恢复后主流程阶段：
  `main report pid=0 seen_state=2 final_state=3 signum=10`
- 最终 acceptance：
  `user flow runs normal -> handler -> sigreturn -> normal without crash: PASS`

结论：

- 已满足。

## 环境说明与限制

- 已在当前 Linux 主机上的 `QEMU virt` 环境完成构建和两次复现实验。
- README 中所有观测结果都来自 QEMU guest 内部打印，不是宿主 Linux 信号机制。
- 该实现是课程验证用最小模型，只验证一个标准信号 `SIGUSR1` 的完整闭环，不代表完整 POSIX signal 子系统。
