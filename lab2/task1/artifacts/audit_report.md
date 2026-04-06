# LAB2 User Task1 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/task1`（QEMU guest 用户态任务：访问 `get_taskinfo` 并打印结果）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/task1/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/task1/src/main.rs), [syscall.rs](/root/os_experiments/lab2/task1/src/syscall.rs), [trap.rs](/root/os_experiments/lab2/task1/src/trap.rs), [boot.S](/root/os_experiments/lab2/task1/src/boot.S)
  - Evidence artifacts in [artifacts/build_output.txt](/root/os_experiments/lab2/task1/artifacts/build_output.txt), [artifacts/run_output.txt](/root/os_experiments/lab2/task1/artifacts/run_output.txt), [artifacts/objdump_ecall.txt](/root/os_experiments/lab2/task1/artifacts/objdump_ecall.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（仅 `?? .codex`，无 task 相关脏改动）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| 通过 `ecall` 进入内核 | 用户态 syscall 封装执行 `ecall`；trap 处理 U-mode ecall | [syscall.rs](/root/os_experiments/lab2/task1/src/syscall.rs) L10-L17 使用 `asm!("ecall")`；[trap.rs](/root/os_experiments/lab2/task1/src/trap.rs) L59-L63 处理 `mcause == 8`；[objdump_ecall.txt](/root/os_experiments/lab2/task1/artifacts/objdump_ecall.txt) 含 `ecall`（`rg` 命中行 136） | pass |
| 打印的 task id/name 与运行状态一致 | 内核声明当前任务，用户态读取并打印相同字段 | [main.rs](/root/os_experiments/lab2/task1/src/main.rs) L86-L91, L141-L148；[run_output.txt](/root/os_experiments/lab2/task1/artifacts/run_output.txt) L2, L4-L5 一致显示 `id=1, name=lab2_task1_user` | pass |
| 非法参数（空指针）不崩溃并可解释 | 内核返回错误码；用户态打印错误说明 | [main.rs](/root/os_experiments/lab2/task1/src/main.rs) L157-L167 发起空指针测试；L234-L245 对空指针返回 `EFAULT`；[syscall.rs](/root/os_experiments/lab2/task1/src/syscall.rs) L47-L53 解释错误；[run_output.txt](/root/os_experiments/lab2/task1/artifacts/run_output.txt) L6-L7 显示 `-14` 与可解释信息 | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- README 中的构建耗时摘录与 artifact 不一致，存在证据口径偏差。  
  [README.md](/root/os_experiments/lab2/task1/README.md) L101-L105 写为 `0.00s`，但 [build_output.txt](/root/os_experiments/lab2/task1/artifacts/build_output.txt) L2 为 `0.11s`。建议改为与 artifact 一致，或明确“示例输出、时间会波动”。
- 运行时证据集可复现性仍偏弱。  
  当前仅有单次运行日志 [run_output.txt](/root/os_experiments/lab2/task1/artifacts/run_output.txt)，缺少 `run_output_repeat.txt` 与 `tool_versions.txt`。对 QEMU guest 任务，补齐这两项可降低评审对“单次偶然结果”的质疑。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设本任务“非法参数测试”只要求覆盖空指针这一类典型无效指针；当前交付已覆盖该场景。
- 假设审查基准以 README 中复制的任务要求为准，未收到额外课程平台隐含检查项。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 证据链主要依赖单次运行日志；若评审更强调稳定复现，可能要求重复运行证明。
  - 文档中的时间字段不一致虽不影响功能正确性，但会削弱“README 即实验记录”的严谨性。
