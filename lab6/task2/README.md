# LAB6 用户态 Task2：深目录树生成与验证

## 1. 原始任务说明

### 任务标题

深目录树生成与验证

### 任务目标

验证目录项组织与路径解析逻辑，发现边界条件错误。

### 任务要求

1. 自动创建 N 级深目录结构；
2. 进行访问验证（创建文件/遍历/删除）；
3. 给出异常处理：路径过长、目录已存在等情况需有合理输出。

### 验收检查

1. 深层级（如 `/a/b/c/d/e`）目录能够被正确创建；
2. 能够在叶子目录正常进行文件操作；
3. 不存在的中间目录会抛出相应错误而非引发内核异常。

## 2. Acceptance -> Evidence 清单

- 深目录树是在 QEMU guest U-mode 里触发的，而不是宿主机 shell 脚本。
  证据：QEMU 日志显示 `[kernel] LAB6 task2 guest deep-directory validation` 后进入 `[config] ...` 和一系列 guest 内 syscall 输出，见 [artifacts/run_output.txt](/root/os_experiments/lab6/task2/artifacts/run_output.txt)。
- 已成功自动创建深层目录 `/a/b/c/d/e/...`。
  证据：日志中有 `[mkdir] depth=1..8`，实际路径扩展到了 `/a/b/c/d/e/f/g/h`。
- 已完成目录遍历、叶子文件创建、读回和删除。
  证据：`[traverse]` 行逐级发现子目录；`[leaf-file] ... readback_match=yes`；随后 `[cleanup-file]` 和 `[cleanup-dir]` 行自底向上清理成功。
- 目录已存在、路径过长、缺失中间目录三类异常都给出合理错误。
  证据：
  - `errno=17 message=already exists`
  - `errno=36 message=path too long`
  - `errno=2 message=no such file or directory`

## 3. 实验目标与实现思路

本实验在 [lab6/task2](/root/os_experiments/lab6/task2) 中实现为真正的 QEMU/RISC-V 裸机任务。内核同样提供一个最小内存文件系统，U-mode 用户程序通过 syscall 在 guest 内触发目录和路径相关操作。

这里继续使用最小内存文件系统，而不是接真实磁盘，原因和 `task1` 一致：本题要验证的是“目录项组织 + 路径解析 + errno 边界条件”，不是块设备持久化。为此，最有效的设计是：

1. 直接在 guest 根目录下顺序创建 `/a/b/c/d/e/f/g/h`；
2. 再从 `/` 开始逐级 `list_dir()`，确认每一级目录项都能找到下一层子目录；
3. 在叶子目录创建 `leaf_payload.txt`，写入、读回、校验；
4. 主动制造三类错误路径：
   - 重新创建已存在目录，验证 `EEXIST`
   - 在缺失中间目录下创建文件，验证 `ENOENT`
   - 构造超过 `PATH_MAX` 的路径，验证 `ENAMETOOLONG`

和 `task1` 一样，本任务也明确省略：

- 真实块设备持久化；
- 删除文件后的数据块回收。

这些省略不影响本题的核心验收，因为目录树结构和路径错误处理完全可以在这层最小模型里得到稳定、可审查的证据。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/task2/Cargo.toml)：Rust 裸机工程配置。
- [.cargo/config.toml](/root/os_experiments/lab6/task2/.cargo/config.toml)：固定目标三元组和链接脚本。
- [linker.ld](/root/os_experiments/lab6/task2/linker.ld)：镜像布局。
- [src/boot.S](/root/os_experiments/lab6/task2/src/boot.S)：M-mode 启动和 trap 入口。
- [src/console.rs](/root/os_experiments/lab6/task2/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/task2/src/trap.rs)：syscall trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/task2/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/task2/src/abi.rs)：syscall 号、错误码、`FsStat` 和文件系统常量。
- [src/syscall.rs](/root/os_experiments/lab6/task2/src/syscall.rs)：U-mode syscall 封装。
- [src/fs.rs](/root/os_experiments/lab6/task2/src/fs.rs)：目录项、路径解析和最小 inode/file 实现。
- [src/main.rs](/root/os_experiments/lab6/task2/src/main.rs)：guest 内核入口和 U-mode 深目录测试程序。
- [artifacts/build_output.txt](/root/os_experiments/lab6/task2/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/task2/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task2/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/task2/artifacts/tool_versions.txt)：Rust/QEMU 版本信息。

## 5. 测量边界与复现步骤

### 5.1 工作负载参数

- 目录深度：`8`
- 目录名序列：`a/b/c/d/e/f/g/h`
- 叶子文件名：`leaf_payload.txt`
- `PATH_MAX = 256`
- 叶子文件内容长度：`39` 字节

### 5.2 计时边界

guest 用户程序记录三段时间：

- `create_us`
  - 从开始逐级 `mkdir` 到最后一级目录创建完成；
- `verify_us`
  - 从异常目录测试和逐级 `list_dir` 开始，到叶子文件写读校验和所有错误码验证完成；
- `cleanup_us`
  - 从删除叶子文件开始，到删除最外层 `/a` 完成。

这些时间只用于证明 guest 内操作完整执行，不作为性能 benchmark。

### 5.3 构建与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/task2
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_task2 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_task2 \
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

[artifacts/build_output.txt](/root/os_experiments/lab6/task2/artifacts/build_output.txt) 的实际内容：

```text
Compiling lab6_task2 v0.1.0 (/root/os_experiments/lab6/task2)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```

### 6.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/task2/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 task2 guest deep-directory validation
[config] depth=8 example_prefix=/a/b/c/d/e path_max=256
[mkdir] depth=1 path=/a
[mkdir] depth=2 path=/a/b
[mkdir] depth=3 path=/a/b/c
[mkdir] depth=4 path=/a/b/c/d
[mkdir] depth=5 path=/a/b/c/d/e
[mkdir] depth=6 path=/a/b/c/d/e/f
[mkdir] depth=7 path=/a/b/c/d/e/f/g
[mkdir] depth=8 path=/a/b/c/d/e/f/g/h
[existing-dir] path=/a errno=17 message=already exists
[traverse] parent=/a/b/c/d/e/f/g child=h found=yes
[leaf-file] path=/a/b/c/d/e/f/g/h/leaf_payload.txt bytes=39 highest_level=direct readback_match=yes
[missing-intermediate] path=/ghost/branch/leaf.txt errno=2 message=no such file or directory
[path-too-long] limit=255 attempted_length=275 errno=36 message=path too long
[summary] directories_created=8 directories_traversed=8 directories_removed=8 file_bytes=39
[timing] create_us=2713 verify_us=4630 cleanup_us=1450
[acceptance] deep directory hierarchy created successfully: PASS
[acceptance] leaf directory file operations succeeded: PASS
[acceptance] missing intermediate directory returned ENOENT: PASS
[acceptance] existing directory returned EEXIST: PASS
[acceptance] path too long returned ENAMETOOLONG: PASS
```

从这组输出可以直接读出：

1. `/a/b/c/d/e` 及更深层路径都在 guest 内创建成功；
2. 用户程序不是只靠“路径存在判断”蒙混，而是逐级 `list_dir()` 做了目录遍历；
3. 叶子文件创建、写入、读回都成功；
4. 缺失中间目录返回 `ENOENT`，没有触发内核异常退出。

### 6.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/task2/artifacts/run_output_repeat.txt) 再次得到同型结果：

```text
[existing-dir] path=/a errno=17 message=already exists
[leaf-file] path=/a/b/c/d/e/f/g/h/leaf_payload.txt bytes=39 highest_level=direct readback_match=yes
[missing-intermediate] path=/ghost/branch/leaf.txt errno=2 message=no such file or directory
[path-too-long] limit=255 attempted_length=275 errno=36 message=path too long
[summary] directories_created=8 directories_traversed=8 directories_removed=8 file_bytes=39
[timing] create_us=2796 verify_us=3811 cleanup_us=1523
[acceptance] deep directory hierarchy created successfully: PASS
[acceptance] leaf directory file operations succeeded: PASS
[acceptance] missing intermediate directory returned ENOENT: PASS
[acceptance] existing directory returned EEXIST: PASS
[acceptance] path too long returned ENAMETOOLONG: PASS
```

第二次运行说明：

- 目录层级、文件大小和 errno 全部保持稳定；
- 两次运行都成功把目录树完全删回空根目录；
- 抖动只体现在微秒级时间统计，不影响验收结论。

### 6.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/task2/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 7. 机制解释

### 7.1 为什么这个测试能覆盖目录项组织与路径解析

单纯 `mkdir("/a/b/c")` 成功并不足以证明目录项组织正确，因为其中可能隐藏这些问题：

- 目录项被创建了，但父目录中不可枚举；
- 叶子目录可以创建，但路径解析在中间层级上有漏洞；
- 删除时元数据回收顺序不对，只是正向路径刚好通过。

因此本实验故意做成闭环：

1. 顺序创建 `/a/b/c/d/e/f/g/h`；
2. 再从 `/` 开始逐级 `list_dir()`，确认每一级目录都能枚举出下一层；
3. 在最深目录放入普通文件，验证路径解析到叶子后仍然正确；
4. 最后从叶子开始逆序删除，验证空目录删除和父子关系更新都没问题。

这比只做一次“深路径 mkdir”更接近真实的目录项组织验证。

### 7.2 为什么缺失中间目录必须返回 `ENOENT`

对 `/ghost/branch/leaf.txt` 来说，路径解析在处理到 `ghost` 时就应该终止，因为根目录里根本没有这个目录项。正确做法是立即返回 `ENOENT`。

如果内核没有在这一层返回错误，而是继续沿着空指针、脏 inode 或未初始化目录项往下走，就会把普通路径错误升级成 trap/panic。这个实验要验证的正是：缺失中间目录必须是一条“可恢复的普通错误路径”，而不是内核异常。

### 7.3 为什么还要测试 `EEXIST` 和 `ENAMETOOLONG`

- `EEXIST`
  - 用来验证目录创建逻辑不会把重复创建静默吞掉；
- `ENAMETOOLONG`
  - 用来验证路径长度边界在解析前就被拦住，而不是让超长路径把解析器带到越界访问。

这两类错误和 `ENOENT` 一起，构成了目录/路径逻辑最常见的边界条件集合。

## 8. 验收检查映射

- [x] 深层级（如 `/a/b/c/d/e`）目录能够被正确创建。
  证据：实际创建到了 `/a/b/c/d/e/f/g/h`，`depth=1..8` 全部成功。
- [x] 能够在叶子目录正常进行文件操作。
  证据：`[leaf-file] ... bytes=39 highest_level=direct readback_match=yes`。
- [x] 不存在的中间目录会抛出相应错误而非引发内核异常。
  证据：`[missing-intermediate] ... errno=2 message=no such file or directory`，随后程序继续完成 cleanup 和全部 `PASS`。

## 9. 环境说明、限制与未决事项

- 本次结果来自 QEMU guest，不是宿主机 WSL 文件系统。
- 文件系统仍然是教学用最小内存模型，不接真实块设备，也不验证重启后的持久化。
- 目录删除验证的是“空目录删除”和父子关系维护，不包含 free inode/data block 的复杂回收策略。
- 若后续 `LAB6` 需要接入真实磁盘镜像，可在当前 `fs.rs` 的目录树和路径解析逻辑外继续补 block cache、journal 或持久化层。
