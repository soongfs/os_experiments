# LAB6 用户态 Task1：大文件写入压力测试

## 1. 原始任务说明

### 任务标题

大文件写入压力测试

### 任务目标

验证文件系统对大文件的支持能力，并为 inode 扩展提供验收依据。

### 任务要求

1. 编写程序持续向单一文件写入大量数据；
2. 记录写入总量与耗时；
3. 在实验记录中说明：为何该测试能验证三重间接 inode 的必要性。

### 验收检查

1. 文件大小显著超过单、双间接块寻址上限；
2. 再次读回大文件校验无丢数据或乱码；
3. 报告计算了三重间接块的理论最大文件容量。

## 2. Acceptance -> Evidence 清单

- 大文件写入发生在 QEMU guest 的 U-mode 用户程序里，而不是宿主机进程。
  证据：QEMU 日志先打印 `[kernel] ... guest large-file stress test`，随后出现 `[user] large-file test started in U-mode`，见 [artifacts/run_output.txt](/root/os_experiments/lab6/task1/artifacts/run_output.txt)。
- 单文件大小显著超过 single/double indirect 上限。
  证据：`target_bytes=16777216`，而 `double_limit_bytes=8459264`，`over_double_ratio=1.983x`。
- 读回校验无丢数据或乱码。
  证据：两次运行的 `[readback]` 都显示 `mismatches=0`，checksum 一致为 `0x55b7ba0cfaaa2866`。
- 三重间接映射不只是理论计算，而是实际被触发。
  证据：`[fs-stat] ... highest_level=triple`，并且 `[acceptance] triple-indirect mapping was actually used: PASS`。
- 三重间接理论容量已计算并输出。
  证据：日志和本 README 都给出 `triple_limit_bytes=1082201088`。

## 3. 实验目标与实现思路

本实验在 [lab6/task1](/root/os_experiments/lab6/task1) 中实现为真正的 QEMU/RISC-V 裸机任务，不再使用宿主机 WSL 用户态程序。运行方式是：

- 内核从 M-mode 启动；
- 通过 `mret` 切换到 U-mode `user_entry`；
- 用户程序通过 `ecall` 进入内核；
- 内核在 guest 内存中维护一个最小内存文件系统，并执行路径型文件系统 syscall。

这次实现故意采用“最小可验证模型”，以便把验收聚焦在 inode 扩展逻辑本身，而不是块设备驱动或持久化细节：

- 文件系统是 in-memory 的，不接真实磁盘；
- 支持 `create_file`、`write_at`、`read_at`、`stat`、`list_dir`、`remove` 等最小接口；
- inode 数据块寻址采用固定模型：
  - `10` 个 direct
  - `1` 个 single indirect
  - `1` 个 double indirect
  - `1` 个 triple indirect
- 为了控制 QEMU 内存占用，本实验选择 `512` 字节块大小，因此双间接上限约 `8.06 MiB`，再写入 `16 MiB` 单文件即可稳定跨过 double ceiling。

这个模型省略了两件事情，并在此明确声明：

1. 没有真实块设备持久化；
2. 删除文件时不回收数据块到 free list。

这两点不影响本题要验证的核心机制：当单文件大小越过 direct + single + double 的可寻址上限时，内核是否能继续通过 triple indirect 为后续逻辑块分配映射，并在读回时保持数据一致。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/task1/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab6/task1/.cargo/config.toml)：固定目标三元组和链接脚本。
- [linker.ld](/root/os_experiments/lab6/task1/linker.ld)：镜像布局与 guest 内存大小，本任务将 RAM 扩到 `128M`。
- [src/boot.S](/root/os_experiments/lab6/task1/src/boot.S)：M-mode 启动、trap 保存现场和 `enter_user_mode`。
- [src/console.rs](/root/os_experiments/lab6/task1/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/task1/src/trap.rs)：U-mode `ecall` trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/task1/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/task1/src/abi.rs)：syscall 号、错误码、`FsStat` 和 inode 模型常量。
- [src/syscall.rs](/root/os_experiments/lab6/task1/src/syscall.rs)：用户态 syscall 封装。
- [src/fs.rs](/root/os_experiments/lab6/task1/src/fs.rs)：内存文件系统核心，包含路径解析、目录项管理和 direct/single/double/triple block mapping。
- [src/main.rs](/root/os_experiments/lab6/task1/src/main.rs)：guest 内核入口、syscall 实现和 U-mode 大文件压力测试程序。
- [artifacts/build_output.txt](/root/os_experiments/lab6/task1/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/task1/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task1/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/task1/artifacts/tool_versions.txt)：Rust/QEMU 版本信息。

## 5. inode 模型、测量边界与复现步骤

### 5.1 inode 模型

本实验采用以下教学模型：

- `BSIZE = 512` 字节
- 每个块号 `4` 字节
- 每个间接块可容纳 `512 / 4 = 128` 个块号
- inode 固定块地址槽位：
  - `10` 个 direct
  - `1` 个 single indirect
  - `1` 个 double indirect
  - `1` 个 triple indirect

由此得到：

- 单间接上限：
  - `(10 + 128) * 512 = 70656` 字节
- 双间接上限：
  - `(10 + 128 + 128^2) * 512 = 8459264` 字节
- 三重间接理论最大容量：
  - `(10 + 128 + 128^2 + 128^3) * 512 = 1082201088` 字节

### 5.2 工作负载参数

- 目标文件路径：`/triple_data.bin`
- 目标文件大小：`16777216` 字节，即 `16 MiB`
- 用户态写入粒度：`4096` 字节
- 目标文件大小相对双间接上限的比例：`1.983x`

### 5.3 计时边界

- 写入计时：
  - 起点：U-mode 开始第一次 `write_at`
  - 终点：最后一次 `write_at` 返回
- 读回计时：
  - 起点：U-mode 开始第一次 `read_at`
  - 终点：最后一次比较完成
- 时间来源：
  - guest 内核读取 `mtime`
  - 按 `10 MHz` timebase 换算为微秒后，经 `SYS_TIME_US` 返回给用户态

### 5.4 构建与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/task1
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 128M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_task1 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 128M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_task1 \
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

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab6/task1/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab6_task1 v0.1.0 (/root/os_experiments/lab6/task1)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
```

### 6.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/task1/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 task1 guest large-file stress test
[kernel] fs model: block_size=512 direct=10 pointers_per_indirect=128 triple_limit_bytes=1082201088
[user] large-file test started in U-mode
[inode-model] single_limit_bytes=70656 double_limit_bytes=8459264 triple_limit_bytes=1082201088
[config] path=/triple_data.bin target_bytes=16777216 chunk_bytes=4096 over_double_bytes=8317952 over_double_ratio=1.983x
[write] bytes=16777216 duration_us=1944977 kib_per_s=8423 checksum=0x55b7ba0cfaaa2866
[fs-stat] size_bytes=16777216 blocks_used=32768 highest_level=triple
[readback] bytes=16777216 duration_us=4079003 kib_per_s=4016 checksum=0x55b7ba0cfaaa2866 mismatches=0
[acceptance] file size exceeds single and double indirect limits: PASS
[acceptance] file data readback matches written pattern: PASS
[acceptance] triple-indirect mapping was actually used: PASS
[acceptance] triple-indirect theoretical capacity reported: PASS
```

从这组数据可以直接读出：

1. 单文件实际大小 `16 MiB`，明显大于双间接上限 `8459264` 字节；
2. inode 统计 `highest_level=triple`，说明不是“只靠理论计算”，而是实际分配到了 triple indirect；
3. 读回 `mismatches=0`，且 checksum 前后一致；
4. 写入和读回耗时都来自 guest 内部计时，而不是宿主 shell。

### 6.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task1/artifacts/run_output_repeat.txt) 再次得到同型结果：

```text
[write] bytes=16777216 duration_us=1942862 kib_per_s=8432 checksum=0x55b7ba0cfaaa2866
[fs-stat] size_bytes=16777216 blocks_used=32768 highest_level=triple
[readback] bytes=16777216 duration_us=4063278 kib_per_s=4032 checksum=0x55b7ba0cfaaa2866 mismatches=0
[acceptance] file size exceeds single and double indirect limits: PASS
[acceptance] file data readback matches written pattern: PASS
[acceptance] triple-indirect mapping was actually used: PASS
[acceptance] triple-indirect theoretical capacity reported: PASS
```

第二次运行说明：

- guest 内写入总量和 inode 层级统计稳定一致；
- checksum 稳定一致；
- 变化只有轻微的运行时间抖动，不影响验收结论。

### 6.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/task1/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 7. 机制解释

### 7.1 为什么这个测试能验证三重间接 inode 的必要性

文件系统能否支持“大文件”，关键不在于 `write()` 能不能被调用很多次，而在于同一个 inode 是否还有足够的块映射空间继续增长。

在本实验模型下：

- direct + single 只能覆盖 `70656` 字节；
- direct + single + double 只能覆盖 `8459264` 字节；
- 目标文件是 `16777216` 字节。

这意味着：当用户程序继续把文件从 `8.06 MiB` 推到 `16 MiB` 时，内核已经无法继续只靠 direct/single/double 为逻辑块编号找到数据块位置。若不引入 triple indirect，后半段写入必然会在映射层失败。

本实验比“只算公式”更进一步，因为它还给出：

- 运行时 inode 统计 `highest_level=triple`
- 实际 `blocks_used=32768`
- 完整读回零错配

所以这里验证的是：不但理论上需要 triple indirect，而且 guest 内核的这份最小文件系统实现也真的把它用起来了。

### 7.2 为什么还要做整文件读回

如果只看文件大小和块映射层级，还无法排除：

- 后半段块号映射错了；
- 三重间接跨层索引时写到了错误块；
- 文件长度更新成功，但数据内容被覆盖或错位。

因此用户态测试程序会：

1. 按绝对偏移生成 deterministic pattern；
2. 顺序写完整个大文件；
3. 再按同样的偏移顺序读回；
4. 逐字节比较并汇总 checksum。

这就是为什么 `[readback] ... mismatches=0` 是不可省略的证据。

## 8. 验收检查映射

- [x] 文件大小显著超过单、双间接块寻址上限。
  证据：`target_bytes=16777216`，`double_limit_bytes=8459264`，`over_double_ratio=1.983x`。
- [x] 再次读回大文件校验无丢数据或乱码。
  证据：两次运行都 `mismatches=0`，checksum 恒为 `0x55b7ba0cfaaa2866`。
- [x] 报告计算了三重间接块的理论最大文件容量。
  证据：`triple_limit_bytes=1082201088`，见第 5.1 节和 QEMU 日志。

## 9. 环境说明、限制与未决事项

- 本次结果来自 QEMU guest，不是宿主机 WSL 进程。
- 文件系统是教学用最小内存模型，不接真实块设备，也不验证持久化重启恢复。
- 删除文件不回收数据块，这是为了把实验重心固定在 inode 扩展和数据一致性上。
- 如果后续 `LAB6` 需要接入真实 virtio block 或镜像文件，可在当前 `fs.rs` 的路径解析和 inode/block mapping 逻辑外再接一层块缓存与持久化层。
