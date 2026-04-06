# LAB3 用户态 task3 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/task3`（LAB3 用户态 Task3：用户态/内核态时间统计验证）
- 审阅输入：
  - `lab3/task3/README.md`
  - `lab3/task3/src/main.rs`
  - `lab3/task3/src/trap.rs`
  - `lab3/task3/src/boot.S`
  - `lab3/task3/artifacts/build_output.txt`
  - `lab3/task3/artifacts/run_output.txt`
  - `lab3/task3/artifacts/run_output_repeat.txt`
  - `lab3/task3/artifacts/accounting_objdump.txt`
  - `lab3/task3/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| 计算密集型用户任务 | 源码中存在长用户态计算路径，少量 syscall 进入内核 | `compute_task_entry()` 执行 `COMPUTE_ITERATIONS=4_000_000`，仅末尾 `finish`；日志 `syscalls=1` | 满足 |
| 系统调用密集型任务 | 源码中存在高频 syscall 路径，内核实际执行处理逻辑 | `syscall_task_entry()` 调用 `probe` 60000 次；`sys_probe()` 有内核计算；日志 `syscalls=60001` | 满足 |
| user/kernel 时间区分统计机制 | 在 trap 入口与返回前存在明确记账逻辑与状态约束 | `dispatch_trap()->account_user_entry()` 与 `return_to_current_task()/finish_current_task()->account_kernel_slice()`；`CPU_MODE` 状态检查 | 满足 |
| 验收1：计算型 user 时间远大于 kernel 时间 | 终端输出有比例或绝对值可直接比较 | 两次运行 `compute_background`：`user≈99.8%`，`kernel≈0.1%` | 满足 |
| 验收2：syscall 型 kernel 占比明显上升 | 同一输出中两任务占比对比明显 | 两次运行 `syscall_probe`：`kernel≈60.70%`，明显高于计算型 | 满足 |
| 验收3：提供相关统计终端输出 | `run_output` 与复验输出存在完整统计行 | `run_output.txt`、`run_output_repeat.txt` 均包含 `stats[...]` 与 `acceptance ... PASS` | 满足 |
| 低层路径可核查 | `ecall`、trap、syscall handler 反汇编证据 | `accounting_objdump.txt` 含 `ecall`、`trap_entry`、`sys_probe`、相关调用链 | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- 本次审计基于现有 artifacts 与源码静态核对，未在审计流程中重跑构建与 QEMU。若作为最终提交前签署，建议再执行一次并刷新 `run_output_repeat.txt` 以确保与当前 HEAD 完全同步。

### nice_to_have

- 可在 README 中补充一行“验收阈值的显式定义”（例如 task1 用 `user > 10*kernel`、task2 用 `kernel_ratio` 提升至少 30 个百分点），让复核者不必反查源码即可理解 PASS 判据。

## 4. Open Questions Or Assumptions

- 假设 README 的“原始任务说明”与教师题面一致；本次未额外校对外部题面源。
- 假设 artifacts 为当前实现生成；日志与源码行为一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 绝对时间值受 QEMU TCG 与宿主调度影响，跨环境数值可比性有限；但占比方向性与差异幅度在两次运行中稳定。
  - 若后续修改 trap 路径或记账边界（`account_user_entry/account_kernel_slice`），需重新生成统计证据。
