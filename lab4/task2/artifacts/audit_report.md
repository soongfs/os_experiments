# LAB4 用户态 Task2 审计报告

## 1. Task scope and reviewed inputs

- Target task: `lab4/task2`（LAB4 用户态 task2）
- Review mode: examiner-style audit against stated requirements and acceptance checks
- Reviewed inputs:
  - `lab4/task2/README.md`
  - `lab4/task2/lazy_swap_trigger.c`
  - `lab4/task2/run_cgroup_experiment.sh`
  - `lab4/task2/Makefile`
  - `lab4/task2/artifacts/build_output.txt`
  - `lab4/task2/artifacts/run_output.txt`
  - `lab4/task2/artifacts/run_output_repeat.txt`
  - `lab4/task2/artifacts/tool_versions.txt`
  - repository `.gitignore`

## 2. Acceptance-to-evidence matrix

| Requirement / Acceptance | Expected evidence | Observed evidence | Result |
|---|---|---|---|
| 编写内存消耗程序，逐步触发缺页并扩大工作集 | source + 分阶段快照日志 | `lazy_swap_trigger.c` 逐页触达并按阶段打印 `[snapshot]`；`run_output*.txt` 中 `grow#`/`revisit#` 连续增长 | pass |
| 若实现 swap，触发 swap in/out 并输出可观察证据 | `VmSwap`、`pswpin/pswpout`、fault 计数 | `run_output*.txt` 中 `medium/high` 出现显著 `VmSwap` 增长与 `pswpin/pswpout` delta > 0 | pass |
| 给出不同压力下行为差异 | low/medium/high 对照数据 | `run_output*.txt` 中 `low` 为 `pswpout=0`，`medium/high` 为大规模 swap 与更高 fault | pass |
| 验收1：可平稳申请超过物理上限的虚拟内存总量 | 在受限内存上限下完成更大 working set | `high` 档在 `memory.max=192M` 下完成 `working_set=640MiB` 并 `[done]` | pass |
| 验收2：日志可明显观察大量 Page Fault 与换出动作 | 高压档 fault 与 swap 指标显著 | `high` 档示例：`minflt_total=1309881`、`pswpout≈607k`、`VmSwap≈462MiB`（两次运行均复现） | pass |
| runtime-sensitive 任务的重复性证据 | repeat run 结果 | `run_output_repeat.txt` 存在，关键趋势与数量级一致 | pass |
| 结构与作用域控制 | task 自包含、README/artifacts 完整、忽略构建产物 | `lab4/task2` 结构完整；`.gitignore` 含 `lab4/task2/lazy_swap_trigger`；git 跟踪中无该二进制 | pass |

## 3. Findings ordered by severity

### blocking

- none

### recommended

- none

### nice_to_have

- 可在 `artifacts/` 额外保存一次 `memory.current`/`memory.swap.current` 的关键行提取摘要，便于评审快速定位核心对照结果（当前完整日志已足够验收）。

## 4. Open questions or assumptions

- 假设课程允许将 cgroup `memory.max=192MiB` 作为“实验物理内存上限”来定义“超出物理内存上限”的验证口径。
- 假设评审环境具备 cgroup v2 memory controller 写权限与可用 swap；否则只能验证 lazy paging，不能复现 swap in/out 证据。

## 5. Readiness verdict and residual risks

- Verdict: `ready`
- Residual risks:
  - 结果依赖宿主机 cgroup/swap 配置与权限；在无 swap 或受限权限环境中，swap 指标不可复现；
  - 在当前证据范围内，代码实现、运行日志和 README 机制解释三者一致，未见影响通过的阻断项。
