# LAB4 内核态 task2：Lazy 按需分页

## 原始任务

> 完成LAB4 内核态task2：Lazy 按需分页
> 目标：基于缺页异常按需分配物理页，减少不必要的预分配。
> 要求：
> 1. 缺页时分配物理页并建立映射；
> 2. 支持至少一种典型场景：堆增长或匿名映射；
> 3. 输出可观察证据：缺页次数、分配次数或日志。
> 验收检查：
> 1. 用户态 sbrk/mmap 时内核不立即分配物理页，仅记录 VMA 结构；
> 2. 拦截 Load/Store Page Fault 并在此刻实际分配物理页表项。

## 实验目标与方案

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现采用 `M-mode -> S-mode -> U-mode` 的最小教学内核路径，在 S-mode 下打开 `Sv39` 并提供 3 个极简 `ecall`：

- `SYS_SBRK`：扩展堆 VMA，仅更新 `heap break` 和 VMA 表，不立即分配物理页。
- `SYS_MMAP`：创建匿名 `mmap` VMA，仅记录 `[start, end)` 区间，不立即分配物理页。
- `SYS_EXIT`：用户探测程序结束后，由内核输出计数器和验收结果。

用户程序先调用一次 `sbrk(8192)` 和一次 `mmap(8192)`，然后：

- 对堆页执行首次写入，触发 `store page fault`
- 对匿名映射页执行首次读取，触发 `load page fault`
- 在每次 fault 时由内核从物理页池里实际分配 4 KiB 页，并把叶子 PTE 补进用户 L0 页表

因此本实现同时覆盖了题目要求的两类典型场景：

- 堆增长
- 匿名映射

## 文件列表

- `src/main.rs`：Lazy VMA、按需分配、syscall、page fault 处理、计数器和验收日志。
- `src/boot.S`：M/S/U 入口、trap 保存恢复、用户态 `sbrk/mmap` 探测程序。
- `src/trap.rs`：trap frame 和 trap vector 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局和栈定义。
- `artifacts/build_output.txt`：最终成功构建输出。
- `artifacts/run_output.txt`：首轮运行日志。
- `artifacts/run_output_repeat.txt`：重复运行日志。
- `artifacts/lazy_paging_objdump.txt`：反汇编证据。
- `artifacts/lazy_paging_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链和 QEMU 版本。

## 关键实现说明

### 1. eager 映射和 lazy 映射的边界

内核只提前映射这些必需页：

- 用户代码页 `0x0040_0000`
- 用户共享数据页 `0x0040_1000`
- 用户栈页 `0x0040_2000`

而这两个业务区间只记录 VMA，不预分配叶子页：

- heap: `[0x0041_0000, 0x0042_0000)`
- mmap: `[0x0042_0000, 0x0044_0000)`

这意味着 `sbrk/mmap` 返回后，相关虚拟地址已经“合法属于用户地址空间”，但页表叶子项仍然为空。

### 2. Lazy fault 处理逻辑

当 U-mode 首次访问某个尚未映射、但位于合法 VMA 内的页时：

1. trap handler 收到 `load page fault` 或 `store page fault`
2. 查找 fault 地址所属 VMA
3. 从内核维护的物理页池中拿出一个零填充页
4. 在用户 L0 页表中安装 `VRW-U-AD` 叶子 PTE
5. 执行 `sfence.vma`
6. `sret` 返回，让原用户指令重新执行

因此，物理页分配和叶子 PTE 建立发生在 page fault 当下，而不是在 `sbrk/mmap` 系统调用阶段。

### 3. 本实验的用户态访问序列

用户探测程序固定做以下事情：

1. `sbrk(8192)`，得到两页堆空间
2. `mmap(8192)`，得到两页匿名映射
3. 写堆第 0 页，触发第 1 次 `store page fault`
4. 写堆第 1 页，触发第 2 次 `store page fault`
5. 读匿名映射第 0 页，触发第 1 次 `load page fault`，首次读到 `0`
6. 读匿名映射第 1 页，触发第 2 次 `load page fault`，首次读到 `0`

最终应观测到：

- `page_faults = 4`
- `load_faults = 2`
- `store_faults = 2`
- `allocs = 4`
- `map_installs = 4`

## 构建与运行

在任务目录下执行：

```bash
cargo build
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab4_kernel_task2
cargo objdump --bin lab4_kernel_task2 -- --demangle -d > artifacts/lazy_paging_objdump.txt
cargo nm --bin lab4_kernel_task2 -- --demangle > artifacts/lazy_paging_nm.txt
```

本次归档的完整证据文件：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/lazy_paging_objdump.txt`
- `artifacts/lazy_paging_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] sys_sbrk len=8192 -> base=0x410000 new_break=0x412000 alloc_count=0 still_unmapped_after_reserve=PASS
[pt] heap_reserved va=0x410000 ... leaf_pte=0x0000000000000000 ... flags=--------
[kernel] sys_mmap len=8192 -> base=0x420000 end=0x422000 alloc_count=0 still_unmapped_after_reserve=PASS
[pt] mmap_reserved va=0x420000 ... leaf_pte=0x0000000000000000 ... flags=--------
[kernel] lazy fault kind=store sepc=0x400056 stval=0x410000 page=0x410000 vma=heap before_present=NO alloc_pa=0x8000a000
[pt] fault_after va=0x410000 ... leaf_pa=0x8000a000 flags=VRW-U-AD
[kernel] lazy fault kind=load sepc=0x40008e stval=0x420000 page=0x420000 vma=mmap before_present=NO alloc_pa=0x8000c000
[pt] fault_after va=0x420000 ... leaf_pa=0x8000c000 flags=VRW-U-AD
[kernel] counters: reserves=2 sbrk_calls=1 mmap_calls=1 page_faults=4 load_faults=2 store_faults=2 allocs=4 map_installs=4
[kernel] user evidence: stage=0x5555666677778888 heap_base=0x410000 mmap_base=0x420000 heap0=0x1111222233334444 heap1=0x5555666677778888 mmap0_initial=0x0000000000000000 mmap0=0x9999aaaabbbbcccc mmap1_initial=0x0000000000000000 mmap1=0xddddeeeeffff0001
[kernel] acceptance sys_sbrk/sys_mmap reserve VMA without immediate physical allocation: PASS
[kernel] acceptance load/store page fault allocates page and installs user PTE on demand: PASS
```

重复运行 `artifacts/run_output_repeat.txt` 与首轮结果一致，说明 fault 次数、分配次数和页表结果是稳定可复现的。

### 反汇编证据

`artifacts/lazy_paging_objdump.txt` 中能看到关键控制流和页表刷新路径：

```text
00000000800007f0 <enter_supervisor>:
80000812: 30200073      mret
0000000080000816 <enter_user_task>:
800008bc: 10200073      sret
00000000800008c0 <machine_trap_entry>:
0000000080000970 <supervisor_trap_entry>:
0000000080000a20 <__user_program_start>:
0000000080002082 <lab4_kernel_task2::handle_user_ecall...>:
00000000800021aa <lab4_kernel_task2::install_user_leaf...>:
800022a6: 12050073      sfence.vma a0
0000000080002f52 <lab4_kernel_task2::handle_lazy_page_fault...>:
```

这说明：

- 内核确实通过 `mret/sret` 在 M/S/U 之间切换。
- 用户态 `sbrk/mmap` 程序是独立代码段。
- 缺页处理路径中存在单页粒度 `sfence.vma`，说明 PTE 安装后进行了 TLB 刷新。

## 机制解释

在 `Sv39` 下，本实验的用户代码、共享页和用户栈是 eager 映射，而 heap/mmap 只做 VMA 预留。于是：

- `sbrk/mmap` 返回后，地址区间已经登记在 VMA 表中，但 `walk_virtual()` 看到的 `leaf_pte` 仍然为 `0`
- 当用户首次读写这些地址时，硬件在页表 walk 中发现叶子项不存在，于是产生 page fault
- 内核根据 `stval` 把 fault 归属到 heap 或 mmap VMA
- 内核补上一页 `VRW-U-AD` 映射后返回用户态，原访存指令重新执行并成功完成

匿名映射首次读取到 `0` 的原因，是新分配物理页在交付前被零填充，因此行为类似匿名零页。

## 验收清单

- [x] 用户态 `sbrk/mmap` 时内核不立即分配物理页，仅记录 VMA 结构。
  - 证据：`sys_sbrk` 和 `sys_mmap` 日志都显示 `alloc_count=0`，且 `heap_reserved/mmap_reserved` 的 `leaf_pte=0x0`。
- [x] 拦截 `Load/Store Page Fault` 并在此刻实际分配物理页表项。
  - 证据：日志里明确出现 `lazy fault kind=store` 和 `lazy fault kind=load`；每次 fault 后的 `fault_after` 均变为 `VRW-U-AD` 的有效用户叶子页。
- [x] 输出可观察证据。
  - 证据：最终计数器固定为 `page_faults=4 load_faults=2 store_faults=2 allocs=4 map_installs=4`。

## 环境信息

来自 `artifacts/tool_versions.txt`：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 复现限制与说明

- 这里的 `sbrk/mmap` 是教学内核里的极简 `ecall` ABI，不是完整 Linux syscall ABI。
- 本实验为了突出 Lazy 分页机制，使用固定大小的 VMA 表和固定大小的物理页池；它证明按需分配机制成立，但不是通用内存管理器的完整实现。
- 当前实验只覆盖匿名可读写页，不涉及文件映射、页回收或 swap。
