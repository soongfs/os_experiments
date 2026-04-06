# LAB2 Kernel Task3 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/kernel_task3`（内核态任务：完成时间统计）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/kernel_task3/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs), [trap.rs](/root/os_experiments/lab2/kernel_task3/src/trap.rs), [syscall.rs](/root/os_experiments/lab2/kernel_task3/src/syscall.rs), and `src/apps/*.rs`
  - Evidence artifacts in [build_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/build_output.txt), [run_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output.txt), [run_output_repeat.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output_repeat.txt), [qemu_timebase_probe.txt](/root/os_experiments/lab2/kernel_task3/artifacts/qemu_timebase_probe.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（当前仅有 `?? .codex` 与本次审计文件）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| 使用 `mtime` 等硬件寄存器计时，单位换算正确 | `mtime` 地址、timebase 频率、tick→us/ms 换算链路可见 | [main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs) L24-L29 定义 `MTIME_ADDR`/`MTIME_FREQ_HZ`；L619-L621 读取 `mtime`；L615-L617 做 ticks→us 换算；日志输出 ticks/us/ms（[run_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output.txt) L31/L57/L113 等） | pass |
| 时间源来源有可复核证据 | QEMU/DTB 中存在 `timebase-frequency` 与 CLINT 节点信息 | [qemu_timebase_probe.txt](/root/os_experiments/lab2/kernel_task3/artifacts/qemu_timebase_probe.txt) 显示 `timebase-frequency`、`clint@2000000` 与 DTB 十六进制证据 `0x00989680`（10,000,000） | pass |
| 计时区间合理，排除应用加载前置开销 | 起点在启动日志后、`mret` 前；终点在首次 trap 回内核后立即采样 | [main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs) L310-L317 在 launch 完成后采样 `start_mtime`；L335-L337 在完成路径先采样 `end_mtime` 再记账；README 机制说明与代码一致 | pass |
| 同一应用多次运行可对比且在合理波动范围内 | 至少重复运行证据 + 每应用多轮统计 | 每应用 `warm-up + 3 measured`（[main.rs](/root/os_experiments/lab2/kernel_task3/src/main.rs) L20-L23, L209-L222）；两份运行日志均有 summary 与 PASS 检查（[run_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output.txt) L126-L133；[run_output_repeat.txt](/root/os_experiments/lab2/kernel_task3/artifacts/run_output_repeat.txt)） | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- `build_output.txt` 与当前复验构建时间存在口径差异，建议同步更新 artifact 或将 README 表述改为“时间仅示例”。  
  当前 [build_output.txt](/root/os_experiments/lab2/kernel_task3/artifacts/build_output.txt) 记录 `0.00s`，我本次复验为 `0.02s`。这不影响功能正确性，但会削弱“README/artifact 与当前状态严格对应”的审查可信度。
- 缺少 `tool_versions.txt` 作为任务级环境快照。  
  已有 `qemu_timebase_probe`，但建议仍补 `rustc/cargo/qemu` 版本文件，便于跨环境复验时快速对齐工具链。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设验收基准以 README 中列出的任务要求为准，未引入课程平台额外隐藏检查项。
- 假设“合理波动范围”以趋势可解释为主，而非要求固定上限阈值；当前两份日志都保持 `compute_spin` 平均最慢、检查项全 PASS。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 计时结果受宿主调度与 QEMU 状态影响，单次运行可能出现离散度升高；当前设计通过 warm-up 与多轮 measured 统计缓解了该风险。

