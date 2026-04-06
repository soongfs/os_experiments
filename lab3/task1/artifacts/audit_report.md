# LAB3 用户态 task1 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/task1`（LAB3 用户态 Task1：浮点抢占验证程序）
- 审阅输入：
  - `lab3/task1/README.md`
  - `lab3/task1/src/main.rs`
  - `lab3/task1/src/trap.rs`
  - `lab3/task1/src/boot.S`
  - `lab3/task1/artifacts/build_output.txt`
  - `lab3/task1/artifacts/run_output.txt`
  - `lab3/task1/artifacts/run_output_repeat.txt`
  - `lab3/task1/artifacts/trap_fp_context_objdump.txt`
  - `lab3/task1/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| 编写浮点密集程序并输出校验值 | 源码中存在持续 FP 运算与最终 checksum 输出 | `src/boot.S` 中 `fp_stress_loop` 使用 `fmadd.d` 长循环；`src/main.rs` 中 `user_task_entry` 输出 reference/preemptive checksum | 满足 |
| 通过并发或交替运行触发抢占/切换 | 定时器中断与切换日志、计数 | `run_output*.txt` 含 `timer interrupt #... preempt ...`；总结行为 `timer_interrupts=82/83`、`forced_switches=82/83` | 满足 |
| 并发运行后结果完全正确 | 参考值 vs 并发值逐位比较且 PASS | `run_output*.txt` 中两任务 `expected == observed` 且 `acceptance ... PASS` | 满足 |
| 给出“保存错误会异常”的判定依据 | README 机制说明 + 代码中的对比逻辑 | `README.md` 说明以逐位相等判定；`finish_experiment()` 中严格相等比较与失败退出逻辑 | 满足 |
| 运行时确实发生中断/抢占（验收检查1） | 中断日志与非零中断计数 | `run_output*.txt` 有中断日志，且 `timer_interrupts>0` | 满足 |
| 并发结果完全正确（验收检查2） | 两个 workload 的最终校验全部 PASS | `run_output*.txt` 两个 workload 均 PASS，重复运行保持一致 | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- `lab3/task1/artifacts/run_output.txt` 与 `lab3/task1/artifacts/run_output_repeat.txt` 中存在用户打印被抢占后行内交错（例如 `preemptive run start` 行被内核日志插入），虽然不影响最终验收结论，但会降低自动解析稳定性。建议额外提供一份“仅内核验收摘要日志”（或在 README 增加明确的解析锚点行）以提升复查效率。

- 本次审计基于现有 artifacts 与源码静态核对，未在审计环节重新执行 `cargo build` / `qemu-system-riscv64`。若用于最终提交前签署，建议再执行一次构建与运行并覆盖 `run_output_repeat.txt`，确保证据与当前 HEAD 同步。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设 `README.md` 中“原始任务说明”与教师给定 Task1 文本一致；本次审计未额外对照外部题面来源。
- 假设 `artifacts/` 日志确认为当前实现版本生成；从日志与源码行为上看一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 串口输出的并发交错会影响日志可读性，但当前关键验收行（中断计数、校验比较、PASS/FAIL）完整且可判定。
  - 如后续改动 trap 或调度路径，需重新生成并归档运行证据，避免“代码与 artifacts 漂移”。
