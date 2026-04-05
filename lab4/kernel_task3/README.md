# LAB4 内核态 task3：COW 机制

## 原始任务

> 完成LAB4 内核态task3：COW 机制
> 目标：实现 fork 后的页共享与写入时复制，保证语义正确与资源利用率。
> 要求：
> 1. fork 时父子共享物理页并设置只读；
> 2. 写入触发缺页，完成物理页复制并改为可写；
> 3. 给出验证：父写不影响子、子写不影响父。
> 验收检查：
> 1. fork 极快完成，仅复制并置空页表项写权限，增加物理页引用计数；
> 2. 处理 Store Page Fault 时，若引用计数 > 1，申请新物理页拷贝数据并重新映射可写权限。

## 实验目标与方案

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现采用最小化的两进程教学内核模型：

- 只有 2 个进程槽位：`parent(pid=0)` 和 `child(pid=1)`
- 每个进程有自己的一套 `Sv39` 根页表 / L1 / L0 用户页表
- 用户地址空间只保留 1 个需要验证的匿名可写数据页 `0x00401000`
- 用户代码页是共享的 `RX` 页，不参与 COW

用户程序通过极简 `ecall` ABI 与内核配合：

- `SYS_FORK`：创建子进程，但不复制匿名页内容
- `SYS_YIELD`：父进程让出执行，先让子进程触发第一次 COW
- `SYS_REPORT`：把用户态观测到的值回传给内核
- `SYS_EXIT`：结束当前进程

核心设计是：

1. `fork` 时父子数据页都改成 `R/U/A + COW`，清掉 `W` 位；
2. 父子 PTE 仍指向同一物理页，并把该页引用计数从 `1` 增加到 `2`；
3. 子进程第一次写该页时，触发 `Store Page Fault`，由于 `refcount_before=2`，内核分配新页、复制 4 KiB 数据并把子进程重新映射为可写；
4. 父进程之后再写时，旧页已只剩父进程独占，`refcount_before=1`，内核不再复制，只把父进程该页恢复为可写。

这样可以同时证明：

- `fork` 阶段不复制页内容，只修改页表和引用计数；
- 真正的物理页复制只发生在首次写时；
- 父写不影响子，子写不影响父。

## 文件列表

- `src/main.rs`：两进程模型、`fork/yield/report/exit`、页表切换、COW 缺页处理、物理页引用计数和验收日志。
- `src/boot.S`：M/S/U 入口、trap 保存恢复，以及最小用户态 COW 探测程序。
- `src/trap.rs`：trap frame 与 trap vector 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局和加大的 kernel/trap 栈。
- `artifacts/build_output.txt`：最终成功构建输出。
- `artifacts/run_output.txt`：第一次完整运行日志。
- `artifacts/run_output_repeat.txt`：第二次完整运行日志。
- `artifacts/cow_kernel_objdump.txt`：反汇编证据。
- `artifacts/cow_kernel_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链和 QEMU 版本。

## 关键实现说明

### 1. fork 时的 COW 建立

父进程初始数据页是一个普通可写匿名页：

- `flags=VRW-U-AD-`
- 物理页 `pa=0x8000b000`
- `refcount=1`

执行 `fork` 时，内核做的事情只有：

1. 克隆父进程的页表结构到子进程
2. 修正子进程根页表中指向自己 L1/L0 页表的指针
3. 把父子双方的数据页 PTE 都改成 `VR--U-A-C`
4. 把共享物理页引用计数从 `1` 加到 `2`

这里没有分配新的匿名数据页，也没有做 4 KiB 数据复制。

### 2. Store Page Fault 的两种处理路径

当写只读 COW 页时，内核在 `Store Page Fault` 中区分两种情况：

- `refcount_before > 1`
  - 分配新物理页
  - 复制旧页 4 KiB 内容
  - 当前进程改映射到新页，并恢复 `W`
- `refcount_before == 1`
  - 说明该页已被当前进程独占
  - 不必再复制，只把当前进程 PTE 恢复成可写

本实验里两种路径都被覆盖了：

- 子进程第一次写时走 `copy`
- 父进程随后写时走 `reuse`

### 3. 用户态验证路径

用户程序的执行序列固定如下：

1. `fork`
2. 父、子都先读取同一变量，验证初始值相同
3. 父 `yield` 给子
4. 子写变量，触发第一次 COW copy，并报告写后值
5. 父恢复执行，再次读取变量，确认未受子影响
6. 父写变量，触发第二次 `Store Page Fault`，但因 `refcount=1` 只恢复可写不复制
7. 内核最终同时检查父页和子页的最终物理页与数值

## 构建与运行

在任务目录下执行：

```bash
cargo build
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab4_kernel_task3
cargo objdump --bin lab4_kernel_task3 -- --demangle -d > artifacts/cow_kernel_objdump.txt
cargo nm --bin lab4_kernel_task3 -- --demangle > artifacts/cow_kernel_nm.txt
```

本次归档的完整证据文件：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/cow_kernel_objdump.txt`
- `artifacts/cow_kernel_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] initial anonymous page pa=0x8000b000 refcount=1 value=0x1111222233334444
[pt] parent_data_before_fork ... leaf_pa=0x8000b000 flags=VRW-U-AD-
[kernel] fork complete: alloc_before=1 alloc_after=1 shared_pa=0x8000b000 ref_before=1 ref_after=2
[pt] parent_after_fork ... leaf_pa=0x8000b000 flags=VR--U-A-C
[pt] child_after_fork ... leaf_pa=0x8000b000 flags=VR--U-A-C
[kernel] cow store fault pid=1 action=copy refcount_before=2 old_pa=0x8000b000 new_pa=0x8000c000
[pt] cow_fault_before pid=1 ... leaf_pa=0x8000b000 flags=VR--U-A-C
[pt] cow_fault_after pid=1 ... leaf_pa=0x8000c000 flags=VRW-U-AD-
[kernel] report pid=1 first=0x1111222233334444 second=0xc0ffee0000000001 third=0x0000000000000000
[kernel] cow store fault pid=0 action=reuse refcount_before=1 pa=0x8000b000
[pt] cow_fault_before pid=0 ... leaf_pa=0x8000b000 flags=VR--U-A-C
[pt] cow_fault_after pid=0 ... leaf_pa=0x8000b000 flags=VRW-U-AD-
[kernel] report pid=0 first=0x1111222233334444 second=0x1111222233334444 third=0xa11ce00000000002
[kernel] final values parent=0xa11ce00000000002 child=0xc0ffee0000000001 parent_refcount=1 child_refcount=1
[kernel] acceptance fork shares page, clears W, and only bumps refcount: PASS
[kernel] acceptance store page fault with refcount>1 copies page and remaps writable: PASS
[kernel] acceptance parent/child writes are isolated both ways: PASS
```

从这份日志可以直接读出完整的 COW 链路：

- `fork` 前：只有父进程映射该匿名页，`refcount=1`
- `fork` 后：父子仍指向同一 `pa=0x8000b000`，而且双方都已经去掉 `W` 位并带 `COW`
- 子写时：`refcount_before=2`，因此复制到 `0x8000c000`
- 父写时：旧页已只剩自己，`refcount_before=1`，因此不复制，直接恢复可写
- 最终：父页值是 `0xa11ce00000000002`，子页值是 `0xc0ffee0000000001`，且两个物理页不同

### 重复运行

`artifacts/run_output_repeat.txt` 与第一次运行结果一致：

- `fork` 后仍然是 `shared_pa=0x8000b000 ref_after=2`
- 子写时仍然是 `action=copy refcount_before=2`
- 父写时仍然是 `action=reuse refcount_before=1`
- 三项 acceptance 继续全部为 `PASS`

说明该 COW 行为是稳定可复现的。

### 反汇编证据

`artifacts/cow_kernel_objdump.txt` 中能看到关键控制流和页表刷新路径：

```text
00000000800009f0 <enter_supervisor>:
80000a12: 30200073      mret
0000000080000a16 <enter_user_task>:
80000abc: 10200073      sret
0000000080000ac0 <machine_trap_entry>:
0000000080000b70 <supervisor_trap_entry>:
0000000080000c20 <__user_program_start>:
000000008000324e <lab4_kernel_task3::store_process_report...>:
000000008000364c <lab4_kernel_task3::handle_cow_store_fault...>:
000000008000425c <lab4_kernel_task3::do_fork...>:
800016e2: 12000073      sfence.vma
8000184e: 12050073      sfence.vma a0
```

这说明：

- 内核确实通过 `mret/sret` 在 M/S/U 之间切换；
- 用户态测试程序是独立代码段；
- `do_fork` 和 `handle_cow_store_fault` 都存在独立实现；
- PTE 更新后执行了 `sfence.vma`。

## 机制解释

这份实验里，真正参与 COW 的只有 1 页匿名数据页 `0x00401000`。其语义可以概括为：

- `fork` 并不复制页内容，只复制页表结构
- 父子页表项仍指向同一物理页，但去掉 `W` 位，并把软件位 `COW` 设为 1
- 当任何一方第一次写时，硬件因为 `W=0` 抛出 `Store Page Fault`
- 内核检查旧页引用计数
  - 如果 `>1`，复制
  - 如果 `==1`，直接恢复可写

因此，COW 的资源节省点在于：

- 在 `fork` 那一刻，完全不复制 4 KiB 数据页
- 只有“真的有人写”时才做复制

本实验中：

- 子进程第一次写触发真正的 COW copy
- 父进程随后写时，因为旧页已经独占，所以无需第二次复制

这正是“按写触发复制”的典型优化。

## 验收清单

- [x] `fork` 极快完成，仅复制并置空页表项写权限，增加物理页引用计数。
  - 证据：`fork complete: alloc_before=1 alloc_after=1`，说明 `fork` 没有新增匿名数据页分配；
  - 证据：`parent_after_fork` 和 `child_after_fork` 都是 `flags=VR--U-A-C`；
  - 证据：父子 `leaf_pa` 都是 `0x8000b000`，且 `ref_after=2`。
- [x] 处理 `Store Page Fault` 时，若引用计数 `> 1`，申请新物理页拷贝数据并重新映射可写权限。
  - 证据：子进程 fault 日志明确显示 `action=copy refcount_before=2 old_pa=0x8000b000 new_pa=0x8000c000`；
  - 证据：子进程 `cow_fault_after` 已变成新的 `leaf_pa=0x8000c000 flags=VRW-U-AD-`。
- [x] 父写不影响子、子写不影响父。
  - 证据：子写后，父报告 `after_child=0x1111222233334444`，仍然是初值；
  - 证据：最终父值是 `0xa11ce00000000002`，子值是 `0xc0ffee0000000001`，且两边 `leaf_pa` 不同。

## 环境信息

来自 `artifacts/tool_versions.txt`：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 复现限制与说明

- 这是教学内核里的极简 `fork` 实验，不是完整 Unix 进程管理器。
- 本实验只把 1 页匿名可写数据页作为 COW 对象，目的是最小化地验证 `fork -> shared readonly -> store fault -> copy` 的核心语义。
- 为了保留较丰富的 trap 日志，本任务把 kernel stack 和 supervisor trap stack 做了放大；这不影响 COW 机制本身，只是为了稳定保留验收证据。
