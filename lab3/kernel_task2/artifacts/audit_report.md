# LAB3 内核态 task2 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/kernel_task2`（LAB3 内核态 Task2：用户态/内核态完成时间统计）
- 审阅输入：
  - `lab3/kernel_task2/README.md`
  - `lab3/kernel_task2/src/main.rs`
  - `lab3/kernel_task2/src/trap.rs`
  - `lab3/kernel_task2/src/boot.S`
  - `lab3/kernel_task2/artifacts/build_output.txt`
  - `lab3/kernel_task2/artifacts/run_output.txt`
  - `lab3/kernel_task2/artifacts/run_output_repeat.txt`
  - `lab3/kernel_task2/artifacts/accounting_objdump.txt`
  - `lab3/kernel_task2/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| PCB 维护 `utime/stime` 计数 | 结构体字段与最终输出中存在 `utime/stime` | `ProcessControlBlock` 包含 `utime/stime`；运行日志打印每个 PCB 的 `utime/stime` 百分比 | 满足 |
| 给出统计口径（开始/停止边界） | README 与内核输出明确边界定义 | README 与运行日志均写明 `utime=[last_timestamp,trap_enter)`、`stime=[last_timestamp,trap_exit/task_complete)` | 满足 |
| Trap 进入与退出做时间戳更新和累加 | trap 入口调用顺序+对应 Rust 计账函数 | `boot.S` 的 `trap_entry` 依次调用 `rust_account_trap_enter -> rust_handle_trap -> rust_account_trap_exit`；`main.rs` 中分别累加 `utime/stime` | 满足 |
| 计算密集 vs syscall 密集体现占比差异 | 两类任务在日志中比例显著不同 | 两次运行 `compute_user_heavy` 约 `utime 98.7%`；`syscall_kernel_heavy` 约 `stime 61%-62%` | 满足 |
| 记账事件平衡与收口完整 | enter/exit/complete 计数与 syscalls 关系自洽 | 日志显示 `trap_enter_updates=60002`、`trap_exit_updates=60001`、`task_complete_updates=1`，与总 syscall 数平衡；`gap=0 ticks` | 满足 |
| 低层路径可核查 | `ecall`、trap、account hooks 的反汇编证据 | `accounting_objdump.txt` 中包含 `ecall`、`trap_entry`、`rust_account_trap_enter/exit`、`account_trap_enter/exit` | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- 本次审计基于现有 artifacts 与源码静态核对，未在审计流程中重跑构建与 QEMU。若用于最终提交前签署，建议再执行一次并刷新 `run_output_repeat.txt`。

### nice_to_have

- README 可补充一句“`gap=0` 的判定是基于当前实验口径（不含 idle 区段）”，让复核者更快理解该等式的边界条件。

## 4. Open Questions Or Assumptions

- 假设 README 中原始任务说明与教师题面一致；本次未额外校验外部题面源。
- 假设 artifacts 来自当前代码版本；日志与源码实现一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 该实现主要验证 `ecall` 驱动的 `U->M->U` 记账边界；若后续要求加入定时器中断或阻塞唤醒路径，需要补充对应记账证据。
  - 绝对时间值受 QEMU TCG 与宿主调度影响，跨环境数值可比性有限，但方向性结论在两次运行中稳定。
