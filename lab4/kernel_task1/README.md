# LAB4 内核态 task1：单页表机制

## 原始任务

> 完成LAB4 内核态task1：单页表机制
> 目标：理解内核映射与用户映射的组织方式，完成单页表方案下的地址空间管理。
> 要求：
> 1. 任务与内核共用同一张页表；
> 2. 明确隔离策略（用户不可访问内核页；权限位正确）；
> 3. 给出验证方式（越权访问应触发异常）。
> 验收检查：
> 1. 内核态的高半核（或恒等映射）与用户态页共同存在于同一棵多级页表中；
> 2. PTE 的 U 标志位被正确设置（内核段无 U 位，用户段有 U 位）；
> 3. 用户程序尝试越权访问内核地址必然触发异常拦截。

## 实验目标与方案

本任务运行在 `QEMU virt` 的 RISC-V 裸机教学内核环境中，不是宿主 Linux 内核。

实现采用 `M-mode -> S-mode` 启动流程，在 S-mode 打开 `Sv39` 后只安装一张根页表：

- 内核使用 `0x8000_0000..0x8100_0000` 的 16 MiB 恒等映射，叶子 PTE 不带 `U` 位。
- UART 和 QEMU test device 也映射进同一地址空间，便于日志和退出。
- 用户程序代码页、数据页、栈页映射在低地址 `0x0040_0000..0x0040_3000`，叶子 PTE 带 `U` 位。
- 内核进入 U-mode 后执行一个最小探测程序，先访问用户数据和用户栈，再故意 `ld 0x8000_0000` 读取内核地址。
- 由于该内核页叶子项 `U=0`，硬件应产生 `load page fault`，并通过 `medeleg` 委托给 S-mode trap handler。

这套方案可以同时证明：

- 内核页和用户页共存于同一棵多级页表。
- 访问隔离由同一棵页表中的 PTE 权限位完成，而不是通过切换不同页表完成。
- 用户态越权访问内核虚拟地址时会被硬件拦截。

## 文件列表

- `src/main.rs`：页表构建、`satp` 切换、U-mode 进入、trap 分发、验收日志输出。
- `src/boot.S`：M/S/U 三种上下文切换入口，以及用户态越权探测程序。
- `src/trap.rs`：trap frame 定义与 `mtvec/stvec` 初始化。
- `src/console.rs`：UART 输出。
- `linker.ld`：镜像布局与三段栈空间。
- `artifacts/build_output.txt`：最终成功构建输出。
- `artifacts/run_output.txt`：首轮完整运行日志。
- `artifacts/run_output_repeat.txt`：重复运行日志。
- `artifacts/single_pagetable_objdump.txt`：反汇编证据。
- `artifacts/single_pagetable_nm.txt`：符号表证据。
- `artifacts/tool_versions.txt`：工具链和 QEMU 版本。

## 关键实现说明

### 1. 单页表组织方式

根页表同时挂接两类映射：

- `vpn2=2` 方向映射到内核 L1 页表，对应 `0x8000_0000` 开始的内核恒等映射。
- `vpn2=0` 方向映射到低地址 L1 页表，再向下挂接用户 L0 页表和设备映射。

因此内核虚拟地址和用户虚拟地址同时存在于同一个 `satp(root)` 指向的根页表中。

### 2. 隔离策略

- 内核恒等映射叶子项权限为 `VRWX--AD`，没有 `U` 位。
- 用户代码页权限为 `VR-XU-A-`。
- 用户数据页和栈页权限为 `VRW-U-AD`。

也就是说，用户页能被 U-mode 访问，但内核页即使在同一页表里存在，U-mode 也不能直接访问。

### 3. 越权访问验证路径

用户探测程序按顺序执行：

1. 从用户数据页读取 `seed`。
2. 写回 `readback`，再经由用户栈回写 `stack_echo`。
3. 写入 `stage_marker=0xfeedface00000001`。
4. 尝试从 `0x8000_0000` 读取一个 64 位值。

如果第 4 步没有触发 fault，程序会继续写出“异常成功读到的值”并执行 `ecall`，内核会将其判定为失败。实际运行中没有走到该路径。

## 构建与运行

在任务目录下执行：

```bash
cargo build
qemu-system-riscv64 -machine virt -bios none -nographic -kernel target/riscv64gc-unknown-none-elf/debug/lab4_kernel_task1
cargo objdump --bin lab4_kernel_task1 -- --demangle -d > artifacts/single_pagetable_objdump.txt
cargo nm --bin lab4_kernel_task1 -- --demangle > artifacts/single_pagetable_nm.txt
```

本次归档的完整日志文件：

- `artifacts/build_output.txt`
- `artifacts/run_output.txt`
- `artifacts/run_output_repeat.txt`
- `artifacts/single_pagetable_objdump.txt`
- `artifacts/single_pagetable_nm.txt`
- `artifacts/tool_versions.txt`

## 实际观测结果

### 运行日志摘录

来自 `artifacts/run_output.txt`：

```text
[kernel] satp(root)=0x8000000000080009 root_pa=0x80009000
[kernel] windows: kernel_identity=[0x80000000, 0x81000000) user=[0x400000, 0x403000)
[pt] kernel_probe va=0x80000000 vpn=(2,0,0) level=L1-2M ... flags=VRWX--AD
[pt] user_text va=0x400000 vpn=(0,2,0) level=L0-4K ... flags=VR-XU-A-
[pt] user_data va=0x401000 vpn=(0,2,1) level=L0-4K ... flags=VRW-U-AD
[pt] user_stack va=0x402000 vpn=(0,2,2) level=L0-4K ... flags=VRW-U-AD
[kernel] trapped user fault: scause=0xd sepc=0x40002c stval=0x80000000 satp=0x8000000000080009
[kernel] user evidence: seed=0x1122334455667788 readback=0x1122334455667788 stack_echo=0x1122334455667788 stage=0xfeedface00000001 unexpected_kernel_value=0x0000000000000000 unexpected_syscall=0x0000000000000000
[kernel] acceptance same multi-level root contains kernel and user mappings: PASS
[kernel] acceptance kernel leaves clear U and user leaves set U: PASS
[kernel] acceptance user kernel-probe load trapped as delegated load page fault: PASS
```

重复运行 `artifacts/run_output_repeat.txt` 结果一致，说明该 trap 路径和页表组织是稳定可复现的。

### 反汇编证据

`artifacts/single_pagetable_objdump.txt` 中能看到关键控制流与页表切换指令：

```text
0000000080000470 <enter_supervisor>:
80000492: 30200073      mret
0000000080000496 <enter_user_task>:
8000053c: 10200073      sret
0000000080000540 <machine_trap_entry>:
00000000800005f0 <supervisor_trap_entry>:
00000000800006a0 <__user_program_start>:
80001906: 18051073      csrw satp, a0
8000190a: 12000073      sfence.vma
```

这说明：

- 内核确实在进入 S-mode/U-mode 前后经过显式的 `mret`/`sret`。
- 页表切换由 `csrw satp` 和 `sfence.vma` 完成。
- 用户探测程序作为独立代码段被复制到用户代码页执行。

## 机制解释

在 `Sv39` 下，地址翻译会逐级查找页表项。对于本实验：

- 访问用户地址时，根页表先落到低地址 L1，再落到用户 L0，最终命中带 `U=1` 的 4 KiB 叶子页。
- 访问 `0x8000_0000` 时，根页表直接落到内核 L1 的 2 MiB 叶子页，叶子项具备 `R/W/X` 但 `U=0`。

当处理器运行在 U-mode 且访问命中 `U=0` 的叶子项时，即使该地址在页表中“存在”，也会因为权限检查失败而产生 page fault。本实验中表现为：

- `scause=0xd`，即 `load page fault`
- `stval=0x80000000`，即用户试图越权读取的内核地址
- `satp` 在 fault 前后保持同一个根页表值，说明 fault 发生在“单页表共享地址空间”的前提下

## 验收清单

- [x] 内核态恒等映射与用户态页共同存在于同一棵多级页表中。
  - 证据：`satp(root)=0x8000000000080009` 固定不变；`kernel_probe` 和 `user_text/user_data/user_stack` 都能从同一根页表走出有效映射。
- [x] PTE 的 `U` 标志位设置正确。
  - 证据：运行日志中 `kernel_probe` 为 `flags=VRWX--AD`，用户页分别为 `VR-XU-A-` 和 `VRW-U-AD`。
- [x] 用户程序越权访问内核地址会触发异常并被拦截。
  - 证据：`scause=0xd`、`stval=0x80000000`，且最终三项 acceptance 都为 `PASS`。

## 环境信息

来自 `artifacts/tool_versions.txt`：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 复现限制与说明

- 本任务使用的是 QEMU `virt` 机器上的教学内核最小环境，不包含完整操作系统进程模型。
- 为了突出“单页表 + U 位隔离”机制，实验使用恒等映射内核窗口，而不是高半内核；这符合题目中“高半核（或恒等映射）”的验收口径。
- 当前实验专注于读越权触发的 `load page fault`。如果需要，也可以扩展为执行越权或写越权的补充验证。
