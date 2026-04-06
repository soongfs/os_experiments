# LAB6 内核态 Task2：stat 系统调用支持

## 1. 原始任务说明

### 任务标题

stat 系统调用支持

### 任务目标

实现文件元数据系统调用，完成 inode 信息到用户态的结构化输出。

### 任务要求

1. 实现 `stat`，输出 inode 元数据；
2. 处理错误路径：文件不存在、路径非法等；
3. 与用户态核对程序一致。

### 验收检查

1. 系统调用能正确检索到文件或目录所在的 Inode；
2. 完成了从磁盘元数据到用户态 `stat` 结构体的正确格式转换并回写内存。

## 2. Acceptance -> Evidence 清单

- `stat` syscall 必须在 QEMU guest 内核中实现并由 U-mode 用户程序触发。
  证据：QEMU 日志显示 `[kernel] LAB6 kernel task2 stat syscall support` 和后续 U-mode `[stat]`、`[kernel-stat]`、`[acceptance]` 输出，见 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output.txt)。
- 文件和目录路径都必须被解析到正确的 inode。
  证据：目录 `/meta` 的 `Inode Number=2`，文件 `/meta/sample.txt` 的 `Inode Number=3`，并且对照行全部 `match=yes`。
- `FsStat` 结构体必须被正确填充并回写到用户缓冲区。
  证据：用户态打印出的 `Type`、`File Size`、`Inode Number`、`Device ID`、`Blocks Used`、`Child Count`、时间戳都与实际对象一致；内核诊断显示 `successful_copyouts=2`。
- 错误路径必须返回合理错误码。
  证据：`missing_path_result=-2`、`invalid_path_result=-22`、`bad_buffer_result=-14`，对应 `ENOENT`、`EINVAL`、`EFAULT`。
- 运行结果必须稳定。
  证据：两次 QEMU 运行 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output_repeat.txt) 的关键数字一致，所有验收项都为 `PASS`。

## 3. 实验环境与实现思路

本实验运行在 QEMU 的 RISC-V bare-metal guest 环境中，属于 `LAB6` 内核态任务。内核从 M-mode 启动，随后进入 U-mode 用户程序；用户程序通过 `ecall` 调用 `SYS_STAT`，内核在 guest 内部执行路径解析、inode 元数据读取和用户态 copyout。

仓库当前没有独立的真实 easy-fs 仓库，因此这里继续使用前几题的最小 teaching fs 模型，但把重点放在 `stat` syscall 的机制正确性：

- 在 inode 中维护 `inode_number`、`size_bytes`、`device_id`、`created_us`、`modified_us` 等 metadata；
- `sys_stat()` 负责校验用户态路径指针和输出缓冲区；
- `fs::stat()` 负责根据路径找到 inode，并把 metadata 转换为 `FsStat`；
- 增加 `StatDiagnostics`，记录 `stat_calls`、`successful_lookups`、`failed_lookups`、`successful_copyouts`、`last_error` 等内核态证据；
- U-mode 核对程序同时覆盖成功路径和错误路径。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/kernel_task2/Cargo.toml)：Rust 裸机工程配置，二进制名为 `lab6_kernel_task2`。
- [.cargo/config.toml](/root/os_experiments/lab6/kernel_task2/.cargo/config.toml)：RISC-V 目标与链接配置。
- [linker.ld](/root/os_experiments/lab6/kernel_task2/linker.ld)：内存布局与栈布局。
- [src/boot.S](/root/os_experiments/lab6/kernel_task2/src/boot.S)：启动和 trap 入口汇编。
- [src/console.rs](/root/os_experiments/lab6/kernel_task2/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/kernel_task2/src/trap.rs)：U-mode `ecall` trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/kernel_task2/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/kernel_task2/src/abi.rs)：`FsStat`、`StatDiagnostics`、错误码和 syscall 号定义。
- [src/syscall.rs](/root/os_experiments/lab6/kernel_task2/src/syscall.rs)：用户态 syscall 封装，包含 `stat()`、`stat_bad_buffer()`、`stat_diag()`。
- [src/fs.rs](/root/os_experiments/lab6/kernel_task2/src/fs.rs)：最小文件系统和 `stat` metadata 导出逻辑。
- [src/main.rs](/root/os_experiments/lab6/kernel_task2/src/main.rs)：内核入口、`sys_stat` 实现、用户态核对程序和错误路径测试。
- [artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task2/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task2/artifacts/tool_versions.txt)：工具版本。

## 5. 关键机制说明

### 5.1 `stat` syscall 流程

`stat` 的 guest 内执行路径是：

1. U-mode 用户程序调用 [src/syscall.rs](/root/os_experiments/lab6/kernel_task2/src/syscall.rs) 的 `stat()`；
2. `ecall` 进入 trap，`handle_syscall()` 分发到 [src/main.rs](/root/os_experiments/lab6/kernel_task2/src/main.rs) 的 `sys_stat()`；
3. `sys_stat()` 先检查路径指针和输出缓冲区：
   - 非法路径返回 `EINVAL`
   - 空/坏用户指针返回 `EFAULT`
4. 参数合法后调用 [src/fs.rs](/root/os_experiments/lab6/kernel_task2/src/fs.rs) 的 `fs::stat()`；
5. `fs::stat()` 通过 `lookup_path()` 找到对应 inode，把 metadata 填入 `FsStat` 并回写用户态结构体。

### 5.2 为什么这能证明“检索到了正确 inode”

当前最小文件系统的 inode 分配是确定性的：

- 根目录 inode 为 `1`
- 新建目录 `/meta` 为 inode `2`
- 新建文件 `/meta/sample.txt` 为 inode `3`

运行结果显示：

- 目录 `Inode Number=2`
- 文件 `Inode Number=3`

并且内核诊断中 `successful_lookups=2`，说明成功路径上的两次 `stat` 都真实完成了 inode 查找。

### 5.3 为什么这能证明“格式转换并回写内存正确”

`FsStat` 在 [src/abi.rs](/root/os_experiments/lab6/kernel_task2/src/abi.rs) 中定义为结构化用户态输出缓冲区。测试程序在调用 `stat()` 之前，先把目录和文件的 `FsStat` 缓冲区写成毒值；如果 copyout 不完整或字段映射错误，后续比较会立即失败。

最后实际观测到：

- `File Size=27`
- `Device ID=0x4c360002`
- 目录 `Child Count=1`
- 文件 `Blocks Used=1`
- 时间戳均被写入且顺序合理

同时内核诊断显示 `successful_copyouts=2`，表明成功路径上的两次 `stat` 都完成了结构体回写。

### 5.4 错误路径覆盖

本题额外验证三类错误路径：

- 不存在路径 `/missing/stat.txt` 返回 `ENOENT(-2)`
- 非法相对路径 `relative/path` 返回 `EINVAL(-22)`
- 空用户输出缓冲区返回 `EFAULT(-14)`

因此这题不仅验证成功路径，也验证 `sys_stat` 的参数校验和错误返回行为。

## 6. 构建、运行与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/kernel_task2
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task2 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task2 \
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

[artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task2/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
```

### 7.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 kernel task2 stat syscall support
[config] dir_path=/meta file_path=/meta/sample.txt payload_bytes=27 invalid_path=relative/path missing_path=/missing/stat.txt
[stat] label=directory path=/meta
  Inode Number: 2
  Device ID: 0x4c360002
[stat] label=file path=/meta/sample.txt
  File Size: 27
  Inode Number: 3
  Device ID: 0x4c360002
[kernel-stat] calls=5 successful_lookups=2 failed_lookups=3 successful_copyouts=2 last_inode=3 last_kind=1 last_size=27 last_error=-14
[errors] missing_path_result=-2 invalid_path_result=-22 bad_buffer_result=-14
[compare] file_size expected=27 actual=27 match=yes
[compare] file_inode expected=3 actual=3 match=yes
[compare] file_device expected=0x4c360002 actual=0x4c360002 match=yes
[compare] dir_inode expected=2 actual=2 match=yes
[compare] dir_device expected=0x4c360002 actual=0x4c360002 match=yes
[acceptance] stat locates the correct inode for file and directory paths: PASS
[acceptance] metadata is converted into user-space stat buffers correctly: PASS
[acceptance] missing file, invalid path, and bad user buffer return expected errors: PASS
[acceptance] kernel stat diagnostics report lookup and copyout activity: PASS
```

从这组结果可以直接确认：

1. 文件和目录都解析到了正确 inode；
2. `FsStat` 的关键字段被正确填充并回写到用户态；
3. 三类错误路径都返回了预期错误码；
4. 内核诊断数据和用户态比较结果是一致的。

### 7.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task2/artifacts/run_output_repeat.txt) 再次得到同型结果：

```text
[kernel-stat] calls=5 successful_lookups=2 failed_lookups=3 successful_copyouts=2 last_inode=3 last_kind=1 last_size=27 last_error=-14
[errors] missing_path_result=-2 invalid_path_result=-22 bad_buffer_result=-14
[compare] file_size expected=27 actual=27 match=yes
[compare] file_inode expected=3 actual=3 match=yes
[compare] file_device expected=0x4c360002 actual=0x4c360002 match=yes
[compare] dir_inode expected=2 actual=2 match=yes
[compare] dir_device expected=0x4c360002 actual=0x4c360002 match=yes
[acceptance] stat locates the correct inode for file and directory paths: PASS
[acceptance] metadata is converted into user-space stat buffers correctly: PASS
[acceptance] missing file, invalid path, and bad user buffer return expected errors: PASS
[acceptance] kernel stat diagnostics report lookup and copyout activity: PASS
```

第二次运行说明：

- lookup/copyout 计数稳定；
- 文件和目录的 metadata 对照稳定；
- 错误码稳定，不依赖偶然条件。

### 7.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task2/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 8. 验收对照

- 系统调用能正确检索到文件或目录所在的 Inode。
  结果：通过。
  证据：目录 inode=`2`、文件 inode=`3`，并且 `successful_lookups=2`。
- 完成了从磁盘元数据到用户态 `stat` 结构体的正确格式转换并回写内存。
  结果：通过。
  证据：用户态 `FsStat` 字段比较全部 `match=yes`，`successful_copyouts=2`。

## 9. 环境说明与限制

- 这是 QEMU guest 内核态实验，不是宿主机 Linux `stat(2)`。
- 当前文件系统是最小 teaching fs 模型，不包含真实磁盘持久化、权限位、用户身份和硬链接语义。
- 这里的“磁盘元数据”指的是该最小模型中的 inode metadata，而不是真实块设备上的 POSIX 文件系统镜像。
