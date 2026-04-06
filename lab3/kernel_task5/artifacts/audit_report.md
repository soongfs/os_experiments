# LAB3 内核态 task5 审计报告

## 1. Task Scope And Reviewed Inputs

- 审计目标：`lab3/kernel_task5`（LAB3 内核态 Task5：内核任务与抢占式切换）
- 审阅输入：
  - `lab3/kernel_task5/README.md`
  - `lab3/kernel_task5/src/main.rs`
  - `lab3/kernel_task5/src/trap.rs`
  - `lab3/kernel_task5/src/boot.S`
  - `lab3/kernel_task5/artifacts/build_output.txt`
  - `lab3/kernel_task5/artifacts/run_output.txt`
  - `lab3/kernel_task5/artifacts/run_output_repeat.txt`
  - `lab3/kernel_task5/artifacts/kthread_switch_objdump.txt`
  - `lab3/kernel_task5/artifacts/tool_versions.txt`
  - 仓库级 `.gitignore`

## 2. Acceptance-To-Evidence Matrix

| 验收项 | 预期证据 | 实际证据 | 结论 |
|---|---|---|---|
| 存在至少一类内核任务 | S-only 任务定义、启动日志、进度计数 | 两个内核任务 `recycler_daemon/logger_daemon`；`[kthread] start ...` 日志；`progress>0` | 满足 |
| 支持抢占式切换 | 时钟中断驱动的 `from -> to` 切换日志与计数 | `handle_supervisor_trap()` 基于 `SSIP` 进行轮转；日志 `reason=timer_preempt from=A -> to=B`；`preempt_switches=18` | 满足 |
| 证明任务在运行且可被抢占 | 两任务都被调度并有 preemption/switch_ins 统计 | summary 中两任务 `switch_ins`、`preemptions` 均非零且 started=yes | 满足 |
| 验收1：独立 S 态 task、无 U 栈、无用户地址空间关联 | 明确 `mode/address_space/u_stack` 信息和 S-mode 切换路径 | 运行日志打印 `mode=S-only address_space=kernel-only u_stack=none`；`enter_kernel_task` 通过 `sret` 入 S 态 | 满足 |
| 验收2：时钟中断可挂起并调度到其他任务 | timer forward + supervisor trap + 切换链路完整 | `MTIP -> SSIP` forward 路径；`machine_timer_forwards>0`、`supervisor_timer_irqs>0`、多条 `timer_preempt` 切换日志 | 满足 |
| 低层实现可核查 | 关键 trap/csr 指令与函数路径可见 | `kthread_switch_objdump.txt` 含 `enter_kernel_task`、`machine_trap_entry`、`supervisor_trap_entry`、`handle_*_trap`、`csrs mie/sie/mip`、`csrc sip`、`mret/sret` | 满足 |

## 3. Findings Ordered By Severity

### blocking

- none

### recommended

- README 的“5.1 构建结果”与当前 [build_output.txt](/root/os_experiments/lab3/kernel_task5/artifacts/build_output.txt) 不一致：README 摘录含 `Compiling ...` 且耗时 `0.09s`，artifact 当前为 `Finished ... in 0.01s`。建议同步 README 为当前实际证据，避免评审时出现文本漂移。

- 本次审计基于现有 artifacts 与源码静态核对，未在审计流程内重跑构建/QEMU。用于最终签署前，建议再执行一次并刷新 `run_output_repeat.txt` 与 README 摘录。

### nice_to_have

- README 可补一句：`TARGET_SWITCHES=18` 是为实验可终止性设置，不影响“抢占式切换已发生”的验收结论。

## 4. Open Questions Or Assumptions

- 假设 README 引用的原始任务说明与教师题面一致；本次未额外校验外部题面。
- 假设 artifacts 来自当前实现版本；日志与源码行为一致。

## 5. Readiness Verdict And Residual Risks

- 结论：**ready with caveats**
- 残余风险：
  - 当前模型聚焦单核 S-mode 内核任务队列；若扩展到混合 user/kernel 任务或 SMP，需要新增调度隔离与并发一致性证据。
  - 使用 `MTIP -> SSIP` forwarder 的最小实现在迁移到 SBI/STIP/Sstc 路径时需复测中断源和 pending 清理流程。
