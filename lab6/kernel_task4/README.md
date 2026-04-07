# LAB6 内核态 Task4：目录结构扩展

## 1. 原始任务说明

### 任务标题

目录结构扩展

### 任务目标

从单级目录扩展到多级目录，掌握路径解析与目录项管理。

### 任务要求

1. 支持二级目录结构（进阶：支持 N 级）；
2. 支持基本操作：创建/删除/遍历；
3. 提供深目录树测试验收。

### 验收检查

1. 文件系统支持将目录当作一种特殊的 Inode 进行读写（存入 Dirent）；
2. 路径名解析器（Path Resolver）能用 `/` 切割并递归查找下一级 Inode 块。

## 2. Acceptance -> Evidence 清单

- 目录必须作为特殊 inode 持有 `Dirent` 内容，而不是只做平面名称表。
  证据：内核日志打印 `dirent_bytes_per_inode=32`，目录 `/a/b/c/d/e/f/g/h` 的 `dir-stat` 显示 `child_count=1 dirent_bytes=32`，说明目录 inode 的内容大小正好等于一个 `Dirent`。
- 路径解析器必须按 `/` 切分路径并递归查找。
  证据：深目录树 8 级遍历全部成功，诊断里 `max_resolve_depth=9`、`path_components_split=100`，见 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output.txt)。
- 创建/删除/遍历和叶子文件操作必须全部成功。
  证据：`directories_created=8 directories_traversed=8 directories_removed=8 file_bytes=40`，并且三条 `acceptance` 都是 `PASS`。
- 错误路径必须稳定返回。
  证据：`existing_dir=17 missing_intermediate=2 path_too_long=36`，分别对应 `EEXIST`、`ENOENT`、`ENAMETOOLONG`。
- 运行结果必须稳定。
  证据：两次 QEMU 运行 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output.txt) 与 [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output_repeat.txt) 的目录结构和解析诊断一致。

## 3. 实验环境与实现思路

本实验运行在 QEMU 的 RISC-V bare-metal guest 环境中，属于 `LAB6` 内核态任务。内核从 M-mode 启动，进入 U-mode 用户程序后，通过 syscall 驱动目录树创建、遍历、文件读写和删除。

这次实现的重点不是“能创建很多路径”本身，而是把目录真正建模成一种特殊 inode：

- 普通文件 inode 继续通过数据块保存文件内容；
- 目录 inode 则通过内部 `Dirent[]` 保存目录项；
- 每个 `Dirent` 记录 `name` 和目标 `inode_index`；
- `lookup_path()` 不再一次性线性处理整个字符串，而是走递归 `resolve_components()`，逐级按 `/` 切分并在当前目录的 `Dirent[]` 中查找下一层。

因此这份实现已经覆盖：

- 二级目录结构；
- N 级目录扩展；
- 创建、删除、遍历；
- 深目录树验收。

这里仍然是最小 teaching fs 模型，不包含真实磁盘镜像、目录块回收和完整 POSIX 权限系统。实验目标是清楚地验证目录 inode 和 path resolver 机制，而不是复刻完整 easy-fs。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/kernel_task4/Cargo.toml)：Rust 裸机工程配置，二进制名为 `lab6_kernel_task4`。
- [.cargo/config.toml](/root/os_experiments/lab6/kernel_task4/.cargo/config.toml)：RISC-V 目标与链接配置。
- [linker.ld](/root/os_experiments/lab6/kernel_task4/linker.ld)：内存布局与栈布局。
- [src/boot.S](/root/os_experiments/lab6/kernel_task4/src/boot.S)：启动和 trap 入口汇编。
- [src/console.rs](/root/os_experiments/lab6/kernel_task4/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/kernel_task4/src/trap.rs)：U-mode `ecall` trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/kernel_task4/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/kernel_task4/src/abi.rs)：`FsStat`、`DirDiagnostics`、错误码和 syscall 号定义。
- [src/syscall.rs](/root/os_experiments/lab6/kernel_task4/src/syscall.rs)：用户态 syscall 封装，包含 `dir_diag()`。
- [src/fs.rs](/root/os_experiments/lab6/kernel_task4/src/fs.rs)：目录 inode、`Dirent` 结构、递归路径解析和文件/目录基本操作。
- [src/main.rs](/root/os_experiments/lab6/kernel_task4/src/main.rs)：内核入口、syscall 处理和深目录树验收程序。
- [artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task4/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task4/artifacts/tool_versions.txt)：工具版本。

## 5. 关键机制说明

### 5.1 目录 inode 如何保存 `Dirent`

[src/fs.rs](/root/os_experiments/lab6/kernel_task4/src/fs.rs) 定义了：

- `Dirent`
- `Inode`

其中 `Inode` 对目录使用：

- `dirents: [Dirent; FS_MAX_DIR_ENTRIES]`
- `child_count`
- `size_bytes`

目录新增子项时，`add_dirent()` 会：

1. 在当前目录 inode 的空闲 `Dirent` 槽位中写入名字和目标 inode；
2. `child_count += 1`；
3. 把 `size_bytes` 更新为 `child_count * size_of::<Dirent>()`。

因此目录 inode 的“内容”就是一组 `Dirent`，这正是本题要验证的目录项组织模型。

### 5.2 路径解析器如何递归下钻

路径解析不再是平铺搜索，而是：

1. `lookup_path()` 做基础校验；
2. `resolve_components()` 从根 inode 开始；
3. 每次遇到 `/` 时切出下一个 path component；
4. 在当前目录的 `Dirent[]` 里查找对应子 inode；
5. 若还有剩余 component，则递归进入下一层目录。

运行时诊断：

- `resolve_calls=119`
- `path_components_split=100`
- `max_resolve_depth=9`

说明它不是简单的一级目录匹配，而是真实按层级递归解析。

### 5.3 深目录树为什么能证明目录结构扩展成功

本实验的目录树是：

`/a/b/c/d/e/f/g/h`

用户态验证程序做了一个闭环：

1. 逐级 `mkdir` 创建 8 级目录；
2. 从 `/` 开始逐级 `list_dir()`，确认每一级目录里都能找到下一层；
3. 在叶子目录创建并读回 `leaf_payload.txt`；
4. 再自底向上删除整个目录树。

如果目录项组织或路径解析存在问题，这四步中的任一步都会暴露出来。

## 6. 构建、运行与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/kernel_task4
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task4 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task4 \
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

[artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task4/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
```

### 7.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 kernel task4 directory structure expansion
[config] depth=8 example_prefix=/a/b/c/d/e path_max=256
[mkdir] depth=1 path=/a
[mkdir] depth=2 path=/a/b
[mkdir] depth=3 path=/a/b/c
[mkdir] depth=4 path=/a/b/c/d
[mkdir] depth=5 path=/a/b/c/d/e
[mkdir] depth=6 path=/a/b/c/d/e/f
[mkdir] depth=7 path=/a/b/c/d/e/f/g
[mkdir] depth=8 path=/a/b/c/d/e/f/g/h
[traverse] parent=/ child=a found=yes
[traverse] parent=/a child=b found=yes
[traverse] parent=/a/b child=c found=yes
[traverse] parent=/a/b/c child=d found=yes
[traverse] parent=/a/b/c/d child=e found=yes
[traverse] parent=/a/b/c/d/e child=f found=yes
[traverse] parent=/a/b/c/d/e/f child=g found=yes
[traverse] parent=/a/b/c/d/e/f/g child=h found=yes
[leaf-file] path=/a/b/c/d/e/f/g/h/leaf_payload.txt bytes=40 highest_level=direct readback_match=yes
[dir-stat] path=/a/b/c/d/e/f/g/h kind=directory child_count=1 dirent_bytes=32
[errors] existing_dir=17 missing_intermediate=2 path_too_long=36
[kernel-diag] dir_inode_count=9 dirent_bytes_per_inode=32 resolve_calls=119 path_components_split=100 max_resolve_depth=9 dirent_reads=109 dirent_writes=9
[summary] directories_created=8 directories_traversed=8 directories_removed=8 file_bytes=40
[acceptance] directory inode stores dirents as internal directory content: PASS
[acceptance] path resolver recursively walks slash-separated components: PASS
[acceptance] deep directory tree create/traverse/remove and leaf file ops succeed: PASS
```

从这组输出可以直接确认：

1. 支持超过二级的多级目录结构；
2. 目录 inode 的内容大小与单个 `Dirent` 大小一致；
3. 解析器成功逐级递归找到每一层子目录；
4. 创建、遍历、叶子文件操作和自底向上删除都成功。

### 7.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task4/artifacts/run_output_repeat.txt) 再次得到一致结果：

```text
[dir-stat] path=/a/b/c/d/e/f/g/h kind=directory child_count=1 dirent_bytes=32
[errors] existing_dir=17 missing_intermediate=2 path_too_long=36
[kernel-diag] dir_inode_count=9 dirent_bytes_per_inode=32 resolve_calls=119 path_components_split=100 max_resolve_depth=9 dirent_reads=109 dirent_writes=9
[summary] directories_created=8 directories_traversed=8 directories_removed=8 file_bytes=40
[acceptance] directory inode stores dirents as internal directory content: PASS
[acceptance] path resolver recursively walks slash-separated components: PASS
[acceptance] deep directory tree create/traverse/remove and leaf file ops succeed: PASS
```

第二次运行说明：

- 多级目录结构和解析深度稳定；
- 目录项读写计数稳定；
- 错误路径返回稳定。

### 7.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task4/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 8. 验收对照

- 文件系统支持将目录当作一种特殊的 Inode 进行读写（存入 Dirent）。
  结果：通过。
  证据：目录 inode 的 `size_bytes=32` 与 `dirent_bytes_per_inode=32` 一致，且 `Dirent[]` 是目录 inode 的内部内容。
- 路径名解析器能用 `/` 切割并递归查找下一级 Inode 块。
  结果：通过。
  证据：`max_resolve_depth=9`，`path_components_split=100`，并且 8 级深目录遍历全部成功。

## 9. 环境说明与限制

- 这是 QEMU guest 内核态实验，不是宿主机 Linux 文件系统测试。
- 当前目录模型使用 inode 内部固定大小 `Dirent[]`，没有实现目录项溢出到额外块页的完整格式。
- 路径解析、目录项管理和深目录树验收都已覆盖，但没有实现 `.`、`..`、相对路径和权限位语义。
