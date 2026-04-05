# LAB3 内核态 Task3：浮点上下文切换与抢占支持

## 1. 原始任务说明

### 任务标题

浮点上下文切换与抢占支持

### 任务目标

完成浮点寄存器的保存/恢复，保证浮点任务在抢占切换后数值正确。

### 任务要求

1. 保存/恢复浮点寄存器集合（以 RISC-V F 扩展为例）；
2. 通过用户态浮点验证程序证明正确性；
3. 在实验记录中说明：何时需要保存浮点上下文（lazy/eager 策略均可，但需说明）。

### 验收检查

1. Trap 上下文或进程上下文中增加了 `f0-f31` 以及 `fcsr` 的空间；
2. 汇编恢复/保存代码逻辑正确，未遗漏状态寄存器；
3. 浮点测试用例在 QEMU 中满分通过。

## 2. 实验目标与实现思路

本实验在 [lab3/kernel_task3](/root/os_experiments/lab3/kernel_task3) 中实现了一个最小 RISC-V 裸机 guest 内核和两个 U-mode 浮点任务。重点不在“复杂内核功能”，而在于把浮点上下文真正纳入 trap 和任务切换路径。

实现采用 `eager` 策略：

1. 每次 trap 进入时，无条件把当前任务的 `f0-f31` 和 `fcsr` 保存进 `TrapFrame`；
2. 每次 trap 返回前，无条件从 `TrapFrame` 恢复 `f0-f31` 和 `fcsr`；
3. 当时间片中断抢占当前任务时，内核把整个 `TrapFrame` 写回当前任务控制块，再把下一个任务的 `TrapFrame` 覆盖到当前 trap frame，最后 `mret` 回到目标任务。

这里选择 `eager` 而不是 `lazy`，原因很直接：

- 当前实验的两个用户任务都长时间处于浮点热路径；
- trap 既可能来自 `ecall`，也可能来自异步定时器中断；
- 为了让验收证据清晰、路径单一，最稳妥的方式就是“只要进 trap，就保存/恢复全部浮点状态”。

验证方式仍然使用用户态浮点校验程序，但结论服务于内核：

1. 先单独运行 `fp_alpha` 和 `fp_beta`，得到参考校验值；
2. 再开启 `mtime/mtimecmp` 定时器抢占，让两个任务在浮点循环中被反复切换；
3. 如果内核在 trap 保存/恢复中遗漏任意浮点寄存器或 `fcsr`，并发阶段最终校验值就会与参考值逐位不一致。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab3/kernel_task3/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab3/kernel_task3/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf` 目标与链接脚本。
- [linker.ld](/root/os_experiments/lab3/kernel_task3/linker.ld)：镜像布局、内核栈与两个用户栈。
- [src/boot.S](/root/os_experiments/lab3/kernel_task3/src/boot.S)：启动入口、`enter_task`、`trap_entry` 和 `fp_stress_loop`。
- [src/trap.rs](/root/os_experiments/lab3/kernel_task3/src/trap.rs)：`TrapFrame` 定义、trap 向量初始化和 Rust trap 分发入口。
- [src/main.rs](/root/os_experiments/lab3/kernel_task3/src/main.rs)：定时器抢占、任务控制块、参考/并发两阶段调度、最终校验与验收输出。
- [src/syscall.rs](/root/os_experiments/lab3/kernel_task3/src/syscall.rs)：用户态 `write/finish` syscall 封装。
- [src/console.rs](/root/os_experiments/lab3/kernel_task3/src/console.rs)：内核 UART 输出。
- [src/user_console.rs](/root/os_experiments/lab3/kernel_task3/src/user_console.rs)：用户态格式化输出。
- [artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/trap_fp_context_objdump.txt](/root/os_experiments/lab3/kernel_task3/artifacts/trap_fp_context_objdump.txt)：`enter_task`、`trap_entry` 和 `fp_stress_loop` 的反汇编证据。
- [artifacts/tool_versions.txt](/root/os_experiments/lab3/kernel_task3/artifacts/tool_versions.txt)：工具链版本。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab3/kernel_task3
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
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task3 > artifacts/run_output.txt
```

第二次运行：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab3_kernel_task3 > artifacts/run_output_repeat.txt
```

导出浮点上下文相关反汇编：

```bash
cargo objdump --bin lab3_kernel_task3 -- --demangle -d | sed -n '/<enter_task>:/,/^$/p;/<trap_entry>:/,/^$/p;/<fp_stress_loop>:/,/^$/p' > artifacts/trap_fp_context_objdump.txt
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
cat artifacts/trap_fp_context_objdump.txt
cat artifacts/tool_versions.txt
```

## 5. 本次实际运行结果

### 5.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 5.2 第一次运行结果

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output.txt)：

```text
[kernel] fp trap frame: 528 bytes, includes f0-f31 (256) + fcsr
[kernel] fp context strategy: eager save/restore on every trap entry/exit
...
[kernel] timer interrupt #1: preempt fp_alpha -> fp_beta at mepc=0x80000f68
[kernel] timer interrupt #2: preempt fp_beta -> fp_alpha at mepc=0x80002e42
[kernel] timer interrupt #3: preempt fp_alpha -> fp_beta at mepc=0x80000f68
...
[kernel] concurrent checksum [fp_alpha] = 0x7ffec4df42aade3f
[kernel] concurrent checksum [fp_beta] = 0x801936f547486022
[kernel] summary: timer_interrupts=86 forced_switches=86
[kernel] result [fp_alpha]: expected=0x7ffec4df42aade3f observed=0x7ffec4df42aade3f => PASS
[kernel] result [fp_beta]: expected=0x801936f547486022 observed=0x801936f547486022 => PASS
[kernel] acceptance concurrent floating-point checksums match reference exactly: PASS
[kernel] acceptance eager fp save/restore preserved f0-f31 and fcsr across preemption: PASS
```

从第一次运行可以直接看到：

1. trap frame 明确给出了 `528 bytes`，并包含 `f0-f31` 与 `fcsr`；
2. 并发阶段真实发生了 `86` 次定时器中断和 `86` 次强制切换；
3. 两个浮点任务的并发校验值与参考值逐位一致；
4. 内核最终给出 `PASS`。

### 5.3 第二次运行结果

以下关键内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output_repeat.txt)：

```text
[kernel] summary: timer_interrupts=90 forced_switches=90
[kernel] result [fp_alpha]: expected=0x7ffec4df42aade3f observed=0x7ffec4df42aade3f => PASS
[kernel] result [fp_beta]: expected=0x801936f547486022 observed=0x801936f547486022 => PASS
[kernel] acceptance concurrent floating-point checksums match reference exactly: PASS
[kernel] acceptance eager fp save/restore preserved f0-f31 and fcsr across preemption: PASS
```

第二次运行仍然通过，说明：

- 抢占和切换稳定发生；
- 浮点上下文在多次切换后仍然保持正确；
- 该实验在当前 QEMU 环境可重复复现。

### 5.4 反汇编证据

[artifacts/trap_fp_context_objdump.txt](/root/os_experiments/lab3/kernel_task3/artifacts/trap_fp_context_objdump.txt) 中可以直接看到：

```text
0000000080000c70 <enter_task>:
80000c9c: 200fb283      ld   t0, 0x200(t6)
80000ca0: 00329073      fscsr t0
80000ca4: 100fb007      fld  ft0, 0x100(t6)
...
80000d20: 1f8fbf87      fld  ft11, 0x1f8(t6)

0000000080000db0 <trap_entry>:
80000e00: a202          fsd  ft0, 0x100(sp)
...
80000e3e: bffe          fsd  ft11, 0x1f8(sp)
80000e40: 003022f3      frcsr t0
80000e44: 20513023      sd   t0, 0x200(sp)
...
80000e5c: 20013283      ld   t0, 0x200(sp)
80000e60: 00329073      fscsr t0
80000e6a: 2012          fld  ft0, 0x100(sp)
...
80000ea8: 3ffe          fld  ft11, 0x1f8(sp)
```

这说明：

1. `enter_task` 在恢复用户上下文前会恢复 `fcsr` 和全部浮点寄存器；
2. `trap_entry` 会完整保存 `f0-f31`；
3. `trap_entry` 还会读取并保存 `fcsr`；
4. 返回前会先恢复 `fcsr`，再恢复全部浮点寄存器。

同一个反汇编文件里还能看到浮点压力循环：

```text
0000000080000f00 <fp_stress_loop>:
80000f12: 00053007      fld ft0, 0x0(a0)
...
80000f32: 2188          fld fa0, 0x0(a1)
...
80000f68: ...           fmadd.d ...
```

说明用户态测试程序确实在长循环中持续使用浮点寄存器，满足验收中“通过用户态浮点验证程序证明正确性”的要求。

## 6. 机制解释

### 6.1 为什么这里选择 eager 策略

本实验采用 `eager` 策略，即“每次 trap 都保存/恢复浮点上下文”。适用时机是：

- 任务可能在 trap 前已经使用了 FPU；
- trap 可能来自异步中断，无法假设用户态此刻没有活跃浮点状态；
- 希望上下文切换逻辑简单、可验证、没有额外的延迟启用分支。

如果改成 `lazy` 策略，通常需要：

- 额外的“任务是否用过 FPU”状态位；
- 首次 FPU 使用异常或显式 FS 状态管理；
- 更复杂的按需保存/恢复逻辑。

本实验不追求最省开销，而是优先保证正确性和证据清晰，因此选 `eager`。

### 6.2 何时需要保存浮点上下文

在本实现里，只要进入 trap，就认为“此时用户态浮点状态可能仍然活跃”，因此需要保存：

1. 用户态执行 `ecall` 进入内核时；
2. 用户态被 `mtime` 定时器中断异步抢占时；
3. 内核准备把当前任务切走、恢复另一个任务前。

也就是说，保存的真正边界是“特权级切换和任务切换边界”，而不是“只有显式调用浮点函数时才保存”。

### 6.3 为什么必须包含 `fcsr`

仅保存 `f0-f31` 还不够，因为浮点舍入模式和异常标志保存在 `fcsr` 中。  
如果两个任务共享浮点寄存器但没有恢复各自的 `fcsr`，即使寄存器值本身没有被破坏，也可能因为状态寄存器不一致导致后续计算结果偏离预期。

因此，本实验把 `fcsr` 与 `f0-f31` 一起纳入 `TrapFrame` 并在汇编中显式保存/恢复。

### 6.4 为什么校验值能证明内核正确

实验判据是“并发运行时的最终 64 位校验值必须与单任务参考值逐位相等”。

这里的优势是：

- 参考值与并发值来自同一份二进制、同一台 QEMU、同样的输入；
- 参考阶段与并发阶段唯一关键差异就是“是否发生抢占和上下文切换”；
- 所以一旦并发值失配，就能直接指向浮点上下文保存/恢复问题。

## 7. 验收检查对应关系

1. Trap 上下文或进程上下文中增加了 `f0-f31` 以及 `fcsr` 的空间：
   - [trap.rs](/root/os_experiments/lab3/kernel_task3/src/trap.rs) 的 `TrapFrame` 包含 `f: [u64; 32]` 和 `fcsr`；
   - [run_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output.txt) 里打印了 `fp trap frame: 528 bytes, includes f0-f31 (256) + fcsr`。
2. 汇编恢复/保存代码逻辑正确，未遗漏状态寄存器：
   - [boot.S](/root/os_experiments/lab3/kernel_task3/src/boot.S) 的 `trap_entry` 用 `fsd` 保存 `f0-f31`，用 `frcsr` 保存 `fcsr`；
   - `enter_task` 和 trap 返回路径用 `fld` 恢复 `f0-f31`，用 `fscsr` 恢复 `fcsr`；
   - [trap_fp_context_objdump.txt](/root/os_experiments/lab3/kernel_task3/artifacts/trap_fp_context_objdump.txt) 给出了对应反汇编证据。
3. 浮点测试用例在 QEMU 中满分通过：
   - 第一次运行：`timer_interrupts=86 forced_switches=86`，两个任务结果均 `PASS`；
   - 第二次运行：`timer_interrupts=90 forced_switches=90`，两个任务结果仍均 `PASS`；
   - 运行日志分别保存在 [run_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output.txt) 和 [run_output_repeat.txt](/root/os_experiments/lab3/kernel_task3/artifacts/run_output_repeat.txt)。

## 8. 环境说明、限制与未解决问题

- 本实验运行在 QEMU `virt` guest 环境，不是宿主 Linux 进程。
- 工具链版本见 [tool_versions.txt](/root/os_experiments/lab3/kernel_task3/artifacts/tool_versions.txt)。
- 用户态打印走 `write` syscall，多任务并发打印时 UART 文本可能交错；这不影响最终以内核汇总出的校验值作为验收依据。
- 当前实验实现的是 `eager` 策略，没有继续扩展成 `lazy` FPU 上下文管理，这在 README 中已明确说明。
