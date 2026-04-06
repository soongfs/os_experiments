# LAB2 User Task3 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/task3`（QEMU guest 用户态任务：裸机调用栈打印）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/task3/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/task3/src/main.rs), [trap.rs](/root/os_experiments/lab2/task3/src/trap.rs), [syscall.rs](/root/os_experiments/lab2/task3/src/syscall.rs), [boot.S](/root/os_experiments/lab2/task3/src/boot.S)
  - Build config in [.cargo/config.toml](/root/os_experiments/lab2/task3/.cargo/config.toml)
  - Evidence artifacts in [build_output.txt](/root/os_experiments/lab2/task3/artifacts/build_output.txt), [run_output.txt](/root/os_experiments/lab2/task3/artifacts/run_output.txt), [symbols.txt](/root/os_experiments/lab2/task3/artifacts/symbols.txt), [frame_pointer_objdump.txt](/root/os_experiments/lab2/task3/artifacts/frame_pointer_objdump.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（当前仅有 `?? .codex` 与本次审计文件）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| 在裸机环境实现基于帧指针/底层指针操作的栈回溯 | 源码中有寄存器读取与裸指针读取帧记录 | [main.rs](/root/os_experiments/lab2/task3/src/main.rs) L107-L109 使用 `asm!("mv {}, s0")` 读取 `fp`；L165-L167 使用 `ptr::read` 从 `fp-16` 读取 `FrameRecord` | pass |
| QEMU 输出至少 3 层连续返回地址 | 运行日志出现连续 `frame#..` 行且 `ra` 可见 | [run_output.txt](/root/os_experiments/lab2/task3/artifacts/run_output.txt) L5-L10 打印 5 层帧记录与结束统计 | pass |
| 报告解释“保留帧指针依赖”和失效场景 | README 中说明 omit-frame-pointer、内联、尾调用等影响 | README 第 7 节给出原理与失效场景；`.cargo/config.toml` 启用 `force-frame-pointers=yes`（[.cargo/config.toml](/root/os_experiments/lab2/task3/.cargo/config.toml) L5-L10） | pass |
| 提供底层证据支撑（符号/反汇编） | 符号表与反汇编显示函数序言和调用点 | [symbols.txt](/root/os_experiments/lab2/task3/artifacts/symbols.txt) 与 [frame_pointer_objdump.txt](/root/os_experiments/lab2/task3/artifacts/frame_pointer_objdump.txt) 包含关键函数地址、`ra/s0` 入栈与 `s0` 建帧序言 | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- 证据存在“地址口径漂移”风险，建议同一构建批次重新生成并同步更新 `run_output/symbols/objdump/README`。  
  本次独立复验（2026-04-06）中 `frame#01/#03/#04` 的 `ra` 分别为 `0x80001604/0x800016bc/0x8000168e`，而当前 [run_output.txt](/root/os_experiments/lab2/task3/artifacts/run_output.txt) 为 `0x80001600/0x800016b8/0x8000168a`。这不影响“至少 3 层回溯”结论，但会削弱 README 第 6.3 节按地址逐条映射的严谨性。
- 缺少 `run_output_repeat.txt` 与 `tool_versions.txt`，复现实验证据仍可加强。  
  对调用栈/低层机制类任务，建议至少补一份重复运行输出与工具版本快照，避免评审时对“单次日志偶然性”和环境漂移提出质疑。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设验收以 README 中列出的任务要求为准，未引入课程平台额外隐藏检查项。
- 假设该任务目标是“证明 fp 链回溯机制可工作”，而非要求地址与固定构建产物永久逐字节一致。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 地址级论证依赖于构建产物一致性；若后续工具链/编译细节变化但 README 不同步，容易出现“机制正确但证据文字失配”的评审争议。

