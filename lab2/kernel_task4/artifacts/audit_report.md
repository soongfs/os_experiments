# LAB2 Kernel Task4 Audit Report

- Review run date: 2026-04-06
- Reviewer mode: `os-lab-task-review` (examiner-style audit)

## 1. Task Scope And Reviewed Inputs

- Scope: `lab2/kernel_task4`（内核态任务：异常信息统计与现场输出）
- Reviewed inputs:
  - Task statement and acceptance checks in [README.md](/root/os_experiments/lab2/kernel_task4/README.md)
  - Implementation in [main.rs](/root/os_experiments/lab2/kernel_task4/src/main.rs), [trap.rs](/root/os_experiments/lab2/kernel_task4/src/trap.rs), [boot.S](/root/os_experiments/lab2/kernel_task4/src/boot.S), [syscall.rs](/root/os_experiments/lab2/kernel_task4/src/syscall.rs), and `src/apps/*.rs`
  - Evidence artifacts in [build_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/build_output.txt), [run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt), [run_output_repeat.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output_repeat.txt)
  - Ignore policy in [.gitignore](/root/os_experiments/.gitignore)
  - Submission scope via `git status --short`（当前仅有 `?? .codex` 与本次审计文件）

## 2. Acceptance-to-Evidence Matrix

| Acceptance item | Expected evidence | Observed evidence | Coverage |
| --- | --- | --- | --- |
| 异常时输出结构化现场信息（类型、PC/地址、关键寄存器），且避免二次崩溃 | 日志含 `scause/sepc/stval`、异常类型、指令与寄存器；读取 fault 指令时有边界防护 | [main.rs](/root/os_experiments/lab2/kernel_task4/src/main.rs) L270-L299 结构化打印；L475-L490 读取指令前做范围/对齐检查；日志见 [run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt) L10-L25 | pass |
| 提供 `Illegal Instruction` 与 `Store/AMO Page Fault` 的现场日志 | 两类 fault 均出现并有稳定关键值 | [run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt) L11-L15（Illegal）与 L21-L25（Store Page Fault）；[run_output_repeat.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output_repeat.txt) 对应值一致 | pass |
| 日志包含明确 `scause/sepc/stval` | 两类异常都带这三项 CSR 值 | Illegal: `scause=0x2 sepc=0x40000b48 stval=0x10001073`；StorePF: `scause=0xf sepc=0x40000bda stval=0x0`（[run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt) L11-L12, L21-L22） | pass |
| 故障进程被安全终止并继续运行后续任务 | fault 后进入下一 app，最终 summary 与检查均 PASS | [main.rs](/root/os_experiments/lab2/kernel_task4/src/main.rs) L301-L343 在 fault 后 `advance_to_next_app()`；日志中 fault 后健康任务继续运行（[run_output.txt](/root/os_experiments/lab2/kernel_task4/artifacts/run_output.txt) L16-L28）且最终检查全 PASS（L36-L41） | pass |
| 异常现场来自真实 S-mode trap 上下文（`scause/sepc/stval`） | trap 入口与 CSR 读取路径明确，U-trap 已委托给 S-mode | [boot.S](/root/os_experiments/lab2/kernel_task4/src/boot.S) L31-L45 U-mode 通过 `sret` 进入；L49-L90 保存 trap frame；[trap.rs](/root/os_experiments/lab2/kernel_task4/src/trap.rs) L57-L64 读取 `scause/stval`；[main.rs](/root/os_experiments/lab2/kernel_task4/src/main.rs) L587-L597 配置 `medeleg` | pass |

## 3. Findings (Ordered By Severity)

### blocking

- none

### recommended

- 缺少 `tool_versions.txt` 任务级环境快照。  
  当前已有双次运行日志，建议补充 `rustc/cargo/qemu` 版本落盘，以便评审复现实验时快速核对环境。
- `build_output.txt` 的耗时与当前复验口径不一致。  
  当前 artifact 记录 `0.00s`，本次复验为 `0.02s`。建议同步更新 artifact 或在 README 明确“构建耗时为示例值，会随环境波动”。

### nice_to_have

- none

## 4. Open Questions Or Assumptions

- 假设验收以 README 中引用的要求为准，不额外要求图形截图文件；当前终端日志已可直接用于截图留档。
- 假设“安全杀掉”定义为 faulting app 不再返回 U-mode 且后续任务继续运行（当前实现满足）。

## 5. Readiness Verdict And Residual Risks

- Verdict: **ready with caveats**
- Residual risks:
  - 该实验是教学化最小 S-mode 模型，尚未覆盖完整进程回收与内存管理；但对本任务关注的异常现场输出与恢复路径，证据链完整。

