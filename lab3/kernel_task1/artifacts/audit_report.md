# LAB3 内核态 task1 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/kernel_task1`（LAB3 内核态 Task1：任务切换过程可视化）
- 审阅输入：
  - `lab3/kernel_task1/README.md`
  - `lab3/kernel_task1/src/main.rs`
  - `lab3/kernel_task1/src/trap.rs`
  - `lab3/kernel_task1/src/boot.S`
  - `lab3/kernel_task1/artifacts/build_output.txt`
  - `lab3/kernel_task1/artifacts/run_output.txt`
  - `lab3/kernel_task1/artifacts/run_output_repeat.txt`
  - `lab3/kernel_task1/artifacts/switch_trace_objdump.txt`
  - `lab3/kernel_task1/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| 切换路径增加必要日志且有可控开关 | 源码中日志开关/限流与切换日志输出 | `src/main.rs` 中 `ENABLE_SWITCH_TRACE`、`SWITCH_TRACE_LIMIT`；`emit_switch_trace()` 打印并限流 | 满足 |
| 展示从 task A 切到 task B 的关键点 | 日志中成对出现 A 保存完毕与 B 恢复开始 | `run_output*.txt` 中同一 `switch#NN` 的 `save_done` + `restore_begin` 成对记录 | 满足 |
| 输出可读且包含 id/name/切换原因 | 日志字段包含 task 身份与 reason | 日志含 `id`、`name`、`reason=explicit_yield/time_slice/task_exit` | 满足 |
| 验收1：日志在核心 `switch_to`/调度函数附近 | 代码与反汇编显示日志路径在 `switch_to`，调用来自调度路径 | `switch_to()` 中统一打印日志；`switch_trace_objdump.txt` 显示 `handle_explicit_yield`/`handle_timer_interrupt` 调用 `switch_to` | 满足 |
| 验收2：能看出 A 保存完毕与 B 恢复开始时机 | 具备两个语义明确的阶段标记 | `save_done`（A 已保存）与 `restore_begin`（B 将恢复）字段明确，且含 `saved_mepc/next_mepc` | 满足 |
| 运行稳定可复验 | 多次运行保持关键行为一致 | `run_output.txt` 与 `run_output_repeat.txt` 均有两类原因切换与 PASS 汇总 | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- 本次审计基于现有 artifacts 与源码静态核对，未在审计流程内重跑构建与 QEMU。若用于最终提交前签署，建议再执行一次并刷新 `run_output_repeat.txt`。

### nice_to_have

- README 可补一行“未覆盖阻塞/唤醒型切换原因仅为本任务范围裁剪”，便于评审快速区分“未实现”与“任务不要求”。

## 4. Open Questions Or Assumptions

- 假设 README 中原始任务文本与教师题面一致；本次未额外校对外部题面源。
- 假设 artifacts 为当前代码版本生成；日志字段与源码实现一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 当前可视化重点是 `explicit_yield/time_slice/task_exit`，不覆盖更复杂阻塞队列与唤醒路径；若后续任务要求扩展原因分类，需要补充对应证据。
  - 限流配置默认只保留前 10 组切换记录，深度问题排查时需手动提高上限。
