# LAB6 内核态 Task1：三重间接 inode 支持

## 1. 原始任务说明

### 任务标题

三重间接 inode 支持

### 任务目标

扩展文件寻址结构，突破单文件大小上限。

### 任务要求

1. 扩展 easy-fs，支持三重间接 inode；
2. 通过用户态大文件写入测试验收；
3. 给出边界测试：接近上限时仍能正确读写。

### 验收检查

1. 磁盘 Inode 结构体扩容增加了三重间接索引（Triple Indirect Block）；
2. 块分配和读写函数递归逻辑完整支持三级寻址解析；
3. 用户态大文件压测满分通过。

## 2. Acceptance -> Evidence 清单

- `DiskInode` 结构必须显式包含三重间接索引槽位。
  证据：QEMU 日志打印 `disk inode layout: size_bytes=168 ... double_offset=160 triple_offset=164`，说明结构体里存在独立的 `triple_indirect` 字段，见 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output.txt)。
- 块映射逻辑必须以递归方式支持单、双、三重间接解析。
  证据：运行时诊断打印 `max_recursion_depth=3`、`triple_resolution_calls=32496`，说明三级递归路径被真实走到。
- 用户态大文件压测必须越过双间接上限并读回无误。
  证据：`target_bytes=16777216`，`double_limit_bytes=8459264`，`highest_level=triple`，`mismatches=0`。
- 接近理论上限时仍能正确读写。
  证据：边界文件 `size_bytes=1082201088` 恰好等于 `triple_limit_bytes`，只写入 3 个稀疏块仍能正确读回，同时再向 `FS_TRIPLE_LIMIT_BYTES` 偏移写 1 字节返回 `efbig_result=-27`。
- 结果必须稳定。
  证据：两次 QEMU 运行 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output_repeat.txt) 的核心数值一致，所有验收行均为 `PASS`。

## 3. 实验环境与实现思路

本实验运行在 QEMU 的 RISC-V bare-metal guest 环境中，属于 `LAB6` 内核态任务。内核在 M-mode 启动后切换到 U-mode 用户程序，由用户程序通过 syscall 驱动文件系统写入和读回；三重间接 inode 的实现本身位于 guest 内核中。

仓库当前没有独立的真实 `easy-fs` 代码库，因此这里采用“easy-fs 风格的最小可验证模型”：

- 定义 `DiskInode`，包含 `direct[10]`、`single_indirect`、`double_indirect`、`triple_indirect`；
- 使用独立的数据块池和指针块池模拟块设备分配；
- 用递归函数 `resolve_indirect_chain()` 统一处理 1/2/3 级间接映射；
- 通过 U-mode 用户程序完成两类验收：
  - `16 MiB` 稠密大文件压测，确保三重间接被实际使用；
  - 接近理论上限的稀疏边界写入，验证最后一个可写块仍能正确读写，同时越界写入返回 `EFBIG`。

这个模型故意省略真实磁盘持久化、日志、回收 free list、权限系统等完整 easy-fs 特性，目的是把验证聚焦在本题的关键机制：`DiskInode` 结构扩容和三级寻址逻辑。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/kernel_task1/Cargo.toml)：Rust 裸机工程配置，二进制名为 `lab6_kernel_task1`。
- [.cargo/config.toml](/root/os_experiments/lab6/kernel_task1/.cargo/config.toml)：RISC-V 目标与链接配置。
- [linker.ld](/root/os_experiments/lab6/kernel_task1/linker.ld)：内存布局与栈布局。
- [src/boot.S](/root/os_experiments/lab6/kernel_task1/src/boot.S)：启动和 trap 入口汇编。
- [src/console.rs](/root/os_experiments/lab6/kernel_task1/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/kernel_task1/src/trap.rs)：U-mode `ecall` trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/kernel_task1/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/kernel_task1/src/abi.rs)：syscall 号、`FsStat`、`FsDiagnostics`、块大小和 direct/single/double/triple 容量常量。
- [src/syscall.rs](/root/os_experiments/lab6/kernel_task1/src/syscall.rs)：用户态 syscall 封装，包含 `fs_diag()`。
- [src/fs.rs](/root/os_experiments/lab6/kernel_task1/src/fs.rs)：内核文件系统核心，实现 `DiskInode`、递归块解析和诊断统计。
- [src/main.rs](/root/os_experiments/lab6/kernel_task1/src/main.rs)：内核入口、syscall 处理、U-mode 压测和边界测试。
- [artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task1/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output.txt)：第一次 QEMU 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output_repeat.txt)：第二次 QEMU 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task1/artifacts/tool_versions.txt)：工具版本。

## 5. 关键机制说明

### 5.1 `DiskInode` 如何扩容

[src/fs.rs](/root/os_experiments/lab6/kernel_task1/src/fs.rs) 里的 `DiskInode` 使用 `#[repr(C)]` 固定布局，核心字段为：

- `direct: [u32; 10]`
- `single_indirect: u32`
- `double_indirect: u32`
- `triple_indirect: u32`

运行时通过 `offset_of!` 和 `size_of::<DiskInode>()` 导出布局诊断，得到：

- `size_bytes=168`
- `direct_offset=116`
- `single_offset=156`
- `double_offset=160`
- `triple_offset=164`

这说明结构体尾部已经明确扩容到三重间接索引。

### 5.2 三级寻址为何是递归实现

本实验没有为 single、double、triple 写三份彼此复制的展开逻辑，而是统一走：

- `resolve_data_block()`：根据逻辑块号判断 direct/single/double/triple 层级；
- `resolve_indirect_chain()`：递归下降指针块树；
- `ensure_ptr_block()`：按需分配新的间接块；
- `resolve_data_slot()`：到叶子数据块槽位时进行最终映射。

其中 `resolve_indirect_chain()` 的 `depth_remaining` 分别取：

- `1` 对应 single indirect
- `2` 对应 double indirect
- `3` 对应 triple indirect

运行期诊断 `max_recursion_depth=3` 和 `triple_resolution_calls=32496` 证明三级递归不是死代码，而是被实际压测和边界测试走到了。

### 5.3 理论容量与边界偏移

本任务的教学模型参数为：

- `BSIZE = 512`
- `NDIRECT = 10`
- 每个间接块可容纳 `512 / 4 = 128` 个块号

因此：

- 单间接上限：`(10 + 128) * 512 = 70656` 字节
- 双间接上限：`(10 + 128 + 128^2) * 512 = 8459264` 字节
- 三重间接理论最大容量：`(10 + 128 + 128^2 + 128^3) * 512 = 1082201088` 字节

边界测试选取三个关键偏移：

- 最后一个双间接块：`8458752`
- 第一个三重间接块：`8459264`
- 最后一个可写块：`1082200576`

最后一个偏移再写满 `512` 字节后，文件大小正好成为 `1082201088` 字节，也就是理论最大文件容量。

### 5.4 为什么边界测试用稀疏文件

理论上限约 `1.008 GiB`，若按稠密写法在当前 QEMU 内存里完整填满，实验成本会显著增加，而且验证重点会从寻址机制转移到内存容量。

因此边界测试采用稀疏写入：

- 只在三处关键逻辑块写入数据；
- 中间未分配的逻辑块按 hole 读为零；
- 文件 `size_bytes` 直接推进到理论上限；
- 再对最后一个合法块做读回校验，并对越界偏移进行 `EFBIG` 检查。

这足以证明“接近上限时仍能正确读写”，同时避免把实验变成纯容量堆砌。

## 6. 构建、运行与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/kernel_task1
```

构建：

```bash
cargo build
```

第一次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 128M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task1 \
  > artifacts/run_output.txt 2>&1
```

第二次运行并保存日志：

```bash
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -m 128M \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task1 \
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

[artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task1/artifacts/build_output.txt) 的实际内容：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.01s
```

### 7.2 第一次 QEMU 运行结果

[artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output.txt) 关键输出：

```text
[kernel] LAB6 kernel task1 triple-indirect inode support
[kernel] fs model: block_size=512 direct=10 pointers_per_indirect=128 triple_limit_bytes=1082201088
[kernel] disk inode layout: size_bytes=168 direct_offset=116 single_offset=156 double_offset=160 triple_offset=164
[dense-config] path=/dense_triple.bin target_bytes=16777216 chunk_bytes=4096 over_double_bytes=8317952 over_double_ratio=1.983x
[dense-write] bytes=16777216 duration_us=1862523 kib_per_s=8796 checksum=0x1907102902dcaba6
[dense-stat] size_bytes=16777216 blocks_used=32768 highest_level=triple
[dense-readback] bytes=16777216 duration_us=3934541 kib_per_s=4164 checksum=0x1907102902dcaba6 mismatches=0
[edge-config] path=/edge_limit_sparse.bin last_double_offset=8458752 first_triple_offset=8459264 last_triple_offset=1082200576
[edge-stat] size_bytes=1082201088 blocks_used=3 highest_level=triple efbig_result=-27
[kernel-diag] recursive_calls=163284 max_recursion_depth=3 triple_resolution_calls=32496 pointer_blocks_used=266 data_blocks_used=32771
[acceptance] disk inode layout includes triple indirect slot: PASS
[acceptance] recursive block resolution reaches triple-indirect depth: PASS
[acceptance] user-space large-file stress and boundary tests passed: PASS
```

从这组输出可以直接确认：

1. `DiskInode` 已经扩容出独立的 `triple_indirect` 字段；
2. `16 MiB` 稠密文件明显越过双间接上限，并且 `highest_level=triple`；
3. 全量读回 `mismatches=0`；
4. 稀疏边界文件的 `size_bytes` 精确达到理论最大容量 `1082201088`；
5. 再往后写 1 字节返回 `EFBIG(-27)`。

### 7.3 第二次 QEMU 运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task1/artifacts/run_output_repeat.txt) 再次得到一致结果：

```text
[dense-write] bytes=16777216 duration_us=1847187 kib_per_s=8869 checksum=0x1907102902dcaba6
[dense-stat] size_bytes=16777216 blocks_used=32768 highest_level=triple
[dense-readback] bytes=16777216 duration_us=3874953 kib_per_s=4228 checksum=0x1907102902dcaba6 mismatches=0
[edge-stat] size_bytes=1082201088 blocks_used=3 highest_level=triple efbig_result=-27
[kernel-diag] recursive_calls=163284 max_recursion_depth=3 triple_resolution_calls=32496 pointer_blocks_used=266 data_blocks_used=32771
[acceptance] disk inode layout includes triple indirect slot: PASS
[acceptance] recursive block resolution reaches triple-indirect depth: PASS
[acceptance] user-space large-file stress and boundary tests passed: PASS
```

两次运行表明：

- 三重间接路径的深度统计稳定；
- 大文件读回 checksum 稳定一致；
- 边界测试的最大文件大小与 `EFBIG` 行为稳定一致。

### 7.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task1/artifacts/tool_versions.txt) 的实际内容：

```text
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
riscv64gc-unknown-linux-gnu
riscv64gc-unknown-linux-musl
riscv64gc-unknown-none-elf (installed)
QEMU emulator version 10.0.8 (Debian 1:10.0.8+ds-0+deb13u1+b1)
```

## 8. 验收对照

- 磁盘 Inode 结构体扩容增加了 Triple Indirect Block。
  结果：通过。
  证据：`triple_offset=164`，且 [src/fs.rs](/root/os_experiments/lab6/kernel_task1/src/fs.rs) 的 `DiskInode` 明确包含 `triple_indirect: u32`。
- 块分配和读写函数递归逻辑完整支持三级寻址解析。
  结果：通过。
  证据：`resolve_indirect_chain()` 统一处理 1/2/3 级间接块；运行时 `max_recursion_depth=3`。
- 用户态大文件压测满分通过。
  结果：通过。
  证据：`16 MiB` 文件越过双间接上限，`highest_level=triple`，`mismatches=0`，边界文件也达到理论最大容量并通过读回与越界检查。

## 9. 环境说明与限制

- 这是 QEMU guest 内核态实验，不是宿主机 Linux 文件系统测试。
- 本实现是 easy-fs 风格的最小验证模型，不包含真实磁盘持久化、回收空闲块、权限系统和日志。
- 删除文件不会回收已分配的数据块和指针块，这是为了让验证重点集中在三重间接映射本身。
- 边界测试使用稀疏文件而不是稠密写满 `1.008 GiB`，因为本题要验证的是寻址上限和边界行为，而不是堆满 guest 内存。
