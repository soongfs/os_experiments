# LAB2 Task3: 裸机调用栈打印

## 1. 原始任务说明

### 任务标题

裸机调用栈打印

### 任务目标

理解在无标准库环境下的调试难题，掌握基于寄存器/栈帧的底层回溯方法。

### 任务要求

1. 在裸机环境下实现调用栈打印（例如读取 frame pointer 并沿栈回溯）；
2. 输出至少 3 层调用关系的返回地址；
3. 在实验记录中解释：为何该方法依赖编译器是否保留帧指针，以及可能的失效场景。

### 验收检查

1. 提供含汇编/底层指针操作的实现源码；
2. QEMU 终端能输出连续的栈帧返回地址（PC 值）；
3. 报告解释开启寄存器优化（如 `-O2` 且省略帧指针）时回溯失效的原理。

## 2. 实验目标与实现思路

本实验在 [lab2/task3](/root/os_experiments/lab2/task3) 中实现一个最小 RISC-V 裸机环境：内核从 M-mode 启动，通过 `mret` 切换到 U-mode 用户程序；用户程序构造 `user_entry -> trace_root -> trace_mid -> trace_leaf -> print_stack_trace` 的多层调用链，然后在 `print_stack_trace()` 中直接读取 `s0/fp`，按 RISC-V 常见栈帧布局沿链回溯。

这次实验的关键点不是“打印符号名”，而是“在没有标准库回溯支持的情况下，仅靠寄存器和栈内存结构打印返回地址”。因此：

- 用户态输出仍通过 syscall `write` 实现；
- 栈回溯逻辑全部在 U-mode 内完成；
- 为了让帧链稳定，编译配置显式打开 `force-frame-pointers=yes`，并对调用链函数使用 `#[inline(never)]`。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/task3/Cargo.toml)：裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab2/task3/.cargo/config.toml)：固定 `riscv64gc-unknown-none-elf`，并显式开启 `force-frame-pointers=yes`。
- [linker.ld](/root/os_experiments/lab2/task3/linker.ld)：镜像布局与用户/内核栈区域。
- [src/boot.S](/root/os_experiments/lab2/task3/src/boot.S)：启动入口、trap 保存现场和 `enter_user_mode`。
- [src/console.rs](/root/os_experiments/lab2/task3/src/console.rs)：内核 UART 输出。
- [src/syscall.rs](/root/os_experiments/lab2/task3/src/syscall.rs)：用户态 `write/exit` syscall 封装。
- [src/trap.rs](/root/os_experiments/lab2/task3/src/trap.rs)：trap 分发逻辑。
- [src/user_console.rs](/root/os_experiments/lab2/task3/src/user_console.rs)：用户态格式化输出。
- [src/main.rs](/root/os_experiments/lab2/task3/src/main.rs)：用户态调用链、帧指针读取、栈帧遍历和最小内核逻辑。
- [artifacts/build_output.txt](/root/os_experiments/lab2/task3/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab2/task3/artifacts/run_output.txt)：QEMU 实际运行输出。
- [artifacts/symbols.txt](/root/os_experiments/lab2/task3/artifacts/symbols.txt)：关键函数符号地址。
- [artifacts/frame_pointer_objdump.txt](/root/os_experiments/lab2/task3/artifacts/frame_pointer_objdump.txt)：关键函数反汇编片段，展示 `ra/s0` 入栈和 `s0` 建帧。

## 4. 实现机制

### 4.1 用户态如何读取帧指针

在 [main.rs](/root/os_experiments/lab2/task3/src/main.rs) 的 `print_stack_trace()` 中，直接使用内联汇编读取当前 `s0`：

```rust
unsafe {
    asm!("mv {}, s0", out(reg) fp, options(nostack, nomem, preserves_flags));
}
```

这一步满足了“含汇编/底层指针操作实现源码”的验收要求。

### 4.2 为什么可以从 `fp-16` 和 `fp-8` 取回上一帧

在当前编译配置下，RISC-V 函数序言会形成类似布局：

```text
addi sp, sp, -frame_size
sd   ra, frame_size-8(sp)
sd   s0, frame_size-16(sp)
addi s0, sp, frame_size
```

因此当前 `fp` 指向的是“本帧顶部”，而：

- `fp - 16` 处保存上一帧的 `s0`，也就是 `previous_fp`
- `fp - 8` 处保存返回地址 `ra`

实验里的 `FrameRecord` 正是按照这个布局读取：

```rust
#[repr(C)]
struct FrameRecord {
    previous_fp: usize,
    return_address: usize,
}
```

随后通过 `ptr::read((fp - 16) as *const FrameRecord)` 取得上一帧信息。

### 4.3 回溯终止条件

为避免把损坏帧链当成有效栈帧继续遍历，代码做了几类基本检查：

1. 当前 `fp` 必须位于用户栈区间 `[__user_stack_bottom, __user_stack_top]` 内；
2. `fp` 必须按机器字对齐；
3. `previous_fp` 必须严格大于当前 `fp`，否则说明帧链断裂或已到栈顶；
4. `ra` 必须落在镜像地址范围 `[0x8000_0000, __image_end)` 内。

这些检查并不等价于完整调试器的展开器，但足以支撑教学实验的稳定回溯。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/task3
```

构建：

```bash
cargo build
```

运行 QEMU：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_task3
```

查看关键符号地址：

```bash
cat artifacts/symbols.txt
```

查看帧指针相关反汇编：

```bash
cat artifacts/frame_pointer_objdump.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab2/task3/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab2_task3 v0.1.0 (/root/os_experiments/lab2/task3)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
```

### 6.2 QEMU 实际输出

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab2/task3/artifacts/run_output.txt)：

```text
[kernel] booted in M-mode
[kernel] launching stack trace demo in U-mode
[user] stack trace demo started
[user] frame pointer walk begins: fp=0x8000b690, stack=[0x80007780, 0x8000b780)
[user] frame#00: fp=0x8000b690 prev_fp=0x8000b6e0 ra=0x80000e6e
[user] frame#01: fp=0x8000b6e0 prev_fp=0x8000b730 ra=0x80001600
[user] frame#02: fp=0x8000b730 prev_fp=0x8000b770 ra=0x80000eae
[user] frame#03: fp=0x8000b770 prev_fp=0x8000b780 ra=0x800016b8
[user] frame#04: fp=0x8000b780 prev_fp=0x80007780 ra=0x8000168a
[user] frame pointer walk finished after 5 frame(s)
[kernel] user requested exit with code 0
```

从输出可见，QEMU 终端连续打印了 5 层返回地址，显然满足“至少 3 层调用关系的返回地址”这一验收条件。

### 6.3 地址与函数的对应关系

[artifacts/symbols.txt](/root/os_experiments/lab2/task3/artifacts/symbols.txt) 中的关键符号如下：

```text
0000000080000e30 t lab2_task3::trace_leaf::hd53c09e157711e07
0000000080000e82 t lab2_task3::trace_root::h2ac938ecb0710440
0000000080000f8e t lab2_task3::print_stack_trace::hb2e01e702d84565b
00000000800015c2 t lab2_task3::trace_mid::h6605153df9c92865
000000008000168e T user_entry
```

将运行时地址和符号表对照：

1. `frame#00` 的 `ra=0x80000e6e`：
   - 落在 `trace_leaf` 体内；
   - 对应 [frame_pointer_objdump.txt](/root/os_experiments/lab2/task3/artifacts/frame_pointer_objdump.txt) 中 `jalr ... <print_stack_trace>` 之后的下一条指令，说明这是 `print_stack_trace -> trace_leaf` 的返回地址。
2. `frame#01` 的 `ra=0x80001600`：
   - 对应 `trace_mid` 中调用 `trace_leaf` 之后的地址；
   - 说明这是 `trace_leaf -> trace_mid` 的返回地址。
3. `frame#02` 的 `ra=0x80000eae`：
   - 对应 `trace_root` 中调用 `trace_mid` 之后的地址；
   - 说明这是 `trace_mid -> trace_root` 的返回地址。
4. `frame#03` 的 `ra=0x800016b8`：
   - 对应 `user_entry` 中调用 `trace_root` 之后的地址；
   - 说明这是 `trace_root -> user_entry` 的返回地址。
5. `frame#04` 的 `ra=0x8000168a`：
   - 对应内核中调用 `enter_user_mode` 的位置，而不是另一个正常用户函数；
   - 这是因为 `user_entry` 是通过 `mret` 进入 U-mode，而不是通过普通 `call` 指令被上层用户函数调用，所以最外层帧会带有“用户态入口过渡残留”的特征。

因此，前 4 层已经足够证明用户态调用链 `print_stack_trace <- trace_leaf <- trace_mid <- trace_root <- user_entry` 被成功恢复；最外层第 5 帧则反映了裸机切换到 U-mode 时没有标准用户 caller 的事实。

### 6.4 反汇编中的帧指针证据

[artifacts/frame_pointer_objdump.txt](/root/os_experiments/lab2/task3/artifacts/frame_pointer_objdump.txt) 中可以直接看到函数序言：

```text
0000000080000e30 <lab2_task3::trace_leaf::hd53c09e157711e07>:
80000e30: 715d          addi    sp, sp, -0x50
80000e32: e486          sd      ra, 0x48(sp)
80000e34: e0a2          sd      s0, 0x40(sp)
80000e36: 0880          addi    s0, sp, 0x50

0000000080000f8e <lab2_task3::print_stack_trace::hb2e01e702d84565b>:
80000f8e: 7125          addi    sp, sp, -0x1a0
80000f90: ef06          sd      ra, 0x198(sp)
80000f92: eb22          sd      s0, 0x190(sp)
80000f94: 1300          addi    s0, sp, 0x1a0
80000f96: 8522          mv      a0, s0
```

这里清楚表明：

- `ra` 被保存到当前栈帧；
- `s0` 被保存并重新设为当前帧指针；
- `print_stack_trace()` 里确实直接使用了 `s0`。

## 7. 为什么依赖“保留帧指针”

### 7.1 省略帧指针时为什么会失效

本实验的回溯算法默认每一层函数都满足：

1. `s0/fp` 是当前帧基址；
2. `fp-16` 保存上一帧 `fp`；
3. `fp-8` 保存当前帧返回地址。

如果编译器在优化时省略帧指针，例如 `-O2` 且允许 omit frame pointer，则常见后果是：

1. 编译器不再执行 `sd s0, ...` / `addi s0, sp, ...` 这套建帧序言；
2. `s0` 可能被当作普通通用寄存器使用，而不再代表稳定的帧链头；
3. 栈中根本不存在 `fp -> previous_fp -> previous_fp` 这种链式结构。

这时本实验的遍历逻辑会从一个“不是帧指针的值”出发，把错误内存解释成 `FrameRecord`，最终表现为：

- 回溯层数异常少；
- 打印出明显不合理的地址；
- 或很快触发边界检查而提前停止。

### 7.2 除了省略帧指针，还有哪些失效场景

即便保留了帧指针，以下情况也可能导致回溯不完整或错误：

1. 函数被内联：原本独立的一层调用在机器码里消失，帧数自然变少；
2. 尾调用优化：函数返回前直接跳到下一函数，当前帧不会按普通调用方式保留；
3. 手写汇编函数未按同样的栈帧约定保存 `ra/s0`；
4. 栈被破坏或越界写覆盖；
5. trap/异常上下文切换没有按相同规则组织用户态帧链。

因此，基于 `fp` 的回溯是一个“依赖 ABI、编译器策略和调用约定”的方法，不是无条件可靠的通用真相。

## 8. 验收检查对应关系

1. 含汇编/底层指针操作的实现源码：
   - [main.rs](/root/os_experiments/lab2/task3/src/main.rs) 中使用 `asm!("mv {}, s0", ...)` 读取帧指针；
   - 同文件中使用裸指针和 `ptr::read()` 读取 `fp-16`、`fp-8` 的栈帧内容。
2. QEMU 输出连续栈帧返回地址：
   - [run_output.txt](/root/os_experiments/lab2/task3/artifacts/run_output.txt) 中连续打印了 `frame#00` 到 `frame#04` 的 `ra`。
3. 报告解释优化导致失效的原理：
   - 本文第 7 节详细说明了省略帧指针、内联和尾调用优化如何破坏帧链。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境完成，使用：
  - `rustc 1.94.1`
  - `cargo 1.94.1`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器复现。
- 本实验是教学化最小裸机环境，并未实现 DWARF unwind、符号解析器或真正的多地址空间隔离；
- 因此这里验证的是“基于帧指针的低层回溯方法”本身，而不是完整调试器的全功能调用栈恢复能力。
