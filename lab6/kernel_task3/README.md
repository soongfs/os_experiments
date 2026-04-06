# LAB6 内核态 Task3：mmap 文件映射

## 1. 原始任务说明

### 任务标题

mmap 文件映射

### 任务目标

理解“文件 = 页缓存 = 内存映射”的统一视角，实现基于内存访问的文件 I/O。

### 任务要求

1. 支持 `mmap` 映射文件到地址空间；
2. 支持读写并能落盘（或按实验要求的刷回策略）；
3. 提供验证：映射写入后从文件读取应一致。

### 验收检查

1. 调用 `mmap` 并触发文件备份区的 Page Fault；
2. 数据成功从磁盘被读入并作为该物理页内容；
3. 内存变动（Dirty Page）在取消映射或 `msync` 时能写回持久化。

## 2. Acceptance -> Evidence 清单

- `mmap` 必须在 QEMU guest 内核中实现，并由 U-mode 用户程序触发。
  证据：QEMU 日志显示 `[kernel] LAB6 kernel task3 mmap file mapping`，随后出现 `[page-fault]`、`[mmap]`、`[flush]`、`[acceptance]` 输出，见 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output.txt)。
- 首次访问映射区域必须触发文件后备页 fault。
  证据：日志中有 `[page-fault] cause=load-access addr=... loaded_bytes=4096`，且 `page_faults=1`。
- 文件内容必须被装入映射页并作为后续内存读的实际内容。
  证据：`initial_match=yes`，同时 `pages_loaded=1`、`last_loaded_bytes=4096`。
- 脏页必须在 `msync` 或 `munmap` 时写回文件。
  证据：`msync_persisted=yes`、`munmap_persisted=yes`，并且诊断里 `msync_writebacks=1`、`munmap_writebacks=1`、`dirty_detections=2`。
- 结果必须稳定。
  证据：两次运行 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output_repeat.txt) 的核心计数一致，验收行全部 `PASS`。

## 3. 实验环境与实现思路

本实验运行在 QEMU 的 RISC-V bare-metal guest 环境中，属于 `LAB6` 内核态任务。内核从 M-mode 启动，进入 U-mode 用户程序后，由用户程序通过 `mmap` 获取文件映射地址，并以普通内存读写方式访问文件内容。

这份实验采用“单页 file-backed mapping”的最小可验证模型：

- 只支持一个长度为 `4096` 字节的映射窗口；
- 映射页对应一个普通文件 `/mapped.bin`；
- 首次访问映射窗口时，内核通过 trap 识别 fault，读取文件数据到保留物理页；
- 后续对该页的写入通过 `msync` 或 `munmap` 刷回文件。

仓库当前没有完整 Sv39 页表管理器，因此这里没有构建真正的 page table + VMA 子系统，而是使用 PMP 禁止 U-mode 访问映射窗口，令首次触发的 `load-access fault` 充当“文件映射缺页”的最小实现。换句话说：

- 语义上，它扮演文件后备页的 page fault；
- 编码上，trap 原因是 PMP 访问异常，而不是启用分页后的 `load/store page fault` 编码。

这点在 README 中明确声明，是本实验刻意采用的最小模型，而不是遗漏。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/kernel_task3/Cargo.toml)：Rust 裸机工程配置，二进制名为 `lab6_kernel_task3`。
- [.cargo/config.toml](/root/os_experiments/lab6/kernel_task3/.cargo/config.toml)：RISC-V 目标与链接配置。
- [linker.ld](/root/os_experiments/lab6/kernel_task3/linker.ld)：内存布局与栈布局。
- [src/boot.S](/root/os_experiments/lab6/kernel_task3/src/boot.S)：启动和 trap 入口汇编。
- [src/console.rs](/root/os_experiments/lab6/kernel_task3/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/kernel_task3/src/trap.rs)：`ecall` 和访问 fault 分发。
- [src/user_console.rs](/root/os_experiments/lab6/kernel_task3/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/kernel_task3/src/abi.rs)：`mmap/msync/munmap` syscall 号、`FsStat`、`MmapDiagnostics` 和常量定义。
- [src/syscall.rs](/root/os_experiments/lab6/kernel_task3/src/syscall.rs)：用户态 syscall 封装。
- [src/fs.rs](/root/os_experiments/lab6/kernel_task3/src/fs.rs)：最小文件系统本体，提供 `create_file`、`read_at`、`write_at`、`stat`。
- [src/main.rs](/root/os_experiments/lab6/kernel_task3/src/main.rs)：内核入口、PMP 映射窗口控制、fault 装页、脏页写回和用户态验证程序。
- [artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task3/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task3/artifacts/tool_versions.txt)：工具版本。

## 5. 关键机制说明

### 5.1 `mmap` 如何建立映射

`mmap` syscall 的最小实现流程是：

1. 用户程序调用 [src/syscall.rs](/root/os_experiments/lab6/kernel_task3/src/syscall.rs) 的 `mmap(path, len)`；
2. 内核 `sys_mmap()` 检查路径和长度；
3. 对目标文件执行 `stat`，确认它是普通文件；
4. 内核记录映射状态，但初始不允许 U-mode 访问映射页；
5. 返回固定映射窗口地址给用户程序。

这时 `mmap` 只是建立了“文件 -> 映射窗口”的关系，还没有把文件内容真正搬到物理页里。

### 5.2 为什么首次访问会触发 fault

内核通过 `configure_pmp(false)` 把映射窗口单独设成 U-mode 不可访问。于是用户程序第一次读取该地址时，会触发一次访问异常：

- 本实验里观测到的是 `load-access fault`
- fault 地址就是映射窗口首地址

trap handler 在 [src/trap.rs](/root/os_experiments/lab6/kernel_task3/src/trap.rs) 中把这类 fault 交给 [src/main.rs](/root/os_experiments/lab6/kernel_task3/src/main.rs) 的 `handle_page_fault()` 处理。

### 5.3 fault 如何把文件读入映射页

`handle_page_fault()` 在确认 fault 地址属于映射窗口后，会执行：

1. `load_mapped_page()` 从文件 `/mapped.bin` 读取 `4096` 字节；
2. 把读取结果放入保留物理页 `MMAP_PAGE`；
3. 同时复制到 `MMAP_SHADOW`，作为后续脏页比较基线；
4. 再次配置 PMP，允许 U-mode 读写该映射页；
5. 返回到同一条用户指令重新执行。

因此 fault 返回后，用户程序读到的就是刚刚从文件读入的页内容。

### 5.4 脏页如何写回

实验实现了两种写回入口：

- `msync(addr, len)`
- `munmap(addr, len)`

它们都走 `flush_mapping()`：

1. 比较当前映射页 `MMAP_PAGE` 与快照 `MMAP_SHADOW`；
2. 若发现差异，则整页写回文件；
3. 写回成功后更新 `MMAP_SHADOW`；
4. 在 `munmap` 情况下，还会关闭映射状态并重新禁止访问该窗口。

用户程序分两次验证：

- 第一次修改两个字节后调用 `msync`；
- 第二次修改另外两个字节后调用 `munmap`；
- 每次之后都重新通过 `read_at` 从文件读取，确认文件内容与内存写入一致。

## 6. 构建、运行与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/kernel_task3
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task3 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task3 \
  > artifacts/run_output_repeat.txt 2>&1
```

记录工具版本：

```bash
{
  rustc --version
  cargo --version
  rustup target list | grep riscv64gc
  qemu-system-riscv64 --version | head -n 1
} > artifacts/tool_versions.txt
```

## 7. 本次实际运行结果

### 7.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task3/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab6_kernel_task3 v0.1.0 (/root/os_experiments/lab6/kernel_task3)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```

### 7.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 kernel task3 mmap file mapping
[kernel] mmap window: addr=0x80037b50 length=4096 bytes
[config] file_path=/mapped.bin mmap_len=4096 mmap_window=0x80037b50
[page-fault] cause=load-access addr=0x80037b50 loaded_bytes=4096
[mmap] returned_addr=0x80037b50 fault_elapsed_us=1423 initial_match=yes
[flush] msync_elapsed_us=1220 munmap_elapsed_us=381 msync_persisted=yes munmap_persisted=yes
[diag] mmap_calls=1 page_faults=1 pages_loaded=1 msync_writebacks=1 munmap_writebacks=1 dirty_detections=2 last_fault_addr=0x80037b50 last_loaded_bytes=4096 last_writeback_bytes=4096
[file-stat] inode=2 size_bytes=4096 device_id=0x4c360003 highest_level=direct
[acceptance] mmap triggers a file-backed page fault on first access: PASS
[acceptance] file data is loaded into the mapped physical page content: PASS
[acceptance] dirty page changes are written back on msync or munmap: PASS
```

从这组输出可以直接确认：

1. `mmap` 返回了固定映射窗口地址；
2. 首次读取该地址时触发了一个 file-backed first-touch fault；
3. fault 后读取结果和原文件内容一致；
4. 两次脏页修改都成功落回文件。

### 7.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task3/artifacts/run_output_repeat.txt) 再次得到同型结果：

```text
[page-fault] cause=load-access addr=0x80037b50 loaded_bytes=4096
[mmap] returned_addr=0x80037b50 fault_elapsed_us=1081 initial_match=yes
[flush] msync_elapsed_us=389 munmap_elapsed_us=369 msync_persisted=yes munmap_persisted=yes
[diag] mmap_calls=1 page_faults=1 pages_loaded=1 msync_writebacks=1 munmap_writebacks=1 dirty_detections=2 last_fault_addr=0x80037b50 last_loaded_bytes=4096 last_writeback_bytes=4096
[acceptance] mmap triggers a file-backed page fault on first access: PASS
[acceptance] file data is loaded into the mapped physical page content: PASS
[acceptance] dirty page changes are written back on msync or munmap: PASS
```

第二次运行说明：

- 映射窗口地址稳定；
- 首访 fault 和装页计数稳定；
- `msync`/`munmap` 的写回计数和文件一致性稳定。

### 7.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task3/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 8. 验收对照

- 调用 `mmap` 并触发文件备份区的 Page Fault。
  结果：通过。
  证据：首访映射窗口触发 `load-access fault`，在本最小模型里它承担 file-backed page fault 的角色；`page_faults=1`。
- 数据成功从磁盘被读入并作为该物理页内容。
  结果：通过。
  证据：`loaded_bytes=4096`，`initial_match=yes`。
- Dirty Page 在 `munmap` 或 `msync` 时能写回持久化。
  结果：通过。
  证据：`msync_persisted=yes`、`munmap_persisted=yes`，写回计数各为 `1`。

## 9. 环境说明与限制

- 这是 QEMU guest 内核态实验，不是宿主机 Linux `mmap(2)`。
- 当前实现是“单页映射 + 单文件 + 单 VMA”的最小模型，不支持多文件、多偏移、多页映射。
- 由于没有启用完整 Sv39 页表系统，首访 trap 采用 PMP 访问异常来模拟 file-backed page fault。
- 写回策略只实现了 `msync` 和 `munmap` 主动刷回，没有后台 writeback 守护线程。
