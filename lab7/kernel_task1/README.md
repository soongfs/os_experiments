# LAB7 内核态 task1：共享内存机制

## 原始任务

> 完成LAB7 内核态task1：共享内存机制
> 目标：在内核中支持跨进程共享页映射，提供明确的创建/映射/释放语义。
> 要求：
> 1. 实现共享内存的创建与映射接口；
> 2. 支持至少两个进程同时映射并读写；
> 3. 提供用户态验证程序与可观察结果。
> 验收检查：
> 1. 新增共享内存结构，脱离单进程管理范围；
> 2. 进程 A 映射某物理页至虚拟地址 X，进程 B 映射同物理页至地址 Y；
> 3. A 向 X 写入内容后，B 立马能通过 Y 读出。

## 实验环境与实现思路

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现采用最小化两进程模型：

- `parent(pid=0)`：创建共享页，把它映射到用户虚拟地址 `X=0x402000`，先写入初值。
- `child(pid=1)`：继承最小内核调度模型后，把同一共享页映射到不同用户虚拟地址 `Y=0x405000`，先读父进程写入的值，再写入新值。
- `parent` 再次运行时从 `X` 读取，验证能立刻看到 `child` 通过 `Y` 写入的结果。

内核里单独维护了 `SharedRegion` 表，而不是把共享页挂在某个单独进程私有结构里：

- `SYS_SHM_CREATE`：分配物理页并创建共享区域句柄
- `SYS_SHM_MAP`：把共享区域物理页映射到指定进程的指定用户虚拟地址

这是一份课程验证用最小实现，重点是证明“内核拥有共享页对象 + 两进程不同 VA 映射同一 PA + 读写立刻可见”。本实现没有扩展到：

- 多页共享段
- 引用计数归零后的真正释放接口
- 权限细分与只读映射
- 完整 Unix 风格 `shmget/shmat/shmdt/shmctl`

## 文件列表

- `src/main.rs`：页表、最小两进程模型、共享区域表、`SYS_SHM_CREATE/SYS_SHM_MAP`、用户态观测汇总和验收日志。
- `src/boot.S`：M/S/U 切换、trap 保存恢复，以及用户态 parent/child 共享内存验证程序。
- `src/trap.rs`：TrapFrame 定义和 trap vector 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局和 kernel/trap 栈。
- `artifacts/build_output.txt`：构建日志。
- `artifacts/run_output.txt`：第一次完整运行日志。
- `artifacts/run_output_repeat.txt`：第二次完整运行日志。
- `artifacts/shm_kernel_objdump.txt`：反汇编证据。
- `artifacts/shm_kernel_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链与 QEMU 版本。

## 关键机制说明

### 1. 共享内存结构脱离单进程管理

内核新增了：

```rust
struct SharedRegion {
    pa: usize,
    parent_mapped: bool,
    child_mapped: bool,
}
```

以及：

- `SHARED_REGIONS: [SharedRegion; MAX_SHARED_REGIONS]`
- `SHARED_REGION_COUNT`

`SYS_SHM_CREATE` 会在这个全局表中创建条目并返回句柄；后续 `SYS_SHM_MAP` 按句柄查找，而不是从单个进程私有页表中“猜”共享页归属。这满足“共享内存对象脱离单进程管理范围”的要求。

### 2. 两个进程映射同一物理页到不同虚拟地址

本实验中：

- 父进程把共享页映射到 `X=0x402000`
- 子进程把同一共享页映射到 `Y=0x405000`

两个进程页表中的 `leaf_pa` 最终都指向同一个物理页 `0x8000b000`，但虚拟地址不同，因此能直接证明“同一 PA，多 VA，多进程”。

### 3. 可观察的数据交换路径

用户态验证程序在 `boot.S` 里的序列是：

1. 父进程 `shm_create`
2. 父进程 `shm_map(handle, X)`
3. 父进程向 `X` 写入 `INITIAL_VALUE`
4. `fork`
5. 子进程 `shm_map(handle, Y)`
6. 子进程从 `Y` 读取，必须读到父写的 `INITIAL_VALUE`
7. 子进程再向 `Y` 写入 `CHILD_WRITE_VALUE`
8. 父进程 `yield` 返回后，从 `X` 再读，必须立刻看到 `CHILD_WRITE_VALUE`

这条链路覆盖了验收里最关键的语义：

- A 写 `X`
- B 读 `Y`
- B 改写 `Y`
- A 再读 `X`
- 整个过程中 `X` 与 `Y` 对应同一物理页

## 构建与运行

在任务目录 `lab7/kernel_task1/` 下执行：

```bash
cargo build
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab7_kernel_task1
cargo objdump --bin lab7_kernel_task1 -- --demangle -d > artifacts/shm_kernel_objdump.txt
cargo nm --bin lab7_kernel_task1 -- --demangle > artifacts/shm_kernel_nm.txt
```

本次归档的证据文件：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/shm_kernel_objdump.txt`
- `artifacts/shm_kernel_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] shared region 0 created pa=0x8000b000
[kernel] pid=0 mapped shared handle=0 va=0x402000 pa=0x8000b000 refcount=2
[pt] shared_map pid=0 va=0x402000 ... leaf_pa=0x8000b000 flags=VRW-U-AD-
[kernel] fork complete
[kernel] pid=1 mapped shared handle=0 va=0x405000 pa=0x8000b000 refcount=3
[pt] shared_map pid=1 va=0x405000 ... leaf_pa=0x8000b000 flags=VRW-U-AD-
[kernel] report pid=1 first=0x1111222233334444 second=0x0c0ffee000000001 third=0x0000000000000000
[kernel] report pid=0 first=0x1111222233334444 second=0x0c0ffee000000001 third=0x0c0ffee000000001
[kernel] final shared page parent_pa=0x8000b000 child_pa=0x8000b000 parent_val=0x0c0ffee000000001 child_val=0x0c0ffee000000001 refcount=3
[kernel] acceptance kernel-owned shared region is created and mapped into both processes: PASS
[kernel] acceptance A writes X and B immediately reads Y from the same physical page: PASS
```

从这份日志可以直接读出：

- 共享页对象先被创建为 `handle=0, pa=0x8000b000`
- 父进程映射 `X=0x402000 -> 0x8000b000`
- 子进程映射 `Y=0x405000 -> 0x8000b000`
- 子进程第一次读取到的是父进程写入的 `0x1111222233334444`
- 子进程写入 `0x0c0ffee000000001` 后，父进程立刻在自己的映射地址上看到同样值
- 最终父子 `leaf_pa` 完全一致，说明并不是“碰巧值相同”，而是确实共享同一物理页

### 重复运行

`artifacts/run_output_repeat.txt` 与第一次结果一致：

- `parent_pa == child_pa == 0x8000b000`
- 父映射地址始终是 `0x402000`
- 子映射地址始终是 `0x405000`
- 两条 acceptance 都稳定为 `PASS`

说明该共享映射行为是稳定可复现的。

### 符号与反汇编证据

`artifacts/shm_kernel_nm.txt` 中的关键符号：

```text
00000000800010a0 T __user_program_start
0000000080000e96 T enter_user_task
0000000080000ff0 T supervisor_trap_entry
00000000800025e6 t lab7_kernel_task1::map_shared_region::...
0000000080003392 t lab7_kernel_task1::create_shared_region::...
0000000080001dae t lab7_kernel_task1::finish_experiment::...
```

`artifacts/shm_kernel_objdump.txt` 中能看到用户态程序确实使用两个不同 VA：

```text
00000000800010a0 <__user_program_start>:
800010a0: 00402437      lui s0, 0x402
800010a4: 004054b7      lui s1, 0x405
...
800010dc: 00543023      sd t0, 0x0(s0)
...
800010f8: 0004ba03      ld s4, 0x0(s1)
80001106: 0064b023      sd t1, 0x0(s1)
...
80001128: 00043b03      ld s6, 0x0(s0)
```

这里可以直接看到：

- `s0 = 0x402000`，即父映射地址 `X`
- `s1 = 0x405000`，即子映射地址 `Y`
- 父先向 `X` 写
- 子从 `Y` 读再向 `Y` 写
- 父最后再从 `X` 读

## 验收检查映射

### 1. 新增共享内存结构，脱离单进程管理范围

证据：

- `src/main.rs` 中的 `SharedRegion`、`SHARED_REGIONS`、`SHARED_REGION_COUNT`
- `run_output.txt` 中的 `shared region 0 created pa=0x8000b000`

结论：

- 已满足。

### 2. 进程 A 映射某物理页至虚拟地址 X，进程 B 映射同物理页至地址 Y

证据：

- `pid=0 mapped shared handle=0 va=0x402000 pa=0x8000b000`
- `pid=1 mapped shared handle=0 va=0x405000 pa=0x8000b000`
- `parent_final` 与 `child_final` 的 `leaf_pa` 都是 `0x8000b000`

结论：

- 已满足。

### 3. A 向 X 写入内容后，B 立马能通过 Y 读出

证据：

- 子进程报告：
  `first=0x1111222233334444`
  说明子进程第一次从 `Y` 读到的就是父进程先前写入 `X` 的值。
- 子进程写入后，父进程报告：
  `second=0x0c0ffee000000001`
  说明父进程随后从 `X` 立刻看到了子进程通过 `Y` 写入的值。

结论：

- 已满足。

## 环境信息

来自 `artifacts/tool_versions.txt`：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 说明与限制

- 这是课程验证用最小共享页机制，不是完整 System V / POSIX 共享内存子系统。
- 共享页当前由内核全局表持有，验证重点是“创建/映射/跨进程立刻可见”；释放语义没有扩展到完整回收接口。
- 为避免与用户栈冲突，子进程共享映射地址使用 `0x405000`，不再与 `0x403000-0x403fff` 的用户栈页重叠。
