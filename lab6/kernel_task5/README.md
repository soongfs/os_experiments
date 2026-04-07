# LAB6 内核态 Task5：日志机制与崩溃一致性

## 1. 原始任务说明

### 任务标题

日志机制与崩溃一致性

### 任务目标

理解崩溃一致性问题与日志（journaling）思想，保证断电/崩溃后的文件系统可恢复。

### 任务要求

1. 引入日志机制（记录元数据更新顺序与提交点）；
2. 提供崩溃注入或非正常退出的测试方法；
3. 验证：重启后文件系统结构一致，不出现明显损坏。

### 验收检查

1. 所有文件修改操作被封装在特定的事务（Transaction）中；
2. 事务块顺序落盘（日志先行），Commit 标志写入后才更新原数据块；
3. 强制 QEMU 退出的异常场景中，重启后文件系统自检可回放日志并修补。

## 2. Acceptance -> Evidence 清单

- 所有文件修改必须在事务中完成。
  证据：第一次运行打印 `[journal] tx=1 stage=begin`、`write-log`、`commit`，恢复启动打印 `committed_log_detected=yes`，且三条 `acceptance` 均为 `PASS`，见 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output.txt)。
- 事务必须先落日志，再写 commit，最后才允许安装 home block。
  证据：第一次运行的顺序固定为 `write-log-blocks -> write-commit -> crash-inject`，崩溃发生点明确是 `after-commit-before-home-install`，说明原数据块尚未被覆盖。
- 崩溃后重启必须回放日志并修补文件系统。
  证据：第二次运行先打印 `[recovery] ... action=replay`，随后执行 `replay-log` 和 `clear-log`，最终 `fsck` 通过，`data_match=yes`，`journal_active=0`、`journal_committed=0`。
- 结果必须可重复。
  证据：两次 crash/recover 流程的结果一致，见 [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output_repeat.txt)。

## 3. 实验环境与实现思路

本实验运行在 QEMU 的 RISC-V bare-metal guest 环境中，属于 `LAB6` 内核态任务，不是宿主机用户态程序。

为了在“崩溃后重启”之间真正保留状态，这次实现使用了一个最小 journaling teaching fs 模型，并通过 QEMU semihosting 将磁盘镜像持久化到宿主机路径 `artifacts/journal_disk.bin`。这样每次 QEMU 重启时，guest 都能重新读取同一份磁盘镜像，再决定是直接启动、丢弃未提交日志，还是回放已提交日志。

这个模型刻意保持最小化：

- 只有根目录 `/` 和一个文件 `/journaled.txt`；
- 日志区只覆盖一个事务头、一个 inode 副本和一个数据块副本；
- 重点验证日志先行、提交点和崩溃恢复，而不是构建完整块设备驱动或完整 easy-fs。

## 4. 文件列表与代码说明

- [Cargo.toml](/root/os_experiments/lab6/kernel_task5/Cargo.toml)：Rust 裸机工程配置，二进制名为 `lab6_kernel_task5`。
- [.cargo/config.toml](/root/os_experiments/lab6/kernel_task5/.cargo/config.toml)：RISC-V 目标和链接配置。
- [linker.ld](/root/os_experiments/lab6/kernel_task5/linker.ld)：内存布局和栈布局。
- [src/boot.S](/root/os_experiments/lab6/kernel_task5/src/boot.S)：启动和 trap 入口汇编。
- [src/console.rs](/root/os_experiments/lab6/kernel_task5/src/console.rs)：UART 输出。
- [src/trap.rs](/root/os_experiments/lab6/kernel_task5/src/trap.rs)：U-mode `ecall` trap 分发。
- [src/user_console.rs](/root/os_experiments/lab6/kernel_task5/src/user_console.rs)：用户态格式化输出。
- [src/abi.rs](/root/os_experiments/lab6/kernel_task5/src/abi.rs)：`FsStat`、`JournalDiagnostics`、错误码和 syscall 号。
- [src/hostio.rs](/root/os_experiments/lab6/kernel_task5/src/hostio.rs)：QEMU semihosting 文件访问封装，用于持久化模式文件和磁盘镜像。
- [src/journal.rs](/root/os_experiments/lab6/kernel_task5/src/journal.rs)：最小文件系统镜像、事务封装、日志写入、提交、崩溃恢复和 `fsck`。
- [src/syscall.rs](/root/os_experiments/lab6/kernel_task5/src/syscall.rs)：用户态 syscall 包装。
- [src/main.rs](/root/os_experiments/lab6/kernel_task5/src/main.rs)：内核入口、syscall 分发、崩溃/恢复验证逻辑。
- [artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task5/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output.txt)：第一次 crash/recover 验证日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output_repeat.txt)：第二次 crash/recover 验证日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab6/kernel_task5/artifacts/tool_versions.txt)：工具版本。

## 5. 关键机制说明

### 5.1 事务封装

[src/journal.rs](/root/os_experiments/lab6/kernel_task5/src/journal.rs) 中的 `transactional_write()` 将对 `/journaled.txt` 的修改包装成一个事务：

1. 生成新的 inode 和数据块副本；
2. 把 inode/data 写入日志区，并设置 `active=1, committed=0`；
3. 将日志区整体落盘；
4. 再把 `committed=1` 落盘；
5. 若不是崩溃注入模式，则安装到 home inode/home data；
6. 清空日志头和日志数据。

这正对应了 write-ahead logging 的基本要求：日志先行，提交点先于原地更新。

### 5.2 崩溃注入

宿主机通过 `artifacts/mode.txt` 指定启动模式：

- `reset_then_crash_after_commit`
- `recover_after_crash`

在 `reset_then_crash_after_commit` 模式下，事务执行到 `commit` 后立即打印：

```text
[journal] tx=1 stage=crash-inject reason=after-commit-before-home-install
```

随后通过 `qemu_exit(2)` 进入非正常结束路径。本仓库沿用前几个 LAB6 内核任务的退出模型，QEMU 最终由 `timeout` 收束，因此日志里能看到：

```text
qemu-system-riscv64: terminating on signal 15 from pid 4 (timeout)
```

这次 `timeout` 不是测试失败，而是有意保留“提交后未完成 home install”的崩溃现场。

### 5.3 重启恢复

重启后 `init()` 会先读取持久化磁盘镜像，再执行 `recover_if_needed()`：

- 若 `active=1 && committed=1`，说明存在已提交但未安装的事务，必须 `replay-log`；
- 若 `active=1 && committed=0`，说明只写了一半日志，直接丢弃；
- 恢复完成后必须 `clear-log`，再执行 `fsck()`。

这次实验的恢复路径在日志中表现为：

```text
[recovery] tx=1 committed_log_detected=yes action=replay
[host-disk] stage=replay-log ...
[journal] tx=1 stage=replay-log home_checksum=0x004ac6e54e982173
[host-disk] stage=clear-log ...
[journal] tx=1 stage=clear-log active=0 committed=0
[fsck] root_dirent_ok=yes home_inode_ok=yes home_checksum_ok=yes journal_clear=yes
```

说明重启后确实完成了 committed journal 的回放和清理。

## 6. 构建、运行与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab6/kernel_task5
```

构建：

```bash
cargo build
```

记录构建日志：

```bash
cargo build > artifacts/build_output.txt 2>&1
```

第一次 crash/recover 验证：

```bash
rm -f artifacts/journal_disk.bin
printf 'reset_then_crash_after_commit\n' > artifacts/mode.txt
timeout 10s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -semihosting-config enable=on,target=native \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task5 \
  > /tmp/kernel_task5_run1_crash.txt 2>&1

printf 'recover_after_crash\n' > artifacts/mode.txt
timeout 10s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -semihosting-config enable=on,target=native \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task5 \
  > /tmp/kernel_task5_run1_recover.txt 2>&1

cat /tmp/kernel_task5_run1_crash.txt /tmp/kernel_task5_run1_recover.txt > artifacts/run_output.txt
```

第二次 crash/recover 验证：

```bash
rm -f artifacts/journal_disk.bin
printf 'reset_then_crash_after_commit\n' > artifacts/mode.txt
timeout 10s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -semihosting-config enable=on,target=native \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task5 \
  > /tmp/kernel_task5_run2_crash.txt 2>&1

printf 'recover_after_crash\n' > artifacts/mode.txt
timeout 10s qemu-system-riscv64 -machine virt -bios none -nographic -m 64M \
  -semihosting-config enable=on,target=native \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab6_kernel_task5 \
  > /tmp/kernel_task5_run2_recover.txt 2>&1

cat /tmp/kernel_task5_run2_crash.txt /tmp/kernel_task5_run2_recover.txt > artifacts/run_output_repeat.txt
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

[artifacts/build_output.txt](/root/os_experiments/lab6/kernel_task5/artifacts/build_output.txt)：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s
```

### 7.2 第一次 crash/recover 结果

[artifacts/run_output.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output.txt) 的关键输出：

```text
[journal] tx=1 stage=begin target=/journaled.txt
[host-disk] stage=write-log-blocks ... bytes=368
[journal] tx=1 stage=write-log data_checksum=0x004ac6e54e982173
[host-disk] stage=write-commit ... bytes=368
[journal] tx=1 stage=commit committed=1
[journal] tx=1 stage=crash-inject reason=after-commit-before-home-install
[recovery] tx=1 committed_log_detected=yes action=replay
[journal] tx=1 stage=replay-log home_checksum=0x004ac6e54e982173
[journal] tx=1 stage=clear-log active=0 committed=0
[fsck] root_dirent_ok=yes home_inode_ok=yes home_checksum_ok=yes journal_clear=yes
[verify] bytes=27 data_match=yes checksum=0x004ac6e54e982173 journal_active=0 journal_committed=0
[diag] ... home_writes=1 recovery_replays=1 committed_logs_seen=1 last_tx_seq=1 ...
[acceptance] all file modifications are wrapped in transactions: PASS
[acceptance] journal blocks commit before home data installation: PASS
[acceptance] reboot recovery replays committed logs and repairs filesystem state: PASS
```

从这组输出可以直接确认：

1. 文件修改确实进入了事务 `tx=1`；
2. 磁盘更新顺序是 `write-log-blocks -> write-commit -> crash-inject`；
3. 崩溃后重启看到了 committed journal，并完成 replay；
4. 恢复完成后日志区被清空，文件内容与 checksum 都一致。

### 7.3 第二次 crash/recover 结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab6/kernel_task5/artifacts/run_output_repeat.txt) 与第一次结果一致，关键数值仍然是：

- `bytes=27`
- `data_match=yes`
- `home_writes=1`
- `recovery_replays=1`
- `committed_logs_seen=1`
- 三条 `acceptance` 全部 `PASS`

说明实验结果稳定可复现。

## 8. 验收结论

- 验收 1：所有文件修改操作被封装在特定事务中。
  结果：`PASS`。`transactional_write()` 明确输出 `tx=1 stage=begin`，并把 inode/data 更新统一放入 journal transaction。
- 验收 2：事务块顺序落盘，Commit 标志写入后才更新原数据块。
  结果：`PASS`。日志显示先 `write-log-blocks`，再 `write-commit`，崩溃点位于 `after-commit-before-home-install`。
- 验收 3：强制 QEMU 退出后，重启自检可回放日志并修补。
  结果：`PASS`。恢复启动检测到 `committed_log_detected=yes`，完成 `replay-log`、`clear-log` 和 `fsck`，最终 `data_match=yes`。

## 9. 环境说明与限制

- 本实验运行于 QEMU RISC-V bare-metal guest，中间状态通过 semihosting 持久化到宿主机文件，以便模拟“崩溃后重启读取同一磁盘镜像”。
- 这是一个最小 journaling 模型，不包含完整块缓存、并发事务、真实块设备驱动、目录层级扩展和多文件恢复逻辑。
- 当前仍沿用仓库已有 LAB6 内核任务的 `qemu_exit()` 方式，QEMU 没有立即自然退出，因此复现实验使用 `timeout` 收束；这不影响 crash/recovery 验证本身，因为本实验本来就需要保留异常退出现场。
