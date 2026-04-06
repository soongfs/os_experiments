# LAB3 用户态 task2 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/task2`（LAB3 用户态 Task2：任务切换开销估算）
- 审阅输入：
  - `lab3/task2/README.md`
  - `lab3/task2/src/main.rs`
  - `lab3/task2/src/trap.rs`
  - `lab3/task2/src/boot.S`
  - `lab3/task2/artifacts/build_output.txt`
  - `lab3/task2/artifacts/run_output.txt`
  - `lab3/task2/artifacts/run_output_repeat.txt`
  - `lab3/task2/artifacts/context_switch_objdump.txt`
  - `lab3/task2/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| 高频触发切换程序 | 源码有高频 syscall/yield 路径，运行日志显示大量操作与切换 | `src/main.rs` 中 `OPS_PER_TASK=25000`，`MODE_YIELD` 下双任务循环 `sys_yield()`；`run_output*.txt` 显示 `hot_ops=50000`、`switches=50000` | 满足 |
| 统计总耗时并估算单次切换开销 | 明确公式、每轮数据、最终估算值 | `finish_experiment()` 输出每轮 `baseline/yield/extra/switch_estimate`，并输出 median/mean/min/max；日志存在微秒级结果 | 满足 |
| 说明误差来源 | README 有技术合理误差分析 | `README.md` 覆盖计时粒度、warm-up/缓存/TCG、宿主调度抖动、差分模型系统偏差 | 满足 |
| 验收1：给出明确单次切换耗时 | 日志有明确数值（ns/us） | 两次运行分别给出 median `1129 ns` 与 `1163 ns`，并在 README 汇总约 `1.15 us` | 满足 |
| 验收2：误差分析具技术合理性 | 误差项与测量边界解释清晰且自洽 | README 第 7 节与实现的计时方法（`mtime`、差分）一致 | 满足 |
| 切换路径确实发生在 trap/syscall 边界 | `ecall`、trap、yield handler 的低层证据 | `context_switch_objdump.txt` 含 `ecall`、`trap_entry`、`handle_yield` 相关符号与调用点 | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- 本次审计基于现有 artifacts 与源码静态核对，未在审计环节重新执行 `cargo build`/QEMU。用于最终提交前签署时，建议再跑一次并刷新 `run_output_repeat.txt` 以确保证据与当前 HEAD 同步。

### nice_to_have

- 可在 README 中补一行“最终提交采用哪一次 median 作为主报告值（或两次取中值策略）”，减少复核时对“1.129 us vs 1.163 us”取值口径的歧义。

## 4. Open Questions Or Assumptions

- 假设 README 引用的“原始任务说明”与教师题面一致；本次未额外校验外部题面源。
- 假设 artifacts 为当前实现版本生成；日志内容与源码行为一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 绝对时间值受 QEMU TCG 与宿主调度影响，跨机器可比性有限；当前结果更适合作为“当前环境下的方法验证与量级估算”。
  - 若后续修改 trap/frame 保存策略（尤其 FPU 路径），应重新采样并更新估算结论。
