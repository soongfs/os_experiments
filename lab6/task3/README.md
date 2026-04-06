# LAB6 用户态 Task3：stat 元数据读取与核对

## 1. 原始任务说明

### 任务标题

stat 元数据读取与核对

### 任务目标

理解文件元数据语义，验证内核/文件系统对元数据的正确导出。

### 任务要求

1. 使用 `stat` 读取 inode 元数据；
2. 与实际文件属性对照（大小、类型、时间戳等视实现支持而定）；
3. 输出格式清晰，并说明无法支持的字段原因（若有）。

### 验收检查

1. 终端成功打印并格式化 `stat` 结构体内容；
2. 解析出的 `File Size`、`Inode Number`、`Device ID` 必须与实际文件系统吻合。

## 2. Acceptance -> Evidence 清单

- `stat` 测试必须运行在 QEMU guest U-mode，而不是宿主机 Linux 进程。
  证据：QEMU 日志以 `[kernel] LAB6 task3 guest stat metadata validation` 开头，随后出现 guest 内 syscall 驱动的 `[stat]`、`[compare]`、`[acceptance]` 输出，见 [artifacts/run_output.txt](/root/os_experiments/lab6/task3/artifacts/run_output.txt)。
- 终端必须格式化打印 `stat` 结构体内容。
  证据：日志中分别打印目录 `/meta` 和文件 `/meta/sample.txt` 的 `Type`、`File Size`、`Inode Number`、`Device ID`、`Blocks Used`、`Child Count`、`Mapping Level`、`Created Timestamp (us)`、`Modified Timestamp (us)`。
- `File Size`、`Inode Number`、`Device ID` 必须与实际文件系统对象一致。
  证据：日志包含 `file_size`、`file_inode`、`file_device`、`dir_inode`、`dir_device` 的显式对照行，全部 `match=yes`。
- 无法支持的字段原因必须被明确说明。
  证据：日志包含 `[support] unsupported_fields=uid,gid,mode,nlink reason=...`。
- 运行结果需要稳定。
  证据：两次 QEMU 运行 [artifacts/run_output.txt](/root/os_experiments/lab6/task3/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task3/artifacts/run_output_repeat.txt) 都打印相同的 metadata 结论和 `PASS` 验收行。

## 3. 实验环境与实现思路

本实验运行在 QEMU 的 RISC-V bare-metal guest 环境中，不是宿主机 WSL 用户态程序。内核在 M-mode 启动后切换到 U-mode 用户程序，用户程序通过 `stat` syscall 请求最小内存文件系统导出 inode 元数据。

本题复用 `lab6/task1` 和 `lab6/task2` 的最小 guest 文件系统脚手架，但把重点放在 metadata 导出：

1. 先在 guest 内创建目录 `/meta` 和文件 `/meta/sample.txt`；
2. 向文件写入固定 27 字节负载；
3. 分别对目录和文件执行 `stat`；
4. 将 `FsStat` 结构体格式化打印到终端；
5. 把导出的 `File Size`、`Inode Number`、`Device ID` 与实际对象逐项对照；
6. 说明哪些 POSIX 字段没有实现，以及为什么没有实现。

这里仍然使用最小内存文件系统，而不是完整磁盘文件系统。这样做是为了把验证集中在“元数据语义是否被正确导出”，而不是引入块设备、持久化、权限模型等无关复杂度。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/task3/Cargo.toml)：Rust 裸机工程配置，二进制名为 `lab6_task3`。
- [.cargo/config.toml](/root/os_experiments/lab6/task3/.cargo/config.toml)：固定目标三元组和链接参数。
- [linker.ld](/root/os_experiments/lab6/task3/linker.ld)：镜像布局和栈符号导出。
- [src/boot.S](/root/os_experiments/lab6/task3/src/boot.S)：M-mode 启动和 trap 入口汇编。
- [src/console.rs](/root/os_experiments/lab6/task3/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/task3/src/trap.rs)：syscall trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/task3/src/user_console.rs)：U-mode 格式化输出。
- [src/syscall.rs](/root/os_experiments/lab6/task3/src/syscall.rs)：用户态 syscall 封装。
- [src/abi.rs](/root/os_experiments/lab6/task3/src/abi.rs)：`FsStat` 布局、文件种类、设备号和错误码定义。
- [src/fs.rs](/root/os_experiments/lab6/task3/src/fs.rs)：最小 inode/file/dir 实现，负责维护 `inode_number`、`device_id`、`size_bytes`、`created_us`、`modified_us` 等 metadata。
- [src/main.rs](/root/os_experiments/lab6/task3/src/main.rs)：guest 内核入口、`stat` syscall 处理和 U-mode 测试程序。
- [artifacts/build_output.txt](/root/os_experiments/lab6/task3/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/task3/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task3/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/task3/artifacts/tool_versions.txt)：Rust/QEMU 版本信息。

## 5. 关键机制说明

### 5.1 `stat` 元数据如何生成

`FsStat` 在 [src/abi.rs](/root/os_experiments/lab6/task3/src/abi.rs) 中定义，字段包括：

- `kind`
- `highest_level`
- `inode_number`
- `device_id`
- `size_bytes`
- `blocks_used`
- `child_count`
- `created_us`
- `modified_us`

当用户程序发起 `SYS_STAT` 时，[src/main.rs](/root/os_experiments/lab6/task3/src/main.rs) 的 `sys_stat()` 会先校验用户态缓冲区，再调用 [src/fs.rs](/root/os_experiments/lab6/task3/src/fs.rs) 的 `fs::stat()`，把对应 inode 的 metadata 拷贝到 `FsStat`。

### 5.2 为什么 `File Size`、`Inode Number`、`Device ID` 可以核对

- `File Size`
  文件 `/meta/sample.txt` 写入固定字节串 `guest stat metadata sample\n`，长度恒为 `27` 字节，因此 `stat.size_bytes` 必须等于 `27`。
- `Inode Number`
  这个最小文件系统在分配 inode 时使用确定性编号规则 `inode_index + 1`。根目录是 inode `1`，因此新建目录 `/meta` 是 `2`，其下文件 `/meta/sample.txt` 是 `3`。
- `Device ID`
  该教学文件系统导出固定设备号 `0x4c360001`，用于表示同一 guest 文件系统实例。目录和文件的 `Device ID` 都必须等于该值。

### 5.3 时间戳语义

本实现支持最小版本的 metadata 时间戳，但它不是 POSIX 实时时钟，而是一个单调递增的 metadata clock：

- inode 创建时填充 `created_us`
- 创建子项、写文件、删除对象等元数据变更会刷新 `modified_us`

因此本题验证的是“内核能正确导出创建/修改次序”，不是“与宿主机真实壁钟时间一致”。

### 5.4 无法支持字段的原因

本实验的目标是验证 inode metadata 导出，而不是复刻完整 POSIX VFS 层，因此以下字段没有实现：

- `uid`
- `gid`
- `mode`
- `nlink`

原因是当前最小 teaching fs 没有用户身份、权限位、硬链接计数等模型，只导出本题验收真正需要的种类、大小、inode 编号、设备号、块使用量和时间戳。

## 6. 构建、运行与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/task3
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_task3 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_task3 \
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

[artifacts/build_output.txt](/root/os_experiments/lab6/task3/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab6_task3 v0.1.0 (/root/os_experiments/lab6/task3)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
```

### 7.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/task3/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 task3 guest stat metadata validation
[config] dir_path=/meta file_path=/meta/sample.txt payload_bytes=27
[stat] label=directory path=/meta
  Type: directory
  File Size: 0
  Inode Number: 2
  Device ID: 0x4c360001
  Blocks Used: 0
  Child Count: 1
  Mapping Level: direct
  Created Timestamp (us): 2
  Modified Timestamp (us): 5
[stat] label=file path=/meta/sample.txt
  Type: regular
  File Size: 27
  Inode Number: 3
  Device ID: 0x4c360001
  Blocks Used: 1
  Child Count: 0
  Mapping Level: direct
  Created Timestamp (us): 4
  Modified Timestamp (us): 6
[compare] file_size expected=27 actual=27 match=yes
[compare] file_inode expected=3 actual=3 match=yes
[compare] file_device expected=0x4c360001 actual=0x4c360001 match=yes
[compare] dir_inode expected=2 actual=2 match=yes
[compare] dir_device expected=0x4c360001 actual=0x4c360001 match=yes
[support] unsupported_fields=uid,gid,mode,nlink reason=minimal teaching fs exports only kind,size,inode,device,timestamps,block usage
[timing] setup_us=2061 stat_us=226 cleanup_us=1249
[acceptance] stat structure printed in formatted form: PASS
[acceptance] file size, inode number, and device id match actual filesystem values: PASS
[acceptance] directory inode number and device id match actual filesystem values: PASS
[acceptance] supported timestamp fields are populated consistently: PASS
```

从这组输出可以直接确认：

1. `stat` 内容被终端成功格式化打印；
2. 文件 `File Size=27` 与实际写入字节数一致；
3. 目录和文件的 `Inode Number` 分别为 `2` 和 `3`，符合 inode 分配规则；
4. 目录和文件的 `Device ID` 都是 `0x4c360001`，与同一文件系统实例一致。

### 7.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task3/artifacts/run_output_repeat.txt) 再次得到相同 metadata 结果：

```text
[compare] file_size expected=27 actual=27 match=yes
[compare] file_inode expected=3 actual=3 match=yes
[compare] file_device expected=0x4c360001 actual=0x4c360001 match=yes
[compare] dir_inode expected=2 actual=2 match=yes
[compare] dir_device expected=0x4c360001 actual=0x4c360001 match=yes
[timing] setup_us=1984 stat_us=204 cleanup_us=276
[acceptance] stat structure printed in formatted form: PASS
[acceptance] file size, inode number, and device id match actual filesystem values: PASS
[acceptance] directory inode number and device id match actual filesystem values: PASS
[acceptance] supported timestamp fields are populated consistently: PASS
```

两次运行说明：

- 核心 metadata 字段稳定；
- guest 内 `stat` 路径和比较逻辑没有一次性偶然成功；
- 微秒级时间存在波动，但不影响元数据语义验证。

### 7.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/task3/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 8. 验收对照

- 终端成功打印并格式化 `stat` 结构体内容。
  结果：通过。
  证据：两次运行都打印了目录和文件的完整 `stat` 字段块。
- 解析出的 `File Size`、`Inode Number`、`Device ID` 与实际文件系统吻合。
  结果：通过。
  证据：`file_size`、`file_inode`、`file_device`、`dir_inode`、`dir_device` 全部 `match=yes`。

## 9. 环境说明与限制

- 本实验运行于 QEMU guest，不是宿主机 Linux 文件系统。
- 文件系统是最小内存模型，不包含真实块设备持久化。
- 时间戳是内部单调 metadata clock，不是 POSIX wall-clock 时间。
- `uid/gid/mode/nlink` 等字段未实现，因为当前模型不包含用户、权限和硬链接语义。
