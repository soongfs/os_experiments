# LAB2 Task1: 访问 get_taskinfo 并打印结果

## 1. 原始任务说明

### 任务标题

访问 `get_taskinfo` 并打印结果

### 任务目标

理解“系统调用”作为用户态进入内核态的唯一受控入口，掌握用户态封装与参数传递。

### 任务要求

1. 编写裸机/用户态应用，调用新增系统调用 `get_taskinfo`；
2. 打印 task id 与 task name（或等价字段）；
3. 若系统调用返回错误码，需在用户态进行可解释的错误处理（打印错误原因或返回值）。

### 验收检查

1. 成功通过 `ecall` 或对应汇编指令陷入内核；
2. 程序打印出的 task id 和 name 与实际运行状态相符；
3. 测试传入非法参数（如空指针）时，程序不崩溃，而是打印出合适的错误信息。

## 2. 实验目标与实现思路

本任务在 [lab2/task1](/root/os_experiments/lab2/task1) 中实现了一个最小可运行的 RISC-V 裸机实验：

- 内核从 M-mode 启动，初始化 `mtvec` 和 PMP，然后通过 `mret` 切换到 U-mode 用户任务。
- 用户态不直接调用内核函数，而是通过封装好的 syscall wrapper 执行 `ecall`。
- 内核 trap 入口把用户寄存器保存到内核栈，在 Rust 侧解析 syscall 号和参数。
- 新增 `get_taskinfo` 系统调用后，内核向用户给出的缓冲区回填 `task_id` 和 `task_name`。
- 用户态分别测试：
  - 正常传入有效指针，打印任务信息；
  - 传入空指针，收到错误码 `-14` 并打印可解释错误信息，而不是崩溃。

为了让“进入内核的唯一受控入口”更明确，用户态输出也走了辅助 `write` syscall；因此从用户态进入内核的路径统一都是 `ecall`。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/task1/Cargo.toml)：Rust 裸机工程配置，开启 `panic = "abort"`。
- [.cargo/config.toml](/root/os_experiments/lab2/task1/.cargo/config.toml)：固定目标三元组为 `riscv64gc-unknown-none-elf`，并指定链接脚本。
- [linker.ld](/root/os_experiments/lab2/task1/linker.ld)：定义镜像装载地址、`.bss`、内核栈和用户栈。
- [src/boot.S](/root/os_experiments/lab2/task1/src/boot.S)：启动入口、`enter_user_mode` 和 trap 汇编入口；使用 `mscratch` 在 trap 时切换到内核栈。
- [src/main.rs](/root/os_experiments/lab2/task1/src/main.rs)：系统整体入口、PMP 配置、`get_taskinfo` 实现、用户指针校验、QEMU 退出逻辑。
- [src/trap.rs](/root/os_experiments/lab2/task1/src/trap.rs)：`TrapFrame` 定义和 `handle_trap()`，负责识别 `UserEnvCall` 并分发 syscall。
- [src/syscall.rs](/root/os_experiments/lab2/task1/src/syscall.rs)：用户态 syscall 封装，内部执行 `ecall`。
- [src/console.rs](/root/os_experiments/lab2/task1/src/console.rs)：内核 UART 输出。
- [src/user_console.rs](/root/os_experiments/lab2/task1/src/user_console.rs)：用户态格式化输出，底层调用 `write` syscall。
- [artifacts/run_output.txt](/root/os_experiments/lab2/task1/artifacts/run_output.txt)：QEMU 实际运行输出。
- [artifacts/objdump_ecall.txt](/root/os_experiments/lab2/task1/artifacts/objdump_ecall.txt)：包含 `ecall` 的反汇编片段。
- [artifacts/build_output.txt](/root/os_experiments/lab2/task1/artifacts/build_output.txt)：本次构建输出。

关键实现点：

1. `TaskInfo` 结构体定义了 `task_id` 和固定长度的 `task_name` 缓冲区。
2. `CURRENT_TASK` 表示当前运行用户任务，实验中固定为 `id = 1`、`name = "lab2_task1_user"`。
3. `sys_get_taskinfo()` 会检查：
   - 指针非空；
   - 地址按 `TaskInfo` 对齐；
   - 缓冲区位于允许的用户内存范围内。
4. 校验失败时返回 `-14 (EFAULT)` 或 `-22 (EINVAL)`；用户态调用 `describe_error()` 打印解释。
5. 初次运行时 U-mode 取指失败，根因是未配置 PMP；最终通过开放一条覆盖整个实验内存空间的 NAPOT 规则解决。

## 4. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/task1
```

构建：

```bash
cargo build
```

运行 QEMU：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_task1
```

查看已保存的运行日志：

```bash
cat artifacts/run_output.txt
```

查看带 `ecall` 的反汇编证据：

```bash
cargo objdump --bin lab2_task1 -- --demangle -d | rg -n -C 8 "invoke_syscall3|get_taskinfo|ecall"
```

## 5. 本次实际运行结果

### 构建结果

`cargo build` 的实际输出已保存到 [artifacts/build_output.txt](/root/os_experiments/lab2/task1/artifacts/build_output.txt)：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### QEMU 实际输出

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab2/task1/artifacts/run_output.txt)：

```text
[kernel] booted in M-mode
[kernel] launching user task id=1 name=lab2_task1_user
[user] task started in U-mode
[kernel] get_taskinfo -> id=1 name=lab2_task1_user
[user] get_taskinfo success: id=1, name=lab2_task1_user
[kernel] get_taskinfo rejected user pointer 0x0 with -14
[user] null pointer call rejected: bad user pointer (-14)
[user] task finished cleanly
[kernel] user requested shutdown with code 0
```

可以看到：

1. 内核启动后声明当前用户任务为 `id=1`、`name=lab2_task1_user`；
2. 用户态通过 `get_taskinfo` 打印出的结果与内核当前任务状态一致；
3. 传入空指针时没有崩溃，而是返回 `-14` 并打印 `bad user pointer`。

### `ecall` 反汇编证据

以下片段来自 [artifacts/objdump_ecall.txt](/root/os_experiments/lab2/task1/artifacts/objdump_ecall.txt)：

```text
0000000080000ee8 <lab2_task1::syscall::get_taskinfo::hd2e27fb9ada967dc>:
80000ee8: 1141          addi    sp, sp, -0x10
80000eea: e406          sd      ra, 0x8(sp)
80000eec: 85aa          mv      a1, a0
80000ef0: 4505          li      a0, 0x1
80000ef6: 00000097      auipc   ra, 0x0
80000efa: 07a080e7      jalr    0x7a(ra) <lab2_task1::syscall::invoke_syscall3::heda18a77e670d003>

0000000080000f70 <lab2_task1::syscall::invoke_syscall3::heda18a77e670d003>:
80000f80: 88aa          mv      a7, a0
80000f8e: 6562          ld      a0, 0x18(sp)
80000f90: 00000073      ecall
```

这说明用户态封装最终确实执行了 `ecall` 指令，满足“通过 `ecall` 陷入内核”的验收要求。

## 6. 机制解释

### 6.1 用户态如何进入内核

1. 用户态调用 [src/syscall.rs](/root/os_experiments/lab2/task1/src/syscall.rs) 中的 `get_taskinfo()`。
2. `invoke_syscall3()` 把 syscall 号放入 `a7`，参数放入 `a0/a1/a2`，执行 `ecall`。
3. 硬件将控制权切回 M-mode，并跳转到 `mtvec` 指向的 [src/boot.S](/root/os_experiments/lab2/task1/src/boot.S) trap 入口。
4. trap 入口先通过 `csrrw sp, mscratch, sp` 把栈从用户栈切到内核栈，再保存通用寄存器和 `mepc`。
5. Rust 侧 [src/trap.rs](/root/os_experiments/lab2/task1/src/trap.rs) 读取 `mcause`，识别 `8` 号异常，即 `Environment call from U-mode`。
6. 内核执行对应 syscall 逻辑，设置返回值到保存的 `a0`，最后通过 `mret` 回到用户态。

### 6.2 `get_taskinfo` 如何回填用户缓冲区

1. 内核把用户传入的 `a0` 解释为 `*mut TaskInfo`。
2. `validated_user_mut::<TaskInfo>()` 做指针合法性检查。
3. 检查通过后，内核把 `CURRENT_TASK` 拷贝到用户缓冲区。
4. 用户态读取自己缓冲区中的 `task_id` 和 `task_name`，并打印结果。

### 6.3 为什么空指针不会崩溃

如果用户传入 `NULL`，`validated_user_mut()` 会直接返回 `EFAULT (-14)`，内核不会解引用该指针，因此不会因为非法访问而崩溃。用户态收到负返回值后，调用 `describe_error()` 打印：

```text
[user] null pointer call rejected: bad user pointer (-14)
```

## 7. 验收检查对应关系

1. `ecall` 陷入内核：
   - 由 [src/syscall.rs](/root/os_experiments/lab2/task1/src/syscall.rs) 的 `invoke_syscall3()` 执行；
   - [artifacts/objdump_ecall.txt](/root/os_experiments/lab2/task1/artifacts/objdump_ecall.txt) 中可见 `00000073 ecall`。
2. 打印出的 task id/name 与实际运行状态相符：
   - 内核启动时打印 `id=1 name=lab2_task1_user`；
   - 用户态随后打印相同的 `id=1, name=lab2_task1_user`。
3. 非法参数不崩溃：
   - 空指针测试得到 `-14`；
   - 用户态明确打印 `bad user pointer (-14)`。

## 8. 环境说明、限制与未解决问题

- 本次实验在当前 Linux 环境完成，版本如下：
  - `rustc 1.94.1 (e408947bf 2026-03-25)`
  - `cargo 1.94.1 (29ea6fb6a 2026-03-24)`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器上再次复现。
- 实验未启用分页或真正的用户态虚拟地址空间，内核和用户共享同一物理地址空间；因此这里的“用户指针校验”是一个教学化、最小化实现。
- 为了让 U-mode 在 `-bios none` 的 QEMU 环境下正常取指，实验显式配置了一条放宽的 PMP 规则。这是最小裸机环境必须补上的一环，但并不代表真实操作系统中的精细权限模型。
