# LAB2 Kernel Task2 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/kernel_task2`（内核态任务：系统调用编号与次数统计）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/kernel_task2/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs), [trap.rs](/root/os_experiments/lab2/kernel_task2/src/trap.rs), [syscall.rs](/root/os_experiments/lab2/kernel_task2/src/syscall.rs), and `src/apps/*.rs`
  - Evidence artifacts in [build_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/build_output.txt), [run_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output.txt), [run_output_repeat.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output_repeat.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（当前仅有 `?? .codex` 与本次审计文件）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| PCB/TCB 中新增 syscall 统计结构 | TCB 内含 total/histogram/error/unknown 等字段 | [main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs) L72-L77 定义 `SyscallStats`；L99-L112 定义 `TaskControlBlock` 并包含 `stats` | pass |
| 统计不受任务切换和内核初始化干扰 | 统计点仅在 syscall 入口；启动新任务时重置统计；fault 不污染 syscall 桶 | [main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs) L199-L227 仅在 `handle_syscall` 记录；L245-L255 启动任务时重置统计；`illegal_trap` 结果全 0（[run_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output.txt) L49-L56） | pass |
| 每个任务退出时输出 syscall 直方图 | 每任务 result + histogram 行可见 | [main.rs](/root/os_experiments/lab2/kernel_task2/src/main.rs) L293-L340 打印结果与直方图；日志见 [run_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output.txt) L29-L34/L36-L41/L43-L48/L51-L56 | pass |
| 统计趋势可复核并与 workload 行为一致 | `io_burst` 写调用高；`compute_spin` 仅 exit；`info_flood` get_taskinfo 高；`illegal_trap` 无 syscall | 两次运行均满足：`io_burst (24,0,1)`、`compute_spin (0,0,1)`、`info_flood (0,20,1)`、`illegal_trap (0,0,0)`（[run_output.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output.txt) L31-L34/L38-L41/L45-L48/L53-L56；[run_output_repeat.txt](/root/os_experiments/lab2/kernel_task2/artifacts/run_output_repeat.txt) L31-L34/L38-L41/L45-L48/L53-L56） | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- 缺少 `tool_versions.txt`，复现环境证据仍不完整。  
  当前已有 `build_output` 和双次运行日志，建议再落盘 `rustc/cargo/qemu` 版本快照，以减少环境差异导致的评审争议。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设本任务验收以 README 引述要求为准，不额外要求反汇编/符号级证据。
- 假设“任务退出时打印直方图”允许 fault 任务在 fault 路径打印统计（当前 `illegal_trap` 已在 fault 后打印直方图）。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 周期数是 QEMU guest `rdcycle`，绝对值会波动；当前已用双次运行证明直方图稳定，趋势结论可靠。

