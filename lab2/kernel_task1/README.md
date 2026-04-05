# LAB2 内核态 Task1: 实现 get_taskinfo 系统调用

## 1. 原始任务说明

### 任务标题

实现 `get_taskinfo` 系统调用

### 任务目标

掌握内核系统调用分发、参数获取与向用户态返回结构化信息的方法。

### 任务要求

1. 扩展内核系统调用表，加入 `get_taskinfo`；
2. 返回当前 task 的 id 与 name（或等价标识）；
3. 保错误路径可控：非法参数/非法指针需返回明确错误码或安全失败。

### 验收检查

1. 内核成功解析相应的系统调用号并进入处理函数；
2. 能正确将内核空间的 task 结构体信息拷贝至用户空间提供的指针地址；
3. 内存边界检查有效，拦截非法用户态指针读写。

## 2. 实验目标与实现思路

由于 [lab2/task1](/root/os_experiments/lab2/task1) 已经用于“用户态调用 `get_taskinfo`”实验，本任务单独放在 [lab2/kernel_task1](/root/os_experiments/lab2/kernel_task1) 中，重点展示内核侧实现：

- 使用显式 syscall 表进行系统调用号分发；
- 从 trap frame 中读取 `a7/a0` 等参数；
- 在内核中维护当前 task 的结构体；
- 通过 `copy_to_user()` 把结构化 `TaskInfo` 安全回填给用户；
- 对空指针、未对齐指针、越界指针返回明确错误码。

为了验证内核路径，实验仍带一个最小 U-mode 测试程序，但重点是内核源码与内核日志，而不是用户态封装本身。

## 3. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab2/kernel_task1/Cargo.toml)：裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab2/kernel_task1/.cargo/config.toml)：固定目标为 `riscv64gc-unknown-none-elf`。
- [linker.ld](/root/os_experiments/lab2/kernel_task1/linker.ld)：镜像、内核栈与用户栈布局。
- [src/boot.S](/root/os_experiments/lab2/kernel_task1/src/boot.S)：启动入口、trap 保存现场和 `enter_user_mode`。
- [src/trap.rs](/root/os_experiments/lab2/kernel_task1/src/trap.rs)：trap 分发逻辑，识别 `ecall` 并交给 syscall dispatcher。
- [src/main.rs](/root/os_experiments/lab2/kernel_task1/src/main.rs)：syscall 表、当前 task、`copy_to_user`、边界检查和用户态测试入口。
- [src/syscall.rs](/root/os_experiments/lab2/kernel_task1/src/syscall.rs)：用户态 `write/get_taskinfo/shutdown` 封装，用于驱动内核测试。
- [src/user_console.rs](/root/os_experiments/lab2/kernel_task1/src/user_console.rs)：用户态输出。
- [src/console.rs](/root/os_experiments/lab2/kernel_task1/src/console.rs)：内核输出。
- [artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/build_output.txt)：构建输出。
- [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt)：QEMU 实际运行输出。

## 4. 内核实现机制

### 4.1 syscall 表扩展

[main.rs](/root/os_experiments/lab2/kernel_task1/src/main.rs) 中定义了 3 项 syscall 号：

- `SYS_WRITE = 0`
- `SYS_GET_TASKINFO = 1`
- `SYS_SHUTDOWN = 2`

对应的 syscall 表如下：

```rust
type SyscallHandler = fn(&mut trap::TrapFrame) -> isize;

const SYSCALL_TABLE: [Option<SyscallHandler>; SYSCALL_TABLE_LEN] = [
    Some(sys_write_handler),
    Some(sys_get_taskinfo_handler),
    Some(sys_shutdown_handler),
];
```

trap 进入内核后，`dispatch_syscall()` 从 `frame.a7` 取 syscall 号，再从表中查找处理函数。找不到则返回 `ENOSYS (-38)`。

### 4.2 当前 task 与返回结构

内核内部使用：

```rust
struct KernelTask {
    id: u64,
    name: [u8; TASK_NAME_LEN],
}
```

当前任务固定为：

```rust
const CURRENT_TASK: KernelTask = KernelTask {
    id: 1,
    name: padded_name(b"kernel_task1_user"),
};
```

返回给用户态的结构体是：

```rust
#[repr(C)]
pub struct TaskInfo {
    pub task_id: u64,
    pub task_name: [u8; TASK_NAME_LEN],
}
```

`sys_get_taskinfo_handler()` 会把 `CURRENT_TASK` 转换成 `TaskInfo`，然后通过 `copy_to_user()` 回填到用户提供的指针地址。

### 4.3 安全拷贝与边界检查

本实验把“可写用户输出缓冲区”约束在用户栈区间 `[__user_stack_bottom, __user_stack_top)` 内。`copy_to_user()` 的核心流程是：

1. 指针不能为 `NULL`，否则返回 `EFAULT (-14)`；
2. 指针必须满足 `TaskInfo` 对齐，否则返回 `EINVAL (-22)`；
3. `[addr, addr + sizeof(TaskInfo))` 必须完整落在用户栈区间内；
4. 校验通过后，才执行 `ptr::write(dst, *src)`。

这一步是本任务最关键的内核安全点：即使没有 MMU，也能通过软件边界检查保证“只向我们允许的用户缓冲区写数据”。

### 4.4 为什么 `write` 的输入检查和 `get_taskinfo` 输出检查不完全一样

本实验中用户态字符串字面量位于镜像地址空间内，因此 `write` 的输入校验允许读取 `[0x8000_0000, __image_end)` 范围的用户可读数据；但 `get_taskinfo` 是向用户写结构体，风险更高，因此只允许写到用户栈中的显式输出缓冲区。

这是一种教学化的最小策略，不是完整操作系统中的最终内存模型。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab2/kernel_task1
```

构建：

```bash
cargo build
```

运行 QEMU：

```bash
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab2_kernel_task1
```

查看保存的日志：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab2_kernel_task1 v0.1.0 (/root/os_experiments/lab2/kernel_task1)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
```

### 6.2 QEMU 实际输出

以下内容来自 [artifacts/run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt)：

```text
[kernel] booted in M-mode
[kernel] current task template: id=1 name=kernel_task1_user
[kernel] syscall table ready: write=0, get_taskinfo=1, shutdown=2
[user] kernel-side get_taskinfo demo started
[kernel] dispatch syscall nr=1 -> sys_get_taskinfo(user_ptr=0x8000bd78)
[kernel] copied task info to user: id=1 name=kernel_task1_user
[user] valid pointer copied task info: id=1, name=kernel_task1_user
[kernel] dispatch syscall nr=1 -> sys_get_taskinfo(user_ptr=0x0)
[kernel] rejected get_taskinfo user pointer 0x0 with -14
[user] null pointer result: bad user pointer (-14)
[kernel] dispatch syscall nr=1 -> sys_get_taskinfo(user_ptr=0x1)
[kernel] rejected get_taskinfo user pointer 0x1 with -22
[user] misaligned pointer result: invalid argument (-22)
[kernel] dispatch syscall nr=1 -> sys_get_taskinfo(user_ptr=0x8000bfb0)
[kernel] rejected get_taskinfo user pointer 0x8000bfb0 with -14
[user] past-stack pointer result: bad user pointer (-14)
[kernel] user requested shutdown with code 0
```

## 7. 验收检查对应关系

1. 内核成功解析 syscall 号并进入处理函数：
   - 日志中明确出现 `dispatch syscall nr=1 -> sys_get_taskinfo(...)`；
   - 说明内核从 `a7` 解析出了 `SYS_GET_TASKINFO = 1`，并成功进入对应 handler。
2. 正确把内核 task 信息拷贝到用户空间：
   - 日志显示 `copied task info to user: id=1 name=kernel_task1_user`；
   - 随后用户态打印 `valid pointer copied task info: id=1, name=kernel_task1_user`，证明回填成功且内容正确。
3. 内存边界检查有效：
   - `NULL` 指针返回 `-14 (EFAULT)`；
   - 未对齐指针 `0x1` 返回 `-22 (EINVAL)`；
   - 超出用户栈顶的指针 `0x8000bfb0` 返回 `-14 (EFAULT)`。

## 8. 结果分析

### 8.1 成功路径

合法用户栈指针 `0x8000bd78` 被内核接受并回填 `TaskInfo`。这表明：

- trap 已正确保存寄存器并把 `a0` 传到 handler；
- syscall 表项 `nr=1` 工作正常；
- `copy_to_user()` 能把结构体从内核逻辑对象安全写回用户地址。

### 8.2 错误路径

三种错误场景分别覆盖了不同的防御点：

1. `NULL`：验证最基础的空指针防御；
2. `0x1`：验证对齐检查，防止错误地址被当作结构体目标；
3. `__user_stack_top`：验证边界检查，防止把数据写到允许区间之外。

这些错误都没有导致内核崩溃，而是返回明确错误码并继续运行，符合“错误路径可控”的目标。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境完成，使用：
  - `rustc 1.94.1`
  - `cargo 1.94.1`
  - `qemu-system-riscv64 10.0.8`
- 本回合未在第二台原生 Linux 服务器复现。
- 该实验仍是教学化最小裸机环境，没有分页和真实用户/内核虚拟地址空间；
- 因此这里的“用户空间边界检查”是软件定义的安全策略，重点用于展示 syscall 参数验证和结构化返回，而不是完整 OS 的硬件隔离实现。
