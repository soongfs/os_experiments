# LAB4 用户态 Task3 审计报告

## 1. Task scope and reviewed inputs

- Target task: `lab4/task3`（LAB4 用户态 task3）
- Review mode: examiner-style audit against stated requirements and acceptance checks
- Reviewed inputs:
  - `lab4/task3/README.md`
  - `lab4/task3/cow_fork_demo.c`
  - `lab4/task3/Makefile`
  - `lab4/task3/artifacts/build_output.txt`
  - `lab4/task3/artifacts/run_output.txt`
  - `lab4/task3/artifacts/run_output_repeat.txt`
  - `lab4/task3/artifacts/tool_versions.txt`
  - repository `.gitignore`

## 2. Acceptance-to-evidence matrix

| Requirement / Acceptance | Expected evidence | Observed evidence | Result |
|---|---|---|---|
| `fork` 后父子共享同段内存并初始读取相同 | post-fork 值一致 + 共享页框证据 | `run_output*.txt` 中 `[post-fork/page0]`、`[post-fork/page1]` 显示父子值与 PFN 相同，`kpagecount=2` | pass |
| 父子分别写入后互不影响 | 双向隔离日志（父视角与子视角） | 子写 page0 后父仍为 seed；父写 page1 后子仍为 seed；`[final]` 与 `[acceptance]` 全部 PASS | pass |
| 写入才复制（COW 语义） | fork 前后页共享计数变化 + 写后 PFN 分裂 | `pre-fork` `kpagecount=1` -> `post-fork` `kpagecount=2`；首次写后写入方 PFN 变化且双方 `kpagecount` 回到 1 | pass |
| 首次写入触发特定 page fault | 写入临界区 minflt/majflt 增量 | 子写 page0：`child_minflt_delta=2`, `majflt=0`；父写 page1：`parent_minflt_delta=1`, `majflt=0`；与 PFN 分裂同时出现 | pass |
| runtime-sensitive 重复性 | repeat run 一致性 | `run_output_repeat.txt` 与首次运行同型：共享 -> 分裂 -> 隔离 -> PASS | pass |
| 结构与作用域控制 | task 自包含、artifacts 完整、忽略构建产物 | `lab4/task3` 目录完整；`.gitignore` 包含 `lab4/task3/cow_fork_demo`；二进制未被跟踪 | pass |

## 3. Findings ordered by severity

### blocking

- none

### recommended

- none

### nice_to_have

- 可在 `artifacts/` 增加一份简化对照表（pre/post-fork 与两次写后 PFN/kpagecount），便于评审快速对照 COW 链路；当前完整日志已足够验收。

## 4. Open questions or assumptions

- 假设课程接受使用 `pagemap + kpagecount` 作为“写入才复制”的内核级证明手段。
- 假设评审环境具备读取相关 `/proc` 接口的权限；若权限不足，该层证据需退化为“值隔离 + fault 计数”。

## 5. Readiness verdict and residual risks

- Verdict: `ready`
- Residual risks:
  - 结果对 `/proc/<pid>/pagemap` 与 `/proc/kpagecount` 访问权限敏感；
  - 在当前证据范围内，代码实现、日志与 README 机制解释一致，未见影响通过的阻断项。
