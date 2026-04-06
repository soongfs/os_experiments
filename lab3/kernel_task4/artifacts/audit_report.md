# LAB3 内核态 task4 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/kernel_task4`（LAB3 内核态 Task4：内核态中断响应）
- 审阅输入：
  - `lab3/kernel_task4/README.md`
  - `lab3/kernel_task4/src/main.rs`
  - `lab3/kernel_task4/src/trap.rs`
  - `lab3/kernel_task4/src/boot.S`
  - `lab3/kernel_task4/artifacts/build_output.txt`
  - `lab3/kernel_task4/artifacts/run_output.txt`
  - `lab3/kernel_task4/artifacts/run_output_repeat.txt`
  - `lab3/kernel_task4/artifacts/interrupt_objdump.txt`
  - `lab3/kernel_task4/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| 支持内核态中断响应 | S-mode 内核收到并处理时钟相关中断，输出诊断路径 | `run_output*.txt` 含 `irq#N source=forwarded_timer(origin=mtime, delivered=ssip)` 与 handler 路径 | 满足 |
| 明确中断屏蔽/开中断策略 | safe interval/critical section 分别开关 `sstatus.SIE`，并有日志与代码对应 | `run_safe_interval()` 开 `SIE`，`InterruptGuard` 在临界区关 `SIE`；日志包含 `safe interval ... enabling sstatus.SIE` 与 `critical section ... sstatus.SIE=0` | 满足 |
| 输出必要诊断信息 | 中断号/来源/处理路径/关键 CSR 上下文可见 | 日志包含 `irq#`、`origin/delivered`、`scause/sepc/path`；summary 含 `last_mcause/last_scause/last_sepc` | 满足 |
| 验收1：`sstatus.SIE` 在安全区打开 | 有开中断证据且 acceptance PASS | 代码 `csrs sstatus`；反汇编 `enable_supervisor_interrupts`；日志与 acceptance 行均为 PASS | 满足 |
| 验收2：内核空间时钟中断不崩溃 | 多次中断后系统完成实验并输出汇总 | 两次运行均输出 summary + 三项 PASS，`supervisor_timer_irqs>0` | 满足 |
| 验收3：锁/临界区使用关中断保护 | 加锁路径绑定关中断，临界区观测到 pending 延后 | `InterruptMutex::lock()` 内创建 `InterruptGuard`；临界区日志 `sstatus.SIE=0` 且 `pending_forwarded_timer_irq=1` | 满足 |
| 低层路径可核查 | machine/supervisor trap entry、mip/sip/sstatus/sie/mie 操作可见 | `interrupt_objdump.txt` 含 `machine_trap_entry`、`supervisor_trap_entry`、`csrs/csrc sstatus`、`csrs mip`、`csrc sip`、`csrs mie/sie` | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- 本次审计基于现有 artifacts 与源码静态核对，未在审计流程中重跑构建与 QEMU。若用于最终提交前签署，建议再执行一次并刷新 `run_output_repeat.txt`。

### nice_to_have

- README 可补一行解释：本实验采用 `MTIP -> SSIP` forwarder 验证“内核态响应与临界区保护”，并非直接 `STIP` 路径，以减少评审对中断类型命名的误解。

## 4. Open Questions Or Assumptions

- 假设 README 的原始任务说明与教师题面一致；本次未额外校验外部题面。
- 假设 artifacts 来自当前提交版本；日志与源码行为一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 该最小模型重点验证单核 S-mode 内核中的“可中断安全区 + 关中断临界区”语义；若后续扩展到 SMP 或更复杂锁层次，需补充并发场景证据。
  - 转发路径依赖 `mideleg + SSIP` 设计，迁移到 SBI/STIP 或 Sstc 实现时需重新验证中断来源和清 pending 流程。
