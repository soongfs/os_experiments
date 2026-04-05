# LAB5 实验与规范调整对话记录

## 对话背景

- 本记录基于当前线程中用户与 AI 的可见对话整理。
- 主题包括：
  - `LAB5` 用户态与内核态任务的阶段性交付
  - `os-lab-task-runner` skill 的改进讨论与更新
  - 仓库级 [AGENTS.md](/root/os_experiments/AGENTS.md) 的精修
- 未记录隐藏系统提示、开发者指令、工具内部参数、私有推理和未直接展示的原始终端输出。

## 摘要

1. 用户先后要求完成 `LAB5` 的多个任务，包括用户态 `task1~task3`、内核态 `task1~task4`。
2. AI 分阶段完成这些任务，给出源码、README、artifact 路径、关键实测结果和对应 commit hash。
3. 在 `LAB5 kernel_task2~4` 实施过程中，AI 明确采用了“最小可验证实现”思路，例如：
   - `kernel_task2` 使用 RR 与静态优先级的双阶段对照实验；
   - `kernel_task3` 使用多核 work queue + spinlock 的基础 SMP 模型；
   - `kernel_task4` 使用 M-mode 下的 `MTIP + MSIP` 多核中断模型和专用中断栈。
4. 用户随后询问 `os-lab-task-runner` skill 是否需要改进。AI 建议精简主流程、补充运行时任务证据模板、增加“依赖命令不得并行”和“复制骨架后的清理清单”等规则。用户同意后，AI更新了 `/root/.codex/skills/os-lab-task-runner/SKILL.md`。
5. 用户继续询问仓库级 `AGENTS.md` 是否需要改进。AI 建议做小幅精修而非扩写，并在用户同意后更新了 [AGENTS.md](/root/os_experiments/AGENTS.md)，提交了独立 commit。
6. 最后，用户要求用 `conversation-markdown-recorder` 记录当前对话并提交成果文件至 git。AI 生成本记录文件并准备单独提交。

## 关键结论

- 本线程中用户可见的 LAB5 任务交付结果包括：
  - `lab5/task1`，commit `27d3f4986621361c04741ddd792ff0231411e103`
  - `lab5/task2`，commit `54a9e8007ba1db22ddc2409146cc9f5ee426e001`
  - `lab5/task3`，commit `d31109f03c4b8944e681f368c48eea8085ec14be`
  - `lab5/kernel_task1`，commit `fa583258caf379c3cacb62d56b968c16407d1581`
  - `lab5/kernel_task2`，commit `4f999f2d2d78c4d8b2b054255a84ef668718a4e3`
  - `lab5/kernel_task3`，commit `7adab4cdb32fff7483e08f0a1091b6ea3dd996a2`
  - `lab5/kernel_task4`，commit `71876a1df330c3dbee0226a82e8db1c23078ae34`
- 仓库级规范更新：
  - [AGENTS.md](/root/os_experiments/AGENTS.md) 已补入复制骨架清单、运行时证据模板、依赖命令串行规则、最小可验证实现说明，commit `79147f52b14f6b821d44aaab766037109b565b3f`
- skill 级更新：
  - `/root/.codex/skills/os-lab-task-runner/SKILL.md` 已精简并补充 `LAB5` 运行时任务模板，但该文件位于仓库外，不属于本仓库 git 提交范围。
- 本次新增对话记录文件：
  - `lab5-task-runner-and-guidelines-session.md`

## AI 显式分析与决策

- AI 明确表示，对 `LAB2+` 的内核态题目一律按 QEMU guest 内核实验处理，而不是宿主 Linux 内核。
- 在 `kernel_task2` 中，AI 决定复用相邻任务骨架，采用“RR 基线 + 静态优先级”双阶段实验，以便在同一镜像内直接对照。
- 在 `kernel_task2` 验收修正中，AI 明确指出原先用 `switch_ins` 作为高优先级受益指标不敏感，改用“完成前 CPU 服务份额”更合理。
- 在 `kernel_task3` 中，AI 选择了“多 HART + 全局 work queue + 自旋锁 + bump allocator”的最小 SMP 模型，以直接覆盖“多核启动、共享结构加锁、系统稳定”三个验收点。
- 在 `kernel_task4` 中，AI 选择了“每 hart 本地 timer interrupt + CLINT `MSIP` IPI + 专用 interrupt stack + 锁状态自检”的设计，以覆盖“多核中断处理、一致性与可重入边界”。
- 在 skill 与 AGENTS 讨论中，AI 明确区分：
  - skill 负责执行流程和启发式；
  - AGENTS 负责仓库级稳定规则；
  - 不建议两者互相重复膨胀。

## 工具调用记录

1. 仓库与文件检查
   - 使用 `sed`、`rg`、`ls`、`find`、`git status --short` 检查相邻任务、技能说明、仓库规范和工作区状态。
   - 目的：确认任务路径、复用骨架、提取 commit hash、验证工作区清洁度。
   - 结果：定位到 `lab5/task*`、`lab5/kernel_task*`、`/root/.codex/skills/os-lab-task-runner/SKILL.md` 和 [AGENTS.md](/root/os_experiments/AGENTS.md)。

2. 代码与文档修改
   - 使用 `apply_patch` 编辑 `Cargo.toml`、`.gitignore`、`linker.ld`、`src/*.rs`、`src/boot.S`、各任务 `README.md`、skill 文件和 `AGENTS.md`。
   - 目的：实现任务逻辑、补充日志与验收输出、修正文档规范。
   - 结果：完成 `kernel_task2~4` 的实现与文档，并完成 skill/AGENTS 精修。

3. 构建与验证
   - 使用 `cargo build`、`qemu-system-riscv64 ... -smp 4`、`cargo objdump`、版本检查命令。
   - 目的：验证运行结果、保存 artifact、确认 QEMU 和 RISC-V 工具链版本。
   - 结果：
     - `kernel_task2`、`kernel_task3`、`kernel_task4` 均完成两轮 QEMU 验证；
     - 生成了 `build_output.txt`、`run_output.txt`、`run_output_repeat.txt`、`*objdump*.txt` 和 `tool_versions.txt`。

4. 调试性修正
   - 在 `kernel_task2` 中，修复了 linker script 与任务栈符号数量不匹配的问题，并调整验收指标。
   - 在 `kernel_task4` 中，修复了 trap 入口对齐问题，并补上 `mstatus.FS` 以支持中断现场中的 FPU 寄存器保存恢复。
   - 结果：相关任务从失败或卡住状态转为稳定通过两轮验收。

5. Git 操作
   - 使用 `git add`、`git commit`、`git rev-parse HEAD`。
   - 目的：为每个完成的任务与文档调整生成独立 commit。
   - 结果：形成多次任务提交，以及 `AGENTS.md` 调整提交。

## 原始对话记录

### 第1轮
**用户**

要求完成 `LAB5` 用户态 `task1`：进程管理系统调用实践，需使用 `nice/fork/exec/spawn`，并说明 `fork` 与 `exec` 的职责边界。

**AI**

回复已完成 `lab5/task1`，给出源码路径、运行日志、构建证据和 commit `27d3f4986621361c04741ddd792ff0231411e103`，并说明：

- `nice` 从 `0` 调到 `4`
- 子进程在 `fork` 后 `exec` 到新镜像且 PID 保持不变
- `README` 明确区分了 `fork` 与 `exec` 的功能边界

### 第2轮
**用户**

要求完成 `LAB5` 用户态 `task2`：逻辑短路与进程树计数，分析 `&& / ||` 与 `fork` 的组合效应，并验证理论与实际输出一致。

**AI**

回复已完成 `lab5/task2`，给出源码、README、日志和 commit `54a9e8007ba1db22ddc2409146cc9f5ee426e001`。AI说明程序支持 `analyze / verify / demo` 模式，并给出经典表达式 `fork() && fork() || fork()` 的理论值与实测值一致。

### 第3轮
**用户**

要求完成 `LAB5` 用户态 `task3`：多线程/多进程性能对比，需讨论线程和进程在调度、地址空间、TLB 刷新等方面的差异。

**AI**

回复已完成 `lab5/task3`，给出两轮实测结果、README 和 commit `d31109f03c4b8944e681f368c48eea8085ec14be`。AI汇总串行版、线程版、进程版的平均耗时、CPU 利用率和 RSS，并指出进程版聚合 RSS 更高，`sys_s` 开销也明显更大。

### 第4轮
**用户**

要求完成 `LAB5` 内核态 `task1`：进程切换过程显示，需输出切换前后 PID 与触发原因，并能区分 `yield` 与时间片耗尽。

**AI**

回复已完成 `lab5/kernel_task1`，给出运行日志、反汇编证据与 commit `fa583258caf379c3cacb62d56b968c16407d1581`。AI说明日志已能区分：

- `explicit_yield`
- `time_slice`
- `process_exit`

### 第5轮
**用户**

要求完成 `LAB5` 内核态 `task2`：实现非 RR 调度算法，并给出交互型/计算型任务在不同调度下的对照实验。

**AI**

在多次中间更新后完成 `lab5/kernel_task2`：

- 采用 RR 基线和静态优先级双阶段实验；
- 交互任务优先级更高，计算任务优先级较低；
- 修复了 linker script 中用户栈符号数量不足的问题；
- 将验收指标从 `switch_ins` 调整为“完成前 CPU 服务份额”。

最终回复给出 commit `4f999f2d2d78c4d8b2b054255a84ef668718a4e3`，并指出交互任务在静态优先级下完成时间约缩短一半、服务份额从约 `20%` 提升到约 `49%~50%`。

### 第6轮
**用户**

要求完成 `LAB5` 内核态 `task3`：多核处理器支持，需支持多核启动、基本调度和自旋锁保护的共享结构。

**AI**

在多次中间更新后完成 `lab5/kernel_task3`：

- 采用 `-smp 4`
- primary hart 负责初始化并释放 secondary hart
- 4 个 hart 通过共享 `run_queue` 调度 8 个 job
- `run_queue` 与 `allocator` 都由 `SpinLock` 保护

最终回复给出 commit `7adab4cdb32fff7483e08f0a1091b6ea3dd996a2`，并指出两轮都得到：

- `ready_harts=4`
- `booted_mask=0xf`
- `completed_jobs=8/8`
- `max_parallel_jobs=4`

### 第7轮
**用户**

要求完成 `LAB5` 内核态 `task4`：内核态中断响应与处理，需在多核背景下正确处理中断，并验证高频中断下系统仍稳定。

**AI**

在多次中间更新后完成 `lab5/kernel_task4`：

- 采用 `MTIP + MSIP` 的多核中断模型；
- trap 入口切换到每 hart 独立的 interrupt stack；
- 普通上下文拿共享锁，中断上下文只做最小原子更新，不再拿共享锁；
- 调试过程中修复了 `trap_entry` 对齐和 `mstatus.FS` 配置问题。

最终回复给出 commit `71876a1df330c3dbee0226a82e8db1c23078ae34`，并说明：

- 每个 hart 都处理了大量 timer interrupt 和 `6` 次 IPI
- `interrupts_while_lock_held` 在所有 hart 上都非零
- `lock_state_violations=0`
- 各 hart `min_sp` 都位于自己的 interrupt stack 区间内

### 第8轮
**用户**

询问结合 `LAB5` 的经验，`os-lab-task-runner` 是否需要改进。

**AI**

回应 skill 整体可用，但建议：

- 增加“依赖命令不得并行”的硬规则
- 增加复制骨架后的清理清单
- 为 `scheduling / SMP / interrupt` 类任务补证据模板
- README 显式说明“刻意未实现的部分”

### 第9轮
**用户**

同意修改 skill。

**AI**

使用 `conversation-markdown-recorder` 与 `skill-creator` 相关规范后，更新了 `/root/.codex/skills/os-lab-task-runner/SKILL.md`：

- 精简为依赖 `AGENTS.md` 的主流程
- 保留环境分类
- 增加骨架复用清单
- 增加依赖命令串行规则
- 增加运行时任务证据模板

AI还说明该 skill 位于仓库外，因此没有仓库内 git commit。

### 第10轮
**用户**

询问 [AGENTS.md](/root/os_experiments/AGENTS.md) 是否需要改进。

**AI**

回应需要“小改而非扩写”，建议：

- 补“依赖命令不得并行”
- 补“复制骨架后的清理清单”
- 补运行时敏感任务的证据偏好
- 明确允许最小可验证实现，但 README 必须说明省略项

### 第11轮
**用户**

同意修改 `AGENTS.md`。

**AI**

直接修改 [AGENTS.md](/root/os_experiments/AGENTS.md)，并提交 commit `79147f52b14f6b821d44aaab766037109b565b3f`。AI补充说明：

- 这是仓库级稳定规则更新；
- 只改 [AGENTS.md](/root/os_experiments/AGENTS.md)，不混入实验任务文件；
- 工作区剩余无关状态只有 `.codex`。

### 第12轮
**用户**

要求使用 `conversation-markdown-recorder` 记录当前对话内容，并将成果文件提交至 git。

**AI**

开始生成本 Markdown 记录，选择英文文件名 `lab5-task-runner-and-guidelines-session.md`，将内容写为简体中文，并准备单独提交到当前仓库。

## 后续动作

- 将本文件 `lab5-task-runner-and-guidelines-session.md` 提交到 git。
