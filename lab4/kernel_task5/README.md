# LAB4 内核态 task5：自映射机制

## 原始任务

> 完成LAB4 内核态task5：自映射机制
> 目标：通过自映射让页表结构自身可被线性访问，便于调试与管理。
> 要求：
> 1. 实现页表自映射；
> 2. 提供示例：通过虚拟地址访问页表项并输出部分映射信息；
> 3. 说明安全边界：用户态不可利用自映射越权读写内核结构。
> 验收检查：
> 1. 顶层页表的某一特定表项指向顶层页表自身物理地址；
> 2. 内核可通过固定模式的虚拟地址高效遍历任意层级的 PTE。

## 实验目标与方案

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现基于 LAB4 `kernel_task1` 的单地址空间教学内核，在同一棵 `Sv39` 根页表上增加一套最小可验证的“递归自映射”机制：

- 选定 `root[511]` 作为递归入口，自身指向 `root_pa`
- 选定保留槽 `510` 作为“页表页别名叶子项”
- 用户代码页 / 数据页 / 栈页仍位于低地址 `0x0040_0000..0x0040_3000`
- 内核恒等映射仍位于 `0x8000_0000..0x8100_0000`
- 用户程序在 U-mode 下故意探测自映射窗口 `0xffffffffffffe000`，验证其因 `U=0` 被硬件拦截

为便于说明，记：

- `S = 511`：递归根页表项
- `A = 510`：保留的页表页别名槽

则本实验里固定模式的虚拟地址公式为：

- 根页表页虚拟地址：`[S, S, A, offset]`
- 某个 L1 页表页虚拟地址：`[S, vpn2, A, offset]`
- 某个 L0 页表页虚拟地址：`[vpn2, vpn1, A, offset]`

这里 `offset` 允许直接把该页表页当成 `512 * sizeof(PTE)` 的普通线性数组来读写，所以：

- `root_pte(vpn2)` 可以直接通过 `root_table_va + vpn2 * 8` 读取
- `l1_pte(vpn2, vpn1)` 可以直接通过 `l1_table_va(vpn2) + vpn1 * 8` 读取
- `l0_pte(vpn2, vpn1, vpn0)` 可以直接通过 `l0_table_va(vpn2, vpn1) + vpn0 * 8` 读取

这正是本任务要求的“通过固定模式虚拟地址高效遍历任意层级 PTE”。

## acceptance -> evidence 设计

- 验收 1：顶层页表某项指向顶层页表自身物理地址
  - 证据：`[selfmap] root[ref=511] direct=... via_root_table_va=... expected=...`
  - 证据：`root_pa=0x8000b000`
- 验收 2：内核可通过固定模式虚拟地址遍历多级 PTE
  - 证据：`selfmap addresses: root_table_va=... low_l1_table_va=... user_l0_table_va=...`
  - 证据：`kernel_probe_via_selfmap / user_text_via_selfmap / user_data_via_selfmap / user_stack_via_selfmap`
  - 证据：`direct/selfmap equality: ... PASS`
- 安全边界：用户态不可利用自映射越权读写内核结构
  - 证据：用户探测 `0xffffffffffffe000` 时 `scause=0xd`
  - 证据：自映射 alias 页 `flags=VRW---AD`，没有 `U`

## 文件列表

- `src/main.rs`：页表构建、自映射地址公式、自映射 walker、U-mode 探测和验收日志。
- `src/boot.S`：M/S/U 模式切换和最小用户态 selfmap 越权探测程序。
- `src/trap.rs`：trap frame 和 trap vector 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局和内核 / trap 栈。
- `artifacts/build_output.txt`：最终成功构建输出。
- `artifacts/run_output.txt`：首轮完整运行日志。
- `artifacts/run_output_repeat.txt`：重复运行日志。
- `artifacts/selfmap_objdump.txt`：反汇编证据。
- `artifacts/selfmap_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链和 QEMU 版本。

## 关键实现说明

### 1. 递归根页表项

根页表除了正常的用户 / 内核 / 设备映射外，还增加了：

- `root[511] = table_pte(root_pa)`

这使得当硬件以 `vpn2=511` 开始翻译时，下一层页表仍然是根页表自己。

实际运行中：

```text
[kernel] satp(root)=0x800000000008000b root_pa=0x8000b000
[selfmap] root[ref=511] direct=0x0000000020002c01 via_root_table_va=0x0000000020002c01 expected=0x0000000020002c01
```

可见：

- `root[511]` 的 PPN 对应 `0x8000b000`
- 内核既能直接从静态页表对象读到它，也能通过自映射窗口再次读到同一值

### 2. 固定模式的页表页访问

单独的 `root[511]` 只能保证“递归回到 root”，还需要一个固定槽把“当前页表页”当成普通 4 KiB 数据页来读。

因此本实验额外保留了 `A = 510` 这个 alias 槽，并在每个页表页中预留一项 kernel-only leaf：

- `root[510] -> root_pa`
- `low_l1[510] -> low_l1_pa`
- `user_l0[510] -> user_l0_pa`
- `kernel_l1[510] -> kernel_l1_pa`
- `dev_l0[510] -> dev_l0_pa`

这样就得到固定访问模式：

- `root_table_va = [511, 511, 510] = 0xffffffffffffe000`
- `low_l1_table_va = [511, 0, 510] = 0xffffffffc01fe000`
- `user_l0_table_va = [0, 2, 510] = 0x5fe000`
- `kernel_l1_table_va = [511, 2, 510] = 0xffffffffc05fe000`

运行日志直接打印了这些值：

```text
[kernel] selfmap addresses: root_table_va=0xffffffffffffe000 low_l1_table_va=0xffffffffc01fe000 user_l0_table_va=0x5fe000 kernel_l1_table_va=0xffffffffc05fe000 probe_va=0xffffffffffffe000
```

### 3. 通过 selfmap walker 遍历 PTE

`main.rs` 里实现了 `selfmap_walk_virtual(va)`，它不再通过物理地址强转访问页表，而是：

1. 先用 `root_table_va + vpn2 * 8` 取根页表项
2. 若根项还是表项，再用 `l1_table_va(vpn2) + vpn1 * 8` 取 L1 项
3. 若 L1 项还是表项，再用 `l0_table_va(vpn2, vpn1) + vpn0 * 8` 取 L0 项

也就是说，整个 walk 过程已经变成“固定地址公式 + 普通内存读”的形式。

运行日志里同时打印了 direct walker 和 selfmap walker 的结果。例如用户代码页：

```text
[pt] user_text_direct va=0x400000 ... root_pte=0x0000000020003801 l1_pte=0x0000000020003c01 l0_pte=0x000000002000245b ...
[selfmap] user_text_via_selfmap va=0x400000 entry_vas(root=0xffffffffffffe000, l1=0xffffffffc01fe010, l0=0x5fe000) ... root_pte=0x0000000020003801 l1_pte=0x0000000020003c01 l0_pte=0x000000002000245b ...
```

内核 2 MiB 映射也同样可通过 selfmap 读取到 L1 叶子项：

```text
[pt] kernel_probe_direct va=0x80000000 ... root_pte=0x0000000020004001 l1_pte=0x00000000200000cf ...
[selfmap] kernel_probe_via_selfmap va=0x80000000 entry_vas(root=0xffffffffffffe010, l1=0xffffffffc05fe000, l0=0x0) ... root_pte=0x0000000020004001 l1_pte=0x00000000200000cf ...
```

最终内核直接汇总：

```text
[kernel] direct/selfmap equality: kernel_probe=PASS user_text=PASS user_data=PASS user_stack=PASS selfmap_probe=PASS
```

这证明自映射路径已经足以高效遍历多级 PTE。

### 4. 安全边界

自映射窗口虽然存在，但所有 alias 叶子项都故意不设置 `U` 位：

```text
[selfmap] root[alias=510] via_root_table_va=0x0000000020002cc7 flags=VRW---AD
[pt] selfmap_probe_direct va=0xffffffffffffe000 ... leaf_pte=0x0000000020002cc7 ... flags=VRW---AD
```

因此它只对 S-mode 内核开放，对 U-mode 是不可访问的。用户程序在进入 U-mode 后：

1. 先正常读取并回写用户数据页
2. 再经过用户栈回写 `stack_echo`
3. 写入 `stage_marker=0xfeedface00000005`
4. 最后故意读取 `SELFMAP_PROBE_VA = 0xffffffffffffe000`

实际结果是：

```text
[kernel] trapped user selfmap probe: scause=0xd sepc=0x40002a stval=0xffffffffffffe000 satp=0x800000000008000b
[kernel] user evidence: seed=0x1122334455667788 readback=0x1122334455667788 stack_echo=0x1122334455667788 stage=0xfeedface00000005 unexpected_selfmap_value=0x0000000000000000 unexpected_syscall=0x0000000000000000
```

可见：

- 用户普通页和用户栈仍可正常访问
- 但一旦去读 selfmap 窗口，就触发 delegated `load page fault`
- 没有读到任何越权值，也没有执行到后续 `ecall` 失败路径

## 构建、运行与复现命令

在任务目录 `lab4/kernel_task5` 下执行：

```bash
cargo build --target riscv64gc-unknown-none-elf
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab4_kernel_task5
cargo objdump --target riscv64gc-unknown-none-elf --bin lab4_kernel_task5 -- --demangle -d > artifacts/selfmap_objdump.txt
cargo nm --target riscv64gc-unknown-none-elf --bin lab4_kernel_task5 -- --demangle > artifacts/selfmap_nm.txt
```

本次归档的证据文件：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/selfmap_objdump.txt`
- `artifacts/selfmap_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] selfmap policy: root[511] recursively points to root_pa; reserved alias index 510 exposes table pages as kernel-only leaf mappings under fixed Sv39 virtual patterns
[kernel] satp(root)=0x800000000008000b root_pa=0x8000b000
[kernel] selfmap addresses: root_table_va=0xffffffffffffe000 low_l1_table_va=0xffffffffc01fe000 user_l0_table_va=0x5fe000 kernel_l1_table_va=0xffffffffc05fe000 probe_va=0xffffffffffffe000
[selfmap] root[ref=511] direct=0x0000000020002c01 via_root_table_va=0x0000000020002c01 expected=0x0000000020002c01
[selfmap] root[alias=510] via_root_table_va=0x0000000020002cc7 flags=VRW---AD
[selfmap] kernel_probe_via_selfmap ... root_pte=0x0000000020004001 l1_pte=0x00000000200000cf ...
[selfmap] user_text_via_selfmap ... root_pte=0x0000000020003801 l1_pte=0x0000000020003c01 l0_pte=0x000000002000245b ...
[selfmap] user_data_via_selfmap ... root_pte=0x0000000020003801 l1_pte=0x0000000020003c01 l0_pte=0x00000000200028d7 ...
[kernel] direct/selfmap equality: kernel_probe=PASS user_text=PASS user_data=PASS user_stack=PASS selfmap_probe=PASS
[kernel] trapped user selfmap probe: scause=0xd sepc=0x40002a stval=0xffffffffffffe000 satp=0x800000000008000b
[kernel] acceptance root[511] points to root_pa and matches recursive readback: PASS
[kernel] acceptance fixed-pattern selfmap virtual addresses traverse root/L1/L0 entries: PASS
[kernel] acceptance user selfmap probe is blocked by U=0 kernel-only aliases: PASS
```

这份日志已经完整覆盖题目要求：

- 根页表指定项确实回指 `root_pa`
- 内核通过固定公式地址读出了 root / L1 / L0 的实际 PTE
- 用户态访问 selfmap 时被硬件拦截，没有越权成功

### 重复运行

`artifacts/run_output_repeat.txt` 与第一次运行结果一致：

- `root_pa` 和 `root[511]` 的自映射回读值一致
- `direct/selfmap equality` 五项继续全部 `PASS`
- 用户态 probe 继续稳定得到 `scause=0xd`
- 三项 acceptance 继续全部 `PASS`

说明该自映射机制和安全边界是稳定可复现的。

### 反汇编与符号证据

`artifacts/selfmap_objdump.txt` 中能看到关键入口和 selfmap 相关代码：

```text
0000000080000e80 <enter_supervisor>:
0000000080000ea6 <enter_user_task>:
0000000080000f50 <machine_trap_entry>:
0000000080001000 <supervisor_trap_entry>:
00000000800010b0 <__user_program_start>:
0000000080001d18 <lab4_kernel_task5::selfmap_walk_virtual...>:
000000008000207a <lab4_kernel_task5::handle_expected_selfmap_fault...>:
8000205c: 12000073      sfence.vma
```

`artifacts/selfmap_nm.txt` 中的关键符号：

```text
0000000080001d18 t lab4_kernel_task5::selfmap_walk_virtual...
000000008000207a t lab4_kernel_task5::handle_expected_selfmap_fault...
0000000080002962 t lab4_kernel_task5::build_page_tables_with_selfmap...
00000000800010b0 T __user_program_start
```

这说明：

- 自映射 walker 是独立实现的
- 页表构建逻辑单独包含 `build_page_tables_with_selfmap`
- 用户探测程序和 trap 路径都在镜像中清晰可见

## 机制解释

这个实验的关键点是：`root[511] = root_pa` 只是“递归入口”，它保证页表树能回到自己；而 `A = 510` 这个保留 alias 槽，则把“当前页表页”变成一个普通的 kernel-only 4 KiB 数据页，从而允许内核直接按数组索引读写 PTE。

因此：

- `root[511]` 负责“回到页表树本身”
- `[*][*][510]` 负责“把某个具体页表页当作普通内存来访问”

二者组合之后，页表就不再只能通过物理地址强转来遍历，而可以通过固定公式虚拟地址做线性访问。这对调试、打印页表、实现更复杂的页表管理逻辑都更友好。

安全边界来自权限位，而不是“隐藏地址”：

- 所有 selfmap alias 叶子项都不设置 `U`
- 因而 U-mode 即使知道 selfmap 地址，也会在硬件权限检查阶段被拦截

本实验中，用户探测 `0xffffffffffffe000` 得到的正是这一机制的直接验证。

## 验收清单

- [x] 顶层页表的某一特定表项指向顶层页表自身物理地址。
  - 证据：`root[ref=511] direct=0x0000000020002c01 via_root_table_va=0x0000000020002c01 expected=0x0000000020002c01`
  - 证据：`root_pa=0x8000b000`，该 PTE 的 PPN 正好对应根页表物理地址。
- [x] 内核可通过固定模式虚拟地址高效遍历任意层级的 PTE。
  - 证据：`root_table_va=0xffffffffffffe000 low_l1_table_va=0xffffffffc01fe000 user_l0_table_va=0x5fe000 kernel_l1_table_va=0xffffffffc05fe000`
  - 证据：`kernel_probe_via_selfmap`、`user_text_via_selfmap`、`user_data_via_selfmap` 都成功读到和 direct walker 完全一致的 PTE。
  - 证据：`direct/selfmap equality: kernel_probe=PASS user_text=PASS user_data=PASS user_stack=PASS selfmap_probe=PASS`
- [x] 用户态不可利用自映射越权读写内核结构。
  - 证据：`root[alias=510] ... flags=VRW---AD` 与 `selfmap_probe_direct ... flags=VRW---AD` 均无 `U`
  - 证据：U-mode 探测得到 `scause=0xd stval=0xffffffffffffe000`
  - 证据：`unexpected_selfmap_value=0` 且 `unexpected_syscall=0`

## 环境信息

来自 `artifacts/tool_versions.txt`：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 限制与说明

- 这个教学实验只覆盖单地址空间内核上的自映射读访问与安全验证，不涉及多进程页表切换。
- `A = 510` 这个 alias 槽是专门为递归路径保留的。它的目的不是提供普通用户 / 内核数据窗口，而是在“当前页表页已经被递归路径选中之后”，把该页表页当作 4 KiB 普通页来访问。
- 本次结果是在当前 Linux 主机上的 QEMU 环境中复现得到，未额外在另一台原生 Linux 服务器上重复验证。
