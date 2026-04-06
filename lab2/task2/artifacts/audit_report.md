# LAB2 User Task2 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/task2`（QEMU guest 用户态任务：系统调用统计与完成时间统计测试集）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/task2/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/task2/src/main.rs), [trap.rs](/root/os_experiments/lab2/task2/src/trap.rs), [syscall.rs](/root/os_experiments/lab2/task2/src/syscall.rs), and `src/apps/*.rs`
  - Evidence artifacts in [build_output.txt](/root/os_experiments/lab2/task2/artifacts/build_output.txt), [run_output.txt](/root/os_experiments/lab2/task2/artifacts/run_output.txt), [run_output_repeat.txt](/root/os_experiments/lab2/task2/artifacts/run_output_repeat.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（当前仅有 `?? .codex` 与本次审计文件）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| 提供不少于 3 个、特征明显不同的独立测试源码 | 至少 3 个独立 app 源码，行为类型可区分 | 已提供 4 个独立 app：`io_burst`、`compute_spin`、`info_flood`、`illegal_trap`（[main.rs](/root/os_experiments/lab2/task2/src/main.rs) L112-L142；`src/apps/*.rs`） | pass |
| 每个应用有预期 syscall/统计趋势，并能与观测值对照 | 启动前打印每个任务预期；结束后输出每任务统计行 | 启动预期输出见 [main.rs](/root/os_experiments/lab2/task2/src/main.rs) L223-L225，日志见 [run_output.txt](/root/os_experiments/lab2/task2/artifacts/run_output.txt) L3/L29/L31/L33；统计结果见 `result ... total/write/get_taskinfo/error/cycles`（L28/L30/L32/L35） | pass |
| 测试报告逻辑自洽，观测值印证行为特征 | README 测试表格 + 双次运行 + 偏差分析 | README 包含测试项、预期、观测、偏差分析；两次运行中 syscall 计数一致且趋势稳定（[run_output.txt](/root/os_experiments/lab2/task2/artifacts/run_output.txt) L28-L45；[run_output_repeat.txt](/root/os_experiments/lab2/task2/artifacts/run_output_repeat.txt) L28-L45） | pass |
| 统计机制正确覆盖 syscall、退出与 fault 路径 | `handle_syscall` 分类计数、错误计数、fault 处理与最终校验 | [main.rs](/root/os_experiments/lab2/task2/src/main.rs) L169-L194（syscall 入口）、L394-L409（计数）、L196-L212（fault 处理）、L308-L357（最终 PASS/FAIL 规则）；日志出现 `illegal_trap` fault 与全量检查 PASS | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- 缺少工具版本落盘 artifact，复现实验环境证据不完整。  
  当前 `artifacts/` 包含 `build_output.txt`、`run_output.txt`、`run_output_repeat.txt`，但缺少 `tool_versions.txt`。对 runtime-sensitive 的 QEMU 任务，建议补充 `rustc/cargo/qemu` 版本快照以降低环境漂移导致的复现争议。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设验收基准以 README 中给出的任务要求与检查项为准，未引入额外课程平台隐藏规则。
- 假设本任务不强制要求反汇编/符号级证据；当前验收重点是统计机制与跨任务行为趋势。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 周期统计是 QEMU guest `rdcycle`，绝对值会随宿主负载浮动；当前已用双次运行证明“趋势稳定”，但仍建议附上工具版本证据。

