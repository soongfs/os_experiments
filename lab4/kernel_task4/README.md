# LAB4 内核态 task4：swap in/out 与置换算法

## 原始任务

> 完成LAB4 内核态task4：swap in/out 与置换算法
> 目标：在内存不足时通过置换算法选择牺牲页，完成换出与换入。
> 要求：
> 1. 实现 swap in/out；
> 2. 实现 Clock 或二次机会置换算法；
> 3. 提供可核验指标：换出页数、换入页数、命中/缺页统计等。
> 验收检查：
> 1. 物理内存耗尽时系统不崩溃，成功触发置换算法；
> 2. 被换出页的 PTE 指向正确的 Swap 磁盘位置；
> 3. 再次访问该页时引发 Page Fault 并成功从磁盘读回并恢复映射。

## 实验目标与方案

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现采用最小可验证的单地址空间教学内核：

- 用户代码页、共享页、用户栈页是启动时即建立的固定映射。
- 额外提供 3 个匿名工作集页：`0x00410000`、`0x00411000`、`0x00412000`。
- 物理驻留帧只给 2 个，因此访问第 3 个工作集页时必然触发置换。
- swap 后端不是 QEMU 块设备，而是内核里一块固定大小的“模拟 swap 区” `SWAP_AREA`，按 4 KiB 页切成 4 个 slot。
- 被换出的页使用软件位 `PTE_SWAP` 编码成“无效但可解码”的 swap PTE：
  - `V=0`
  - `PTE_SWAP=1`
  - `pte >> 10` 保存 slot 编号

置换算法使用标准 Clock / second-chance 思路：

1. 需要新驻留页时，如果还有空帧则直接分配。
2. 如果 2 个驻留帧都已占满，就沿 `CLOCK_HAND` 扫描候选页。
3. 候选页的 PTE 若带 `A` 位，则清掉 `A` 位并给予 second chance。
4. 找到 `A=0` 的页后，将其内容复制到 swap slot，把原用户 PTE 改成 swap PTE。
5. 之后再次访问该页，会在 `load/store page fault` 中识别为 swapped PTE，再从 slot 拷回内存并恢复为可写用户映射。

为了让行为稳定可复现，用户态测试程序的访问序列固定为：

1. 写 page0
2. 写 page1
3. 写 page2，迫使 page0 被第一次换出
4. 读 page2，形成一次命中
5. 读 page0，触发 swap in
6. 再读一次 page0，形成第二次命中

这条序列会稳定产生：

- `page_faults=4`
- `lazy_allocs=3`
- `swap_outs=2`
- `swap_ins=1`
- `hits=2`
- `second_chances=2`

## acceptance -> evidence 设计

- 验收 1：内存耗尽时不崩溃并触发置换
  - 证据：`swap out trigger ...`
  - 证据：`clock_scans` / `second_chances` 计数
  - 证据：最终三项 acceptance 全部 `PASS`
- 验收 2：被换出页的 PTE 指向正确 swap 位置
  - 证据：`victim_after_swap ... swap_slot=0 swap_pa=0x80017000 flags=--------S`
  - 证据：`first_evicted_swap_pte=0x100` 与 `first_evicted_slot=0`
- 验收 3：再次访问时触发缺页并成功换入
  - 证据：`swap in kind=load ... page_index=0 slot=0`
  - 证据：`swap_before` 显示该页仍是 swapped PTE
  - 证据：`swap_after` 和 `page0_final` 显示已经恢复成 `VRW-U-AD-`
  - 证据：用户回读值仍等于 `0x1111222233334444`

## 文件列表

- `src/main.rs`：页表构建、工作集页 lazy fault、Clock 置换、swap in/out、计数器和验收日志。
- `src/boot.S`：M/S/U 模式入口、trap 保存恢复，以及固定访问序列的用户态测试程序。
- `src/trap.rs`：trap frame 与 trap vector 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局和内核 / trap 栈定义。
- `artifacts/build_output.txt`：最终成功构建输出。
- `artifacts/run_output.txt`：第一次完整运行日志。
- `artifacts/run_output_repeat.txt`：第二次完整运行日志。
- `artifacts/swap_clock_objdump.txt`：反汇编证据。
- `artifacts/swap_clock_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链和 QEMU 版本。

## 关键实现说明

### 1. 工作集页只在缺页时真正落到物理帧

启动时内核只建立用户代码页、共享页、栈页三种固定映射。3 个工作集页的 L0 PTE 先都置空：

- `working_page0_before ... leaf_pte=0x0`
- `working_page1_before ... leaf_pte=0x0`
- `working_page2_before ... leaf_pte=0x0`

因此用户第一次读写它们时，都会进入 `handle_user_page_fault()`：

- 无映射：走 `lazy_allocate_page()`
- `PTE_SWAP=1`：走 `swap_in_page()`

### 2. Clock / second-chance 的触发路径

用户先写 page0 和 page1，占满 2 个驻留帧：

- page0 -> `frame=0 pa=0x80013000`
- page1 -> `frame=1 pa=0x80014000`

随后写 page2 时，没有空帧，内核进入 `evict_with_clock()`。第一次扫描看到 page0 / page1 的 PTE 都有 `A` 位，因此：

```text
[clock] frame=0 victim_page=0 ... second_chance old_pte=0x20004cd7 new_pte=0x20004c97
[clock] frame=1 victim_page=1 ... second_chance old_pte=0x200050d7 new_pte=0x20005097
```

第二轮再回来时，page0 的 `A` 位已经清掉，于是它成为牺牲页：

```text
[kernel] swap out trigger incoming_page=2 victim_page=0 frame=0 victim_va=0x410000 victim_pa=0x80013000 slot=0 swap_pte=0x100
[pt] victim_after_swap ... leaf_pte=0x0000000000000100 swap_slot=0 swap_pa=0x80017000 flags=--------S
```

这说明：

- 置换算法确实被触发；
- page0 的用户 PTE 已经不再指向物理页，而是编码成了 slot 0 的 swap PTE；
- 日志中能直接读出 slot 编号和模拟 swap 区内的页位置。

### 3. swap in 路径

之后用户再次读取 page0，会触发 `load page fault`。fault handler 检测到 `PTE_SWAP=1` 后：

1. 获取一个可用驻留帧，如果没有空帧就先继续置换；
2. 从 `swap_slot=0` 的 `swap_pa=0x80017000` 拷回数据；
3. 用普通 `VRW-U-AD-` 用户叶子 PTE 恢复映射；
4. 清理该 slot 的占用标记。

实际日志：

```text
[kernel] swap in kind=load sepc=0x400076 stval=0x410000 page_index=0 slot=0 restored_pa=0x80014000 swap_pte=0x100
[pt] swap_before ... leaf_pte=0x0000000000000100 swap_slot=0 swap_pa=0x80017000 flags=--------S
[pt] swap_after ... leaf_pte=0x00000000200050d7 leaf_pa=0x80014000 flags=VRW-U-AD-
```

这说明 page0 的再次访问确实经历了：

- 先命中 swapped PTE
- 触发 page fault
- 从 slot 0 换入
- 重新建立用户态可写映射

### 4. 统计项

实验最终统计由内核直接打印：

```text
[kernel] counters: accesses=6 hits=2 page_faults=4 load_faults=1 store_faults=3 lazy_allocs=3 swap_outs=2 swap_ins=1 clock_scans=4 second_chances=2
```

含义如下：

- `accesses=6`：用户程序总共 6 次工作集页访问
- `hits=2`：第 4 次读 page2 和第 6 次再读 page0 命中
- `page_faults=4`：前三次首次访问 + 第 5 次 swap in fault
- `swap_outs=2`：page0、page1 各被换出一次
- `swap_ins=1`：page0 被换入一次
- `clock_scans=4`：Clock 手指共扫描 4 次
- `second_chances=2`：两页都先获得了一次二次机会

## 构建、运行与复现命令

在任务目录 `lab4/kernel_task4` 下执行：

```bash
cargo build --target riscv64gc-unknown-none-elf
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab4_kernel_task4
cargo objdump --target riscv64gc-unknown-none-elf --bin lab4_kernel_task4 -- --demangle -d > artifacts/swap_clock_objdump.txt
cargo nm --target riscv64gc-unknown-none-elf --bin lab4_kernel_task4 -- --demangle > artifacts/swap_clock_nm.txt
```

本次归档的证据文件：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/swap_clock_objdump.txt`
- `artifacts/swap_clock_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] satp(root)=0x800000000008000d root_pa=0x8000d000
[kernel] swap backend: 4 slots in simulated swap disk area at pa=0x80017000
[kernel] lazy map kind=store sepc=0x400022 stval=0x410000 page_index=0 frame=0 pa=0x80013000
[kernel] lazy map kind=store sepc=0x400046 stval=0x411000 page_index=1 frame=1 pa=0x80014000
[clock] frame=0 victim_page=0 va=0x410000 second_chance old_pte=0x20004cd7 new_pte=0x20004c97
[clock] frame=1 victim_page=1 va=0x411000 second_chance old_pte=0x200050d7 new_pte=0x20005097
[kernel] swap out trigger incoming_page=2 victim_page=0 frame=0 victim_va=0x410000 victim_pa=0x80013000 slot=0 swap_pte=0x100
[pt] victim_after_swap ... leaf_pte=0x0000000000000100 swap_slot=0 swap_pa=0x80017000 flags=--------S
[kernel] swap out trigger incoming_page=0 victim_page=1 frame=1 victim_va=0x411000 victim_pa=0x80014000 slot=1 swap_pte=0x500
[kernel] swap in kind=load sepc=0x400076 stval=0x410000 page_index=0 slot=0 restored_pa=0x80014000 swap_pte=0x100
[pt] swap_before ... leaf_pte=0x0000000000000100 swap_slot=0 swap_pa=0x80017000 flags=--------S
[pt] swap_after ... leaf_pte=0x00000000200050d7 leaf_pa=0x80014000 flags=VRW-U-AD-
[kernel] counters: accesses=6 hits=2 page_faults=4 load_faults=1 store_faults=3 lazy_allocs=3 swap_outs=2 swap_ins=1 clock_scans=4 second_chances=2
[kernel] user evidence: stage=0xabcdef0123456789 page2_hit=0x9999aaaabbbbcccc page0_swapin=0x1111222233334444 page0_hit_again=0x1111222233334444
[kernel] acceptance memory pressure triggers Clock replacement without crashing: PASS
[kernel] acceptance swapped-out PTE encodes the correct swap slot: PASS
[kernel] acceptance reaccess faults, swaps page back in, and restores mapping/data: PASS
```

可以直接从这份日志读出完整的换入换出链路：

- 第 3 个工作集页到来时，Clock 先给 page0 / page1 一次二次机会；
- page0 最终被换出到 `slot=0`，其用户 PTE 被改成 `0x100`；
- 再次读 page0 时，触发 fault 并从 `slot=0` 换回；
- 换回后的 page0 重新变成普通 `VRW-U-AD-` 映射；
- page0 的数据值没有损坏，仍然是 `0x1111222233334444`。

### 重复运行

`artifacts/run_output_repeat.txt` 与第一次运行保持一致：

- `swap_outs=2`
- `swap_ins=1`
- `clock_scans=4`
- `second_chances=2`
- 三项 acceptance 继续全部为 `PASS`

说明这个 swap + Clock 行为是稳定可复现的。

### 反汇编与符号证据

`artifacts/swap_clock_objdump.txt` 中能看到关键入口与 swap/缺页处理路径：

```text
00000000800003e0 <enter_supervisor>:
0000000080000406 <enter_user_task>:
00000000800004b0 <machine_trap_entry>:
0000000080000560 <supervisor_trap_entry>:
0000000080000610 <__user_program_start>:
0000000080000cba <lab4_kernel_task4::swap_in_page...>:
0000000080001786 <lab4_kernel_task4::evict_with_clock...>:
0000000080004104 <lab4_kernel_task4::handle_user_page_fault...>:
800040e6: 12000073      sfence.vma
800045a8: 12050073      sfence.vma a0
```

`artifacts/swap_clock_nm.txt` 中的关键符号：

```text
0000000080000cba t lab4_kernel_task4::swap_in_page...
0000000080001786 t lab4_kernel_task4::evict_with_clock...
0000000080003206 t lab4_kernel_task4::finish_user_program...
0000000080004104 t lab4_kernel_task4::handle_user_page_fault...
0000000080000610 T __user_program_start
```

这说明：

- 用户程序、缺页处理、Clock 置换和 swap in 都有独立实现；
- PTE 更新后执行了 `sfence.vma`；
- M/S/U 入口与 trap 保存恢复路径都存在。

## 机制解释

这个实验里的“swap 磁盘”是软件模拟的：本质上是内核保留的一块按页切分的数组 `SWAP_AREA`。这不是完整块设备驱动，但足以验证 swap PTE 编码、换出、换入和置换策略的核心语义。

整个 OS 路径可以概括为：

1. 用户访问工作集页
2. 若尚未建立叶子 PTE，则触发 page fault 并 lazy 分配物理页
3. 若物理驻留帧耗尽，则进入 Clock 扫描
4. 若候选页 `A=1`，清 `A` 并给 second chance
5. 若候选页 `A=0`，把其内容复制到 swap slot，并把 PTE 改成 `V=0 + PTE_SWAP + slot`
6. 用户再次访问 swapped 页时，硬件因 `V=0` 抛出 page fault
7. fault handler 识别 `PTE_SWAP`，从 slot 读回数据，恢复为普通用户叶子映射

因此，这个任务验证了三件事：

- “物理内存不足时不崩溃”的前提是可以把牺牲页编码成可恢复的 swap PTE；
- 置换算法不是随便挑页，而是优先给近期访问过的页一次机会；
- 重新访问 swapped 页时，真正的恢复动作发生在 page fault 路径，而不是后台预取。

## 验收清单

- [x] 物理内存耗尽时系统不崩溃，成功触发置换算法。
  - 证据：`swap out trigger incoming_page=2 victim_page=0 ...`
  - 证据：`clock_scans=4 second_chances=2`
  - 证据：最终 `acceptance memory pressure triggers Clock replacement without crashing: PASS`
- [x] 被换出页的 PTE 指向正确的 swap 位置。
  - 证据：`victim_after_swap ... leaf_pte=0x100 swap_slot=0 swap_pa=0x80017000 flags=--------S`
  - 证据：`first_evicted_page=0 first_evicted_slot=0 first_evicted_swap_pte=0x100`
- [x] 再次访问该页时引发 page fault 并成功从 swap 区读回恢复映射。
  - 证据：`swap in kind=load ... page_index=0 slot=0 restored_pa=0x80014000`
  - 证据：`swap_before` 仍显示 swapped PTE，`swap_after` 已恢复到 `VRW-U-AD-`
  - 证据：`page0_swapin=0x1111222233334444` 和 `page0_hit_again=0x1111222233334444`

## 环境信息

来自 `artifacts/tool_versions.txt`：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 限制与说明

- 本实现的 swap 后端是内核内存中的模拟 swap 区，不是 virtio-blk 或真实磁盘驱动。
- 这个版本重点验证“swap PTE 编码 + fault 驱动的换入换出 + Clock 置换”语义，不涉及多进程竞争、脏页回写优化或异步 I/O。
- 本次结果是在当前 Linux 主机上的 QEMU 环境中复现得到，未额外在另一台原生 Linux 服务器上重复验证。
