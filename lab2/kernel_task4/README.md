# LAB2 内核态 Task4: 异常信息统计与现场输出

## 1. 原始任务说明

### 任务标题

异常信息统计与现场输出

### 任务目标

掌握异常处理路径，能够在异常发生时保留现场并输出足够的诊断信息。

### 任务要求

1. 当应用触发异常时，输出：异常类型、出错地址、出错指令（或 PC）、必要寄存器信息；
2. 输出需结构化（便于助教核验），并确保不会导致二次崩溃；
3. 在实验记录中解释：异常现场信息如何从硬件 trap 上下文中获取。

### 验收检查

1. 提供应用触发 Store Page Fault / Illegal Instruction 等异常的现场日志截图；
2. 日志包含明确的 `scause`、`sepc`、`stval` 等关键寄存器值；
3. 故障进程被安全杀掉，系统继续运行其他任务而不挂死。

## 2. 实验目标与实现思路

本任务单独放在 [lab2/kernel_task4](/root/os_experiments/lab2/kernel_task4) 中，并且不再沿用前几个任务的 M-mode 内核框架，而是改成：

- M-mode 只负责启动、PMP 设置和 trap delegation；
- 真正的内核运行在 S-mode；
- 用户应用运行在 U-mode；
- U-mode 触发异常后，硬件把现场写入 `scause/sepc/stval`，再跳到 S-mode `stvec`。

这样做的目的很直接：验收要求明确点名 `scause/sepc/stval`，因此最稳妥的方案就是让异常真实落到 S-mode，而不是在 M-mode 中打印“等价寄存器”。

为了覆盖验收场景，这次顺序运行 5 个应用：

1. `healthy_before_faults`：正常 `write + exit`，作为基线；
2. `illegal_instruction`：U-mode 执行 `csrw sstatus, zero`，触发 `Illegal Instruction`；
3. `healthy_after_illegal`：验证系统能在非法指令 fault 后继续运行；
4. `store_page_fault`：U-mode 向地址 `0x0` 写数据，触发 `Store/AMO Page Fault`；
5. `healthy_after_store_fault`：验证系统能在页故障后继续运行。

最后内核还会输出异常计数统计：

- `illegal_instruction`
- `store_page_fault`
- `load_page_fault`
- `instruction_page_fault`
- `other_faults`
- `clean_exits`

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/kernel_task4/Cargo.toml)：Rust 裸机工程配置，包名为 `lab2_kernel_task4`。
- [.cargo/config.toml](/root/os_experiments/lab2/kernel_task4/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab2/kernel_task4/linker.ld)：镜像、内核栈、用户栈布局。
- [src/boot.S](/root/os_experiments/lab2/kernel_task4/src/boot.S)：M-mode 到 S-mode 切换、S-mode trap 入口、U-mode `sret` 返回路径。
- [src/trap.rs](/root/os_experiments/lab2/kernel_task4/src/trap.rs)：读取 `scause/stval` 并分发 `ecall` 与异常。
- [src/main.rs](/root/os_experiments/lab2/kernel_task4/src/main.rs)：页表、trap delegation、异常日志、异常统计、任务推进与最终报告。
- [src/syscall.rs](/root/os_experiments/lab2/kernel_task4/src/syscall.rs)：用户态 `write/exit` 封装。
- [src/console.rs](/root/os_experiments/lab2/kernel_task4/src/console.rs)：S-mode 串口输出。
- [src/apps/healthy_before_faults.rs](/root/os_experiments/lab2/kernel_task4/src/apps/healthy_before_faults.rs)：正常任务。
- [src/apps/illegal_instruction.rs](/root/os_experiments/lab2/kernel_task4/src/apps/illegal_instruction.rs)：非法指令异常任务。
- [src/apps/healthy_after_illegal.rs](/root/os_experiments/lab2/kernel_task4/src/apps/healthy_after_illegal.rs)：非法指令后的恢复任务。
- [src/apps/store_page_fault.rs](/root/os_experiments/lab2/kernel_task4/src/apps/store_page_fault.rs)：Store Page Fault 任务。
- [src/apps/healthy_after_store_fault.rs](/root/os_experiments/lab2/kernel_task4/src/apps/healthy_after_store_fault.rs)：页故障后的恢复任务。
- [artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/build_output.txt)：最近一次构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt)：第一次完整运行输出。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output_repeat.txt)：第二次完整运行输出。

## 4. 关键机制说明

### 4.1 为什么这次能拿到 `scause/sepc/stval`

启动顺序如下：

1. `_start` 在 M-mode 进入 [start_machine](/root/os_experiments/lab2/kernel_task4/src/main.rs)；
2. M-mode 设置 `PMP` 并把 `Illegal Instruction`、`Page Fault`、`U-mode ecall` 等异常委托给 S-mode；
3. 通过 [enter_supervisor](/root/os_experiments/lab2/kernel_task4/src/boot.S) 使用 `mret` 进入 S-mode；
4. S-mode 设置 `stvec`，之后 U-mode 的 trap 直接进入 [trap_entry](/root/os_experiments/lab2/kernel_task4/src/boot.S)。

一旦 U-mode 发生异常，硬件会自动完成三件事：

- 把异常原因写入 `scause`
- 把出错 PC 写入 `sepc`
- 把附加信息写入 `stval`

Rust 侧的 [handle_trap](/root/os_experiments/lab2/kernel_task4/src/trap.rs) 只是把这些硬件已经保存好的 CSR 读出来，并结合汇编入口保存的通用寄存器，形成结构化日志。

### 4.2 trap 现场是如何保存的

[src/boot.S](/root/os_experiments/lab2/kernel_task4/src/boot.S) 的 `trap_entry` 做了两步关键操作：

1. `csrrw sp, sscratch, sp`：把 U-mode 的用户栈切换到内核栈；
2. 把 `ra/gp/t0...a7/s0...` 等寄存器和 `sepc` 全部压到 trap frame。

[TrapFrame](/root/os_experiments/lab2/kernel_task4/src/trap.rs) 中保存了：

- `ra`
- `gp`
- `s0`
- `a0/a1/a7`
- `user_sp`
- `sepc`
- 其余 GPR

因此异常日志中的寄存器值不是“事后推测”，而是 trap 入口第一时间从硬件上下文里保存下来的。

### 4.3 为什么是真正的 Store Page Fault

要触发真正的 `Store Page Fault`，必须让 U-mode 运行在分页开启的地址空间里，而且目标地址真的“未映射”。  
本实验在 [src/main.rs](/root/os_experiments/lab2/kernel_task4/src/main.rs) 中建立了一个最小 Sv39 页表：

- `root[2]`：把 `0x8000_0000` 开始的 RAM 做 S-mode 内核恒等映射，`U=0`
- `root[1]`：把同一段物理 RAM 别名映射到 `0x4000_0000`，供 U-mode 执行，`U=1`
- `root[0]`：只映射 UART 和 `qemu-exit` 所需的 MMIO

关键点是：地址 `0x0` 没有被映射。  
因此 [store_page_fault.rs](/root/os_experiments/lab2/kernel_task4/src/apps/store_page_fault.rs) 中：

```rust
core::ptr::write_volatile(0 as *mut u64, 0xdead_beef_dead_beefu64);
```

在 U-mode 下会命中真正的 `Store/AMO Page Fault (scause=0xf)`，而不是 `Access Fault`。

### 4.4 如何避免二次崩溃

异常日志里除了 `sepc`，我还打印了 faulting instruction。  
但直接去读一段未知地址，可能导致内核自己再次 fault，所以 [read_user_instruction](/root/os_experiments/lab2/kernel_task4/src/main.rs) 先做了两层防护：

1. `sepc` 必须落在用户别名区 `[USER_BASE, user_memory_end())`；
2. 只读取 `sepc` 指向的指令，不读取 `stval` 对应的数据地址。

因此即便 `stval=0x0`，日志也不会因为解引用空指针而产生二次崩溃。

### 4.5 故障进程如何被安全杀掉

[handle_user_exception](/root/os_experiments/lab2/kernel_task4/src/main.rs) 会：

1. 把 fault 信息保存到 `RUNS[index]`；
2. 增加全局异常统计；
3. 立刻打印结构化日志；
4. 调用 `advance_to_next_app()` 跳过当前故障任务，直接启动下一个任务。

因此 faulting app 不会返回用户态，也不会拖垮整个系统。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/kernel_task4
```

构建：

```bash
cargo build
```

运行一次并保存输出：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task4 > artifacts/run_output.txt
```

再次运行复验：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task4 > artifacts/run_output_repeat.txt
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 6.2 第一次运行完整输出

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt)：

```text
[kernel] booted in S-mode
[kernel] starting LAB2 kernel task4 exception diagnostics suite
[kernel] trap CSRs: scause/sepc/stval will be logged from supervisor trap context
[kernel] address layout: kernel identity @ 0x80000000, user alias @ 0x40000000
[kernel] exception summary target: Illegal Instruction + Store/AMO Page Fault, then continue running later tasks
[kernel] launch app=healthy_before_faults | expected=normal write+exit path should succeed
[user] healthy_before_faults
[kernel] result app=healthy_before_faults status=exit(0)
[kernel] launch app=illegal_instruction | expected=U-mode writes sstatus CSR and should raise Illegal Instruction
[kernel] exception app=illegal_instruction action=kill-and-continue
[kernel]   scause=0x2 interrupt=0 code=2 type=Illegal Instruction
[kernel]   sepc=0x40000b48 stval=0x10001073
[kernel]   instruction=0x10001073
[kernel]   regs ra=0x80001018 sp=0x400141e0 gp=0x0 s0=0x0 a0=0x40000b48 a1=0x400141e0 a7=0x1
[kernel] result app=illegal_instruction status=fault(type=Illegal Instruction, scause=0x2, sepc=0x40000b48, stval=0x10001073)
[kernel] launch app=healthy_after_illegal | expected=system should continue after illegal instruction fault
[user] survived_illegal_instruction
[kernel] result app=healthy_after_illegal status=exit(0)
[kernel] launch app=store_page_fault | expected=U-mode store to unmapped address 0x0 should raise Store/AMO Page Fault
[kernel] exception app=store_page_fault action=kill-and-continue
[kernel]   scause=0xf interrupt=0 code=15 type=Store/AMO Page Fault
[kernel]   sepc=0x40000bda stval=0x0
[kernel]   instruction=0x0000e188
[kernel]   regs ra=0x40000bd4 sp=0x400141a0 gp=0x0 s0=0x0 a0=0xdeadbeefdeadbeef a1=0x0 a7=0x1
[kernel] result app=store_page_fault status=fault(type=Store/AMO Page Fault, scause=0xf, sepc=0x40000bda, stval=0x0)
[kernel] launch app=healthy_after_store_fault | expected=system should continue after store page fault
[user] survived_store_page_fault
[kernel] result app=healthy_after_store_fault status=exit(0)
[kernel] final exception summary:
[kernel] result app=healthy_before_faults status=exit(0)
[kernel] result app=illegal_instruction status=fault(type=Illegal Instruction, scause=0x2, sepc=0x40000b48, stval=0x10001073)
[kernel] result app=healthy_after_illegal status=exit(0)
[kernel] result app=store_page_fault status=fault(type=Store/AMO Page Fault, scause=0xf, sepc=0x40000bda, stval=0x0)
[kernel] result app=healthy_after_store_fault status=exit(0)
[kernel] exception_stats illegal_instruction=1 store_page_fault=1 load_page_fault=0 instruction_page_fault=0 other_faults=0 clean_exits=3
[kernel] check healthy task before faults exits cleanly: PASS
[kernel] check illegal instruction fault captured with scause/sepc/stval: PASS
[kernel] check system continues after illegal instruction: PASS
[kernel] check store page fault captured with scause/sepc/stval: PASS
[kernel] check system continues after store page fault: PASS
[kernel] check exception counters are consistent: PASS
```

### 6.3 第二次运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output_repeat.txt) 与第一次一致，关键 fault 值稳定复现：

- `Illegal Instruction`: `scause=0x2`, `sepc=0x40000b48`, `stval=0x10001073`
- `Store/AMO Page Fault`: `scause=0xf`, `sepc=0x40000bda`, `stval=0x0`
- 两次 fault 之后，后续健康任务都能继续输出并正常 `exit(0)`

## 7. 结果分析

### 7.1 Illegal Instruction

[illegal_instruction.rs](/root/os_experiments/lab2/kernel_task4/src/apps/illegal_instruction.rs) 在 U-mode 执行：

```rust
asm!("csrw sstatus, zero", options(nostack));
```

这是特权指令，U-mode 无权执行，因此：

- `scause=0x2`
- `type=Illegal Instruction`
- `stval=0x10001073`
- `instruction=0x10001073`

可以看到，QEMU 在这条异常上把 faulting instruction bits 也放进了 `stval`，和我们从 `sepc` 位置安全读取到的指令一致。

### 7.2 Store Page Fault

[store_page_fault.rs](/root/os_experiments/lab2/kernel_task4/src/apps/store_page_fault.rs) 对虚地址 `0x0` 做写操作，而 `0x0` 在页表中未映射，因此：

- `scause=0xf`
- `type=Store/AMO Page Fault`
- `stval=0x0`
- `sepc=0x40000bda`

这说明异常原因是“对未映射页的存储访问”，不是普通的 `Access Fault`。

### 7.3 系统恢复性

这次的关键验收点不是“能看到 fault”，而是“fault 后系统不挂”。  
日志里先后出现：

- `[user] survived_illegal_instruction`
- `[user] survived_store_page_fault`

说明两个故障任务都被安全终止，系统继续运行了后续任务，没有因为异常路径而整体卡死。

## 8. 验收检查对应关系

1. 提供触发 `Store Page Fault / Illegal Instruction` 的现场日志：
   - [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt) 已包含两类 fault 的完整终端输出；
   - 你复现时可直接在 QEMU 终端截图。
2. 日志包含 `scause/sepc/stval`：
   - 非法指令行中有 `scause=0x2`、`sepc=0x40000b48`、`stval=0x10001073`
   - Store Page Fault 行中有 `scause=0xf`、`sepc=0x40000bda`、`stval=0x0`
3. 故障进程被安全杀掉、系统继续运行：
   - `healthy_after_illegal` 与 `healthy_after_store_fault` 都成功执行并退出；
   - 最终 `exception_stats` 与 6 项检查全部 `PASS`。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境完成，使用：
  - `rustc 1.94.1`
  - `cargo 1.94.1`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器复现。
- 这是教学化最小 S-mode 内核，只实现了足以触发和诊断异常的最小页表与 trap 路径；
- 没有完整进程管理、信号机制和独立地址空间回收逻辑；
- 这里的“安全杀掉”是指：当前 faulting app 不再返回用户态，内核保存现场后跳到下一个任务继续执行。
