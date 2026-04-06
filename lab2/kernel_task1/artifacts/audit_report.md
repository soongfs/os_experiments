# LAB2 Kernel Task1 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/kernel_task1`（内核态任务：实现 `get_taskinfo` 系统调用）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/kernel_task1/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/kernel_task1/src/main.rs), [trap.rs](/root/os_experiments/lab2/kernel_task1/src/trap.rs), [syscall.rs](/root/os_experiments/lab2/kernel_task1/src/syscall.rs), [boot.S](/root/os_experiments/lab2/kernel_task1/src/boot.S)
  - Evidence artifacts in [build_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/build_output.txt), [run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（当前仅有 `?? .codex` 与本次审计文件）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| 扩展 syscall 表并正确分发 `get_taskinfo` | syscall 号到 handler 的映射与 trap 分发路径可见 | [main.rs](/root/os_experiments/lab2/kernel_task1/src/main.rs) L80-L84 定义 syscall 表；L123-L136 按 `a7` 分发；[trap.rs](/root/os_experiments/lab2/kernel_task1/src/trap.rs) L59-L63 在 `mcause==8` 时进入分发；日志见 [run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt) L5/L8/L11/L14 | pass |
| 将当前 task 信息拷贝到用户指针 | `TaskInfo` 回填与用户态读取一致 | [main.rs](/root/os_experiments/lab2/kernel_task1/src/main.rs) L195-L207 组装并回填 `TaskInfo`；日志见 [run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt) L6-L7（内核回填与用户读取一致） | pass |
| 非法参数/非法指针被边界检查拦截并返回明确错误码 | NULL、未对齐、越界三类检查 | [main.rs](/root/os_experiments/lab2/kernel_task1/src/main.rs) L278-L298 做输出地址校验（NULL/对齐/范围）；L159-L179 用户态触发三类坏指针；日志见 [run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt) L8-L16 返回 `-14/-22/-14` | pass |
| 错误路径可控，不导致内核崩溃 | 错误后系统继续运行并正常结束 | [run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt) 显示三次错误后仍能继续并正常 shutdown（L17） | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- 缺少 `run_output_repeat.txt`，对运行时行为的稳定性证据偏弱。  
  当前仅有单次运行日志 [run_output.txt](/root/os_experiments/lab2/kernel_task1/artifacts/run_output.txt)。对于 syscall 参数校验类任务，建议至少补一次重复运行输出，证明错误路径与返回码稳定。
- 缺少 `tool_versions.txt`，复现实验环境证据不完整。  
  README 写了工具版本，但未作为 artifact 落盘；建议补充 `rustc/cargo/qemu` 版本快照文件，降低评审时的环境漂移争议。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设验收基准以 README 中给出的任务要求/检查项为准，未引入额外平台隐藏检查项。
- 假设本任务不强制要求额外反汇编证据；当前重点是 syscall 分发、参数校验与安全拷贝路径。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 当前证据主要来自单次运行；虽然我本地复验结果一致，但仍建议补齐重复运行与工具版本 artifact，增强提交稳健性。

