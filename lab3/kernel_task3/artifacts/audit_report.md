# LAB3 内核态 task3 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/kernel_task3`（LAB3 内核态 Task3：浮点上下文切换与抢占支持）
- 审阅输入：
  - `lab3/kernel_task3/README.md`
  - `lab3/kernel_task3/src/main.rs`
  - `lab3/kernel_task3/src/trap.rs`
  - `lab3/kernel_task3/src/boot.S`
  - `lab3/kernel_task3/artifacts/build_output.txt`
  - `lab3/kernel_task3/artifacts/run_output.txt`
  - `lab3/kernel_task3/artifacts/run_output_repeat.txt`
  - `lab3/kernel_task3/artifacts/trap_fp_context_objdump.txt`
  - `lab3/kernel_task3/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| Trap/进程上下文包含 `f0-f31` 与 `fcsr` | 结构体与汇编偏移显示浮点上下文空间 | `TrapFrame` 包含 `f: [u64;32] + fcsr`；`boot.S` 定义 `TF_F0..TF_F31` 与 `TF_FCSR`，`TRAP_FRAME_SIZE=528` | 满足 |
| 保存/恢复逻辑正确且不遗漏状态寄存器 | trap 入口/返回与 enter_task 中存在完整 `fsd/fld + frcsr/fscsr` 路径 | `boot.S` 和 `trap_fp_context_objdump.txt` 可见保存 `f0-f31`、`frcsr`，返回前 `fscsr` + 恢复 `f0-f31` | 满足 |
| 用户态浮点验证程序证明正确性 | 参考值 + 抢占并发值逐位一致，多次运行 PASS | 两次运行 `fp_alpha/fp_beta` 在并发阶段均与参考校验值逐位一致，`timer_interrupts>0` 且验收行均 PASS | 满足 |
| 说明何时保存浮点上下文（eager/lazy） | README 与运行日志给出策略和边界 | README 明确采用 eager；运行日志打印 `fp context strategy: eager save/restore on every trap entry/exit` | 满足 |
| 抢占切换真实发生 | 定时器中断/强制切换计数大于 0 | `run_output.txt` 为 `timer_interrupts=86 forced_switches=86`；复验为 `90/90` | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- `README.md` 的“5.1 构建结果”示例与当前 [build_output.txt](/root/os_experiments/lab3/kernel_task3/artifacts/build_output.txt) 不完全一致：artifact 实际包含 `Blocking waiting for file lock on artifact directory` 且构建耗时为 `0.19s`，README 仍写 `0.00s`。建议同步为当前真实产物，避免评审时出现“证据文本不一致”疑问。

- 本次审计基于现有 artifacts 与源码静态核对，未在审计流程内重跑构建与 QEMU。若用于最终提交前签署，建议再执行一次并刷新 `run_output_repeat.txt` 与 README 摘录。

### nice_to_have

- 可在 README 增补一句：当前任务验证的是 eager 策略正确性，不比较 eager vs lazy 的性能开销，以明确实验范围。

## 4. Open Questions Or Assumptions

- 假设 README 中原始任务说明与教师题面一致；本次未额外校验外部题面源。
- 假设 artifacts 来自当前实现版本；日志与源码路径一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 当前证据链已证明正确性，但若后续切换到 lazy FPU 管理，需要新增首次使用路径和状态位管理的专门验证。
  - 并发 UART 输出存在交错，自动解析日志时应以内核汇总行与 PASS 行为准。
